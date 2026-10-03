-- Delonix Meet — IVR de dial-in PSTN (FreeSWITCH / mod_lua)
--
-- Fluxo: atende a chamada → pede o PIN por DTMF → valida no control plane Rust
-- (/internal/v1/voice/ivr/validate, autenticado por segredo partilhado) → junta o
-- chamador à conferência da sala (nome = room_code). No fim, envia o CDR.
--
-- Segredos NUNCA em claro: lidos de variáveis globais do FreeSWITCH que, por sua
-- vez, vêm do ambiente (ver vars.xml / docker-compose.voice.yml):
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

local MAX_TRIES = 3
local PIN_LEN   = 6
-- Perfil sofia de onde sai a perna para a ponte. Só `internal` existe na
-- configuração distribuída (voice/freeswitch/sip_profiles/); a variável deixa
-- apontar a outro sem tocar no script.
local bridge_profile = (api:executeString("global_getvar delonix_bridge_profile") or ""):gsub("%s+$", "")
if bridge_profile == "" then bridge_profile = "internal" end

-- POST JSON ao control plane via mod_curl; devolve o corpo (string) ou nil.
local function http_post(path, body)
  -- curl app: url, método, headers e dados; resultado fica em ${curl_response_data}
  local args = string.format(
    "%s%s post content-type=application/json '%s' " ..
    "'X-Voice-Secret: %s'",
    control_url, path, body, secret)
  session:execute("curl", args)
  return session:getVariable("curl_response_data")
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

session:answer()
session:setVariable("rtp_secure_media", "mandatory") -- não recusa esta perna, já negociada: isso é da global (R226)
session:sleep(300)

local did = session:getVariable("sip_to_user") or session:getVariable("destination_number") or ""
-- Normaliza para +E.164 (o DID chega tipicamente sem '+').
if did ~= "" and did:sub(1, 1) ~= "+" then did = "+" .. did end

local room_code = nil
local voice_room_id = nil
local room_bridge = nil
for try = 1, MAX_TRIES do
  -- Pede o PIN (min=len, max=len, tries=1, timeout, terminador #).
  local pin = session:playAndGetDigits(
    PIN_LEN, PIN_LEN, 1, 7000, "#",
    "ivr/ivr-please_enter_pin_followed_by_pound.wav",
    "ivr/ivr-that_was_an_invalid_entry.wav",
    "\\d+")

  if pin and #pin == PIN_LEN then
    local body = string.format('{"did_e164":"%s","pin":"%s"}', did, pin)
    local resp = http_post("/internal/v1/voice/ivr/validate", body)
    room_code = json_str(resp, "room_code")
    voice_room_id = json_str(resp, "voice_room_id")
    room_bridge = room_bridge_from_json(resp)
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
  freeswitch.consoleLog("info", string.format(
    "[delonix ponte] sala=%s -> %s (srtp=%s)\n",
    room_code, dial, tostring(room_bridge.srtp_profile)))
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
if voice_room_id and #voice_room_id > 0 then
  local cdr = string.format(
    '{"voice_room_id":"%s","caller_number":"%s","did_e164":"%s","duration_secs":%d}',
    voice_room_id, caller, did, duration)
  http_post("/internal/v1/voice/ivr/cdr", cdr)
end
