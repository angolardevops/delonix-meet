# Frente D — Canais na sala: estado em PAUSA (2026-09-17)

Branch `delonix-meet-backend/v3-canais`. Worktree `.worktrees/delonix-meet/v3-canais`.
Base: `origin/seg/ssrf-saida` = `d3ffd8f` (já traz a main #84, com R156/R157 e o `webrtc-ice` vendorizado). Rebase feito antes de mexer no `sfu.rs`.
Não há nada por commitar. Não arranquei servidores nem Vite. A base `v3_d` foi criada no Postgres 127.0.0.1:5445 e ainda não tem migrações da frente.

## Feito (commits)

| sha | O quê | Prova |
|---|---|---|
| `6af67da` | Domínio `conferencing::channels`: canal e origem, o que quem está fora da app não recebe, máquina de estados do dial-out, regras do PIN de uso único, texto do SMS, histerese da «ligação fraca». `phone_bridge`: G.711 μ/A, G.711→Opus a 8 kHz com relógio de 48 kHz, misturador mix-minus Opus→G.711, jitter e perda (RFC 3550). Nova dependência: `opus-rs =0.1.33` (Rust puro). | 16 testes unitários da ponte e os do domínio, verdes |
| `2a780d2` | Protocolo: o `PeerInfo` ganha o `Seat` achatado (`channel`, `carrier`, `number_masked`, `anonymous`, `video_unavailable`, `weak_link`, `on_stage`, `call_id`). Mensagens novas: `peer-updated`, `channel-summary`, `dial-out-updated` e `session-cost` (as duas últimas só para quem pode admitir). No hub: `join_external`, `update_external`, `seat_of`, `channel_summary`, e o resumo enviado a quem entra e a cada saída de alguém de fora da app. | 5 testes de protocolo e de hub: serde, compatibilidade com o `PeerInfo` antigo, número nunca inteiro no que a sala recebe, dial-out só para admitters |
| `4672307` | Cherry-pick do contrato da frente C (`382861d`): `telephony::{number, dial_plan, money, cost, ports}`. | os testes deles, verdes |
| `fe76698` | Refactor: o número, a máscara, o dinheiro e o custo passam a vir do `telephony`, e as cópias locais saem. `parse_invitee` exige E.164 e recusa emergência com `telephony.emergency_not_invitable`. `session_cost` soma `Money` por moeda. | 85 testes do crate domain, verdes |
| `93107af` | Ponte no SFU. A `Publication` passa a ter uma origem (`PubSource::Remote`/`Bridge`), com `activate_publication` partilhado. `feed_bridges` copia o áudio da sala para as chamadas. O seletor de oradores conta o nível medido e respeita `pinned` (palco). Uma sala só com a chamada não se apaga. Saem o stub `spawn_phantom_listener` e o `pstn_outbounds`, que não tinham chamadores. `phone_bridge::leg`: uma perna por chamada, com socket RTP simétrico, lista de IPs permitidos, silenciar na ponte e estatísticas. Teste `sfu_e2e::ponte_telefone_sala_tom_nos_dois_sentidos` (R221). | `cargo test --release --lib sfu` → 24/24; `sfu_e2e` 7/7 em duas corridas seguidas |

### Medições (R221, máquina com carga 40–70 em 32 threads)
- **Telefone→sala:** 0,1–0,4 ms, do envio do RTP G.711 à chegada do RTP Opus ao subscritor webrtc-rs. Não inclui o jitter buffer do browser.
- **Sala→telefone:** 20–40 ms (pré-buffer de 40 ms do misturador mais o alinhamento ao tique de 20 ms).
- **CPU dos codecs:** 0,6–1,4 % de um núcleo por chamada, com um orador na sala.
- **Mix-minus:** o próprio tom volta com magnitude 0,006 (tom da Ana: 0,246).
- **Sonda fora do repo** (`.worktrees/delonix-meet/v3-canais-exp/opusprobe`, o `target` foi apagado):
  - `opus-rs` codifica a 8 kHz e a libopus (ffmpeg) descodifica o tom exacto (1 kHz: 0,2501, sem fugas);
  - a libopus codifica a 48 kHz e o `opus-rs` descodifica a 8 kHz (440 Hz: 0,2499);
  - encode p50 235–278 µs, decode p50 79 µs por bloco de 20 ms;
  - G.711: descodificação igual à do ffmpeg nos 256 códigos; codificação diferente em 512 (μ) e 964 (A) de 65 536 valores, nas fronteiras de degrau.

## A meio
Nada fica a meio no disco. O desenho já decidido e ainda sem código:
- **Dial-out:** vai pelas portas da frente C (`CallOriginator`, `SmsGateways`) e pelas funções de serviço `place_call` / `estimate_price_per_min` / `room_call_costs` / `send_sms`, que ainda não existem na branch dela. Nesta branch, sem essas portas, a resposta é `not_configured`. Nos testes entra um originador falso ao nível da porta. **Não** se escreve um adaptador ESL próprio: o contrato proíbe um segundo caminho para o FreeSWITCH.
- **Contrato a pedir à C:**
  1. uma variante `AfterAnswer::RoomBridge { room_code, rtp_addr }`, ou equivalente, para o FreeSWITCH mandar a media para a perna da ponte e não para a conferência local;
  2. eventos de progresso (a tocar) e de fim da chamada, além do `OriginateOutcome` final;
  3. uma única extracção `sms::enqueue`, feita por uma das duas frentes e não pelas duas.

## Próximos passos, por ordem
1. **Migração `0080_room_channels.sql`:**
   - `room_call_leg`: org, sala, tipo, canal, rede, número, nome, contacto, estado, causa, tarifa, `answered_at`/`ended_at`, `muted`, `on_stage`, porta da ponte, quem pediu;
   - `room_phone_pin`: hash do PIN, DID, expiração, uso, tentativas;
   - `org_whatsapp_config`: `phone_number_id`, token selado com `secrets_at_rest`, template, idioma, voz ligada.
2. **Módulo `server/src/room_channels.rs` e registo `RoomChannels` no `AppState`.** Pernas vivas por sala. Traduz os eventos da perna em `update_external` e `DialOutUpdated`, e as saídas do originador em estados.
3. **Rotas BFF, cada uma com `#[utoipa::path]`, auditoria e 404 para outra org:**
   - `POST/GET /api/rooms/{room_code}/dial-outs`;
   - `GET/DELETE /api/rooms/{room_code}/dial-outs/{dial_out_id}` (o DELETE cancela);
   - `POST /api/rooms/{room_code}/dial-outs/quotes` (custo estimado antes de ligar);
   - `PUT /api/rooms/{room_code}/phone-participants/{call_id}/stage`;
   - `PUT …/mute`;
   - `PUT …/identity` (associa um contacto e fica auditado);
   - `POST …/redial`;
   - `DELETE …` (desliga pelo `CallOriginator::hangup`);
   - `GET /api/rooms/{room_code}/session-cost`;
   - `PUT/GET/DELETE /api/orgs/{org_id}/whatsapp`.

   Autorização: `rooms::room_access(..).admitter` (dono ou co-anfitrião) até existir o `require_capability` da frente A. Capacidades propostas: `meeting.participants.invite_external` (dial-outs, SMS, WhatsApp), `meeting.participants.manage_external` (palco, silenciar, remover), `contacts.identify_number` (identificar) e `org.integrations.whatsapp.manage`.
4. **SMS com PIN.** Gera o PIN, guarda-o em hash, envia pela porta `SmsGateways` e aceita-o uma só vez no `voice::validate_pin`, que partilha o limitador por DID. Testes: expiração, uso único e força bruta.
5. **`WhatsAppProvider`.** Adaptador real para a Graph API da Meta, com `state.outbound.operator()` + `check_operator_url` e base configurável por `WHATSAPP_API_BASE`. Em testes, um servidor HTTP falso com `OUTBOUND_ALLOW_HOSTS=127.0.0.1`. Sem credenciais, `not_configured`.
6. **Testes de integração** em `server/tests/room_channels.rs` contra Postgres (`DATABASE_URL=…5445/v3_d`):
   - outra org → 404;
   - membro sem poder → 403 com código estável;
   - número mascarado para quem não é anfitrião;
   - dial-out com originador falso: estados em tempo real e cancelamento.
7. **Casos em `web/e2e/isolamento.mjs`** e servidor próprio (`BIND_ADDR=127.0.0.1:8440`, Redis db 13, `SFU_UDP_MIN=53000 SFU_UDP_MAX=53199`).
8. **Documentação:**
   - ADR-0010 (decisões e medições acima; opções RTP directo vs `mod_verto`, com a razão da escolha);
   - R220 (protocolo de canais), R221 (ponte), R222+ (PIN, dial-out, WhatsApp);
   - `docs/reference/api-routes.md` e regravar as specs OpenAPI.

## Portões por correr
- `cargo fmt --check` (o `cargo fmt` foi corrido antes de cada commit, mas o `--check` não) e `cargo test --release --workspace`. Até agora só corri a bateria da lib (`--lib sfu`, `signaling::tests`, `phone_bridge`) e a do crate domain; os binários de `tests/` ficaram de fora.
- `scripts/check-clippy-ratchet.sh`: fasquia 26 depois da base nova. Há avisos `dead_code` novos (`RtpQuality::observe`, `LegHandle::muted`, `seat_of`, os campos de `DialOutView`/`SessionCostView`) que só desaparecem quando o passo 2 os usar.
- `check-route-auth.sh`, `check-openapi.sh`, `check-isolamento-cobertura.sh`, `check-arquitectura-catraca.sh`, `check-crate-deps.sh` (dependência nova `async-trait` no domain, vinda da C, e `opus-rs`/`bytes` no monólito), `check-repo-hygiene.sh`, `check-docs-drift.sh`.
- `web/e2e/isolamento.mjs` contra o servidor da frente.

## EXTERNAL — o que falta para uma chamada Unitel real entrar numa sala
1. **Tronco SIP da Unitel** (contrato, DIDs, IPs, codec — provavelmente PCMA) no Kamailio/FreeSWITCH, com a configuração corrigida (`sip-realidade.md` §2). É da frente C.
2. **O FreeSWITCH tem de mandar a media da chamada para a perna da ponte:**
   - opção a) `mod_rtp`/endpoint RTP com o endereço da perna;
   - opção b) uma perna SIP para um UAS mínimo.

   Nenhuma das duas foi validada: não há FreeSWITCH em contentor nesta máquina e não descarreguei nenhum. Falta ainda o `AfterAnswer::RoomBridge` no contrato da C.
3. **Rede:** intervalo de portas UDP da ponte exposto só na rede interna e `allowed_sources` com os IPs do FreeSWITCH (config por criar), mais NetworkPolicy/deploy do FreeSWITCH, que nenhum manifesto entrega.
4. **SRTP entre o FreeSWITCH e a ponte:** não implementado. Hoje o RTP é em claro, só na rede interna.
5. **Multi-réplica:** a perna tem de nascer no nó que tem a sala (ADR-0001). A REST não tem afinidade, por isso falta o pedido pelo bus Redis.
6. **Browsers reais:** a publicação da ponte só foi recebida por webrtc-rs. Nunca foi medida num Chrome ou Firefox, nem na gravação.
