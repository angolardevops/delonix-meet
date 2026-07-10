-- Delonix Meet — IVR de dial-in PSTN (FreeSWITCH / mod_lua)
--
-- Fluxo: atende a chamada → pede o PIN por DTMF → valida no control plane Rust
-- (/api/voice/ivr/validate, autenticado por segredo partilhado) → junta o
-- chamador à conferência da sala (nome = room_code). No fim, envia o CDR.
--
-- Segredos NUNCA em claro: lidos de variáveis globais do FreeSWITCH que, por sua
-- vez, vêm do ambiente (ver vars.xml / docker-compose.voice.yml):
--   ${delonix_control_url}     ex.: http://127.0.0.1:8180
--   ${delonix_voice_secret}    == VOICE_INTERNAL_SECRET do backend
--
-- Requisitos: mod_lua, mod_curl, mod_conference, mod_dptools. SRTP é imposto no
-- perfil SIP (rtp_secure_media=mandatory) — este script não faz media em claro.

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

-- Extrai um valor string simples de um JSON plano (sem dependências externas).
local function json_str(json, key)
  if not json then return nil end
  return json:match('"' .. key .. '"%s*:%s*"([^"]*)"')
end

session:answer()
session:setVariable("rtp_secure_media", "mandatory") -- SRTP obrigatório, sem fallback
session:sleep(300)

local did = session:getVariable("sip_to_user") or session:getVariable("destination_number") or ""
-- Normaliza para +E.164 (o DID chega tipicamente sem '+').
if did ~= "" and did:sub(1, 1) ~= "+" then did = "+" .. did end

local room_code = nil
local voice_room_id = nil
local sfu_rtp_port = nil
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
    sfu_rtp_port = json_str(resp, "sfu_rtp_port")
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

if sfu_rtp_port and #sfu_rtp_port > 0 then
  -- Integração Fase 3: Envia RTP diretamente para o SFU WebRTC (Rust)
  local sfu_ip = "127.0.0.1" -- O IP pode ser extraído do json futuramente
  session:execute("bridge", string.format("sofia/internal/sip:sfu@%s:%s", sfu_ip, sfu_rtp_port))
else
  -- Fallback original: Conferência isolada no FreeSWITCH
  session:execute("conference", room_code .. "@delonix")
end

-- Pós-chamada: envia o CDR ao control plane (duração em segundos).
local duration = os.time() - started
local caller = session:getVariable("caller_id_number") or ""
if voice_room_id and #voice_room_id > 0 then
  local cdr = string.format(
    '{"voice_room_id":"%s","caller_number":"%s","did_e164":"%s","duration_secs":%d}',
    voice_room_id, caller, did, duration)
  http_post("/api/voice/ivr/cdr", cdr)
end
