# Regressões conhecidas — NÃO reintroduzir

> Cada entrada aqui já quebrou produção/demos pelo menos uma vez. São armadilhas onde a "correção óbvia" reintroduz o bug. Antes de mexer no código relacionado, lê a entrada. Revisores (`agents/*`) devem verificar estas explicitamente no diff.

Formato: **Sintoma** → **Causa raiz** → **Regra** (o que nunca fazer) → ficheiros.

---

## Media / WebRTC / SFU

### R1 — Oferta SFU inicial nunca enviada (media morta, tile preto, sem `track published`)
- **Sintoma:** juntar-se à sala não estabelece PC; logs sem `pc connected` nem `track published`; nenhum vídeo/áudio nos dois sentidos.
- **Causa raiz:** a `SfuCall` é criada *dentro* do handler `signal.on('joined')` (via `callHolder` em `Room.tsx`). Se a oferta inicial for enviada por um `signal.on('joined')` registado **no construtor da `SfuCall`**, esse listener regista-se DEPOIS do evento já ter disparado → a oferta nunca sai.
- **Regra:** a `SfuCall` envia o `sfu-offer` inicial **no construtor** (dentro de `enqueue`), nunca gateado por `joined`. Não "arrumar" isto movendo a oferta para um listener.
- **Ficheiros:** `web/src/webrtc.ts` (construtor `SfuCall`), `web/src/pages/Room.tsx` (`callHolder`).

### R2 — Reload em loop após admitir um convidado
- **Sintoma:** admitir da sala de espera → o convidado faz reload; por vezes flood/desconexão.
- **Causa raiz:** o convidado montava a `SfuCall` **enquanto aguardava** admissão → oferta stale → glare/rollback repetido → rajada de mensagens → o rate-limit WS derruba → o cliente recarrega.
- **Regra:** convidado em espera **não** cria `SfuCall`. A call só nasce no handler `joined` (após admissão real). `callHolder.start()` é idempotente (`if (callRef.current || cancelled) return`).
- **Ficheiros:** `web/src/pages/Room.tsx` (`callHolder`, handler `joined`).

### R3 — Media num só sentido / falha admissão / screen-share em K8s multi-réplica
- **Sintoma:** com ≥2 réplicas, media só num sentido; admissão e partilha de ecrã falham intermitentemente.
- **Causa raiz:** o SFU é **in-memory por pod**; o Redis fana sinalização/presença mas NÃO RTP. Pares da mesma sala em pods diferentes = split-brain. A afinidade por sala depende de `/ws` ter um **Service DEDICADO** — se `/ws` partilhar Service com `/api`/`/rtc`, o ingress-nginx funde os backends e **descarta** o `upstream-hash-by`.
- **Regra:** manter o Service dedicado `delonix-server-ws` + ingress `upstream-hash-by: $arg_room` e o cliente a enviar `/ws?...&room=CODE`. Verificar: `curl .../ws?room=X` repetido cai sempre no mesmo pod. `/rtc` NÃO precisa de afinidade.
- **Ficheiros:** `deploy/k8s/*-ingress.yaml`, `*-server.yaml` (Service `delonix-server-ws`), `web/src/signaling.ts` (`&room=`).

### R4 — ICE "liga" mas o vídeo fica preto em K8s (hairpin relay-a-relay)
- **Sintoma:** `pc connected`, `track published`, mas o tile do outro fica preto (sem frames). Logs coturn: sessões com `reason: allocation timeout` e **`peer usage: rp=0`** (nunca relayou um pacote de peer).
- **Causa raiz:** o IP do pod (10.244.x) é inalcançável de fora → o SFU precisa de relay. MAS forçar `iceTransportPolicy:relay` nos **DOIS** lados pelo MESMO coturn faz o candidato de cada lado ser `coturn_ip:porta` → o "peer" que cada alocação tenta alcançar é o **próprio IP do coturn** → o coturn nega (hairpin: `403 Forbidden IP` p/ o próprio IP e loopback) → `peer rp=0` → timeout → preto. **NÃO é** instabilidade do cliente TURN nem o `438 Stale nonce` (esse é tratado pelo webrtc-rs: atualiza nonce e reenvia; ver `relay_conn.rs`).
- **Regra:** relay-only **só no SFU** (`sfu.rs` `RTCConfiguration`); o **cliente fica `all`** (`/api/ice` NÃO emite `iceTransportPolicy:relay`) para oferecer candidato host/srflx com IP real. O par vira cliente-host(IP real) ↔ SFU-relay, e o peer da alocação do SFU é o IP real do cliente — que o coturn relaya. coturn alcançável (stage: HOST via `deploy/run-host-coturn.sh`, sem `--Verbose`, `TURN_HOST=172.30.0.1:3478`). Em local não ligar relay.
- **Diagnóstico (repro sem 2 browsers):** dentro do container, `turnutils_peer -p 3480 &` + `turnutils_uclient -W <secret> -u t -e <IP-real> -r 3480 -p 3478 -n 5 <coturn-ip>` → 0% perda com IP real, 100% com o próprio IP do coturn. Confirma o hairpin.
- **Ficheiros:** `server/src/config.rs` (`force_turn_relay`), `server/src/rooms.rs` (`ice_servers` — cliente `all`), `server/src/sfu.rs` (`RTCConfiguration` relay-only), `deploy/run-host-coturn.sh`.
- **⚠ Limitação:** se AMBOS os lados forem mesmo obrigados a relay (NAT simétrico sem srflx), o hairpin volta → produção precisa de coturn com IP alcançável + ≥1 lado com candidato não-relay, ou TURN dedicado. Ver memória `k8s-media-turn`.

### R5 — `IVFWriter` PTS pela contagem de frames (gravação em velocidade errada)
- **Sintoma:** vídeo gravado acelerado/lento.
- **Causa raiz:** o `IVFWriter` da lib usa contador de frames como PTS.
- **Regra:** `recorder.rs` usa PTS em **ms reais do RTP**; dims VP8 lidos do keyframe e corrigidos no close. Não reverter para o writer default.
- **Ficheiros:** `server/src/recorder.rs`.

## Sinalização / servidor autoritativo

### R6 — Rate-limit WS derruba o próprio anfitrião
- **Sintoma:** o host cai durante a rajada de ICE/renegociação.
- **Causa raiz:** janela fixa apertada não absorve a rajada legítima de ICE.
- **Regra:** manter **token bucket** (600 burst / 300 sustained) em `signaling.rs`. Não voltar a janela fixa baixa.
- **Ficheiros:** `server/src/signaling.rs`.

### R7 — Ações de sala partilhadas decididas no cliente
- **Sintoma:** whiteboard fecha só para um; screen-share parado não limpa a apresentação para os outros; painel de transcrição abre para todos.
- **Causa raiz:** cliente a decidir estado partilhado sozinho.
- **Regra:** servidor autoritativo — `wb-close`, `Presenting`/limpar apresentação ao parar share, e o gate de transcrição são difundidos/validados em `signaling.rs`. Transcrição é **host-only** e host-gated (só o anfitrião liga; cada cliente transcreve o próprio mic; fallback Web Speech→Whisper WASM local).
- **Ficheiros:** `server/src/signaling.rs`, `web/src/pages/Room.tsx`.

## Build / deploy

### R8 — `make stage` falha no build do web
- **Sintoma:** build da imagem web falha (falta `web/dist` ou lê certos de dev).
- **Causa raiz A:** `.dockerignore` a excluir `web/dist` — mas o `Dockerfile.web.stage` faz `COPY web/dist`.
- **Causa raiz B:** `vite.config.ts` a ler certos de dev também no `build`.
- **Regra:** `.dockerignore` exclui `server/target`, `web/node_modules`, `web/public/{ort,ort-rvm,models/*}`, `deploy/*.env`, `agents/worktrees` — **nunca** `web/dist`. `vite.config.ts` lê certos de dev só no `serve`.
- **Ficheiros:** `.dockerignore`, `web/vite.config.ts`, `Dockerfile.web.stage`.

### R9 — Migração nova não aplica / Rust não recompila
- **Regra:** migrações re-embebem só com `touch server/src/main.rs`; após migração nova sempre `cargo build --release` antes de restart. reqwest **0.12 rustls-tls** (não 0.13).
- **Ficheiros:** `server/src/main.rs`, `server/Cargo.toml`.

## Auth / presença

### R10 — `/rtc` devolve 401 na primeira ligação
- **Causa raiz:** access token expirado ao abrir o WS de presença.
- **Regra:** `presence.ts` refresca o token proativamente (`jwtExpired()`) **antes** de ligar o `/rtc`.
- **Ficheiros:** `web/src/presence.ts`.

## Frontend

### R11 — Tiles do grid congelam em janela background
- **Regra:** nunca `var()` CSS para largura/altura de tiles — dimensões inline por tile (`useGridLayout` + `ResizeObserver`).
- **Ficheiros:** `web/src/pages/Room.tsx`, `web/src/styles/`.

### R12 — Poda de chaves i18n apaga chaves genéricas
- **Regra:** a poda por regex é greedy (apagou `common.save`) — cuidado com chaves curtas/genéricas ao podar.
- **Ficheiros:** `web/src/i18n.ts`.
