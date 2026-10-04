# Frente E — Resiliência da emissão (DelonixRecovery) · estado em pausa

**Pausada a 2026-09-17** por decisão do dono do produto (as frentes correm uma de cada vez).
Branch `delonix-meet-backend/v4-recuperacao`, worktree `.worktrees/delonix-meet/v4-recuperacao`,
base `origin/seg/ssrf-saida` em `d3ffd8f`. Sem push, sem PR.
Reservas do ramo original (já sem efeito: no `develop` a migração é a 0074 e as regressões
nunca chegaram a ser escritas): migrações «85 a 89», ADR-0013, regressões «230 a 239», servidor 8450, Redis db 14, base `v4_e`,
`SFU_UDP_MIN=53400 SFU_UDP_MAX=53599`.

## Feito (commits)

| sha | O quê | Prova |
|---|---|---|
| `3606085` | `docs/adr/0013-emissao-que-sobrevive-a-queda-da-rede.md` (Proposto), escrito ANTES do código, com a linha de base medida e as quatro exigências da delonix-meet-44 | medições M1–M5 abaixo |
| `1d4b026` | `server/crates/delonix-meet-domain/src/content/live_output.rs`: máquina de estados pura de um destino, backoff exponencial com tecto e jitter, recusas com código estável, `Profile`, `EventCode`/`EventKind`, `should_adapt` | `cargo test --release -p delonix-meet-domain live_output`: 8/8 verdes; `rustfmt --check` limpo; clippy do crate sem avisos nesse ficheiro |

### Medições feitas (nesta máquina, load average 35–72 por outras sessões; mediamtx em contentor; fonte 2160p/16 Mbit/s H.264+Opus em Matroska por cano)

- **M1 — defeito no código actual (candidato a R230):** com ≥2 destinos, o `montar_argumentos` do `broadcast.rs` põe `-c:v copy -c:a aac` só antes da 1.ª saída; as saídas 2..N recebem `h264 -> flv1` e `opus -> mp3` (visto no `Stream mapping`), e o mediamtx fecha com `unsupported video codec: 2`. O multi-destino da linha de backend nunca funcionou para lá do primeiro destino, e queima CPU a codificar FLV1.
- **M2:** um ffmpeg com 2 saídas correctas; destino 2 em buraco negro (`docker pause`) aos 10 s → o destino saudável recebeu **9,1 s** em 50 s.
- **M3:** `-f tee` com `onfail=ignore` → o saudável recebeu **9,8 s** em 50 s; não reinicia nada.
- **M4 (CPU, 4 destinos, `pidstat` 40 s):** um processo 6,0 % de um core · tee 4,8 % · **um ffmpeg por destino 25,9 %** (~6,5 % cada).
- **M5 (2160p→1080p libx264 veryfast 4,5 Mbit/s):** `-threads 1` → 65 % e só 0,5× tempo real; `-threads 4` → **147 %** a 1,01×.
- Scripts das medições: scratchpad da sessão (`…/scratchpad/m/medA.sh`, `medC.sh`, `medD.sh`, `cpu.sh`) — **não estão no repo**; os comandos essenciais estão no ADR. Se se quiser reprodutibilidade no repo, passam para o cabeçalho do e2e.

## A meio (por commitar, não compilado no crate do servidor)

- `server/migrations/0085_live_sessions.sql` — tabelas `live_sessions` (com `snapshot` jsonb + `observed_at` para detectar processos mortos, índice único parcial de uma sessão aberta por sala, `recording_id … ON DELETE CASCADE` para a retenção seguir a da gravação), `live_session_destinations`, `live_session_events` (`seq` como cursor). **Nunca correu contra Postgres.**
- `server/src/broadcast/matroska.rs` — detecção de Cluster (com e sem CRC-32, portada de `ea6fc31`), Timestamp do Cluster, resolução lida do cabeçalho (`Tracks/TrackEntry/Video/PixelWidth|PixelHeight`), com testes unitários. **Não está ligado** (o `broadcast.rs` ainda não declara `mod matroska`), por isso nem compilou nem os testes correram.

## Próximos passos, por ordem

1. `git fetch && git rebase origin/seg/ssrf-saida` (ou merge) se a base tiver avançado.
2. Transformar `server/src/broadcast.rs` em `server/src/broadcast/mod.rs` com submódulos (desenho no ADR-0013 §1–§9):
   - `matroska.rs` (já escrito) — ligar e correr os testes;
   - `output.rs` — `Destino` (com `kind` e `saved_id`), `montar_argumentos(destino, perfil, threads, transcode_threads)` **com as opções por saída** (corrige M1), `-progress pipe:1`, `-rw_timeout`, drenagem do `stderr`, `redact` (chave E o URL), `classify_failure`, fila por destino com orçamento em bytes e reentrada com cabeçalho + próximo Cluster, e o supervisor que executa a `OutputMachine` (Spawn/Kill/ScheduleRetry/CancelRetry/Log), `connect_timeout`, `stable_after`, cálculo de `backlog_ms` a partir da fila e do débito de entrada, adaptação só com orçamento; reaproveitar `54e8cd8`/`ea6fc31` (ramos locais `frontend/l2-*`, não empurrados);
   - `recording.rs` — gravador do fluxo emitido: thread + fila limitada, `write(2)` para `RECORDINGS_DIR/live-<sessão>.webm`, bytes perdidos contados, timestamps dos Clusters para a duração, finalização para `recordings` (`{id}.webm`) e `live_sessions.recording_id`; extrair do `recorder::finalize_inner` o anúncio (notificação + webhook `recording.ready`) para uma função partilhada em vez de copiar;
   - `journal.rs` — tarefa por sessão que escreve eventos/estado dos destinos/retrato na base POR ORDEM, à medida que acontecem, e emite `ServerMsg::BroadcastEvent`/`BroadcastDestination` por `hub.broadcast_hosts`; `seq` e `offset_ms` atribuídos em memória;
   - `session.rs` — `LiveSession` (repartidor síncrono para gravador + destinos, eventos de sessão: `session.started`, `recording.started`, `output.all_down`, `recording.continued` com os bytes perdidos reais, `recording.degraded`, `outputs.stopped`, `session.ended`), `Registo` do nó com contador de transcodificações (`LIVE_TRANSCODE_MAX`);
   - `ws.rs` — o `GET /api/rooms/{room_code}/live` (manter as recusas legíveis pós-upgrade), destinos guardados por `resolve_for_broadcast`;
   - `api.rs` + `store.rs` — REST abaixo.
3. `config.rs`: `LIVE_RETRY_MAX_ATTEMPTS` (8), `LIVE_RETRY_INITIAL_MS` (2000), `LIVE_RETRY_MAX_MS` (15000), `LIVE_CONNECT_TIMEOUT_SECS` (30), `LIVE_DEGRADED_BACKLOG_MS` (2000), `LIVE_ADAPT_AFTER_SECS` (10), `LIVE_TRANSCODE_MAX` (**0**), `LIVE_TRANSCODE_THREADS` (4), `LIVE_ORPHAN_AFTER_SECS` (30), fila do gravador; `libc` como dependência directa (já no lockfile) para `statvfs`.
4. Rotas (BFF, sessão; autorização: dono da sala ou co-anfitrião em `room_admitters` via `rooms::room_access().admitter`; sem acesso → `404`, colega sem papel → `403 broadcast.host_required`; registar por rota a capacidade futura da frente A `broadcast.public_destinations` / `broadcast.manage_rtmp_keys` — já existem no catálogo do ramo `v3-rbac-utilizadores`, migração 0060 dele):
   - `GET /api/rooms/{room_code}/broadcast` — agregado, destinos, cartões (gravação, Internet, sala), «o que está seguro», disco, «quem perdeu o quê», `recovery: {live_resumes_at_present: true, lost_seconds_backfilled_live: false, lost_seconds_in_recording}`, `profiles_available`;
   - `GET /api/rooms/{room_code}/broadcast/events` — `Page<T>` por `seq`;
   - `GET /api/recordings/{recording_id}/broadcast-events` — o registo no leitor da gravação;
   - `POST /api/rooms/{room_code}/broadcast/destinations/{destination_id}/reconnections` — `202`;
   - `PUT /api/rooms/{room_code}/broadcast/destinations/{destination_id}/profile` — `{"profile":"source"|"1080p"}`, `202` ou `409 broadcast.transcode_unavailable`;
   - `POST /api/rooms/{room_code}/broadcast/stop-outputs` — método personalizado (a regra 4 do `api-routes.md` permite), a gravação continua.
   Acções chegam ao pod da emissão por `RedisRoomEvent::LiveCommand` (novo); o pod que responde pré-valida com o estado da base.
5. `lib.rs`: rotas, `openapi.rs`, tarefa de varrimento de sessões órfãs (retrato parado > `LIVE_ORPHAN_AFTER_SECS` → `interrupted` + evento `session.node_lost`), gauges; `recorder::retention_sweep` passa a apagar sessões sem gravação pela `retention_days` da org de quem emitiu.
6. `metrics.rs`: `delonix_live_reconnect_attempts_total`, `delonix_live_destinations_lost_total`, `delonix_live_offair_seconds_total`, `delonix_live_buffered_bytes`, `delonix_live_recording_dropped_bytes_total`, `delonix_node_recordings_disk_free_bytes`; runbook e alertas em `docs/ops/emissao-resiliente.md`.
7. Contagens reais: sala = `hub.room_size`; sondagens/perguntas = `room.polls/questions` no hub (null se a sala não estiver neste pod); chat = `room_chat_messages` desde o início; quadro = `whiteboards.room_code`; faixas = acessor novo no `SfuState` (sessão de gravação da sala); telefone = `voice_participant` PSTN activos; WhatsApp `null` (`not_available`, frente D); públicos `null` (`external`).
8. `scripts/check-capability-claims.sh`: recusar texto nos locales que prometa envio em diferido/backfill do directo.
9. Testes:
   - unidade: supervisor com ffmpeg falsos (entupido, surdo, recusa, lento) e `redact` (chave e URL);
   - integração `server/tests/broadcast_recovery.rs` contra Postgres: contrato das rotas, códigos 409, isolamento (outra org 404/colega 403), registo escrito à medida, sessão órfã fechada depois de «reinício»;
   - e2e `web/e2e/directo-recuperacao.mjs` (fora do CI, razão em `scripts/e2e-fora-do-ci.txt`, comando no cabeçalho): 2 mediamtx, um cai a meio (`docker stop` e `docker pause`) → (a) `ffprobe` da gravação final sem descontinuidade e com a duração da sessão; (b) `interrupted → retrying → live` e media DO PRESENTE no mediamtx reposto (não o diferido); (c) o outro destino com bytes a crescer durante a queda; (d) destino permanentemente em baixo acaba em `lost` sem afectar os outros; (e) `PUT …/profile 1080p` com `LIVE_TRANSCODE_MAX=1` não descontinua a gravação; (f) reiniciar o servidor a meio e ler o registo;
   - `web/e2e/isolamento.mjs`: casos negativos das rotas novas.
10. Docs: R230 (M1), R231+ para o que os testes apanharem; actualizar o portão do ADR-0013 e do ADR-0003 (a decisão «um ffmpeg com N saídas» cai); `docs/reference/api-routes.md`; HARNESS.

## Restrições da delonix-meet-44 ao ADR-0013 — o que falta aplicar

| Restrição | Estado |
|---|---|
| Sem envio em diferido para destinos RTMP ao vivo; os segundos perdidos ficam na gravação; retoma no presente | **no ADR e na máquina** (não há `catching_up`). Falta: campos `recovery` na API, e a guarda no `check-capability-claims.sh` |
| Nomes dos campos | a mensagem dizia `lost_seconds_in_recording: false (os segundos ficam na gravação)` — contraditório; o ADR adopta `true` + `lost_seconds_backfilled_live: false`. **Confirmar com a delonix-meet-44** |
| Falhas isoladas por destino, custo MEDIDO no nó | **medido e escrito** (M2–M4). Falta a implementação |
| Backoff exponencial com tecto e jitter | **feito** no domínio |
| Chave decifrada só no servidor, nunca no registo nem nos logs, redacção com teste | por fazer (`redact` com teste) |
| Estado no pod da sala; registo escrito na base à medida; teste com drain/reinício a meio | migração escrita (não corrida); journal e teste por fazer |
| Retenção do registo segue a das gravações | desenhado (`ON DELETE CASCADE` + sweep); por fazer. Nota: a mensagem chamou-lhe «G8», mas no backlog G8 é o centro de notificações; usei a retenção das gravações (`organizations.retention_days`) |
| «Baixar para 1080p» muda o perfil do destino, não a gravação; gravação separada do encoder; prova com `ffprobe` | desenhado (gravador não é ffmpeg); **perfil 1080p desligado por omissão** porque M5 mede 1,47 cores; prova por fazer |
| `isolamento.mjs`, `check-route-auth`, OpenAPI na bff; WS em `/live` | por fazer |

## Portões por correr (nenhum corrido no crate do servidor)

`cd server && cargo fmt --check && cargo test --release --workspace` (com `DATABASE_URL` da base `v4_e` em 127.0.0.1:5445) · `check-clippy-ratchet.sh` (fasquia 26) · `check-route-auth.sh` · `check-openapi.sh` · `check-isolamento-cobertura.sh` · `check-arquitectura-catraca.sh` · `check-crate-deps.sh` · `check-repo-hygiene.sh` (pode queixar-se do buraco 0049–0084 nas migrações, esperado até à integração) · `check-docs-drift.sh` · `check-capability-claims.sh` · `node web/e2e/isolamento.mjs` contra o servidor em 8450 · o e2e de recuperação com mediamtx.

## Riscos e avisos para quem retomar

- **Conflito com a linha l2:** `frontend/l2-*` (local, não empurrado) tem outro `broadcast.rs` (um processo por destino) e uma `0060_stream_destinations.sql` que colide com a frente A e duplica a `0042`. Na integração, o `broadcast.rs` fica o desta frente; a 0060 da l2 não entra.
- O WebSocket `/api/rooms/{room_code}/live` não tem afinidade por sala no ingress (só o `/ws` tem); por isso as acções REST precisam do `LiveCommand` pelo Redis.
- Não há envio para MinIO/S3 no servidor: «sobe depois» é o volume do `RECORDINGS_DIR`.
- Nada foi validado contra YouTube, Facebook ou LinkedIn.
- Processos: nenhum servidor foi arrancado; os quatro contentores `mtx-v4e-{1..4}` foram removidos; o `cargo build` de aquecimento foi parado (o `target/` da worktree ficou parcialmente construído).
