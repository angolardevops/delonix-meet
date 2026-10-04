-- Delonix Meet — IVR de dial-in PSTN (FreeSWITCH / mod_lua)
--
-- Fluxo: atende a chamada → pede o PIN por DTMF → valida no control plane Rust
-- (/internal/v1/voice/ivr/validate, autenticado por segredo partilhado) → junta o
-- chamador à conferência da sala (nome = room_code). No fim, envia o CDR.
--
-- DOIS MODOS, o mesmo IVR (R273):
--   (sem argumento)  dial-in PSTN: a sala é a do (DID marcado, PIN).
--   `ramal`          um ramal interno marcou o número de acesso às reuniões
--                    (ramais_dial.lua chama `dialin_ivr.lua ramal`): não há
--                    DID; a sala é a do PIN DENTRO DA ORGANIZAÇÃO DO RAMAL, e
--                    quem decide isso é o control plane
--                    (/internal/v1/voice/ivr/validate-extension). O ramal
--                    identifica-se pelo que o FreeSWITCH AUTENTICOU por digest
--                    (sip_auth_username / sip_auth_realm) — nunca pelo From,
--                    que o telefone escreve como quiser. Sem CDR: não é uma
--                    chamada PSTN, e o CDR do dial-in cobra a tarifa de entrada.
--   O modo `ramal` NUNCA correu contra um FreeSWITCH real: só a sintaxe está
--   verificada (scripts/check-lua-sintaxe.sh).
--
-- QUEM LIGA, IDENTIFICADO (R277):
--   modo `ramal`     o aparelho já está autenticado: NÃO se pede PIN pessoal.
--                    O control plane resolve o ramal → pessoa e devolve, nas
--                    `channel_vars` do `room_bridge`, um bilhete opaco de uso
--                    único que a ponte troca pelo nome. Este script não sabe
--                    que o bilhete existe: copia as variáveis, como sempre.
--   dial-in          depois do PIN da sala, o IVR oferece a identificação:
--                    ramal + cardinal, ou só cardinal (ou silêncio) para
--                    continuar. Com ramal, pede o PIN PESSOAL e verifica-o em
--                    /internal/v1/voice/ivr/verify-extension-pin, com a ORIGEM
--                    da chamada (número de quem liga e endereço do par SIP) —
--                    é a chave do travão do servidor. Acerto: o bilhete vem na
--                    resposta e junta-se às variáveis da perna para a ponte.
--                    Falha: UMA só frase, igual para PIN errado, ramal que não
--                    existe, PIN por definir, ramal bloqueado e origem travada
--                    — por telefone não se enumera ramais. Duas tentativas por
--                    chamada, e a pessoa entra na mesma, como anónima: falhar
--                    a identificação nunca desliga a chamada.
--   Nem o ramal, nem o PIN pessoal, nem o bilhete são escritos no log por este
--   script (R227). NADA disto correu contra um FreeSWITCH real: só a sintaxe.
--
-- Segredos NUNCA em claro: lidos de variáveis globais do FreeSWITCH que, por sua
-- vez, vêm do ambiente (ver voice/cluster/freeswitch-entrypoint.sh):
--   ${delonix_control_url}     ex.: http://127.0.0.1:8180
--   ${delonix_voice_secret}    == VOICE_INTERNAL_SECRET do backend
--
-- Requisitos: mod_lua, mod_curl, mod_conference, mod_dptools. O SRTP à entrada
-- é imposto pela variável GLOBAL rtp_secure_media=mandatory (R226), não por
-- este script: o `setVariable` abaixo vem depois do `answer`, com o SDP da
-- perna já negociado.
--
-- ============================================================================
-- Ponte telefone↔sala (ADR-0010) — LIGADA
-- ============================================================================
-- A pergunta que este ficheiro deixou em aberto na Abordagem B — «qual é o
-- mecanismo FreeSWITCH para mandar/receber RTP cru, com uma chave SRTP dada
-- por fora, a um par UDP arbitrário, sem um segundo diálogo SIP?» — foi
-- respondida contra um FreeSWITCH 1.11.3 real: NÃO HÁ. A imagem de stock não
-- traz um módulo que o faça, e o que ela sabe fazer bem é originar uma segunda
-- perna SIP. A decisão está em docs/pstn-sfu-bridge-design.md §Superseded.
--
-- Por isso o lado do SFU ganhou o shim que a própria pergunta antecipava: um
-- UA SIP mínimo com SDES-SRTP (server/src/phone_bridge/), que atende o INVITE
-- desta perna, transcodifica G.711↔Opus e publica o chamador na sala como um
-- participante normal, devolvendo-lhe a mistura menos a própria voz.
--
-- O contrato é o objecto "room_bridge" de /internal/v1/voice/ivr/validate
-- (server/src/voice.rs::RoomBridgeResp): { sip_uri, channel_vars,
-- srtp_profile }. Mudar os nomes de um lado sem o outro deixa a ponte
-- silenciosamente inactiva — o IVR cai na conferência local, como sempre fez.
--
-- A ponte RECUSA (488) uma oferta sem a=crypto: `channel_vars` traz o
-- `rtp_secure_media` que obriga o FreeSWITCH a oferecer SRTP nesta perna.
-- ============================================================================

local api = freeswitch.API()
local control_url = (api:executeString("global_getvar delonix_control_url") or ""):gsub("%s+$", "")
local secret      = (api:executeString("global_getvar delonix_voice_secret") or ""):gsub("%s+$", "")

-- `argv` é o que o dialplan (ou outro script) passou depois do nome do ficheiro.
local modo_ramal = (argv ~= nil and argv[1] == "ramal")

local MAX_TRIES = 3
local PIN_LEN   = 6
-- Identificação de quem liga de fora (R277): tentativas por chamada.
local MAX_ID_TRIES = 2
-- Perfil sofia de onde sai a perna para a ponte. Só `internal` existe na
-- configuração distribuída (voice/freeswitch/sip_profiles/); a variável deixa
-- apontar a outro sem tocar no script.
local bridge_profile = (api:executeString("global_getvar delonix_bridge_profile") or ""):gsub("%s+$", "")
if bridge_profile == "" then bridge_profile = "internal" end

-- POST JSON ao control plane via mod_curl; devolve o corpo (string) ou nil.
local function http_post(path, body)
  -- Sintaxe do mod_curl: as opções vêm ANTES do método, cada uma com o seu
  -- valor separado por espaço (`content-type <tipo>`, `append_headers
  -- <nome:valor>`), e o corpo é o argumento a seguir a `post`. Na forma
  -- antiga (`post content-type=… '<corpo>' '<cabeçalho>'`) o módulo tomava
  -- «content-type=application/json» pelo corpo e mandava o pedido sem
  -- Content-Type nem segredo: o servidor respondia 415 e todo o PIN era «errado».
  local args = string.format(
    "%s%s content-type application/json append_headers 'X-Voice-Secret: %s' post '%s'",
    control_url, path, secret, body)
  -- Pela API do mod_curl, e não pela aplicação de dialplan (R227): os
  -- argumentos de uma aplicação — o segredo e, no IVR, o PIN — ficam escritos
  -- na linha EXECUTE do log a cada chamada, e no app_log do CDR. A API leva os
  -- mesmos argumentos e devolve o corpo da resposta.
  return api:execute("curl", args)
end

-- Extrai um valor STRING simples de um JSON plano (sem dependências externas).
local function json_str(json, key)
  if not json then return nil end
  return json:match('"' .. key .. '"%s*:%s*"([^"]*)"')
end

-- Extrai um valor NUMÉRICO simples (sem aspas) — usado para "port".
local function json_num(json, key)
  if not json then return nil end
  return json:match('"' .. key .. '"%s*:%s*(%d+)')
end

-- Extrai um sub-objecto JSON como string, para os `json_str`/`json_num` acima
-- procurarem DENTRO dele (o parser é plano de propósito). Devolve nil se o
-- campo estiver ausente — ver voice.rs::room_bridge_for, que enumera todas as
-- razões, nenhuma delas um erro do ponto de vista deste IVR.
local function json_sub_object(json, key)
  if not json then return nil end
  return json:match('"' .. key .. '"%s*:%s*(%b{})')
end

-- Lê os campos da ponte telefone↔sala. Contrato exacto com
-- server/src/voice.rs::RoomBridgeResp.
local function room_bridge_from_json(resp)
  local obj = json_sub_object(resp, "room_bridge")
  if not obj then return nil end
  local sip_uri = json_str(obj, "sip_uri")
  if not sip_uri or sip_uri == "" then return nil end
  -- `channel_vars` é um mapa string→string de chaves que o backend escolhe —
  -- lê-se par a par em vez de por nome, senão acrescentar uma variável do lado
  -- do Rust exigiria mexer aqui (que é exactamente o acoplamento que já nos
  -- mordeu uma vez).
  local vars = {}
  local obj_vars = json_sub_object(obj, "channel_vars")
  if obj_vars then
    for k, v in obj_vars:gmatch('"([^"]+)"%s*:%s*"([^"]*)"') do vars[k] = v end
  end
  return { sip_uri = sip_uri, channel_vars = vars, srtp_profile = json_str(obj, "srtp_profile") }
end

-- Modo `ramal`: quem liga é o utilizador que o perfil `internal` autenticou
-- (auth-calls=true). Sem essas duas variáveis a chamada não foi autenticada —
-- desliga-se, não se recua para o From. Os valores vão para dentro de um JSON
-- e de um argumento entre plicas: só se aceita o alfabeto de um AOR e de um
-- domínio.
local ramal_user, ramal_domain = "", ""
if modo_ramal then
  ramal_user = session:getVariable("sip_auth_username") or ""
  ramal_domain = session:getVariable("sip_auth_realm") or ""
  local limpo = "^[%w%._%-]+$"
  if not ramal_user:match(limpo) or not ramal_domain:match(limpo) then
    freeswitch.consoleLog("warning",
      "[delonix ivr] modo ramal sem sip_auth_username/sip_auth_realm validos — a rejeitar\n")
    session:hangup("CALL_REJECTED")
    return
  end
end

session:answer()
session:setVariable("rtp_secure_media", "mandatory") -- não recusa esta perna, já negociada: isso é da global (R226)
session:sleep(300)

local did = ""
if not modo_ramal then
  did = session:getVariable("sip_to_user") or session:getVariable("destination_number") or ""
  -- Normaliza para +E.164 (o DID chega tipicamente sem '+').
  if did ~= "" and did:sub(1, 1) ~= "+" then did = "+" .. did end
end

local room_code = nil
local voice_room_id = nil
local room_bridge = nil
local org_sip_domain = nil
for try = 1, MAX_TRIES do
  -- Pede o PIN (min=len, max=len, tries=1, timeout, terminador #).
  local pin = session:playAndGetDigits(
    PIN_LEN, PIN_LEN, 1, 7000, "#",
    "ivr/ivr-please_enter_pin_followed_by_pound.wav",
    "ivr/ivr-that_was_an_invalid_entry.wav",
    "\\d+")

  if pin and #pin == PIN_LEN then
    local resp
    if modo_ramal then
      local body = string.format('{"sip_username":"%s","domain":"%s","pin":"%s"}',
        ramal_user, ramal_domain, pin)
      resp = http_post("/internal/v1/voice/ivr/validate-extension", body)
    else
      local body = string.format('{"did_e164":"%s","pin":"%s"}', did, pin)
      resp = http_post("/internal/v1/voice/ivr/validate", body)
    end
    room_code = json_str(resp, "room_code")
    voice_room_id = json_str(resp, "voice_room_id")
    room_bridge = room_bridge_from_json(resp)
    org_sip_domain = json_str(resp, "org_sip_domain")
    if room_code and #room_code > 0 then break end
  end

  if try < MAX_TRIES then
    session:streamFile("conference/conf-bad-pin.wav")
  end
end

if not room_code then
  session:streamFile("voicemail/vm-goodbye.wav")
  session:hangup()
  return
end

-- Identificação de quem liga de fora (R277). Só faz sentido com a ponte: na
-- conferência local do FreeSWITCH não há censo onde o nome apareça. E só com
-- o que o `validate` devolveu — o domínio da organização e a sala de voz —,
-- validados antes de irem para dentro de um JSON entre plicas.
local function identificar_quem_liga()
  if modo_ramal or not room_bridge then return end
  if not org_sip_domain or not org_sip_domain:match("^[%w%._%-]+$") then return end
  if not voice_room_id or not voice_room_id:match("^[%x%-]+$") then return end
  -- A origem, como o FreeSWITCH a vê. Só o alfabeto de um número e de um
  -- endereço; o resto cai (o servidor volta a limpar).
  local function limpa(v) return ((v or ""):gsub("[^%w%+%.:_%-]", "")) end
  local origem_numero = limpa(session:getVariable("caller_id_number"))
  local origem_rede = limpa(session:getVariable("sip_network_ip"))

  for _ = 1, MAX_ID_TRIES do
    if not session:ready() then return end
    -- «Por favor digite o seu ramal, depois a tecla sustenido.» Só cardinal,
    -- ou silêncio, é «continuar sem me identificar».
    local ramal = session:read(0, 5, "ivr/ivr-please_enter_extension_followed_by_pound.wav", 5000, "#") or ""
    if ramal == "" then return end
    local pin_pessoal = ""
    if ramal:match("^%d%d%d%d?%d?$") then
      pin_pessoal = session:read(PIN_LEN, PIN_LEN, "ivr/ivr-please_enter_pin_followed_by_pound.wav", 7000, "#") or ""
    end
    -- Um ramal ou um PIN mal formados nem chegam ao servidor — e levam a
    -- MESMA recusa que um PIN errado.
    if pin_pessoal:match("^%d%d%d%d%d%d$") then
      local body = string.format(
        '{"domain":"%s","extension":"%s","pin":"%s","voice_room_id":"%s","origin":{"caller_number":"%s","network_ip":"%s"}}',
        org_sip_domain, ramal, pin_pessoal, voice_room_id, origem_numero, origem_rede)
      local resp = http_post("/internal/v1/voice/ivr/verify-extension-pin", body)
      if resp and resp:match('"valid"%s*:%s*true') then
        -- O bilhete para a ponte vem em `channel_vars`; copia-se par a par,
        -- sem o conhecer pelo nome (como as do `room_bridge`).
        local obj_vars = json_sub_object(resp, "channel_vars")
        if obj_vars then
          for k, v in obj_vars:gmatch('"([^"]+)"%s*:%s*"([^"]*)"') do
            room_bridge.channel_vars[k] = v
          end
        end
        session:streamFile("ivr/ivr-thank_you.wav")
        return
      end
    end
    -- UMA só recusa («O seu número PIN ou ramal não é válido»), seja qual for
    -- a razão do servidor: `invalid`, `not_set`, `locked` ou `origin_locked`.
    session:streamFile("ivr/ivr-pin_or_extension_is-invalid.wav")
  end
  -- Esgotadas as tentativas: continua como participante anónimo.
end
identificar_quem_liga()

-- Marca o início e junta à conferência da sala (perfil 'delonix' com SRTP).
local started = os.time()
session:streamFile("conference/conf-welcome.wav")

local ponte_ok = false
if room_bridge then
  -- As variáveis do backend vão no PREFIXO `[...]` da dial string, não por
  -- `session:setVariable`: essas ficariam na perna A (o chamador), e o que
  -- precisa delas é a perna B. É o `rtp_secure_media=mandatory:<perfil>` que
  -- obriga ESTA perna a oferecer SRTP — sem `a=crypto` a ponte responde 488.
  -- (Um valor com vírgula partiria a lista; hoje nenhum tem, e o backend é
  -- quem os escolhe — ver voice.rs::room_bridge_for.)
  local vars = {}
  for k, v in pairs(room_bridge.channel_vars) do
    vars[#vars + 1] = string.format("%s=%s", k, v)
  end
  local prefixo = ""
  if #vars > 0 then prefixo = "[" .. table.concat(vars, ",") .. "]" end
  local dial = string.format("%ssofia/%s/%s", prefixo, bridge_profile, room_bridge.sip_uri)
  -- O log leva o destino, não as variáveis: entre elas pode ir o bilhete de
  -- identidade de quem liga (R277).
  freeswitch.consoleLog("info", string.format(
    "[delonix ponte] sala=%s -> sofia/%s/%s (srtp=%s)\n",
    room_code, bridge_profile, room_bridge.sip_uri, tostring(room_bridge.srtp_profile)))
  session:execute("bridge", dial)
  ponte_ok = (session:getVariable("originate_disposition") == "SUCCESS")
  if not ponte_ok then
    -- Ponte em baixo, INVITE recusado (IP fora da allowlist, sem a=crypto, sem
    -- G.711) ou sala já sem SFU: não se perde o chamador por isso.
    freeswitch.consoleLog("warning", string.format(
      "[delonix ponte] sala=%s: bridge falhou (%s) — cai na conferencia local\n",
      room_code, tostring(session:getVariable("originate_disposition"))))
  end
end

-- Recuo (e o caminho de sempre quando a ponte não está configurada):
-- conferência isolada no FreeSWITCH — quem liga por telefone ouve os outros
-- chamadores PSTN, mas não os participantes WebRTC da mesma sala.
if not ponte_ok and session:ready() then
  session:execute("conference", room_code .. "@delonix")
end

-- Pós-chamada: envia o CDR ao control plane (duração em segundos).
local duration = os.time() - started
local caller = session:getVariable("caller_id_number") or ""
if not modo_ramal and voice_room_id and #voice_room_id > 0 then
  local cdr = string.format(
    '{"voice_room_id":"%s","caller_number":"%s","did_e164":"%s","duration_secs":%d}',
    voice_room_id, caller, did, duration)
  http_post("/internal/v1/voice/ivr/cdr", cdr)
end
