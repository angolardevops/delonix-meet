# Plano para fechar as lacunas — enterprise, operadoras e estúdio de TV

**Data:** 2026-10-04 · **Ramo medido:** `origin/develop` · **Cabeças:** `45c82940` (TV, enterprise,
deploy, frontend) e `974edaae` (telefonia, depois da fusão do #181) · **Método:** leitura do git
(`git show`, `git grep`) por cinco revisões paralelas, cada uma com ordem para **refutar** a lista
de lacunas; os achados de maior impacto foram reconfirmados à mão.

**Nada foi executado.** Nenhum teste, build, browser, chamada ou cluster: o host estava com carga
18 e o laboratório parado. Cada linha deste plano é «o código diz», não «corri e obtive». O que
pede medição diz-o na coluna da prova.

Este documento **não substitui** o [plano de produção de 2026-10-03](plano-producao-2026-10-03.md):
re-mede-o (a coluna «≙» aponta o item de lá) e junta-lhe o que ele não cobria — o que um comprador
enterprise e uma operadora pedem antes de assinar. «Ausente» só aparece onde ficou um grep
registado com mais de uma variante de nome.

## 0. Estado da onda 0 (2026-10-04, ao fim do dia)

A onda 0 foi executada no mesmo dia do plano. Cada item tem a sua entrada no
catálogo de regressões, com o que ficou provado e o que não.

| Item | O que ficou | PR | Regressão |
|---|---|---|---|
| V1 | CI em push na `develop`; num ramo de integração uma corrida já não cancela a anterior | #190 | — |
| V2 | `check-capability-claims.sh` lê os 100 ficheiros das quatro línguas e falha se não ler nenhum | #190 | — |
| V3 | protocolos de luz fora do ecrã. **Aberto:** a bandeira `iso_recording` e o `kind: "minio"` de `studio.rs` (contrato do estúdio; dependem da D3) | #190 | — |
| V4 | documentos corrigidos nos dois sentidos | #190 | — |
| V5 | **aberto — decisão do dono** (D9): oito ADR continuam «Proposto» com código fundido (0005, 0007, 0009, 0011, 0013, 0014, 0015, 0016) | — | — |
| S1 | o Secret da aplicação sai do repositório e nasce do `.env`; o servidor recusa os três valores publicados | #195 | R284 |
| S2 | mudar a password pede prova de identidade e termina as outras sessões | #193 | R281 |
| S3 | `sip_ha1` cifrado em repouso; as três rotas de máquina dos ramais no listener interno | #195 | R286 |
| S4 | `X-Forwarded-For` lido da direita, com `TRUSTED_PROXY_HOPS`; tecto e uma espera por identidade na sala de espera | #193 | R282, R283 |
| S5 | `DATA_ENCRYPTION_KEYS` obrigatória em produção, e presente em todos os caminhos que arrancam o servidor | #195 | R285 |
| S6 | a chave de emissão vai na primeira trama do `/live`, não no URL; a palavra-passe de um link de partilha vai no corpo, com passe de leitura e travão | #199 | R287, R288 |
| S7 | o destino de um directo é revalidado antes de cada reinício do `ffmpeg`. **Aberto:** a janela de DNS dentro de um arranque — a defesa completa é de rede e depende do Channel Engine | #199 | R289 |

**O que a onda 0 não validou.** Nada correu com o laboratório de pé: nem um
browser, nem o `make compose-voice-check`, nem um cluster instalado pelos caminhos
que mudaram. O que correu foi a bateria de testes contra Postgres real, os
portões, e — para o S3 — um FreeSWITCH 1.11.3 real com um servidor de andaime
(`scripts/softphone-prova.sh srtp-real`).

**O que quem opera tem de fazer depois de actualizar** (está em cada PR e em
`docs/deployment.md` §6):

1. `make bootstrap` antes do próximo `make compose-up` ou `make cluster`:
   acrescenta `DATA_ENCRYPTION_KEYS` ao `.env`. Sem ela o servidor não arranca.
2. Um cluster instalado com o `deploy/k8s/01-config.yaml` antigo tem de ser rodado:
   a imagem nova não arranca com o `JWT_SECRET` publicado.
3. O FreeSWITCH e o servidor sobem juntos: a configuração antiga pede o directório
   dos ramais num caminho que já não existe.
4. Uma instalação com dois proxies que acrescentam ao `X-Forwarded-For` precisa de
   `TRUSTED_PROXY_HOPS=2`.

## 1. O que a validação mudou

### Estava dado como feito, e não está

| O que se dizia | O que o código mostra |
|---|---|
| «Convidado sem conta: feito» | Só o servidor e o cliente da API. `guestJoin` (`web/src/api.ts:2661`) não tem chamador fora dos testes; quem não tem sessão vai para o Login (`web/src/App.tsx:128-137`). **Continua a ser o bloqueio nº 1 de adopção.** |
| «Troncos, plano de marcação e CDR na develop» | O código Rust está, mas **a configuração que o compose, o cluster e o chart distribuem não o liga**: `voice/freeswitch/autoload_configs/xml_curl.conf.xml:50-78` só tem as duas bindings dos ramais; a binding de `/internal/v1/telephony/freeswitch-config` e o `json_cdr.conf.xml` só existem em `voice/freeswitch/telefonia-prova/`. Um ramal não sai para a PSTN, e o 112 marcado de um ramal não tem caminho (grep `112\|emerg\|resolve_number` em `ramais.rs` e `ramais_dial.lua` = 0; lido, não executado). |
| «Backend do estúdio de TV feito» | Dez rotas `/api/orgs/{org_id}/studios…` sem um único consumidor em `web/src` (grep = 0), e o tempo real do estúdio sem emissor no cliente. |
| «Emissão que sobrevive à queda da rede (ADR-0013)» | Ligado está só o supervisor por destino. `live_sessions` existe apenas como migração `0074`; nenhum código a lê ou escreve. |
| «O portão de capacidades protege o que se anuncia» | `scripts/check-capability-claims.sh:48` procura `web/src/locales/*.ts`; os ficheiros estão um nível abaixo (`pt/`, `en/`, `fr/`, `zh/`). Passa sem ler nada. |
| «MLS no roadmap com código» | `server/src/mls.rs` é desenho com `#![allow(dead_code)]`, sem rota e sem cliente. |

### Estava dado como em falta, e existe

| O que se dizia | O que o código mostra |
|---|---|
| «Provisionamento só pelo Odoo» | Há conta criada à primeira entrada por OIDC (`auth.rs:1360-1372`), importação por CSV e convites com token (`directory.rs:8-11`). Falta a **entrega**: `delivery_channel` é sempre `manual`. |
| «Sem Q&A moderado» | Existe (`room_tools.rs:140-185`), em memória e Redis. Falta ao webinar o papel só-ver e o registo. |
| «Sem `OPTIONS` de vida» | Existe no dispatcher (`kamailio.cfg:85-86`) e nos gateways (`telephony_fs_xml.rs:209`). |
| «Sem limitador no directo» | Há limitador **por canal** (`mesaDeSom.ts:407-412`). Não há no mestre. |
| «Sem legendas no directo» | Há legendas **queimadas** na imagem (`useLegendas.ts` → `compositor.ts:254,574`). Não há legendas fechadas. |
| «DLP é roadmap» | `dlp.rs` está ligado em nove sítios. É estreito: três expressões fixas e máscara de palavrões, sem política por organização nem registo. |
| «Sem retoma de sessão» (o grep procurou `reconnect_token`) | Existe desde a R91 com outro nome: `reconnect_secret` e `SignalingHub::reclaim` (`signaling.rs:1918`) devolvem o lugar e o papel a quem cai e volta dentro de `RECONNECT_GRACE_SECS`, sem passar outra vez pela sala de espera. O que falta é o lugar sobreviver à morte do pod. **Corrigido a 2026-10-04, depois de outra sessão o apontar.** |
| «Ramos por unificar» | Unificados em conteúdo pelo #174. Migrações `0001`–`0091` sem buracos nem repetidos. |
| «Sem chart Helm» | Existe (`deploy/helm/delonix-meet`, #156) e recusa produção sem Secret. Só foi instalado no perfil local. |

### Em curso noutras sessões — não duplicar

| PR / ramo | O que resolve | Linhas deste plano que toca |
|---|---|---|
| #184 `integra/caller-controls` | controlos da conferência | nenhuma |
| #185 `integra/nota-usb-telemovel` | texto do cartão de câmaras USB | nenhuma |
| #186 `integra/tv-d3-gpl-develop` | D3: ffmpeg GPL só no Channel Engine, servidor LGPL (ADR-0015) | fecha a decisão D3; a #170 contra a `main` fica obsoleta |
| #187 `integra/central-autenticada` | a central de uma organização autentica-se no bordo (ADR-0016) | T10, e parte do T9 (falhas de digest) |
| #188 `integra/carga-reavaliacao` | reavaliação dos defeitos do SFU sob carga | X2 (documenta; não repete em hardware dedicado) |
| #191 `voz/qr-linphone-e-ramal-ao-entrar` | QR do Linphone e ramal automático | item 3.8 do plano de produção |
| ramo local `delonix-meet-backend/lugar-em-redis` | o lugar reservado sobrevive à morte do pod (cópia no Redis) | X1 |
| ramo local `meet-arch/tv-canal-live-sessions` | liga as `live_sessions` a um canal de TV (migração e ADR-0015) | D3, TV4 |

## 2. Decisões que só o dono toma

Cada uma trava itens abaixo. Sem elas, a ordem das ondas 2 a 4 é hipótese.

| # | Decisão | Trava |
|---|---|---|
| D1 | Caminho de produção: chart Helm directo, ou pelo PaaS (`deploy/delonix/`). O `deploy/ansible` deste repo instala kubeadm, Calico e MetalLB — substrato, contra a regra do workspace | O1, O4 |
| D2 | Armazenamento das gravações: volume partilhado (RWX) ou objectos (S3/MinIO) | O5, E8 |
| D3 | Um só modelo de TV. Coexistem `studios` + `live_sessions` (ADR-0013/0014) e `tv_channels` + Channel Engine (ADR-0015), todos «Proposto» | TV1, TV4, TV11–TV13 |
| D4 | SRTP à entrada por tronco. Hoje é global e obrigatório: uma operadora que só entregue RTP em claro não nos consegue ligar | T4, T5 |
| D5 | Âmbito de PBX no Meet. Transferência, espera, voicemail, grupos e filas não existem; a central vendida é a appliance FreePBX (`PbxService`) | T19 |
| D6 | Identidade: SAML além do OIDC, ou só OIDC com SCIM | E4, E5 |
| D7 | Fornecedor de correio (SMTP próprio do cliente, relay, ou ambos) | E2, E3, E7 |
| D8 | A quem se vende o Estúdio de TV. Sem um cliente de televisão nomeado, a onda 4 ordena-se por suposição | onda 4 |
| D9 | Estado dos ADR com código fundido e ainda «Proposto»: 0009, 0011, 0013, 0014, 0015 | V5 |
| D10 | App móvel nativa, ou PWA com Web Push | E17, X6 |
| D11 | Gravação de chamadas de voz para conformidade, e o que o regulador exige (SIPREC, intercepção) — verificação jurídica do cliente, não afirmação nossa | T17 |

## 3. Como ler

- **≙** item equivalente no plano de produção de 2026-10-03.
- **Tam.:** P (um a dois dias), M (até uma semana), G (mais de uma semana). Ordem de grandeza: a
  capacidade da equipa não foi medida.
- **Prova:** o que tem de ficar medido para o item fechar. «Está implementado» não fecha nada.
- A onda 0 vem primeiro. As ondas 1 a 4 são **frentes paralelas** por área, cada uma com a sua
  ordem interna; a onda 5 depende das outras.
- Revisor de cada frente: `delonix-meet-devops` (onda 1), `delonix-meet-security` e
  `delonix-meet-api` (onda 2), skills `delonix-meet-telefonia` e `delonix-meet-voip` (onda 3),
  `delonix-meet-webrtc` e `delonix-meet-frontend` (onda 4), `delonix-meet-rust` (onda 5).

---

## Onda 0 — Verdade, portões e o que sangra

Objectivo: o que o repo afirma é o que o código faz, e nenhum caminho aplica um segredo público.

| # | Item | Evidência | Prova | ≙ | Tam. |
|---|---|---|---|---|---|
| V1 | O CI corre em push na `develop`, e na `main` uma corrida não cancela a anterior | `.github/workflows/ci.yml:15-23`; a cabeça da develop não tem corrida; três commits da `main` ficaram com a corrida cancelada | corrida verde listada para a cabeça da develop | 0.3 | P |
| V2 | Reparar `check-capability-claims.sh` | `:48` glob vazio, `:67` engole o erro | controlo negativo: uma capacidade inventada num ficheiro de língua faz o portão falhar | 0.4 | P |
| V3 | Tirar do ecrã e da API o que não tem código | DMX, Art-Net e Philips Hue em `Iluminacao.tsx:120,125`; bandeira `iso_recording` (`0081`, `studio.rs:125`); `kind:"minio"` constante (`studio.rs:1338-1345`) | V2 verde com estes termos na lista | 6.11 | P |
| V4 | Corrigir os documentos nos dois sentidos | `competitive-positioning.md:117,120` (convidado, rotas SSO, DLP); `adopcao-vs-concorrencia.md` §3.6 (S5, S6); skill de telefonia `:306-311` (os tipos de dial-out existem); skill voip `:81` (o bordo também escuta 5060); `EntrarComCodigo.tsx:2-3`; `docs/deployment.md:152` (`SECRETS_KEY` não existe no servidor) | cada frase com o ficheiro e a linha que a sustenta | Contínuo | P |
| V5 | Fixar o estado dos ADR (D9); dois `0004` e nenhum `0012` em `docs/adr/` | cabeçalhos dos ADR | estado e data actualizados | 3.12 | P |
| S1 | Segredos literais fora de `deploy/k8s/01-config.yaml` e dos `helm-values`; o servidor recusa os valores publicados | `01-config.yaml:41-52`; `config.rs:788-812` só recusa o default de dev | servidor arrancado com o valor publicado sai com erro; a higiene falha se voltar | 0.1 | P |
| S2 | Mudar a password exige a actual (ou reautenticação recente) e termina as outras sessões | `users.rs:104-146` | sem reautenticação dá 403; o token da outra sessão dá 401 | 0.5 | P |
| S3 | `sip_ha1` dos ramais selado em repouso; as três rotas de máquina dos ramais no listener interno | `ramais.rs:432-444,602-610`; `lib.rs:1044-1050` | base sem HA1 em claro; as rotas não respondem no listener público | 0.6 | M |
| S4 | Limite por IP que o cliente não falsifica; tecto e deduplicação na sala de espera | `rate_limit.rs:120-135` usa o primeiro valor do `X-Forwarded-For`; `signaling.rs:2014` insere sem limite | e2e com cabeçalho forjado; mil ligações com um token não enchem a lista | 2.10 | P |
| S5 | O servidor exige `DATA_ENCRYPTION_KEYS` em produção (hoje só o chart) | `config.rs:416-425` | sem a chave recusa arrancar | 2.4 | P |
| S6 | Segredos fora da query string: chaves RTMP e token de sala no WebSocket do directo; palavra-passe do link de partilha | `broadcast.rs:1367-1385,1418`; `SharePage.tsx:59` | captura de proxy sem o segredo no URL | B8 | M |
| S7 | DNS rebinding no destino RTMP: o `ffmpeg` resolve o nome outra vez | `net_guard.rs:146-150` | nome que muda para IP privado entre a validação e a ligação é recusado | B2 | P |

## Onda 1 — Instalar e operar (frente de produção)

Objectivo: um pacote que se instala num cluster limpo, aguenta perder um pod e avisa alguém.

| # | Item | Evidência | Prova | ≙ | Tam. |
|---|---|---|---|---|---|
| O1 | ADR do caminho de produção (D1) e retirar o que o contradiz: `make prod` aponta para uma base, um serviço e uma imagem que ele próprio não instala (lido, não executado) | `Makefile:547-561`; `02-server.yaml:44` | ADR aceite; um só caminho documentado | 1.6 | P |
| O2 | O CI constrói, publica por digest e assina o servidor e a web; `Dockerfile.server.slim` ganha ffmpeg ou sai | seis jobs em `ci.yml`, nenhum constrói imagem; `Makefile:318` | imagem com o SHA do commit no registo, assinatura verificável; e2e de gravação contra ela | 1.2, 1.3, 1.7 | M |
| O3 | Chart: passar `DELONIX_SIP_ADVERTISE` e `DELONIX_SIP_IFACE` ao Kamailio (a R277 não chega à produção); CPU do servidor a 4 como o #177 mediu; WebAuthn derivado do host; regra de host `.local` e IP privado; etiqueta do FreeSWITCH na NetworkPolicy dos overlays | `templates/voice.yaml`; `values-production.yaml:42-44`; `values.yaml:148`; `networkpolicy.yaml:44-51` | `check-helm.sh` com um controlo negativo por regra | 1.4, 1.5 | M |
| O4 | Instalar o chart em perfil de produção num cluster limpo, com duas ou mais réplicas | nunca feito (`README.md:279-281` do chart) | pods prontos; reunião que sobrevive ao drain de um nó; afinidade por sala medida | 1.1 | G |
| O5 | Armazenamento das gravações com várias réplicas (D2) | base com 3 réplicas e volume RWO (`02-server.yaml:9,133`) | três réplicas em nós diferentes gravam e servem a mesma gravação | 2.1 | G |
| O6 | Backup agendado do Postgres e restauro testado; o único `pg_dump` que existe aponta para a base com o nome errado e engole o erro | `Makefile:570-574` | restauro para uma base limpa, com a aplicação a arrancar sobre ela | 2.2 | M |
| O7 | Uma só definição de Postgres e de Redis (há cinco); Sentinel coerente com o que o servidor fala; Redis com autenticação | `redis-values.yaml:2-5`; grep `sentinel` em `server/` só dá comentários | failover do Redis sem perder presença | 2.3 | M |
| O8 | `/ready` mede Postgres e Redis | `lib.rs:1920-1929` só lê a flag de drain | pod sem Postgres sai da rotação | 2.6 | P |
| O9 | Alertas e SLO como regras; ServiceMonitor; dashboards | grep `PrometheusRule\|ServiceMonitor` em `deploy/` = 0 | um alerta dispara num ensaio | 2.7 | M |
| O10 | TURN por TCP e TLS na 443, mais de uma réplica, credencial renovada antes de expirar | `coturn.yaml:76` `--no-tcp-relay --no-tls`; credencial de 3600 s pedida uma vez (`useCallSession.ts:135`) | reunião numa rede com UDP bloqueado; reconexão depois de uma hora | 2.8 | M |
| O11 | Migrações por Job em todas as edições e procedimento de recuo de release | `deploy/contexts/README.md:46` | recuo de uma versão num cluster de ensaio | 2.5 | M |
| O12 | Retenção com valor por omissão definido (hoje 0 = nunca apaga) e teste do varredor, que só apaga o `.webm` e a miniatura | `0009…sql:4`; `recorder.rs:1184-1190` | gravação fora do prazo desaparece com todos os artefactos | 2.9 | P |
| O13 | Rastos distribuídos (OpenTelemetry) e logs JSON também na base | grep `opentelemetry\|otlp` = 0 | um pedido seguido do ingress à base num rasto | — | M |
| F1 | O build deixa de embutir tudo em `data:` (choca com a CSP de produção) | `web/vite.config.ts:134`; `nginx-delonix.conf:72` | fontes e wasm em `dist/assets`; supressão de ruído activa com a CSP de produção | 4.1 | P |
| F2 | *Error boundary* por rota | grep = 0 | um ficheiro em falta mostra o erro, não o ecrã branco | 4.2 | P |
| F3 | Gravações duráveis na sala e o Program do Estúdio em escrita incremental | `media.ts:1054-1055`; `compositor.ts:997-1034` | e2e que corta a rede a meio e recarrega | 4.3 | M |
| F4 | Mudar de língua não destrói o compositor nem pára o directo | `Studio.tsx:173-195` (deps `[t]`); `Lobby.tsx:132-136` | e2e: mudar de língua a meio de uma emissão | 4.5 | P |
| F5 | Sair com prazo; falha da supressão de ruído visível; fila de envio sem salas duplicadas e com apagar | `useCallSession.ts:452-460`; `useLocalMedia.ts:123-125`; `Studio.tsx:467,513` | cada um com a rede parada | 4.4, 4.6, 4.7 | M |
| F6 | Confirmação nas acções destrutivas pelo diálogo do kit; diálogos que prendem o foco; título por rota; teclado nos menus; auditoria automática de acessibilidade no CI | `ExtensionsCard.tsx:98-172`; `kit.tsx:378-394`; nenhum `axe` em `web/package.json` | teste de teclado e leitor de ecrã; axe no CI | 4.8, 4.9 | M |
| F7 | Erros traduzidos pelo código, não pelo texto do servidor (95 chamadas) | `api.ts:97-105` | falha de rede em inglês, francês e chinês | 4.10 | M |
| F8 | ESLint com as regras dos hooks no CI e `StrictMode` em desenvolvimento | sem eslint em `web/package.json` | o lint corre no CI | 4.11 | P |
| F9 | Service worker: não apagar caches alheias, actualização com aviso, precache do que o Estúdio importa dinamicamente | `web/public/sw.js:40,47`; `vite.config.ts:63` | deploy com uma sessão aberta, sem ecrã branco | 7.1, 7.2 | M |

## Onda 2 — Adopção enterprise

Objectivo: passar um questionário de segurança e um RFP de banco, governo ou operadora.

| # | Item | Evidência | Prova | Tam. |
|---|---|---|---|---|
| E1 | **Ecrã de convidado sem conta**: link, nome, sala de espera, sala | `guestJoin` sem chamador; `App.tsx:128-137` | e2e sem conta, do link à sala, em menos de 60 s | M |
| E2 | Correio (D7): entrega dos convites, lembretes e alteração de hora | grep `smtp\|lettre` em `server/` = só um comentário (`directory.rs:479`) | convite recebido numa caixa de ensaio; SSRF do relay guardado | M |
| E3 | Reposição de password (pela própria pessoa e pelo administrador) e política de password além do comprimento | únicos `UPDATE … password_hash`: `users.rs:140`, `auth.rs:731`, `odoo_sso.rs:879` | fluxo completo com token de uso único e auditoria | M |
| E4 | SAML 2.0 (D6) | grep `saml` = 0 | entrada por um IdP de ensaio, com asserção assinada e a recusa da não assinada | G |
| E5 | SCIM 2.0: utilizadores, grupos e **desprovisionamento** | grep `scim` = 0 | suspender no IdP termina as sessões no Meet | M |
| E6 | OIDC completo: grupos do IdP para papéis, logout único, prova de posse do domínio de email por DNS | grep `groups\|backchannel\|domain_verif` = 0 | cada um com o seu controlo negativo | M |
| E7 | Calendário: `.ics` com participantes e actualização por correio; depois sincronização com Google e Microsoft | hoje um `VEVENT` `METHOD:PUBLISH` sem participantes (`meetings.rs:1393`) | reunião marcada aparece e actualiza-se na agenda do convidado | G |
| E8 | Gravações em objectos (D2), cifra em repouso e chave gerida pelo cliente | `storage.rs:162` só local, NFS e WebDAV; sem cliente S3 no `Cargo.toml` | gravação servida de um MinIO de ensaio; ficheiro ilegível sem a chave | G |
| E9 | Ciclo de vida dos dados: apagar uma gravação (não há rota nem ecrã), apagar a conta, *legal hold*, exportação por organização | grep `legal.?hold\|delete_account` = 0; `data_exports.rs` só cobre a própria pessoa | cada operação com auditoria e teste entre organizações | M |
| E10 | Auditoria completa: moderação em sala, início, fim e apagar de gravação, uso da IA; exportação para SIEM | `audit::` = 0 em `signaling.rs`, `recorder.rs`, `ai.rs` | evento na cadeia para cada acção; exportação lida por um colector de ensaio | M |
| E11 | Políticas por organização: «IA só local», «nunca Web Speech», restrição por IP, DLP configurável com registo de ocorrências | `useTranscription.ts:168` arranca Web Speech no Chrome; grep `ip_allow\|ai_local_only` = 0 | captura de rede sem saída para fora do cluster numa reunião com legendas | M |
| E12 | RLS em todas as tabelas com `org_id` (hoje uma) e portão que não salte sem cluster | `0024_rls_employee_groups.sql`; `check-tenant-rls.sh:19-20` | portão vermelho ao retirar uma política | G |
| E13 | Impor, ou retirar do catálogo, as nove capacidades de papel sem imposição | `authorization.rs:68,744-746` | teste por capacidade | M |
| E14 | Relatório de presenças: entrada, saída, duração, convidados e telefones, exportável | `room_participants` só tem `joined_at` (`0004:2-7`) | relatório de uma reunião de ensaio igual ao observado | M |
| E15 | Webinar: papel só-ver, painelistas, registo de participantes, Q&A persistido | grep `webinar\|recvonly\|panelist` = 0 | audiência que não publica media; relatório de registos | G |
| E16 | Marca por organização (logótipo, cores, domínio) — hoje é por browser | `web/src/branding.ts:3-4` | duas organizações com marcas diferentes no mesmo servidor | M |
| E17 | Notificações com a aplicação fechada (D10) | grep `PushManager\|vapid\|fcm` = 0 | chamada recebida com o separador fechado | M |
| E18 | Webhooks: id de evento estável entre tentativas e carimbo de tempo na assinatura | `webhooks.rs:163-168,416` | receptor de ensaio que deduplica e recusa repetição antiga | P |
| E19 | Paridade da sala: premir para falar, anotar sobre o ecrã partilhado, fixar mais de um participante, partilha de ecrã para vídeo (hoje 5–15 fps), canais de interpretação, salas de grupo fora do formato treino | `useLayout.ts:71`; `webrtc.ts:174`; `api.ts:16` | um e2e por função | G |
| E20 | Acesso de operador auditado (quebra de vidro) e medição de consumo por organização para facturar | grep `impersonat\|break.?glass\|invoice` = 0 | decisão comercial primeiro | M |

## Onda 3 — Operadoras e PBX de cliente

Objectivo: o departamento de interligação de uma operadora consegue ligar um tronco, medir e
facturar sem trabalho à medida.

| # | Item | Evidência | Prova | ≙ | Tam. |
|---|---|---|---|---|---|
| T1 | **Ligar a telefonia de troncos na configuração distribuída**: binding `freeswitch-config`, perfil de tronco e `json_cdr` no arranque do compose, do cluster e do chart | só em `voice/freeswitch/telefonia-prova/`; `freeswitch-entrypoint.sh:67-71` não os copia | gateway registado e CDR com MOS e custo entregue, no compose e no cluster | — | G |
| T2 | Um ramal sai para a PSTN pelo plano de marcação, e o 112 tem caminho | `00_delonix_extensions.xml:6-9,25`; `ramais_dial.lua:71-88` | chamada de um ramal a um número de ensaio; 112 encaminhado, não gravado e não travado | — | M |
| T3 | Perfil de tronco no repo, não o `external` vanilla da imagem: PCMA e PCMU fixados, RFC 2833 com payload 101, formato de número por tronco | `internal.xml:36,44-45`; grep `strip\|number_format` = 0 | DTMF de um tronco de ensaio aceite pelo IVR; número no formato pedido | — | M |
| T4 | Decisão do SRTP à entrada por tronco (D4) | `internal.xml:26`; `telephony_fs_xml.rs:230` | tronco sem SRTP: aceite por rede privada ou recusado, conforme a decisão | — | P |
| T5 | **Primeira operadora**: pedido de informação, tronco de ensaio, as três medições da skill `delonix-meet-voip` | nenhuma chamada passou por uma operadora (`regressions.md:2317`) | chamada nos dois sentidos, DTMF, e os controlos negativos | 3.11 | G |
| T6 | Identidade do chamador: `P-Asserted-Identity`, pedido de privacidade, e só apresentar um número que o inquilino possui | grep `P-Asserted\|privacy` = 0; `caller_id: None` (`telephony_service.rs:507`) | captura SIP com a identidade certa; número alheio recusado | — | M |
| T7 | Session timers e tempo-limite de RTP nos perfis distribuídos | grep `session-expires\|enable-timer` = 0 | chamada morta termina e deixa de facturar | — | P |
| T8 | Antifraude: tecto de gasto diário por inquilino, tranca própria de internacional, lista de prefixos de tarifa majorada, alarme de volume; limite de canais também no `originate` e à entrada | grep `spend\|fraud\|premium` = 0; `telephony_esl.rs:303-405` | cada regra com a chamada que trava | — | G |
| T9 | Bordo de produção: UDP e TCP 5060 fechados ou declarados, limite de taxa, NAT de media, topologia escondida, `CANCEL` sem to-tag (hoje `405`), filtro de `REFER`, TLS com verificação do par, `OPTIONS` só para a allowlist | `kamailio.cfg:55-61,70-81,112-128`; `voice/cluster/tls.cfg:5,12` | `INVITE` de fora recusado; inundação travada; chamada cancelada antes de atender | 3.3, 3.5 | G |
| T10 | Allowlist do bordo ligada aos troncos da API, sem rollout; a central autenticada (**em curso, #187**) | `templates/voice.yaml:29-32,232` | tronco criado na consola passa de recusado a aceite sem reiniciar | 3.9, 3.10 | M |
| T11 | Voz com mais de uma instância: dois FreeSWITCH no dispatcher, ESL fora do loopback, `INVITE` ao pod que tem a sala, FreeSWITCH sem root e com logs no stdout; SDES nunca em claro entre o Kamailio e o FreeSWITCH nem nos ramais | `dispatcher.list:5`; `freeswitch-entrypoint.sh:34-37,131`; `_helpers.tpl:192-199` | telefone dentro da sala no cluster; reiniciar o FreeSWITCH e repetir | 3.1, 3.2, 3.4 | G |
| T12 | CDR de operadora: Call-ID SIP e causa Q.850, exportação em CSV e por API paginada, reconciliação com a factura; tarifa por destino e incremento configurável | `telephony_cdr.rs:118`; `cost.rs:18`; grep `text/csv\|q850` = 0 | CDR de ensaio reconciliado com o da operadora | — | M |
| T13 | API de parceiro: telefonia em `/api/v1` com escopos (troncos, números, CDR); ciclo de vida dos DID | `identity/api_key.rs:63-69` sem telefonia; `voice_did` sem `PATCH` nem `DELETE` | aprovisionar um tronco e um número só com chave de API | — | G |
| T14 | Chamada de saída a partir da sala, com custo por sessão | produtor só em testes (`signaling.rs:6371`); cinco `dead_code` em `telephony_service.rs` | convidar um telefone da sala, ver o custo e desligar | — | M |
| T15 | Interop e qualidade: captura SIP (HEP ou equivalente), métricas de telefonia, alarmes por tronco | grep `siptrace\|hep` = 0; métricas de telefonia = 0 | uma falha de interop diagnosticada só com o que a instalação guarda | — | M |
| T16 | IVR com portão de comportamento (PIN, ponte, recuo), não só de sintaxe | `dialin_ivr.lua:18-19,40` | o portão falha com o PIN errado aceite | — | M |
| T17 | Gravação de chamadas com dono: ingestão, retenção e acesso (D11) | `record_session` para WAV local (`telephony_fs_xml.rs:105-109`); grep `recording_path` = 0 | gravação de uma chamada de ensaio ouvida pela consola e apagada pela retenção | — | M |
| T18 | Funções de central no Meet (D5) e softphone no browser — só depois da decisão | grep `REFER\|voicemail\|mod_verto` = nada funcional | — | — | — |

Fora desta onda, até uma operadora o exigir por escrito: STIR/SHAKEN, T.38, portabilidade e ENUM,
SMS de entrada (`deliver_sm`).

## Onda 4 — Estúdio e estação de TV

Objectivo: o que o ecrã promete existe, a emissão não depende de um separador aberto, e o sinal
entra e sai nos formatos que um canal usa. Tudo depende de D3 e D8.

| # | Item | Evidência | Prova | ≙ | Tam. |
|---|---|---|---|---|---|
| TV1 | Ligar o backend do estúdio ao ecrã: fontes, emparelhamento, documentos, tally | dez rotas em `lib.rs:1144-1193` sem consumidor | fontes e emparelhamento a funcionar pela consola | 6.1 | G |
| TV2 | Ecrã de destinos guardados; a chave deixa de passar pelo browser | `Studio.tsx:442`; `useMulticam.ts:98-101` | emissão iniciada só com o id do destino | B8 | M |
| TV3 | Codificador do directo: áudio a 48 kHz (hoje 44,1), keyframes fixos, débito pelo perfil escolhido (hoje 4,5 Mbps fixos), contrapressão no envio | `broadcast.rs:307-312`; `directo.ts:288-306`; grep `bufferedAmount` = 0 | `ffprobe` contra um servidor de ensaio; aviso visível num uplink lento | 5.3 | M |
| TV4 | Emissão que não morre com o browser do anfitrião (B4) | `broadcast.rs:1275,1820-1833` | queda induzida do socket sem terminar os destinos | 5.4, 5.5 | G |
| TV5 | Áudio de emissão: limitador no mestre, alvo de sonoridade configurável (−23 para televisão) medido depois do último processamento, atraso por canal | `mesaDeSom.ts:134-146`; `MesaDeSomEcra.tsx:17`; grep `createDelay` = 0 | medição do ficheiro emitido igual ao número mostrado | 6.3, 6.4 | M |
| TV6 | Mix-minus por convidado e intercomunicação | mix-minus só em `phone_bridge/audio.rs`; grep `talkback\|intercom` = 0 | convidado ouve o programa sem se ouvir | 6.5 | M |
| TV7 | Gravação: pedir keyframe e alinhar pelo primeiro frame real; detectar perda e reordenação; quota antes de compor; aviso quando uma faixa não é gravável (só VP8 e Opus se gravam); ISO por fonte ou retirar a bandeira | `sfu.rs:1859-1930,2012-2019`; `recorder.rs:115-122,394,1022-1037` | par clique e flash com o desvio medido no ficheiro final | 5.1, 5.2, 5.6, 6.6 | G |
| TV8 | Fontes: ficheiro de vídeo e clip, mais de seis, bastidores, tally de pré-visualização para a câmara remota, emparelhar o telemóvel por QR | `fontes.ts:11,19`; `signaling.ts:124` | um alinhamento com clip, convidado em bastidores e duas câmaras remotas | — | G |
| TV9 | Multiview em saída própria e gráficos por programa | grep `multiview` = 0; `palco.ts:159-180` | segundo ecrã com todas as fontes | 6.9 | G |
| TV10 | Legendas fechadas no directo (hoje queimadas na imagem) | grep `608\|708\|subtitle` em `broadcast.rs` = 0 | legendas que o destino liga e desliga | 6.10 | M |
| TV11 | Ingestão externa: fechar o spike do MediaMTX (WHIP de um browser, um OBS real, 30 min, leitores reais) e aceitar o ADR-0015; credencial por fonte e expulsão pela API de controlo | grep `whip\|srt://\|mediamtx` fora de `docs/tv/` = 0 | um OBS entra como fonte e é expulso ao revogar a chave | — | G |
| TV12 | Distribuição própria: LL-HLS e leitor público servido pela instalação; SRT de saída | grep `hls\|m3u8` = 0; `broadcast.rs:187-203` só RTMP | leitor num telemóvel com a latência medida; descodificador SRT | 6.2 | G |
| TV13 | Canal: executor com posse (as funções `claim`, `renew` e `report` são `dead_code`), grelha, playout, continuidade | `tv_broadcasts.rs:10-18,337-387` | emissão agendada que arranca sozinha e sobrevive à queda do executor | — | G |

Fora desta onda, por não ter cliente que o peça: NDI, SMPTE ST 2110, repetição instantânea,
marcadores de publicidade, timecode, atraso de emissão, câmara virtual.

## Onda 5 — Escala

| # | Item | Evidência | Prova | Tam. |
|---|---|---|---|---|
| X1 | O lugar reservado sobrevive à morte do pod. A retoma no MESMO nó já existe (R91: `reconnect_secret`, `SignalingHub::reclaim`, `RECONNECT_GRACE_SECS`); o lugar vive em memória do nó (**em curso no ramo `delonix-meet-backend/lugar-em-redis`**) | `signaling.rs:1918`; `config.rs:618` | reentrada sem voltar pela sala de espera depois de o pod da sala morrer | M |
| X2 | Carga em hardware dedicado, com browsers reais, TURN e simulcast (**o #188 reavalia e documenta; não repete nestas condições**) | `docs/ops/teste-de-carga-2026-09-17.md` | SLO da maior sala cumprido e publicado, com o commit | G |
| X3 | Sincronismo e codecs no SFU: RTCP SR com NTP; preferência de codec no cliente | `sfu.rs:48-50`; grep `setCodecPreferences` = 0 | desvio áudio-vídeo medido numa gravação | M |
| X4 | Mais de uma região: hoje só desenho | `docs/multi-region-scaling.md`; grep `cascad` só dá `ON DELETE CASCADE` | ADR antes de código | G |
| X5 | Crates em falta do ADR-0006 (media, tempo real, integrações) | só `core`, `domain`, `protocol` e `store` | catraca da arquitectura a descer | G |
| X6 | App móvel nativa (D10) | zero projectos nativos no repo | decisão primeiro | G |

## 4. Caminho crítico

Três perguntas, três sequências. As frentes correm em paralelo; dentro de cada uma, a ordem conta.

1. **«Pode um banco ou um ministério adoptá-lo?»** — onda 0 inteira → O2, O4, O6, O8, O9, O10 →
   E1, E2, E3 → E5 ou E4 → E8, E9, E10, E11.
2. **«Pode uma operadora pedir integração?»** — T1 → T2, T3 → D4/T4 → T5 → T6, T7, T8, T9 → T12,
   T13. Sem o T1, o que se mostra a uma operadora é a prova, não o produto.
3. **«Pode um canal emitir por ele?»** — D3 e D8 → TV1, TV2, TV3 → TV4 → TV5, TV6, TV7 → TV11, TV12
   → TV13. Até ao TV4 não se anuncia «Estúdio de TV para canais».

## 5. O que este plano não valida

- Nada correu: nem testes, nem build, nem browser, nem uma chamada. O `X-Forwarded-For` forjado, a
  troca de password sem a actual, o 112 de um ramal, a mudança de língua a meio de uma emissão e o
  `make prod` foram **lidos**, não exercitados.
- As provas citadas do chart (#156) e da imagem com ffmpeg (#179) são as que as PRs afirmam.
- O perfil `external` e o `vars.xml` do FreeSWITCH vivem na imagem, não no repo: DTMF e codecs do
  tronco não são verificáveis sem ela.
- Os números de linha de `server/src/lib.rs` são os de `45c82940`; a fusão do #181 deslocou-os
  quatro linhas.
- Preços, regiões e funções dos concorrentes, e qualquer exigência regulatória (INACOM, Lei 22/11),
  não foram verificados aqui.
- Os tamanhos são ordem de grandeza. Não há datas porque não há capacidade de equipa medida.
