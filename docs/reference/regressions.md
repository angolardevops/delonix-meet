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
- **Guardado por:** [`docs/adr/0001-room-shard-affinity.md`](../adr/0001-room-shard-affinity.md) (a decisão) + `scripts/check-room-affinity.sh` (fitness function, corre em `make fitness`/`make test`).

### R4 — ICE "liga" mas o vídeo fica preto em K8s (hairpin relay-a-relay)
- **Sintoma:** `pc connected`, `track published`, mas o tile do outro fica preto (sem frames). Logs coturn: sessões com `reason: allocation timeout` e **`peer usage: rp=0`** (nunca relayou um pacote de peer).
- **Causa raiz:** o IP do pod (10.244.x) é inalcançável de fora → o SFU precisa de relay. MAS forçar `iceTransportPolicy:relay` nos **DOIS** lados pelo MESMO coturn faz o candidato de cada lado ser `coturn_ip:porta` → o "peer" que cada alocação tenta alcançar é o **próprio IP do coturn** → o coturn nega (hairpin: `403 Forbidden IP` p/ o próprio IP e loopback) → `peer rp=0` → timeout → preto. **NÃO é** instabilidade do cliente TURN nem o `438 Stale nonce` (esse é tratado pelo webrtc-rs: atualiza nonce e reenvia; ver `relay_conn.rs`).
- **SOLUÇÃO CANÓNICA (✅ validada 2 browsers, 11/07/2026 — NÃO regredir):**
  1. **coturn IN-CLUSTER + LoadBalancer** (`deploy/k8s/51-coturn.yaml`): coturn como pod normal + Service LoadBalancer (VIP metallb `172.30.0.201`). **NUNCA coturn-no-host da mesma máquina do kind** — o relay UDP pod→bridge-docker é partido pelo SNAT (100% perda; provado). Pod E browser alcançam o VIP a 0% perda.
  2. `external-ip` **=** `allowed-peer-ip` **=** o IP do LB (`--allowed-peer-ip` autoriza o hairpin: em relay-only nos 2 lados o peer é o próprio IP do coturn; sem o flag = 403 Forbidden). Clientes só usam a 3478 (relay-a-relay é interno) → o LB só expõe UDP 3478.
  3. App: `TURN_HOST=<VIP>:3478` (**NÃO** DNS de ClusterIP — o browser não resolve), `FORCE_TURN_RELAY=1` (SFU **e** cliente relay-only → poucos candidatos, sem explosão de ICE), `SFU_EXTERNAL_IP=""`.
  4. `securityContext` do coturn: `capabilities.add:[NET_BIND_SERVICE]`, **não** `drop:[ALL]`+`allowPrivilegeEscalation:false` (o binário tem file-caps → EPERM no exec).
  - **Cliente relay-only, NÃO `all`:** forçar `all` num host multi-homed gera dezenas de host candidates (explosão de ICE) que inundam o WS → "Ligação instável". Produção real: trocar o VIP metallb por LB de cloud com IP público; resto igual.
- **Diagnóstico (repro sem 2 browsers):** `turnutils_uclient -y -W <secret> -u t -n 6 -m 2 <coturn-ip>` (c2c) de um pod E do host → **0% perda** = OK. `turnutils_peer -p 3480 &` + `turnutils_uclient ... -e <peer-ip> -r 3480 ...` para testar peer específico. Sinal de saúde: `logs -l app=coturn | grep "peer usage"` com `rb>0`.
- **Ficheiros:** `deploy/k8s/51-coturn.yaml` (Deployment+LB), `deploy/k8s/01-config.yaml` (`TURN_HOST`/`FORCE_TURN_RELAY`), `Makefile` (targets `stage`/`prod` aplicam o `51-coturn.yaml`), `server/src/rooms.rs` (`ice_servers` relay-only), `server/src/sfu.rs` (`RTCConfiguration` relay-only). O webrtc-rs TURN client trata o `438 Stale nonce` sozinho (não é bug).
- **⚠ Deploy-path (não regredir):** só existe UM conjunto de manifests (namespace `delonix-meet`) — o `make stage`/`prod` e o `kustomization.yaml` aplicam os mesmos ficheiros. Os antigos duplicados 10/20/30/40 (namespace `delonix`) foram eliminados porque causavam drift (a config ia para lá e nunca chegava ao cluster). Editar `01-config.yaml` + `51-coturn.yaml`, não recriar um "Set B".

### R5 — `IVFWriter` PTS pela contagem de frames (gravação em velocidade errada)
- **Sintoma:** vídeo gravado acelerado/lento.
- **Causa raiz:** o `IVFWriter` da lib usa contador de frames como PTS.
- **Regra:** `recorder.rs` usa PTS em **ms reais do RTP**; dims VP8 lidos do keyframe e corrigidos no close. Não reverter para o writer default.
- **Ficheiros:** `server/src/recorder.rs`.

## Sinalização / servidor autoritativo

### R6 — Rate-limit WS derruba o próprio anfitrião
- **Sintoma:** o host cai durante a rajada de ICE/renegociação.
- **Causa raiz:** janela fixa apertada não absorve a rajada legítima de ICE.
- **Regra:** manter **token bucket** (600 burst / 300 sustained no `/ws`; 120/60 no `/rtc`). Não voltar a janela fixa baixa.
- **Guardado por:** `rate_limit::TokenBucket` (struct única, usada por `signaling.rs` e `presence.rs`) + testes deterministas `r6_*` em `server/src/rate_limit.rs` (correm em `make test`). Um deles prova por contraste que a janela fixa cortaria a rajada.
- **Ficheiros:** `server/src/rate_limit.rs` (`TokenBucket`), `server/src/signaling.rs`, `server/src/presence.rs`.

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
