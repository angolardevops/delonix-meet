# Paridade medida — Delonix Meet contra o Zoom, o Teams e o Google Meet

**Medido a 2026-10-18** contra `develop` @ `f21e6fc47` (confirmado em `e48658f06`, que só acrescenta o `zip` 9.0.5).
Fila de execução: [`.claude/paridade/`](../.claude/paridade/) — um ficheiro por lacuna, pronto a virar contrato de sessão.

## Leia isto antes de citar um número

**O [`plano-paridade-2026-10-13.md`](plano-paridade-2026-10-13.md) está ERRADO em dois pontos verificados, cinco dias depois de ser escrito.** Dizia que SCIM era «zero» — há `server/src/scim.rs` com 899 linhas, rotas registadas em `lib.rs:456-466` e dez `#[sqlx::test]`. E dizia que `mail.rs:466` era texto simples «sem parte `text/calendar`» — tem, em `mail.rs:412`. Lê-se esse documento para INTENÇÃO (os onze épicos e as três regras continuam boas); não se citam os números dele.

Este documento tem a mesma validade: é uma fotografia de um commit. A diferença é que a fila ao lado diz como se remede cada linha.

**Convenções.** `[M]` = medido nesta árvore, com `ficheiro:linha`. `[CG]` = conhecimento geral sobre o concorrente, **não medido** — não temos como instrumentar os produtos deles, e dizê-lo é mais honesto do que fingir paridade de rigor.

## Resumo

| | |
|---|---|
| Capacidades avaliadas | **44** |
| Superamos | **11** |
| Ao nível | **14** |
| Atrás | **9** |
| Não existe | **10** |

A contagem não é um placar. Uma linha — participantes por sala — pesa mais do que as onze em que ganhamos, porque é a primeira pergunta de qualquer comprador.

**Escala do produto, para contexto `[M]`:** 335 rotas no router (`grep -cE '^\s*\.route\(' server/src/lib.rs`), 131 794 linhas de Rust em `server/src` + `server/crates`, 129 904 de TypeScript em `web/src`, 83 ficheiros de teste no servidor e 115 no frontend (1378 testes), 123 migrações, 31 ADR, 4 línguas, e implantação por compose, k8s, Helm, Ansible e Tofu.

---

## 1. Onde superamos

### 1.1 Soberania do dado — o trunfo mais sólido

| Peça | Medido `[M]` |
|---|---|
| Legendas ao vivo | Whisper-tiny WASM **no browser**, com `env.allowRemoteModels = false` (`web/src/whisperWorker.ts:1-11`); ou `whisper-server` soberano na rota `/asr` |
| Acta, resumo, capítulos, recortes, tradução | **Ollama in-cluster**, `{base}/api/generate` — e a funcionalidade **desliga-se** sem `OLLAMA_URL` em vez de recuar para uma nuvem (`server/src/ai.rs:1-3`) |
| Provedores de IA na nuvem | `grep -rinE 'openai\|anthropic\|azure\|bedrock\|vertexai\|generativelanguage'` em `server/` = **zero** |
| Saída HTTP | `reqwest::Client::builder()/new()` fora do `net_guard` = **zero**, por catraca (`server/src/net_guard.rs:100-344`) |
| Frontend | sem CDN, sem Google Fonts, zero URLs externos em `web/src`; modelos servidos de `web/public/models` |
| TURN | coturn próprio, sem STUN da Google |
| Phone-home / licença | não existe; OTLP só se o operador der `OTEL_EXPORTER_OTLP_ENDPOINT` |

**A única fuga medida**, e é deliberada: a Web Speech API do Chrome (`web/src/media.ts:633,911`) envia áudio à Google. É o **último** motor da ordem de preferência, por soberania escrita (`web/src/legendasPref.ts:58-68`), e o ecrã diz-o em vez de prometer «processamento local». **Ressalva honesta:** não há política do lado do servidor que a proíba — ver `A-07` na fila.

`[CG]` Zoom, Teams e Meet oferecem *escolha de região de datacenter*. O conteúdo é sempre processado pelo fornecedor. É uma diferença de natureza, não de grau.

### 1.2 E2EE que custa menos do que a do Zoom

`web/src/e2ee.ts:1-12` — Insertable Streams, AES-GCM por frame com header autenticado como AAD, chave por PBKDF2 de uma frase fora da plataforma que **nunca chega ao servidor**, e **fail-closed nos dois sentidos**: sem chave o frame é descartado, nunca emitido em claro (`:28-35,:53-56`).

**O que se perde:** auto-gravação desligada (`recorder.rs:1750`); gravação pelo servidor só por delegação de chave com consentimento explícito (`recorder.rs:1089-1143`); dial-out recusa (`dial_outs.rs:307`).
**O que NÃO se perde, e é o ponto:** legendas, tradução e acta continuam a funcionar, porque o ASR é do cliente e só a frase sobe (`dubbing.rs:5-11`).

`[CG]` O Zoom desliga transcrição ao vivo, breakouts e sondagens com E2EE activa. O Teams só cifra 1:1. O Meet não tem E2EE.

**Buraco conhecido:** o dial-IN não verifica `rooms.e2ee` — ver `A-01`, que é a lacuna mais urgente deste documento.

### 1.3 Auditoria verificável e isolamento como artefacto

Cadeia SHA-256 por organização, com **gatilhos de base de dados que recusam `UPDATE` e `DELETE`** (`server/migrations/0037_audit_logs_append_only.sql:76,86`), verificação que recalcula a cadeia em SQL (`audit.rs:163-205`), falha de escrita como ERRO com métrica própria, e o nome do actor fotografado no momento.

Ao lado: **349 chamadas** `recusado`/`recusadoNaPorta` em `web/e2e/isolamento.mjs` (1948 linhas) contra servidor e Postgres reais, com catraca (`scripts/check-isolamento-cobertura.sh`) que falha se uma rota com escopo de organização não for exercitada.

`[CG]` Os três respondem a isto com SOC 2 / ISO 27001 — que nós **não temos** (ver `A-10`). A diferença é que uma certificação é um relato de terceiros e isto é uma prova que o cliente corre contra quem administra a base.

### 1.4 O estúdio de TV — não é paridade, é outra categoria

**25 901 linhas** de TS/TSX e **8 163** de Rust.

| Peça | Estado `[M]` |
|---|---|
| Corte PGM/PRÉ, T-bar, transições | feito (`web/src/studio/tv/mesa.ts:43-180`) — `tbarEmBaixo` modela o barramento invertido de uma mesa real |
| Tally | feito (`delonix-meet-domain/src/studio/tally.rs`) — fonte em PROGRAMA **e** em PRÉ fica PROGRAMA: «a luz vermelha é a que não pode mentir». Cada telefone recebe só o seu |
| Telemóvel como câmara | feito — CAM 1-16, controlos reais da óptica negociados contra as `capabilities`, emparelhamento por código/QR com tentativas limitadas |
| Mesa de som | feito, no browser por desenho (ADR-0014 §1: o DSP não entra no pod) |
| Superfície física | feito, por **Web MIDI** (`studio/tv/mesaFisica.ts`), com mapa fixo declarado |
| Directo RTMP | feito, **8 destinos com um processo ffmpeg por destino** — «um destino mau não termina os outros» (`broadcast.rs:1131`) |
| Editor não-linear | feito — timeline, bin, cor, render, demux WebM próprio, projecto em IndexedDB |
| Gravação ISO | **NÃO existe** — ver `A-02`, e está ligada por omissão |
| DMX / Hue | **NÃO existe, e está correctamente não-anunciado**: o ecrã recusa-se a nomear protocolos que nenhum código fala, guardado por `check-capability-claims.sh` |

`[CG]` O par de mercado não é o Zoom: é o **vMix** (Windows, licença perpétua) e o **OBS**. O telemóvel-como-câmara com tally compara-se com Larix/Teradek. Do lado dos três, o vizinho mais próximo é o Zoom Events — que é gestão de evento e registo, não régie.

### 1.5 As restantes

| Capacidade | Medido `[M]` | `[CG]` Eles |
|---|---|---|
| Dobragem de voz ao vivo | frase → tradução → TTS → **uma pista Opus por língua** no SFU; o que o DLP mascara na legenda fica mascarado na voz (`dubbing.rs:1-30`, ADR-0030 aceite a 2026-10-14) | Zoom só legendas; Teams intérprete humano; Meet nada |
| Fundos virtuais | RobustVideoMatting em WebGPU com recuo WASM — *alpha matte* contínuo (`web/src/matte.ts:3-4`) | segmentador binário nos três |
| O telefone como participante | censo com crachás medidos no RTP, `ForceMute` **imposto no servidor**, destaque que fixa o áudio no SFU e sobrevive ao selector de oradores (R224, R225, com controlo negativo) | um nome na lista |
| Tarifação de telefonia | preços por tronco, câmbios, custo **ao preço em vigor quando a chamada aconteceu** (R212), CDR idempotente, emergência nunca gravada nem travada | o Zoom Phone factura-te; não te dá o motor |
| Banda larga na perna do telefone | Opus do softphone atravessa até ao browser **sem recodificação** (ADR-0018, `phone_bridge/sip.rs:262-320`) | perna PSTN a 8 kHz |
| MCP para agentes | 17 ferramentas, e a lista é o cruzamento dos escopos do token com as capacidades **da pessoa** — «anunciar uma tool que vai responder 403 é vender o que não se tem» (`mcp.rs:334-355`) | o escopo é da aplicação |
| Rigor de i18n e ecrã estreito | ~6125 chaves com paridade entre 4 línguas **trancada por teste**; três portões contra texto fora do `t()` que apanharam 85 frases reais; layout a 375 px provado em browser real sobre o componente verdadeiro (R86) | ninguém publica equivalente |
| PWA com fronteira honesta | precache derivado do grafo do build; `/api`, `/ws` e `/rtc` **nunca** de cache, com a razão escrita (`web/public/sw.js:10-15`) | — |

---

## 2. Onde estamos ao nível

Catorze capacidades em que a diferença já não decide nada: gravação na nuvem e local, legendas e tradução ao vivo, IA pós-reunião (acta, capítulos, recortes), quadro branco com *stylus* (pressão, rejeição de palma, inclinação), sondagens, Q&A moderado, reacções, salas de grupo, convidado sem conta, anexo de calendário com `SEQUENCE`, chaves de API com escopos, bots na reunião, OAuth 2.1 com PKCE obrigatório, webhooks assinados, atalhos de teclado num catálogo único, e troncos SIP multi-inquilino.

Três com nota:

- **O chat da sala já não é apagado** — mudou a 2026-10-14 (`room_chat.rs:10-22`); o efémero é agora opção explícita por organização.
- **A gravação fica no disco do cliente** (`RECORDINGS_DIR`), não numa nuvem estrangeira: paridade de função, vantagem de repouso.
- **O convidado sem conta** entra com um campo e dois cliques, sem instalar nada (`web/e2e/convidado-ecra.mjs:84-89`) — ao nível do Meet, que é o melhor dos três nisto. Mas a sala de espera é **obrigatória** e sem interruptor (`guests.rs:144,151`): ver `B-08`.

---

## 3. Onde estamos atrás

Por ordem do que bloqueia uma venda. O id liga ao ficheiro da fila.

| id | Capacidade | Medido `[M]` | `[CG]` Eles |
|---|---|---|---|
| `A-03` | **Push com a app fechada** | `grep -ri "vapid\|pushManager\|web-push"` em todo o repo = **0**; o `sw.js` não tem handler de `push`; `notifications.rs:21-24` entrega só por WebSocket; `meetings_v1.rs:946` devolve `offline: [uuid]` para quem não tem socket. No móvel aceita-se o token FCM e `server/tests/voice_push_wake.rs:368` declara que **nada é enviado** — «não finge» | os três têm |
| `A-04` | **Participantes por sala** | **40 medidos**: 1560/1560 fluxos, 0,03 % de perda, **11,2 de 16 núcleos** (`docs/ops/teste-de-carga-2026-10-05.md:29`); a rampa parou por esgotar degraus, «onde a sala quebra não se sabe». O chart envia `ORG_MAX_PARTICIPANTS: "40"`. ADR-0001 fixa a sala a **um pod** porque o RTP não passa pelo Redis | Meet 500 · Zoom 1000 · Teams 1000 / 10 000 view-only |
| `A-05` | **Calendário nos dois sentidos** | `grep -ri "graph.microsoft\|googleapis\|caldav"` em código = **0**; os únicos resultados da árvore são documentos a declarar a ausência | nativo nos três |
| `A-06` | **Canais persistentes de equipa** | única rota de chat em todo o repo: `lib.rs:801` → `GET /api/rooms/{code}/messages`. O chat é sempre por sala; `room_chat.rs:16` admite que é «a que o Teams usa para vender» | é o produto do Teams |
| `B-01` | **Cliente móvel de reuniões** | `pubspec.yaml` **sem** `flutter_webrtc`; é softphone SIP cujo motor não existe em release (`release/.../MotorFabrica.kt:11` — «licença por assinar», AGPL); iOS é o template de 16 linhas; sem i18n | apps nativas |
| `B-02` | **Assistente de IA ao vivo** | não existe «o que perdi?»; a acta na sala é **heurística de regex** (`room/useTranscription.ts:24`), o LLM corre depois | AI Companion · Copilot · Gemini |
| `B-03` | **Pesquisa semântica** | FTS do Postgres com dicionário próprio e trigramas, incluindo transcrição — mas lexical: `grep pgvector\|embedding` = 0 | semântica nos três |
| `B-04` | **Partilha de ecrã fluida** | 1920×1080 com `frameRate: {ideal: 5, max: 15}` (`webrtc.ts:184-189`) — texto nítido, vídeo não serve. Em troca temos segunda fonte como câmara e partilha concedida por participante, que eles não têm | 30 fps; múltiplas em grelha |
| `B-05` | **Composição fora do nó de media** | 20 composições em paralelo levaram as chamadas vivas a **64 % de perda** (`teste-de-carga-2026-09-17.md:85-89`); o travão por omissão é `FFMPEG_MAX_CONCURRENT=1` — a confissão do problema | compõem fora do pod de media |
| `B-06` | **Entrada na sala acima de 20 pessoas** | `reevaluate_room` percorre os pares **em série com `.await`** (`sfu.rs:1689-1703`); a entrada sobe de 0,3 s com 8 para **1,7 s com 40** | Meet < 1 s |
| `B-07` | **API pública e SDK** | v1 = **9 caminhos / 12 operações** contra **282 / 404** da BFF — 3 % da superfície. SDK: não existe, e já foi afirmação falsa apanhada pela R85 | Graph; SDKs em 4 plataformas |
| `A-07` | **MFA obrigatório e lockout central** | `require_mfa` só impede remover o **último** factor (`identity/factors.rs:39-55`): quem nunca inscreveu entra com password. O travão de tentativas é **em processo** (`rate_limit.rs:22-37`) — com N réplicas, 8 tentativas tornam-se 8N | Conditional Access · smart lockout |
| `B-09` | **Acessibilidade com prova** | 849 `aria-*` em 161 ficheiros e 24 `aria-live` — mas `grep axe` = **0**, e `prefers-contrast`/`forced-colors` = 0 | VPAT nos três |

---

## 4. Não existe

| id | O que falta | Medido `[M]` | O que custa |
|---|---|---|---|
| `A-08` | **Legal hold e eDiscovery** | `grep -ri "legal_hold\|ediscovery"` = **0**. Há retenção por dias, auditoria encadeada e DLP; não há *hold* que suspenda a retenção, pesquisa transversal para um regulador, nem exportação ao nível da organização (`search/global.rs` respeita a visibilidade de cada pessoa — não serve um regulador) | Contra o Purview e o Vault não é «atrás», é «não concorre». É o que fecha a venda **no segmento que a soberania abre** — banca, seguros, governo |
| `A-09` | **Modo webinar** | zero código. `broadcast` e `hybrid` são aceites e persistidos (`ROOM_FORMATS`, `rooms.rs:123`) e **só o `training` muda comportamento** (`rooms.rs:38`). As peças existem todas: sala de espera, `Role::Attendee`, palco que fixa áudio no SFU, Q&A com destaque, sondagens, RTMP. `grep hls` no servidor = 0 | a funcionalidade mais pedida por quem compra eventos |
| `C-01` | **PBX: tratamento de chamadas** | filas (`call_queue`), voicemail/MWI, atendedor multinível, supervisão/escuta/irrupção, espera, estacionamento, grupos de toque, BLF: **todos zero por grep**. Transferência **activamente desligada** (`disable-transfer=true`, por decisão escrita, R292). A gravação de chamada escreve para um disco que **nenhuma rota lê** | é exactamente o produto que o Zoom Phone e o Teams Phone vendem |
| `C-02` | **Softphone no nosso produto** | `grep sip.js\|jssip\|webphone` em `web/src` = 0; e o build de release do DelonixPhone não leva motor SIP | a tese «uma app para reuniões e chamadas» não está servida |
| `C-03` | **Bordo de operadora** | nenhuma operadora real alguma vez entregou uma chamada (T5); sem `P-Asserted-Identity`; sem `session-expires`/`rtp-timeout` nos perfis; sem tecto de gasto nem tranca de internacional; chamadas de ENTRADA por tronco não deixam registo | um ramal com password roubada liga para tudo o que o plano deixar |
| `C-04` | **Salas de hardware (Poly, Cisco, H.323)** | zero em código. **O bloqueio não é o codec**: o SFU já registra seis perfis de H.264 (`sfu.rs:900-901`). É não haver gateway SIP↔WebRTC de **vídeo** — a ponte é só áudio — mais o gravador que só sabe VP8 (`sfu.rs:2185`) e a E2EE que assume VP8 | fecha as salas de reunião de empresas grandes. É um projecto, não um `fmtp` |
| `A-11` | **SAML 2.0** | `grep -i "saml\|AuthnRequest"` = 0. O OIDC existe e é bom, com `enforce_sso` só sobre domínio verificado por DNS TXT | clientes com IdP antigo (ADFS, Shibboleth) só falam SAML |
| `B-10` | **SCIM acima de 200 pessoas, e `/Groups`** | `startIndex` e `count` são **ignorados** (`scim.rs:344-352`) e `totalResults` devolve `recursos.len()` com `MAX_RESULTS = 200`: numa org de 500, o Entra lê 200 e **o campo confirma-lhe que são 200** — truncatura silenciosa, não erro. `/Groups` não tem rota | o bloqueador nº1 de um provedor real, e a correcção mais barata de alto valor desta lista |
| `B-11` | **App de desktop** | zero Electron, zero Tauri. Só browser e PWA | atrito, e **defensável** — é até um argumento contra os ~500 MB do Zoom |
| `A-10` | **Certificações** | nenhum artefacto de SOC 2, ISO 27001, BAA de HIPAA ou FedRAMP na árvore | é o portão de compras real no segmento que a soberania abre. Não é código |

---

## 5. Três afirmações a um passo de ser falsas

Isto não é comparação — é dívida de honestidade, e corrige-se antes de pôr qualquer tabela destas à frente de um cliente.

1. **A gravação ISO está ligada por omissão e não existe.** `iso_recording BOOLEAN NOT NULL DEFAULT TRUE` (`migrations/0081_studios.sql:12`), com interruptor e ecrã — e `recording_tracks` dá **zero** no servidor e nas migrações. O contrato do estúdio nomeia a tabela e as duas rotas como por implementar. → `A-02`
2. **Dois formatos de sala que não fazem nada.** `broadcast` e `hybrid` validados e persistidos, nenhum lido para decidir comportamento. → `A-09`
3. **Um telefone entra numa sala cifrada de ponta a ponta.** O dial-OUT recusa com razão escrita — «uma sala cifrada de ponta a ponta não aceita um telefone: a ponte não decifra» (`dial_outs.rs:307`) — e o dial-IN não chega a olhar: `grep -c e2ee server/src/voice.rs` = **0**. → `A-01`

## 6. O que fazemos melhor do que os três e não cabe numa tabela

O repositório diz a verdade sobre si próprio, por construção. O ecrã de iluminação **recusa-se a nomear DMX** porque nenhum código o fala, e há um portão que rejeita a palavra. O ecrã de IA avisa que a Web Speech **envia o áudio à Google** em vez de dizer «processamento local», e põe esse motor em último lugar. O módulo do canal de TV escreve «**pedir não é estar no ar**» e proíbe a interface de apresentar uma emissão que ainda não existe. O erro do LLM tem cinco causas distintas em vez de «algo correu mal».

Isto não vende numa tabela de funcionalidades. Vende numa auditoria pública — e é a razão pela qual esta medição foi possível em meia hora em vez de uma semana.

## 7. Quem compra isto hoje

Não a empresa que quer um Zoom mais barato: essa compara 40 contra 1000 e vai-se embora.

**A estação de televisão, rádio ou canal institucional lusófono ou africano** — uma TPA provincial, um canal de assembleia, uma diocese, uma universidade com canal próprio — que hoje produz um directo com `vMix + OBS + Zoom + YouTube` e que, por lei ou política, **não pode pôr o sinal de um plenário num Zoom Events alojado fora do país**. Para esse cliente o estúdio é a única razão pela qual o produto entra na sala, e é o único segmento onde as 40 pessoas por sala não são o critério de compra.

O segundo comprador é **o Estado e o sector regulado**, pela soberania medida — e é aí que a falta de *legal hold* (`A-08`) dói mais, porque é a porta que a soberania abre e que o eDiscovery fecha.

## 8. Método, e o que não está provado

As colunas do Delonix são medidas nesta árvore com `ficheiro:linha`: greps de ausência, contagens lidas dos JSON de OpenAPI commitados, números tirados dos relatórios de carga do próprio repositório, e `npx tsc --noEmit` e `npx vitest run` corridos (115 ficheiros, 1378 testes, verdes). As colunas dos concorrentes são **conhecimento geral, não medido**.

Não está provado: nenhum e2e foi corrido nesta medição (precisam de Postgres, servidor e `dist` servido); nada foi visto em telemóvel real, iOS, Safari ou Firefox; a suite Rust não correu por falta de `DATABASE_URL`; os tamanhos de bundle vêm de um `dist` de 15-10; e os três achados de segurança do §5 estão **confirmados por leitura, não explorados**.
