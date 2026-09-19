-- Delonix Meet — IVR de dial-in PSTN (FreeSWITCH / mod_lua)
--
-- Fluxo: atende a chamada → pede o PIN por DTMF → valida no control plane Rust
-- (/api/voice/ivr/validate, autenticado por segredo partilhado) → junta o
-- chamador à sala. No fim, envia o CDR.
--
-- Segredos NUNCA em claro: lidos de variáveis globais do FreeSWITCH que, por sua
-- vez, vêm do ambiente (ver vars.xml / docker-compose.voice.yml):
--   ${delonix_control_url}     ex.: http://127.0.0.1:8180
--   ${delonix_voice_secret}    == VOICE_INTERNAL_SECRET do backend
--
-- Requisitos: mod_lua, mod_curl, mod_conference, mod_dptools. SRTP é imposto no
-- perfil SIP (rtp_secure_media=mandatory) — este script não faz media em claro.
--
-- ============================================================================
-- Ponte PSTN↔SFU (Abordagem B, docs/pstn-sfu-bridge-design.md) — ESTADO
-- ============================================================================
-- O control plane (server/src/voice.rs::ivr_validate_pin) já devolve, quando
-- consegue activar a ponte, um objecto "pstn_bridge": { host, port,
-- ingress_key_b64, egress_key_b64, profile, payload_type }. Este script já
-- sabe LER esses campos (ver `pstn_bridge_from_json` abaixo — é o lado do
-- CONTRATO que não pode andar dessincronizado do backend, e está testado
-- contra o formato real da resposta).
--
-- O que este script NÃO faz ainda, e é a lacuna HONESTA que fica documentada
-- em vez de inventada: a acção FreeSWITCH que efectivamente manda/recebe RTP
-- puro, cifrado com estas chaves SRTP, para o host:porta do SFU — SEM um
-- segundo diálogo SIP (que reintroduziria o acoplamento que a Abordagem B
-- existe para evitar, ver o design doc). Não foi possível verificar com
-- confiança, nesta sandbox sem uma instância FreeSWITCH real, qual é o
-- verbo/variável de canal correcto para isto. Non-cabidatos considerados e
-- descartados por não se ajustarem ao que é preciso:
--   - `bridge`/`sofia/internal/sip:...`  → é o que a Abordagem A (REJEITADA,
--     ver docs/pstn-bridge-architecture.md) tinha esboçado; exige um SEGUNDO
--     diálogo SIP e um respondedor SIP do lado do SFU — o SFU teria de falar
--     SDP/SDES-SRTP, e deixaríamos de estar a evitar sinalização nova no SFU.
--   - `mod_audio_fork`  → existe e é real, mas manda áudio por WEBSOCKET para
--     STT em tempo real, não RTP/SRTP bidireccional para um par UDP arbitrário.
--   - `uuid_deflect`, `snoop`, `unicast` → não encontrámos confirmação de que
--     qualquer um faça "RTP/SRTP cru para host:porta arbitrário" tal como
--     este design precisa.
--
-- PERGUNTA EXACTA para quem tiver uma instância FreeSWITCH real à mão:
--   Qual é o mecanismo documentado do FreeSWITCH (dialplan app, variável de
--   canal, ou módulo) para fazer uma chamada/conferência ACTIVA enviar E
--   receber RTP puro, cifrado com uma chave SRTP fornecida externamente
--   (não negociada por SDP), a um endereço UDP arbitrário — sem abrir um
--   segundo diálogo SIP para esse endereço? Se a resposta for "não há, tem de
--   ser SIP", então a Abordagem B precisa de um pequeno respondedor
--   SIP/SDES-SRTP no lado do SFU (um "shim" mais magro que o WebRTC completo
--   que a Abordagem A evitava, mas um shim) — decisão a tomar com essa
--   confirmação em mãos, não adivinhada aqui.
--
-- Enquanto essa pergunta não tiver resposta confirmada, este script faz a
-- coisa SEGURA: regista a informação da ponte (útil para depuração/telemetria)
-- e continua a cair na conferência LOCAL do FreeSWITCH — o comportamento de
-- sempre. Isto significa que, à saída desta tarefa, o áudio PSTN↔WebRTC
-- CONTINUA por ligar de facto (o lado SFU está pronto e testado — ver
-- server/src/pstn_bridge.rs — mas nada o liga aqui ainda). Ver o relatório da
-- tarefa para o que falta confirmar antes de activar a chamada real.
-- ============================================================================

local api = freeswitch.API()
local control_url = (api:executeString("global_getvar delonix_control_url") or ""):gsub("%s+$", "")
local secret      = (api:executeString("global_getvar delonix_voice_secret") or ""):gsub("%s+$", "")

local MAX_TRIES = 3
local PIN_LEN   = 6

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

-- Extrai o sub-objecto "pstn_bridge": {...} como string, para os `json_str`/
-- `json_num` acima procurarem DENTRO dele (o parser é plano de propósito —
-- nunca precisou de recursão até este campo existir). Devolve nil se o campo
-- estiver ausente (backend não conseguiu activar a ponte — ver
-- voice.rs::activate_pstn_bridge_for, que descreve todas as razões possíveis,
-- nenhuma delas um erro do ponto de vista deste IVR).
local function json_sub_object(json, key)
  if not json then return nil end
  return json:match('"' .. key .. '"%s*:%s*(%b{})')
end

-- Lê os campos da ponte PSTN↔SFU da resposta de /api/voice/ivr/validate.
-- Contrato exacto com server/src/voice.rs::PstnBridgeResp — mudar os nomes
-- de um lado sem o outro é o erro silencioso que este comentário existe
-- para evitar (ver a nota no cabeçalho do ficheiro).
local function pstn_bridge_from_json(resp)
  local obj = json_sub_object(resp, "pstn_bridge")
  if not obj then return nil end
  local host = json_str(obj, "host")
  local port = json_num(obj, "port")
  if not host or not port or host == "" then return nil end
  return {
    host = host,
    port = port,
    ingress_key_b64 = json_str(obj, "ingress_key_b64"),
    egress_key_b64 = json_str(obj, "egress_key_b64"),
    profile = json_str(obj, "profile"),
    payload_type = json_num(obj, "payload_type"),
  }
end

session:answer()
session:setVariable("rtp_secure_media", "mandatory") -- SRTP obrigatório, sem fallback
session:sleep(300)

local did = session:getVariable("sip_to_user") or session:getVariable("destination_number") or ""
-- Normaliza para +E.164 (o DID chega tipicamente sem '+').
if did ~= "" and did:sub(1, 1) ~= "+" then did = "+" .. did end

local room_code = nil
local voice_room_id = nil
local pstn_bridge = nil
for try = 1, MAX_TRIES do
  -- Pede o PIN (min=len, max=len, tries=1, timeout, terminador #).
  local pin = session:playAndGetDigits(
    PIN_LEN, PIN_LEN, 1, 7000, "#",
    "ivr/ivr-please_enter_pin_followed_by_pound.wav",
    "ivr/ivr-that_was_an_invalid_entry.wav",
    "\\d+")

  if pin and #pin == PIN_LEN then
    local body = string.format('{"did_e164":"%s","pin":"%s"}', did, pin)
    local resp = http_post("/api/voice/ivr/validate", body)
    room_code = json_str(resp, "room_code")
    voice_room_id = json_str(resp, "voice_room_id")
    pstn_bridge = pstn_bridge_from_json(resp)
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

if pstn_bridge then
  -- A ponte SFU está activa e pronta do lado do Rust (ingress+egress SRTP,
  -- mistura Opus — ver server/src/pstn_bridge.rs). O que falta é o passo
  -- documentado no cabeçalho deste ficheiro: o verbo/variável FreeSWITCH
  -- correcto para mandar/receber RTP/SRTP cru para pstn_bridge.host:port SEM
  -- um segundo diálogo SIP. Regista-se a informação (visível em
  -- `fs_cli -x "uuid_dump <uuid>"` / log) para depuração, e cai-se no
  -- caminho seguro de sempre — NUNCA se inventa aqui uma chamada a uma API
  -- que não foi possível confirmar.
  freeswitch.consoleLog("info", string.format(
    "[delonix pstn_bridge] sala=%s sfu=%s:%s profile=%s pt=%s — " ..
    "PONTE NÃO LIGADA (ver comentário no topo de dialin_ivr.lua): " ..
    "a confirmar o mecanismo FreeSWITCH de RTP/SRTP cru antes de activar.\n",
    room_code, tostring(pstn_bridge.host), tostring(pstn_bridge.port),
    tostring(pstn_bridge.profile), tostring(pstn_bridge.payload_type)))
end

-- Fallback (e, até a ponte SFU ser ligada acima, o caminho SEMPRE seguido):
-- conferência isolada no FreeSWITCH — quem liga por telefone ouve os outros
-- chamadores PSTN, mas ainda não os participantes WebRTC da mesma sala.
session:execute("conference", room_code .. "@delonix")

-- Pós-chamada: envia o CDR ao control plane (duração em segundos).
local duration = os.time() - started
local caller = session:getVariable("caller_id_number") or ""
if voice_room_id and #voice_room_id > 0 then
  local cdr = string.format(
    '{"voice_room_id":"%s","caller_number":"%s","did_e164":"%s","duration_secs":%d}',
    voice_room_id, caller, did, duration)
  http_post("/api/voice/ivr/cdr", cdr)
end
