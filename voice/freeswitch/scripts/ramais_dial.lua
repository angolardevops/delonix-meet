-- Delonix Meet — dialplan Lua dos RAMAIS internos (FreeSWITCH / mod_lua)
--
-- Fluxo: um ramal já registado disca um número curto (3–5 dígitos) → resolve
-- no control plane Rust QUAL AOR (sip_username) esse número é, DENTRO da
-- mesma org do chamador (/api/voice/ivr/resolve-extension, autenticado por
-- segredo partilhado) → liga directamente ao registo desse AOR
-- (`bridge(user/<sip_username>@<domínio>)`).
--
-- Excepção (R273): o NÚMERO DE ACESSO ÀS REUNIÕES. Não está escrito aqui nem
-- no dialplan — é o control plane que o conhece (VOICE_MEETING_ACCESS_NUMBER)
-- e responde `"meeting_access":true` em vez de um AOR. A chamada passa então
-- para o IVR da sala (`dialin_ivr.lua ramal`), que pede o PIN e a entrega à
-- ponte telefone↔sala. NUNCA correu contra um FreeSWITCH real.
--
-- Porquê um passo de tradução em vez de discar `user/${destination_number}`
-- directamente: o número curto (extensão) só é único DENTRO da org — o AOR
-- registado (sip_username) é que é globalmente único. Ver o comentário no
-- topo de server/src/ramais.rs sobre esta escolha e o que NÃO foi verificado
-- contra uma instância real.
--
-- Segredos NUNCA em claro: lidos de variáveis globais do FreeSWITCH que, por
-- sua vez, vêm do ambiente (ver vars.xml.inc / docker-compose.voice.yml):
--   ${delonix_control_url}     ex.: http://127.0.0.1:8180
--   ${delonix_voice_secret}    == VOICE_INTERNAL_SECRET do backend
--
-- Requisitos: mod_lua, mod_curl, mod_sofia. O SRTP à entrada é imposto pela
-- variável GLOBAL rtp_secure_media=mandatory (R226), não por este script:
-- o perfil dos ramais negoceia o SDP à chegada, antes de ele correr.

local api = freeswitch.API()
local control_url = (api:executeString("global_getvar delonix_control_url") or ""):gsub("%s+$", "")
local secret      = (api:executeString("global_getvar delonix_voice_secret") or ""):gsub("%s+$", "")

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
  session:execute("curl", args)
  return session:getVariable("curl_response_data")
end

-- Extrai um valor string simples de um JSON plano (sem dependências externas).
local function json_str(json, key)
  if not json then return nil end
  return json:match('"' .. key .. '"%s*:%s*"([^"]*)"')
end

-- O domínio do CHAMADOR (o que ele usou para registar-se) — é o que escopa
-- a busca do número curto à org certa. `domain_name` é o que o FreeSWITCH
-- preenche a partir do registo (via o directório dinâmico, mod_xml_curl);
-- `sip_from_host` é o recuo se, por alguma razão, aquele não vier preenchido.
local domain = session:getVariable("domain_name") or session:getVariable("sip_from_host") or ""
local destination = session:getVariable("destination_number") or ""

if domain == "" or destination == "" then
  freeswitch.consoleLog("WARNING", "[delonix_ramais] sem domínio ou destino — a rejeitar\n")
  session:hangup("UNALLOCATED_NUMBER")
  return
end

local body = string.format('{"domain":"%s","extension":"%s"}', domain, destination)
local resp = http_post("/api/voice/ivr/resolve-extension", body)

if resp and resp:match('"meeting_access"%s*:%s*true') then
  -- O IVR atende, autentica o ramal pelo digest e fala com o listener INTERNO
  -- do control plane — por isso é outro script, com o seu próprio URL.
  session:execute("lua", "dialin_ivr.lua ramal")
  return
end

local target_sip_username = json_str(resp, "sip_username")

if not target_sip_username or #target_sip_username == 0 then
  -- Não encontrado NESTA org (o número não existe ou pertence a outra org —
  -- indistinguível de propósito, para não revelar o inventário de outra org).
  freeswitch.consoleLog("INFO", "[delonix_ramais] " .. destination .. "@" .. domain .. " não resolvido\n")
  session:hangup("UNALLOCATED_NUMBER")
  return
end

session:setVariable("rtp_secure_media", "mandatory") -- não recusa esta perna, já negociada: isso é da global (R226)
session:execute("bridge", string.format("user/%s@%s", target_sip_username, domain))
