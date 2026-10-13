# Levantamento — Delonix Meet contra Teams, Zoom e Google Meet: paridade, enterprise, escala e stack

**Medido a 2026-10-13** contra `origin/develop` @ `0958ec08` (PR #336 fundida), com os
portões corridos nesta árvore. **Não é um documento de venda**: cada linha ou tem
`ficheiro:linha` por trás, ou diz que não foi medida.

Sucede, sem os substituir, o [posicionamento competitivo](competitive-positioning.md)
(secção medida a 2026-10-05) e a [adopção vs concorrência](adopcao-vs-concorrencia.md)
(2026-09-16, com correcções datadas). O que estes dois não tinham, e é a razão deste:
**uma resposta em números à pergunta «quantas chamadas aguentamos»** e **um veredicto
sobre a stack por contexto**.

## Resumo — as sete frases que contam

1. **O produto está mais perto da paridade do que a infra-estrutura está da escala.** A
   lista de funcionalidades que faltam encurtou (o convite por correio fechou); a lista de
   limites de capacidade não se mexeu.
2. **Hoje, em produção, o tecto é de ~150 participantes em TODA a plataforma** — pelas 301
   portas de relay e `totalQuota: 300` do único coturn
   (`deploy/helm/delonix-meet/values-production.yaml:267-275`, que faz a conta pelas suas
   palavras). E pode morder mais cedo: com `FORCE_TURN_RELAY=1` e hairpin **cada byte de
   media atravessa o coturn duas vezes** (`deploy/k8s/51-coturn.yaml:111-116`), o que pelos
   4 CPU dele dá **~30 participantes em HD** — estimativa, porque o coturn nunca foi medido
   sob carga.
3. **Uma sala em produção satura muito antes do que a máquina aguenta.** Medido a
   2026-10-05 em metal: 40 pessoas = 11,17 cores e 2 929 MB. O pod de produção tem
   **2 CPU e 2 GiB** (`values-production.yaml:80-82`). Pelos números medidos, isso dá uma
   sala de **16 a 24 câmaras**, não de 40 — e não foi medido em produção.
4. **A protecção contra sobrecarga existe no código e está desligada.** `NODE_PEER_CAPACITY`
   (`signaling.rs:4076`) e `ORG_MAX_PARTICIPANTS` (`signaling.rs:4180`) não aparecem em
   nenhum `values`: o nó degrada em vez de recusar, e a parte justa por inquilino nunca
   dispara.
5. **A sala não atravessa pods, por desenho** (ADR-0001: `hash(room)` → sempre o mesmo
   pod). O HPA (3→8 réplicas, CPU a 70%) escala salas NOVAS, nunca a sala que está cheia —
   e a colocação é por hash, não por carga: uma sala nova cai no pod do hash, mesmo que
   seja o mais ocupado.
6. **O que nos distingue continua a distinguir**: self-host real, IA local, voz/PBX no
   mesmo produto, Estúdio-TV, superfície MCP para agentes, e isolamento entre organizações
   provado em **190 rotas** (portão corrido hoje).
7. **O que bloqueia enterprise não é a chamada**: SCIM e SAML, calendário do Outlook/Google,
   retenção legal com pesquisa, app nativa, e modo webinar. São cinco, estão nomeadas, e
   nenhuma está começada.

---

## Método, e o que este documento NÃO prova

- Medido por leitura de código e de configuração na árvore acima, mais os portões
  `check-isolamento-cobertura.sh` (corrido: ✓, 190 rotas) e a leitura dos dois documentos
  de carga existentes (`docs/ops/teste-de-carga-2026-09-17.md` e `-2026-10-05.md`).
- **Nada foi medido no cluster de produção nesta sessão.** Os números de produção são os
  dos manifestos e dos `values`, não os do que lá corre.
- **Nenhuma carga nova foi corrida.** Os números de capacidade são de 2026-09-17 e
  2026-10-05, num portátil, em loopback, **sem TURN, sem simulcast, sem browsers reais e
  sem rede**. O arnês que os produziu (`.carga/corre.sh`) **não está no repositório**.
- Os concorrentes não foram re-verificados: o que se diz de Zoom, Teams e Meet é
  conhecimento público e deve ser confirmado antes de entrar numa proposta.

### O tamanho do que existe (medido hoje)

| | |
|---|---|
| Rust (`server/`) | 418 ficheiros, **208 509 linhas** |
| TypeScript (`web/src`) | 625 ficheiros, **123 133 linhas** |
| Migrações | **119** (a última é a `0119_meetings_remarcar.sql`) |
| Catálogo de regressões | **231** (a última é a R311) |
| Binários de teste | **79** em `server/tests/` |
| Portões `scripts/check-*.sh` | **28** |
| ADRs | **30** (o último é o 0030, dobragem ao vivo) |
| PRs abertas | 5 (#251, #308, #335, #337, #338) |

---

## Parte I — Onde estamos melhor, medido

Não é «melhor em tudo»: é o que um dos três não faz, ou faz pior, e que aqui tem código.

| # | Vantagem | Prova | O que a limita |
|---|---|---|---|
| 1 | **Self-host a sério** (binário, compose, Helm, air-gap) | `deploy/helm/delonix-meet/`, `deploy/compose/`, `deploy/k8s/` | Nenhum dos três tem equivalente. A conformidade vem de ONDE se instala — o produto não tem definição de residência de dados |
| 2 | **IA local, sem saída para nuvem** | `server/src/ai.rs`, `ai-worker/`, ADR-0024/0029; sem `OLLAMA_URL` a IA desliga em vez de degradar para uma nuvem | Copilot/AI Companion ganham em capacidade ao vivo; a nossa acta trunca em janela (achado aberto do `num_ctx`) |
| 3 | **Voz e PBX no mesmo produto** | ponte telefone↔sala (ADR-0010), ramais, troncos, plano de marcação, CDR e custo (ADR-0009), FreeSWITCH e Kamailio em `voice/` | Zoom Phone e Teams Phone são add-ons pagos; nós não temos filas, atendedor automático nem correio de voz |
| 4 | **E2EE em grupo com gravação por delegação de chave** | `web/src/e2ee.ts`, `rooms.e2ee` | Chave partilhada AES-GCM; o MLS (`server/src/mls.rs`) é desenho sem rota |
| 5 | **Estúdio-TV**: cenas, faders, sobreposições, editor não destrutivo, emissão | `web/src/studio/`, `server/src/broadcast.rs` | Nenhum dos três tem isto. A emissão é RTMP para fora — não há origem HLS própria |
| 6 | **Superfície MCP + OAuth 2.1**: um agente opera o produto | `server/src/mcp.rs`, ADR-0027/0028, migração `0117` | Nenhum dos três oferece isto hoje. Falta SDK e apps dentro da reunião |
| 7 | **Isolamento entre organizações provado, não afirmado** | `check-isolamento-cobertura.sh` ✓ — **190 rotas** de inquilino exercitadas por `web/e2e/isolamento.mjs` | A defesa é da aplicação: RLS está em **1 tabela de 111** (`0024_rls_employee_groups.sql:14`) |
| 8 | **API-first sem plano pago**: OpenAPI gerado, webhooks HMAC, chaves com escopos | `check-openapi.sh` com catraca a zero; `apikeys.rs`, `webhooks.rs` | Sem SDK publicado |
| 9 | **Edições sem licenciamento fechado** | `server/crates/delonix-meet-core/src/edition.rs:4` — «não é licenciamento: nenhuma capacidade fica fechada no código por edição» | É uma escolha de produto; a sustentar quando houver pressão comercial |
| 10 | **Hierarquia de organização, filiais e grupos** | `org.rs`, `directory.rs` | Zoom/Teams/Meet resolvem-no com tenants e domínios |
| 11 | **Exportação dos próprios dados, assíncrona e assinada** | `server/src/data_exports.rs:1-8` | Falta o equivalente para o OPERADOR (eDiscovery) |
| 12 | **Odoo como primeira classe** (login, provisionamento, agenda, acta de volta) | `odoo_sso.rs`, `odoo.rs`, `meetings_v1.rs` | Quem vive no Outlook não tem equivalente |

## Parte II — O que falta para igualar (re-medição das 14 linhas de 2026-10-05)

A tabela de 2026-10-05 tinha 14 linhas. Oito dias depois, **uma fechou** e as outras não.

| # | Linha de 2026-10-05 | Estado a 2026-10-13 |
|---|---|---|
| 1 | App móvel nativa | **a linha antiga estava errada: existe neste repo** — `mobile/delonixphone/` (Flutter, 38 ficheiros Dart). Mas é um **softphone SIP** (`pubspec.yaml:2`) e **não entra numa reunião**: o `cliente_meet.dart` só regista o aparelho e a organização. A PWA continua sem receber chamada com a app fechada (`vapid\|pushManager` = 0 em `server/src` e `web/src`), e dos providers de push só o `lab` envia (`voice_devices.rs:16-21`) |
| 2 | Convidar por correio | **FECHADA**: `invitation_mail::send_best_effort` ligado em `directory.rs:860` e `:1095`; `mail.rs` e `org_mail.rs` (ADR-0025, SMTP por org no #325). Mas **ainda não se convida um endereço externo para uma reunião**: `meeting_invitees` só tem `user_id` (`0004_library_and_calendar.sql:35`) e a notificação filtra co-membros da org com endereço provado; e o correio **não leva `.ics`** (`mail.rs:466` é texto simples), por isso o convite não aterra em calendário nenhum |
| 3 | Calendário externo (Outlook, Google) | **continua a não existir**: `graph.microsoft`, `googleapis` e `caldav` não aparecem em `server/src` |
| 4 | SAML e SCIM | **continuam a não existir** (`grep -i saml\|scim` em `server/src`: nada). Há OIDC e prova de domínio por DNS TXT (#299) |
| 5 | Modo webinar | **continua a não existir**: `webinar` só aparece em documentos |
| 6 | Canais persistentes | **a prova antiga estava trocada**: a `0086_room_channels.sql` não é sobre canais de equipa — cria `room_dial_outs`, `room_phone_pins` e `org_whatsapp_configs` (acrescentar à sala por número), e as duas primeiras **já têm consumidor** (`dial_outs.rs:103,390`). A lacuna real é maior: não há canal de equipa nenhum, **e o chat da sala é apagado ao fim do dia por desenho** (`room_chat.rs:10-12`, `:220`). Sem consumidor fica só `org_whatsapp_configs` |
| 7 | PBX a sério (filas, atendedor, correio de voz) | **continua a faltar a camada**; a base (IVR, ramais, troncos, CDR) está lá |
| 8 | Retenção legal e eDiscovery | **continua a faltar**: retenção por dias e auditoria encadeada existem; `legal_hold` e pesquisa transversal não existem em `server/src` nem nas migrações |
| 9 | Salas de hardware e SIP de sala | **continua a faltar, e há uma razão técnica nova**: o SFU registra os codecs por omissão (`sfu.rs:895`), mas a gravação só trata **VP8→IVF** (`recorder.rs:2`) e a E2EE assume VP8 (`e2ee.ts:2`). Uma Poly de sala fala H.264 |
| 10 | Marketplace e SDK | **mudou de matéria-prima**: OAuth 2.1 (`0117`) e MCP (ADR-0027) dão a um terceiro um caminho de integração que não existia. Continua sem SDK publicado nem apps na reunião |
| 11 | Rede má | inalterado: somos browser-first, sem FEC próprio |
| 12 | Escala de uma sala | **já não é verdade que não esteja medida** — está, em metal (ver Parte IV). O que falta é o número **em produção**, e ele é mais baixo |
| 13 | IA na reunião | pós-reunião forte; ao vivo continua a faltar. Dobragem ao vivo decidida (ADR-0030) e travada por licença de voz (#335) |
| 14 | Polimento | inalterado |

## Parte III — O que falta para ser a opção do mercado enterprise

Por ordem do que mata um negócio. As quatro primeiras não são de chamada: são de
administração.

| # | Lacuna | Porque bloqueia | Custo de não ter |
|---|---|---|---|
| 1 | **SCIM** (e depois SAML) | Uma empresa de 200+ pessoas pergunta isto antes do preço: entradas e saídas têm de vir do directório dela. Sem SCIM, cada saída é manual — e uma conta que não sai é um achado de auditoria | perde-se a conversa na primeira reunião técnica |
| 2 | **Calendário do Outlook/Google** | A reunião nasce no calendário, não no nosso ecrã. Sem isto, o produto é um sítio onde se vai ter a reunião, não onde ela se marca | atrito diário em 100% dos utilizadores |
| 3 | **Retenção legal e pesquisa transversal** | Banca, seguros, governo: o regulador pede uma pesquisa por pessoa e um «não se apaga isto». Temos retenção e auditoria; não temos *hold* nem pesquisa | exclui sectores regulados, que são exactamente os que querem self-host |
| 4 | **Defesa em profundidade na base** | O isolamento está provado em 190 rotas da aplicação. Na base de dados, RLS cobre **1 tabela de 111** — e o portão `check-tenant-rls.sh` procura um pod `delonix-postgres-postgresql-0` que não é o CNPG de produção (`meet-pg`), por isso **salta em silêncio** | uma pergunta de due diligence sem boa resposta |
| 5 | **App nativa** | O nosso mercado vive no telemóvel; a PWA não recebe uma chamada com a aplicação fechada | perde-se a adopção de quem não está ao computador |
| 6 | **Modo webinar** | O evento de 500 pessoas é o que justifica o orçamento anual | fecha a porta ao caso de uso mais caro |
| 7 | **Medição do consumo e facturação** | Há quotas (`max_seats`, `max_concurrent_participants`, `0013_org_quotas`) e resumo de custo de VOZ (`/voice/billing`). Não há medição de minutos, armazenamento ou lugares para facturar | sem SaaS vendável por consumo |
| 8 | **SLO escrito e suporte** | Há alertas (`deploy/k8s/observabilidade/alertas.yaml`) e OTLP. Não há SLO nem error budget escritos | não há o que prometer num contrato |

**O que já não é lacuna, e às vezes é dito que é:** entrada de convidado sem conta (R290),
PSTN dial-in (ADR-0010), gravação com transcrição e pesquisa, quadro branco persistido,
sondagens e quiz, salas de grupo, legendas e tradução locais, companion mode, MFA TOTP,
auditoria encadeada com verificação, convite por correio (desde o ADR-0025).

---

## Parte IV — Infraestrutura: quantas chamadas aguentamos

### IV.1 O modelo de escala, em quatro frases

1. **Uma sala vive num pod.** ADR-0001: o `/ws` leva `&room=CODE` e o balanceador faz
   `hash(room)` → sempre o mesmo pod (`scripts/check-room-affinity.sh`, e em Gateway API a
   `BackendTrafficPolicy` do Envoy Gateway em `deploy/helm/delonix-meet/templates/gateway.yaml:131-153`).
   O SFU é memória do processo; partir isso é a regressão R3, «media num só sentido» — o
   chart recusa `affinity.implementation: none` em produção (`_helpers.tpl:357`).
2. **Escala-se por salas, nunca dentro de uma sala.** O HPA de produção vai de 3 a 8
   réplicas por CPU a 70% (`values-production.yaml:76-79`, `values.yaml:189-194`). Uma sala
   cheia não ganha nada com uma réplica nova.
3. **A colocação é por hash, não por carga.** Existe inventário de nós com ocupação e
   batimento (`server/src/nodes.rs`, `/api/operator/v1/nodes`) e existe admissão por nó
   (`admit_new_room`, `signaling.rs:4076`). **Não existe quem escolha o nó**: a sala nova cai
   no pod do hash, ocupado ou não.
4. **Em produção toda a media passa pelo relay.** `FORCE_TURN_RELAY=1`
   (`templates/configmap.yaml:36`, reconhecido em `values-production.yaml:231-232`), coturn
   em `hostNetwork`, **uma réplica**.

### IV.2 O que está medido (metal, loopback) e o que produção permite

Medições existentes, as duas no mesmo portátil (Ryzen 9, 16 núcleos para o servidor),
**sem TURN, sem simulcast, sem browsers, sem rede** — `docs/ops/teste-de-carga-2026-10-05.md`
e `-2026-09-17.md`:

| Cenário | Pessoas | CPU do servidor | RSS | Resultado |
|---|---|---|---|---|
| 1 sala, 720p | 16 | 1,91 cores | 577 MB | 240/240 fluxos, 0% perda |
| 1 sala | 24 | 3,28 | 1 100 MB | 552/552, 0% |
| 1 sala | 30 | 7,01 | 1 807 MB | 870/870, 0,01% |
| 1 sala | **40** | **11,17** | **2 929 MB** | 1560/1560, 0,03% — **a rampa acabou os degraus sem falhar: não é o tecto** |
| 40 salas × 4 | 160 | 4,8 | — | 471/480 fluxos, 0,26% |
| 50 salas × 4 | 200 | 5,8 | — | **colapso**, 29–48% de perda |
| 20 salas × 4 **a compor gravação** | 80 | ~12 (ffmpeg) | — | **chamadas inutilizáveis**: 64% de perda, 13–15 min por gravação de 58 s |

Agora os limites que a instalação impõe:

| Limite declarado | Onde | O que implica |
|---|---|---|
| `server` 2 CPU / 2 GiB, 3–8 réplicas | `values-production.yaml:80-82,73-79` | pelos números acima, **uma sala de 16 a 24 câmaras** por pod; aos 30 já se precisava de 7 cores |
| `server` 4 CPU / **1 GiB** | `deploy/k8s/02-server.yaml:146-152` | o manifesto cru é **pior**: 1 GiB é o RSS de uma sala de ~22 pessoas → OOMKill antes do CPU |
| coturn: 1 réplica, `relayPorts 20000-20300`, `totalQuota: 300` | `values-production.yaml:267-275` | **~150 participantes em simultâneo em toda a plataforma** (2 alocações por participante — o comentário do próprio ficheiro di-lo) |
| coturn 4 CPU / 1 GiB, `relayThreads: 4` | `values.yaml:289,298-300` | a media de todos passa aqui; não há segunda instância nem DNS round-robin |
| `FFMPEG_MAX_CONCURRENT` = 1 | `server/src/config.rs:316-325,846` | uma composição por pod — mas **no mesmo pod do SFU**, que é a condição que inutilizou as chamadas em Setembro |
| `NODE_PEER_CAPACITY` | **não definido em nenhum `values`** | a admissão por nó e a parte justa por inquilino (`node_new_rooms_refused_fair_share_total`) **nunca disparam** |
| `ORG_MAX_PARTICIPANTS` | **não definido em nenhum `values`** | sem tecto por organização por omissão; só o que o operador puser por org (`/organizations/{id}/concurrency`) |
| Gravações em PVC `ReadWriteMany` 100Gi | `values-production.yaml:115-118` | o gravador ainda não escreve no MinIO (ADR-0020 por fechar): o disco partilhado é o que não escala |

**A conta honesta, hoje:** o tecto de produção é o do coturn — **~150 participantes
simultâneos** — e, dentro dele, **nenhuma sala passa de ~20 câmaras** sem o pod bater no
limite. Ambos são deduzidos de configuração e de medições feitas noutra máquina: **nenhum
foi medido em produção**.

### IV.3 As oito lacunas de infra, por ordem de estrago

1. **TURN é um ponto único, e o tecto real pode ser metade do das portas.** Uma réplica,
   um nó, `hostNetwork`, `strategy: Recreate` e **sem PDB** (`templates/coturn.yaml:4-7,16-17`).
   Pior do que as 301 portas: com `FORCE_TURN_RELAY=1` e hairpin, **cada byte de media
   atravessa o coturn duas vezes** — o próprio manifesto escreve a regra, «N pessoas em HD
   ≈ N × 12 Mbps × 2; 10 pessoas ≈ 250 Mbps» (`deploy/k8s/51-coturn.yaml:111-116`). Com 4
   CPU, isso são **~30 participantes em HD** por analogia com os ~180 Mbps/núcleo do SFU —
   **estimativa: o coturn nunca foi medido sob carga neste projecto**. Falta: mais
   instâncias, `--max-bps` por sessão, TURN/TLS em 443 (hoje `--no-tls --no-tcp-relay`: uma
   rede que só deixe sair 443/TCP fica sem media), e a decisão sobre forçar relay sempre.
2. **Admissão desligada, e meia mesmo quando ligada.** Sem `NODE_PEER_CAPACITY` o nó aceita
   até degradar — e o alerta que existia para isto (`MeetSalasRecusadasPorCapacidade`,
   `alertas.yaml:112`) **nunca pode disparar**. Mesmo com a variável posta, o tecto do nó só
   é verificado **à criação da sala** (`signaling.rs:4077`: `if !state.hub.tem_sala(room_id)`):
   depois de a sala existir no pod, as entradas seguintes não têm tecto de nó nenhum.
3. **Composição de gravação no pod do SFU.** O tecto de 1 é correcto e insuficiente: a
   composição tem de ser um Deployment próprio, com o seu limite e a sua classe.
4. **Sem colocação por carga.** Hash puro: uma sala nova pode nascer no pod mais cheio.
   O inventário de nós já tem os dados para decidir; falta quem decida.
5. **Sem cascading entre nós, e portanto sem multi-região.** `docs/multi-region-scaling.md`
   é um desenho — não há código S2S. Uma reunião entre Luanda e Lisboa paga a travessia
   em cada participante.
6. **O limite de memória do manifesto cru (1 GiB) contradiz a medição.** Em `deploy/k8s`,
   o OOMKill chega antes do CPU.
7. **Carga sem portão e sem arnês no repo.** O `.carga/corre.sh` ficou num worktree; não
   há medição nocturna, nem número publicado com simulcast, TURN ou browsers.
8. **Postgres e Redis não foram avaliados sob carga de chamada.** Em produção o Postgres
   está a **2 de 3** réplicas (sprint de 2026-10-12) e o Redis é o guarda do lugar (seats);
   o efeito de os perder a meio de 100 salas não está medido.

### IV.4 O que é preciso para cada escalão (estimativa, declarada como tal)

| Escalão | O que basta | O que é preciso construir |
|---|---|---|
| **até 150 participantes** (hoje) | o que está instalado, com a admissão ligada | ligar `NODE_PEER_CAPACITY`/`ORG_MAX_PARTICIPANTS`; publicar o tecto por sala |
| **1 000 simultâneos** | ~8 pods de 2 CPU, 4–6 coturn com gama de portas larga, composição fora | colocação por carga; TURN em várias instâncias; worker de composição; gravações no MinIO; carga medida com browsers e simulcast |
| **10 000 simultâneos** | vários nós por região | cascading S2S entre SFUs (sem isto, uma sala nunca passa de um pod); SFU como processo próprio com o seu ciclo de vida; FEC; Postgres com réplicas de leitura; SLO e error budget |
| **sala única > 100 pessoas** | — | não é alcançável no modelo actual: o tecto da sala é o tecto de um pod. Exige cascading ou um modo de audiência (webinar) que não encaminhe todos para todos |

### IV.5 O que, no código, limita a sala (e não está escrito como limite)

**Não existe tecto de participantes por sala em sítio nenhum do código.** O que existe é por
nó, por conta e por organização. O que limita de facto:

| O que limita | Medido | Consequência |
|---|---|---|
| **Fan-out é O(N) por pacote, O(N²) por sala** | uma tarefa por publicação, escrita em série para cada subscritor (`sfu.rs:1478`, `:1541-1557`) | aos 40 participantes medidos, ~292 k pacotes/s de saída para 11,17 cores — **~38 µs de CPU por pacote reencaminhado** (aritmética sobre o medido, não um perfil; não existe perfil do `sfu.rs` no repo) |
| **Reavaliação total da sala a cada entrada, saída ou câmara ligada** | `reevaluate_room` percorre todos os peers **em série com `.await`** (`sfu.rs:1689-1703`), chamado em `:1251`, `:1861`, `:2511` | O(N²) de trabalho por evento. É o que se vê no tempo de entrada a subir de 0,3 s (8 pessoas) para 1,7 s (40), e na perda das 40 concentrada na primeira janela |
| **Um lock mantido através de `.await` no caminho de saída** | `sfu.rs:1810-1821`: o `Mutex` das publicações fica retido através de `subscribers.lock().await` | uma saída de participante serializa o selector de oradores (que pede o mesmo lock a cada 200 ms, `:2065`) e as reavaliações da sala — candidato a defeito R16, a confirmar com perfil |
| **Uma tarefa de dreno RTCP por SUBSCRIÇÃO** | `sfu.rs:2270` | O(N²) tarefas: numa sala de 40 com áudio e vídeo são ~3 120 tarefas e ~620 temporizadores/s só de liveness (estimado) |
| **Lock por pacote mesmo sem gravação** | `publication.rec.lock().await` (`sfu.rs:1501`) | custo fixo em todo o pacote; sai com um `AtomicBool` |
| **Clone por pacote mesmo sem subscritores** | `strip_hop_extensions(&packet)` antes do `for` (`sfu.rs:1546-1547`) | numa sala grande, onde a paginação desliga a maioria dos vídeos, paga-se o que ninguém vê |
| **201 portas UDP por processo** | `SFU_UDP_MIN/MAX = 50000..50200` (`sfu.rs:850-851`) | tecto de ~100–200 ligações por pod, **antes** de qualquer CPU. A medição de Outubro precisou de alargar o intervalo à mão (400 portas) |
| **Gravação: ~2 threads do SO e 2 ficheiros por participante gravado** | `recorder.rs:639-678` (thread por pista, fora do executor — R40 respeitada) | 50 salas a gravar = milhares de threads; e `.expect("thread de gravação")` (`recorder.rs:678`) entra em pânico precisamente no limite de recursos |
| **Travões que existem e que já protegem** | 3 oradores reencaminhados em salas ≥5 (`sfu.rs:474-478`); camada forçada a `q` acima de 8 e a `h` acima de 4 (`sfu.rs:600-622`); sala de espera 500 (`signaling.rs:1442`); 16 sockets por conta (`config.rs:50`); legendas parciais só até 12 na sala (`signaling.rs:1220`); 2 emissões em directo por nó | a sala grande **já** degrada de propósito em vez de rebentar — é a parte boa deste desenho |

**O que isto significa para o produto:** a sala grande não está limitada por uma decisão de
produto, está limitada por quatro caminhos O(N²) e pelo limite do pod. Subir de ~20 para
~50 câmaras por sala é trabalho de SFU (reavaliação incremental, lock de saída, fan-out
preguiçoso), não de infra-estrutura. Passar de ~50 para centenas exige cascading, que não
existe.

### IV.6 Seis armadilhas de operação, medidas na configuração

Não são lacunas de capacidade: são formas de a instalação se partir em silêncio.

1. **Há um caminho de fuga para a afinidade por sala.** O nginx da imagem web encaminha
   `^/(ws|rtc)` para o Service **partilhado** `delonix-server:8180`, em round-robin, sem hash
   (`deploy/k8s/nginx.conf:107`). Em produção o `/ws` só não cai ali porque a `HTTPRoute
   delonix-ws` existe à parte — e essa rota **já esteve `Accepted=False`** neste cluster
   (`values-production.yaml:149-153`). Se voltar a estar, o `/ws` cai no catch-all `/`, vai
   à web, e a web reparte por pods: split-brain de media sem um único erro. Nenhum portão lê
   o `nginx.conf` e nada verifica em runtime se a rota está aceite.
2. **O alerta de TURN está permanentemente a disparar, por desenho.** `MeetTudoPorTurn`
   acende quando >90% das chamadas passam por relay (`alertas.yaml:97-106`) — e
   `FORCE_TURN_RELAY=1` garante 100%. Um alerta que está sempre vermelho é um alerta que
   ninguém lê.
3. **O ponto mais estreito não é raspado.** O coturn expõe `--prometheus` e tem as anotações
   (`templates/coturn.yaml:26-28,89-95`), mas o `ServiceMonitor` só cobre `app:
   delonix-server` (`observabilidade/servicemonitor.yaml:28-30`). Dos 16 alertas, nenhum
   olha para alocações contra a quota.
4. **Um `helm upgrade` do coturn corta toda a media.** Uma réplica com `strategy: Recreate`
   e sem PDB: não é uma fracção dos utilizadores, são todos.
5. **O HPA a subir remapeia o hash consistente.** As ligações abertas ficam; quem entrar
   **depois** numa sala remapeada aterra no pod novo, onde a sala não existe. Não encontrei
   guarda, teste nem nota sobre isto — é inferência estrutural, não medição, e devia ser a
   próxima medição a fazer antes de se confiar no autoscaling.
6. **O estado partilhado não tem failover, e a base não tem pooler.** O Redis é um master
   com réplicas e **Sentinel desligado** por razão medida (`redis-values.yaml:60-79`):
   promover é passo do operador, e com ele em baixo perdem-se presença, sinalização entre
   pods e revogação de sessão. Não há alerta de Redis nem de Postgres nos 16. E o pool da
   aplicação é **10 ligações por pod, fixo em código** (`lib.rs:1982-1983`), com
   `acquire_timeout` de 5 s: contenção de pool aparece como 500/503, não como latência.
   Não há PgBouncer, e as três instâncias do CNPG servem um só `DATABASE_URL` (sem réplicas
   de leitura).

**Dois tectos que ninguém decidiu:** `MAX_DIRECTOS` é 2 **por pod** (`config.rs:847`) — com
8 réplicas são 16 emissões em directo que ninguém aprovou; e dos ~17 ciclos de fundo do
`run()`, **um** tem `CancellationToken` (`jobs.rs:7-13`, que o escreve: «a forma normal de
perder trabalho neste sistema é fazer deploy»), e as varreduras correm em **todas** as
réplicas.

**A moldura física:** a quota do namespace são 13 CPU de requests e 62 de limits
(`values.yaml:592-607`), e os workers do cluster partilhado somam ~30 CPU e ~82 GiB
(`templates/quota.yaml:49-59`). Qualquer plano acima de algumas centenas de participantes
é um plano de **mais ferro**, não de afinação.

---

## Parte V — A stack é adequada? Por contexto

Veredicto por camada, no contexto em que ela é usada. «Adequada» aqui significa: não é a
stack que nos impede de vender esse contexto.

| Camada | O que é | Contexto onde é adequada | Onde deixa de ser, e o que fazer |
|---|---|---|---|
| **API e sinalização** — axum 0.8, tokio, sqlx/Postgres, utoipa | monólito em transição com 4 crates (ADR-0006) | tudo, de 1 a 10 000 utilizadores | nada a mudar. A dívida é de organização (o monólito), não de capacidade |
| **SFU** — `webrtc-rs 0.17` dentro do mesmo processo | fan-out SRTP em Rust, sem GC | reuniões até algumas dezenas por pod; é um diferenciador real (latência determinística, sem pausas) | **o SFU partilhar processo com a API é o problema, não o Rust.** Primeiro passo: Deployment próprio com limites próprios e ciclo de vida próprio (hoje um OOM do pod derruba API e salas juntas). Não trocar por mediasoup/LiveKit: perde-se o controlo e a razão de ser |
| **Codecs** — `register_default_codecs`, mas gravação só VP8→IVF e E2EE a assumir VP8 | VP8 + Opus | browser desktop | **bloqueia salas de hardware (H.264) e custa banda e bateria no telemóvel (sem AV1/VP9 no caminho completo)**. Qualquer promessa de endpoint SIP de vídeo passa por aqui |
| **Rede má** — simulcast + escolha de camada + TWCC por salto | adaptação razoável | 4G estável | sem FEC (`ulpfec`/`flexfec` = 0): num 3G com 20% de perda o Zoom ganha e não se fecha enquanto formos browser-first. A matriz `netem` existe e **não corre no CI** |
| **TURN** — coturn 4.6, uma réplica, `FORCE_TURN_RELAY=1` | garante media atrás de NAT; foi o que fez a produção funcionar | até ~150 participantes | **é o gargalo central e o ponto único de falha.** Forçar relay simplificou a entrega e comprou o tecto mais baixo da plataforma. Várias instâncias + mais portas, e reavaliar o forçar |
| **Processamento de media** — ffmpeg filho do servidor, `FFMPEG_MAX_CONCURRENT=1` | gravação, composição, recortes | poucas gravações por nó | **tem de sair do pod do SFU** (medido: 20 composições = chamadas inutilizáveis). Worker próprio, fila já existe (`jobs`) |
| **Armazenamento de media** — PVC RWX 100Gi; cliente S3 existe e **não está ligado ao gravador** (`objectos.rs:9-14`) | laboratório e primeiras instalações | — | **não escala e não é multi-nó saudável.** Fechar o ADR-0020: gravador → MinIO/S3 |
| **Frontend** — React 19, Vite 6, Insertable Streams, onnxruntime/transformers.js | consola e sala | desktop e tablet | no telemóvel: sem app nativa, sem notificação com app fechada, e os modelos locais pesam. O `mobile/delonixphone` existe mas é softphone SIP |
| **Voz** — FreeSWITCH + Kamailio + SIP/SRTP, SMPP próprio | PBX, dial-in, troncos, SMS | é um diferenciador que nenhum dos três iguala sem add-on pago | falta a camada de PBX (filas, atendedor, correio de voz) e a cadeia com operadora ligada na configuração distribuída |
| **IA** — Ollama + faster-whisper no cluster, nunca nuvem externa (medido: zero chamadas a `api.openai`/`api.anthropic`/Google/Azure) | soberania; acta, capítulos, tradução de legenda ao vivo | é o argumento mais forte que temos | não serve «copilot ao vivo»: nenhuma tool é ao vivo, a acta trunca em janela (`num_ctx`), e a dobragem está travada por licença de voz (ADR-0030, #335). Precisa de GPU dedicada declarada |
| **Dados** — Postgres (CNPG) + Redis + MinIO | estado, lugares, objectos | tudo | o efeito de perder o primário a meio de 100 salas **não está medido**; em produção o Postgres está a 2 de 3 réplicas |
| **Observabilidade** — 124 métricas `delonix_*`, OTLP opcional, alertas e ServiceMonitor | operação | boa, e melhor do que é habitual nesta fase | **sem SLO nem error budget escritos**; sem painel de capacidade (as métricas de recusa existem e ninguém as olha porque a admissão está desligada) |
| **Multi-região** — `docs/multi-region-scaling.md` | — | nenhum | **é um desenho sem código.** Enquanto não houver cascading, uma reunião entre dois continentes paga a travessia por participante |

**Resumo da stack:** a stack está certa para o produto que queremos vender — Rust para o
caminho de media, browser-first para alcance, local-first para soberania. Os dois sítios
onde a escolha nos vai doer e já se vêem são: **o SFU dentro do processo da API** (um limite
de pod derruba as duas coisas) e **o browser-first no telemóvel** (que a app nativa resolve
fora deste repo, não aqui).

---

## Parte VI — O que fazer, por ondas, com portão

Cada item nomeia a prova que o fecha. A ordem é por estrago, não por esforço.

### Onda 1 — tirar os tectos acidentais (dias, não semanas)

| # | Trabalho | Prova que fecha |
|---|---|---|
| 1 | **Ligar a admissão**: `NODE_PEER_CAPACITY` e `ORG_MAX_PARTICIPANTS` nos `values` de produção, derivados do limite do pod | um teste que mostre a recusa com 503 e `delonix_node_new_rooms_refused_total` a subir; a prova de que antes não subia |
| 2 | **Alinhar os limites do pod com a medição**: o chart tem `cpu "1"/1Gi` por omissão (`values.yaml:108`), produção `2/2Gi`, o manifesto cru `4 CPU/1Gi`. Nenhum destes aguenta o que foi medido | uma sala de 16 e de 24 medida **contra os limites de produção**, com simulcast, e o tecto por sala publicado |
| 3 | **Compor fora do pod do SFU**: Deployment próprio para a composição | 10 gravações a compor com uma sala viva no mesmo nó, e a perda da sala medida (a referência é 64% de Setembro) |
| 4 | **Medir e vigiar o coturn, antes de o alargar**: `ServiceMonitor` próprio, alerta de alocações contra a quota, `--max-bps` por sessão, PDB, e corrigir o `MeetTudoPorTurn` que está sempre vermelho | uma corrida de carga **contra o coturn**, que não existe; e o painel a mostrar alocações e banda |
| 5 | **TURN fora dos 150**: mais portas, mais instâncias, TURN/TLS em 443 | alocações simultâneas medidas contra o novo tecto; e a decisão escrita sobre manter `FORCE_TURN_RELAY=1` |
| 6 | **Fechar o caminho de fuga da afinidade**: portão que leia o `nginx.conf` e verificação em runtime de que a `HTTPRoute delonix-ws` está `Accepted` | um controlo negativo: desligar a rota e ver o portão (ou o alerta) falhar |
| 7 | **Medir o que o HPA faz a meio de uma reunião** (remapeamento do hash com gente dentro) | duas réplicas, uma terceira a entrar, e uma pessoa a juntar-se a uma sala remapeada com media a funcionar |
| 8 | **Arnês de carga no repositório** e uma corrida nocturna, **em Kubernetes** | o `.carga/corre.sh` versionado, e um número com browsers reais, TURN e dois pods |

### Onda 2 — o que desbloqueia a venda enterprise (semanas)

| # | Trabalho | Prova |
|---|---|---|
| 6 | **Convite a endereço externo com `.ics` no corpo do email** — a espinha do correio já existe; falta convidado externo em `meeting_invitees` e uma parte `text/calendar` em `mail.rs:466` | um convite recebido numa caixa de correio externa que abre no calendário |
| 7 | **SCIM 2.0** (e SAML depois) | criação, alteração de papel e **desactivação** vindas de um directório de teste |
| 8 | **Legal hold + pesquisa transversal do operador + política de DLP por organização** | um *hold* que impede a varredura de retenção de apagar, provado com a varredura a correr |
| 9 | **Decidir a defesa na base**: ou RLS a sério nas tabelas de inquilino, ou declarar que a defesa é da aplicação e corrigir o portão (`check-tenant-rls.sh` procura um pod que não existe em produção e salta em silêncio) | o portão a falhar quando se tira o RLS de uma tabela coberta, num cluster a sério |
| 10 | **Modo webinar** sobre as peças que já existem | 200 espectadores que não publicam, com Q&A moderado, medidos |
| 11 | **Reavaliação incremental no SFU** e o lock de saída | o tempo de entrada aos 40 participantes abaixo do 1,7 s medido |

### Onda 3 — mercado (trimestres)

| # | Trabalho | Prova |
|---|---|---|
| 12 | **App nativa que entra numa reunião** (hoje o `mobile/delonixphone` é softphone) e push que chega com a app fechada | uma chamada atendida num telemóvel real com a app fechada — hoje «nenhum push chegou a um telemóvel real» (`voice_devices.rs:21`) |
| 13 | **Cascading SFU↔SFU** e, só depois, multi-região | uma sala com participantes em dois nós, com a media a atravessar uma só vez |
| 14 | **SDK publicado** sobre o que já existe (OAuth 2.1, MCP, `bots:join`, v1) | um integrador externo a fazer uma reunião e a ler a acta sem escrever cliente próprio |
| 15 | **Salas de hardware/SIP de vídeo** | exige H.264 no caminho de gravação e E2EE; decisão antes de código |

### Quatro desonestidades a corrigir já (custam credibilidade, não trabalho)

1. **ADR-0027 e ADR-0028 estão «Proposto» com o código fundido** (MCP e OAuth 2.1). O mesmo
   vale para o ADR-0009 e o ADR-0026. Um ADR proposto com código em produção é a documentação
   a mentir ao contrário.
2. **Dois escopos OAuth consentíveis não têm nenhuma tool por trás** — `shorts.publish` e
   `live.translate` (`domain/identity/oauth.rs:62-63`): a pessoa concede ao agente uma
   permissão que não faz nada.
3. **«Gravações no S3 do cliente» não se pode dizer**: o cliente de objectos existe e o
   gravador continua a escrever em disco — o próprio ficheiro o diz (`objectos.rs:9-14`).
4. **`Channel::SipRoom`, `Channel::IpCamera` e `org_whatsapp_configs` são modelo sem fio**:
   existem no domínio e nada os produz. Pelo critério da casa, são lacunas e não capacidades.

---

## O que este levantamento NÃO prova

- **Nada foi medido no cluster de produção.** Os tectos de produção (coturn a ~150 por
  portas ou ~30 por banda, sala a ~16-24) são aritmética sobre configuração e sobre
  medições feitas num portátil. Têm de ser medidos lá.
- **Os valores realmente aplicados não foram lidos** (`helm get values`/`get manifest`): em
  particular `coturn.externalIP`/`privateIP`, o `nodeSelector` do coturn, e se a
  `HTTPRoute delonix-ws` está `Accepted`. A produção corre uma imagem atrás da `develop`.
- **Nenhuma carga nova foi corrida**, e as existentes são sem TURN, sem simulcast, sem
  browsers reais e sem rede. O número que se põe num contrato ainda não existe.
- **Não corri a bateria de testes nem o `make fitness`** nesta árvore: corri o
  `check-isolamento-cobertura.sh` (✓, 190 rotas) e o `check-capability-claims.sh` (✓).
- **O lock de saída do SFU (`sfu.rs:1810-1821`) e os ~38 µs por pacote** são leitura e
  aritmética, não perfil. Não existe perfil do SFU no repositório.
- **As capacidades de Zoom, Teams e Google Meet não foram verificadas**: é conhecimento
  público e deve ser confirmado antes de entrar numa proposta. Só o nosso lado de cada linha
  está medido.
- **Não foi medido o `mobile/delonixphone` a funcionar**, nem nada no browser.

## Apêndice — deriva a corrigir nos documentos antigos

Quatro linhas do [`competitive-positioning.md`](competitive-positioning.md) já não são
verdade, e três delas eram contra nós:

| Onde | O que diz | O que é verdade a 2026-10-13 |
|---|---|---|
| `:124` | «o servidor não envia correio» | envia: `mail.rs` com fila durável, retry e dedupe (ADR-0025) |
| `:133` | atribui à `0086_room_channels.sql` canais sem consumidor | essa migração é de dial-out e PINs, e já tem consumidor (`dial_outs.rs`) |
| `:144` | «o tecto de participantes por sala não está medido» | havia um documento de carga à data e há dois agora |
| `:80` | aponta `rooms.rs:32` para `rooms.e2ee` | hoje é `rooms.rs:37` |

Também vale corrigir a linha «zero projectos nativos no repositório»: existe
`mobile/delonixphone/` — softphone, não cliente de reunião.
