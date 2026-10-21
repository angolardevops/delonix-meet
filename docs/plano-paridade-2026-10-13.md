# Plano de paridade — igualar o Teams, o Zoom e o Meet sem perder o que nos põe à frente

**Escrito a 2026-10-13** contra `origin/develop` @ `26a380c3`. Fonte do diagnóstico:
[levantamento de 2026-10-13](levantamento-2026-10-13-concorrencia-escala-stack.md). Este
documento não repete o diagnóstico: transforma-o em onze épicos com definição de pronto,
prova e decisões.

## As três regras deste plano

1. **Iguala-se a FUNCIONALIDADE, não a arquitectura.** O que o Zoom faz com protocolo
   próprio e o Teams com a nuvem da Microsoft, nós fazemos de outra maneira ou não fazemos
   — e dizemos qual. Nenhum épico aqui pede que os dados saiam do servidor do cliente.
2. **Um épico fecha com prova medida, nunca com «está implementado».** Cada um abaixo tem a
   linha «Prova». Sem ela, o épico está aberto, mesmo com o código fundido — foi o que
   aconteceu ao ADR-0017, que esteve oito dias no código e desligado na instalação.
3. **Nenhuma diferença nossa regride para pagar paridade.** A lista das diferenças e o
   portão que guarda cada uma está no §3. Um épico que precise de quebrar uma delas para-se
   e vai a ADR.

---

## 1. O que falta, medido hoje

Onze épicos. A coluna «onde estamos» é medida nesta árvore, não recordada.

| # | Épico | Onde estamos, medido | Bloqueia |
|---|---|---|---|
| **P1** | **SCIM 2.0** (e SAML 2.0 depois) | `grep -i saml\|scim` em `server/src`, `server/crates` e `server/migrations` = **zero**. Há OIDC a sério (`auth.rs`, `org_sso_configs`), prova de domínio por DNS TXT (`0110`), e um ciclo de vida completo **para o Odoo** (`odoo.rs:396` cria/actualiza/muda papéis, `:517` suspende quem saiu) | a primeira reunião técnica de qualquer empresa com 200+ pessoas |
| **P2** | **Convite externo e calendário** | `meeting_invitees` tem só `(meeting_id, user_id)` (`0004_library_and_calendar.sql:35-39`): **não existe convidado externo**. O correio sai (ADR-0025, `mail.rs`) mas `mail.rs:466` é texto simples — **sem parte `text/calendar`**. O `.ics` existe como descarga (`meetings.rs:2007`) e já sobe o `SEQUENCE` ao remarcar (`:2098`). `graph.microsoft\|googleapis\|caldav` = zero | o uso diário de todos |
| **P3** | **Canais persistentes de equipa** | não existe nenhum. E o chat da sala, que existe e é persistido (`room_chat_messages`, `0018`), **é apagado ao fim do dia por desenho** (`room_chat.rs:10-12`, `:220`) | o argumento central do Teams |
| **P4** | **Legal hold e eDiscovery** | `legal_hold\|ediscovery` = zero. Existe retenção por dias, auditoria encadeada com verificação, e DLP de três expressões (`dlp.rs:5-25`). A pesquisa global (`search/global.rs`) respeita a visibilidade de cada pessoa — não serve um regulador | banca, seguros, governo — o segmento da soberania |
| **P5** | **Móvel que entra numa reunião** | `mobile/delonixphone/` existe (Flutter, 38 ficheiros Dart) e é **softphone SIP**: as dependências são `xml`, `crypto`, `flutter_secure_storage`, `mobile_scanner` — **não há `flutter_webrtc`**. O `cliente_meet.dart` regista o aparelho e a organização, não entra. PWA sem push com a app fechada (`vapid\|pushManager` = zero) | a adopção de quem vive no telemóvel |
| **P6** | **Modo webinar** | `webinar` = zero em `server/src` e `web/src`. As peças estão todas: `rooms.format` (`rooms.rs:40`), sala de espera (500), sondagens, Q&A, palco com `Spotlight` (R225), RTMP (`broadcast.rs`), e o SFU já só reencaminha 3 áudios (`sfu.rs:474`) | o evento que justifica o orçamento anual |
| **P7** | **SDK e apps na reunião** | a matéria-prima está feita: OpenAPI gerado, OAuth 2.1 (`oauth.rs`, `0117`), MCP com 17 tools, `bots:join` (`apikeys.rs:512`). Mas a v1 são **9 caminhos e 12 operações** (contadas em `docs/reference/openapi/v1.json`) contra 391 na BFF, e não há SDK publicado | integradores e revendedores |
| **P8** | **PBX a sério** | `call_queue\|voicemail\|auto_attendant` = zero. Existe IVR de entrada, ramais, provisionamento por QR, troncos, plano de marcação, CDR e custo (ADR-0009), e dial-out de ramal (`dial_outs.rs`) | competir com Zoom Phone e Teams Phone |
| **P9** | **IA ao vivo na reunião** | pós-reunião é forte (acta em fila com trava, capítulos, recortes); ao vivo há transcrição e **tradução de legenda por ouvinte**. Nenhuma das 17 tools do MCP é ao vivo: não há assistente a quem se pergunte durante a chamada | a comparação com o Copilot |
| **P10** | **Salas de hardware e SIP de vídeo** | `h.323\|poly\|cisco` = zero. O domínio já tem o vocabulário (`Channel::SipRoom`, `Channel::IpCamera`, `channels.rs:34,36`) e **nada o produz**. O bloqueio é técnico: o gravador é VP8 (`recorder.rs:33` usa `Vp8Packet`) e a E2EE assume VP8 (`e2ee.ts:2`) — uma Poly fala H.264 | salas de reunião de empresas grandes |
| **P11** | **Polimento** | enquadramento automático não existe, **mas a dependência já cá está**: o `@mediapipe/tasks-vision` serve a segmentação de pessoa (`web/src/room/immersive/personSegmenter.ts`, `media.ts`). Reacções: cinco emoji (`useChat.ts:17`) | nada; é a distância que se sente a usar |

---

## 2. Os épicos, um a um

Cada um: **pronto** (o que conta como igualar), **trabalho**, **prova**, **decisões** e
**o que não fazemos**.

### P1 — SCIM 2.0, depois SAML 2.0

- **Pronto:** um administrador liga o Delonix ao directório da empresa (Entra ID, Okta,
  Google) e uma pessoa criada lá aparece aqui com o papel certo; uma pessoa desactivada lá
  **perde a sessão aqui**; um grupo mapeia um `employee_group`.
- **Trabalho:** `/scim/v2/{Users,Groups}` com os filtros que os provedores usam, token por
  organização com escopo próprio, mapeamento papel↔grupo, e o `DELETE`/`active:false` a
  chamar o caminho de suspensão que **já existe** (`org::suspend_odoo_leavers`, hoje só
  usado pelo Odoo — generaliza-se, não se copia). SAML 2.0 é uma segunda PR: metadata,
  assinatura, `AuthnRequest`, e as mesmas tabelas de sessão.
- **Prova:** um provedor a sério em teste (conta gratuita de Entra ID ou Okta) a criar,
  mudar de papel e desactivar, com a sessão do alvo a morrer no acto; mais o
  `isolamento.mjs` a cobrir as rotas novas (é a catraca que o repo já impõe).
- **Decisões:** quem é o dono do papel quando o directório e o Delonix discordam; e se uma
  pessoa que o directório não conhece pode continuar a existir aqui (hoje pode).
- **Não fazemos:** SCIM como fonte de verdade de organizações — a organização nasce aqui ou
  no Odoo, não num directório alheio.

### P2 — Convite externo com `.ics`, depois calendário nos dois sentidos

Duas fases, e a primeira é a melhor relação esforço/venda de todo o plano.

- **Fase 1, pronto:** convido `maria@outra-empresa.ao` para uma reunião; ela recebe um email
  com o link e **um anexo `text/calendar`** que entra no Outlook ou no Google; remarcar
  envia uma actualização que substitui o evento em vez de criar outro (o `SEQUENCE` já sobe,
  `meetings.rs:2098`).
- **Fase 1, trabalho:** coluna de convidado externo em `meeting_invitees` (email + token, sem
  conta), `mail.rs` a emitir multipart com `METHOD:REQUEST`/`CANCEL`, e a decisão de endereço
  provado (hoje a notificação exige endereço verificado, o que um externo nunca tem).
- **Fase 2, pronto:** quem usa Outlook ou Google vê as reuniões do Delonix na sua agenda e
  cria reuniões do Delonix de lá, sem copiar links.
- **Fase 2, trabalho:** CalDAV primeiro (padrão, serve Apple e Google) e Microsoft Graph
  depois; sincronização incremental com `syncToken`, e a quarentena de colisão que já existe
  (`0006_collision_quarantine.sql`).
- **Prova:** uma caixa de correio externa a sério (não um mock) a receber o convite, a aceitar
  e a receber a actualização; e, na fase 2, um evento criado no Outlook a aparecer aqui em
  menos de um minuto.
- **Decisões:** o Graph exige registo de aplicação na Microsoft — isso cria uma dependência de
  um fornecedor para um produto que se vende como soberano. Fica dito: é opcional e por
  organização, nunca no caminho de ninguém que não a ligue.

### P3 — Canais persistentes, e o chat que se apaga

- **Pronto:** uma equipa tem um canal com histórico que sobrevive às reuniões, pesquisável, e
  as reuniões do canal aparecem nele.
- **Trabalho:** tabelas próprias (não reaproveitar `room_chat_messages`, que tem semântica de
  sala), API de mensagens com cursor, ligação reunião↔canal, e **a decisão de retenção**: hoje
  a varredura apaga o chat da sala ao fim do dia (`room_chat.rs:220`) e isso é uma promessa
  escrita na migração `0018`. Um canal com retenção de um dia não é um canal.
- **Prova:** uma mensagem escrita hoje, lida amanhã, depois de a varredura correr — e a prova
  de que o chat de SALA continua a obedecer à sua própria política.
- **Decisões:** retenção por omissão de um canal (sugestão: a da organização, como as
  gravações) e se o canal é por `employee_group` ou um objecto novo.

### P4 — Legal hold, eDiscovery e DLP por organização

- **Pronto:** um oficial de conformidade coloca um *hold* numa pessoa ou num caso; a partir daí
  nada do que ela produz é apagado pela retenção; e pode pesquisar transversalmente
  (gravações, actas, chat, auditoria) com cada acesso registado.
- **Trabalho:** tabela de *holds* com âmbito, o `retention_sweep` a respeitá-los (um `JOIN`,
  não uma cópia da regra), um papel de conformidade separado do administrador, uma superfície
  de pesquisa que **ignora a visibilidade pessoal** e por isso **tem de registar cada
  consulta** na auditoria imutável, e política de DLP por organização com registo de
  ocorrências.
- **Prova:** uma gravação sob *hold* que a varredura tenta apagar e não consegue, com a
  tentativa registada; e uma consulta de conformidade que aparece na trilha de auditoria.
- **Decisões:** quem pode ser oficial de conformidade (não pode ser quem administra, ou a
  trilha não vale nada), e se o operador da plataforma vê alguma coisa (proposta: não).
- **Não fazemos:** dar ao operador da plataforma acesso ao conteúdo dos inquilinos. Isso
  mataria o argumento da soberania para ganhar uma caixa de inspecção.

### P5 — Móvel que entra numa reunião

- **Pronto:** no Android e no iOS, a pessoa recebe a chamada **com a aplicação fechada**, entra,
  ouve, é ouvida, vê e partilha ecrã; e o mesmo binário continua a ser o softphone SIP que já é.
- **Trabalho:** `flutter_webrtc` no `mobile/delonixphone` (hoje não há WebRTC nenhum: as
  dependências são `xml`, `crypto`, `flutter_secure_storage`, `mobile_scanner`), o fluxo de
  entrada contra a API que já existe, e o push a chegar de verdade — hoje só o provider `lab`
  envia (`voice_devices.rs:16-21`) e «nenhum push chegou a um telemóvel real» (`:21`). iOS
  exige CallKit e PushKit, que é trabalho próprio.
- **Prova:** uma chamada atendida num telefone real com a aplicação fechada, nos dois sistemas,
  com áudio nos dois sentidos — e a bateria medida numa chamada de 20 minutos.
- **Decisões:** o motor SIP do DelonixPhone depende de licença comercial Belledonne (ADR-0022,
  cotação por pedir). Entrar numa reunião **não** precisa dela: é WebRTC. Vale decidir se o
  binário de reunião sai antes do softphone, para não ficar preso a uma negociação.
- **Não fazemos:** uma aplicação móvel que seja a PWA numa WebView — não resolve o push nem a
  bateria, e é o que já temos.

### P6 — Modo webinar

- **Pronto:** 200 pessoas assistem sem publicar, com inscrição, painel de oradores, Q&A
  moderado e sondagens; o anfitrião promove alguém da audiência a orador e volta a baixá-lo.
- **Trabalho:** papéis assimétricos na sala (hoje há anfitrião, convidado e serviço), uma
  audiência que **não negocia envio** no SFU, inscrição com página pública, e a moderação do
  Q&A que já existe por trás. A escala da audiência é o ponto: 200 subscritores de 3 oradores
  são 600 fluxos, o que a medição de 2026-10-05 já cobriu (1560 fluxos a 11 núcleos) — mas num
  pod de 4 CPU, não.
- **Prova:** 200 clientes de audiência medidos no `loadgen` **em Kubernetes**, com a entrada
  abaixo de 3 s e perda abaixo de 2%; e a decisão escrita de quantos cabem por pod.
- **Depende de:** a Onda 1 do levantamento (admissão, limites, tecto por sala medido). Sem
  isso, o webinar promete o que a infra-estrutura não dá.
- **Não fazemos:** audiência de milhares no nosso SFU. Acima de ~500 a resposta é o directo
  (RTMP/HLS) e dizê-lo em vez de o esconder.

### P7 — SDK publicado e apps na reunião

- **Pronto:** um integrador instala um pacote (`npm` e `pub`), autentica-se com OAuth 2.1 e
  marca uma reunião, lê a acta e põe um bot na sala sem escrever um cliente HTTP.
- **Trabalho:** gerar os clientes do OpenAPI que já existe (TypeScript e Dart), **alargar a v1**
  — hoje 9 caminhos e 12 operações contra 391 na BFF: faltam participantes, gravações por sala,
  webhooks e telefonia —, e os 7 eventos de webhook crescerem com o que um integrador espera.
  Apps dentro da reunião é uma segunda fase (painel com `iframe` e uma API de sala restrita).
- **Prova:** um integrador externo (ou nós, num repositório separado, sem olhar para este
  código) a fazer o percurso completo com o pacote publicado.
- **Decisões:** os dois escopos que hoje se consentem e não fazem nada (`shorts.publish`,
  `live.translate`) saem ou ganham tools — um consentimento vazio é uma promessa falsa.

### P8 — PBX: filas, atendedor automático, correio de voz

- **Pronto:** um cliente compra o Delonix e desliga a central: fila de atendimento com música e
  posição, atendedor automático com horário, correio de voz com transcrição e envio por email.
- **Trabalho:** estado das filas e das caixas na base, o plano de marcação a encaminhar para
  elas, Lua no FreeSWITCH para a experiência, e a transcrição a reutilizar o Whisper que já
  temos. Quase nada disto é novo: é a camada acima dos ramais, troncos, DID e CDR que já estão.
- **Prova:** uma chamada real que entra numa fila, espera, é atendida, e outra que cai no
  correio de voz e chega por email transcrita — contra um FreeSWITCH a sério, como manda a
  skill da telefonia.
- **Não fazemos:** gravação de chamadas por omissão. É um botão com consequências legais.

### P9 — IA ao vivo na reunião

- **Pronto:** durante a chamada, uma pessoa pergunta «o que já se decidiu?» e recebe a resposta
  do que foi dito até ali, sem nada sair do servidor.
- **Trabalho:** contexto incremental da transcrição que já corre, uma tool ao vivo (as 17 do MCP
  são todas pedidos ao BFF, nenhuma é de sala), e o tecto de concorrência no Ollama — que hoje
  já é o item mais pesado do namespace (4 CPU / 8Gi).
- **Prova:** a pergunta respondida em menos de 10 s numa reunião de 30 minutos, com a acta a
  continuar a sair dentro do tempo; e a medição do custo em GPU.
- **Depende de:** fechar o defeito da janela da acta (`num_ctx`), que já está aberto.

### P10 — Salas de hardware e SIP de vídeo

- **Pronto:** uma Poly ou uma Cisco de sala entra numa reunião por SIP, com vídeo.
- **Trabalho:** H.264 no caminho de media — e aqui está o custo real: o gravador depacketiza
  **VP8** (`recorder.rs:33`) e a E2EE assume VP8 (`e2ee.ts:2`), por isso aceitar H.264 obriga a
  decidir o que acontece à gravação e à E2EE dessas salas. Mais: INVITE entrante para uma sala,
  perfil de endpoint, e DTMF.
- **Prova:** um endpoint SIP de vídeo real (ou um `pjsua` com H.264) dentro de uma reunião, com
  a gravação a sair válida.
- **Decisões:** é o épico mais caro da lista e o que menos clientes nossos pedem hoje. Proposta:
  **fica para o fim**, e enquanto não houver, diz-se que não há.

### P11 — Polimento

- **Pronto:** enquadramento automático do orador, galeria completa de reacções, e as pequenas
  coisas que se sentem ao usar.
- **Trabalho:** o enquadramento é barato porque a dependência já cá está — o
  `@mediapipe/tasks-vision` já serve a segmentação (`room/immersive/personSegmenter.ts`): é
  acrescentar detecção de rosto e recorte. As reacções são uma lista (`useChat.ts:17`).
- **Prova:** o enquadramento medido em CPU no portátil mais fraco que temos, e a prova de que
  desliga sozinho quando a bateria ou o CPU apertam (a política de camadas já faz isso).

---

## 3. As diferenças que se defendem, e o portão que as guarda

Paridade não se paga com isto. Cada linha tem o portão que falha se alguém a quebrar.

| Diferença | O que a guarda hoje |
|---|---|
| **Soberania: nada sai do servidor** | zero chamadas a nuvens de IA em `server/src`, `ai-worker/` e `whisper-server/` (medido); `check-capability-claims.sh` |
| **Self-host num comando** | `check-helm.sh`, `check-k8s-render.sh`, `check-replicas-compose.sh` |
| **IA local** | sem `OLLAMA_URL` a IA desliga em vez de degradar para uma nuvem (`ai.rs:634`) |
| **Isolamento entre organizações** | `check-isolamento-cobertura.sh` — 190 rotas, e cresce com cada rota nova |
| **Voz e PBX no mesmo produto, sem licença por posto** | `check-fs-xml.sh`, `check-lua-sintaxe.sh`, as provas contra FreeSWITCH real |
| **Superfície MCP para agentes** | `check-mcp-tools.sh` |
| **Afinidade por sala (o que faz o SFU funcionar)** | `check-room-affinity.sh`, e o chart recusa desligá-la em produção |
| **Sem capacidade afirmada sem código** | `check-capability-claims.sh`, `check-docs-drift.sh` |

**Três épicos tocam numa diferença e por isso levam ADR antes de código:** o P2 fase 2
(dependência da Microsoft), o P4 (quem pode ler o que não é seu) e o P10 (H.264 contra a
gravação e a E2EE que são VP8).

---

## 4. Sequência

Quatro ondas. A regra é a do levantamento: não se promete numa onda o que a infra-estrutura
da anterior não sustenta.

| Onda | Épicos | Porque esta ordem | O que desbloqueia |
|---|---|---|---|
| **A** | infra Onda 1 (do levantamento) · **P2 fase 1** · **P1 (SCIM)** · **P3 fase 1** (parar de apagar o chat) · **P4 fase 1** (*hold*) | são os quatro «não» que se ouvem antes do preço, e nenhum depende de outro épico | a conversa com uma empresa de 200+ pessoas e com um regulado |
| **B** | **P6 (webinar)** · **P3 fase 2** (canais) · **P7 (SDK e v1)** · **P2 fase 2** (calendário) | o webinar precisa do tecto medido da onda A; os canais precisam da decisão de retenção | o orçamento anual de eventos e os integradores |
| **C** | **P5 (móvel)** · **P9 (IA ao vivo)** · **P8 (PBX)** | são os três que mudam o produto no mercado, e os três mais caros | o mercado de quem vive no telemóvel e o de quem desliga a central |
| **D** | **P10 (salas de hardware)** · **P11 (polimento, contínuo)** | o P10 é o mais caro e o menos pedido | empresas com salas equipadas |

**Quadro de paridade.** As 14 linhas da tabela de `competitive-positioning.md` passam a
ter-se de re-medir a cada onda, com data e `ficheiro:linha` — e o levantamento de 2026-10-13
mostrou porquê: três dessas linhas estavam erradas, duas contra nós.

## 5. O que não fazemos, e a razão

- **Protocolo de media próprio** (como o Zoom). Fecharia a diferença da rede má e matava o
  browser-first, que é o nosso alcance. A resposta é FEC dentro do que o WebRTC permite.
- **Controlo remoto do ecrã** sem agente nativo. Já está recusado (R109) e mantém-se: o
  encaminhamento de input não pode existir só no browser.
- **Audiência de milhares no SFU.** Acima de ~500 é directo, e diz-se.
- **IA na nuvem de outro, nem como opção «para quem quiser».** É a diferença que mais vale.
- **Operador da plataforma a ver conteúdo de inquilinos**, nem para eDiscovery.

## 6. O que este plano não resolve

- **Não tem estimativas de tempo.** Cada épico tem âmbito e prova; quem o faz dá o prazo
  quando o abrir — e os três de Onda C são trabalho de meses, não de sprints.
- **Não foi validado com clientes.** A ordem é por «o que bloqueia uma venda» medido nas
  conversas que já tivemos, não por pedidos escritos.
- **As capacidades dos concorrentes não foram re-verificadas** nesta sessão: é conhecimento
  público e confirma-se antes de entrar numa proposta. Só o nosso lado está medido.
- **A infra-estrutura continua à frente na fila.** O tecto de produção (~150 participantes
  pelo coturn, ~20 por sala) não é um épico de paridade, mas nenhum destes épicos vale nada
  sem ele.
