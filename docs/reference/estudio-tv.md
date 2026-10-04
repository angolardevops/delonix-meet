# Contrato do Estúdio de TV (Navegavel5) — backend → UI

**Frente:** backend «v5: estúdio de TV» (`delonix-meet-backend/v5-estudio-tv`).
**Autoridade:** [ADR-0014 «Estúdio de TV num PC»](../adr/0014-estudio-de-tv-num-pc.md).
**Ecrãs servidos:** DelonixSources, DelonixSwitcher, DelonixAudioMixer, DelonixLighting,
DelonixStudioLive, DelonixPhoneCam.
**Estado medido:** 2026-09-24, sobre `origin/main` `b6b769d` (PR #120 fundido).
As secções 2, 3, 4 e 6 estão `FEITO` — commitadas, com testes de integração contra
Postgres e servidor reais e casos de isolamento em `web/e2e/isolamento.mjs`. A 5 está
`PARCIAL` e as 7 e 8 continuam `CONTRATO`.

**Nota sobre a cópia de trabalho:** o `notas-ui-template/` referido na primeira versão
deste texto **não existe em nenhum ramo** — nem na `main`, nem na `feat/console-ui-template`.
Este ficheiro é o único texto do contrato; não há segunda cópia a divergir.

**Re-medido a 2026-10-04 sobre a `develop`:** a pasta `studio-agent/` (§0 e §8) **não
existe em nenhum ramo de `origin`** — o agente local de iluminação e os adaptadores
Art-Net e Hue que o §8.3 dá como «provados» não estão no repositório. E nenhum ecrã
consome as rotas `/api/orgs/{org_id}/studios…` das secções `FEITO`: no web há zero
chamadas (plano de lacunas, TV1). Lê `FEITO` como «o servidor tem a rota e o teste»,
não como «o operador consegue usar».

> **Estado.** Cada secção diz se está `CONTRATO` (acordado, por implementar), `FEITO` (commitado e com teste) ou
> `EXTERNAL` (depende de hardware que não existe nesta máquina). A UI só consome o que
> está `FEITO`. O OpenAPI gerado (`docs/reference/openapi/bff.json` e
> `integrations.json`) ganha a este texto quando divergirem.

## 0. Divisão de trabalho (resumo do ADR-0014)

| Onde corre | O quê | Porquê |
|---|---|---|
| **Browser** (operador) | corte PGM/PRÉ, transições (cortar/misturar/limpar/stinger), T-bar, sobreposições, mistura de som (ganho, EQ, dinâmica, barramentos, LUFS), correcção de imagem por software, composição do programa, codificação H.264 do directo | o compositor (`web/src/studio/compositor.ts`) e o motor de exportação já existem; o servidor **não** recodifica vídeo que o browser já compõe (ADR-0003, opção C) |
| **Servidor** | emparelhamento do telefone, token de fonte, tally e comandos pelo WebSocket da sala, gravação ISO por fonte (cópia, sem recodificar), persistência das cenas/macros/sobreposições/luz/perfis/alinhamento, estado do directo, fila de comandos de luz | é o que precisa de identidade, autorização, persistência ou do SFU |
| **Agente local** (`studio-agent/`) | DMX por Art-Net (UDP), Philips Hue na LAN (HTTP), inventário de aparelhos | o servidor não vê a LAN do estúdio — o mesmo facto do ADR-0005 com o USB |

## 1. Regras comuns

- Erros: envelope plano `{"error", "code", "details", "request_id"}`. `404` a quem não
  chega ao recurso (inclui outra org), `403` com `code` a quem chega mas não pode. Nunca
  `401` por falta de permissão.
- Listas: `Page<T>` = `{items, next_page_token}`, `page_size` 1–100 (omissão 50).
- Ids UUID em string; datas RFC 3339 UTC; `snake_case`.
- **Quem pode** (capacidades PROPOSTAS para o catálogo da frente A; hoje verificadas com
  `org::role_in_org`):

| Capacidade proposta | Verificação hoje (main) | Recusa |
|---|---|---|
| `studio.view` | membro activo da org | `404` |
| `studio.operate` | administrador da org **ou** quem criou o estúdio (`created_by`) | `403 studio.not_operator` |
| `studio.manage` | administrador da org | `403` (`forbidden`) |
| (WebSocket) comandar fontes | anfitrião **actual** da sala do estúdio (`hub.is_host`, sobrevive a `transfer-host`) | mensagem descartada + `error` |

## 2. Estúdios — `FEITO`

Um estúdio é da organização e tem **uma sala SFU própria** (criada com ele, `topology=sfu`,
`e2ee=false` — o directo recusa E2EE, ADR-0003).

| Método e caminho | Quem | Corpo → resposta |
|---|---|---|
| `GET /api/orgs/{org_id}/studios` | view | `200 Page<Studio>` |
| `POST /api/orgs/{org_id}/studios` | manage | `{name, iso_recording?}` → `201` + `Location` + `Studio` |
| `GET /api/orgs/{org_id}/studios/{studio_id}` | view | `200 Studio` |
| `PATCH /api/orgs/{org_id}/studios/{studio_id}` | operate | `{name?, iso_recording?}` → `200 Studio` |
| `DELETE /api/orgs/{org_id}/studios/{studio_id}` | manage | `204` |

`Studio = {id, org_id, name, room_code, iso_recording, created_by, created_at, updated_at}`.
Erros: `400 studio.invalid_name` (1–80 caracteres).

## 3. Emparelhar a app Delonix Câmara (PhoneCam) — `FEITO`

### 3.1 O operador gera um código

`POST /api/orgs/{org_id}/studios/{studio_id}/pairing-codes` (operate)
`{label?: "telefone da Ana", number?: 2}` → `201`

```json
{"id":"…","code":"K7QD-4M9Z","expires_at":"…","number":2,"label":"telefone da Ana",
 "max_attempts":5,"room_code":"abc-defg-hij"}
```

- **Formato:** `XXXX-XXXX` do alfabeto Crockford base32 sem ambíguos
  (`0123456789ABCDEFGHJKMNPQRSTVWXYZ`); o hífen e as minúsculas aceitam-se na entrada.
  Os 4 primeiros localizam o código; os 4 últimos são o segredo (comparação em tempo
  constante; só o hash SHA-256 é guardado).
- **Validade:** 10 min. **Uso único.** **5 tentativas** erradas queimam o código
  (os 4 primeiros certos + os 4 últimos errados contam como tentativa).
- `number` (CAM n): 1–16; omisso → o menor livre no estúdio. `409 studio.source_number_taken`
  se já houver uma fonte activa com esse número.
- O código em claro só aparece nesta resposta (a UI mostra-o e gera o QR a partir dele).

`GET …/pairing-codes` → `Page<PairingCode>` (sem o código; `{id, number, label,
expires_at, attempts, max_attempts, consumed_at, state: active|consumed|expired|burned}`).
`DELETE …/pairing-codes/{code_id}` → `204` (revoga).

### 3.2 O telefone troca o código por um token de fonte (sem conta)

`POST /api/studio-pairings` — **rota pública** (em `scripts/rotas-publicas.txt`),
rate-limit por IP.

```json
{"code":"k7qd4m9z","device":{"model":"Galaxy S23","platform":"android","app_version":"1.0.0"}}
```

→ `201`

```json
{"source_id":"…","studio_id":"…","number":2,"label":"CAM 2 · telefone da Ana",
 "room_code":"abc-defg-hij","source_token":"eyJ…","expires_at":"…",
 "ws_path":"/ws?token=eyJ…&room=abc-defg-hij"}
```

Recusas (todas com o mesmo `code` para não revelar qual das metades falhou):
`404 studio.pairing_invalid` (não existe, expirou, já usado, queimado, ou segredo errado);
`400 studio.pairing_malformed` (formato); `429` rate-limit.

**O token de fonte** é um JWT `typ: "source"` (`sub` = `source_id`, `room` = sala do
estúdio, validade 12 h). Nenhuma outra rota o aceita (todas exigem `typ` `access` ou
`room`). No `/ws` ele:
- entra **sem sala de espera** (o código foi emitido pelo operador);
- nunca é anfitrião nem admite;
- só pode mandar `sfu-offer`, `sfu-answer`, `sfu-ice`, `leave`, `studio-source-status`
  e `studio-command-result`. Tudo o resto recebe `{"type":"error","message":"source.forbidden_message"}`
  e é descartado;
- é recusado no upgrade se a fonte estiver revogada (`DELETE` da fonte) ou o estúdio apagado.

### 3.3 Fontes do estúdio

| Método e caminho | Quem | Resposta |
|---|---|---|
| `GET /api/orgs/{org_id}/studios/{studio_id}/sources` | view | `200 Page<Source>` |
| `GET /api/orgs/{org_id}/studios/{studio_id}/sources/{source_id}` | view | `200 Source` |
| `PATCH …/sources/{source_id}` | operate | `{label?, number?}` → `200 Source` (`409 studio.source_number_taken`) |
| `DELETE …/sources/{source_id}` | operate | `204` — revoga o token e expulsa o telefone da sala |

```json
Source = {"id","studio_id","number","label","kind":"phone_app",
  "device":{"model","platform","app_version"},
  "paired_at","revoked_at":null,"last_seen_at",
  "connected": true, "tally":"program|preview|free",
  "status": SourceStatus | null}
```

`connected`/`tally` vêm da sala viva quando o pedido cai no pod da sala; noutro pod vêm do
último estado persistido (`last_seen_at` diz de quando é).

## 4. WebSocket da sala — mensagens novas — `FEITO`

Todas em JSON com `type` em kebab-case, no mesmo `/ws` da sala (afinidade `&room=` do
ADR-0001).

### 4.1 Operador → servidor (só o anfitrião actual da sala do estúdio)

| `type` | Campos | Efeito |
|---|---|---|
| `studio-tally` | `program: [source_id]`, `preview: [source_id]` (≤ 16 cada) | fixa o tally; cada telefone recebe o SEU estado se mudou. Um id em `program` e `preview` fica `program`. |
| `studio-command` | `source_id`, `command_id` (string ≤ 64, do cliente), `command` (abaixo) | valida e encaminha ao telefone |

`command` (`kind` + parâmetros, intervalos validados no servidor):

| `kind` | Parâmetros |
|---|---|
| `focus-face` | — |
| `focus-distance` | `meters` 0.1–100 |
| `lock-exposure-focus` | `locked` bool |
| `exposure` | `ev` −3.0–3.0 |
| `white-balance` | `kelvin` 2000–10000 |
| `iso` | `value` 50–12800 |
| `zoom` | `factor` 0.5–10 |
| `mirror` | `on` bool |
| `local-recording` | `on` bool |

Recusas ao operador: `{"type":"error","message":"studio.not_operator"|"studio.unknown_source"|"studio.source_offline"|"studio.invalid_command: <razão>"}`.

### 4.2 Servidor → telefone

| `type` | Campos |
|---|---|
| `studio-tally` | `state: "program"|"preview"|"free"` — enviado ao entrar e a cada mudança |
| `studio-command` | `command_id`, `command` |

### 4.3 Telefone → servidor

| `type` | Campos |
|---|---|
| `studio-source-status` | `battery_percent` 0–100, `charging` bool, `temperature_c` −20–90, `thermal_state` `nominal|fair|serious|critical`, `network: {kind: "wifi"|"cellular"|"usb"|"ethernet", link_mbps?, rtt_ms?}`, `video: {width, height, fps}`, `local_recording` bool. Tudo opcional; ≤ 1 por segundo é guardado. |
| `studio-command-result` | `command_id`, `ok` bool, `error?` (≤ 200) |

### 4.4 Servidor → operador(es) (todos os anfitriões da sala)

| `type` | Campos |
|---|---|
| `studio-sources` | `sources: [{source_id, peer_id, number, label, connected, tally, status}]` — ao entrar e a cada mudança de ligação/tally. `peer_id` liga as tracks do SFU à fonte. |
| `studio-source-status` | `source_id` + os campos de 4.3 + `at` |
| `studio-command-result` | `source_id`, `command_id`, `ok`, `error?` |

## 5. Gravação por câmara (ISO) e faixas de áudio separadas — `PARCIAL`

> `FEITO`: `GET …/recording-target` (espaço livre real por `statvfs`).
> `CONTRATO`, por implementar: `recording_tracks`, `GET /api/recordings/{recording_id}/tracks`
> e `…/tracks/{track_id}/content`, e a gravação ISO em ficheiro por fonte. A UI **não**
> pode mostrar «REC · ISO n» com um número de faixas — ainda não há de onde o ler.

**Decisão (ADR-0014 §3):** o SFU grava cada publicação (já o fazia, em IVF/OGG) e, no
fim, cada fonte fica num ficheiro próprio por **cópia** (`-c copy`, sem recodificar).
Cada faixa de áudio fica também à parte, antes de qualquer mistura. O browser não envia
faixas.

- Liga-se com `iso_recording: true` no estúdio. Grava-se com o `server-record` que já existe.
- A gravação agregada é a linha normal de `recordings` (a composição de sempre) **mais**
  N linhas em `recording_tracks`.

| Método e caminho | Quem | Resposta |
|---|---|---|
| `GET /api/recordings/{recording_id}/tracks` | quem vê a gravação | `200 Page<RecordingTrack>` |
| `GET /api/recordings/{recording_id}/tracks/{track_id}/content` | quem vê a gravação | o ficheiro (`video/webm` ou `audio/ogg`), `Content-Disposition` |
| `GET /api/orgs/{org_id}/studios/{studio_id}/recording-target` | view | `200 RecordingTarget` |

```json
RecordingTrack = {"id","recording_id","kind":"video|audio","label":"CAM 2 · telefone da Ana",
  "source_id":null|"…","number":2|null,"codec":"vp8|opus","container":"webm|ogg",
  "size_bytes","duration_ms"|null,"offset_ms","width"|null,"height"|null,"created_at"}
RecordingTarget = {"kind":"local","free_bytes","total_bytes","iso_recording",
  "object_storage":{"kind":"minio","state":"not_configured"}}
```

- `free_bytes`/`total_bytes` são o `statvfs` real do volume de gravações.
- **Ligação ao #93** (`backend/bw2-gravacoes`, por fundir): `recording_tracks.recording_id`
  referencia `recordings(id)`. O acesso usa `recordings::load_item` + `can_see`. Quando o
  #93 entrar, o `RecordingLibraryItem` ganha `track_count` — é a única linha que toca.
- Só VP8 e Opus são gravados (mesma fronteira do `recordable_codec`); uma track H.264
  fica fora da gravação com erro escrito.

## 6. Persistência do estúdio (documentos com versões) — `FEITO`

Seis tipos, todos debaixo do estúdio, com o mesmo contrato:

| Tipo | Caminho (`…` = `/api/orgs/{org_id}/studios/{studio_id}`) |
|---|---|
| Cena de mistura | `…/mixer-scenes` |
| Macro F1–F12 | `…/macros` |
| Sobreposição | `…/overlays` |
| Cena de luz | `…/light-scenes` |
| Perfil de correcção por câmara | `…/camera-profiles` |
| Alinhamento do programa | `…/rundowns` |

| Método | Quem | Contrato |
|---|---|---|
| `GET …/{tipo}` | view | `200 Page<Document>` |
| `POST …/{tipo}` | operate | `{name, body}` → `201` + `Location` + `Document` (`version: 1`) |
| `GET …/{tipo}/{document_id}` | view | `200 Document` |
| `PATCH …/{tipo}/{document_id}` | operate | `{version, name?, body?}` → `200 Document` (`version+1`); `409 studio.version_conflict` se `version` não for a actual |
| `DELETE …/{tipo}/{document_id}` | operate | `204` |
| `GET …/{tipo}/{document_id}/versions` | view | `200 Page<DocumentVersion>` (mais recente primeiro) |

`Document = {id, studio_id, kind, name, key|null, version, body, summary|null, created_by, updated_by, created_at, updated_at}`;
`DocumentVersion = {version, name, body, updated_by, updated_at}`.
Erros: `400 studio.invalid_document` com `details[].field`; `409 studio.key_taken` (tecla
repetida no mesmo tipo e estúdio).

### 6.1 Corpos (`body`)

**mixer-scenes**
```json
{"sample_rate":48000,"bit_depth":24,
 "loudness":{"target_lufs":-16,"true_peak_max_dbtp":-1},
 "master":{"level_db":-3},
 "buses":[{"id":"aux1","name":"retorno do palco","kind":"aux|record|program","level_db":-8}],
 "channels":[{"number":1,"name":"Microfone principal","input":"XLR 1",
   "gain_db":0,"fader_db":-2.1,"mute":false,"solo":false,"pan":0,
   "buses":["aux1"],
   "eq":{"enabled":true,"bands":[
      {"type":"low_shelf","freq_hz":80,"gain_db":-2,"q":0.7},
      {"type":"peak","freq_hz":420,"gain_db":1.5,"q":1},
      {"type":"peak","freq_hz":2400,"gain_db":2.5,"q":1},
      {"type":"high_shelf","freq_hz":8000,"gain_db":1,"q":0.7}]},
   "dynamics":{"gate":{"enabled":true,"threshold_db":-42},
     "compressor":{"enabled":true,"threshold_db":-18,"ratio":3.2,"attack_ms":12,"release_ms":180},
     "limiter":{"enabled":true,"ceiling_db":-2}},
   "cleanup":{"echo_cancellation":true,"noise_reduction_db":-14,"silence_removal":false,"voice_leveling":true}}]}
```
Limites: 1–64 canais, números únicos; `gain_db` −60–24; `fader_db` −90–10 ou `null` (−∞);
`pan` −1–1; exactamente 4 bandas, `freq_hz` 20–20000, `gain_db` −18–18, `q` 0.1–10;
`ratio` 1–20; `target_lufs` −30–−5; `true_peak_max_dbtp` −9–0; `sample_rate` 44100|48000;
`bit_depth` 16|24; ≤ 8 barramentos com ids únicos e referidos por canais.

**macros** — `key` obrigatória `F1`–`F12`
```json
{"key":"F2","steps":[{"action":"set-preview","source_number":2},
  {"action":"transition","kind":"cut|mix|wipe|stinger","duration_ms":600},
  {"action":"overlay","overlay_id":"…","on":true},
  {"action":"light-scene","document_id":"…"},
  {"action":"mixer-scene","document_id":"…"},
  {"action":"channel","number":11,"fader_db":-18,"mute":false},
  {"action":"wait","ms":500},
  {"action":"recording","on":false},{"action":"end-broadcast"}]}
```
1–50 passos; `wait` ≤ 60 000 ms. Ids referidos têm de ser documentos deste estúdio.
As macros **executam-se no browser**; o servidor só as guarda e valida.

**overlays** — `key` opcional `mod+1`–`mod+9`
```json
{"key":"mod+1","type":"lower-third","lower_third":{"title":"Ana Mbala","subtitle":"directora de tecnologia"}}
{"type":"logo","logo":{"text":"DELONIX · AO VIVO","position":"top-left|top-right|bottom-left|bottom-right"}}
{"type":"clock","clock":{"format":"HH:mm:ss","timezone":"Africa/Luanda"}}
{"type":"poll","poll":{"question":"…","options":["…","…"]}}
```

**light-scenes** — `key` opcional `F1`–`F12`
```json
{"key":"F2","transition_ms":1800,
 "fixtures":[{"fixture_key":"dmx:1:1","level":86,"cct_k":5200},{"fixture_key":"hue:bridge-1:3","level":48,"cct_k":3000}]}
```
`level` 0–100; `cct_k` 1000–10000 ou `null`; `transition_ms` 0–60 000; ≤ 128 aparelhos.

**camera-profiles**
```json
{"source_number":2,"exposure_ev":0.4,"temperature_k":5200,"tint":0,"contrast":1.0,
 "face_enhance":"off|low|medium|high","noise_reduction":"off|low|medium|high",
 "background_blur":false,"white_balance_match":true}
```
A correcção corre no browser (compositor); o servidor guarda o perfil.

**rundowns**
```json
{"items":[{"title":"Abertura","duration_ms":60000,"note":"stinger + genérico","macro_key":"F1"}]}
```
1–200 itens; `duration_ms` 0–86 400 000. O `Document` de um alinhamento traz
`summary: {total_duration_ms, item_count}` calculado no servidor (`summary` é `null` nos
outros tipos).

## 7. Directo a partir do programa — `CONTRATO`

O emissor continua a ser `GET /api/rooms/{room_code}/live` (WebSocket, ADR-0003, browser
compõe e codifica). Acrescenta-se o estado e a paragem:

| Método e caminho | Quem | Resposta |
|---|---|---|
| `GET /api/rooms/{room_code}/live-session` | dono da sala ou admin da org do dono | `200 LiveSession` |
| `DELETE /api/rooms/{room_code}/live-session` | idem | `204`; `404 live.not_active` |

```json
LiveSession = {"active":true,"started_at":"…","destinations":["YouTube","RTMP próprio"],
 "bytes_in":123456789,"bitrate_bps":9400000,"server_write_p95_ms":3,
 "platform_delay_ms":null,"measured_at":"…","node":"pod-name"}
```

- `bitrate_bps` = bytes recebidos do browser nos últimos 5 s × 8 / 5 — é o bitrate
  **real** que chega ao servidor.
- `server_write_p95_ms` = p95 do tempo de escrita no `stdin` do ffmpeg nos últimos 5 s
  (contra-pressão do lado do servidor).
- `platform_delay_ms` é **sempre `null`**: o atraso até ao espectador é da plataforma e o
  servidor não o mede. A UI não inventa «atraso 8 s».
- `active:false` (com os outros campos a `null`) quando não há emissão.
- Com Redis, o estado é espelhado (`live:session:{room_id}`, TTL 15 s) e o `DELETE` pede a
  paragem ao pod que emite (`live:stop:{room_id}`); sem Redis, só o pod local responde.

## 8. Agente local de iluminação — servidor `CONTRATO`; hardware `EXTERNAL`

### 8.1 BFF (sessão)

| Método e caminho | Quem | Resposta |
|---|---|---|
| `GET /api/orgs/{org_id}/studios/{studio_id}/light-agents` | view | `200 Page<LightAgent>` |
| `POST …/light-agents` | manage | `{name}` → `201` + `Location` + `{…LightAgent, token}` (o token `dlxs_` só aqui) |
| `GET …/light-agents/{agent_id}` | view | `200 LightAgent` |
| `DELETE …/light-agents/{agent_id}` | manage | `204` |
| `GET …/fixtures` | view | `200 Page<Fixture>` (de todos os agentes do estúdio) |
| `POST …/light-commands` | operate | `202` + `Location` + `LightCommand` |
| `GET …/light-commands/{command_id}` | view | `200 LightCommand` |

`LightAgent = {id, studio_id, name, prefix, created_at, last_seen_at, online, version}` —
`online` = visto há menos de 30 s.
`Fixture = {agent_id, fixture_key, name, protocol:"artnet|hue", address, capabilities:["level","cct","rgb"], level|null, cct_k|null, state:"ok|unreachable|warning", warning|null, last_seen_at}`.
`LightCommand` pedido: `{"type":"set-levels","fixtures":[{fixture_key, level, cct_k?}],"transition_ms":0}`
ou `{"type":"apply-scene","document_id":"…"}` (o servidor resolve a cena para `set-levels`)
ou `{"type":"blackout","transition_ms":2000}`.
Resposta: `{id, agent_id, status:"queued|claimed|done|failed|expired", request, error, created_at, finished_at}`.
Recusas: `422 studio.no_light_agent` (nenhum agente online com os aparelhos pedidos) —
**nunca** se aceita e fica parado. Um comando `claimed` há mais de 60 s passa a `expired`.

### 8.2 Agente (`Authorization: Bearer dlxs_…`)

| Método e caminho | Corpo → resposta |
|---|---|
| `PUT /api/integrations/studio-agent/v1/fixtures` | `{version, fixtures:[Fixture sem agent_id/last_seen_at]}` → `200 {poll_interval_ms}` |
| `POST /api/integrations/studio-agent/v1/claim` | → `200 {commands:[{id, request}]}` (até 10, só deste agente) |
| `POST /api/integrations/studio-agent/v1/commands/{command_id}/result` | `{ok, error?}` → `204`; `404` se não for deste agente |

### 8.3 O que o agente faz (e o que não faz)

- **Art-Net:** envia `ArtDmx` (OpCode `0x5000`, ProtVer 14) por UDP para o nó configurado
  (unicast; broadcast só se o operador o escrever). Transições interpoladas a 40 Hz.
  Mapas de aparelho no ficheiro de configuração (`dimmer`, `dimmer-cct`, `rgb`).
  **DMX não se descobre:** o operador declara o que está em cada canal.
- **Hue:** API local v1 da bridge (`PUT /api/{username}/lights/{id}/state`, `bri` 1–254,
  `ct` em mireds 153–500, `transitiontime` em décimas de segundo). O `username` obtém-se
  carregando no botão da bridge (`POST /api`), passo manual **uma vez**, documentado.
- **EXTERNAL:** nenhum nó Art-Net nem bridge Hue nesta máquina. Os adaptadores estão
  provados contra um receptor Art-Net UDP falso e uma bridge HTTP falsa.

## 9. O que a UI precisa de saber (lista curta)

1. O corte, a mistura e a correcção de imagem são **da UI**; o backend guarda cenas e
   perfis, não os aplica.
2. O PhoneCam fala: `POST /api/studio-pairings` → `/ws?token=…&room=…` → publica pelo SFU →
   ouve `studio-tally` e `studio-command`, manda `studio-source-status`.
3. A mesa de corte manda `studio-tally` sempre que PGM/PRÉ mudam, e liga tracks a fontes
   pelo `peer_id` de `studio-sources`.
4. «REC · ISO n» = `server-record` numa sala de estúdio com `iso_recording: true`; o número
   de faixas vem de `GET /api/recordings/{id}/tracks` depois de finalizada.
5. «AO VIVO · atraso» — mostrar `bitrate_bps` e `server_write_p95_ms`; **não** há atraso da
   plataforma para mostrar.
6. «CPU/GPU» do StudioLive é do PC do operador (browser), não do servidor.
7. «Corte automático por voz», «Realce de rosto por IA», «Supressão de eco» e afins são
   processamento local do browser; não há rota para eles.
8. MinIO: `object_storage.state = "not_configured"` — mostrar o destino local e o espaço
   livre real.
