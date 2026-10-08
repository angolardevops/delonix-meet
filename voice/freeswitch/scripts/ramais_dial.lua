-- Delonix Meet — dialplan Lua dos RAMAIS internos (FreeSWITCH / mod_lua)
--
-- Fluxo: um ramal já registado disca um número curto (3–5 dígitos) → resolve
-- no control plane Rust QUAL AOR (sip_username) esse número é, DENTRO da
-- mesma org do chamador (/internal/v1/voice/ivr/resolve-extension, autenticado por
-- segredo partilhado) → liga directamente ao registo desse AOR
-- (`bridge(user/<sip_username>@<domínio>)`).
--
-- Excepção (R273): o NÚMERO DE ACESSO ÀS REUNIÕES. Não está escrito aqui nem
-- no dialplan — é o control plane que o conhece (VOICE_MEETING_ACCESS_NUMBER)
-- e responde `"meeting_access":true` em vez de um AOR. A chamada passa então
-- para o IVR da sala (`dialin_ivr.lua ramal`), que pede o PIN e a entrega à
-- ponte telefone↔sala. NUNCA correu contra um FreeSWITCH real.
--
-- Segunda excepção (R292): a REDE PÚBLICA. Um número que não é de um ramal
-- sai por um tronco se o plano de marcação da organização o mandar — e os
-- números de emergência saem SEMPRE, antes de se procurar um ramal. Quem
-- decide é o control plane, que responde `"outbound":true` com a organização
-- do ramal que o FreeSWITCH AUTENTICOU (sip_auth_username / sip_auth_realm,
-- nunca o From). A chamada passa ao contexto `delonix-outbound`, cujo plano
-- o servidor serve número a número (mod_xml_curl, telephony_fs_xml.rs).
--
-- Porquê um passo de tradução em vez de discar `user/${destination_number}`
-- directamente: o número curto (extensão) só é único DENTRO da org — o AOR
-- registado (sip_username) é que é globalmente único. Ver o comentário no
-- topo de server/src/ramais.rs sobre esta escolha e o que NÃO foi verificado
-- contra uma instância real.
--
-- Segredos NUNCA em claro: lidos de variáveis globais do FreeSWITCH que, por
-- sua vez, vêm do ambiente (ver voice/cluster/freeswitch-entrypoint.sh):
--   ${delonix_control_url}     ex.: http://127.0.0.1:8181 (o listener INTERNO)
--   ${delonix_voice_secret}    == VOICE_INTERNAL_SECRET do backend
--
-- Requisitos: mod_lua, mod_curl, mod_sofia. O SRTP à entrada é imposto pela
-- variável GLOBAL rtp_secure_media=mandatory (R226), não por este script:
-- o perfil dos ramais negoceia o SDP à chegada, antes de ele correr.

local api = freeswitch.API()
local control_url = (api:executeString("global_getvar delonix_control_url") or ""):gsub("%s+$", "")
local secret      = (api:executeString("global_getvar delonix_voice_secret") or ""):gsub("%s+$", "")

-- POST JSON ao control plane via mod_curl; devolve o corpo (string) ou nil.
local function http_post(path, body, full_url)
  -- Sintaxe do mod_curl: as opções vêm ANTES do método, cada uma com o seu
  -- valor separado por espaço (`content-type <tipo>`, `append_headers
  -- <nome:valor>`), e o corpo é o argumento a seguir a `post`. Na forma
  -- antiga (`post content-type=… '<corpo>' '<cabeçalho>'`) o módulo tomava
  -- «content-type=application/json» pelo corpo e mandava o pedido sem
  -- Content-Type nem segredo: o servidor respondia 415 e todo o PIN era «errado».
  -- Com tempo-limite: sem ele uma chamada — o 112 incluído — ficava pendurada
  -- o tempo que o control plane demorasse a não responder.
  local args = string.format(
    "%s%s connect-timeout 3 timeout 6 content-type application/json append_headers 'X-Voice-Secret: %s' post '%s'",
    full_url and "" or control_url, full_url or path, secret, body)
  -- Pela API do mod_curl, e não pela aplicação de dialplan (R227): os
  -- argumentos de uma aplicação — o segredo e, no IVR, o PIN — ficam escritos
  -- na linha EXECUTE do log a cada chamada, e no app_log do CDR. A API leva os
  -- mesmos argumentos e devolve o corpo da resposta.
  return api:execute("curl", args)
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

-- O que o digest autenticou nesta chamada. É disto, e só disto, que o
-- servidor tira a organização que paga uma chamada para fora.
local auth_user = session:getVariable("sip_auth_username") or ""
local auth_realm = session:getVariable("sip_auth_realm") or ""

-- Nada do que vai para o JSON pode fechar a cadeia de caracteres: o destino
-- vem do padrão do dialplan (dígitos e «+»); o resto, por via das dúvidas.
-- Nem a plica, que delimita o corpo no argumento do mod_curl, nem o `%`: o
-- mod_curl descodifica `%XX` no corpo DEPOIS desta limpeza.
local function limpo(s) return (s:gsub("[%c\"'\\%%]", "")) end

local body = string.format('{"domain":"%s","extension":"%s","auth_user":"%s","auth_realm":"%s"}',
  limpo(domain), limpo(destination), limpo(auth_user), limpo(auth_realm))
local resp = http_post("/internal/v1/voice/ivr/resolve-extension", body)

if resp and resp:match('"meeting_access"%s*:%s*true') then
  -- O IVR atende, autentica o ramal pelo digest e fala com o listener INTERNO
  -- do control plane — por isso é outro script, com o seu próprio URL.
  session:execute("lua", "dialin_ivr.lua ramal")
  return
end

if resp and resp:match('"outbound"%s*:%s*true') then
  local org_id = json_str(resp, "org_id")
  -- Só um UUID: é o que vai escolher os troncos de uma organização.
  if not org_id or not org_id:match("^%x%x%x%x%x%x%x%x%-%x%x%x%x%-%x%x%x%x%-%x%x%x%x%-%x%x%x%x%x%x%x%x%x%x%x%x$") then
    freeswitch.consoleLog("WARNING", "[delonix_ramais] saída sem organização válida — a rejeitar\n")
    session:hangup("CALL_REJECTED")
    return
  end
  session:setVariable("delonix_org_id", org_id)
  -- À operadora apresenta-se o número curto de quem marca, não o utilizador
  -- SIP do ramal — que é metade da credencial dele.
  local caller = json_str(resp, "caller_extension")
  if caller and caller:match("^%d+$") then
    session:setVariable("effective_caller_id_number", caller)
    session:setVariable("effective_caller_id_name", caller)
  end
  session:transfer(destination, "XML", "delonix-outbound")
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

-- S-02 (ADR-0023): o destino é um telemóvel e pode estar com a app morta, sem registo. Sem isto a
-- chamada morria em 10 ms (`USER_NOT_REGISTERED`, medido). Com `delonix_push_wait_secs` > 0 (DESLIGADO
-- por omissão: o comportamento antigo) a chamada fica a tocar ao chamador, o control plane é avisado
-- para acordar os aparelhos do ramal (push) e espera-se pelo REGISTO, até ao limite. Se o ramal já está
-- registado nada muda. A espera não é infinita e acaba logo que o chamador desligue.
local function wait_secs()
  return tonumber((api:executeString("global_getvar delonix_push_wait_secs") or ""):match("%d+")) or 0
end

local function registered(user, dom)
  local c = api:executeString(string.format("sofia_contact internal/%s@%s", user, dom)) or ""
  return c ~= "" and not c:match("^error/")
end

local wait = wait_secs()
if wait > 0 and not registered(target_sip_username, domain) then
  -- Só caracteres que o mod_curl e o JSON aguentam (a lista é a do resto do script).
  local body = string.format('{"domain":"%s","sip_username":"%s","call_uuid":"%s","caller_sip_username":"%s"}',
    limpo(domain), limpo(target_sip_username), limpo(session:get_uuid()), limpo(auth_user))
  -- `delonix_push_wake_url` (URL completo) só existe para a prova do S-02; em produção é o control plane.
  local wake_full = (api:executeString("global_getvar delonix_push_wake_url") or ""):gsub("%s+$", "")
  local wake
  if wake_full ~= "" and wake_full:match("^https?://") then
    wake = http_post("", body, wake_full)
  else
    wake = http_post("/internal/v1/voice/push/wake", body)
  end
  -- O servidor decide se há aparelhos acordáveis. Sem resposta, ou `awaiting:false`, não se espera:
  -- é o comportamento de antes (falha já), e não uma chamada presa à espera de nada.
  if wake and wake:match('"awaiting"%s*:%s*true') then
    session:execute("ring_ready")
    local deadline = os.time() + wait
    while session:ready() and os.time() < deadline and not registered(target_sip_username, domain) do
      session:sleep(500)
    end
    if not session:ready() then
      return -- o chamador desistiu à espera
    end
    if not registered(target_sip_username, domain) then
      freeswitch.consoleLog("INFO", "[delonix_ramais] " .. destination .. "@" .. domain .. " não acordou em " .. wait .. " s\n")
      session:hangup("NO_USER_RESPONSE")
      return
    end
    freeswitch.consoleLog("INFO", "[delonix_ramais] " .. destination .. "@" .. domain .. " registou-se a tempo\n")
  end
end

session:setVariable("rtp_secure_media", "mandatory") -- não recusa esta perna, já negociada: isso é da global (R226)
session:execute("bridge", string.format("user/%s@%s", target_sip_username, domain))
