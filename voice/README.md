# Delonix Meet — Camada de Media do Dial-in PSTN (sub-fase 2)

Infra-as-code da **camada de media** do dial-in PSTN. O **control plane** (salas de
voz, PIN, DID, CDR, billing) vive no backend Rust (`server/src/voice.rs`) — **não**
há serviço novo. Aqui está só o media: Kamailio (borda SIP) + FreeSWITCH (IVR +
conferência), que falam com o control plane pela API interna de IVR.

## Fluxo

```
Telefone → SIP Trunk → Kamailio (ACL trunk + TLS + dispatcher)
                            │  load-balance
                            ▼
                       FreeSWITCH (N nós)
                        1) atende (SRTP obrigatório)
                        2) IVR pede PIN (DTMF)
                        3) POST /internal/v1/voice/ivr/validate  ─────► Control plane (Rust)
                           (X-Voice-Secret)               ◄───── { room_code, voice_room_id }
                        4) conference(room_code@delonix)
                        5) no fim: POST /internal/v1/voice/ivr/cdr ────► CDR + custo estimado
```

## Ficheiros
| Caminho | Papel |
|---|---|
| `kamailio/kamailio.cfg` | Proxy SIP: ACL do trunk (anti-fraude), TLS, **dispatcher** para os FreeSWITCH |
| `kamailio/dispatcher.list` | Pool de nós FreeSWITCH (acrescentar linhas para escalar) |
| `freeswitch/scripts/dialin_ivr.lua` | IVR: PIN → valida no control plane → junta à conferência → CDR |
| `freeswitch/dialplan/public/00_delonix_dialin.xml` | Encaminha inbound para o IVR |
| `freeswitch/autoload_configs/conference.conf.xml` | Perfil de conferência `delonix` (não impõe SRTP: isso é de cada perna SIP) |
| `cluster/freeswitch-entrypoint.sh` | O arranque do FreeSWITCH: fecha a vanilla da imagem, põe as variáveis do ambiente e os ficheiros do Meet. É o mesmo no `compose.yaml` e no cluster |
| `../compose.yaml` (serviços `kamailio`, `freeswitch`, `pbx`) | O laboratório de voz: `make compose-up`, medido por `make compose-voice-check` |
| `pbx-cliente/central.conf.tmpl` | O tronco da CENTRAL do PBX de laboratório (TLS, autenticado com a conta SIP da organização — ADR-0016). Modelo: o `make bootstrap` e o `scripts/cluster-voice.sh` põem-lhe o nome do bordo e a password |

## O PBX de laboratório tem dois troncos

O mesmo Asterisk (`pbx` no compose, `pbx-cliente` no cluster) entra no bordo de duas
maneiras, e não são a mesma coisa:

| Tronco | Como entra | O que o Meet sabe da chamada |
|---|---|---|
| `meet` (UDP 5060) | pela **allowlist** do bordo — faz de tronco contratado | nada: é um dial-in por `(número, PIN)` |
| `meet-central` (TLS 5061) | **autenticado** com a conta SIP da organização «ngolacloud» («Registo SIP», que o `make seed` grava) | a organização: a sala procura-se dentro dela |

Para os dois caberem no mesmo PBX, a allowlist do laboratório só aceita a **porta 5060 de
origem** — a do tronco UDP. O tronco TLS sai de uma porta efémera, não está na lista, e é
desafiado. O `make seed` cria também uma sala com PIN
(`deploy/compose/generated/sala-telefone.txt`), e o `make compose-voice-check` liga por cada
tronco: pelo da central, com o PIN certo (entra) e com um errado (autenticada, e recusada
pelo IVR). Com o PIN certo, diz também **para onde a chamada foi** — a ponte telefone↔sala
do SFU, ou a conferência local do FreeSWITCH —, lendo-o no registo do IVR em vez de o
presumir pelo ambiente.

```bash
make bootstrap     # gera VOICE_CENTRAL_PASSWORD, DATA_ENCRYPTION_KEYS e o tronco da central
make compose-up    # recusa arrancar se o bootstrap for anterior a isto
make compose-voice-check
# À mão, do PBX:  channel originate PJSIP/+244222000001@meet-central extension <PIN>@prova-pin
```

Um laboratório criado antes disto precisa de `make bootstrap` outra vez (não muda os
segredos que já tem) e de `make compose-down && make compose-up`.

## Segurança (não-negociável)
- **SRTP obrigatório**, sem fallback: quem recusa com `488` uma chamada em claro é a
  variável **global** `rtp_secure_media=mandatory`, posta por `sip_profiles/internal.xml`
  (e pelo arranque, `cluster/freeswitch-entrypoint.sh`). Não há parâmetro de perfil nem de
  conferência que o faça, e o `set` do dialplan só a recusa num perfil que negoceie tarde
  (`inbound-late-negotiation=true`), o que não é o caso do perfil dos ramais (R226).
  Portão: `bash scripts/softphone-prova.sh srtp-real`.
- **SIP-TLS** (5061) no Kamailio; certificado montado por volume (`/etc/ssl/delonix`),
  nunca comitado. Em dev usar self-signed; nunca desativar a camada.
- **Sem excepção por tronco**: um tronco declarado `srtp=off` só faz chamadas de **saída**
  em claro. À entrada, uma chamada em claro leva `488` venha de onde vier — uma operadora
  sem SRTP não nos consegue ligar. É de propósito, e foi decidido assim a 2026-10-04.
- **Anti-toll-fraud**: só se aceita inbound dos **IPs do trunk** (`ao_trunk.txt`,
  fornecido pelo provedor 5.1). Sem outbound não autenticado.
- **Segredos do ambiente**: `VOICE_INTERNAL_SECRET` (== do backend) e URLs vêm de env,
  nunca hardcoded no repo. O backend recusa (503) um segredo vazio, com menos de 32
  caracteres ou que já tenha estado publicado no repositório (R154); gera-o com
  `openssl rand -hex 32`.

## Rodar o segredo de voz

O `VOICE_INTERNAL_SECRET` autentica o FreeSWITCH perante o servidor: IVR, directório dos
ramais (o HA1 de cada ramal) e CDR. Roda-o sempre que possa ter sido lido por quem não
devia — em particular, uma instalação que tenha corrido com a configuração de antes da R227
escreveu-o no log do FreeSWITCH.

```bash
make voice-secret-rotate        # troca-o no .env; não mostra o valor
make compose-down && make compose-up   # compose: o `up` sozinho NÃO recria contentores que já existem
make cluster                    # cluster: reaplica o Secret delonix-voice e reinicia os dois
make compose-voice-check        # o FreeSWITCH volta a falar com o servidor
```

O servidor e o FreeSWITCH têm de mudar no mesmo passo: com valores diferentes, o servidor
responde `401` a cada registo de ramal e a cada PIN. Em produção (chart Helm) o Secret é
teu (`secrets.existingSecret`): troca-lhe a chave `VOICE_INTERNAL_SECRET` e reinicia os dois.
No fim, apaga os logs antigos do FreeSWITCH que possam ter o valor anterior.

O directório de logs do FreeSWITCH não leva o segredo: o arranque tira o nível DEBUG do
log e manda a configuração expandida (`freeswitch.xml.fsxml`) para um directório privado
ao lado da configuração. `DELONIX_FS_LOG_DEBUG=1` volta a ligar o DEBUG — e, com ele, o
segredo e os PIN no log. Os dígitos marcados (o PIN) também não: os dois planos de
marcação põem `sensitive_dtmf=true`, sem o qual o FreeSWITCH escreve uma linha por tecla.

## Testar sem trunk (com softphone SIP)
A camada de media valida-se **sem** o SIP trunk, usando um softphone (Linphone/Zoiper):
1. Backend Rust a correr com `VOICE_INTERNAL_SECRET` definido; criar um DID + sala de
   voz (obter o número e o PIN) — ver `docs/pstn-dial-in-fase0.md` e o E2E do control plane.
2. `make voice-images` e `make compose-up` (o `compose.yaml` da raiz; `LAN_IP=<ip>` expõe
   os ramais e a borda à rede local).
   **QR do Linphone no telemóvel:** o URL do QR tem de ser um nome que o telefone resolva e
   com um certificado em que ele confie — `meet.ngolacloud.local` (mDNS, autoassinado) não é.
   Duas vias: `make compose-up LAN_IP=…` (a rede local: a borda fica com um certificado de uma
   raiz de laboratório, que o telemóvel instala uma vez a partir de
   `http://<ip>:8080/lab-ca.crt`; cobre também o registo SIP) ou `make tunnel` (um túnel
   Pinggy com um URL novo a cada execução, 60 minutos, **publica a borda inteira na
   Internet**, só o QR e a descarga — o UDP do SIP não passa; `make tunnel-stop` fecha). **Medido a 2026-10-04:** o Pinggy
   serve a página HTML de aviso dele a um cliente com User-Agent de navegador (a câmara ou um
   navegador a abrir o URL do QR não recebem a configuração); um cliente que não o pareça passa
   para o servidor.
3. Registar o softphone no Kamailio e "ligar" para o número da sala.
4. Introduzir o PIN → deve entrar na conferência. Confirmar o CDR em
   `GET /api/orgs/{org}/voice/call-records`.

## Ramais internos — chamada ramal-a-ramal (Fase 1, `server/src/ramais.rs`)

Infra-as-code de um segundo fluxo, PARALELO ao dial-in PSTN acima e que não o
toca: um "ramal" é uma conta SIP permanente (1:1 com um `org_member`, número
curto atribuído, migração `0055_ramais.sql`) para chamadas **só entre ramais
da MESMA organização**. O PSTN é a Fase 2 e a entrada numa reunião a Fase 3,
as duas mais abaixo.

```
Softphone A (ramal 101, acme.ramais.delonix.meet)
     │ REGISTER + INVITE 102           DIRECTAMENTE ao FreeSWITCH — o
     ▼                                  Kamailio NÃO entra neste caminho
FreeSWITCH — perfil "internal" (porta DELONIX_RAMAIS_SIP_PORT, default 5070)
     1) REGISTER → mod_xml_curl → POST /internal/v1/voice/ivr/directory  ──► Control plane
                                  (segredo por HTTP Basic — nunca no URL, R227)
                                  (a1-hash do digest SIP)          ◄── XML directory
     2) INVITE 102 → dialplan "delonix_ramais" → ramais_dial.lua
        → POST /internal/v1/voice/ivr/resolve-extension  ──────────────────► Control plane
          (domínio do chamador + "102")                            ◄── sip_username
     3) bridge(user/<sip_username_de_102>@acme.ramais.delonix.meet)
```

Porquê o Kamailio fica de fora: ver o comentário no topo de
`kamailio/kamailio.cfg` — hoje é um SBC puro para o trunk (sem usrloc/
registrar/auth_db/DB), e dar-lhe isso era maior risco do que esta fase pede.

| Caminho | Papel |
|---|---|
| `freeswitch/sip_profiles/internal.xml` | Perfil Sofia dos ramais — porta própria, realm por org (`challenge-realm=auto_from`) |
| `freeswitch/autoload_configs/xml_curl.conf.xml` | Directório dinâmico (REGISTER) — consulta o control plane em vez de um XML estático. O terceiro binding (`delonix_telefonia`) é o dos troncos: gateways e o plano de marcação do contexto `delonix-outbound` (ADR-0009, R291) |
| `freeswitch/autoload_configs/json_cdr.conf.xml` | Registos de chamada (`mod_json_cdr`) entregues ao servidor — custo, duração e qualidade de cada chamada por tronco (R291) |
| `freeswitch/dialplan/default/00_delonix_extensions.xml` | Contexto `delonix_ramais`: tudo o que um ramal marca (3 a 15 dígitos) → `ramais_dial.lua`. Não decide nada nem tem troncos |
| `freeswitch/scripts/ramais_dial.lua` | Pergunta ao control plane o que é o número marcado: outro ramal (AOR registado, e faz o bridge), o número de acesso às reuniões (segue para o IVR, Fase 3), ou um número que sai para a rede pública pelo plano de marcação da organização do ramal autenticado (R292) |

**Domínio SIP e endereço público são duas coisas.** O softphone precisa de
quatro dados, e a consola mostra-os no diálogo «Credenciais SIP» de um ramal:

| Dado | De onde vem | O que é |
|---|---|---|
| Servidor / proxy | `sip_server` — `VOICE_RAMAIS_PUBLIC_HOST`, `VOICE_RAMAIS_PUBLIC_PORT` (omissão `5070`), `VOICE_RAMAIS_PUBLIC_TRANSPORT` (`udp`\|`tcp`\|`tls`, omissão `udp`) | O endereço PÚBLICO a que o softphone se liga, pronto a colar: `sip:host:porta;transport=x` |
| Utilizador | `sip_username` | A conta SIP (`ramal_…`) |
| Password | `sip_password` | Só aparece na criação e na regeneração |
| Domínio | `sip_domain` — `<slug>.<VOICE_RAMAIS_DOMAIN_SUFFIX>` | O realm do digest: um nome LÓGICO, que não tem de resolver em DNS |

- **Sem `VOICE_RAMAIS_PUBLIC_HOST` a API devolve `sip_server: null`** e a
  consola diz que a instalação não tem o endereço configurado. O servidor não
  adivinha por onde é alcançável — um host mal formado (com `sip:` ou com a
  porta lá dentro) conta como ausente, com aviso no arranque.
- **Em produção, `VOICE_RAMAIS_DOMAIN_SUFFIX` deve ser um sufixo do domínio
  público da instalação** (p.ex. `ramais.meet.exemplo.ao`), e não a omissão
  `ramais.delonix.meet`: um softphone que derive o servidor do domínio, ou um
  SRV/NAPTR futuro, só funciona se o nome for da instalação.
- **Não se muda o sufixo com ramais criados.** O domínio entra no HA1 (abaixo):
  mudá-lo invalida as passwords de TODOS os ramais existentes, que têm de ser
  regeneradas uma a uma. Escolhe-se antes do primeiro ramal.
- Os dois laboratórios definem o endereço: `compose.yaml` (`meet.ngolacloud.local`,
  trocado pelo IP da máquina com `make compose-up LAN_IP=…`) e
  `scripts/cluster.sh` (`${MEET_HOST}`). **Não validado:** que a porta 5070 é
  alcançável de fora nesses endereços — no compose só com `LAN_IP`, e no
  cluster o serviço do FreeSWITCH é interno (`clusterIP: None`).

**HA1, não Argon2, para o digest SIP.** `voice_extensions.sip_password_hash`
(Argon2) é só a segurança em repouso da nossa própria base — o protocolo SIP
Digest (RFC 2617) exige `HA1 = MD5(sip_username:domínio:password)`, guardado
à parte (`sip_ha1`) porque nenhum hash genérico serve para validar um desafio
digest. MD5 aqui não é escolha nossa — é o que o protocolo pede.

**O que NÃO foi possível verificar aqui** (sem uma instância FreeSWITCH real):
os nomes exactos dos campos que o `mod_xml_curl` desta versão envia no POST
de directório, e o comportamento de `challenge-realm=auto_from` com um realm
que varia por organização. Ambos ficam documentados nos ficheiros de
configuração respectivos (`xml_curl.conf.xml`, `sip_profiles/internal.xml`) —
antes de produção, activar `debug="true"` e confirmar contra um REGISTER
real. Tudo o resto (modelo de dados, API REST de gestão, geração/regeneração
de credenciais, UI de administração) foi corrido e verificado contra um
Postgres real neste repositório.

### Configurar o Linphone por QR (R278)

Ninguém digita a password SIP. Na consola (cada ramal activo) e em Definições →
Segurança → «O meu ramal» há **«Configurar o Linphone»**: o servidor emite um
bilhete de uso único (10 minutos) e a consola mostra-o num QR. No Linphone:
Assistente → Configuração remota → ler o QR. O aparelho descarrega de
`https://<origem pública>/api/public/extension-provisioning/<bilhete>` um XML
`lpconfig` com a conta (utilizador, domínio/realm, o proxy público
`sip:host:porta;transport=…` como registo e rota, e SRTP obrigatório).

- **Ler o QR troca a password SIP do ramal.** O aparelho que estava registado
  com a anterior deixa de registar. O bilhete serve uma vez; um segundo pedido
  ao mesmo URL recebe `404`.
- **Lê-se só com o Linphone.** O resgate é um `GET`: a câmara do telemóvel, um
  leitor de QR genérico ou uma pré-visualização de link abrem o endereço,
  gastam o bilhete e trocam a password na mesma. Por isso a consola não mostra
  o URL em texto (só se não conseguir desenhar o QR).
- **O bilhete vai no caminho do URL.** Os três nginx do repositório não o
  registam (`access_log off` em `/api/public/extension-provisioning/`); um
  proxy ou ingress à frente que não seja nosso regista, se ninguém lho disser.
- **A password vai em claro no XML**, sobre `https`. E com o transporte por
  omissão (`udp`) as chaves SRTP (SDES) seguem em claro na sinalização SIP:
  `VOICE_RAMAIS_PUBLIC_TRANSPORT=tls` é o que fecha isso.
- **Precisa de duas coisas da instalação:** `VOICE_RAMAIS_PUBLIC_HOST` (onde o
  aparelho regista) e a primeira origem de `CORS_ORIGINS` em `https` e pública
  (de onde descarrega). Sem uma delas a emissão é recusada com `422` — o
  servidor não emite um QR que não leva a lado nenhum.
- O diálogo «Credenciais SIP» (regenerar a password e copiar os quatro dados)
  continua a existir como caminho de recurso, para softphones que não lêem QR.
- **Não validado:** nenhum Linphone real leu um destes QR, e o formato do XML
  não foi verificado contra um aparelho. No laboratório do compose o certificado
  é auto-assinado e o nome é `meet.ngolacloud.local`: um telemóvel só o
  descarrega se resolver esse nome e confiar no certificado.

**Ramal automático a quem entra.** Com «Atribuir ramal automaticamente a quem
entra» ligado na consola (ao lado do intervalo; desligado por omissão), cada
pessoa que entra na organização **por um acto de um administrador ou de um
IdP** (colaborador junto pelo administrador, convite aceite, SSO, Odoo,
reactivação) recebe o primeiro número livre do intervalo. Convidados externos
não recebem; quem se regista sozinho e o convidado de uma reunião criada pela
API v1 também não — o registo não verifica o email. Se o intervalo se esgotar,
a pessoa entra sem ramal e fica um registo na auditoria.

**Aberto:** quem sai da organização continua a registar com a password antiga —
arquivar um membro não desactiva o ramal dele (R278).

## Ramal alcançável do PSTN — DID dedicado (Fase 2, `server/src/ramais.rs`)

Estende o fluxo acima: um ramal pode receber um DID (migração
`0056_ramais_did.sql`, estende `voice_did`) e passa a ser alcançável
DIRECTAMENTE do PSTN — quem ligar para esse número cai no ramal, sem PIN e
sem IVR. Não toca em `voice_room`/PIN/dial-in nem na ponte para uma sala de
reunião em vídeo (fase seguinte, continua por fazer).

```
Telefone → SIP Trunk → Kamailio → FreeSWITCH (contexto "public")
                                       │
                                       ▼  mod_xml_curl, secção "dialplan"
                        POST /internal/v1/voice/ivr/dialplan-did ───► Control plane
                        (X-Voice-Secret, número discado)   ◄── XML dialplan
                                       │
                          número é DID de ramal?
                     sim → bridge directo ao ramal (SEM PIN)
                     não → "not found" → cai no dialplan estático
                           (00_delonix_dialin.xml, dial-in por PIN de sempre)
```

Reutiliza a MESMA ligação `mod_xml_curl` da Fase 1 (`xml_curl.conf.xml`) —
uma segunda `<binding>`, secção "dialplan" em vez de "directory" — em vez de
um mecanismo paralelo. `POST /api/orgs/{org}/extensions/{id}/did` (PUT/DELETE,
admin) atribui/desatribui o DID; só um DID dedicado à MESMA org do ramal pode
ser atribuído (um número do pool partilhado fica disponível para todas as
orgs, atribuí-lo a um ramal quebraria isso para as outras), e fica bloqueado
enquanto uma `voice_room` activa o estiver a usar.

**O que NÃO foi possível verificar aqui** (sem uma instância FreeSWITCH real,
mesma ressalva da Fase 1 acima, agora também para a secção "dialplan"): os
nomes exactos dos campos do POST (`ramais.rs::DIALPLAN_DESTINATION_KEYS`
aceita vários candidatos por não ter confirmação), e — mais importante — a
PRECEDÊNCIA real entre esta resposta dinâmica e o dialplan estático já
carregado de `dialplan/public/*.xml`: o pressuposto é que o `mod_xml_curl` é
consultado por chamada e "not found" cai no estático (o mesmo padrão que a
Fase 1 já assume, sem instância real, para a secção "directory"). Se isso não
se confirmar, o sintoma esperado é inofensivo — um DID atribuído a um ramal
continua simplesmente a pedir PIN como antes — nunca uma chamada perdida,
porque a via antiga (`00_delonix_dialin.xml`) não foi tocada. Confirmar com
`debug="true"` em `xml_curl.conf.xml` antes de produção. O modelo de dados,
a API REST de atribuição e a UI foram corridos e verificados contra um
Postgres real neste repositório.

## Ramal entra numa reunião — número de acesso (Fase 3, R273)

Um ramal registado marca o **número de acesso às reuniões** (`8000` por
omissão; `VOICE_MEETING_ACCESS_NUMBER` no servidor, 3–5 dígitos), ouve o pedido
de PIN e entra na sala pela ponte telefone↔sala abaixo — a mesma do dial-in.

```
Softphone (ramal 101, acme.ramais.delonix.meet)
     │ INVITE 8000 (autenticado por digest no perfil "internal")
     ▼
FreeSWITCH — dialplan "delonix_ramais" → ramais_dial.lua
     1) POST /internal/v1/voice/ivr/resolve-extension ("8000") ──► Control plane
                                                        ◄── {"meeting_access": true}
     2) dialin_ivr.lua ramal → atende, pede o PIN
     3) POST /internal/v1/voice/ivr/validate-extension ──► Control plane
        {sip_username, domain, pin}                        (sala ACTIVA da ORG do ramal)
                                                        ◄── room_code + room_bridge
     4) bridge para o room_bridge; se falhar, conferência local
```

- **O número só se configura no servidor.** Nem o dialplan nem o Lua o têm
  escrito: perguntam. Nenhum ramal o pode ter (`409 ramais.extension_reserved`)
  e a consola recebe-o em `meeting_access_number`, em cada ramal de
  `GET /api/orgs/{org}/extensions`.
- **Isolamento por organização.** A sala procura-se na org do ramal que o
  FreeSWITCH autenticou (`sip_auth_username`/`sip_auth_realm`), não na de um
  cabeçalho que o telefone escreva. O PIN de uma sala de outra org é recusado.
- **Sem CDR** para esta chamada, e quem entra aparece na sala como «Telefone».

**O que NÃO foi verificado:** nenhuma chamada real percorreu este caminho. A
regra do servidor está medida contra Postgres (`server/tests/ramal_entra_na_sala.rs`);
os dois Lua só têm a sintaxe verificada. Antes de produção, confirmar contra um
FreeSWITCH real que as duas variáveis `sip_auth_*` vêm preenchidas e que o
`ramais_dial.lua` consegue chamar o `dialin_ivr.lua` com o argumento `ramal`.

## Ponte telefone↔sala (ADR-0010) — ligada

> **No compose a ponte está LIGADA**; no cluster local e no Helm continua desligada (o chart
> pede IPs exactos, ver o «Achado» em `deploy/helm/delonix-meet/README.md`). `PHONE_BRIDGE_FREESWITCH_IPS`
> aceita IPs **e nomes**: o servidor resolve os nomes (só endereços privados, nunca loopback) a
> cada `PHONE_BRIDGE_RESOLVE_SECS` e, se o nome deixar de resolver durante 3 ciclos seguidos, a
> lista cai para os IPs literais — um IP que ficou livre não herda o acesso. O IP de RTP, se não
> for configurado, deduz-se da rota para o FreeSWITCH.
> **Modelo de confiança:** a lista de origens é a única autenticação do socket SIP da ponte;
> por nome, a âncora de confiança passa a ser o DNS (em produção, IPs literais ou um nome de um
> DNS interno de confiança; prefere o nome absoluto com ponto final, que não passa pelos
> domínios de pesquisa). **Limite conhecido:** na rede do compose, um contentor com `NET_RAW`
> (o `pbx` não confiado é um) pode forjar a origem do FreeSWITCH; vale também para IPs literais.
> **Event Socket:** com `TELEPHONY_ESL_PASSWORD` no ambiente do FreeSWITCH, o ESL escuta em
> todas as interfaces com a ACL `delonix_esl` (só redes privadas, `default=deny`) e essa password; sem
> ela fica em loopback com password aleatória. O servidor liga-se com `TELEPHONY_ESL_ADDR`. O ESL
> origina chamadas: nunca se publica no host, e a password vem do segredo (`.env` / Secret
> `delonix-voice`; `make esl-secret-rotate` troca-a). **A ACL não separa nada nestes laboratórios:**
> o compose e a rede de pods são redes privadas, e o `pbx` não confiado (compose) e qualquer pod
> (cluster) passam por ela — a password é a única barreira, em claro (o ESL não tem TLS) e visível no
> argv do `fs_cli`. O ESL dá `originate` e `api system`. **`DELONIX_ESL_CIDRS`** (opcional) estreita a
> lista às redes de onde o servidor fala — com ela, quem não é o servidor nem com a password certa
> entra (medido, R300; a máscara tem de ser válida e diferente de zero); o compose e o `make cluster` não a usam, porque os contentores não têm
> endereço fixo. **No chart do Helm** o FreeSWITCH recebe a password do Secret com
> `server.telephony.eslAddr`, aceita `voice.freeswitch.eslCidrs`, e com `networkPolicy.enabled` o
> `:8021` fica só para os pods do servidor — conferido no render, por aplicar num cluster; em `hostNetwork` a política não se aplica e o chart exige `eslCidrs`. É pelo
> ESL que um tronco criado, alterado ou apagado chega ao FreeSWITCH na hora (R300). Por fechar antes
> de produção: Kamailio sem o `.env` inteiro, e `fs_cli` sem `-p`.
> A ponte está medida contra um FreeSWITCH real em `sfu_e2e`, não nestes laboratórios.

Quem entra por telefone é um **participante da sala**: fala e ouve os
participantes WebRTC. O caminho é o que o FreeSWITCH de stock sabe percorrer:
o IVR valida o PIN, o control plane devolve `room_bridge` (para onde fazer
`bridge` e que variáveis de canal pôr antes), e o `dialin_ivr.lua` executa esse
`bridge`. Do outro lado atende o UA SIP da ponte (`server/src/phone_bridge/`),
que negoceia SDES-SRTP no SDP, transcodifica G.711↔Opus e publica o chamador
no SFU, devolvendo-lhe a mistura menos a própria voz.

**Fail-closed em três sítios:** sem `PHONE_BRIDGE_SIP_BIND` o UA não arranca;
com `PHONE_BRIDGE_FREESWITCH_IPS` vazio também não; e uma oferta sem
`a=crypto` leva `488` — media da reunião nunca vai em claro. Em qualquer
falha, incluindo o `bridge` não completar, o IVR **cai na conferência local**
do FreeSWITCH, que é o comportamento de sempre: um chamador nunca fica de fora
por causa da ponte.

A **Abordagem B** anterior (`pstn_bridge.rs`: RTP cru num par UDP com chaves
entregues por fora, no JSON do IVR) foi **abandonada** — assentava num
mecanismo que o FreeSWITCH 1.11.3 não tem. O historial está em
`docs/pstn-sfu-bridge-design.md` (marcado superseded) e a decisão em
`docs/adr/0010-ponte-telefone-sala.md`. R221 e R222 no catálogo de regressões.

**Medido contra um FreeSWITCH real** (`scripts/fs-canais.sh up`, imagem
`delonix-meet/freeswitch:1.11.3`, de `voice/freeswitch/image/`), não contra um duplo:
`sfu_e2e::ponte_com_freeswitch_real_tom_nos_dois_sentidos`. **Por medir:** a
cadeia com uma operadora a sério e um softphone através do Kamailio, e um
browser em vez do cliente webrtc-rs.

## Produção (microVM + Cilium)
- Kamailio e cada FreeSWITCH em **microVM dedicada** (isolamento de jitter — não
  multiplexar no runtime das apps web).
- CiliumNetworkPolicy: SIP (5060/5061) e RTP (faixa dinâmica) só entre trunk↔Kamailio↔FreeSWITCH.
- Escalar 300+ canais: acrescentar nós ao `dispatcher.list`; o Kamailio balanceia.
