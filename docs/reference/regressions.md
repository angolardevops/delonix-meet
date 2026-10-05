# Regressões conhecidas — NÃO reintroduzir

> Cada entrada aqui já quebrou produção/demos pelo menos uma vez. São armadilhas onde a "correção óbvia" reintroduz o bug. Antes de mexer no código relacionado, lê a entrada. Revisores (`.claude/agents/delonix-meet-*`) devem verificar estas explicitamente no diff.

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
- **Ficheiros:** `web/src/pages/Room.tsx` (`callHolder`, handler `joined`), `web/src/sfuLifecycle.ts` (`makeCallHolderStart` — a guarda extraída).
- **Guardado por (R1+R2):** `web/src/sfuLifecycle.ts` (a guarda de idempotência/não-montar-em-espera vive num só sítio testado) + `web/src/sfuLifecycle.test.ts` (vitest: 3 testes que codificam R2; correm em `make test`). R1 (oferta no construtor) fica garantido pelo `SfuCall` que o `create` instancia.

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

### R13 — Glare do lado do SERVIDOR: partilha de ecrã que nunca aparece
- **Sintoma:** partilha de ecrã (ou ligar a câmara a meio) simplesmente não chega aos outros. Intermitente — acontece quando alguém entra/sai na mesma janela de tempo. Nos logs, só um `warn` "sfu message failed".
- **Causa raiz:** o cliente TAMBÉM oferta (`startScreen`/`stopScreen`/`enableVideo` em `webrtc.ts`). Se o `renegotiation_loop` do servidor tinha uma oferta por responder (espera até 10 s), o `set_remote_description(offer)` do cliente caía em `HaveLocalOffer + SetRemote(Offer)` — transição **inexistente** em `webrtc-rs` 0.17 (`signaling_state.rs`), que **não faz rollback implícito** e nem sequer aceita `set_local_description(rollback)` a partir de `have-local-offer`. A oferta era descartada; o cliente fazia rollback e respondia à oferta do servidor, ficando com a track de ecrã adicionada mas **nunca negociada** e sem nada que voltasse a ofertar.
- **Regra:** ofertas do cliente, respostas do cliente e renegociações do servidor passam TODAS pelo canal único `NegoMsg` → `negotiation_loop` (um peer = uma negociação de cada vez). Uma oferta do cliente que chegue com a nossa pendente é **adiada** (`deferred`) e aplicada quando a PC volta a `stable` — nunca descartada. Em timeout, re-ofertar (`have-local-offer → SetLocal(offer)` É válido) até 3 vezes. **Não** voltar a aplicar `set_remote_description` diretamente no `on_client_msg`.
- **Observabilidade:** `delonix_sfu_offers_deferred_total` (glare a acontecer) e `delonix_sfu_renegotiations_failed_total` (peer que ficou sem media nova).
- **⚠ São DUAS metades — adiar no servidor não chega.** O cliente resolve o glare com `rollback`, e o rollback **descarta a oferta dele**: as tracks que ela publicava ficam por negociar, e a resposta que o servidor acabar por mandar à oferta adiada é descartada pelo cliente (já está `stable`). Sem uma **RE-OFERTA** do cliente logo a seguir a responder, a partilha de ecrã desaparecia à mesma — mesmo com o servidor corrigido. Esta metade foi descoberta pelo teste e2e, não por leitura do código.
- **Guardado por:** `server/src/sfu_e2e.rs` (`client_offer_during_server_offer_is_deferred_not_dropped` — prova que o servidor adia em vez de perder) + `web/src/glare.test.ts` (prova o rollback→resposta→re-oferta; e que SEM glare não se re-oferta, senão era renegociação a mais em cada subscrição).
- **Nota de âmbito:** o cliente de teste em Rust é webrtc-rs e **não tem rollback**, por isso não consegue encenar a metade do cliente — daí o teste do lado web.
- **Ficheiros:** `server/src/sfu.rs` (`NegoMsg`, `negotiation_loop`, `run_renegotiation`, `apply_client_offer`).

### R14 — PLI periódico a queimar bitrate (vídeo aos "solavancos")
- **Sintoma:** vídeo com picos de bitrate e blocos/"pumping" regulares em redes limitadas; CPU do publicador acima do esperado.
- **Causa raiz:** um ticker de PLI de 3 s **por publicação** — e com simulcast cada camada é uma publicação, logo 3 tickers por câmara — forçava um keyframe a cada 3 s para sempre, mesmo sem subscritores novos. A task só morria quando a PC fechava, pelo que continuava a correr contra SSRCs já mortos (ex.: partilha de ecrã parada). Existia porque o PLI/FIR **dos subscritores era deitado fora** na drenagem de RTCP do sender, deixando o ticker como única forma de recuperar um keyframe perdido.
- **Regra:** keyframes só a pedido — subscrição nova, troca de camada, ou PLI/FIR **reencaminhado** do subscritor para o publicador (com rate-limit de 1 s por publicação, `Publication::pli_allowed`). Não reintroduzir tickers periódicos.
- **Observabilidade:** `delonix_sfu_keyframes_requested_total`.
- **Ficheiros:** `server/src/sfu.rs` (`request_keyframe`, drenagem RTCP em `subscribe_layer`).

### R15 — Camada simulcast fixa: o downlink não escalava para quem já estava na sala
- **Sintoma:** salas grandes continuam pesadas para os participantes antigos; só quem entra por último recebe camada leve.
- **Causa raiz:** a camada era decidida **apenas** quando chegava uma publicação nova. Ao entrar o 9.º participante, só ele subscrevia em `q`; os 8 anteriores mantinham `h`/`f` para sempre. Não havia qualquer sinal de rede a influenciar a escolha — um participante em ligação fraca recebia a camada cheia até o vídeo colapsar.
- **Regra:** `reevaluate_peer` reavalia TODAS as subscrições de um peer a partir de (tamanho da sala + `Quality.shift` derivado dos Receiver Reports RTCP dele). Chamada em cada entrada/saída (`reevaluate_room`) e sempre que a perda de pacotes muda de nível. `pick_layer` mantém a camada atual enquanto a desejada não existir — senão o arranque `q`→`h`→`f` de cada publicador gerava 3 renegociações.
- **Observabilidade:** `delonix_sfu_layer_switches_total`, `delonix_sfu_degraded_subscribers`.
- **Guardado por:** testes `layer_follows_room_size_and_loss`, `quality_downgrades_fast_and_upgrades_slow`, `pick_layer_keeps_current_while_wanted_is_missing` em `server/src/sfu.rs`.
- **Ficheiros:** `server/src/sfu.rs` (`wanted_rid`, `Quality`, `pick_layer`, `reevaluate_peer`).

### R16 — Lock dos subscritores retido através do `await` (um cliente lento trava a sala)
- **Sintoma:** com um participante em rede má, TODA a sala engasga; entradas/saídas ficam lentas.
- **Causa raiz:** a bomba de RTP fazia `subscribers.lock().await` e escrevia para cada subscritor **com o lock retido**. Uma escrita lenta bloqueava a entrega a todos os outros (head-of-line blocking) e, pior, o `remove_peer` esperava por esse mesmo lock **enquanto retinha `publications`** — bastava um cliente lento para congelar a sala inteira.
- **Regra:** a bomba mantém um **snapshot** dos destinos e só volta a pegar no lock quando `Publication::subs_version` muda; as escritas acontecem FORA do lock. Chamar `touch_subs()` SEMPRE a seguir a inserir/remover subscritores, senão o snapshot fica stale (media a ir para quem saiu / a não ir para quem entrou).
- **Ficheiros:** `server/src/sfu.rs` (`Publication::subs_version`/`touch_subs`, bomba de RTP em `handle_publish`).

### R17 — Gravação perdida quando a sala cai por falha de ICE
- **Sintoma:** gravação server-side desaparece; `tmp-<uuid>` órfão no volume de gravações.
- **Causa raiz:** `on_peer_connection_state_change(Failed)` chamava `remove_peer` e **descartava** o `Option<RecordingSession>` devolvido. Se era o último peer, a sessão nunca chegava ao `recorder::finalize`.
- **Regra:** a sessão vai para `SfuState::orphan_recordings` e o `remove_peer` seguinte (do `signaling.rs`) recolhe-a — inclusive quando a sala já não existe.
- **Observabilidade:** `delonix_sfu_recordings_orphaned_total`.
- **Ficheiros:** `server/src/sfu.rs` (`orphan_recordings`, `remove_peer`).

### R18 — Gravação corrompida em silêncio quando o codec não é VP8/Opus
- **Sintoma:** gravação na biblioteca com vídeo ilegível, sem qualquer erro.
- **Causa raiz:** `recorder.rs` despacketiza **sempre** como VP8 e escreve IVF `VP80`. O `MediaEngine` regista VP9/H264/AV1, por isso basta o browser negociar outro codec: o depacketizer devolve lixo, o `is_key` lê bits errados e o ffmpeg compõe na mesma.
- **Regra:** `recordable_codec()` verifica o mime real da track antes de abrir writer; codec não suportado → track **excluída** da gravação + `error!` no log. Melhor não gravar do que gravar corrompido. (Aberto: restringir o `MediaEngine` a VP8+Opus resolveria de vez, ao custo do H264.)
- **Ficheiros:** `server/src/sfu.rs` (`recordable_codec`, `handle_publish`, `start_recording`), `server/src/recorder.rs`.

### R19 — Esconder um tile SILENCIA o participante
- **Sintoma:** ativar «Ocultar participantes sem vídeo» e deixar de ouvir essas pessoas. Como a maioria está com a câmara desligada, o utilizador fica praticamente surdo sem perceber porquê.
- **Causa raiz:** o `<audio>` de cada peer vivia **dentro** do `RemoteTile`. Qualquer decisão de layout que escondesse o tile (filtro, palco, paginação) desmontava o elemento e parava a reprodução.
- **Regra:** o áudio de TODOS os participantes é reproduzido pelo componente `AudioSink`, sempre montado e **fora** da `video-area`. O que está no ecrã é layout; o que se ouve não pode depender do layout. Não voltar a pôr `<audio>` dentro de um tile.
- **Ficheiros:** `web/src/pages/Room.tsx` (`AudioSink`, `PeerAudio`, `RemoteTile`).

### R20 — Quem entra sem microfone nunca consegue falar
- **Sintoma:** entrar com o mic negado/ocupado (ou em modo espectador), clicar depois no microfone: o botão acende, o medidor de nível mexe, e ninguém ouve.
- **Causa raiz:** `replaceAudioTrack` procurava `getSenders().find(s => s.track?.kind === 'audio')`. Sem áudio inicial não há sender nenhum (só câmara) ou o transceiver é `recvonly` com `sender.track === null` → **no-op silencioso**, sem renegociação. `enableVideo` tinha o fallback de `addTrack`; o áudio não.
- **Regra:** `replaceAudioTrack` (SFU **e** mesh) reaproveita o transceiver `recvonly` (`reusableTransceiver` → `replaceTrack` + `direction = 'sendrecv'`) ou faz `addTrack`, e **renegoceia**. Qualquer caminho novo de publicação de media tem de ter fallback de negociação.
- **Ficheiros:** `web/src/webrtc.ts` (`reusableTransceiver`, `SfuCall.replaceAudioTrack`, `MeshCall.replaceAudioTrack`).

### R21 — Re-render da sala 5,5×/segundo em silêncio absoluto
- **Sintoma:** CPU alto na sala, tiles a engasgar com muitos participantes.
- **Causa raiz:** `new LevelWatcher(s => setSpeaking(new Set(s)))` — o watcher dispara a cada 180 ms e o `new Set` cria sempre identidade nova, pelo que o componente `Room` (milhares de linhas, N tiles) re-renderizava ~5,5×/s durante toda a reunião, mesmo sem ninguém a falar, arrastando o efeito de talk-over.
- **Regra:** comparar o conjunto (`sameSet`) e devolver o estado anterior quando não muda. Vale para qualquer estado alimentado por um timer.
- **Ficheiros:** `web/src/pages/Room.tsx` (`sameSet`, `LevelWatcher`).

### R22 — Seleção de oradores: as três armadilhas que silenciam gente
O SFU só reencaminha os `MAX_ACTIVE_SPEAKERS` microfones mais ativos (downlink de áudio de O(n) → O(1)). Três detalhes, cada um capaz de **silenciar participantes**, e nenhum óbvio:
- **Renumeração obrigatória.** O `TrackLocalStaticRTP` reescreve SSRC e payload type mas **preserva a sequência de origem**. Suprimir pacotes sem renumerar deixa buracos que o recetor reporta como perda — e essa perda falsa faz o `Quality` (R15) baixar a camada de **vídeo** dele sem razão. Todo o áudio reencaminhado passa por `AudioMeter::next_seq()`.
- **O decaimento é por TEMPO, não por pacote.** Com o DTX ligado, quem se cala deixa de enviar pacotes; um decaimento por-pacote nunca correria e essa pessoa ficaria eternamente no top-N a **bloquear a entrada de quem começa a falar**. `AudioMeter::decay()` é chamado uma vez por tick do seletor. `observe_level` só faz o ataque (`fetch_max`).
- **Sem extensão de nível, não se suprime.** Se a extensão RFC 6464 não foi negociada, a energia é 0 para toda a gente e a ordenação seria arbitrária — silenciava pessoas ao acaso. Esses microfones passam **sempre** (`audio_level_id != 0` é condição para entrar na seleção).
- **Gravação, PSTN e áudio de ecrã nunca são suprimidos** — a seleção é uma decisão de entrega ao vivo, não pode apagar ninguém da ata nem da chamada telefónica. Na bomba de RTP, o writer e o PSTN vêm **antes** do teste de `forwarding`.
- **Observabilidade:** `delonix_sfu_audio_suppressed`.
- **Ficheiros:** `server/src/sfu.rs` (`AudioMeter`, `speaker_selector`, bomba de RTP), `server/src/sfu.rs` `new_api` (registo da extensão).

### R23 — Paginação da grelha: o servidor tem de saber quando ela desliga
- **Sintoma:** a sala encolhe abaixo do limiar de paginação e alguns participantes ficam sem vídeo para sempre.
- **Causa raiz:** o cliente envia `video-interest` com a página visível e o SFU deixa de enviar o resto. Se o cliente simplesmente **parasse** de enviar quando a paginação deixa de ser precisa, o servidor ficava com a última página em memória.
- **Regra:** o cliente envia `video-interest` **sempre** que o conjunto muda — com a página visível quando pagina, e com **todos** os peers quando não pagina. Nunca "deixar de enviar" como forma de dizer "todos". No servidor, `video_interest: None` (nunca recebido) = todos, para clientes antigos.
- **Nota:** só afeta `video`. Áudio e ecrã partilhado nunca dependem do interesse — ver R19.
- **Ficheiros:** `web/src/pages/Room.tsx` (`videoInterest`), `server/src/sfu.rs` (`set_video_interest`, `reevaluate_peer`), `server/src/signaling.rs` (`ClientMsg::VideoInterest`).

### R24 — Desligar a câmara não pode acrescentar m-lines
- **Sintoma:** depois de alguns ciclos desligar/ligar câmara, a SDP cresce e o simulcast desaparece.
- **Causa raiz:** libertar mesmo a câmara (`track.stop()`, para o LED apagar) faz `getSenders().find(s => s.track?.kind === 'video')` deixar de encontrar nada — e o `enableVideo` criava um transceiver **novo** a cada religação. As `sendEncodings` de simulcast só podem ser definidas na criação do transceiver, pelo que o novo vinha sem elas.
- **Regra:** `SfuCall` guarda `videoSender`; `disableVideo()` faz `replaceTrack(null)` (mantém o transceiver, não renegoceia) e `enableVideo()` reutiliza esse sender. Não voltar a `enabled = false` (mantinha a captura e o LED acesos) nem a criar transceiver por religação.
- **Ficheiros:** `web/src/webrtc.ts` (`videoSender`, `disableVideo`), `web/src/pages/Room.tsx` (`toggleCam`).

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

### R32 — Fila de saída ILIMITADA: um consumidor lento derrubava o nó
- **Sintoma:** memória do pod a subir sem parar e OOM-kill, levando consigo TODAS as salas do pod. Sem erro, sem aviso, sem correlação óbvia com nada.
- **Causa raiz:** as cinco filas de saída eram `unbounded_channel`. O `writer` de cada socket só drena ao ritmo a que o TCP do cliente aceita bytes; um cliente em rede degradada (o caso NORMAL do nosso mercado), com a aba suspensa ou parado num depurador, deixa de drenar — e a sala continua a difundir-lhe traços de quadro, legendas parciais e ICE. Com a afinidade por sala (ADR-0001) a concentrar salas no mesmo pod, UM participante derrubava todas as outras. Era um DoS ao alcance de qualquer participante.
- **Regra:** **nenhuma fila de saída sem limite.** `WS_QUEUE_CAP` (default 512) e `NEGO_QUEUE_CAP` (default 64). Cheia: descarta-se só o EFÉMERO e auto-substituível (legenda parcial, traço de quadro, reacção — `ServerMsg::is_droppable`) e conta-se; com uma mensagem de PROTOCOLO ou ESTADO fecha-se o socket UMA vez e o cliente reentra. Entregar meio protocolo é pior do que desligar: deixa o cliente a acreditar num sistema que já não existe. Nunca `send().await` — os emissores correm dentro do lock do `DashMap` das salas (R16); é sempre `try_send`.
- **O fecho tem de ser ORDENADO:** acordar o laço de LEITURA por `Notify`, para a saída do laço correr a limpeza normal do peer. Abortar só a task de escrita NÃO serve — com `split()` as duas metades partilham o socket, e o peer ficaria na sala com o caminho de saída morto, que é pior que o problema original.
- **Ficheiros:** `server/src/signaling.rs` (`PeerTx`), `server/src/presence.rs` (`ConnTx`), `server/src/sfu.rs`, `server/src/config.rs`, `server/src/metrics.rs`.

### R33 — Bandeira de coalescing presa: peer sem renegociar NUNCA MAIS
- **Sintoma:** um participante deixa de receber media nova — quem entra depois dele fica invisível para ele, para sempre, sem erro nenhum.
- **Causa raiz:** o `trigger_renegotiate` levanta `renegotiate_queued` ANTES de enviar, para colapsar rajadas de subscrição numa só oferta. Enquanto a fila era ilimitada o envio nunca falhava. Ao limitá-la passou a poder falhar — e a bandeira ficava a `true` com o pedido perdido, estado do qual não há saída: toda a renegociação futura é coalescida contra um pedido que não existe.
- **Regra:** quem levanta a bandeira ANTES de enviar tem de a **repor em falha** (`coalesce_renegotiate`). Vale para qualquer coalescing futuro, não só este.
- **Ficheiros:** `server/src/sfu.rs` (`coalesce_renegotiate`, `trigger_renegotiate`); teste `renegotiate_flag_is_restored_when_the_queue_rejects`.

### R30 — O Ansible voltava a confiar no `:latest`
- **Sintoma:** o deploy kind injecta no cluster uma imagem velha, ou a tarefa falha por não encontrar a tag.
- **Causa raiz:** `kind_host` usava `image_tag | default('latest')` com `image_tag` **indefinido em lado nenhum** — logo era sempre `:latest`. Só que o `make export-images` deixou de exportar `:latest` DE PROPÓSITO (é a regra «nunca confiar no `:latest`», ver R9 e o HARNESS.md). Um `default` silencioso para a tag errada é precisamente o que essa regra proíbe.
- **Regra:** a tag é **obrigatória e explícita** (`assert` no role), passada pelo `make` (`-e image_tag=$(IMAGE_TAG)`, o mesmo `git describe` com que as imagens foram construídas). Nunca um `| default('latest')`.
- **Ficheiros:** `deploy/ansible/roles/kind_host/tasks/main.yml`, `Makefile` (`deploy-kaeso`).

### R31 — Um default global partiu um modo de deploy inteiro
- **Sintoma:** todos os deploys single-host abortam à saída da caixa, sem ninguém ter escolhido motor nenhum.
- **Causa raiz:** o default global passou a `container_engine: delonix` (para o caminho kind) e o role `single_host` tem uma guarda que falha para tudo o que não seja docker — o compose dele usa `-f a -f b` e `--wait`, que o delonix não suporta.
- **Regra:** um default global novo tem de ser confrontado com TODAS as guardas que dependem dessa variável. Fixado nos `vars` da play (vence `group_vars`, perde para `-e`), portanto quem escolher outro motor de propósito continua a receber a mensagem da guarda — que passou a nomear a causa real em vez de sugerir só «usa docker».
- **Ficheiros:** `deploy/ansible/site.yml`, `deploy/ansible/roles/single_host/tasks/main.yml`, `deploy/ansible/group_vars/all.yml`.

## Auth / presença

### R10 — `/rtc` devolve 401 na primeira ligação
- **Causa raiz:** access token expirado ao abrir o WS de presença.
- **Regra:** `presence.ts` refresca o token proativamente (`jwtExpired()`) **antes** de ligar o `/rtc`.
- **Ficheiros:** `web/src/presence.ts`.

### R25 — Tomada de conta: a autoridade de autenticação escolhida por sorteio
- **Sintoma:** nenhum. É a pior classe — o atacante entra como a vítima e tudo parece normal. Descoberto em revisão de código, nunca em produção (2026-08-06).
- **Causa raiz:** quatro elos, cada um razoável sozinho. (1) `PUT /integration/odoo` só exige `require_admin` da PRÓPRIA org — qualquer utilizador cria uma org e aponta-a a um Odoo que controla. (2) `odoo_sso::upsert_member` casava por email e fazia `UPDATE users SET odoo_uid, odoo_managed = TRUE` — reclamava a conta de quem fosse listado no directório desse Odoo. (3) `odoo::org_odoo_config` escolhia contra QUE Odoo validar a password juntando por `org_members` com `LIMIT 1` e **sem `ORDER BY`** — para quem estivesse em várias orgs, saía uma arbitrária. (4) `auth::login` valida então a password contra esse Odoo, que responde "autenticado" ao que o atacante quiser — e o código grava essa password como hash local da vítima.
- **Regra:** a autoridade de autenticação de uma conta é **`users.odoo_org_id`** — a org que a GERE, gravada quando a conta nasce de um Odoo e nunca reescrita por outra. NULL = conta local, autenticada localmente. **Nunca** resolver o provedor de autenticação por email nem por pertença a org. E uma sincronização de directório **nunca reclama uma conta existente**: nem de outra org (a mesma regra `ForeignOrg` que `meetings_v1::resolve_org_user` já aplicava), nem local — ligar uma conta local a um Odoo é acto do DONO, não efeito lateral de alguém escrever o endereço dela algures. As duas metades são precisas: fechar só uma deixa a porta entreaberta.
- **Armadilha ao corrigir:** o caminho de corrida do `unique_violation` relia por email e devolvia o `id` — reabria a reclamação por essa porta. Tem de reaplicar a MESMA regra de autoridade.
- **Custo conhecido:** quem já tenha conta local e apareça depois no Odoo da org **não entra por SSO** até existir um fluxo deliberado de ligação de conta. Por desenhar.
- **Ficheiros:** `server/migrations/0033_user_odoo_authority.sql`, `server/src/odoo_sso.rs` (`upsert_member`), `server/src/odoo.rs` (`org_odoo_config`), `server/src/auth.rs` (login).

### R26 — Debandada de sincronizações de directório contra o ERP
- **Sintoma:** numa manhã de segunda (toda a empresa a entrar), o Odoo e o Postgres levam N leituras completas do directório em paralelo, uma por login.
- **Causa raiz:** o carimbo `odoo_synced_at` só era escrito no FIM da sincronização e o teste de frescura só LIA. Entre o teste e a escrita cabiam todos os logins concorrentes.
- **Regra:** reivindicar a sync **ATOMICAMENTE antes** de a fazer — `UPDATE ... WHERE <velho> RETURNING`, que o Postgres serializa na linha; quem recebe zero linhas desiste. Sem lock aplicacional. Uma sync que FALHA repõe o carimbo anterior, senão adia a tentativa seguinte por `SYNC_MAX_AGE_SECS` inteiros.
- **Medido:** com 5 reivindicações simultâneas ganha exactamente 1. O `RETURNING` com subconsulta correlacionada devolve o valor ANTERIOR (é disso que a reposição depende).
- **Ficheiros:** `server/src/odoo_sso.rs` (`claim_directory_sync`, `spawn_directory_sync`).

## Frontend

### R11 — Tiles do grid congelam em janela background
- **Regra:** nunca `var()` CSS para largura/altura de tiles — dimensões inline por tile (`useGridLayout` + `ResizeObserver`).
- **Ficheiros:** `web/src/pages/Room.tsx`, `web/src/styles/`.

### R12 — Poda de chaves i18n apaga chaves genéricas
- **Regra:** a poda por regex é greedy (apagou `common.save`) — cuidado com chaves curtas/genéricas ao podar.
- **Ficheiros:** `web/src/i18n.ts`.

## API pública v1

### R27 — Convidados "ignorados" eram APAGADOS da reunião
- **Sintoma:** um `PATCH /api/v1/meetings/{id}` com convidados que o servidor ignora remove-os da reunião; com a lista TODA ignorada, a lista de convidados desaparece inteira.
- **Causa raiz:** a remoção era `NOT (user_id = ANY(<resolvidos>))`. Um convidado devolvido como `skipped` não entra nos resolvidos e era portanto apagado — apesar de o chamador o ter listado e de a resposta lhe dizer «ignorado», não «removido». Com tudo ignorado os resolvidos ficam vazios, e em Postgres `x = ANY('{}')` é **FALSE** → `NOT FALSE` é TRUE para todas as linhas.
- **Regra:** decidir a remoção pelos **emails PEDIDOS**, nunca pelos que resolveram. Quem foi pedido fica, tenha ou não sido possível (re)adicioná-lo.
- **Ficheiros:** `server/src/meetings_v1.rs` (`PATCH`, bloco `invitees`).

### R28 — Fallback de idempotência inalcançável E destrutivo
- **Sintoma:** erro devolvido depois de a base de dados ter criado e apagado uma reunião e uma sala para nada.
- **Causa raiz:** com a linha de `meeting_external_refs` presente mas a reunião irresolúvel, um `if let Ok(...)` caía para «criar de novo». Esse caminho não pode ter sucesso: a linha velha continua lá, o INSERT do `external_ref` colide sempre, o tratamento da colisão APAGA a reunião e a sala acabadas de criar, relê o MESMO id que já falhara e propaga o erro à mesma.
- **Regra:** a linha de mapeamento cai em CASCATA com a reunião (migração 0031) — se a linha existe, a reunião existe. Não a conseguir resolver é estado **incoerente**: dizê-lo, não mascarar com um «criar de novo» que escreve e apaga sem poder ter sucesso.
- **Ficheiros:** `server/src/meetings_v1.rs` (`POST`, bloco de idempotência).

### R29 — Deduplicação de org que não deduplica
- **Sintoma:** o provisionamento cria uma organização DUPLICADA para uma empresa Odoo que já tem uma — exactamente para os módulos antigos, que é a população que o bloco existe para servir.
- **Causa raiz:** a dedup exigia `odoo_db` E `odoo_company_id`, mas o segundo é `#[serde(default)]` — um módulo Odoo antigo não o envia, o `match` não casa, e cria-se org nova em silêncio.
- **Regra:** **fail-closed**. Não desdobrar para «dedup só por `odoo_db`»: uma BD Odoo hospeda VÁRIAS empresas e isso fundiria tenants distintos — pior que duplicar. Recusar com a acção concreta (actualizar o módulo).
- **Ficheiros:** `server/src/apikeys.rs` (`provision`).

### R250 — O `{kind}` dos documentos do estúdio engoliria `sources`, `pairing-codes` e `recording-target`
- **Sintoma:** (apanhado antes de sair do worktree, ADR-0014 §5.) `GET /api/orgs/{org}/studios/{id}/sources` deixa de devolver as fontes da régie e passa a `404` — ou, pior, uma lista de documentos VAZIA, que o operador lê como «não há câmaras emparelhadas» no meio de uma emissão.
- **Causa raiz:** os seis tipos de documento entram no router por UMA rota com o tipo no caminho (`…/studios/{studio_id}/{kind}`), porque o contrato dos seis é idêntico e seis cópias das mesmas seis queries é a duplicação que a catraca da arquitectura recusa. Essa rota fica IRMÃ dos segmentos concretos que já lá estavam (`sources`, `pairing-codes`, `recording-target`). Funciona porque o matcher do axum dá precedência ao segmento estático sobre o parâmetro — uma propriedade do router, não do nosso código, e invisível em qualquer teste que olhe só para um dos dois lados.
- **Regra:** os segmentos concretos continuam a ganhar ao `{kind}`. Não «arrumar» isto trocando a ordem de registo das rotas, nem passando as vizinhas concretas a `{kind}` com um `match` no handler (era o mesmo bug com mais passos). Um tipo que não seja um dos seis segmentos conhecidos é `404` em `kind_from_segment` — nunca um tipo novo criado por um caminho inventado, e nunca o valor da coluna (`mixer_scene` não abre `…/mixer_scene`). Se algum dia uma vizinha concreta nova entrar debaixo de `…/studios/{studio_id}/`, acrescenta-se ao teste.
- **Portão:** `server/tests/studio_docs.rs::r250_segmentos_concretos_ganham_ao_tipo_de_documento` — exercita as três vizinhas concretas E os seis tipos no MESMO estúdio, e exige `404` para quatro segmentos que não são nem uma coisa nem outra. O teste unitário `studio_docs::tests::so_os_seis_segmentos_conhecidos_sao_tipos` fixa a outra metade (o que conta como tipo).
- **Ficheiros:** `server/src/lib.rs` (registo das rotas do estúdio), `server/src/studio_docs.rs` (`kind_from_segment`).

## Higiene / pipeline

### R34 — Chave privada e artefactos compilados seguidos no git
- **Sintoma:** um clone do repositório traz consigo a chave privada TLS de `*.delonix.local` e um `.pyc`.
- **Causa raiz:** um `git add` num directório que ainda não estava no `.gitignore`. Não houve má-fé nenhuma — é o modo normal como isto acontece.
- **Regra:** **nenhum material de chave privada seguido, nem de dev.** Uma chave num repositório é uma chave comprometida: qualquer clone a tem. Os certificados de dev são GERADOS (`make certs`). O `check-repo-hygiene.sh` recusa por extensão E por cabeçalho PEM dentro de qualquer ficheiro seguido, mais artefactos compilados, dumps de base de dados e migrações com números repetidos ou buracos.
- **Nota que não pode faltar:** `git rm --cached` tira do HEAD, **não purga o histórico**. Uma chave que esteve seguida continua alcançável em commits anteriores e tem de ser tratada como comprometida.
- **Ficheiros:** `scripts/check-repo-hygiene.sh`, `.gitignore`, `Makefile` (`certs`).

### R35 — Documentação a descrever um sistema que já não existe
- **Sintoma:** um agente (ou um humano novo) escreve código contra a API errada, ou desenha um CI que espera uma base de dados no build.
- **Causa raiz:** a doc dizia `axum 0.7`/`sqlx 0.7` com o código em 0.8, e anunciava `sqlx::query!` com verificação em compile time quando `server/src` tem 118 chamadas à API de runtime e ZERO macros.
- **Regra:** o `check-docs-drift.sh` compara as versões das crates estruturais com o `Cargo.toml` e recusa qualquer doc que anuncie SQL verificado em compile time enquanto o código usar a API de runtime. **Um portão que nunca se viu ficar vermelho não prova nada** — os dois foram verificados a falhar com o drift reintroduzido de propósito.
- **Ficheiros:** `scripts/check-docs-drift.sh`, `HARNESS.md`, `AGENTS.md`, `GEMINI.md`.

### R36 — Par de candidatos lido pelo estado `succeeded`: TURN «nunca em uso»
- **Sintoma:** a métrica de uso de TURN responde **sempre** que a media é directa, e o par de candidatos vem a `null`. Nenhum erro, nenhum aviso — só um número errado com ar de certo.
- **Causa raiz:** a extracção procurava o `candidate-pair` com `state === 'succeeded'`. Medido contra Chromium a sério: o Chrome mantém **treze** pares em `in-progress`/`waiting` muito depois de a ligação estar feita, e o `succeeded` só aparece de forma transitória. Resultado: `null` em **16 de 16** amostras — e, com o par nulo, `turnRelay` fica sempre `false`.
- **Regra:** o par escolhido vem do **`transport.selectedCandidatePairId`**, que é o que a especificação define. O `succeeded`/`nominated` fica só como recuo para browsers que não publiquem o `transport`.
- **A lição que interessa mais do que o bug:** os testes sintéticos passaram os dois lados com a mesma suposição errada — o fixture foi escrito por quem escreveu o código. Só correr contra um browser a sério o apanhou. Um teste cujo fixture nasce da mesma cabeça que o código não é uma verificação independente.
- **Ficheiros:** `web/src/callQuality.ts`, teste `segue o selectedCandidatePairId do transport`.

### R37 — Sondar o `getStats()` mais depressa do que o browser o actualiza
- **Sintoma:** um teste conclui «sem media» numa chamada perfeitamente saudável — no cenário de REFERÊNCIA, que é onde um falso negativo mais se nota.
- **Causa raiz:** o Chrome actualiza as estatísticas ~1×/s. Duas leituras dentro do mesmo intervalo trazem o **mesmo carimbo temporal**; como toda a extracção é por delta, o resultado é zero. Sondar a 500 ms produzia uma matriz inteira de falsos negativos.
- **Regra:** nunca sondar o `getStats()` abaixo de ~1,5 s. Vale para o arnês de teste E para qualquer painel ao vivo.
- **Ficheiros:** `e2e/netem-matrix.mjs`.

### R38 — Camada simulcast escolhida por adivinhação: o palco servido em `q`
- **Sintoma:** numa sala de dez, o orador em palco a ocupar 70% do ecrã aparece borratado. Na simétrica, uma sala de três gasta banda a servir vídeo inteiro a miniaturas de 90 px.
- **Causa raiz:** `wanted_rid(kind, room_size, shift)` decidia com DOIS sinais — o número de participantes e um degrau por perda. O servidor não tem como saber o tamanho a que um tile está desenhado, se a aba está em segundo plano, se a máquina está travada por CPU, a bateria ou a poupança de dados; estava a inferir tudo isso do número de pessoas na sala.
- **Regra:** **o cliente pede, a realidade da rede corta.** A camada desejada por publicador é decidida no cliente (`web/src/layerPolicy.ts`) e enviada no `video-interest`; o servidor aplica por cima o degrau da perda MEDIDA por RTCP — que nunca é anulado pela sugestão — e limita a `MAX_FULL_LAYERS_PER_SUB` quantas camadas altas um subscritor pode segurar, porque a sugestão vem de fora.
- **Compatibilidade:** sugestão ausente ou com rótulo desconhecido ⇒ decide-se como sempre se decidiu, pelo tamanho da sala. Clientes com a app em cache antiga continuam a funcionar.
- **Medido** (2026-08-25, dois Chromium contra o SFU): subscritor em `q` = 235 kbps; sugestão `h` = 325; sugestão `q` = 93. A sugestão vale 3,5× em downlink.
- **Ficheiros:** `web/src/layerPolicy.ts`, `server/src/sfu.rs` (`wanted_rid`, `cap_full_layers`), `server/src/signaling.rs` (`VideoInterest`).

### R39 — Mensagem do cliente que não desserializa morre em SILÊNCIO
- **Sintoma:** uma funcionalidade nova simplesmente não acontece. Sem erro, sem log, sem nada para ver.
- **Causa raiz:** o handler faz `match serde_json::from_str::<ClientMsg>(&text)` e os casos que não casam caem num braço vazio. Um campo novo com a forma errada — ou um `#[serde(default)]` em falta — descarta a mensagem INTEIRA.
- **Regra:** todo o campo novo num `ClientMsg` leva um teste que desserializa **o JSON exacto que o cliente escreve**, mais um que prova que a mensagem SEM o campo continua a ser aceite (clientes com a app em cache antiga).
- **Ficheiros:** `server/src/signaling.rs`, testes `video_interest_aceita_a_sugestao_de_qualidade` e `video_interest_sem_qualidade_continua_a_ser_aceite`.

### R40 — Escrita de gravação a bloquear o executor do Tokio
- **Sintoma:** com o volume de gravações lento ou cheio, salas SEM GRAVAÇÃO NENHUMA ficam lentas. A ligação entre as duas coisas não é óbvia e o sintoma não aponta para a gravação.
- **Causa raiz:** `RecWriter::write_rtp` era chamado de dentro da task async que reencaminha RTP e escrevia com `std::fs::File`, que é **síncrono**. Um worker do Tokio bloqueado numa escrita não serve só aquela gravação — serve todas as salas que calharem naquela thread. O `BufWriter` reduziu a frequência das syscalls; não tirou a escrita do executor.
- **Regra:** a escrita corre numa **thread dedicada** por track, alimentada por uma fila LIMITADA (`REC_QUEUE_CAP`). O `write_rtp` faz `try_send` e nunca bloqueia.
- **A parte que é fácil estragar:** o `close()` tem de **esperar** a thread esvaziar a fila E fechar o ficheiro, porque o `finalize` invoca o ffmpeg logo a seguir. Fechar sem esperar dá uma gravação truncada sem um único erro pelo caminho — é a família da R18. O `close` é `async` e faz o `join` em `spawn_blocking` (fazer `join` no executor seria repor o problema que a mudança resolve). E recolhe-se o writer com o lock na mão, larga-se o lock, e só depois se espera (R16).
- **Fila cheia = perda CONTADA, nunca silenciosa:** `delonix_recording_packets_dropped_total` e um aviso a cada 500. Uma gravação degradada tem de ser visível; a alternativa (bloquear até o disco alcançar) é pior, e perder em silêncio é o pior de todos.
- **Validado com gravações REAIS** (2026-08-25, três execuções): 12 s gravados ⇒ artefactos de **12,000000 s** e **12,020000 s**, VP8 1280×720 + Opus 48 kHz estéreo, `recording_packets_dropped_total = 0`. Uma fila por esvaziar teria dado um ficheiro mais curto — é essa a prova.
- **Por validar:** só o caminho de REMUX (`-c copy`, um publicador) foi exercitado. O de RECOMPOSIÇÃO (vários publicadores → reencode VP9+Opus) não: não se conseguiu pôr dois publicadores em simultâneo neste arnês. É o caminho com mais risco e continua sem prova.
- **Exercitado a 2026-10-05 (R295):** dois publicadores em simultâneo, grelha e mistura, numa gravação a sério — e foi aí que apareceu o áudio a recuar. O que a R295 deixa por provar está nela.
- **Ficheiros:** `server/src/recorder.rs` (`RecWriter`, `RecSink`), `server/src/sfu.rs` (os três fechos), `server/src/config.rs`.

## Criptografia / E2EE

### R41 — Endpoints MLS abertos, sem autenticação, a responder «feito»
- **Sintoma:** nenhum. É esse o problema — `/api/mls/key-packages`, `/api/mls/rooms/{id}/key-packages` e `/api/mls/welcome` respondiam `201`/`200`/`202` com `"status": "delivered"` a **qualquer pessoa**, sem sessão, sem token, sem verificação de pertença à sala.
- **Causa raiz:** o `mls.rs` foi escrito como desenho da camada MLS futura e o router ficou registado no `main.rs`. Os handlers não têm sequer extractor `AuthUser`.
- **Regra:** **uma superfície que responde «feito» sem fazer nada é pior do que não existir.** Um integrador constrói contra ela e um auditor conta-a como capacidade. O módulo fica como documento de desenho; as rotas saem do router até haver MLS a sério — com `AuthUser` e `can_access_room`, que é o que lhes falta.
- **Medido** (2026-08-25): antes, 201/200/202 sem autenticação nenhuma; depois, **404** nas três.
- **Ficheiros:** `server/src/main.rs`, `server/src/mls.rs`.

### R42 — Worker de cifra a deixar passar frames EM CLARO sem chave
- **Sintoma:** media não cifrada a sair de uma sala marcada como E2EE, sem nada que o reporte.
- **Causa raiz:** `encryptFrame` fazia `if (!key) { controller.enqueue(frame); return }` — fail-**open**. Hoje o `setKey` é esperado antes de existir um único sender, por isso não acontecia; mas a garantia de confidencialidade estava a depender da ordem de chamadas num ficheiro de 4000 linhas noutro módulo.
- **Regra:** **fail-closed em cifra, sempre.** Sem chave, o frame é DESCARTADO. Sem media é um sintoma visível que alguém reporta; media em claro numa sala E2EE é uma quebra silenciosa que ninguém vê. Vale igual na decifra: entregar ciphertext ao descodificador é entregar-lhe ruído.
- **Ficheiros:** `web/src/e2ee.ts`.

### R43 — Segredo dentro de um tipo que deriva `Debug`
- **Sintoma:** a chave AES-256 da sala num ficheiro de log, em base64, pronta a ler.
- **Causa raiz:** o `ClientMsg` deriva `Debug` e a chave E2EE cedida pelo anfitrião viajava lá dentro como `String`. Nenhum log a imprimia — mas bastava um `tracing::debug!(?msg)` acrescentado por boas razões num dia mau.
- **Regra:** material de chave nunca vive num tipo que derive `Debug` sem redacção. Usa-se `signaling::Secret`, cujo `Debug` imprime `[segredo redigido]` e cujo `Drop` sobrescreve os bytes. Vale para qualquer segredo novo — tokens, passwords, chaves de API em trânsito.
- **Ficheiros:** `server/src/signaling.rs` (`Secret`), `server/src/recorder.rs` (limpeza dos bytes descodificados).

### R44 — Rota registada sem autenticação, sem nada que o detecte
- **Sintoma:** um endpoint aberto ao mundo, e nenhum sinal disso. Foi assim que o `/api/mls/*` esteve a responder `201`/`200`/`202` a qualquer pessoa (R41) — encontrado por acaso, numa auditoria manual.
- **Causa raiz:** não há middleware de autenticação global. Cada handler declara a sua — por extractor (`AuthUser`, `ApiKey`, `OdooTokenAuth`) ou por guarda no corpo (`check_media_secret`). Um handler que se esqueça fica simplesmente aberto, e compila.
- **Regra:** `check-route-auth.sh` percorre as **93 rotas** (incluindo as de routers ANINHADOS) e exige que cada uma tenha autenticação ou esteja em `scripts/rotas-publicas.txt` **com a razão escrita**. Acrescentar uma linha a esse ficheiro é uma decisão de segurança: se não se souber escrever a razão, a rota não devia ser pública.
- **O portão também falha** quando uma rota está na lista mas já ganhou autenticação (lista velha), quando a lista refere rotas que já não existem, e quando não consegue LER um handler (closure inline, assinatura não encontrada) — porque uma rota que o portão não vê é uma rota sem portão.
- **A primeira versão deste portão aprovou a reintrodução do `/api/mls` sem uma queixa**: não olhava para dentro de `.nest(...)`, que é exactamente onde o buraco estava. Um portão que não apanha o caso que o originou é decoração. Corrigido e reprovado nas quatro classes: router aninhado, rota nova sem auth, handler inline, e autenticação removida de um handler existente.
- **Ficheiros:** `scripts/check-route-auth.sh`, `scripts/rotas-publicas.txt`.

### R45 — Teste de isolamento com a expectativa errada
- **Sintoma:** um teste de segurança a acusar vulnerabilidade onde há desenho — ou, pior no sentido inverso, a passar porque exige a coisa errada.
- **Causa raiz:** exigiu-se que a org A levasse `403` ao ler uma sala da org B. Leva `200`, e está certo: o código da sala é uma **capability** à maneira do Meet. Quem o conhece vê os metadados e pode PEDIR para entrar; quem não é membro cai na sala de espera.
- **Regra:** a invariante a testar não é «o pedido é recusado», é **«A nunca obtém acesso DIRECTO à media de outra organização»** — e isso verifica-se no WebSocket, não no código HTTP. Medido: o dono recebe `joined`, a outra org recebe `waiting`.
- **A lição geral:** antes de chamar vulnerabilidade a um `200`, lê-se o desenho e verifica-se a segunda metade da promessa. O comentário no `join_room` dizia que os não-membros vão para a sala de espera; podia estar desactualizado, e por isso foi verificado no fio.
- **Ficheiros:** `web/e2e/isolamento.mjs`.

### R46 — Anel de foco assente em `box-shadow` numa folha que disputa `box-shadow`
- **Sintoma:** elementos focáveis por teclado sem indicação visível nenhuma. Eram 11 `outline: none` em `web/src/styles.scss`, vários sem substituto — incluindo o campo do Cmd-K, onde a navegação por teclado é a única forma de uso.
- **Causa raiz da primeira tentativa falhada:** a rede de segurança nasceu como `:where(button, [href], input, …):focus-visible { box-shadow: var(--ring) }`. O `:where()` tem especificidade **ZERO** — de propósito, para os componentes poderem sobrepor-se — mas isso faz com que perca para **qualquer** regra de classe que toque em `box-shadow`. E este ficheiro tem **94 declarações de `box-shadow`**, 15 delas em regras de classe, contra 22 de `outline` (11 das quais são o próprio `outline: none`).
- **Regra:** **um anel de foco global escolhe a propriedade que ninguém disputa.** `outline` é essa propriedade — e, no browser atual, segue o `border-radius`, por isso não se perde nada. `box-shadow` fica para os anéis de componente, que têm especificidade de classe para se defenderem.
- **Segunda regra, sobre inputs sem borda:** os seis sítios eram o mesmo padrão — input sem borda dentro de um contentor com borda. O anel vai no **contentor**, via `:focus-within`; no input desenhava um retângulo a flutuar dentro da peça.
- **O que NÃO ficou provado:** o anel não foi confirmado visualmente. O painel de browser usado não resolve estado de foco — uma regra `!important` *sem* pseudo-classe também não alterava o valor computado, o que mostra que o instrumento estava cego, não o CSS. Fica verificado por teste (a regra existe, tem a forma certa e usa outline) e por inspeção do CSS construído; **falta uma passagem de teclado numa janela real**.
- **Ficheiros:** `web/src/styles.scss`, `web/src/lote1.invariantes.test.ts`.

### R47 — Um widget partilhado a arrastar o módulo inteiro para o chunk de arranque
- **Sintoma:** `React.lazy` aplicado às páginas e, ainda assim, a consola inteira no bundle inicial de quem só vê a landing pública.
- **Causa raiz:** `LanguageToggle` e `ThemePicker` viviam dentro de `components/Shell.tsx`. O `Login`, a `Landing` e o `Room` importavam-nos **de lá** — e um `import { LanguageToggle } from '../components/Shell'` traz o grafo do Shell todo atrás: `CommandPalette`, `NotificationCenter`, `OnboardingTour`, `SettingsModal`, `PasswordInput`, `branding`, `api`. O mesmo se passava com o `initTheme`, importado pelo `main.tsx`.
- **Regra:** **o que é partilhado entre um ecrã leve e um ecrã pesado vive em módulo próprio.** Um named export não corta o grafo: o bundler segue o módulo inteiro. Antes de aplicar `lazy` a uma página, verifica-se quem mais importa o que ela importa.
- **Medido** (2026-08-25, `vite build`): chunk de arranque **648,82 KB → 347,40 KB** cru, **194,55 → 110,84 KB** comprimido. Na landing pública, o browser vai buscar **dois** ficheiros JS — o de entrada e, só se o utilizador clicar EN, o dicionário inglês (10,5 KB). Nem `Room` (135,47 KB), nem `Calendar`, nem `Analytics`, nem o dicionário francês são pedidos.
- **Ficheiros:** `web/src/theme.ts`, `web/src/components/{ThemePicker,LanguageToggle}.tsx`, `web/src/App.tsx`, `web/src/main.tsx`.

### R48 — Sessão terminada por um erro que não é de sessão
- **Sintoma:** o utilizador cai no ecrã de login a meio do trabalho, e voltar a autenticar-se não resolve — porque a sessão dele nunca esteve inválida.
- **Causa raiz:** o `refreshSession` fazia `if (!res.ok) { logout() }`. Qualquer resposta não-OK do `/api/auth/refresh` — 500, 502, 503, um gateway a reiniciar — era lida como «a sessão não serve». O `request` também atirava um `Error` nu, sem estado HTTP, o que obrigava quem apanha a adivinhar pela mensagem.
- **Regra:** **só 401 e 403 são sessão inválida.** Tudo o resto é o servidor com um problema seu: fica-se onde se está e oferece-se tentar de novo. A separação vive em `isAuthFailure(e)` e depende de o erro carregar o `status` — daí o `ApiError`.
- **Vem do `delonix-portal`** (`src/api/client.ts`), que já tinha pago por isto. As duas consolas partilham as armadilhas; passam a partilhar as guardas.
- **Ficheiros:** `web/src/api.ts`, `web/src/api.guardas.test.ts`.

### R49 — `.catch()` sem `isAbort` transforma limpeza de efeito em erro
- **Sintoma:** um estado de erro pintado em cada montagem, só em desenvolvimento.
- **Causa raiz:** o duplo-efeito do StrictMode monta, desmonta e volta a montar. A limpeza chama `AbortController.abort()`, o `fetch` rejeita com `AbortError`, e um `.catch()` que não distinga isso pinta erro — ou, pior, desloga. No portal isto faltava em **onze** sítios e o sintoma era a consola a saltar sozinha para o login.
- **Regra:** **todo o `.catch()` de um pedido que leva `AbortSignal` começa por `if (isAbort(e)) return`.** Abortar é a limpeza a funcionar, não a API a falhar.
- **Regra irmã:** um `.catch(() => {})` não é tratamento de erro, é supressão. O `myOrgs()` do Shell engolia até a resposta que dizia que a pessoa É admin — o menu de administração desaparecia sem nada que o explicasse.
- **Ficheiros:** `web/src/components/AsyncSection.tsx`, `web/src/components/Shell.tsx`.

### R50 — Corrigir por sobreposição em vez de apagar a regra velha
- **Sintoma:** o campo «entrar por código» invisível DENTRO da gaveta móvel que tinha acabado de ser criada para o alojar.
- **Causa raiz:** a correcção acrescentou uma camada nova com `.qa-bar { display: none }` mas deixou de pé a regra antiga `@media (max-width: 860px) { .app-bar-date, .app-bar-join { display: none } }`. Essa apanha `.app-bar-join` em QUALQUER sítio — incluindo dentro da gaveta. A gaveta abria, e o campo que ela existia para mostrar não estava lá.
- **Regra:** **quando se muda um elemento de sítio, apaga-se a regra que o escondia no sítio antigo.** Sobrepor uma regra de posicionamento resolve o caso que se está a testar e deixa o outro partido — é o achado 3.2.1 deste mesmo relatório a repetir-se em cima de si próprio.
- **Como foi apanhado:** por uma fitness function escrita ANTES de a correcção estar dada por terminada, e confirmado no browser (`getComputedStyle` do campo dentro da gaveta dava `display: none`). O teste que verifica a correcção tem de olhar para o que ela promete, não para o que ela tocou.
- **Ficheiros:** `web/src/styles.scss`, `web/src/lote2.invariantes.test.ts`.

### R51 — Um teste que aponta ao sítio errado passa sem provar nada
- **Sintoma:** um teste verde a dar por confirmada uma correcção que ele não tinha tocado.
- **Causa raiz (duas, na mesma tarefa):**
  1. O teste do anel de foco media um `.land-link` — um botão que **nunca teve `outline: none`** e por isso sempre teve o anel do próprio browser. Passava com a correcção e passaria sem ela. Os sujeitos certos eram os **seis controlos que estavam cegos**.
  2. A tentativa de ver o portão da gaveta ficar vermelho usou um `sed` com `^\.shell\.nav-open` — e a regra está **indentada** dentro de uma media query. O `sed` não mudou nada, o build foi o mesmo, e o «vermelho» foi um verde disfarçado.
- **Regra:** **o teste tem de apontar ao que a correcção mudou, e o vermelho tem de ser verificado, não presumido.** Depois de partir o invariante, confirma-se que o ficheiro mudou mesmo (`grep` ao alvo, ou `git diff`) antes de acreditar no resultado. Um `sed` que não casa é silencioso.
- **Regra irmã, sobre o instrumento:** quando a propriedade em causa é de pintura (`transform`, `outline`), o `getComputedStyle` de um painel que não compõe frames **mente** — mediu-se que nem um `!important` inline a altera. A leitura fiável é geométrica (`boundingBox`) ou por **comparação de pixéis**.
- **Ficheiros:** `web/e2e/layout-consola.mjs`.

### R52 — Um banco de ensaio que mede o invólucro em vez do componente
- **Sintoma:** um benchmark a dar **exactamente o mesmo número** com e sem a optimização, e prestes a ser publicado como «não faz diferença».
- **Causa raiz:** o contador de renders estava num componente `Contado` que envolvia o `<RemoteTile>` memoizado. O invólucro **não** é memoizado, por isso renderiza sempre — e era o invólucro que estava a ser contado. O `memo` estava a funcionar; o instrumento é que olhava para o sítio errado.
- **Regra:** **um contador de renders num invólucro mede o invólucro.** Para medir o efeito de uma barreira de memoização mede-se **tempo de commit da subárvore** — `<Profiler>` do React, `actualDuration` — ou instrumenta-se por dentro do componente. Com o instrumento certo: 2,352 ms/tique sem `memo` contra 0,038 com, a 12 pares.
- **A leitura que o número certo dá, e o errado escondia:** sem `memo` o custo **cresce com o número de pessoas na sala**; com `memo` é plano. O pior caso deixa de ser caso.
- **Ficheiros:** `web/e2e/bench/tiles.tsx`.

<!-- Numeração: os R46–R52 vieram do trabalho de UI/UX que fundiu primeiro.
     As entradas desta cadeia continuam em R53 para não haver dois R46. -->

### R53 — O código do segundo factor tem de ser CONSUMIDO, não só verificado
- **Sintoma:** um código TOTP apanhado por cima do ombro (ou num proxy, ou num screenshot) serve outra vez durante os trinta segundos seguintes. O segundo factor deixa de ser posse do dispositivo e passa a ser posse de seis dígitos.
- **Causa raiz:** verificar um TOTP é fácil; o que se esquece é que ele continua válido durante toda a janela. Sem estado, a verificação é repetível.
- **Regra:** `user_mfa.last_step` guarda o passo temporal aceite, e a actualização é CONDICIONAL (`WHERE last_step IS NULL OR last_step < $2`) — é a barreira que também resolve duas tentativas em paralelo, porque só uma delas afecta a linha. O mesmo vale para os códigos de recuperação (`used_at`, com `UPDATE ... WHERE used_at IS NULL`).
- **A consequência que não é óbvia e está testada:** o código usado para ACTIVAR o MFA não serve para o login seguinte, porque foi consumido. O primeiro login usa o código da janela a seguir. É correcto, é anti-replay entre operações diferentes, e sem estar escrito parece uma avaria.
- **Ficheiros:** `server/src/mfa.rs` (`consome_codigo`), migração 0035, `web/e2e/mfa.mjs`.

### R54 — Um portão que não compila o artefacto deixa passar o artefacto partido
- **Sintoma:** `make test` inteiro verde — 94 testes Rust, 137 vitest, tsc limpo — e a aplicação a devolver **500 em todos os pedidos** quando um browser real a abre. A página de login nunca renderizava.
- **Causa raiz:** a resolução de um conflito de merge deixou `src/styles.scss` com as chavetas desequilibradas. Nenhum dos portões toca em SCSS: o `tsc` só olha para tipos, o `vitest` importa módulos TS e nunca a folha de estilos, e o `cargo test` é do outro lado. O erro só aparece quando o Vite **compila** — isto é, no `build` ou no primeiro pedido do browser.
- **Regra:** o portão local tem de **produzir o artefacto**, não só analisá-lo. `make test` passou a correr `npm run build`, provado a falhar com uma regra SCSS aberta de propósito e a voltar a passar depois de fechada. O CI já tinha o build no `job` de frontend; era o ciclo local que mentia — e é o local que decide o que se commita.
- **O padrão por trás:** typecheck e testes unitários cobrem o que é *importado por testes*. Tudo o que só o empacotador vê — folhas de estilo, `assets`, imports dinâmicos de rotas sem teste — está fora do alcance deles por construção.
- **Ficheiros:** `Makefile` (alvo `test`), `web/src/styles.scss`.

### R55 — Um conflito de merge que abre dentro de um comentário parte as duas metades
- **Sintoma:** os marcadores `<<<<<<<`/`>>>>>>>` foram removidos, cada lado parecia íntegro na revisão, e o ficheiro ficou sintacticamente inválido.
- **Causa raiz:** os dois lados acrescentaram um bloco no fim do ficheiro começado pela MESMA linha decorativa (`/* ====…`). O git tratou essa linha como contexto partilhado e abriu o conflito **depois** dela — por isso nenhum dos lados contém o seu próprio abre-comentário. Pior: o `}` final também era contexto partilhado, e ficou a fechar só um dos blocos, deixando a última regra do outro lado aberta.
- **Regra:** quando um conflito abre a meio de um comentário ou de um bloco, **não se resolve escolhendo linhas** — reconstrói-se cada lado inteiro, com o seu próprio cabeçalho e o seu próprio fecho, e valida-se com o compilador da linguagem (aqui `npx sass`), não com a leitura.
- **Sinal de alarme:** conflito cujo primeiro `<<<<<<<` está imediatamente a seguir a uma linha que os dois lados também têm.
- **E uma armadilha no script que resolve o conflito, encontrada em paralelo noutro ramo:** um regex `n(.*?)\n=======` sobre o catálogo de regressões falha, porque há entradas que **citam** os marcadores a meio de uma frase. Marcadores a sério só contam **no início da linha** — o padrão tem de ser ancorado, ou o script resolve o sítio errado.
- **Ficheiros:** `web/src/styles.scss`.

### R56 — Ter corrido `make certs` mudava se os testes de browser corriam de todo
- **Sintoma:** `net::ERR_CERT_AUTHORITY_INVALID` em toda a bateria de browser, numa árvore onde nada de aplicacional tinha mudado.
- **Causa raiz:** o Vite arranca em HTTPS quando encontra os certificados locais e em HTTP quando não os encontra. Nenhum dos contextos do Playwright tolerava o certificado auto-assinado, por isso o resultado da bateria dependia de um efeito lateral de outro alvo do `Makefile`.
- **Regra:** todos os `newContext` do harness passam `ignoreHTTPSErrors: true`. O harness tem de correr contra as duas formas em que a aplicação local pode estar servida — a alternativa é uma bateria que passa ou falha conforme comandos anteriores, que é o mesmo que não ter bateria.
- **Ficheiros:** `web/e2e/ui-mfa.mjs`, `web/e2e/layout-consola.mjs`, `web/e2e/netem-matrix.mjs`.

### R57 — O intervalo «fixo e conhecido» do SFU está dentro do intervalo efémero do SO
- **O que está medido:** `SFU_UDP_MIN..SFU_UDP_MAX` = 50000–50200 cai **inteiro** dentro de `ip_local_port_range` (32768–60999, omissão do Linux). Qualquer processo do host — um browser aberto, os Chromium do Playwright — pode ficar com essas portas. Com as 201 ocupadas por um processo externo, o estabelecimento da ligação passou de **0,11 s para 1,12 s** (três corridas, valor idêntico). Não falha: o `webrtc-rs` recorre a outra porta. Fica dez vezes mais lento.
- **Consequência:** em K8s cada pod tem o seu namespace de rede e o intervalo não colide entre réplicas — em produção o risco é baixo. No **host de desenvolvimento e no runner de CI** o intervalo é partilhado com tudo o resto, e um custo de 10× no estabelecimento entra directamente no orçamento de qualquer teste com prazo.
- **Regra:** os testes usam um intervalo **abaixo de 32768** (20000+), que o SO nunca entrega como porta efémera, fatiado pelo PID. O intervalo do produto passou a ser configurável (`SFU_UDP_MIN`/`SFU_UDP_MAX`), o que também permite mover o produto para fora do intervalo efémero num nó onde isso importe.
- **O que NÃO ficou provado, e é importante dizê-lo:** esta **não** é a causa do timeout de 30 s em `sfu_e2e::media_flows_both_ways`. A hipótese foi testada directamente — intervalo do produto esgotado *e* o SFU apontado a ele — e o teste passou nas três corridas. A causa desse timeout continua **por estabelecer**; o `E2E_TIMEOUT_FACTOR` é mitigação, não diagnóstico.
- **Ficheiros:** `server/src/sfu.rs`, `server/src/config.rs`, `server/src/main.rs`, `server/src/sfu_e2e.rs`.
### R58 — Gravação que falha a compor desaparece em SILÊNCIO
- **Sintoma:** o anfitrião carrega em «gravar», vê o indicador aceso a reunião inteira, e no fim não há nada na biblioteca. Nem gravação, nem aviso, nem sinal de que houve tentativa. Do lado dele é indistinguível de nunca ter gravado — e o artefacto não se pode refazer depois de a reunião acabar.
- **Causa raiz:** o `finalize` registava o erro no log do SERVIDOR e apagava o directório temporário. A biblioteca lê a tabela `recordings`, onde nunca chegou a entrar linha nenhuma.
- **Regra:** uma tentativa de gravação que falha entra na biblioteca com `status = 'failed'` e uma **causa em linguagem de utilizador** (migração 0036). A entrada existe para ser vista: sem miniatura clicável, sem ▶, sem descarregar, sem partilhar — oferecer «reproduzir» sobre algo que não existe é prometer duas vezes à mesma pessoa.
- **A causa é TRADUZIDA, nunca o erro cru:** o stderr do ffmpeg traz caminhos do servidor e nomes de ficheiros temporários. O detalhe fica no log, onde serve a quem opera; no ecrã entra só o que se pode mostrar a alguém. Há teste que verifica que não vazam caminhos nem códigos internos.
- **O contexto põe-se na ORIGEM, não por adivinhação de texto depois:** um ffmpeg em falta chegava como `No such file or directory (os error 2)`, indistinguível de um ficheiro de track em falta. O `spawn` passa a marcar o caso, e a causa passa a dizer «o servidor não tem o ffmpeg instalado» — que nomeia um problema de OPERAÇÃO e poupa a investigação a quem recebe a queixa.
- **Descarregar uma falhada** devolve `400` com a explicação, em vez de descer até ao `File::open` e voltar um `500` opaco.
- **Ficheiros:** migração 0036, `server/src/recorder.rs` (`registar_falha`, `causa_legivel`), `server/src/recordings.rs`, `web/src/pages/Recordings.tsx`, teste `web/e2e/gravacao-falhada.mjs`.

### R59 — Uma funcionalidade correcta fica incompleta quando a UI ganha vistas por baixo dela
- **Sintoma:** o R58 (gravação falhada sem acções) estava implementado e testado — e depois de fundir a base de UI/UX a MESMA gravação falhada voltava a oferecer ▶, descarregar e partilhar. Nenhum conflito de merge assinalou nada.
- **Causa raiz:** a lógica de `status === 'failed'` foi escrita contra a ÚNICA vista que existia (cartões). A base acrescentou entretanto a vista de **tabela** e um visualizador de **biblioteca** — código novo, que o git juntou sem conflito porque não tocava nas mesmas linhas. O visualizador ainda pedia o ficheiro inexistente e mostrava «falha ao carregar o vídeo», um erro genérico que **esconde a causa já registada**.
- **Regra:** uma regra de apresentação que depende de estado (`failed`, `expired`, `revoked`) pertence a **todas** as vistas do mesmo recurso, e a lista dessas vistas cresce. Quando se acrescenta uma vista, verificam-se os estados; quando se acrescenta um estado, verificam-se as vistas. As três — cartões, tabela, biblioteca — mais o visualizador estão agora cobertas.
- **O que isto diz sobre merges:** «sem conflitos» é uma afirmação sobre LINHAS, não sobre comportamento. Duas mudanças correctas em ficheiros diferentes produzem um produto errado, e nenhum portão de texto apanha isso — só abrir o ecrã.
- **Ficheiros:** `web/src/pages/Recordings.tsx` (vista de tabela e `ViewerBody`), `web/src/styles.scss`.
### R60 — `readinessProbe` no `/health`: o K8s mandava entradas novas para um pod a fechar
- **Sintoma:** um deploy derruba TODAS as reuniões dos pods substituídos, e quem tenta entrar durante a janela cai numa sala que morre em segundos.
- **Causa raiz, em duas metades.** (1) O `readinessProbe` apontava para o `/health`, que devolve `ok` enquanto o processo viver — incluindo durante o encerramento; o pod ficava nos endpoints do Service e continuava a receber entradas novas. (2) O SIGTERM só fazia o axum parar de ACEITAR ligações, e as WebSockets em curso não fecham sozinhas: o processo ficava a aguardá-las até o SIGKILL do `terminationGracePeriod`, e aí caía tudo de uma vez. Com a afinidade por sala (ADR-0001) a concentrar salas no mesmo pod, é muita gente ao mesmo tempo.
- **Regra:** **liveness e readiness respondem a perguntas diferentes e não podem partilhar endpoint.** `/health` = «o processo está vivo?» (sim, mesmo a drenar — um pod a drenar deve ser DEIXADO TERMINAR, não reiniciado). `/ready` = «pode receber tráfego NOVO?» (não, assim que o SIGTERM chega).
- **A ORDEM do drain importa:** primeiro pôr a readiness em 503 e ESPERAR que o balanceador retire o pod; só depois avisar os clientes. Avisar primeiro fá-los reconectar para o mesmo pod, que ainda está nos endpoints.
- **O jitter no cliente não é enfeite:** o pod avisa a sala inteira no mesmo instante, e sem jitter vinte pessoas reconectam no mesmo milissegundo — trocava-se um encerramento ordenado por uma avalanche no pod novo.
- **Os prazos têm de encaixar:** `DRAIN_READINESS_SECS` (12) + `DRAIN_GRACE_SECS` (40) < `terminationGracePeriodSeconds` (60). Com os 45 s anteriores, o SIGKILL chegava a meio do drain e ele não servia para nada.
- **Porque é que a migração funciona:** o SFU é in-memory por pod, mas o hash por sala manda a sala INTEIRA para o mesmo pod novo assim que este sai dos endpoints. Reconectar em conjunto é migrar; reconectar em ordens diferentes seria split-brain — e é por isso que o servidor manda o atraso em vez de deixar cada cliente escolher.
- **Ficheiros:** `server/src/main.rs` (`drenar`, `readiness`), `server/src/signaling.rs` (`ServerMsg::Draining`, `broadcast_draining`, `tem_sala`), `deploy/k8s/02-server.yaml`, teste `web/e2e/drain.mjs`.

### R61 — Auditoria que se podia editar, apagar, e que desaparecia com a conta
- **Sintoma:** nenhum — é esse o problema. Três buracos que só aparecem quando a trilha é precisa, e aí já não há como a reconstituir.
- **Causa raiz, em três partes:**
  1. **Qualquer pessoa com escrita na base de dados podia fazer `UPDATE`/`DELETE`** numa linha para apagar o que fez. Um registo de auditoria que se pode editar não é um registo de auditoria — e o adversário que interessa aqui é precisamente alguém com privilégios.
  2. **`ON DELETE CASCADE` para `users`:** apagar um utilizador APAGAVA a trilha dele. É o inverso do que uma auditoria faz — a história tem de sobreviver às entidades que descreve. E o `list` fazia `JOIN users`, por isso mesmo sem o cascade os eventos de uma conta apagada já desapareciam da vista do administrador. São os eventos que mais interessam logo a seguir a uma saída.
  3. **Os eventos de LOGIN eram escritos com `org_id = NULL`**, caindo numa cadeia sem organização. Um administrador podia verificar a sua trilha e receber «intacta» sem que os logins lá estivessem sequer.
- **Regra:** **cadeia de hash por organização.** Cada linha inclui o hash da anterior; editar ou apagar parte a cadeia. Os gatilhos que recusam `UPDATE`/`DELETE` são a primeira barreira; a cadeia é a que sobrevive a quem tenha poder de esquema para os remover. O nome do actor é gravado NO MOMENTO — e é o nome que ele tinha então, que é o que uma auditoria quer, não o actual.
- **O encadeamento vive num gatilho, com um lock por cadeia.** Dois `INSERT` concorrentes a ler o mesmo `prev_hash` produziriam um RAMO, e a verificação acusaria quebra sem ninguém ter mexido em nada.
- **O material do hash está numa função SQL usada pelo escritor E pela verificação.** Duas cópias divergem, e uma verificação que discorda do escritor acusa falsas quebras.
- **A escrita continua a não falhar a operação principal** (recusar um login porque a auditoria está em baixo é pior que o problema), mas passou a ser **ERRO** e a contar em `delonix_audit_write_failures_total`: uma trilha partida é uma falha de conformidade em curso, não um aviso perdido no log.
- **Provado atacando a tabela por SQL**, não pela API — um teste que só usa a API prova que a API não deixa, não que os dados estão protegidos. Com os gatilhos DESACTIVADOS: alterar uma linha é detectado («o registo nº 1 foi ALTERADO depois de escrito») e apagar uma do meio também («falta o registo nº 2: a numeração salta para 3»).
- **Ficheiros:** migração 0037, `server/src/audit.rs`, teste `web/e2e/auditoria.mjs`.

### R62 — Portão de CI calibrado para o portátil de quem o escreveu
- **Sintoma:** o CI falha ao acaso num teste que passa sempre em local. Aqui foi o `sfu_e2e::media_flows_both_ways`, com «timeout à espera de: B recebe áudio+vídeo de A».
- **Causa raiz:** os testes de media montam `RTCPeerConnection`s a sério — ICE, DTLS e o primeiro RTP. Numa máquina de desenvolvimento resolvem-se em ~0,1 s; num runner de CI partilhado com 2 vCPU chegam a estourar os 30 s. Não é uma avaria do produto: é o mesmo trabalho numa máquina muito mais lenta.
- **Regra:** prazos de teste ponta-a-ponta são **generosos e ajustáveis** (`E2E_TIMEOUT_FACTOR`, ×4 no CI), nunca calibrados para o ambiente de quem os escreveu. **Um portão que falha ao acaso perde a credibilidade toda** — à terceira vez, quem o vê vermelho assume flake e segue, e a partir daí ele não protege nada.
- **E a mensagem de timeout tem de dizer o que falta para o distinguir:** o prazo, o número de tentativas e o tempo decorrido. Sem isso, um timeout não separa «o produto está partido» de «a máquina é lenta» — e foi exactamente essa dúvida que custou uma ida ao CI.
- **O que o prazo NÃO resolveu, medido a 2026-08-25:** com `E2E_TIMEOUT_FACTOR=4`, o `media_flows_both_ways` falhou no CI com **1188 tentativas em 120 s** — o laço correu bem, a media é que nunca chegou. Não é lentidão. Repetido o MESMO commit, passou em 2m35s. É flake genuíno, e a causa continua **por estabelecer** — ver também o R57, onde a hipótese das portas UDP foi testada e refutada.
- **O que se fez em vez de subir o prazo outra vez:** a mensagem de timeout passa a trazer um RETRATO dos dois lados — estado de sinalização, de ICE, da ligação, tracks recebidas e contagem de RTP. As três avarias possíveis davam a mesma mensagem e mandavam investigar em sítios diferentes: ICE que nunca liga é rede, ICE ligado sem tracks é subscrição, tracks sem RTP é fan-out. Verificado a sair legível com um timeout forçado. **Subir um prazo esconde; um retrato no momento da falha é o que torna a próxima ocorrência utilizável** — e num flake que não reproduz em local, é a única coisa que adianta.
- **Ficheiros:** `server/src/sfu_e2e.rs` (`prazo`, `eventually`, `eventually_com_diagnostico`, `TestClient::retrato`), `.github/workflows/ci.yml`.

### R63 — Duas branches acrescentam ao fim do mesmo ficheiro e o git funde em silêncio
- **Sintoma:** o catálogo de regressões ficou com **dois R49** e **dois R50**, a falar de coisas diferentes, e o `Ver R49` do `HARNESS.md` passou a apontar para ambos. Zero conflitos de merge.
- **Causa raiz:** cada ramo acrescentou a sua entrada no fim do ficheiro, em posições diferentes do texto. O git funde por linhas: nunca houve colisão. A colisão é de **significado** — um espaço de nomes partilhado (o número) sem ninguém a guardá-lo, exactamente como acontecia com os números das migrações.
- **Regra:** o `check-repo-hygiene.sh` passou a recusar (a) números de regressão repetidos e (b) uma referência `R<n>` em qualquer ficheiro do repo sem entrada correspondente no catálogo — que é o que apanha uma renumeração feita a meio e esquecida algures. Provado a falhar nas duas faltas antes de se confiar nele verde.
- **A generalização:** qualquer ficheiro append-only com um identificador sequencial partilhado entre ramos precisa de um portão. Já se sabia das migrações; o catálogo tinha o mesmo problema e ninguém o tinha visto porque um número duplicado não parte nada — só engana quem lê.
- **Ficheiros:** `scripts/check-repo-hygiene.sh`, `docs/reference/regressions.md`.
- **Ficheiros:** `server/src/sfu_e2e.rs` (`prazo`, `eventually`), `.github/workflows/ci.yml`.

### R64 — Medir o tempo a partir de quando o CÓDIGO está pronto
- **Sintoma:** um «tempo de entrada» bonito que não corresponde ao que o utilizador espera.
- **Causa raiz possível, e evitada de propósito:** começar a linha do tempo dentro da `SfuCall`. Quando essa classe existe, já passaram o pedido do room token, a resolução de ICE servers e a abertura do WebSocket — mede-se o código, não a experiência. A linha do tempo começa em `Room.tsx`, no instante em que o utilizador quis entrar.
- **Regra:** o «tempo até entrar» conta da INTENÇÃO até haver MEDIA. Um WebSocket aberto com o ecrã preto não é ter entrado numa reunião, e por isso o marco final é `connected` da PC, não o `open` do socket.
- **Cada marco é registado UMA vez.** O segundo `primeiro_audio` não é o primeiro: sem essa regra, uma renegociação a meio da chamada reescrevia o instante e o «tempo até ouvir» passava a medir a última renegociação — um número que parece bom e não quer dizer nada.
- **Marcos em falta dão `null`, nunca zero.** Zero é uma medição («foi instantâneo») e enviesa as médias para baixo; `null` é a ausência dela.
- **Não se reporta uma sessão que nunca ligou.** Enviesaria a média de «tempo até entrar» com sessões que não entraram — essas contam-se na taxa de sucesso, não aqui.
- **A cauda tem contador próprio** (`delonix_join_slow_total`, > 5 s): uma média de 1,2 s esconde perfeitamente 5% de pessoas à espera doze segundos, e é essa gente que abre o ticket.
- **Primeira medição real** (2026-08-25, dois Chromium contra o SFU): join 364 ms, `ws_ms` 345, ICE gathering 14 ms. **95% do tempo de entrada é token + WebSocket** — e é precisamente para isolar isso que o `ws_ms` existe em separado.
- **Ficheiros:** `web/src/callTimings.ts`, `web/src/webrtc.ts`, `web/src/pages/Room.tsx`, migração 0038, `server/src/rooms.rs` (`post_timings`), teste `web/e2e/tempos.mjs`.

### R65 — Quatro corridas boas depois de uma mudança não são prova de nada
- **O que aconteceu:** o `web/e2e/tempos.mjs` deu `join_ms` = 211, 239, 1060, 3887, 4436 e `null` em seis execuções do MESMO commit. Notei que a máquina tem **oito** interfaces IPv4 (Wi-Fi, três pontes de libvirt, quatro do Docker), formulei a hipótese de o ICE se perder a verificar pares inúteis, restringi o Chromium à interface por omissão, corri **quatro vezes** — 231, 233, 258, 312 — e escrevi «causa estabelecida, não é hipótese». Estava errado.
- **O que a medição controlada mostrou:** dez corridas alternadas, na mesma sessão. **Sem** a restrição: 291, 250, 334, 244, 204 — todas boas. **Com** a restrição: `null`, 236, 278, 267, 358. A restrição não melhora nada, e a única falha da série foi com ela.
- **A causa real da variância original:** a máquina estava carregada — dois worktrees, três servidores de desenvolvimento, vários Chromium de outras fusões a correr ao mesmo tempo. Com a máquina parada, o `join_ms` é ~200–350 ms sem flag nenhuma.
- **O erro de método, que é o que interessa guardar:** medi o DEPOIS e comparei-o com um ANTES recolhido noutras condições. Quatro sucessos seguidos parecem prova e não são: sem correr a linha de base **na mesma sessão e alternada**, a mudança leva o crédito do que mudou no ambiente. A regra passa a ser: **uma correcção de instabilidade só se aceita com A/B alternado na mesma sessão**, nunca com «corri N vezes depois e passou».
- **E o que quase custou:** a alteração chegou a ser empurrada e **partiu o CI** — lá deu `join_ms=null` e dois reinícios de ICE. Uma correcção verificada só de um lado teria trocado ruído local por uma falha permanente no portão.
- **O que fica:** nada no harness. A alteração foi revertida por inteiro. Fica o número honesto — o `join_ms` é ~200–350 ms numa máquina parada — e o que ele NÃO autoriza: publicar um SLO. Isso exige uma série num ambiente controlado, com percentil e número de amostras ao lado.
- **E a confirmação, que veio do CI e não de mim:** o `tempos.mjs` passava no runner (536 ms) até o merge trazer o bloco de interface que acrescentei à `gravacao-falhada.mjs` — um segundo Chromium com login e três vistas, a correr **imediatamente antes** da medição. A partir daí, `join_ms=null` no CI, com o flag e sem ele. O runner tem 2 vCPU: é a mesma carga que localmente levava o `join_ms` a nunca ligar. A medição passou a correr ANTES dos testes pesados, com a máquina quieta.
- **A regra que fica:** **o único teste da bateria que MEDE não pode correr atrás dos que carregam a máquina.** Verificar um invariante é robusto a carga; medir um tempo não é. Misturá-los na mesma sequência faz o número depender da ordem — e um número que depende da ordem não é um número.
- **E a causa que o número acabou por entregar:** `ice_gathering_ms=39969`. Quarenta segundos a recolher candidatos, contra 377 ms antes do merge — com o servidor byte-a-byte igual e o código de media do cliente também. O que cresceu foi a APLICAÇÃO: o Vite em modo dev transforma os módulos ao primeiro pedido, duas páginas a carregar de raiz saturam um runner de 2 vCPU durante dezenas de segundos, e o agente de ICE do browser fica esfomeado. O teste passou a fazer uma passagem de aquecimento antes de medir.
- **A lição por trás dessa:** o número que estava a falhar (`join_ms`) não dizia nada; o que estava ao lado dele (`ice_gathering_ms`) dizia tudo. **Uma medição que só reporta o agregado não se diagnostica** — foi preciso ter as parcelas para saber que o problema não era do produto.
- **Ficheiros:** `web/e2e/tempos.mjs`, `web/e2e/pg.mjs`, `.github/workflows/ci.yml`.
### R66 — O dev server sem COOP/COEP faz a segmentação falhar SÓ em desenvolvimento
- **Sintoma:** os fundos e efeitos da sala, e o recorte sem fundo do Estúdio, sem nada a acontecer e sem erro na interface. Em produção funcionam.
- **Causa raiz:** o WASM multi-thread do ONNX Runtime (RVM) e do MediaPipe precisa de `SharedArrayBuffer`, que só existe numa página **cross-origin isolated** — ou seja, com `Cross-Origin-Opener-Policy: same-origin` e `Cross-Origin-Embedder-Policy: require-corp`. O `deploy/k8s/nginx.conf` põe os dois; o dev server do Vite **não punha**. Medido: `globalThis.crossOriginIsolated` dava `false` no `vite` e `true` depois da correcção.
- **Porque não deu erro:** o pipeline é fail-soft por desenho — o RVM cai no MediaPipe, e o MediaPipe cai em nada. Um caminho que degrada em silêncio é bom para o utilizador e péssimo para quem procura a causa.
- **Regra:** **o dev server serve os mesmos cabeçalhos de isolamento que o nginx.** Uma capacidade que depende de um cabeçalho tem de ter esse cabeçalho nos DOIS sítios, senão testa-se sempre o caminho degradado.
- **Nota sobre os assets:** o `public/ort-rvm/*` e o `public/models/*.tflite` são obtidos no build da imagem e **não estão no git**. Num worktree novo o RVM devolve `index.html` com estado 200 (fallback da SPA) e falha com `expected magic word 00 61 73 6d, found 3c 21 64 6f` — que é `<!do`. Não é corrupção: é HTML onde se esperava WASM.
- **Ficheiros:** `web/vite.config.ts`.

### R67 — Três testes seguidos a olhar para o sítio errado
- **Sintoma:** portões verdes que não guardavam nada, e um a dizer «não arrancou» sobre uma funcionalidade a funcionar.
- **As três, na mesma tarefa:**
  1. `expect(c).toContain('silencio.connect(...)')` continuava a passar com a linha **comentada** — o `toContain` encontra a string dentro do comentário. Corrigido com um `readCodigo()` que remove comentários antes de comparar.
  2. A detecção da segmentação fazia `querySelector('.studio-grupo small')` — **singular**. Ao acrescentar uma dica de arrasto, o primeiro `<small>` passou a ser outro, e o teste declarou «não arrancou» com o segmentador a correr em GPU.
  3. A asserção da silhueta usava `brilho > 5` contra um fundo de brilho **18**: passava com o ecrã vazio. A câmara do Chromium de teste é um padrão de cores, não uma pessoa — o segmentador corre, não encontra ninguém, e não há silhueta para medir. Substituída por uma que verifica que o *pipeline* arrancou, mais uma nota escrita a dizer que o resto precisa de uma câmara real.
- **Regra:** **antes de acreditar num verde, pergunta o que teria de estar partido para ele ficar vermelho.** Se a resposta for «nada», o teste é decoração. E quando o limiar é numérico, mede-se primeiro o valor de repouso — um limiar abaixo do fundo é um teste que passa sozinho.
- **Ficheiros:** `web/src/studio.invariantes.test.ts`, `web/e2e/estudio.mjs`.

### R68 — `toContain` com o nome de uma função aceita a função errada
- **Sintoma:** dois portões do editor a ficarem VERDES com o invariante deliberadamente partido.
- **Causa raiz (duas, na mesma passagem):**
  1. `expect(e).toContain('decodeAudioData')` continua a passar quando a chamada é trocada por `decodeAudioDataX` — a string está lá dentro. Um nome de função verifica-se com **fronteira de palavra** (`/\bdecodeAudioData\(/`), nunca com `toContain`.
  2. `expect(c).toContain('for (const g of this.todos)')` passava com o `pausar()` partido, porque o `retomar()` tem o mesmo padrão. Quando o invariante é «TODOS os sítios fazem X», **conta-se** — `expect(ocorrências).toBeGreaterThanOrEqual(3)` — em vez de confirmar que existe um.
- **Regra:** um portão baseado em texto tem de responder «o que teria de estar partido para isto ficar vermelho?». Se a resposta for «uma coisa que ninguém escreveria por engano», o portão não guarda o que diz guardar.
- **É a quarta vez nesta série** (ver R67): comentário aceite como código, `querySelector` singular, limiar abaixo do fundo, e agora substring de nome de função. O padrão comum é o mesmo — a asserção é mais frouxa do que a frase que a descreve.
- **Ficheiros:** `web/src/studio.invariantes.test.ts`.

### R69 — Cinco portões escritos com asserções que valiam nos dois estados
- **Sintoma:** testes verdes que continuavam verdes com o invariante partido. Cinco vezes na mesma série de tarefas, sempre pelo mesmo motivo.
- **O catálogo dos cinco:**
  1. `toContain('silencio.connect(...)')` passava com a linha **comentada** (R67).
  2. `querySelector` **singular** apanhava o primeiro `<small>` da secção e declarou «não arrancou» com o segmentador a correr (R67).
  3. `brilho > 5` contra um fundo de brilho **18** — passava com o ecrã vazio (R67).
  4. `pausas.length === 1` num teste de limiar relativo: com um limiar FIXO a gravação baixa fica **toda** classificada como pausa, o que também dá comprimento 1.
  5. `pausas[0].inicio >= 2` para verificar uma margem de 0,12 s: verdade com margem (2,12) e sem ela (2,00).
- **A causa comum:** a asserção foi escrita a partir do que o código *devia* fazer, e nunca comparada com o que produz **quando está partido**.
- **Regra:** **mede o valor nos dois estados antes de escolher o limiar.** Correr o código sabotado e ler o número leva um minuto; foi o que separou `>= 2` (inútil) de `> 2.05` (que apanha). Um limiar escolhido de cabeça tem tanta hipótese de cair do lado errado como do certo.
- **Regra irmã:** contar elementos não é verificar onde eles estão. Um detector que devolve «uma pausa» pode ter devolvido a gravação inteira.
- **Ficheiros:** `web/src/studio/analise.test.ts`, `web/src/studio.invariantes.test.ts`, `web/e2e/estudio.mjs`.

### R70 — Lista de precache escolhida por NOME em vez de derivada do grafo
- **Sintoma:** com a rede cortada a app abria, mas o Estúdio — a única coisa que se prometia offline — deixava a raiz do React **vazia**, sem erro visível na interface.
- **Causa raiz:** a lista de precache do service worker foi montada com padrões de nome de ficheiro (`assets/index-*`, `assets/Studio-*`). O `Studio` importa `media.ts`, que o Rollup separou num chunk `media-*.js` que nenhum padrão apanhava. Sem rede, o `import()` da rota rejeitava e a árvore não montava.
- **Regra:** **uma lista de precache vem do grafo de dependências, não de padrões de nome.** O Rollup conhece as importações de cada chunk (`bundle[f].imports`); o fecho transitivo a partir do entry e das rotas que se querem offline é exacto. Adivinhar pelo nome falha exactamente no ficheiro em que ninguém pensou — e falha em silêncio, porque um chunk em falta não tem sintoma que aponte para a cache.
- **Segunda regra, sobre o que NÃO entra:** precachear tudo seria pior. Os modelos de IA e o `whisperWorker` passam dos 30 MB e obrigariam toda a gente a descarregá-los na instalação. A linha é: o esqueleto e as rotas que se PROMETEM offline entram; o resto entra em cache no primeiro uso.
- **Ficheiros:** `web/vite.config.ts`, `web/public/sw.js`.

### R71 — Contar «×» na saída dá VERDE a um crash
- **Sintoma:** dois portões dados como «não ficam vermelhos» quando na verdade ficavam.
- **Causa raiz:** a verificação contava linhas com `×` na saída do vitest. Uma das sabotagens partia o `vite.config.ts`, o vitest nem chegava a correr, a saída não tinha `×` nenhum — e a contagem de zero foi lida como «o teste passou apesar do invariante partido».
- **Regra:** **para saber se um portão fica vermelho, usa-se o CÓDIGO DE SAÍDA, não uma contagem de padrões na saída.** Um crash é vermelho. Contar sintomas de falha na saída dá falsos verdes precisamente nos casos mais graves, em que nem se chega a correr.
- **Erro irmão, na mesma sessão:** a asserção de ordem («o arquivo é escrito antes do upload») procurava `uploadRecording(` no ficheiro INTEIRO. Havia outra chamada numa função acima, encontrada primeiro, e a ordem invertida passava. Uma asserção de ordem tem de ser feita dentro do âmbito onde a ordem importa.
- **Ficheiros:** `web/src/studio/offline.invariantes.test.ts`.

### R72 — Um teste que existe e nunca corre não é portão nenhum
- **Sintoma:** cinco ficheiros em `web/e2e/` — escritos, comprometidos, com dezenas de asserções contra Chromium real — **não apareciam em lado nenhum do workflow**. Uma regressão em qualquer deles passava os seis jobs verdes.
- **Medido** (2026-08-26, contra `origin/feat/console-ui-template`): existiam 12 `.mjs`, o CI corria 7. Fora ficavam `estudio.mjs`, `layout-consola.mjs`, `offline.mjs`, `netem-matrix.mjs` e `pg.mjs`.
- **Causa raiz:** cada um nasceu com a sua funcionalidade, num ramo, e o passo do CI ficou por acrescentar. Nada avisa — o ficheiro existe, o `git status` está limpo, e um CI verde não distingue «passou» de «não correu».
- **Regra:** **um teste ponta-a-ponta só conta depois de se ver o nome dele na saída de uma corrida do CI.** Até lá é documentação executável: útil, mas não protege nada.
- **Como se apanha, em dois comandos:** `ls web/e2e/*.mjs` contra `grep -o 'node web/e2e/[a-z-]*\.mjs' .github/workflows/ci.yml`. A diferença é a lista dos que não guardam nada.
- **Nem todos devem correr, mas a ausência tem de ser DECLARADA:** o `pg.mjs` é um ajudante, não um teste; o `netem-matrix.mjs` precisa de `CAP_NET_ADMIN` que o runner não concede — e passou a dizê-lo no cabeçalho. Um teste fora do CI sem razão escrita é indistinguível de um esquecido.
- **Dois defeitos reais que estes testes escondiam, e que só apareceram ao ligá-los:** o `layout-consola.mjs` injectava uma sessão FALSA no `localStorage`, o que funciona contra um mock e falha contra o servidor a sério (401 → renovação → logout → o teste morre no ecrã de login). Passou a registar uma conta e a entrar pela interface, como os outros já faziam. E o R73 abaixo.
- **E os dois comandos passaram a ser um portão**, porque uma verificação que depende de alguém se lembrar de a correr tem a mesma esperança de vida que o passo de CI que ficou por acrescentar. O `check-repo-hygiene.sh` recusa qualquer `web/e2e/*.mjs` que não seja invocado pelo workflow nem declare no cabeçalho `NÃO CORRE NO CI: <razão>` ou `MÓDULO DE APOIO`. A declaração fica no ficheiro, não numa lista à parte, porque é lá que quem o abre a lê. Provado a recusar um teste novo por ligar e uma razão apagada, antes de se confiar nele verde.
- **Ficheiros:** `.github/workflows/ci.yml`, `web/e2e/sessao.mjs`, `web/e2e/netem-matrix.mjs`, `scripts/check-repo-hygiene.sh`, `web/e2e/pg.mjs`.

### R73 — O `Vary` da resposta faz o `caches.match` não casar, e o precache fica inútil
- **Sintoma:** com a rede cortada a app abria e ficava com a raiz do React **vazia**. O service worker estava registado, a cache tinha as 10 entradas certas, e o bundle de arranque estava lá dentro — e mesmo assim o pedido dava `net::ERR_FAILED`.
- **Causa raiz:** a Cache API **honra o cabeçalho `Vary`** da resposta guardada. Se ela trouxer `Vary: Origin` (o `vite preview`) ou `Vary: Accept-Encoding` (qualquer nginx com gzip), o `caches.match(request)` compara os cabeçalhos do pedido com os da resposta e **não casa** — devolve `undefined` sobre uma cache que tem o ficheiro.
- **A ironia que a torna importante:** o `Vary: Accept-Encoding` do nginx foi acrescentado por nós, com o gzip (R-gzip). Ou seja, a optimização de carregamento partia o modo offline, e as duas coisas nunca tinham sido testadas juntas.
- **Regra:** **no service worker, `caches.match(pedido, { ignoreVary: true })`.** Estes recursos são identificados pelo URL e mais nada; não há variantes a distinguir, e honrar o `Vary` só cria um modo de falha silencioso. Vale para as QUATRO chamadas, não só para a do precache.
- **Como foi apanhado:** por correr o teste offline contra um servidor DIFERENTE do que se usou a escrevê-lo. Contra o servidor de simulação (sem `Vary`) passava; contra o `vite preview` falhou. Um teste que só corre contra um servidor prova o servidor tanto como o código.
- **Ficheiros:** `web/public/sw.js`.

### R74 — `configure()` do WebCodecs não falha sem hardware: cai para software em silêncio
- **Sintoma:** o corte de dois segundos de uma aula não acabava em **90 segundos** no runner do CI. Localmente, na mesma versão do código, acabava em poucos. Nenhum erro, nenhum aviso — só uma barra de progresso que não anda.
- **Causa raiz:** o corte usa WebCodecs precisamente porque o `VideoEncoder` usa o encoder de HARDWARE do dispositivo. Mas o `configure()` **não falha** quando não há hardware: cai para software sem dizer nada, e um VP9 de 1080p a 6 Mbps em software leva minutos onde levava segundos. O runner do CI não tem GPU — e nem é um caso de laboratório: é o que acontece a quem edita num portátil sem aceleração ou numa máquina virtual.
- **Regra:** **pergunta-se ao browser antes de assumir.** `VideoEncoder.isConfigSupported()` com `hardwareAcceleration: 'prefer-hardware'`, e uma escada de perfis que desce até um que o software aguenta. Pior qualidade é melhor do que uma espera que parece uma avaria. Medido em Chromium sem GPU: `vp9 prefer-hardware` → `false`, `vp8 prefer-hardware` → `false`, `vp8 sem preferência` → `true`.
- **O defeito que a correcção introduziu, e que o mesmo teste apanhou:** ao pôr o encoder a descer para VP8, o multiplexador continuou fixo em `V_VP9`. Um ficheiro rotulado com o codec errado abre **sem duração e sem imagem**, e outra vez sem erro nenhum. O codec do multiplexador tem de ser DERIVADO do perfil escolhido, e há um portão que verifica a ordem (perfil antes do multiplexador) e a derivação.
- **Como foi apanhado:** por o teste passar a correr no CI (R72). Localmente passava; a diferença de hardware entre a máquina de quem escreve e o runner é exactamente o que um portão existe para expor.
- **Ficheiros:** `web/src/studio/editor.ts`, `web/src/studio.invariantes.test.ts`.

### R75 — Código de fundação que ninguém chama é o mesmo problema que um teste que nunca corre
- **Sintoma:** a catraca do clippy subiu de 32 para 41 avisos ao acrescentar um módulo novo. Todos eram `never constructed` / `never used`.
- **A tentação:** silenciar com `#![allow(dead_code)]` e uma nota a dizer «vai ser ligado no PR seguinte». A regra do repo é explícita ao contrário — «código novo entra LIMPO».
- **Porque é que a regra tem razão:** um módulo que nada chama não protege nada, não corre em lado nenhum, e não se sabe se funciona — é exactamente o R72 noutra forma. E o «PR seguinte» é onde este tipo de código costuma ficar a apodrecer.
- **Regra:** **uma camada de fundação entrega-se ligada.** Nem que seja pela superfície mínima que a torne alcançável e testável de ponta a ponta. Se ainda não se sabe ligar, então ainda não se sabe o suficiente para a escrever.
- **O que ligá-la destapou:** duas coisas que um módulo solto nunca teria mostrado — a rota nova falhava o portão de autorização (R44) por não usar um extractor `AuthUser`, e obrigou a escrever a razão pela qual autentica por token de query (um WebSocket não leva os nossos cabeçalhos); e o registo por sala teve de nascer, porque dois anfitriões a carregar em «ir para o ar» dariam dois ffmpeg contra a mesma chave.
- **Ficheiros:** `server/src/broadcast.rs`, `server/src/main.rs`, `scripts/rotas-publicas.txt`.

### R76 — `-f webm` a ler H.264 funciona por sorte, não por contrato
- **Sintoma:** nenhum, e é esse o problema. O caminho do directo declarava `-f webm` ao ffmpeg e funcionava.
- **O que está medido:** o `MediaRecorder` do Chromium aceita `video/webm;codecs=h264,opus` e produz um ficheiro que o `ffprobe` descreve como `format_name=matroska,webm` com `codec_name=h264`. O WebM **oficialmente só admite VP8/VP9/AV1** — o que sai é Matroska com H.264 lá dentro, com a extensão errada.
- **Porque é que passava:** o desmultiplexador de WebM do ffmpeg **É** o de Matroska. Declarar `-f webm` e dar-lhe H.264 acerta por o código ser o mesmo, não por a declaração estar certa.
- **Regra:** **declara-se o que a coisa É, não o que a extensão sugere.** Uma build mais estrita, ou uma versão futura que separe os dois desmultiplexadores, recusaria — e o sintoma seria um directo que deixa de arrancar depois de uma actualização de imagem, sem nada no nosso código ter mudado.
- **Como foi apanhado:** por gerar um ficheiro REAL com o `MediaRecorder` num Chromium e correr o comando REAL do servidor sobre ele, num contentor de ffmpeg. Medido: entra `h264`+`opus`, sai `h264`+`aac` — o vídeo copiado e o áudio transcodificado, que é a decisão inteira do ADR-0003 provada de ponta a ponta.
- **Ficheiros:** `server/src/broadcast.rs`.

### R77 — Quatro testes marcados `#[ignore]` porque o produto mudou, e ninguém voltou
- **Sintoma:** todas as corridas diziam `4 ignored` e ninguém contava. Um deles era **`rooms_are_isolated`** — um invariante de isolamento sem cobertura ao nível da unidade desde que a semântica do hub mudou.
- **Causa raiz, medida ao corrê-los:** os quatro falhavam pela MESMA razão, e nenhuma era um defeito. O `join` passou a difundir o anúncio de entrada para **toda a sala, incluindo quem entra** (`broadcast_all_local` percorre `room.peers`, e quem entra já lá está). A primeira mensagem que cada participante recebe é o anúncio de si próprio, e os testes esperavam a de outra pessoa. O `rooms_are_isolated` chegava a acusar uma fuga entre salas que **não existe**: o que ele apanhava era o anúncio do próprio `b`.
- **A regra que a nota `#[ignore]` violava:** «reescrever mais tarde» não é um estado. Marcar um teste como ignorado por mudança de semântica esconde a pergunta que interessa — *o comportamento novo está certo?* Aqui estava, mas foram precisos oito minutos para o saber, e esteve anos por responder.
- **O que ficou no lugar:** os quatro voltam à bateria (`0 ignored`), e a semântica que os partiu passou a ter teste PRÓPRIO — `joiner_also_receives_its_own_announcement`. Sem ele, a próxima pessoa a ler estes testes conclui que o hub está partido; com ele, quem mudar o comportamento é obrigado a mudar aqui também.
- **Uma armadilha na correcção:** o `join` entrega ao anfitrião o anúncio E, a seguir, a fila de espera acumulada. Um `recv()` único apanha o anúncio; um `drain()` seguido de `recv()` consome os dois e **fica pendurado**. Recolhe-se o que há e afirma-se sobre o conjunto.
- **E o que se confirmou no browser:** não há retrato fantasma. O cliente acrescenta o próprio `peer-joined` à lista, mas quem está sozinho vê **um** retrato, o local, marcado «eu». O `web/e2e/reuniao.mjs` cobre isso e o resto do caminho.
- **Ficheiros:** `server/src/signaling.rs`, `web/e2e/reuniao.mjs`.

### R78 — `ExecutableFileBusy` num teste: a corrida não é pelo ficheiro, é entre `fork` e `exec`
- **Sintoma:** os testes de emissão falhavam com `Os { code: 26, ExecutableFileBusy }` em cerca de **3 corridas em 20**, e o teste afectado mudava de cada vez — a assinatura de uma corrida, não de um defeito lógico.
- **A hipótese errada, e porque parecia certa:** o ajudante escrevia `dlx-sorvedouro-<pid>.sh` em `/tmp`, partilhado pelos quatro testes, guardado por `if !caminho.exists()`. Parece óbvio: dois testes, um ficheiro. Dar um ficheiro **único a cada chamada**, escrito e fechado antes do `chmod` e publicado por `rename` atómico — **não resolveu**. Continuou a falhar.
- **Causa real:** o Linux recusa executar um ficheiro que QUALQUER processo tenha aberto para escrita. Os testes correm em paralelo no mesmo processo: enquanto um thread tem o ficheiro aberto, outro faz `fork` para lançar o seu próprio filho, e esse filho **herda o descritor de escrita** na janela entre o `fork` e o `exec`. O ficheiro ser único não ajuda — o descritor herdado é do ficheiro do outro.
- **Regra:** um teste não escreve um executável enquanto há lançamentos a decorrer. O ajudante passou a criá-lo **uma vez por processo**, atrás de um `OnceLock`: quem chega depois espera pela escrita terminada em vez de correr contra ela, e a partir daí ninguém volta a abrir o ficheiro para escrever. **25 corridas, zero falhas.**
- **O atalho que não serve, e está aqui para não se repetir:** usar o `cat` do sistema em vez do script parece eliminar o problema pela raiz. Não serve — sem a redirecção que o script faz, o processo sai com estado diferente de zero e o teste do ciclo de vida deixa de valer. Falhou 25 em 25.
### R79 — Parar a gravação emudecia um directo a decorrer
- **Sintoma:** o directo continuava no ar, mas sem som, a partir do instante em que se parasse a gravação local. Nenhum erro, nem no browser nem no servidor.
- **Causa raiz:** a gravação e o directo consomem o MESMO fluxo composto (canvas + áudio misturado). O `terminarGravacao` fechava o `AudioContext` — porque, quando só existia a gravação, fechá-lo era exactamente o que devia fazer.
- **Regra:** **um recurso partilhado só se desmonta quando o ÚLTIMO consumidor o larga.** O `montarFluxo` conta quem entra e o `largarFluxo` conta quem sai; a desmontagem vive num sítio só.
- **A regra irmã, que evitou o problema seguinte:** o fluxo é montado num sítio só. Duplicar a montagem para o directo teria dado duas versões que divergiriam à primeira correcção feita numa delas — e o áudio é onde isso doeria, porque a fonte silenciosa que evita gravações vazias é uma armadilha fácil de esquecer no segundo sítio.
- **Como foi apanhado:** a ler o `terminarGravacao` antes de ligar o directo ao mesmo fluxo, não em execução. Um acoplamento entre duas funcionalidades que nunca correram juntas não tem sintoma até correrem.
- **Ficheiros:** `web/src/studio/compositor.ts`, `web/src/studio/directo.test.ts`.

### R80 — O proxy do vite não encaminha WebSockets debaixo de `/api`
- **Sintoma:** o directo aparecia recusado na interface, e o log do servidor **não tinha nada** — nem a recusa, nem o arranque. O pedido nunca chegou ao handler.
- **Causa raiz:** o `server.proxy` do `vite.config.ts` declarava `ws: true` no `/ws` e no `/rtc`, mas não no `/api`. A rota do directo (`/api/rooms/{code}/broadcast`) é um WebSocket **debaixo do prefixo `/api`**, e sem essa flag o vite responde ao upgrade com HTTP em vez de o encaminhar.
- **Porque é que engana:** não há erro em lado nenhum. No browser o socket fecha com o código 1006 e sem razão; no servidor não há sequer registo de tentativa. Procura-se a causa nos dois lados do túnel e ela está no meio.
- **Regra:** **um WebSocket novo verifica-se no PROXY, não só nas duas pontas.** A pergunta a fazer é «que prefixo o serve, e esse prefixo encaminha upgrades?».
- **Como foi apanhado:** por o log do servidor estar vazio. Um handler que devia registar recusa OU arranque e não regista nenhum dos dois não foi chamado — e isso aponta para fora do processo.
- **Ficheiros:** `web/vite.config.ts`.

### R81 — Uma recusa devolvida ANTES do upgrade de WebSocket não chega ao browser
- **Sintoma:** a recusa de E2EE — a mais importante do ADR-0003, e a que explica ao utilizador porque é que a sala dele não pode emitir — chegava à interface como «não foi possível ligar ao servidor de emissão». A frase inteira, escrita com cuidado no servidor, era deitada fora pelo caminho.
- **Causa raiz:** o handler devolvia `ApiError::BadRequest(razão)` antes de aceitar o upgrade. A API de WebSocket do browser **não expõe o estado nem o corpo** de um handshake que falhou: o `onclose` traz `reason` vazio e o código 1006, e o `onerror` não traz nada. Um `Response` HTTP bem construído é invisível a quem o pede por WebSocket.
- **Regra:** **num WebSocket, a recusa entrega-se DEPOIS do upgrade.** Aceita-se, manda-se a razão numa trama de TEXTO, e fecha-se. O `reason` do frame de `Close` não serve para isto: está limitado a 123 bytes e é truncado sem aviso — e estas mensagens são frases inteiras de propósito, porque explicam o porquê E o que fazer.
- **Detalhe que custou uma compilação:** o `WebSocket` do axum não tem `close()`; fecha-se enviando `Message::Close(None)`. Largar o socket sem a enviar deixa o browser outra vez com um 1006 sem razão.
- **Como foi apanhado:** por correr o teste ponta a ponta contra um servidor SEM ffmpeg — o mesmo ambiente do CI. Com ffmpeg presente, o caminho de recusa nunca corria.
- **Ficheiros:** `server/src/broadcast.rs`, `web/src/studio/directo.ts`.

### R82 — Uma substituição de texto que não casa falha em SILÊNCIO
- **Sintoma:** um `map_err` que devia distinguir «ffmpeg em falta» de qualquer outra falha continuava a devolver o erro genérico, depois de o script que o alterava ter dito que correu bem.
- **Causa raiz:** o script fazia três substituições e só assertava a existência de UMA delas. O `cargo fmt` tinha reformatado o bloco entretanto — quebrando os argumentos em linhas — e o texto procurado deixou de existir. O `str.replace` não encontra, não substitui, e **não se queixa**.
- **Regra:** **cada substituição tem o seu `assert`.** Um script que altera N sítios e verifica um só reporta sucesso com N-1 por fazer. E depois de formatar, qualquer alvo escrito antes da formatação é suspeito.
- **Como foi apanhado:** por o log do servidor mostrar a mensagem antiga depois de o teste do módulo passar. Os testes de unidade cobriam o `Display` da recusa nova — que existia — mas não o handler, que nunca a usou.
- **Ficheiros:** `server/src/broadcast.rs`.

### R83 — Um corte de seis segundos no servidor deixava a reunião inteira presa numa mensagem técnica
- **Medido:** com uma chamada estabelecida, `SIGKILL` ao servidor e ressurreição seis segundos depois. O participante ficava com **«Erro: Internal Server Error»** no ecrã da reunião — e assim permanecia, com o servidor já de volta. Reproduzido em todas as corridas.
- **Causa raiz:** o `catch` que envolve o arranque da sala em `Room.tsx` fazia `setStatus(\`Erro: ${err.message}\`)` e parava ali. Sem nova tentativa, uma falha transitória no arranque é indistinguível de uma permanente — e o texto que sobrava era a mensagem do protocolo HTTP, que não é uma frase que se mostre a alguém numa reunião.
- **Regra:** montar a sala passa a ter **nova tentativa com recuo** (seis, reutilizando o `backoffDelay` que já existia), com um estado legível a dizer a tentativa em curso; só depois de as esgotar aparece uma frase terminal que diz o que fazer. O contador vive FORA da função, senão cada tentativa reinicia-o e o recuo nunca cresce.
- **O que NÃO ficou resolvido, e é a parte que interessa:** a recuperação da **sinalização** depois de uma morte abrupta é **inconstante**. Quatro corridas contra o mesmo commit deram uma falha, um êxito e duas sem terminar no prazo. Quando falha, o sintoma é pior do que o erro que se corrigiu: a sala parece saudável e o participante está **surdo** — um convidado novo pede entrada e o pedido nunca chega. A causa não está estabelecida.
- **Porque é que o teste não entra no CI:** um portão que falha ao acaso perde a credibilidade toda (R62). Fica declarado em cabeçalho, com o comando para o correr à mão, até a causa ser conhecida.
- **Duas asserções que este teste teve e que passavam em VAZIO:** «a página recarregou» — a recuperação por nova tentativa não recarrega, e exigi-lo dava falha com o produto já correcto; e «a sala está aberta e tem retratos» — o DOM não muda quando o socket cai, por isso a condição já era verdadeira com o servidor morto (mediu-se «0,0 s de recuperação»). A prova que não engana é **funcional**: entra outra pessoa, e o anfitrião tem de a ver, admitir e passar a vê-la.
- **Ficheiros:** `web/src/pages/Room.tsx`, `web/e2e/morte-abrupta.mjs`.

### R84 — a chave sai do índice, fica no histórico, e o portão passa a dizer que está tudo bem
- **Sintoma:** nenhum. É esse o problema. O `check-repo-hygiene.sh` ficou **verde** no minuto seguinte a 3b80b8a, e a exposição não tinha mudado nada.
- **Causa:** os pontos 1 e 2 do portão liam o **índice** (`git ls-files`, `git grep`), que é o que sai num clone *hoje*. O `git rm --cached` tira do índice e **não** tira do histórico: as duas chaves continuam alcançáveis em quatro commits, e o repositório é público. Um `git log` de qualquer pessoa chega lá.
- **A leitura que faltava, e o que ela destapou:** a auditoria de 2026-08-25 registou **uma** chave. Ao ler o histórico em vez do índice apareceu a **segunda** — `meet.delonix.local.key`, de 98f5b28, três dias mais velha do que a wildcard. Uma leitura anterior do histórico não é o histórico.
- **Regra:** o ponto 6 lê `git rev-list --objects --all`. Todo o caminho de chave encontrado tem de estar em `scripts/leaked-keys-accepted.txt` **com a razão e a data escritas** — o mesmo padrão do `rustsec-accepted.txt`: o portão não impede a decisão, impede a decisão **silenciosa**. Ambas as chaves estão lá, registadas como QUEIMADAS, com o raio de dano medido e a decisão de não reescrever o histórico escrita por extenso.
- **O limite, dito e não subentendido:** a busca é por **caminho**, não por conteúdo. Uma chave colada dentro de um ficheiro qualquer do histórico não é apanhada — ler todos os blobs não cabe num portão de CI. Preferimos escrevê-lo a dar uma garantia que não temos.
- **Ficheiros:** `scripts/check-repo-hygiene.sh`, `scripts/leaked-keys-accepted.txt`.

### R85 — a página de preços vendia o que a árvore não tinha
- **Sintoma:** «SSO SAML e SCIM» num plano **pago**, nos três idiomas. Zero linhas de código para qualquer um dos dois: as **únicas** ocorrências das duas palavras em toda a árvore eram as próprias strings de marketing. Mais quatro entradas do roteiro com `done: true` para coisas que não existiam — SVC, estimativa de banda no servidor, códigos de segurança E2EE verificáveis, e um SDK público. E, no plano de topo, um **SLA de 99,99 %** numa plataforma sem SLO, sem error budget, sem teste de carga e sem teste de caos.
- **Causa:** a copy foi escrita contra o **roteiro** e não contra a árvore, e ninguém a voltou a ler. Não é desonestidade — é o que acontece quando nada relê.
- **Regra:** `scripts/check-capability-claims.sh`. Se um termo guardado (SAML, SCIM, WebAuthn, passkey, SVC, SDK, webinar, MinIO/S3) aparecer numa linha que diz que a capacidade **foi entregue** — `done: true` no roteiro, ou a lista `features:` de um plano — tem de existir código fora dos ficheiros de locale. Prometer está bem; **dar por entregue** é o que isto recusa.
- **O SLA precisou de regra própria, e a primeira versão estava errada:** guardar a palavra «SLA» recusava também «SLA negociado em contrato», que é um termo comercial e não diz nada sobre o software. O que exige prova é o **número**: uma percentagem é uma promessa que a plataforma tem de conseguir cumprir e demonstrar. O portão passou a guardar o número, não a palavra — e foi ele que encontrou o 99,99 %, que a revisão humana tinha deixado passar duas vezes.
- **Limite honesto:** isto prova que uma capacidade não é vendida com **zero** código por trás. Não prova que o código está completo, alcançável ou autorizado — um stub com o nome certo satisfazia-o. Apanha a falha que aconteceu de facto.
- **Ficheiros:** `scripts/check-capability-claims.sh`, `web/src/locales/{pt,en,fr}.ts`, `web/src/pages/Analytics.tsx`.

### R86 — No telemóvel não se conseguia desligar a chamada
- **Medido** (2026-09-03, arnês com o CSS compilado a sério): a 375 px a `.controls-bar` transbordava **318 px**, a 320 px transbordava **373 px**. O botão de **desligar** ficava inteiramente fora do ecrã, e com ele o grupo da direita — pessoas, chat, notas, ferramentas. Sair de uma reunião no telemóvel só era possível fechando o separador, que é o gesto que a máquina de recuperação lê como quebra de rede e tenta reverter.
- **Causa raiz:** a barra é `grid: 1fr auto 1fr` e a única regra abaixo dos 900 px escondia o código da sala. Nada envolvia, nada deslizava, nada colapsava. Nove controlos não cabem em 375 px e ninguém tinha dito ao CSS o que fazer nesse caso.
- **Causa a montante, e é a que interessa:** o tamanho do controlo estava fixado com `width: 38px !important` na camada da consola. Qualquer camada posterior teria de escalar para `!important` também — e a seguir a próxima. O `!important` não era o remédio da cascata, era o que a tornava intratável. Passou a **variável** (`--ctrl-size`), e a regra dos 44 px ao toque não precisa de um único `!important`.
- **Regra:** há três acções sem as quais não se opera uma reunião — microfone, câmara e desligar. Em ecrã estreito ficam **fixas**: não deslizam, não encolhem, não entram em menu. Tudo o resto partilha tiras que deslizam, com máscara de desvanecimento — nada se perde e um botão cortado a meio deixa de parecer avaria.
- **O atalho que NÃO serve:** pôr a barra inteira em `overflow-x`. O botão de sair continua escondido, só que atrás de um gesto que ninguém adivinha. **Esconder por transbordo é esconder.**
- **Duas linhas e não um menu «mais»:** um menu poria pessoas e chat atrás de mais um toque, e são os dois painéis mais usados. Duas linhas cabem — medido a 320 px, a largura mais estreita que ainda se vende.
- **Sobre a medição, que quase saiu errada:** a emulação de viewport do navegador **não** mexia no `innerWidth` da página — dava 693 px com o ecrã a 375. Se tivesse acreditado nela, teria concluído que estava tudo bem. O portão fixa a viewport com Playwright, que é a única que se verificou fiável.
- **Portão:** `web/e2e/bar-responsivo.mjs`, no CI. Compila a folha a sério e mede a 320/375/414/768/1440. Visto a falhar (3 larguras) com a camada responsiva desligada e a recuperar com ela.
- **Ficheiros:** `web/src/styles.scss`, `web/e2e/bar-responsivo.{mjs,html}`, `.github/workflows/ci.yml`.

### R87 — O cartão de convite tapava o vídeo a reunião inteira
- **Sintoma:** «A tua reunião está pronta» aparecia em **todas** as capturas da sala, inclusive com painéis abertos, a tapar o canto inferior esquerdo do vídeo. No telemóvel ocupava metade do ecrã.
- **Causa:** só fechava por clique explícito no X ou em «Adicionar participantes». Não fechava ao fim de tempo nenhum, não fechava quando entrava a segunda pessoa — que é precisamente o instante em que deixa de fazer sentido —, e não fechava ao abrir um painel.
- **Regra:** um cartão que interrompe fecha-se sozinho no momento em que perde a razão de ser. Três saídas: alguém entrou, abriu-se um painel, ou passaram 20 s. O `sessionStorage` continua a impedir que volte na mesma sessão.
- **Ficheiros:** `web/src/pages/Room.tsx`.

### R88 — A mesma barra falava duas linguagens visuais
- **Sintoma:** o ecrã da sala usava emoji como iconografia — ⏳ no temporizador, 📊 nas sondagens, ❓ no Q&A, 🛡 no código de segurança, 📌 no fixar, 🔊 no testar som — **a par** do conjunto SVG do `icons.tsx`, no mesmo sítio e por vezes na mesma barra. Nenhum dos três concorrentes usa um único emoji como ícone de interface; é o sinal isolado que mais faz um produto parecer projecto pessoal.
- **A regra já existia e faltava-lhe cobertura:** o cabeçalho do `icons.tsx` diz desde sempre «o emoji fica onde é CONTEÚDO — as reações da sala —, nunca onde é controlo». O portão que a impunha (`lote2`, 3.2.5) cobria **5 ficheiros** da consola e nenhum da sala.
- **A contagem inicial estava errada, e isso importa:** a primeira leitura deu «171 emoji». Ao separar o que é conteúdo legítimo — `REACTION_EMOJIS`, `CHAT_EMOJIS`, nomes de teclas em `<kbd>` — e os que só aparecem em comentários a documentar conversões antigas, sobravam **154**, e destes só **~58 em JSX**, que são os que podem receber um SVG. Os restantes vivem em strings de notificação e em rótulos tipográficos (`↖↗↙↘` para cantos) onde um SVG não cabe. Contar antes de converter evitou trocar reações por ícones.
- **O buraco no portão, que quase passou:** a asserção era `>\s*([^<>{}\n]{1,4})\s*<` — no máximo **quatro** caracteres entre tags. O caso mais comum é o glifo SEGUIDO do rótulo: `>📊 Sondagens<` tem mais de quatro e **escapava**. O portão dava verde com o defeito à frente. Só apareceu ao tentar vê-lo falhar de propósito, que é a única forma de saber se um portão guarda alguma coisa (R71). Alargado para 120, apanhou logo mais **oito** que ninguém tinha visto.
- **Regra:** o portão 3.2.5 passou de 5 para **11 ficheiros**, incluindo `Room.tsx`, `RemoteTile.tsx` e `Lobby.tsx`. Onde um SVG não cabe — `<option>`, atributos `title` — o glifo fica, e no `<option>` passou a entidade HTML.
- **O que fica de fora, declarado:** `Landing.tsx`, `Analytics.tsx` e `Studio.tsx` ainda têm glifos em **arrays de dados** (listas de funcionalidades, rótulos de canto). Convertê-los mexe na forma dos dados e não na marcação — é outro trabalho, e está escrito aqui para não passar por esquecimento.
- **Ficheiros:** `web/src/icons.tsx` (+11 ícones), `web/src/pages/Room.tsx`, `web/src/room/RemoteTile.tsx`, `web/src/pages/Lobby.tsx`, `web/src/components/MfaPanel.tsx`, `web/src/App.tsx`, `web/src/lote2.invariantes.test.ts`.

### R89 — A folha de estilos usava a cor de marca da Google
- **Sintoma:** `#ea4335` com o comentário «vermelho Meet exato», no botão de desligar, no microfone silenciado e no ponto de gravação. Sete ocorrências entre a cor e o seu tom de *hover*.
- **Porque é defeito e não detalhe:** o §37 do mandato diz para não copiar identidade alheia, e a `--danger` da casa (`#e05252`) já existia três camadas abaixo — era ela que efectivamente vencia na cascata em quase todos os sítios. A cor da Google estava lá a fazer de conta, e a **vencer mesmo** no ponto de gravação.
- **Regra:** portão 3.2.6 — a folha não pode conter a paleta de marca do Meet, do Teams nem do Zoom. Guarda-se a paleta **alheia**, não «cores literais» em geral: a folha tem centenas delas e proibi-las todas seria um portão que ninguém põe verde.
- **Ficheiros:** `web/src/styles.scss`, `web/src/lote2.invariantes.test.ts`.

### R90 — Um portão que falha num teste DIFERENTE de cada vez não guarda nada
- **Sintoma:** três corridas do job `isolamento` sobre o mesmo código, **três falhas diferentes**, nenhuma a repetir: o corte do Estúdio («não encurtou»), a sala («dois retratos» → 1) e os tempos de chamada (`join_ms: null`). O reteste da segunda passou. O histórico mostra o mesmo job vermelho a 2026-08-26, antes deste trabalho.
- **Porque é grave e não é ruído:** pela regra da casa (R62), um portão que falha ao acaso perde a credibilidade toda. Um que falha num sítio diferente de cada vez é pior: treina toda a gente a carregar em «repetir» e a partir daí o vermelho deixa de ser informação. Todos os outros portões dependem deste job para significar alguma coisa.
- **Não eram três problemas — eram três causas, e só uma era tempo.**

  **(1) `reuniao.mjs` — esperar por uma condição e afirmar outra.** Esperava-se por «1 retrato sem “eu”» e afirmava-se «2 retratos no total», com **duas leituras separadas** do DOM. Entre elas o DOM muda. O CI apanhou-a a dar `juntaram = true` com **um único retrato** na lista — uma contradição impossível de depurar a partir do relatório. Pior: quando o retrato local perdia o texto por um instante (um `<svg>` não tem `textContent` — R88), a espera casava com o retrato ERRADO e devolvia cedo.

  **Regra:** espera-se pela condição que se vai afirmar, e a fotografia tira-se **dentro** da espera, para ser a mesma que a satisfez. Nunca duas idas ao DOM.

  **(2) A identidade do retrato passou a ser um atributo.** Distinguir local de remoto por o texto conter «eu» é uma asserção sobre **decoração**, e quebra-se — em silêncio, dando verde — sempre que a decoração muda. Os retratos ganharam `data-peer="local|remoto"` e `data-peer-id`. Provado: com o atributo sabotado no produto, o teste passa a recusar (`remotos:0` com `total:2`); a versão anterior dava **verde** ao mesmo produto partido.

  **(3) `tempos.mjs` — a espera esgotava em silêncio.** Ao fim de 45 s o ciclo saía sem dizer nada e a asserção seguinte reportava `join_ms medido: null`, uma frase que faz parecer que o produto mediu mal quando o teste é que leu cedo demais. «Ainda não chegou» e «veio errado» são diagnósticos diferentes e não podem partilhar a mesma mensagem. A espera passou a declarar-se, e o tempo que esperou aparece no relatório.

  **(4) O Estúdio NÃO era tempo — e a primeira explicação estava errada.** Assumi lentidão do runner e pus o prazo a escalar com `E2E_TIMEOUT_FACTOR` (que o job do backend já usava e este não). **Falhou na mesma com o prazo a 360 s**, o que descarta lentidão. O prazo maior fica — é correcto por si — mas não era a causa.

  **A causa ficou estabelecida** (2026-09-05), e o diagnóstico novo é que a deu: `duração 5.86s — não desceu abaixo de 2,9s`, ou seja o corte corre e **não corta**, com a duração original intacta. Não é prazo nem WebM sem cabeçalho: é o que o `escolherPerfil` do `editor.ts` já documentava — **sem GPU o corte cai para software e não termina em tempo útil**, nem com o prazo a 360 s. A asserção do corte passou a ser condicional a haver encoder acelerado, PERGUNTADO ao browser em vez de assumido, e onde não há diz-se que não se verificou e porquê. Não se pôs o `estudio.mjs` fora do CI: ele tem outras trinta asserções que protegem o Estúdio, e perdê-las para acomodar uma seria trocar cobertura por silêncio.

  O texto que segue foi escrito antes de a causa se saber, e fica como registo do que se assumiu: «não encurtou» cobre três coisas que mandam investigar sítios diferentes — duração **ilegível** (`Infinity`/`NaN`, ou seja um WebM sem cabeçalho de duração, defeito do que se PRODUZ e não do corte), duração **igual** à original (o corte não correu), ou duração diferente mas acima do alvo (cortou o troço errado). O diagnóstico passou a distingui-las. **Fica em aberto, com o instrumento para o fechar** — que é mais honesto do que uma correcção que não corrige.

  A lição que se repete: **assumir a causa e corrigir sem prova custa uma volta inteira.** Foi o mesmo erro que a dica do timeout cometia — apontar um remédio sem ter medido o problema.
  **(5) E a dica do timeout apontava o remédio ERRADO.** O próprio PR que corrige isto apanhou uma quinta instância, desta vez no job do BACKEND: `sfu_e2e::media_flows_both_ways` estourou o prazo e a mensagem disse «se for lentidão do ambiente, sobe `E2E_TIMEOUT_FACTOR`» — mas o estado impresso ao lado dizia `ice=Failed` nos **dois** pares. `Failed` é terminal: mais prazo não liga um ICE que já desistiu, e a dica mandou investigar tempo quando a causa está na rede do ambiente (UDP bloqueado, sem candidatos de host). **Uma mensagem que aponta o remédio errado custa mais do que uma que não aponta nenhum.** A dica passou a depender do estado observado, com teste que a vê mudar nos dois casos.
- **O que isto não resolve, dito por inteiro:** cinco causas fechadas não provam que o CI ficou estável. Provam que estas cinco estão fechadas. A estabilidade mede-se em corridas repetidas ao longo do tempo, e essa medição ainda não existe.
- **Uma armadilha em que caí a escrever o próprio teste desta correcção:** o `#[tokio::test]` foi colado DENTRO de outra função de teste. Compila — é uma função aninhada — e o `cargo test` diz `ok` com **0 testes a correr**. É o R72 outra vez, agora em Rust: só se apanha ao ver o nome do teste na saída, nunca ao ver a suite verde.
- **Ficheiros:** `web/e2e/reuniao.mjs`, `web/e2e/tempos.mjs`, `web/e2e/estudio.mjs`, `web/src/room/RemoteTile.tsx`, `web/src/pages/Room.tsx`, `server/src/sfu_e2e.rs`, `.github/workflows/ci.yml`.

### R91 — Um F5 a meio da reunião devolvia o convidado à sala de espera
- **Sintoma medido:** um participante admitido que recarregue a página cai **outra vez** na sala de espera e fica à espera de ser admitido de novo. Reproduzido com dois browsers contra servidor real: `caiu na SALA DE ESPERA: À espera que o anfitrião te deixe entrar…`.
- **Causa raiz:** o `peer_id` nasce por SOCKET (`Uuid::new_v4()` no `handle_socket`). Quando o socket cai, o `leave` corre de imediato e leva com ele tudo o que era estado de execução: o papel, as autorizações de partilha, o lugar de apresentador, e a própria admissão. O servidor não tinha como saber que quem voltou é quem estava.
- **O que já existia e cobria PARTE do problema:** o co-anfitrião é persistido em `room_admitters` (migração 0017) precisamente «para reconexões», e o dono da sala volta sempre a entrar directo. Por isso o defeito **não se vê** no anfitrião — e foi isso que fez a primeira versão do teste passar com a correcção desligada.
- **Correcção:** o lugar passa a ficar **reservado** durante uma janela (`RECONNECT_GRACE_SECS`, 45 s por omissão). Quem entra recebe um segredo opaco de 32 bytes, guardado em `sessionStorage`; ao voltar, envia-o em `?reconnect=` e herda o `peer_id`, o papel e a admissão. Os outros veem `peer-reconnecting` em vez de `peer-left`: o retrato fica no sítio, esbatido, em vez de desaparecer e reaparecer.
- **O que NÃO se herda, e é deliberado:** nada de media. O socket é novo, a `RTCPeerConnection` é nova, e a negociação faz-se do zero — tentar reaproveitar estado de media reabriria o glare que o R13 fechou.
- **Quatro recusas que o segredo tem de fazer, todas testadas:** segredo vazio, segredo errado, segredo de OUTRA sala, e segredo de alguém que está **vivo** (um segredo copiado não expulsa o dono do lugar). Sem a última, quem copiasse o segredo entrava por cima de quem estava lá.
- **Porque não se reutiliza o token de sala:** esse é uma capability sobre a SALA — quem o tiver entra como quem quiser. Este é sobre o LUGAR, e é o que autoriza herdar o papel de anfitrião. Confundir os dois dá promoção a anfitrião por conhecer um link.
- **Sair não é cair:** o cliente apaga o segredo no `leaveRoom`. Sem isso, quem sai de propósito continuaria a ocupar lugar na sala durante a janela inteira.
- **A armadilha, e repetiu-se TRÊS vezes:** a asserção passava com a correcção desligada. Primeiro por testar o **anfitrião**, que volta a entrar de qualquer maneira. Depois por ler o DOM antes de o servidor responder, apanhando a barra montada sem o aviso de espera ainda renderizado. Só à terceira — convidado de outra organização, com tempo para assentar — é que o teste ficou a **discriminar**. Uma correcção sem uma asserção que a distinga do seu contrário não está provada, por mais código que tenha.
- **Portão:** `web/e2e/reentrada.mjs`, no CI. Visto a falhar com a reclamação desligada no cliente.
- **Ficheiros:** `server/src/signaling.rs`, `server/src/config.rs`, `server/src/main.rs`, `server/src/metrics.rs`, `server/src/apikeys.rs`, `web/src/signaling.ts`, `web/src/pages/Room.tsx`, `web/src/room/RemoteTile.tsx`, `web/src/styles.scss`, `web/e2e/reentrada.mjs`.

### R92 — Faltavam cinco controlos de anfitrião, e a contagem que os motivou estava errada
- **A contagem errada, primeiro:** o relatório de lacunas dizia «2 controlos de anfitrião contra ~15». **Está errado e vale a pena a correcção**, porque uma lacuna exagerada leva a construir o que já existe. Medido contra a árvore: existem **13** — `ForceMute`, `Kick`, `RoomLock`, `HostShareOnly`, `ShareGrant`, `Admit`/`Deny`, `TranscriptionToggle`, `ServerRecord`, `Presenting`, `RemoteControl`, breakouts, sondagens/Q&A/temporizador, e a co-admissão persistida em `room_admitters`.
- **O que faltava mesmo,** medido com `grep` em servidor e cliente (zero ocorrências de cada): silenciar TODOS, desligar a câmara de alguém, fechar o chat, transferir o papel de anfitrião, e impedir que quem foi silenciado se volte a ligar.
- **A decisão que NÃO se tomou, e porquê:** o plano previa um modelo de capacidades a substituir o `is_host`. Não se fez. O padrão existente funciona, está testado, e generalizá-lo agora seria YAGNI (§52 do mandato) — o modelo justifica-se quando chegarem os papéis de webinar (painelista, assistente), não antes. A Regra 0 da arquitectura diz o mesmo: não se refactoriza código que funciona sem justificação escrita.
- **Regra 1 — o estado vive na SALA, não na mensagem.** «Silenciar todos sem voltar a ligar» e «chat fechado» ficam no `Room` e entram no `RoomSettings` que quem chega a meio recebe. Sem isso, alguém que entrasse depois falava numa sala que o anfitrião julgava fechada.
- **Regra 2 — a recusa é do SERVIDOR.** O chat fechado é imposto no handler do `Chat`, não escondendo a caixa de texto: esconder um botão não impede ninguém de enviar a mensagem pelo socket. É a invariante 8 do AGENTS.md, e o teste envia a mensagem à socket com o chat fechado para o provar.
- **Regra 3 — o anfitrião continua a falar com o chat fechado.** Um moderador sem voz não modera.
- **Regra 4 — a troca de anfitrião é atómica**, sob o mesmo lock. Um instante com dois anfitriões, ou com nenhum, e «nenhum» numa sala com sala de espera activa tranca lá toda a gente.
- **Os controlos novos NÃO são descartáveis** numa fila cheia: se um «silenciar todos» pudesse cair, alguém ficava com o microfone aberto numa sala que o anfitrião julgava fechada. A classificação é uma lista de permissões, por isso já estavam certos — e agora está afirmado por teste.
- **A armadilha, e é a mesma família do R69:** o teste «um participante não se promove a anfitrião» pedia `b → b` e **passava com a guarda removida**. A razão está no próprio código: transferir para si próprio põe `is_host` a `true` e logo a `false` na mesma passagem — o resultado é igual com e sem guarda. Só com um TERCEIRO participante (`b` promove `c`) é que a asserção passou a distinguir. Foi apanhado a desligar as três guardas uma a uma: duas ficaram vermelhas, esta ficou verde.
- **Ficheiros:** `server/src/signaling.rs`, `web/src/signaling.ts`, `web/src/pages/Room.tsx`, `web/src/styles.scss`.

### R93 — Uma bateria verde deixou de ser prova, e havia como medir isso
- **O padrão que motivou isto:** cinco correcções seguidas foram entregues com testes que davam VERDE com o produto partido (R69, R71, R90, R91, R92). Quatro dessas foram apanhadas por acaso, ao tentar ver o portão falhar. Não é distração pontual — é o modo de falha dominante deste trabalho, e a partir de certo ponto «218 testes verdes» deixa de ser uma afirmação com conteúdo.
- **A medição:** `scripts/mutantes.mjs` aplica mutações pequenas e semanticamente reais ao CÓDIGO (`>=`→`>`, `&&`→`||`, `===`→`!==`, uma de cada vez) e corre a bateria. Uma mutação **morta** significa que algum teste deu por ela; uma que **sobrevive** é uma linha que ninguém defende.
- **Primeira medição, contra os seis módulos de decisão pura:** 57 mutações, **46 mortas, 11 sobreviveram** — 81 %. Depois de fechar as lacunas: **52 mortas, 5 equivalentes, 0 por explicar** — 91 %, e os 5 restantes com razão escrita.
- **O que as 6 lacunas reais eram, e nenhuma era trivial:**
  1. **`callQuality`: um `NaN` do `getStats()` entrava nas contas.** A guarda `typeof v === 'number' && Number.isFinite(v)` não tinha teste — e `typeof NaN === 'number'` é `true`. Um NaN numa métrica não rebenta: propaga-se para a pontuação, a média e o gráfico, e **mostra-se**.
  2. **A escolha do par de candidatos não tinha teste nenhum** — três mutações sobreviviam na mesma linha. É a fonte do `turnRelay`, que a consola mostra ao utilizador e o `/metrics` publica.
  3. **Um orçamento de banda ZERO era tratado como «sem banda»** em vez de «desconhecido», degradando tudo ao mínimo. Um `0` vindo de uma API que ainda não mediu é a primeira coisa que acontece.
  4. **Uma duração de 0 ms virava `null`** — e um `null` num painel lê-se como «não medido», não como zero.
  5. **Uma pausa com EXACTAMENTE a duração mínima era descartada** no Estúdio — e a duração mínima é precisamente o número que o utilizador escreve no cursor.
  6. **Um silêncio até ao fim da gravação** não tinha teste, e é o caso mais comum de todos.
- **O ledger de equivalentes** (`scripts/mutantes-equivalentes.txt`) segue o padrão do `rotas-publicas.txt`: um sobrevivente sem razão escrita é indistinguível de um esquecimento, e sem ele as mesmas cinco linhas voltam a ser investigadas daqui a três meses. Um dos cinco revelou **lógica morta** — a sentinela `j === n` em `analise.ts` só pode marcar um início que a linha seguinte descarta.
- **A armadilha, dentro da própria auditoria:** os meus primeiros três testes do par de candidatos passavam E os mutantes sobreviviam. A razão: o arnês muta UMA ocorrência de cada vez, e eu tinha trocado as duas à mão ao verificar. Os `===` sobreviviam porque os pares dos testes não definiam `selected`/`nominated` — com `undefined`, tanto `=== true` como `!== true` deixam a cadeia `||` verdadeira. **Só um par que se declara explicitamente não-escolhido distingue as duas versões.**
- **O que isto NÃO cobre, dito por inteiro:** seis módulos de decisão pura, não a sala, não o SFU, não o Rust. São os mais baratos de mutar (sem rede, sem DOM, sem relógio) e por isso os primeiros — não os únicos que interessam.
- **Ficheiros:** `scripts/mutantes.mjs`, `scripts/mutantes-equivalentes.txt`, `web/src/mutantes.lacunas.test.ts`, `web/src/studio/analise.test.ts`.

### R94 — Oito autorizações de anfitrião podiam ser removidas sem um teste dar por isso
- **Como se soube:** o `scripts/mutantes-rust.mjs` desliga as guardas de autorização do `signaling.rs` **uma a uma** — troca `if self.is_host(…)` por `if true` — e corre a bateria. Primeira medição: **13 guardas, 5 defendidas, 8 SEM TESTE**.
- **Porque isto é grave e não é dívida de testes:** a invariante 8 do AGENTS.md afirma que os controlos do anfitrião são validados no servidor e nunca confiados ao cliente. Essa afirmação é sobre treze `if` espalhados por 2 800 linhas, e oito deles podiam desaparecer com a suite verde. Uma invariante que ninguém verifica é uma intenção.
- **As oito:** fechar sondagem, conceder partilha de ecrã, abrir o quadro a todos, ligar a transcrição, trancar a sala, restringir a partilha ao anfitrião, desligar a câmara de alguém, fechar o chat.
- **A mais perigosa:** `ShareGrant`. Sem a guarda, um participante **concede a si próprio** a permissão de partilha que o anfitrião lhe negou — a mensagem leva `to`, e nada obrigava esse `to` a não ser ele mesmo.
- **Duas eram código escrito DOIS DIAS ANTES** (`ForceCam` e `ChatToggle`, R92), com testes ao lado. O teste do chat verificava a anfitriã a fechá-lo e a recusa do envio — e nunca um participante a tentar fechá-lo.
- **O padrão comum a todas as oito, e é o que se leva daqui:** o teste que existia verificava **quem PODE a usar** o controlo, nunca **quem NÃO PODE a tentar**. «Funciona para o anfitrião» e «é recusado ao participante» são duas afirmações diferentes, e só a segunda é a autorização. Um controlo com teste só da primeira metade está tão desprotegido como um sem teste nenhum — com a agravante de parecer coberto.
- **Porque o arnês do Rust muta guardas e não operadores:** em Rust cada mutação custa uma recompilação. Mutar operadores seria horas para um relatório cheio de equivalentes; mutar as treze guardas dá treze perguntas, todas com significado de segurança, em minutos.
- **Depois:** 13 de 13 defendidas.
- **Ficheiros:** `scripts/mutantes-rust.mjs`, `server/src/signaling.rs`.

### R95 — Seis rotas de organização nunca tinham sido testadas contra outro inquilino
- **Como se soube:** comparando o inventário do que EXISTE (`grep` às rotas `/api/orgs/{org_id}/*` do `main.rs` — 19) com o inventário do que se TESTA (o `isolamento.mjs` — 13). Mesmo método que apanhou os testes ponta-a-ponta que nunca corriam (R72): dois `grep` e a diferença é a lista.
- **As seis:** ler a trilha de auditoria de outra empresa, verificar-lhe a cadeia de hash, ler a configuração de SSO, ler a facturação de voz, **apagar-lhe uma chave de API** e **apagar-lhe um webhook**.
- **Nenhuma estava vulnerável. Nenhuma estava provada.** A diferença importa: o `check-route-auth.sh` garante que cada rota tem extractor de **autenticação** — sabe QUEM é. Não diz nada sobre **autorização** — se o handler confere que esse quem pertence à organização do caminho. São duas metades, e só havia portão para a primeira.
- **O que a sabotagem mostrou, e é a medida do raio de dano:** com o `require_admin` a devolver sempre `Ok`, a org A lê a trilha de auditoria da B **com nomes de actores e acções**, verifica-lhe a cadeia, lê o SSO, e apaga-lhe a chave de API e o webhook. Doze asserções ficam vermelhas.
- **Um `404` sozinho não prova autorização.** Nos dois `DELETE`, testar com um UUID ao acaso daria `404` — que o helper conta como recusa — sem provar coisa nenhuma: só que o recurso não existe. A versão que vale é B **criar** o recurso, A tentar apagá-lo, e a asserção final ser **«o recurso da B continua lá»**. Foi essa que apanhou a destruição quando a guarda caiu; a do código de estado teria passado na mesma se o handler apagasse e devolvesse 404.
- **As recusas devolvem `404` e não `403`**, deliberadamente: um `403` confirmaria que a organização existe.
- **Portão:** `scripts/check-isolamento-cobertura.sh`, no CI. Visto a recusar uma rota de organização acrescentada sem cobertura.
- **Ficheiros:** `web/e2e/isolamento.mjs`, `scripts/check-isolamento-cobertura.sh`, `.github/workflows/ci.yml`.

### R96 — Vinte e uma rotas de recurso por ID nunca tinham sido pedidas com o token errado
- **Como se soube:** a mesma comparação de inventários do R95, agora aplicada aos recursos POR ID. Existiam **32 rotas não-públicas** de sala, reunião, gravação e quadro; o teste de isolamento tocava em **8**.
- **A regra que decide o que é grave:** uma **sala é uma capability** — quem sabe o código vê os metadados e pede para entrar, e isso está no topo do `isolamento.mjs` desde sempre. Um **recurso por ID não é**: a acta de uma reunião, o ficheiro de uma gravação e o PNG de um quadro não têm código para partilhar, e o `id` é opaco. Confundir os dois faz parecer aceitável o que não é.
- **O que a extensão do teste encontrou, e é um defeito a sério:** `POST /api/rooms/{code}/minutes` corria a consulta da reunião **antes de qualquer autorização** e devolvia `200 {"ok":false","reason":"no meeting for room"}` a quem apenas soubesse o código, de outra organização. Nada era escrito — o delegado `save_minutes` autoriza —, mas a resposta já dizia **se a sala tinha reunião agendada**, e um `200` num pedido não autorizado é o padrão que o `/v2/apply` já tinha ensinado a não repetir. Corrigido: a autorização entra na própria consulta e as duas hipóteses («não há reunião» e «não é tua») passam a dar o mesmo `404`.
- **Duas armadilhas apanhadas pelo CONTROLO POSITIVO, e é ele que salva o teste:**
  1. `/api/meetings/{id}` só tem `DELETE` e `/minutes` só tem `POST`. Um `GET` devolve **405**, que o helper contava como recusa — duas asserções verdes a medir o router, não a autorização. Só se deu por isso porque o controlo positivo («B lê a sua própria reunião») **também** devolveu 405.
  2. Dois `POST` devolviam **422** por corpo mal formado (`response` em vez de `status`, `shared` em vez de `public`). Recusados por validação, não por autorização.
- **Um `404` num id inventado não prova nada** — só que o recurso não existe. Onde o recurso se pode fabricar (reunião, quadro), o teste cria-o com a org B, tenta destruí-lo com a A, e afirma que **continua lá**. Onde não se pode (gravações, que precisam de uma chamada a sério), está escrito que a prova é mais fraca.
- **Portão:** o `check-isolamento-cobertura.sh` passou a cobrir também os recursos por ID — 53 rotas ao todo. Foi ele que encontrou mais sete que eu tinha deixado de fora depois de julgar a lista completa.
- **Ficheiros:** `web/e2e/isolamento.mjs`, `scripts/check-isolamento-cobertura.sh`, `server/src/meetings.rs`.

### R97 — O mesmo erro duas vezes, e cinco camadas construídas por cima dele
- **O erro:** um teste que usa Playwright colocado ANTES do `npx playwright install` do próprio job. Morre com `Executable doesn't exist at .../chrome-headless-shell`.
- **Duas vezes em dois dias:** o portão da barra responsiva no job `frontend` (R86), e o teste de reentrada no job `isolamento` (R91). Nos dois casos a causa é a mesma e a correcção foi a mesma.
- **Porque é fácil de repetir:** o `npm ci` dá a sensação de ter instalado tudo. Traz a **biblioteca** do Playwright; os browsers vêm de um comando à parte. E o sintoma não aponta para a causa — parece um problema de ambiente, não uma linha fora de ordem. A segunda vez foi ainda mais fácil porque o job `isolamento` **já tinha** um `playwright install`: bastou pôr o passo novo vinte linhas acima dele.
- **O que custou de verdade, e é a parte que interessa:** empurrei o R91 e **não verifiquei o CI dele**. Depois construí **cinco PRs por cima**. Os seis estiveram vermelhos no mesmo sítio durante quatro iterações, e só apareceu ao ir fundir a pilha. Corrigir o mesmo erro duas vezes é distração; construir cinco camadas sobre ele sem olhar é **processo**.
- **Regra:** um PR empurrado sem se ver o CI dele é trabalho por verificar, não trabalho feito — e uma pilha faz herdar o vermelho para cima em silêncio.
- **E houve uma TERCEIRA, na mesma linha, ao corrigir a segunda:** ao mover o passo para depois do `playwright install`, ele apanhou o `working-directory: web` do passo vizinho. Neste job os e2e correm da raiz e o caminho já diz `web/e2e/` — com o working-directory, o Node procura `web/web/e2e/reentrada.mjs` e morre com `MODULE_NOT_FOUND`. Três erros seguidos na mesma linha de CI, cada um encontrado só quando o anterior deixou de tapar o seguinte.
- **E a CAUSA VERDADEIRA, que só apareceu à quarta:** o `npx playwright install chromium` do job `isolamento` corria na **raiz**, onde não há `node_modules`. O npx descarregava o Playwright mais recente e instalava os browsers **dessa** versão, enquanto os testes usam a do `web/node_modules`. O aviso estava no log e passou despercebido: `npm warn exec The following package was not found and will be installed: playwright@1.63.0`. O job `frontend` nunca teve o problema porque tem `defaults.run.working-directory: web` — foi por isso que o portão da barra funcionou e este não. Estava latente desde que o passo existe; só começou a doer quando o Playwright a montante subiu de versão.
- **Porque é que as três primeiras correcções pareceram certas:** as quatro causas dão o **mesmo sintoma**, `Executable doesn't exist`. Cada correcção tapava a anterior e o erro reaparecia igual, o que se lê como «não ficou bem corrigido» em vez de «é outra coisa». A lição: quando a mesma mensagem volta depois de uma correcção que se acredita certa, a hipótese a testar não é «corrigi mal» — é **«há mais do que uma causa»**.
- **E uma QUINTA, que não é de browsers:** o passo apontava a `localhost:5174` e o vite é arrancado **dentro** do bloco `run:` do passo do MFA — o meu vinha antes. `ERR_CONNECTION_REFUSED`, que se lê como «o vite não subiu» quando o que aconteceu foi correr cedo demais. Cinco problemas seguidos com a mesma linha de CI, e só o quinto tinha uma mensagem diferente dos outros quatro.
- **O que isto diz sobre passos de CI que dependem de serviços:** o vite não vive num passo próprio; nasce e morre dentro de um `run:`. Isso não se vê de fora, e um passo novo colocado «logo a seguir» pode cair fora do que julga estar dentro. O portão passou a exigir que um `APP=…:PORTA` tenha um `vite --port PORTA` (ou `vite preview --port`) antes, **no mesmo job**.
- **A lição sobre mover código:** um passo movido não leva só o que está seleccionado. Leva a POSIÇÃO, e com ela tudo o que a posição implicava — neste caso um `working-directory` que pertencia ao vizinho e que ninguém olhou porque não fazia parte do que se copiou.
- **Portão:** `scripts/check-browser-antes-do-e2e.sh`, e verifica TRÊS coisas porque o mesmo passo errou nas duas: (1) para cada JOB, todo o teste de `web/e2e/` que importa `@playwright/test` corre depois de um `playwright install` **nesse mesmo job** — um noutro job não vale, que foi a suposição que falhou da primeira vez; (2) um passo cujo comando diz `node web/e2e/…` **não** tem `working-directory: web`; (3) o `playwright install` corre onde **está** o `node_modules`. Visto a recusar as três versões do erro.
- **O portão foi reescrito com um parser de YAML** depois de a primeira versão, feita com expressões regulares, atribuir passos ao job errado. Um portão que reporta o sítio errado é pior do que nenhum: manda procurar onde não está.
- **Ficheiros:** `scripts/check-browser-antes-do-e2e.sh`, `.github/workflows/ci.yml`.

### R98 — Duas renovações de sessão ao mesmo tempo punham o utilizador na página de entrada
- **Sintoma:** depois de um `F5`, o utilizador aparece na página de entrada. A sessão não expirou — foi **revogada por ele próprio**.
- **Causa raiz:** o servidor **rota** o refresh token (`UPDATE refresh_tokens SET revoked = TRUE` a cada uso, `auth.rs`), e o cliente não tinha guarda de concorrência. Duas chamadas que levem 401 quase ao mesmo instante chamam `refreshSession()` as duas com o **mesmo** cookie: a primeira roda-o, a segunda encontra-o revogado, leva 401, e o `refreshSession` faz `logout()` mais `dx-auth-expired`.
- **Quando acontece a sério:** logo depois de um refresh da página, quando várias chamadas partem em paralelo com o token de acesso já expirado. Numa máquina rápida a primeira renovação acaba antes de a segunda chamada falhar e não se vê; numa lenta — ou numa **rede** lenta, que é o caso normal do nosso mercado — sobrepõem-se. Há dois chamadores independentes: o `request()` a retomar um 401, e o `tryRefreshToken()` que o cliente de presença usa antes de cada reconexão de WebSocket.
- **Como foi encontrado, e é a parte que interessa:** o `web/e2e/reentrada.mjs` passava em local e falhava **sempre** no runner do CI. Durante **seis rondas** tratei-o como um problema do ambiente do teste — e cinco vezes era mesmo (browsers em falta duas vezes, caminho duplicado, `install` na raiz, vite ainda não arrancado, R97). À sexta pu-lo fora do CI com razão escrita. Só ao ir investigar a razão é que se viu que a sexta era **o produto a dizer a verdade**.
- **A lição:** «passa aqui e falha no CI» é uma hipótese sobre o AMBIENTE, e é a mais provável — mas não é a única. Uma máquina lenta não inventa defeitos: **expõe corridas que a rápida esconde**. Quando as diferenças de ambiente estão todas fechadas e o sintoma fica, o candidato seguinte é o produto.
- **Correcção:** uma promessa partilhada — quem chegar enquanto uma renovação decorre espera pela mesma, em vez de começar outra.
- **Portão:** teste em `api.guardas.test.ts` com o esboço a **rotar** como o servidor (a segunda renovação devolve 401). Sem a guarda, uma das duas chamadas rebenta com «session expired»; visto a falhar. E o `reentrada.mjs` volta ao CI, que é onde tinha de estar.
- **Ficheiros:** `web/src/api.ts`, `web/src/api.guardas.test.ts`, `.github/workflows/ci.yml`, `scripts/e2e-fora-do-ci.txt`.

### R99 — O ecrã principal do produto não existia em inglês nem em francês
- **Medido:** o `Room.tsx` — 4 300 linhas, a sala, o ecrã onde uma reunião acontece — tinha **zero** chamadas a `t()`. Não era uma tradução incompleta: era uma sala que só existia em português, com 164 literais visíveis directamente na marcação.
- **A minha contagem anterior estava errada e vale a pena dizer porquê:** reportei «147 `t()` e 96 literais» num relatório anterior. O `grep -o 't('` estava a contar `getContext('2d')`, `import('../webrtc')` e `document.querySelector(...)`. Um `grep` que casa o nome de uma função sem a fronteira de palavra mede outra coisa — e a conclusão que dele saiu («i18n incompleto») era mais benigna do que a realidade («i18n ausente»).
- **O produto vende-se como lusófono E internacional.** Com a landing, o login e a consola traduzidos e a SALA não, um utilizador inglês percorre o produto em inglês até ao momento em que entra numa reunião — e a partir daí está tudo em português. É o pior sítio possível para a tradução parar.
- **Feito:** espaço `room` com **145 chaves** em `pt`, `en` e `fr`, agrupadas por painel (pré-entrada, espera, pessoas, chat, ferramentas, definições, fundos, barra, quadro). 163 substituições no `Room.tsx`, mais o `useTranslation()` no componente principal e nos três sub-componentes que o ficheiro define (`DeviceControl`, `Whiteboard`, `PresentationTile`) — cada componente precisa do seu, e o compilador foi quem os apontou.
- **As traduções são minhas, não de tradutor.** São defensáveis para interface, mas uma revisão por falante nativo de francês é trabalho por fazer, e está dito no PR em vez de escondido.
- **Portão:** `lote2`, 3.2.7, e guarda duas coisas — que não voltem a entrar literais visíveis fora do `t()`, e que os três locales tenham **exactamente** as mesmas chaves. A segunda metade importa tanto como a primeira: uma chave só em `pt` mostra-se ao utilizador inglês como o identificador cru, que é pior do que a frase em português. Visto a recusar as duas.
- **O que fica de fora, declarado:** 86 literais nos outros 20 ficheiros (`MfaPanel` 18, `ApiDocs` 15, `Shell` 9, `SharePage` 9, …). O portão cobre a sala; os outros seguem o mesmo padrão.
- **Ficheiros:** `web/src/pages/Room.tsx`, `web/src/locales/{pt,en,fr}.ts`, `web/src/lote2.invariantes.test.ts`.
### R100 — A marca-branca estava feita a meio: renomear a aplicação deixava o logótipo alheio em cinco ecrãs
- **O que se via primeiro, e era o menor dos dois problemas:** duas marcas para a mesma aplicação. O globo de `/logo.svg` na landing, no lobby, no estado, no legal e nos docs; e um quadrado com a inicial no rail da consola. Incoerente, mas inofensivo.
- **O defeito a sério só aparece ao RENOMEAR.** O `branding.ts` deixa quem usa o produto pôr-lhe outro nome. O quadrado adapta-se — usa a inicial do nome configurado. Os cinco ecrãs com `<img src="/logo.svg">` **não olhavam para o nome**: continuavam a mostrar o globo Delonix. Uma instalação renomeada mostrava a marca **de outra empresa** em metade do produto.
- **Porque é que não se via:** a funcionalidade de renomear existe e funciona — o nome muda em todo o lado. É só o SÍMBOLO que não acompanha, e ninguém testa uma instalação renomeada.
- **A lição sobre o que se lê num relatório:** eu tinha isto anotado como «a landing usa um glifo, a app usa um quadrado — escolher um». Se tivesse agido pelo relatório, teria escolhido um dos dois e **fixado o defeito**: escolher o globo quebra a marca-branca por inteiro; escolher o quadrado deita fora o logótipo. A resposta certa não era escolher — era **decidir em função do nome**, e isso só se vê a olhar para o `branding.ts`.
- **Feito:** um componente `BrandMark` único nos seis sítios. Desenha o logótipo enquanto o nome for o de origem, e o quadrado com a inicial a partir do momento em que deixar de ser. Reage ao evento `dx-branding`, como o resto do sistema de marca. A variante `big` — que o `.brand-logo` tinha e o quadrado não — passou a existir para os dois.
- **Portão:** `lote2`, 3.2.8, com duas metades: nenhum dos seis ecrãs desenha `/logo.svg` à mão, e o `BrandMark` **decide pelo nome** e não por uma constante. A segunda impede o caso mais fácil de errar — um invólucro que devolve sempre o logótipo teria passado a primeira e deixado o defeito de pé, agora escondido atrás de um nome tranquilizador.
- **Ficheiros:** `web/src/components/BrandMark.tsx`, `web/src/branding.ts`, `web/src/components/Shell.tsx`, `web/src/pages/{Status,Legal,Lobby,Landing,ApiDocs}.tsx`, `web/src/styles.scss`.

### R101 — Corrigi o símbolo da marca e deixei o nome escrito à mão ao lado
- **Continuação directa do R100, e é uma correcção minha incompleta.** O `BrandMark` fez o símbolo seguir o nome configurado. Mas o NOME continuava escrito à mão mesmo ao lado dele — `<BrandMark /> Delonix <span>Meet</span>` — na landing (×2), no lobby, no legal, no estado e nos docs.
- **O resultado era pior do que antes da correcção:** uma instalação renomeada passava a mostrar o símbolo novo colado ao nome antigo. Antes havia uma incoerência; depois havia uma contradição.
- **Como apareceu:** ao inventariar os literais que faltavam traduzir. As ocorrências de `Delonix` apareceram na lista como «texto por traduzir» — e não são: o nome de uma marca não se traduz, **configura-se**. Foi a lista errada que revelou o problema certo.
- **E escapou-me uma à primeira:** converti quatro páginas e deixei o `Legal.tsx`, que tem exactamente o mesmo padrão. Só apareceu ao correr um `grep` pelo padrão em vez de confiar na lista que eu próprio tinha feito.
- **Feito:** `BrandLockup` — símbolo e nome da mesma fonte, com um `suffix` opcional para os cabeçalhos que acrescentam algo («— Estado do serviço», «· API REST»).
- **Portão:** `lote2`, 3.2.8, terceira asserção — nenhuma das sete páginas escreve `Delonix <span>`. Deliberadamente estreito: proíbe o LOCKUP escrito à mão, não o nome dentro de uma frase, que é problema de i18n e resolve-se por interpolação.
- **Ficheiros:** `web/src/components/BrandMark.tsx`, `web/src/pages/{Landing,Lobby,Legal,Status,ApiDocs}.tsx`, `web/src/lote2.invariantes.test.ts`.

### R102 — W2.5 fechado: zero texto de interface fora do `t()` em todo o `web/src`
- **Depois da sala (R99) sobravam 47** strings de interface em 14 ficheiros — MFA, docs da API, partilha de gravação, definições, notificações, estado.
- **A contagem que eu tinha era 86 e estava alta:** o regex do relatório apanhava strings de uma palavra só. O portão exige um ESPAÇO — o que distingue uma frase de um identificador — e com esse critério eram 67, dos quais 20 eram marca ou código (`Delonix`, `X-Delonix-Signature`, `sha256`, `NFS`). Texto a sério: **47**.
- **Oito já tinham chave.** O `pt.ts` tem 634 entradas, e «A carregar…», «Cancelar», «Email», «Silenciar» e outras já lá estavam. Criar chaves novas para elas teria duplicado o dicionário — a diferença entre inventariar e traduzir.
- **Quatro espaços de nomes novos** (`api`, `mfa`, `share`, `status`) e 39 chaves em pt/en/fr.
- **O hook não vai onde o ficheiro começa, vai onde o `t` é usado.** Vários ficheiros definem mais do que um componente, e a primeira tentativa pôs o `useTranslation()` no primeiro de cada um — o compilador respondeu com «`t` is declared but never read» num sítio e «cannot find name `t`» noutro. A colocação passou a ser guiada pelas linhas que o `tsc` aponta, em ciclo, até parar.
- **O portão passou a cobrir `web/src` INTEIRO**, com a lista de ficheiros DERIVADA da árvore. A alternativa — acrescentar ficheiros a uma lista à mão — fica desactualizada no dia em que alguém cria um ficheiro novo, e o portão passa a proteger menos do que diz. Provado com um literal posto num ficheiro que nunca esteve em lista nenhuma.
- **O que fica de fora, e é deliberado:** marca (`Delonix`, que se **configura** — R100/R101) e identificadores técnicos (`X-Delonix-Signature`, `sha256`, `NFS`, `WebDAV`). Nenhum deles se traduz.
- **Ficheiros:** 14 `.tsx`, `web/src/locales/{pt,en,fr}.ts`, `web/src/lote2.invariantes.test.ts`.
### R103 — No telemóvel, começar uma reunião exigia abrir o menu
- **Medido nas capturas:** na Home em 375px não há forma de criar nem de entrar numa reunião. O bloco «Nova reunião · Introduz um código · Participar» vive na barra do topo, e abaixo dos 900px a barra passa-o para a gaveta. A acção principal do produto ficava atrás de um toque no menu — num ecrã com **metade da altura vazia**.
- **Duas decisões deliberadas por trás disto, e nenhuma estava errada:** (1) a 3.1.4 diz que as acções **mudam-se** para a gaveta em vez de desaparecerem — e tem teste; (2) o `Home.tsx` diz por comentário que as acções foram **movidas** para a barra, para não duplicarem. As duas são defensáveis.
- **O que ninguém considerou foi o CORPO da página.** A escolha foi sempre entre barra e gaveta. Com a barra a esvaziar-se em ecrã estreito e o corpo a ficar vazio, o sítio óbvio nunca entrou na conversa.
- **Correcção:** o MESMO componente (`QuickActions`, que o teste 3.1.4 já exigia que fosse um só) ganha uma terceira variante e aparece no corpo da Home — escondido acima dos 900px, onde a barra já o tem, e visível abaixo, no mesmo limiar em que a barra o larga. **Não é duplicação: é o mesmo bloco a viver onde é alcançável em cada largura.**
- **O que NÃO se fez, e porquê:** o relatório de UI dizia «quatro cartões duplicam o rail — remover». Não se removeram. No telemóvel o rail é uma gaveta escondida, e esses quatro atalhos são a única navegação visível; removê-los por serem redundantes **em ecrã largo** partia o ecrã estreito. Ver antes de apagar.
- **Verificado por captura nas duas larguras**, não só por asserção: em 375px o botão aparece a toda a largura sobre o campo de código; em 1280px o corpo fica exactamente como estava.
- **Portão:** `lote2`, 3.1.4 — a regra tem duas metades e as duas foram vistas a falhar: escondido acima dos 900px, e **visível abaixo**. Sem a segunda, esconder nas duas larguras passaria.
- **Ficheiros:** `web/src/components/Shell.tsx`, `web/src/pages/Home.tsx`, `web/src/styles.scss`, `web/src/lote2.invariantes.test.ts`.

### R104 — A sala falava, e ninguém que não visse a ouvia
- **Três defeitos de acessibilidade na sala, todos com a mesma forma: informação que existe no ecrã e não chega a quem não o vê.**
  1. **Os avisos que exigem decisão apareciam em silêncio.** «Alguém quer entrar», «pedido de controlo remoto», «pedido de partilha», sondagem — quatro cartões com `role="dialog"`, que **não anuncia nada**: só rotula. Um anfitrião com leitor de ecrã não sabia que tinha gente à porta. Passaram a viver numa região `aria-live="assertive"` — interrompem de propósito, porque um convidado à espera não pode esperar pela próxima pausa na leitura.
  2. **A linha de estado não era anunciada.** «O anfitrião silenciou o teu microfone» era escrito no ecrã e mais nada: quem não vê ficava silenciado sem saber porquê. Passou a `role="status"` com `aria-live="polite"` — informa, não interrompe.
  3. **O Esc não fechava os painéis da sala.** Chat, pessoas, ferramentas e definições fechavam-se só no ×, o que obriga quem navega por teclado a percorrer o painel inteiro. O resto da consola já o fazia — a sala era a excepção. E o foco **volta a quem abriu**: fechar sem devolver o foco deixa o leitor de ecrã no `<body>` e perde-se o sítio.
- **O que NÃO se fez, e é deliberado:** não se prende o foco dentro dos painéis. Um `<aside>` não é um modal, e prender lá dentro impediria de chegar aos controlos da chamada — que é precisamente o que não se pode tirar a ninguém.
- **E o meu relatório estava errado numa afirmação:** «Esc fecha nada». Fechava em seis sítios — gaveta, menu de conta, notificações, tour, paleta e o emoji do chat. O `grep` que produziu essa frase tinha as aspas partidas e devolvia zero. **A sala é que era a excepção**, e a frase certa é muito mais estreita do que a que escrevi.
- **O PONTO CEGO dos dois portões anteriores:** o de emoji (R88) e o de i18n (R99/R102) olham para **JSX** — texto entre tags e atributos. Não olhavam para strings passadas a **funções**. Havia **46 mensagens de estado em português fixo**, duas delas com emoji, invisíveis para ambos. Isso passou a importar mais desde que a linha de estado é anunciada: **anunciar português a quem escolheu inglês é pior do que não anunciar**.
- **Portão:** `lote2` — nenhuma chamada a `setStatus`/`setErr`/`setError`/`setMsg` leva um literal em português. Visto a falhar, e apanhou logo uma que a minha própria conversão tinha deixado para trás (a que tinha o 🎮: a chave ficou sem o emoji e o texto não casou).
- **Ficheiros:** `web/src/pages/Room.tsx`, `web/src/components/Shell.tsx`, `web/src/pages/{Analytics,SharePage}.tsx`, `web/src/locales/{pt,en,fr}.ts`, `web/src/lote2.invariantes.test.ts`.

### R105 — O portão dos emoji só via metade da consola, e a régua do i18n cortava aos 80 caracteres

**Sintoma.** O portão 3.2.5 dava verde com 20 pictogramas colados a texto ainda
espalhados por cinco ficheiros da consola (`Analytics`, `Home`, `Landing`,
`Room`, `RemoteTile`), e o portão 3.2.7 dava verde com três frases longas
literais por traduzir.

**Causa.** Duas réguas escolhidas de cabeça em vez de derivadas do porquê. O
padrão de emoji cobria um intervalo que deixava de fora `U+FE0F` — o selector de
variação que faz de `⚙` um `⚙️` — e o de i18n só olhava para literais entre 3 e
80 caracteres, por eu ter presumido que texto de interface é curto. As três
frases que escaparam tinham 96, 118 e 141 caracteres.

**Regra.** A régua vem do PORQUÊ, não de um intervalo confortável. O emoji é
recusado como iconografia porque **rende conforme o sistema operativo do
visitante e não herda `currentColor`** — logo o padrão é «pictograma», incluindo
o selector de variação, e não «bloco Unicode X a Y». O texto de interface é
recusado fora do `t()` porque **um utilizador francês não o lê** — e uma frase
longa é lida por ele tanto como uma curta; o tecto sobe para 300.

**Portão.** `web/src/lote2.invariantes.test.ts`, testes 3.2.5 (`EMOJI =
/[\u{1F300}-\u{1FAFF}\u{FE0F}]/u` sobre 16 ficheiros de consola, saltando os
blocos `REACTION_EMOJIS`/`CHAT_EMOJIS` e os comentários) e 3.2.7 (literais de
3 a 300 caracteres). Provado vermelho antes de verde: as duas primeiras corridas
listaram 20 e 2 sítios reais.

**A quarta versão, e o pior dos quatro defeitos.** Depois de convertidos os 20,
o portão continuava a dar verde por cima de **223 linhas**. A regra era «a partir
de uma linha que mencione `REACTION_EMOJIS`, ignora até um `]`» — e a linha
`{REACTION_EMOJIS.map((e) => (` está a meio do JSX da barra de controlo; o `]`
que a fechava só aparecia 223 linhas abaixo. Toda a barra ficava fora do portão,
com dois emoji e duas frases por traduzir lá dentro. A isenção passou a valer
para a **linha** que nomeia a constante — uma linha, nunca um intervalo — e para
o corpo das declarações, delimitado por contagem de parênteses rectos.

**Onde `<option>` está em causa:** um `<option>` não aceita um `<svg>` dentro. Aí
o pictograma **sai** e fica só o texto — não se troca por um ícone que o browser
descarta em silêncio.

**Ficheiros.** `web/src/icons.tsx` (`KeyIcon`, `GlobeIcon`, `BotIcon`,
`ThumbIcon`), `web/src/pages/{Analytics,Home,Landing,Room}.tsx`,
`web/src/room/RemoteTile.tsx`, `web/src/locales/{pt,en,fr}.ts`,
`web/src/lote2.invariantes.test.ts`.

### R106 — Um arnês de mutação morto a meio deixava o produto sabotado na árvore

**Sintoma.** Depois de o `scripts/mutantes.mjs` ser interrompido por um timeout,
o `web/src/layerPolicy.ts` ficou na árvore de trabalho com um `&&` trocado por
`||` — a sabotagem que o arnês injecta de propósito. Foi encontrada por acaso, ao
ler um `git status` antes de um commit. Um `git add -A` tê-la-ia empurrado.

**Causa.** O restauro do ficheiro estava só no caminho normal, entre a escrita do
mutante e a corrida seguinte. Qualquer morte no meio — Ctrl-C, `kill`, timeout do
CI, a máquina a desligar-se — saltava-o.

**A tentativa que não chegou.** Um `process.on('SIGTERM', restaurar)` parece a
correcção óbvia e não é: o arnês passa a vida dentro de um `execSync` (a bateria
de testes), e o Node só corre o handler quando essa chamada síncrona regressa —
minutos depois, ou nunca. Medido: um SIGTERM ao arnês do Rust deixou-o vivo mais
de dois minutos com o `signaling.rs` mutado. E um SIGKILL não corre handler
nenhum.

**Regra.** A rede não pode viver na memória do processo que morre. O original vai
para um **marcador em disco ANTES** de o mutante ir para o ficheiro, e cada
corrida começa por devolver o que encontrar lá. Sobrevive a SIGKILL, a queda de
máquina e a bateria descarregada.

**Portão.** `scripts/check-repo-hygiene.sh` recusa um commit com
`scripts/.mutante-em-voo.json` presente e nomeia o ficheiro em risco. Provado
vermelho: com o marcador escrito à mão, o portão aponta o ficheiro; sem ele,
verde. E a recuperação foi provada a sério — ficheiro sabotado + marcador, o
arnês a arrancar escreveu `corrida anterior morreu a meio — … restaurado` e
devolveu-o.

**Ficheiros.** `scripts/mutantes.mjs`, `scripts/mutantes-rust.mjs`,
`scripts/check-repo-hygiene.sh`, `.gitignore`.

### R107 — O «zero texto fora do `t()`» era verdade só para nós de texto

**Sintoma.** O R102 fechou o lote do i18n com «zero texto de interface fora do
`t()` em todo o `web/src`», e o portão dava verde. Medido de outra maneira: **112
frases visíveis** ainda em português duro — 64 no `Room.tsx` e 21 no resto da
árvore. Entre elas o título de *todos* os botões da barra de controlo
(«Desativar microfone (Ctrl+D)», «Partilhar ecrã», «Levantar a mão»), os avisos
que o leitor de ecrã anuncia («Foste removido da reunião», «O anfitrião silenciou
toda a gente»), e mensagens de erro («Erro ao convidar. Tenta novamente.»).

**Causa.** O portão procurava `>frase<` — nós de texto JSX — mais três
atributos. Uma frase dentro de uma **expressão** nunca lhe passou à frente:

```tsx
title={micOn ? 'Desativar microfone (Ctrl+D)' : 'Ativar microfone (Ctrl+D)'}
setStatus('O anfitrião recusou a tua entrada')
```

Não é um caso raro — é como se escreve metade da interface de uma sala, onde
quase tudo tem dois estados.

**Regra.** O portão procura a FRASE, não o sítio onde ela está. Uma frase
distingue-se de um identificador pelo que uma pessoa lê: começa por maiúscula,
tem pelo menos um espaço, e tem uma palavra de três letras minúsculas. Isso deixa
de fora `'grid'`, `'room-topo'`, `'POST'` e os nomes de eventos sem ter de saber
onde cada literal é usado. As chamadas ao `t()` são retiradas antes de procurar —
são a solução, não o problema.

**Nomes próprios** ficam de fora **um a um, com razão escrita** dentro do portão
(`Microsoft Teams`, `Google Meet`, `API / Signaling`) — nunca por a regra ser
afrouxada. Um nome de produto não se traduz; uma frase sim.

**Portão.** `web/src/lote2.invariantes.test.ts`, teste «nenhum ficheiro tem
frases visíveis dentro de expressões» — árvore inteira, não só o `Room.tsx`.
Provado vermelho antes de verde (85 frases listadas) e provado outra vez depois:
devolver `'Base de dados'` ao `Status.tsx` põe-no a vermelho.

**E ainda não era tudo — mais duas famílias na mesma passagem.**

*Nós de texto MISTURADOS com expressões.* A regra dos nós de texto usava a classe
`[^<>{}\n]`, que **exclui `{`**. Um nó como `Notas AI {transcribing && <span/>}`
ou `A IA segmenta-te localmente… {bgBusy ? t(…) : ''}` era invisível para ela.
Dezasseis assim. A regra passou a **retirar as expressões** — respeitando o
encaixe das chavetas — e a julgar o que sobra como prosa.

*Atributos inventados.* A regra verificava três atributos **por nome**: `title`,
`placeholder`, `aria-label`. Mas quem escreve um componente inventa os seus —
`label=`, `desc=`, `data-tip=` — e todos acabam no ecrã ou no leitor de ecrã.
Onze escaparam assim, incluindo o rótulo de leitor de ecrã de **cinco botões da
barra de controlo**. A regra deixou de nomear atributos.

**O que fica de fora, e é honesto dizê-lo.** Uma frase que comece por minúscula
(`'nova password'` num `placeholder`) continua a passar. A maiúscula inicial é o
que distingue uma frase de um `className` como `'brand-square big'` — sem ela, o
portão acusa 162 literais dos quais a esmagadora maioria são nomes de classe, e
um portão que grita por tudo é ignorado tal como um portão cego. Fica registado
como limite conhecido, não como problema resolvido.

**Lição, e é a mesma pela quinta vez.** Um portão construído sobre a FORMA que o
defeito tinha da última vez erra na forma seguinte — R88, R99, R102, R105 e agora
esta. O que dura é a regra escrita a partir do PORQUÊ: aqui, «um utilizador
francês não lê isto», que não faz distinção entre um nó de texto e um ternário.

**Ficheiros.** `web/src/pages/{Room,SharePage,Status}.tsx`,
`web/src/components/{MfaPanel,PasswordInput,PresenceProvider,Shell}.tsx`,
`web/src/room/RemoteTile.tsx`, `web/src/locales/{pt,en,fr}.ts`,
`web/src/lote2.invariantes.test.ts`.

### R108 — W3.5: quem sai do separador da reunião perdia a reunião

**Lacuna, não regressão.** Numa reunião de trabalho ninguém fica no separador da
reunião: vai ao documento, ao terminal, ao email. O Meet e o Teams põem uma
janela pequena por cima de tudo; a sala não tinha nada — o PiP existia só no
visualizador de gravações.

**Como está feito.** A decisão de **quem** aparece na janela saiu do componente
para `web/src/pipPolicy.ts`, puro e testado à parte, pela mesma razão do
`layerPolicy.ts`: corre dezenas de vezes por reunião e não precisa de DOM. A
ordem vem do que a pessoa foi lá fazer — apresentação, depois afixado, depois
quem fala, depois o último que falou, depois qualquer um com câmara. O próprio
nunca é candidato.

**A guarda que não é óbvia:** `deveTrocarFonte`. Numa conversa a três,
`escolherFontePip` alterna de cara a cada frase, e a janela ficaria a piscar de
segundo a segundo — o browser faz um corte visível em cada troca. Por isso só se
troca quando a fonte actual **deixou de servir**: desligou a câmara, saiu, ou
alguém começou a apresentar.

**As três armadilhas que fazem o PiP falhar em silêncio**, todas com portão:

1. **`display: none` no vídeo escondido.** É a forma óbvia de o esconder e é a
   única que o browser trata como «não tem imagem» — o pedido é recusado sem
   erro visível. Esconde-se com 1×1 e `opacity: 0`.
2. **Botão onde o browser não suporta.** Firefox e o Safari de iOS não têm
   `pictureInPictureEnabled`. Um botão que não faz nada é pior do que botão
   nenhum: a pessoa carrega, não acontece nada, e conclui que o produto está
   partido.
3. **Recusa engolida.** O `requestPictureInPicture` rejeita se já houver uma
   janela noutro separador. Um `catch {}` vazio aqui era o R104 outra vez — o
   produto sabe que falhou e a pessoa não.

**Gestos.** Não há «abre sozinha quando mudo de separador»: essa permissão está
reservada a PWAs instaladas. O pedido exige um gesto E que o elemento já tenha
imagem — por isso a fonte é escolhida e ligada no clique, não no efeito que só
corre depois de o estado mudar.

**Portões.** `web/src/pipPolicy.test.ts` (18 testes; **9 mutações, 9 mortas**) e
`web/src/pip.invariantes.test.ts` (6 portões de forma). As três armadilhas foram
sabotadas uma a uma e as três puseram testes a vermelho.

**O que NÃO está provado.** Nenhum destes testes abre uma janela: o
`requestPictureInPicture` não corre em jsdom. Prova-se que a decisão está certa e
que as armadilhas não voltam a entrar — não que a janela abre no Chrome. Falta
uma passagem à mão num browser real, e está dito assim no PR.

**Ficheiros.** `web/src/pipPolicy.ts`, `web/src/pipPolicy.test.ts`,
`web/src/pip.invariantes.test.ts`, `web/src/pages/Room.tsx`,
`web/src/icons.tsx` (`PipIcon`), `web/src/locales/{pt,en,fr}.ts`,
`scripts/mutantes.mjs`.

### R109 — O controlo remoto dizia-se «ativo» e não encaminhava um único clique

**Sintoma.** Quem partilhava o ecrã via um botão «Solicitar Controlo Remoto». Ao
carregar, o dono do ecrã recebia um diálogo a pedir consentimento. Ao aceitar, o
outro lado recebia:

> **Pedido aceite — controlo remoto da tela partilhada ativo**

Não estava. Não há uma linha em todo o repositório que encaminhe um clique, uma
tecla ou uma coordenada para a máquina do outro. O handshake acaba na mensagem.

**A parte que interessa não é o botão.** É o **consentimento que não quer dizer
nada**. A pessoa foi informada de que estava a entregar o controlo da sua
máquina, disse que sim, e passou a comportar-se em conformidade — parou de
mexer, esperou que o outro agisse, ou (pior) ficou a achar que alguém tem acesso
ao seu teclado. Uma funcionalidade que não existe é uma lacuna; um consentimento
que não faz nada é um dano.

**Causa.** A sinalização foi construída primeiro — e bem — e a mensagem de
sucesso foi escrita a descrever o que a sinalização *iria* permitir, não o que
permitia. Ninguém voltou a ler.

**Porque é que não se «corrige a implementar».** Um browser **não consegue**
injectar rato ou teclado no sistema operativo de outra máquina. Não é uma API em
falta na nossa implementação: é a fronteira da sandbox, e é ela que faz do
browser um sítio seguro para abrir uma reunião. O Zoom e o Teams fazem-no porque
instalam uma aplicação **nativa** com permissões de injecção de input. Controlo
remoto a sério = um agente nativo, com a superfície de segurança que isso traz —
uma porta para o teclado da vítima é exactamente o que um atacante quer. Isso é
um projecto com ADR próprio, não uma tarefa.

**Regra.** Uma capacidade pode ser PROMETIDA (roadmap sem `done`) e não pode ser
ANUNCIADA COMO ACTIVA sem código por trás. A promessa fica; a promessa cumprida
sai. `web/src/capabilities.ts` passa a ser o único sítio onde isto se liga:
enquanto `AGENTE_CONTROLO_REMOTO` for `false`, o botão não aparece e um pedido
que chegue de um cliente antigo é **recusado automaticamente** — antes de abrir
qualquer diálogo. A sinalização fica intacta: é a base correcta, já testada.

**Portão.** `web/src/capabilities.invariantes.test.ts`. Três testes, e o terceiro
é o que interessa a prazo: **se alguém ligar a bandeira sem construir o agente, o
portão passa a exigir que exista encaminhamento de input** — e falha. Provado
vermelho nas duas direcções: ligar a bandeira põe-no a vermelho; devolver o
diálogo ao pedido põe-no a vermelho.

**Limite honesto.** Isto prova que ESTA capacidade não se anuncia sem código.
Não varre o produto à procura de outras mensagens de sucesso que mintam — o
`check-capability-claims.sh` cobre as afirmações de marketing (roadmap com `done`
e listas de preço), e este cobre a de runtime que já falhou. Uma varredura geral
por mensagens de sucesso continua por fazer.

**Ficheiros.** `web/src/capabilities.ts`,
`web/src/capabilities.invariantes.test.ts`, `web/src/pages/Room.tsx`,
`docs/competitive-positioning.md`.

### R110 — A busca por texto por traduzir nunca olhou para crases

**Sintoma.** Doze frases visíveis continuavam em português duro depois do R107,
entre elas o aviso que uma pessoa lê justamente quando a rede está má:

```tsx
`Sem ligação ao servidor — a tentar de novo (${tentativas}/${MAX_TENTATIVAS})…`
`Quadro branco partilhado por ${m.by}`
`Transcrição iniciada por ${m.by} — a tua fala é captada`
```

**Causa.** Todas as versões do portão procuraram literais entre plicas. Uma
frase com um valor lá dentro escreve-se com **crases** — e é precisamente a
frase que tem um valor lá dentro que costuma ser a mais importante: o nome de
quem partilhou, o número da tentativa, o estado da ligação.

**Regra.** A interpolação é retirada antes de julgar. O que interessa é a prosa
à volta dela — é ela que um utilizador francês não lê. As chaves passam a levar
parâmetros (`{{nome}}`, `{{n}}/{{total}}`), que é como o i18next já sabe fazer.

**Portão.** `web/src/lote2.invariantes.test.ts`, teste «nenhum TEMPLATE LITERAL
leva uma frase escrita à mão». Provado vermelho: devolver o template ao
`Quadro branco partilhado por` põe-no a vermelho.

**Sexta vez.** R88, R99, R102, R105, R107 e agora esta. A causa nunca é a mesma
forma — é sempre a mesma decisão: escrever o portão a partir da forma que o
defeito tinha da última vez. Aqui a regra que teria evitado as seis está escrita
desde o R107 e não foi aplicada até ao fim: **procurar a FRASE, em qualquer
forma que o TypeScript tenha de a escrever** — plica, crase, nó de texto,
atributo.

**Ficheiros.** `web/src/pages/Room.tsx`,
`web/src/components/{PresenceProvider,Shell}.tsx`,
`web/src/locales/{pt,en,fr}.ts`, `web/src/lote2.invariantes.test.ts`.

### R111 — Em sala mesh, «parar partilha» não parava a captura do ecrã

**Sintoma.** Numa sala com `topology: "mesh"`, carregar no botão de parar
partilha repunha a câmara — e **deixava o browser a capturar o ecrã**, com o
aviso «está a partilhar o seu ecrã» aceso. A pessoa acreditava que tinha parado.

**Causa.** Os dois caminhos guardam o stream do `getDisplayMedia` em sítios
diferentes, e só um deles o parava:

- **SFU** — o ecrã é uma track ADICIONAL, e o stream fica em `presentation`. Ao
  parar, `presentation?.stream.getTracks().forEach(stop)` apanha tudo. Correcto.
- **Mesh** — o ecrã **substitui** a câmara por `replaceVideoTrack`. O stream não
  fica em `presentation` nem em lado nenhum: passada a chamada, a única
  referência era a variável local `display`, já fora de alcance. Nada o parava.

Só parava por acidente: se a pessoa usasse o botão **do browser**, o
`screenTrack.onended` disparava. Pelo nosso botão, não.

**Segundo defeito, na mesma função.** O `SCREEN_CONSTRAINTS` pede áudio do
sistema — por isso o browser mostra a caixa «partilhar áudio do separador». No
mesh não há para onde o enviar: o ecrã viaja no lugar da câmara e não há uma
segunda track a publicar. A pessoa marcava a caixa, a track era criada, ninguém a
publicava e ninguém a parava. **É o consentimento vazio do R109 em ponto
pequeno**: uma caixa que se marca e não faz nada.

**Regra.** Quem adquire uma captura é dono de a parar, e o dono tem de ser
alcançável a partir do sítio onde se pára. No mesh isso passou a ser o
`displayStreamRef`. O áudio que o mesh não pode publicar é parado **e
explicado** — não descartado em silêncio.

**Portão.** `web/src/partilhaEcra.invariantes.test.ts`. Quatro testes: os dois
caminhos param, o áudio órfão é parado com aviso, e o caminho SFU **continua** a
publicar áudio do sistema — este último para que a promessa «partilha de ecrã com
áudio do sistema» não se torne falsa nos dois caminhos em vez de um. Provado
vermelho nos três sítios.

**O que NÃO está provado.** Não abri dois browsers. O que se prova é que as
tracks são paradas e que o aviso existe — não que o indicador do Chrome apaga.
Isso é uma verificação à mão, e continua por fazer.

**Ficheiros.** `web/src/pages/Room.tsx`,
`web/src/partilhaEcra.invariantes.test.ts`, `web/src/locales/{pt,en,fr}.ts`.

### R112 — Sete versões depois, o portão do i18n deixou de ser uma expressão regular

**O que ainda escapava.** Depois de seis gerações do portão, **76 frases**
visíveis continuavam sem passar pelo `t()` — e não eram cantos: a página inteira
de documentação da API, a explicação da E2EE («Com a frase errada não vês nem
ouves os outros…»), a do MFA, a das legendas com LLM local, e o aviso de ligação
insegura do `App.tsx` («câmara, microfone e chamadas NÃO funcionam»).

**A causa, e é a mesma das seis vezes anteriores.** Uma expressão regular **não
sabe o que é JSX**. Sabe o que é `>` e `<` — e por isso confunde um genérico
`useState<Foo>` com uma tag, não distingue `className` de `aria-label`, e não vê
que um nó de texto continua depois de uma expressão. A exigência de **maiúscula
inicial** existia só para calar esse ruído: medido, sem ela a regex acusava
**400 sítios, quase todos código**. E era essa exigência que deixava passar
`, como administrador). Envia-a num destes headers:` — meia frase, mas frase.

**A regra.** O portão passou a usar o **parser do TypeScript**. Um `JsxText` é
texto que aparece no ecrã, por definição. Um `JsxAttribute` tem um nome que se
pode ler. Não há heurística sobre a forma da linha, e por isso não há forma
seguinte por onde fugir.

E a lista de atributos passou a fazer o **contrário** do que fazia: nomeia os que
**chegam** a uma pessoa (`title`, `alt`, `label`, `desc`, `data-tip`, `aria-*`,
…) em vez dos que não chegam. Um atributo novo entrava em silêncio na versão
antiga; nesta, entra no portão.

**O que fica de fora, e porquê.** O parser cobre JSX. Strings passadas a funções
(`setStatus('…')`) e template literals continuam a ser cobertos pelos dois
portões de expressão que já existiam — esses não são JSX e o parser não os
distingue de código. São três portões complementares, não um a substituir outro.

**Ledger.** Dezasseis nomes e fragmentos técnicos ficam de fora **um a um, com a
razão ao lado** — `kubectl apply`, `X-Delonix-Signature: sha256=…`, `TrueNAS /
NFS`, e os nomes dos idiomas, que se escrevem **no** idioma. Nunca por a regra
ser afrouxada.

**Efeito colateral apanhado a tempo.** Ao mover as frases para os locales, um
`🔌` foi com elas — e o portão dos emoji só olha para `.tsx`. Passar um problema
para onde o portão não olha não é resolvê-lo: virou `PlugIcon`.

**Portão.** `web/src/lote2.invariantes.test.ts`, teste «nenhum JSX tem texto de
interface fora do t() (parser, não regex)». Provado vermelho nas duas formas que
as seis versões anteriores deixaram passar em alturas diferentes: devolver um nó
de texto (`Esbater fundo`) e devolver um `aria-label`.

**Ficheiros.** `web/src/lote2.invariantes.test.ts` (três portões de regex
substituídos por um de parser), `web/src/App.tsx`,
`web/src/components/{MfaPanel,PresenceProvider,Shell}.tsx`,
`web/src/pages/{Analytics,ApiDocs,Recordings,Room,SharePage,Status}.tsx`,
`web/src/room/RemoteTile.tsx`, `web/src/icons.tsx` (`PlugIcon`),
`web/src/locales/{pt,en,fr}.ts`.

### R113 — O francês tinha vinte chaves a menos, e ninguém falhava

**Sintoma.** Um utilizador francês via `admin.ssoTitle`, `recordings.searchPh` e
mais dezoito **identificadores crus** no ecrã — o painel de SSO inteiro e metade
das gravações. Uma chave em falta no i18next não falha nem avisa: mostra o nome
da chave.

**Causa.** O portão de paridade olhava **só para o bloco `room`** — foi escrito
quando o problema era a sala (R99) e nunca cresceu com o ficheiro. Fora desse
bloco, os três locales podiam divergir à vontade. E divergiam: além das 20 em
falta, o francês tinha quatro chaves órfãs num bloco `rec` que nada lê.

**Segundo defeito, encontrado a MEDIR e não a olhar.** O francês tinha, no mesmo
botão onde o português diz «Reunião E2EE» (14 caracteres), a frase «🔒 Créer une
réunion E2EE (chiffrée de bout en bout, avec phrase secrète)» — 62. E o mesmo em
«Sala de espera». Um rótulo que quadruplica não cabe onde cabia, e **ninguém dá
por isso sem abrir a aplicação em francês** — que é precisamente o que eu tinha
escrito duas vezes como «não validado».

**Regra.** Paridade em **todas** as chaves, não num bloco. E uma tradução mais de
2,2× mais longa (+12 caracteres) do que o original é tratada como defeito: o
limiar deixa passar a expansão normal do francês e do inglês, que é real e ronda
os 20 %, e apanha quem escreveu uma explicação onde devia estar um rótulo.

**Terceiro.** O portão dos emoji passou a olhar também para os locales — o R112
mostrou que uma frase com emoji mudada para lá deixa de ser vista. A fronteira é
entre **iconografia** e **prosa**, e é o porquê que a traça: um emoji no início
de um rótulo está no lugar de um ícone (`🚪 Sala presencial`, `📅 .ics` →
`DoorIcon`, `CalendarIcon`); um emoji dentro de uma frase é tom («Tudo pronto!
🎉») ou aponta para um glifo que o próprio browser desenha («clica no cadeado 🔒
na barra de endereço») — e aí trocá-lo por um ícone nosso tornaria a frase menos
útil. Não há regra de posição a adivinhar: há uma lista curta, com a razão ao
lado.

**Portão.** `web/src/lote2.invariantes.test.ts` — paridade total (>900 chaves),
divergência de comprimento, e emoji nos locales. Os três sabotados, os três a
vermelho.

**Ficheiros.** `web/src/locales/{pt,en,fr}.ts`, `web/src/pages/Calendar.tsx`,
`web/src/lote2.invariantes.test.ts`.

### R115 — O módulo que cifra a media não tinha um único teste

**Sintoma (ausência, não avaria).** `web/src/e2ee.ts` é o que cumpre a promessa
mais destacada do produto — «nem o SFU nem qualquer intermediário consegue
ver/ouvir». Não tinha **um** teste. Nem de derivação de chave, nem de ida e
volta, nem de recusa.

**Porque é que passou despercebido.** A lógica vive dentro de uma **string**
(`WORKER_SRC`): é o código que corre no Worker, e um Worker recebe texto. Isso
põe-na fora do alcance do TypeScript, do lint e da cobertura — um erro de sintaxe
lá dentro só apareceria quando alguém entrasse numa sala E2EE. Uma string não
compila.

**Como se testa sem browser.** A string é **extraída do próprio ficheiro** e
avaliada em Node, com a WebCrypto do Node — que é a mesma API. Não se copia o
código para o teste: um teste sobre uma **cópia** prova que a cópia funciona, e é
assim que se deixa de ver a divergência.

**O que ficou provado.** O offset do header (10/3/1 — se mudar, os frames deixam
de ser desempacotáveis e o sintoma é vídeo preto, não um erro); o **fail-closed**
nos dois sentidos; a ida e volta byte a byte; o código da sala como **sal** (sem
ele, a mesma frase-chave em duas reuniões daria a mesma chave, e gravar uma
serviria para abrir a outra); a frase errada a não decifrar; e o **header
autenticado** — mexer num byte do header em claro invalida o frame, que é a razão
de ele ir como `additionalData`.

**O defeito que só apareceu a sabotar.** A guarda de frame curto é
`data.byteLength <= offset`. Trocada por `<`, sobreviveu aos oito primeiros
testes: um keyframe de **exactamente** 10 bytes é só header, e com `<` sairia um
frame com ciphertext vazio, tag e IV — 28 bytes de nada. O teste usava 8 bytes,
que está do mesmo lado da fronteira nas duas versões. É o off-by-one de sempre, e
só se vê a atacar a condição.

**Não provado.** Isto não corre num Worker nem numa `RTCPeerConnection`. Prova as
decisões — que é onde os defeitos desta família vivem —, não a ligação aos
`RTCRtpSender`.

**Ficheiros.** `web/src/e2ee.test.ts`.
### R117 — Um código TOTP continuava a servir na janela seguinte à sua

**Sintoma.** O `web/e2e/mfa.mjs` falhou no CI com:

```
✗ replay entre operações
    HTTP 200 — o código da activação foi reaceite
```

O código usado para **activar** o MFA foi aceite outra vez, minutos depois, para
**iniciar sessão**. Não era uma falha intermitente do teste: era o produto a
dizer a verdade, e o teste a apanhá-la só quando o relógio ajudava.

**Causa.** O anti-replay guarda o último passo temporal usado (`last_step`) e
exige que o seguinte **avance**:

```sql
UPDATE user_mfa SET last_step = $2 WHERE user_id = $1
  AND (last_step IS NULL OR last_step < $2)
```

A barreira está certa. O que estava errado era **de onde vinha o `$2`**: do
relógio (`agora() / STEP_SECS`), e não do código.

O `SKEW_STEPS` aceita, de propósito, um código do passo N apresentado durante o
passo N±1 — é o que tolera relógios dessincronizados. Só que, apresentado durante
N+1, esse código registava `last_step = N+1`, e a comparação passava a ser
`N < N+1` → **verdadeiro**. O mesmo código servia duas vezes, com até trinta
segundos de folga sobre a sua própria janela.

Ou seja: o anti-replay funcionava **excepto** no caso que existe para impedir —
um código apanhado por cima do ombro e usado logo a seguir.

**Porque é que parecia intermitente.** O teste só falha quando as duas chamadas
caem em passos diferentes. Se caírem no mesmo, `N < N` é falso e a rejeição
acontece pela razão certa. É uma corrida contra a fronteira dos 30 segundos, e
por isso passou muitas vezes.

**Regra.** O passo vem do **código**, nunca do relógio. O `verifica` foi
**apagado**: a função devolvia um `bool`, e um `bool` obriga quem chama a
descobrir o passo por outro meio — que é exactamente como o defeito nasceu.
Ficou só o `passo_do_codigo`, que devolve `Option<i64>`. Apagar a forma que
causou o erro vale mais do que documentá-la.

**Tempo constante mantido.** Continuam a percorrer-se todos os passos e todos os
bytes; o passo encontrado acumula-se com uma **máscara** (`-(bate as i64)`) e não
com um `if`, para o tempo de resposta não revelar qual acertou. O `+1` interno
distingue «passo 0» — um instante real, 1970 — de «nenhum».

**Portão.** `o_passo_vem_do_codigo_e_nao_do_relogio`: o mesmo código apresentado
em N-1, N e N+1 tem de devolver **sempre N**. Provado vermelho a repor o passo do
relógio.

**Ficheiros.** `server/src/mfa.rs`.
### R114 — Entrar pelo telemóvel e pelo portátil dava um ciclo de eco

**Lacuna, e uma afirmação a corrigir.** A matriz competitiva dava o *companion
mode* como feito, «via QoS + múltiplos joins». Era outra maneira de dizer «entrar
duas vezes funciona» — e funcionava: as duas sessões entravam, ambas com
microfone e ambas com altifalante. No mesmo espaço físico isso é um ciclo de
realimentação, e o ruído não é problema de quem o causa: é de **toda a gente na
reunião**, que é o pior tipo de defeito de UX.

O companion mode existe porque é útil — o telemóvel serve de comando, de segunda
câmara, de vista da apresentação. O que não pode é o áudio duplicar.

**Quem decide.** O **servidor**. O cliente não tem como saber que a outra sessão
é dele: uma heurística no cliente («já vi este nome no roster») falharia com dois
homónimos e falharia sempre que alguém mudasse o nome. O `Hub::join` compara o
`user_id` **dentro do lock de escrita** — se fosse uma pergunta separada antes do
join, duas entradas simultâneas do mesmo utilizador podiam ambas ler «não está» e
entrar as duas com microfone.

**O `F5` não é um segundo dispositivo.** Um lugar reservado por queda de socket
(R91) tem `disconnected_at` e **não** conta: trancar o áudio a quem volta de uma
quebra de rede seria o oposto exacto do que o R91 foi resolver.

**Mudo nos dois sentidos.** Só calar o microfone não chega — o altifalante deste
dispositivo a tocar a reunião ao pé do microfone do outro fecha o ciclo na mesma.
O `<audio>` é **silenciado, não desmontado**: pela mesma razão do `AudioSink`, o
elemento fica ligado ao stream para que ligar o som seja instantâneo.

**E a pessoa fica a saber.** Um dispositivo mudo sem explicação é indistinguível
de um produto partido — e é essa a queixa que se recebe, nunca a causa. O aviso
fica no ecrã **até a pessoa decidir** (não é uma notificação que passa: é um
estado), e o botão «usar o áudio aqui» devolve-lhe a decisão, dizendo o que fazer
ao outro dispositivo.

**E desliga-se sozinho.** Uma funcionalidade que se liga sozinha e não se
desliga sozinha é meia funcionalidade: quem fechasse o portátil ficava com o
telemóvel mudo e um aviso a falar de um aparelho que já não está lá. Quando a
outra sessão sai, o servidor manda `CompanionEnded` — e **só quando resta uma**:
com três sessões, sair uma deixa duas, e duas ainda fazem eco. Essa distinção
entre «resta UMA» e «resta ALGUMA» foi encontrada a sabotar: com `>= 1` os testes
continuavam verdes, porque nenhum tinha três sessões. Tem-no agora.

**Portões.** Rust: `segunda_sessao_da_mesma_conta_entra_como_companion` e
`reentrar_depois_de_uma_queda_nao_e_companion`,
`quando_a_outra_sessao_sai_o_companion_termina` e
`com_tres_sessoes_sair_uma_nao_desliga_o_companion` (144 testes). Frontend:
`web/src/companion.invariantes.test.ts`, seis portões, e o `web/e2e/companion.mjs`
que entra duas vezes com a mesma conta pela interface real. Sabotado nos cinco sítios
que importam — ignorar o `disconnected_at`, nunca detectar, não calar o
microfone, não calar o altifalante, não passar a bandeira ao `AudioSink` — e
vermelho em todos.

**O e2e foi provado a vermelho, no CI, contra a stack a sério.** Com o cliente a
ignorar a bandeira do servidor (`if (false && m.companion)`), a corrida deu:

```
· telemóvel: {"aviso":false,"audios":1,"todosMudos":false}
✗ o telemóvel É AVISADO de que a conta já está na reunião
=== 2 FALHARAM ===
```

E com o código certo:

```
· telemóvel: {"aviso":true,"audios":1,"todosMudos":true}
✓ o telemóvel É AVISADO de que a conta já está na reunião
```

O `audios: 1` nos dois é o que impede o verde em vazio: sem roster não haveria
`<audio>` nenhum, e um `every` sobre lista vazia devolve `true`.

**Não provado.** Nada disto abre dois browsers com microfones reais. Prova-se a
decisão e o silenciamento; não se prova a ausência de eco numa sala com duas
máquinas — isso é uma verificação à mão e continua por fazer.

**Ficheiros.** `server/src/signaling.rs` (`Entrada`, `companion` no `Joined`),
`web/src/signaling.ts`, `web/src/pages/Room.tsx`, `web/src/styles.scss`,
`web/src/companion.invariantes.test.ts`, `web/src/locales/{pt,en,fr}.ts`,
`docs/competitive-positioning.md`.

### R119 — Um symlink para a minha máquina entrou na `main`

**Sintoma.** Quem clonasse o repositório ficava com

```
web/node_modules -> /tmp/wtp2/web/node_modules
```

um link pendurado para um caminho que não existe em máquina nenhuma além da
minha. Entrou pelo PR #54 e ficou lá durante cinco PRs.

**Causa, em duas metades.** A primeira sou eu: uso worktrees em `/tmp` e ligo o
`node_modules` de todos ao de um, para não instalar cinco vezes. Um `git add -A
web` levou o symlink junto com o trabalho.

A segunda é a que interessa a prazo: **o `.gitignore` tinha `web/node_modules/`,
com barra final.** A barra faz o padrão casar **só com um directório** — e um
symlink não é um directório. A linha que existia exactamente para impedir isto
não o impediu, e ninguém tinha razão para desconfiar dela.

**Porque é que o CI não deu por nada.** O `npm ci` substitui a pasta e segue. O
verde do CI não é prova de que uma checkout limpa funciona: o CI **repara** este
caso ao passar por ele.

**Regra.** Nenhum caminho versionado aponta para fora da árvore. A regra é geral
e não sobre `node_modules`: um symlink **relativo e interno** é legítimo; um
**absoluto**, ou um que **suba acima da raiz**, é a máquina de alguém a entrar no
repositório.

**Portão.** `scripts/check-repo-hygiene.sh`. O alvo lê-se do **índice**
(`git cat-file -p :caminho`) e não do `HEAD` — um symlink acabado de adicionar
ainda não está em commit nenhum, e era assim que ele entrava. O `cat` **não
serve**: segue o link e devolve vazio quando o alvo não existe, que é exactamente
o caso mau. A primeira versão deste portão falhou por isso e deu zero achados
nos dois testes de sabotagem.

Provado nos três casos: absoluto → vermelho, a subir → vermelho, relativo interno
→ passa.

**Ficheiros.** `.gitignore`, `scripts/check-repo-hygiene.sh`, e a remoção de
`web/node_modules` do índice.
### R116 — O directo tinha testes de contrato e nenhum de ciclo de vida

**Como apareceu.** A pôr o `src/studio/directo.ts` no arnês de mutação: **7 das 8
mutações sobreviviam**. A bateria ficava verde com o browser dado como capaz sem
saber H.264, com pedaços vazios a ir para a rede, com envios num socket fechado,
e com as três decisões de fase invertidas.

**Causa.** O `directo.test.ts` cobria o **contrato** — o codec (que é a decisão
inteira do ADR-0003) e a construção do URL. São os testes certos para o que
guardam, e não tocam no ciclo de vida. E é no ciclo de vida que este módulo falha
**em silêncio**: enviar num socket fechado atira dentro de um `then` sem `catch`,
e enviar um pedaço vazio é largura de banda a troco de nada.

**O que passou a estar defendido**, cada um com o seu porquê:

- **`MediaRecorder` sem H.264** (Firefox) tem de recusar. Deixá-lo arrancar dava
  um directo que o servidor teria de reencodificar — a decisão que o ADR-0003
  recusou.
- **Pedaço vazio** não vai para a rede, e não conta bytes.
- **Socket já fechado**: nem envio nem contagem.
- **Socket que fecha ENTRE o pedaço e o `arrayBuffer()`** — o `arrayBuffer` é
  assíncrono, e é por isso que a guarda é dupla. Sem a segunda, o `send` atira.
- **`parar()` fecha com 1000** e volta a «parado».
- **O socket a cair leva a «erro», não a «parado»** — a distinção é o que a
  interface mostra: «parado» foi decisão da pessoa, «erro» é uma emissão que caiu
  e que ela tem de saber que caiu.
- **Um fecho tardio depois de `parar()` não põe «erro» no ecrã** — o `parar()`
  fecha o socket e o `onclose` chega a seguir.
- **`parar()` sem nunca ter começado não atira** — foi o último sobrevivente: sem
  o `g &&`, lê `.state` de `null` dentro de um `onClick`, onde um erro não
  tratado passa despercebido até alguém abrir a consola.

Nada disto precisa de rede nem de câmara: o `MediaRecorder` e o `WebSocket` são
substituídos por duplos com a mesma forma. O que se prova são as decisões.

**Portão.** `src/studio/directo.ts` entrou nos alvos do `scripts/mutantes.mjs`.
**8 mutações, 8 mortas.**

**Ficheiros.** `web/src/studio/directo.test.ts`, `scripts/mutantes.mjs`.

### R118 — O único teste de media que ignorava o `E2E_TIMEOUT_FACTOR`

**Sintoma.** Três corridas seguidas do CI a falhar no `web/e2e/tempos.mjs`, num
PR que só mexia em testes do estúdio e no catálogo:

```
✗ os tempos NÃO ficaram prontos em 90000 ms
✗ join_ms medido: null ms
```

A leitura fácil — «o produto não liga» — estava errada, e o próprio relatório
tinha a resposta uma linha acima:

```
{"join_ms":null,"ws_ms":30,"ice_gathering_ms":74840,
 "first_audio_ms":185,"first_video_ms":215,"ice_restarts":2} (esperou 90000 ms)
```

**A media chegou**: áudio a 185 ms, vídeo a 215 ms. O que estourou foi a recolha
de candidatos ICE — **74 840 ms**, contra os ~377 ms de uma máquina normal. É o
esfomeamento do R65 outra vez, duas ordens de grandeza pior.

**Causa.** O CI declara `E2E_TIMEOUT_FACTOR=4` precisamente porque sabe que o
runner é lento, e escreve porquê: *«um portão que falha ao acaso perde a
credibilidade toda»*. O `tempos.mjs` era **o único teste do trabalho a
ignorá-lo** — o `estudio.mjs`, ao lado no mesmo job, honra-o.

**Regra.** Quem espera por uma ligação de media lê o factor do ambiente. E a
mensagem de esgotamento passa a **distinguir as duas causas**: se a recolha de
ICE passou dos 10 s e a media chegou, diz-se que foi a máquina a esfomear o
agente — não o produto. Sem essa linha, três falhas leram-se como avaria.

**Portão.** `web/src/e2eFator.invariantes.test.ts`.

**E o portão falhou à primeira, pela razão mais instrutiva.** A versão 1
procurava a palavra `E2E_TIMEOUT_FACTOR` no ficheiro — e **sobreviveu** a tirar o
factor do `tempos.mjs`, porque o comentário logo acima continuava a mencioná-lo.
Media a presença de uma palavra, não o comportamento. A versão 2 tira os
comentários primeiro e exige a **leitura do ambiente**
(`process.env.E2E_TIMEOUT_FACTOR`), não a menção. É exactamente a falha que este
portão existe para impedir, cometida ao escrevê-lo.

**Ficheiros.** `web/e2e/tempos.mjs`, `web/src/e2eFator.invariantes.test.ts`.

### R120 — Um AudioWorklet pequeno de mais parte em produção, e funciona em dev

**Sintoma.** Nenhum, em desenvolvimento — e é esse o perigo. Um `import('./x.js?url')` para um módulo de AudioWorklet novo (o noise gate a seguir ao RNNoise) resolvia para um `data:text/javascript;base64,...`, e `audioContext.audioWorklet.addModule(url)` engolia isso sem se queixar, no dev server.

**Causa raiz.** O Vite inlinha em `data:` qualquer asset `?url` abaixo de 4 KB por omissão (`assetsInlineLimit`). O ficheiro do gate tem 3131 bytes — abaixo do limiar. O `rnnoiseWorklet.js` (do pacote `@sapphi-red/web-noise-suppressor`) nunca tinha mostrado este problema só por ser maior, não por o padrão de import estar certo. Em produção, o CSP (`deploy/nginx-delonix.conf`) declara `worker-src 'self' blob:` e `script-src 'self' 'wasm-unsafe-eval'` — **sem `data:`** — e um browser que respeite CSP recusa carregar o worklet a partir desse URL. O `addModule()` falha, a promise rejeita, e sem um `.catch()` a apanhar especificamente isto o utilizador fica sem o gate e sem aviso nenhum — o RNNoise continua a funcionar (é um ficheiro maior, nunca inlinado), por isso a chamada não fica muda; só perde a etapa nova, em silêncio.

**Regra.** Um módulo de AudioWorklet (ou Worker) importado via `?url` precisa de ficar **sempre** como ficheiro à parte, nunca inline — independentemente do tamanho. Configurou-se `build.assetsInlineLimit` como função em `vite.config.ts` a excluir qualquer `*Worklet.js` da inlining. **Não chega testar em dev**: o dev server não aplica o CSP de produção, por isso o sintoma só existe atrás do nginx real — exactamente o gap que o R73 (`Vary` e o service worker) já tinha ensinado desta app.

**Como se apanhou.** Por inspecionar o `dist/` a olho depois do build (`head -c 300` no ficheiro emitido) em vez de confiar em "o build passou e os testes ficaram verdes" — nenhum teste automático desta app corre atrás de um nginx com CSP real.

**Ficheiros.** `web/vite.config.ts`, `web/src/noiseGateWorklet.js`.

### R121 — Três regras de acesso escritas em mais de um sítio, e a cópia que decidia estava errada

**Sintoma.** Nenhum para quem usa o produto como deve — e é o pior tipo. Provado ao vivo (2026-09-16, contra servidor e Postgres reais, com o `isolamento.mjs` escrito ANTES da correcção, 8 falhas):

- **S1.** Qualquer pessoa que se registasse lia e reescrevia o armazenamento das gravações de TODA a plataforma, e o `/platform/storage/test` punha o servidor a fazer `PROPFIND` a um URL à escolha dela (SSRF — o teste antigo contava o `400` da ligação falhada como «recusa»).
- **S2.** Uma org com uma chave `dlx_` listava no «directório Odoo» o email do admin de OUTRA org: a conta era renomeada, marcada como gerida, e entrava na org do atacante como admin.
- **S3.** Um funcionário arquivado continuava a ler o chat das salas, a encontrar colegas na pesquisa, e (se admin) a descarregar gravações da ex-empresa; a chave da org criava reuniões com ele como anfitrião.

**Causa raiz.** A mesma nas três: a regra existia CERTA num sítio e ERRADA numa cópia.
- S1: «admin da plataforma» = «admin de qualquer org» (`storage.rs`), quando o `register` cria SEMPRE um admin.
- S2: `odoo::provision` tinha o seu próprio «liga por email», sem a guarda de autoridade que `odoo_sso::upsert_member` já tinha desde a R25.
- S3: 17 verificações de pertença escritas à mão sem `archived_at IS NULL`, que `org::role_in_org` e `org::org_co_members` já filtravam.

**Regra.**
- Administrador da plataforma é uma lista EXPLÍCITA de UUIDs (`PLATFORM_ADMIN_USER_IDS`), nunca derivada de `org_members`. UUID e não email: sem verificação de email, um endereço declarado antes de a conta existir podia ser registado por outro. Falta de papel é `403` (`ApiError::Forbidden`), não `401` — o web lê `401` como sessão caducada.
- Uma sincronização de directório passa SEMPRE por `upsert_member`. A cópia saiu em vez de ser remendada. Contas recusadas vão em `skipped` com a razão, sem falhar o lote.
- «Colega» e «admin que pede» são membros ACTIVOS. O SUJEITO não se filtra quando o dado é da organização: a gravação de quem saiu continua a ser descarregável pelo admin activo (retenção, eDiscovery); a auditoria e a retenção continuam a contar quem saiu.

**Armadilha que o controlo positivo apanhou.** `/api/users/search` e `recordings::shares` devolviam SEMPRE `500` («no column found for name: locale» — o SQL de runtime não verifica colunas na compilação). A asserção «a arquivada já não encontra ninguém» passava ANTES da correcção — não por estar certa, mas porque a rota rebentava. Sem o «antes» positivo, o teste mediria uma avaria.

**Portão.** `web/e2e/isolamento.mjs` (secções S1–S3, com controlo positivo antes de cada recusa); `storage::tests`; a metade positiva do S1 (utilizador declarado → `200`) foi verificada ao vivo com o servidor reiniciado com a variável, e não está automatizada — o utilizador do teste só nasce depois do arranque.

**Ficheiros.** `server/src/{storage,config,error,odoo,rooms,users,recordings,meetings_v1}.rs`, `web/src/pages/Analytics.tsx`, `web/e2e/isolamento.mjs`, `docs/deployment.md`, `deploy/delonix.env.example`.

### R122 — `add_employee` capturava uma conta de outra organização (a 4.ª cópia da mesma regra)

**Sintoma.** Nenhum para a vítima. O admin de uma organização **legada** (com `email_domain` vazio) chamava `POST /api/orgs/{org}/employees` com o email de alguém de outra empresa e, com `role: "admin"`, tornava-se colega dessa pessoa: via as salas dela (`room_access` conta `org_mate`), encontrava-a na pesquisa, e podia ligar-lhe. Provado ao vivo a 2026-09-16 — a vítima entrava na org do atacante com `role=admin`.

**Causa raiz.** A regra «tornar-me colega de alguém» estava escrita em quatro sítios, e a auditoria de 2026-09-16 (R121) só fechou três. `add_employee` liga uma conta EXISTENTE por email e tinha a sua própria noção de fronteira: o `email_domain` da org. Mas o `email_domain` é `''` nas organizações anteriores à migração 0010 (e nunca é editável pela API), e nesse caso a verificação de domínio é **saltada por inteiro** — não havia segunda barreira. É a mesma classe da S2: saber o email de alguém não pode puxá-lo para o nosso inquilino.

**Regra.** A mesma `ForeignOrg` do `meetings_v1::resolve_org_user` e da R25: `add_employee` recusa (`409`) uma conta que já seja membro ACTIVO de outra org. Re-adicionar alguém que já é membro DESTA org continua a funcionar (mudar papel/filial) — a guarda é só o *outro* org. Ligar uma conta a uma segunda organização é acto do dono, não efeito de um admin escrever o email dela.

**Porque não foi apanhado por um teste black-box.** Os domínios são únicos por org (índice de 0010), por isso, num sistema novo, A nunca pode adicionar um email do domínio de B — a verificação de domínio trata disso, com ou sem esta correcção. Um teste ao nível da API passaria nos dois estados, e um teste que passa com e sem a correcção é um falso portão (R51/R94). A guarda nova só é alcançável na org legada, que a API não cria: o teste ataca a base directamente (esvazia o `email_domain` por SQL), como a auditoria. Verificado a falhar no binário SEM a correcção e a passar COM ela.

**Portão.** `web/e2e/captura-empregado.mjs` (ataque directo à base, no job `isolamento` do CI).

**Ficheiros.** `server/src/org.rs` (`add_employee`), `web/e2e/captura-empregado.mjs`, `.github/workflows/ci.yml`.

### R126 — Uma emissão parada parava as do nó, e um destino mau parava os outros

**Sintoma.** Dois, ambos medidos (auditoria de 2026-09-16, problemas 1 e 2):

- Um ffmpeg que escrevesse muito para o stderr (um destino a recusar e a
  repetir o erro) ou que deixasse de ler o stdin (rede parada) pendurava a
  escrita da sua emissão — e, como `Registo::escrever` escrevia **com o lock do
  registo preso**, pendurava também a emissão de qualquer outra sala do mesmo
  nó. Reproduzido antes da correcção: o teste
  `uma_emissao_parada_nao_bloqueia_outra_sala` falhava contra `7f02f00` com «a
  sala B ficou bloqueada pela emissão parada da sala A».
- Um só ffmpeg com N saídas `-f flv`: a primeira saída que falhasse terminava o
  processo e levava todos os destinos com ela.

**Causa raiz.** `stderr(Stdio::piped())` sem leitor (o cano enche aos 64 KiB e
o processo bloqueia a escrever nele), um `await` sobre I/O de um filho dentro de
uma secção crítica partilhada pelo nó, e a falha de uma saída tratada como falha
do processo inteiro.

**Regra.**
- **Nenhum `Stdio::piped()` sem quem o leia até ao fim.** O stderr do ffmpeg é
  drenado sempre (e é daí que sai o `motivo`), com linhas cortadas e contadas.
- **Nenhum `await` sobre I/O de um processo filho com um lock partilhado
  preso.** O repartidor (`Emissao::escrever`) é síncrono e só faz `send` em
  filas com orçamento em bytes; só a tarefa de escrita de cada destino espera
  pelo stdin desse destino. Um destino que não acompanha é morto e reiniciado.
- **Um processo por destino**, com supervisor e backoff limitado. Um processo
  que não chega ao ar em `connect_timeout` conta como queda — sem isto um
  ffmpeg à espera de media ficava «a ligar» para sempre.
- **Reentrar a meio de um fluxo Matroska exige o cabeçalho e um Cluster.** O
  cabeçalho guarda-se do início; o ponto de entrada é um id de Cluster com
  tamanho EBML válido e o Timestamp como primeiro filho — **ou um CRC-32 e
  depois o Timestamp**.

**Armadilha que só o RTMP real apanhou.** A primeira versão do detector de
Cluster só aceitava o Timestamp logo a seguir ao tamanho — é o que o
MediaRecorder do Chromium escreve, e os testes unitários passavam. Contra o
mediamtx, com media gerada pelo ffmpeg (que põe um CRC-32 primeiro), nenhum
Cluster era reconhecido: o destino válido ficava «a ligar» para sempre depois
de o servidor RTMP reiniciar, e o inválido nunca voltava a falhar (ficava à
espera de input), por isso nunca contava tentativas. Duas correcções: aceitar o
CRC-32, e a vigia de arranque, que transforma «à espera para sempre» em queda
contada. O teste real corre agora nos dois formatos (`SEM_CRC=1`).

**Portão.** Testes em `server/src/broadcast.rs` (ffmpeg falsos: entupido no
stderr, surdo no stdin, a recusar a ligação, lento com reinício e Cluster
partido entre pedaços, mudo); `web/e2e/directo-destinos.mjs` contra um RTMP real
(fora do CI, razão em `scripts/e2e-fora-do-ci.txt`).

**Ficheiros.** `server/src/broadcast.rs`, `docs/adr/0003-directo-para-plataformas.md`,
`web/e2e/directo-destinos.mjs`.

### R123 — O portão de autorização não via o segundo handler de uma rota

**Sintoma.** Nenhum visível, e é isso o problema. `scripts/check-route-auth.sh` dava verde com uma rota como `.route("/api/users/me", get(users::me).patch(<handler sem autenticação>))`. Medido a 2026-09-16 com o controlo negativo: trocar `update_me` por um handler público → portão antigo **verde**, portão corrigido **vermelho**.

**Causa raiz.** O corpo de cada `.route(…)` lia-se com uma regex preguiçosa, `\.route\(\s*"…"\s*,(.*?)\)\s*(?=[,.\n])`, que pára no primeiro `)` seguido de `.`. Em `get(a).patch(b)` o corpo capturado era só `get(a`: o `b` encadeado nunca era inspeccionado. Eram **26 handlers** fora do portão — todos os `PATCH`/`PUT`/`DELETE`/`POST` escritos a seguir a um `get(…)` (`update_me`, `webhooks::create`, `sso` PUT/DELETE, `recordings` link, …). Nenhum estava de facto sem autenticação; nenhum estava provado.

**Regra.** Um portão que lê código lê-o por estrutura, não por regex preguiçosa: o corpo de `.route(` é o texto entre parêntesis EQUILIBRADOS. E um portão novo nasce com o controlo negativo do caso que o originou (R51/R94) — aqui, o handler público encadeado.

**Portão.** `scripts/check-route-auth.sh` (parser equilibrado); o `scripts/check-openapi.sh` usa o mesmo, e foi ao contar operações que a diferença apareceu (94 contadas pela regex vs 120 montadas).

**Ficheiros.** `scripts/check-route-auth.sh`, `scripts/check-openapi.sh`.

### R124 — «Permitir admissão» enviava uma mensagem que o servidor recusava

**Sintoma.** O anfitrião carregava no escudo ao lado de um participante («permitir admissão»), o crachá não mudava e o participante nunca via a sala de espera. No socket do anfitrião chegava `{"type":"error","message":"invalid message"}`.

**Causa raiz.** Uma funcionalidade a meio, nas duas pontas. O web enviava `promote-admit` e esperava `admit-role`/`peer-role` (`web/src/signaling.ts`), mas o `ClientMsg` do servidor não tinha a variante: a desserialização falhava. Do lado de dentro também faltava metade: a tabela `room_admitters` (0017) era LIDA no token (`adm` → `can_admit`) mas nunca ESCRITA (`rooms::set_room_admitter` sem chamadores), e o `can_admit` não autorizava nada — `decide_waiting` só aceitava o anfitrião e a sala de espera só ia para anfitriões.

**Regra.** Uma mensagem do protocolo tem as duas pontas no mesmo commit, e um teste de formato (`promote_admit_wire_format`) prova que a forma que o web envia desserializa. O papel muda em memória no hub (síncrono, sob o lock); a persistência é IO e corre FORA do lock, no loop do socket. A sala de espera vai para quem PODE admitir (`broadcast_admitters`, com evento Redis próprio), e quem volta com o papel persistido é avisado (`admit-role`) — o cliente só assume esse poder para o anfitrião.

**Portão.** `signaling::tests::{host_promotes_co_admitter_who_can_then_admit, persisted_co_admitter_is_told_its_role_on_join, promote_admit_wire_format}`. Não verificado em browser nem a persistência ponta-a-ponta por WebSocket.

**Ficheiros.** `server/src/{signaling,pubsub,lib}.rs`.

### R125 — `PATCH /api/action-items/{id}` vazio devolvia o item a qualquer conta

**Sintoma.** Nenhum para a vítima. Uma conta autenticada de OUTRA organização que soubesse o id de um item do plano de acção (5W2H) fazia `PATCH /api/action-items/{id}` com `{}` e recebia `200` com o item inteiro: o quê, porquê, quem, recursos. Provado ao vivo a 2026-09-16 contra Postgres real (`tests/security.rs`, que falhou com `200` antes da correcção).

**Causa raiz.** A autorização dependia do CONTEÚDO do pedido: os campos de edição exigiam o anfitrião, e o `status` exigia ser membro — mas um pedido sem nenhum dos dois não passava por verificação nenhuma e seguia para o `SELECT` final, que devolve o item. Encontrado ao documentar o handler para o OpenAPI (ADR-0006 §3), não por teste.

**Regra.** A verificação de acesso ao RECURSO vem primeiro e é incondicional; o que o pedido quer alterar só pode ACRESCENTAR exigências (anfitrião para editar), nunca decidir se há verificação. E valida-se antes de escrever: no `patch_agenda_item` vizinho, um tópico inválido dava `400` depois de o `done` já estar gravado.

**Portão.** `server/tests/security.rs::action_item_patch_does_not_leak_to_other_org` (controlo positivo: o dono lê o item pelo mesmo PATCH vazio).

**Ficheiros.** `server/src/actions.rs`, `server/tests/security.rs`.

### R150 — Colaborador adicionado sem password nascia com `changeme123`

**Sintoma.** Nenhum para a vítima. `POST /api/orgs/{org}/employees` sem `password` criava a conta com a password FIXA `changeme123`. Quem soubesse o email de um colaborador recém-adicionado entrava como ele até à primeira mudança de password. Provado a 2026-09-16 contra Postgres real (`login` com `changeme123` → `200` com sessão).

**Causa raiz.** Um valor por omissão escrito como conveniência (`unwrap_or("changeme123")`) numa credencial. A validação de password corria sobre ele e passava — tem 11 caracteres.

**Regra.** Nenhuma credencial tem valor por omissão conhecido. Sem password indicada gera-se uma aleatória (`core::crypto::random_hex`), devolvida UMA vez ao admin em `temporary_password` para a entregar; com password indicada, o campo não aparece.

**Portão.** `server/tests/security.rs::added_employee_without_password_does_not_get_a_known_password`.

**Ficheiros.** `server/src/org.rs`.

### R151 — Uma chave de API ocupava contas de outro domínio pela v1

**Sintoma.** `POST /api/v1/meetings` da org A com `host_email: ninguem@beta.test` (domínio da org B) criava a conta como membro da A. Quando a B tentava adicionar a pessoa, recebia `409` («já pertence a outra organização», R122) — a identidade ficava presa na A. Os convidados desconhecidos tinham o mesmo efeito.

**Causa raiz.** `meetings_v1::resolve_org_user` recusava contas de OUTRA org (`ForeignOrg`) mas criava as que não existiam — e juntava contas órfãs — sem olhar para o domínio da organização, que é a fronteira que o registo e o `add_employee` já impõem.

**Regra.** Criar ou juntar uma conta por email só dentro do domínio da organização (`organizations.email_domain`; numa org legada sem domínio não há regra a aplicar). Anfitrião fora do domínio → `422 meeting.host_outside_org_domain`; convidado → `skipped` com a razão.

**Portão.** `server/tests/api_v1.rs::v1_meeting_refuses_to_create_accounts_outside_org_domain` (controlo positivo: anfitrião novo do próprio domínio continua a nascer; a org dona do domínio adiciona a pessoa sem conflito).

**Ficheiros.** `server/src/meetings_v1.rs`.

### R152 — `GET /api/orgs` mostrava a org a um membro arquivado

**Sintoma.** Um colaborador arquivado deixava de alcançar as rotas da organização (S3) mas continuava a vê-la em `GET /api/orgs`, com o papel antigo, e o `member_count` contava os arquivados.

**Causa raiz.** A 18.ª verificação de pertença escrita à mão sem `archived_at IS NULL` — a mesma classe da S3, num `JOIN` que a auditoria não apanhou por estar dentro de `org.rs`.

**Regra.** A da S3: «membro» é membro ACTIVO, também nas listagens e contagens.

**Portão.** `server/tests/organization.rs::my_orgs_hides_org_from_archived_member`.

**Ficheiros.** `server/src/org.rs`.

### R153 — Falta de permissão respondia 401, e o web renovava a sessão por nada

**Sintoma.** Um membro sem papel de admin (ou um participante sem acesso a uma sala, ou um convidado que não é anfitrião) recebia `401`. O `web/src/api.ts` lê `401` como «a sessão caducou»: chamava `/api/auth/refresh`, repetia o pedido, levava outro `401` e só então mostrava o erro — dois pedidos a mais por clique, e um erro que dizia «sessão» quando o problema era papel.

**Causa raiz.** `org::require_admin`, o acesso a sala em `rooms.rs`, as guardas de `whiteboards.rs` e `actions.rs` usavam `ApiError::Unauthorized` para falta de PERMISSÃO. O `error.rs` já tinha `Forbidden` com o comentário a explicar exactamente isto; faltava usá-lo.

**Regra.** `401` é só «não sei quem és». Sem o papel: `403`. Recurso de outra organização ou reunião de que não és membro: `404` — não se confirma que existe. 22 asserções dos testes de caracterização mudaram com intenção (18× `401→403`, 3× `401→404`), e o OpenAPI descreve os três casos em separado.

**Ficheiros.** `server/src/{org,rooms,whiteboards,actions}.rs`, `server/tests/{content,organization,scheduling}.rs`. Por fazer: as mesmas guardas em `meetings.rs` e `recordings.rs` (esta última foi reescrita no G4–G6 com 403/404).
### R130 — O SSO de uma organização abria sessão em contas de OUTRA organização

**Sintoma.** Nenhum para a vítima. O administrador de uma organização configura o IdP OIDC dela (`PUT /api/orgs/{id}/sso`) — e portanto controla o email que esse IdP afirma. Bastava o IdP devolver `admin@outra-org` para o `/api/auth/sso/callback` responder `302` com uma sessão da vítima. O mesmo callback criava contas de QUALQUER domínio e juntava-as à org, e reabria a porta a membros arquivados. Provado a 2026-09-16 contra Postgres real e um IdP OIDC falso (discovery, JWKS, id_token RS256): antes da correcção, `left: (302, Some("<id da vítima>"))`.

**Causa raiz.** O callback tratava o email do id_token como prova de pertença: `SELECT … FROM users WHERE email = $1` e, se existisse, abria sessão; se não, criava e juntava. A assinatura do id_token prova só que o IdP da org o disse — e esse IdP é escolhido por quem administra a org.

**Regra.** Família R25/R122, na forma mais restritiva (`auth::sso_login_decision`): o SSO da org X só (a) abre sessão numa conta que seja membro ACTIVO de X, ou (b) cria conta nova se o domínio do email for o `email_domain` (não vazio) de X. Conta existente fora de X → `403 sso.account_not_in_org`; conta nova de outro domínio → `403 sso.email_domain_mismatch`. Nunca se junta uma conta existente à org pelo SSO. A recusa fica na auditoria (`auth.sso_refused`).

**Portão.** `server/tests/security_identity.rs::{sso_callback_refuses_account_of_another_org, sso_jit_only_creates_accounts_of_the_org_domain, sso_refuses_archived_member}` (controlo positivo em cada: o membro activo entra, o JIT do próprio domínio cria), e `auth::tests::sso_login_decision_is_the_most_restrictive_rule`. Não validado contra um IdP real (Google, Entra, Okta); o `email_verified` do id_token continua sem ser lido.

**Ficheiros.** `server/src/auth.rs`, `server/tests/security_identity.rs`, `server/Cargo.toml` (`rsa` em dev-dependencies, para a chave do IdP falso gerada em memória).

### R131 — A activação e a desactivação do MFA aceitavam tentativas ilimitadas

**Sintoma.** Nenhum para a vítima. Com uma sessão roubada, `POST /api/users/me/mfa/disable` aceitava quantos códigos errados o atacante quisesse — seis dígitos adivinham-se, e acertar desliga o segundo factor. O `activate` tinha o mesmo oráculo sem travão. Provado a 2026-09-16 contra Postgres real: a sexta tentativa errada devolvia `left: 401, right: 429`.

**Causa raiz.** O passo MFA do LOGIN (`/api/auth/mfa`) tinha travão por conta desde o início (`login_limiter`, chave `mfa:{user}`); os dois endpoints da sessão, escritos depois, não o herdaram. Não havia teste que contasse tentativas fora do login.

**Regra.** Todo o endpoint que verifica um segredo curto (código MFA, PIN) tem travão por conta, e o travão pergunta ANTES de verificar (`RateLimiter::is_blocked`) — senão o código certo passa durante o bloqueio e o travão só atrasa quem adivinha. Só as falhas contam (`check` depois da falha, como o `voice_pin_limiter`): quem acerta à primeira nunca gasta tentativas. `mfa_limiter`: 5 falhas em 5 min, partilhado entre activar e desactivar → `429` com `Retry-After`.

**Portão.** `server/tests/security_identity.rs::{mfa_activate_locks_after_five_failures, mfa_disable_locks_after_five_failures}` (controlo positivo: noutra conta, 4 falhas não bloqueiam e o código certo activa; o código de recuperação desactiva), `mfa_login_step_is_limited_per_account` (guarda do travão que já existia), e `rate_limit::tests::is_blocked_*`. O limitador é em memória por pod: com N réplicas o orçamento é N×5 — o mesmo limite dos outros travões (`rate_limit.rs`).

**Ficheiros.** `server/src/{mfa,rate_limit,lib}.rs`, `server/tests/security_identity.rs`, `HARNESS.md`.

### R132 — Contas de domínio com SSO exclusivo não tinham travão por conta no login

**Sintoma.** Nenhum visível. `POST /api/auth/login` para uma conta cujo domínio exige SSO respondia sempre `400` («exige login via SSO») — antes do travão por conta, por isso nunca `429`. Provado a 2026-09-16 contra Postgres real: dez tentativas seguidas davam `left: 400, right: 429`.

**Causa raiz.** A ordem das verificações no `auth::login`: o `is_sso_enforced` corria primeiro e respondia sem passar pelo `login_limiter`. O risco medido é baixo — a recusa depende do DOMÍNIO, não da conta, e o `/api/auth/sso/check` já diz publicamente que o domínio exige SSO; não há password a adivinhar por aqui. Mas é uma resposta sem travão num endpoint de credenciais, e a próxima verificação específica que alguém lá puser herdava o mesmo defeito.

**Regra.** No login, o travão por conta é a PRIMEIRA coisa que responde; nenhuma resposta dependente da conta ou do domínio sai antes dele.

**Portão.** `server/tests/security_identity.rs::login_rate_limit_applies_to_sso_enforced_accounts` (controlo positivo: a recusa `400` do SSO exclusivo continua a ser dita até ao limite).

**Ficheiros.** `server/src/auth.rs`, `server/tests/security_identity.rs`.
### R140 — Dial-in PSTN ligado à sala de conferência de OUTRA organização

**Sintoma.** Nenhum para a vítima. O admin (ou qualquer membro) da org A fazia `POST /api/voice/rooms` com o `room_code` de uma sala da org B e recebia `200` com um PIN e um número de dial-in da SUA org. Quem ligasse para esse número com esse PIN era validado pelo IVR (`/api/voice/ivr/validate` e o gRPC `IvrService.ValidatePin`, que partilham `voice::validate_pin`) e posto dentro da reunião de B. Provado a 2026-09-16 contra Postgres real: `tests/security_voice_odoo.rs` falhou com `devolveu 200: {"dial_in_number":"+244222100001",…,"pin":"197966","room_code":"ifa-mrjw-nei"}` antes da correcção.

**Causa raiz.** `create_room` normalizava o código e gravava-o sem o procurar em `rooms` — a própria documentação do handler dizia «NÃO é verificado contra as salas». A fronteira multi-tenant do módulo era o par (DID, PIN), mas o ALVO desse par era texto livre escolhido por quem pede.

**Regra.** A sala de voz só se liga a uma sala cujo DONO é membro ACTIVO da organização de quem pede (`org::role_in_org`, sem `org_members` novo em `voice.rs`). Das regras do `rooms::room_access` é a mais restritiva: convite na agenda e co-anfitrião dão acesso a uma PESSOA, não tornam a sala num recurso da org. Inexistente e alheia dão a mesma resposta, `404` `voice.room_not_found` — não se revela que o código existe.

**Portão.** `server/tests/security_voice_odoo.rs::voice_room_for_another_orgs_room_code_is_refused` (controlo positivo: B liga a sua sala e o IVR HTTP devolve-a; A continua a ligar a sua). O caminho gRPC não é testado de novo: a correcção está na criação, a montante das duas validações. Não verificado com FreeSWITCH nem chamada PSTN real.

**Ficheiros.** `server/src/voice.rs`, `server/tests/security_voice_odoo.rs`, `docs/reference/openapi/bff.json`.

### R141 — Qualquer membro encerrava a sala de voz de outro, e um admin de org escrevia no pool partilhado de DIDs

**Sintoma.** (1) `POST /api/voice/rooms/{id}/close` dava `200` a qualquer membro da org dona: um colega cortava a chamada PSTN de todos os participantes da sala de voz de outra pessoa. (2) `POST /api/orgs/{org}/voice/dids` com `{"e164": …}` (modelo `shared` por omissão, sem `org_scoped`) gravava o número com `org_id = NULL` — o POOL PARTILHADO que `create_room` usa para o dial-in de TODAS as organizações. Qualquer conta que se registe é admin da sua org, portanto qualquer pessoa injectava números no dial-in dos outros. Provado a 2026-09-16 contra Postgres real: `um membro qualquer encerrou a sala de voz: {"ok":true}` e `um admin de org escreveu no pool partilhado: {…,"org_id":null,…}`.

**Causa raiz.** O fecho só perguntava «é membro da org?» (a documentação do handler dizia-o por escrito), e o inventário de DIDs confundia «admin da org» com «dono da plataforma» — a mesma confusão que a S1 do R121 fechou no armazenamento.

**Regra.** Encerrar uma sala de voz é do CRIADOR ou de um admin da org dona (`org::role_in_org`); outro membro recebe `403` `voice.room_close_forbidden`, e quem não é membro continua a receber `404`. Escrever no pool partilhado exige o administrador da PLATAFORMA (`storage::require_platform_admin`, agora `pub(crate)` em vez de copiado); o admin de org recebe `403` `voice.shared_did_requires_platform_admin` e cria DIDs só da sua org (`org_scoped: true` ou `model: dedicated`). A recusa corre antes de escrever. Sem consumidor no web (nenhum ecrã chama estas rotas), por isso a mudança do omisso não parte nada visível.

**Não fechado.** Um admin de org continua a poder registar QUALQUER número +E.164 para a sua org — não há prova de posse do número (exigiria o fornecedor SIP). O efeito fica confinado à sua org, mas ocupa o número (índice único) e o `409` revela que um número já está no inventário.

**Portão.** `server/tests/security_voice_odoo.rs::{voice_room_close_requires_creator_or_org_admin, shared_did_pool_requires_platform_admin}` (controlos positivos: a criadora e o admin encerram; o admin de org cria DIDs da sua org; o administrador da plataforma escreve no pool).

**Ficheiros.** `server/src/{voice,storage}.rs`, `server/tests/security_voice_odoo.rs`, `docs/reference/openapi/bff.json`.

### R142 — A chave de API do inquilino (`dlx_`) abria as rotas da integração Odoo

**Sintoma.** `GET /api/v1/integration/odoo/users` e `POST /api/v1/integration/odoo/provision` aceitavam, além do token de integração `dlxo_`, a chave de API `dlx_` da organização. Uma chave emitida para ler salas e reuniões (`/api/v1/org`, `/rooms`, `/meetings`) listava o directório de membros e provisionava contas e papéis — incluindo com a integração Odoo DESACTIVADA, porque o ramo `dlx_` não olhava para `odoo_enabled`. Foi este o vector da S2 (R121). Provado a 2026-09-16 contra Postgres real: `dlx_ lista o directório Odoo: devia ser recusado e devolveu 200: [{"email":"admin@zeta-odoo.ao",…,"role":"admin"}]`.

**Causa raiz.** O `OdooTokenAuth` tinha dois ramos, e o segundo justificava-se por um fluxo («a auto-provisão via `/admin/orgs` gera uma `dlx_` que o módulo usa directamente») que o módulo não segue: medido a 2026-09-16 em `kaeso-18/nokubiko/nk_delonix_meet` (e nas outras árvores do módulo no workspace), a `dlx_` só é usada em `/api/v1/admin/orgs` e `/api/v1/meetings`; nenhuma chama `/integration/odoo/*`. Duas credenciais com públicos diferentes (inquilino vs integração) numa mesma porta.

**Decisão de compatibilidade — explícita.** A descrição OpenAPI das duas rotas DOCUMENTAVA a `dlx_` como aceite («a chave `dlx_` da organização também é aceite»), embora o `api-contract.md` e o `HARNESS.md` já dissessem `dlxo_`. A aceitação é retirada sem período de transição nem flag: um integrador que siga a descrição antiga passa a receber `401` e tem de emitir o token em `POST /api/orgs/{org}/integration/odoo/token`. Não se encontrou nenhum consumidor real; se aparecer, a correcção é do lado dele, não reabrir a porta.

**Regra.** O extractor de uma superfície aceita a credencial DESSA superfície e mais nenhuma. `OdooTokenAuth` recusa (`401`) tudo o que não seja `dlxo_`, antes de consultar a base.

**Portão.** `server/tests/security_voice_odoo.rs::odoo_integration_routes_refuse_tenant_api_key` (controlos positivos: a mesma `dlx_` abre `/api/v1/org`; o `dlxo_` abre as duas rotas). `web/e2e/isolamento.mjs` S2 passa a atacar com o `dlxo_` — com a `dlx_` o ataque já nem chegava ao `upsert_member`.

**Ficheiros.** `server/src/odoo.rs`, `server/tests/{security_voice_odoo,api_v1}.rs` (o `odoo_provision_does_not_capture_accounts` passa a autenticar com `dlxo_`), `web/e2e/isolamento.mjs`, `docs/reference/openapi/v1.json`.

### R143 — O directório do Odoo recebia membros ARQUIVADOS como se ainda estivessem na empresa

**Sintoma.** `GET /api/v1/integration/odoo/users` devolvia todos os registos de `org_members` da organização, incluindo os arquivados (`archived_at` preenchido por `remove_employee`). O Odoo via quem saiu da empresa como membro activo, com o papel que tinha (`admin` incluído). Aberto na skill `delonix-meet-backend` desde o R121. Provado a 2026-09-16 contra Postgres real: `o membro arquivado continua no directório: [… {"email":"saiu@eta-odoo.ao", …}]`.

**Causa raiz.** A regra S3 do R121 («colega e quem pede são membros ACTIVOS») foi aplicada às cópias que decidiam acesso; esta listagem tinha a sua própria query sobre `org_members` e ficou de fora — o padrão que a catraca `pertenca_org_fora_de_org_rs` mede.

**Regra.** O directório entregue a uma integração é o de membros ACTIVOS. O filtro `archived_at IS NULL` entra na query existente (não soma uma ocorrência nova de `org_members` fora de `org.rs`). O destino é um helper em `org.rs` que devolva email/username/`odoo_uid`/papel (ADR-0004 §6 passo 3); não foi criado aqui porque `org.rs` estava a ser editado por outra sessão.

**Portão.** `server/tests/security_voice_odoo.rs::odoo_list_users_excludes_archived_members` (controlo positivo: o mesmo membro aparece antes de ser arquivado; o activo e o admin continuam depois).

**Ficheiros.** `server/src/odoo.rs`, `server/tests/security_voice_odoo.rs`, `docs/reference/openapi/v1.json`.

### R156 — A câmara ligada não aparecia nos outros participantes depois de uma troca de camada

**Sintoma.** Reportado pelo dono do produto: «a imagem da câmara ligada não aparece nas telas de outros participantes». Nos logs, `sfu layer switch failed` com `new track must have the same envelope as previous`, a seguir `sfu subscribe failed`, e a PC do subscritor em `failed`. Acontecia sempre que um subscritor voltava a uma camada simulcast que já tinha usado (`f → h → f`) — o que o `layerPolicy.ts` faz a cada redimensionamento de tile, aba em segundo plano ou perda medida.

**Causa raiz.** Quatro defeitos empilhados; cada correcção destapou o seguinte.
1. **Transceiver parado reaproveitado.** A troca removia a track e subscrevia de novo com o MESMO id (`<pub>-video-f`). O `add_track` do webrtc-rs 0.17 reaproveita um transceiver parado cujo id seja igual, e o `replace_track` interno recusa porque o `track_encodings` está vazio — falha sempre, não por corrida.
2. **Numeração e relógio crus.** Com a troca a fazer-se no MESMO sender (`replace_track`), cada camada chega com a sua numeração e o seu relógio RTP. O receptor via-os recuar e descartava os fotogramas: vídeo congelado depois da troca (medido com browsers: 20 fps → 0–2).
3. **Extensões de cabeçalho do publicador reencaminhadas.** Os ids de `a=extmap` só valem na negociação onde foram acordados, e o publicador e cada subscritor negoceiam em separado. Medido com clientes webrtc-rs: o `rid` do publicador (id 2, 1 byte) chegava ao subscritor no id que este usa para `transport-cc`; o interceptor TWCC falhava a leitura (`buffer too small`), a primeira leitura da track falhava e o `on_track` nunca disparava — subscrição negociada, RTP a sair do SFU, nenhum vídeo. Existia antes das trocas; só não havia um teste com simulcast real para o ver.
4. **Pacote perdido na fronteira.** Com o `LayerRewriter` a fechar a camada antiga DEPOIS do `replace_track`, um pacote dela já aceite (e com a numeração já avançada) ia para a track antiga, já desligada, e perdia-se: buraco na numeração exactamente na troca (medido a ~1 kpps: 2 fronteiras em 12).

**Regra.**
- A troca de camada faz-se no MESMO sender (`replace_track`), sem renegociar. A remoção + nova subscrição é só o recurso quando o `replace_track` recusa. Um id de track de subscrição é ÚNICO (`NEXT_TRACK_SEQ`) — nunca derivado só de publicador/tipo/rid.
- Cada subscrição de vídeo tem um `LayerRewriter`: numeração contígua e relógio sempre a avançar através das trocas; pacotes de outra camada são recusados. A porta à camada antiga fecha-se ANTES do `replace_track`.
- O vídeo reencaminhado vai SEM as extensões de cabeçalho do publicador (`strip_hop_extensions`); o sender do subscritor põe as suas. Não «optimizar» tirando a cópia.
- O PLI de um subscritor vai para a camada que o alimenta AGORA (`current_source`), não para a da subscrição original.

**Portão.** `sfu_e2e::troca_de_camada_simulcast_sem_renegociar_e_com_rtp_continuo` — publicador webrtc-rs com simulcast real (três encodings, numeração e relógio próprios, camada marcada no payload), subscritor a pedir `f → h → f → q → f` pela mensagem `video-interest` do browser. Verificado a falhar contra a base `4ff5249` em três pontos distintos: sem nenhuma correcção, o subscritor nunca recebe vídeo (defeito 3); só com a correcção das extensões, a troca #1 renegoceia (transceivers 3 → 4) e, com essa asserção desligada, a troca #2 (`h → f`) sai com `sfu layer switch failed` (defeito 1); com `e0184f5` sem o `LayerRewriter`, `timestamp recuou: 3000093000 (f) → 1500096000 (h)` (defeito 2). O defeito 4 é uma corrida: o teste publica a 200 pacotes/s por camada para a tornar provável, mas não a apanha de forma determinista.

**Observabilidade.** `delonix_sfu_layer_switch_failures_total` (novo) — cada unidade é um subscritor sem o vídeo de alguém; tem de estar a zero. Antes só existia num `warn`.

**Não validado.** Com browsers reais depois das quatro correcções (a máquina estava a carga 40–70 e o ICE caía por CPU). Se o defeito 3 também tirava vídeo a um subscritor Chrome/Firefox (os ids que o Chrome propõe não são os do SFU, mas o browser pode tolerar a extensão errada) não foi medido. O áudio continua a ser reencaminhado com as extensões do publicador (leva o nível de voz): não se mediu se colide com os ids do subscritor.

**Ficheiros.** `server/src/sfu.rs` (`switch_layer`, `subscribe_layer`, `LayerRewriter`, `strip_hop_extensions`, `current_source`), `server/src/metrics.rs`, `server/src/sfu_e2e.rs`.

### R157 — A câmara aparecia e sumia: o browser perdia o consentimento ICE ~5 s depois de ligar

**Sintoma.** Continuação do reporte da R156 («a imagem da câmara ligada não aparece nas telas de outros participantes»), com a R156 já corrigida. Com dois Chromium: um dos participantes (quase sempre o que entra em segundo lugar, às vezes o primeiro) liga o ICE, recebe umas centenas de pacotes de vídeo e **~5 s depois** passa a `disconnected`; o ICE restart do `callRecovery.ts` não recupera e acaba em `failed`. No servidor o agente ICE desse peer continua `Connected` e o log enche-se de `[controlled]: inbound isControlled && a.isControlling == false`. Parecia que o SFU deixava de enviar; não deixava — deixava de RESPONDER.

**Causa raiz.** Os dois lados ficavam com o papel ICE CONTROLLED, e o webrtc-ice não resolve conflitos de papel.
1. O libwebrtc actual (medido no Chrome 151.0.7922.34) passa a CONTROLLED sempre que aplica `setLocalDescription(answer)` estando CONTROLLING (`JsepTransportController::SetLocalDescription_n`: `if (ice_role_ == ICEROLE_CONTROLLING) SetIceRole_n(type == kOffer ? CONTROLLING : CONTROLLED)`). O browser oferta primeiro (fica controlling) e o SFU oferta logo a seguir para o subscrever a quem já está na sala (e outra vez a cada câmara/ecrã) — o browser responde e muda de papel.
2. Entre dois libwebrtc isto resolve-se sozinho pelo conflito de papel da RFC 8445 §7.3.1.1 (tiebreaker ou erro 487). O webrtc-ice 0.17.1 — e a 0.17.2 — num agente controlled que recebe `ICE-CONTROLLED` **descarta o pedido sem responder** (`handle_inbound`) e ignora 487. As verificações de consentimento do browser (RFC 7675) ficavam sem resposta e o browser declarava a ligação morta. Metade das vezes o browser safava-se sozinho (o tiebreaker dele perdia contra um pedido do SFU e voltava a controlling); a outra metade respondia 487 ao SFU, que o ignorava — daí «às vezes funciona».
3. O ICE restart do cliente não saía do impasse pela mesma razão: o browser oferta o restart já controlled, e o SFU continua controlled.

**Medição que o prova.** Agente webrtc-ice instrumentado (papel, tiebreaker e USERNAME de cada pedido): o Chrome da Teresa manda `ICE-CONTROLLING` às 00:20:53.267 e `ICE-CONTROLLED` às 00:20:53.385 — ~70 ms depois de aplicar a resposta à oferta do SFU (00:20:53.345); a Ana muda às 00:20:54.494, depois da renegociação dela. No log do Chrome (`--enable-logging --v=1`) aparecem `Got role conflict; switching to controlling role` / `Sending STUN BINDING error response: reason=Role Conflict` para as portas do SFU. Não é o caminho de envio do SFU: nenhum lock nem tarefa parada (CPU ~2 %), e os clientes webrtc-rs do teste de carga (que não mudam de papel) nunca o viram.

**Onde nasce.** Não num commit deste repositório. Reproduzido na `origin/main` (`4ff5249`, UI antiga, 3/3 corridas a falhar, 284 pedidos descartados numa corrida com `webrtc_ice=debug`), na `delonix-meet-backend/sfu-troca-camada` e na `frontend/ui-template-rebuild` (`9f09319`, UI nova, 3/3). A oferta do SFU depois da do browser existe desde `3537eba` (o primeiro commit); o gatilho é o comportamento do libwebrtc. Em que versão do Chrome essa regra entrou não foi determinado.

**Regra.**
- Um agente ICE nosso NUNCA descarta em silêncio um pedido autenticado com o mesmo papel que o dele: responde 487 com MESSAGE-INTEGRITY. É o que manda o browser inverter o papel (`Connection::OnConnectionRequestErrorResponse` → `NotifyRoleConflict`). O SFU não troca de papel.
- O patch vive em `server/vendor/webrtc-ice` (0.17.1 intacto no commit `34be6f8`, a alteração no seguinte, só em `agent_internal.rs`). Actualizar o webrtc-rs obriga a reaplicar o patch ou a provar, com os testes abaixo, que o upstream já o faz.
- ICE-lite no SFU também resolvia (o Chrome fica controlling contra um lite), mas é incompatível com o relay-only do Kubernetes (`FORCE_TURN_RELAY=1`): um agente lite só usa candidatos host. Por isso não foi essa a correcção.

**Portão.**
- `sfu_e2e::conflito_de_papel_ice_recebe_487_autenticado` — sonda STUN pelo par seleccionado, com as credenciais reais: papel certo → sucesso autenticado (controlo); `ICE-CONTROLLED` → 487 autenticado. Sem o patch falha em `Silencio`.
- `sfu_e2e::media_e_consentimento_sobrevivem_as_renegociacoes_do_sfu` — dois peers, 24 s de media nos dois sentidos, renegociação do SFU logo após cada ligação e a meio; a cada 4 s exige PC `connected`, RTP a subir nos dois sentidos, sucesso ao consentimento e 487 ao conflito. Sem o patch falha na janela 1. Âmbito: o cliente webrtc-rs não muda de papel ao responder, a inversão do browser é encenada pela sonda.
- Dois Chromium com câmara falsa (script da sessão em `.worktrees/delonix-meet/sfu-envio-run/cruzado.mjs`, fora do repo): 5/5 corridas de 60 s com `framesDecoded` a subir nos dois sentidos em todas as amostras, na UI antiga (base desta branch) e 5/5 na UI nova (`frontend/ui-template-rebuild` + estes commits), com TURN desligado e o Chrome restrito à interface por omissão; mais 2/2 na UI nova com TURN e todas as interfaces, e o script das trocas de camada (janela 480↔1440) sem quedas. Sem o patch: 3/3 a falhar em cada base.

**Não validado.** Firefox e Safari (não usam o libwebrtc para o papel ICE da mesma forma; o 487 é RFC, deviam tratá-lo). Kubernetes com relay-only (`FORCE_TURN_RELAY=1`): o mecanismo é o mesmo mas não se correu. Chrome 153.0.8010.12 só uma corrida de 60 s na UI nova (estável, e o SFU respondeu 487 durante ela — o 153 também muda de papel).

**Ficheiros.** `server/vendor/webrtc-ice/src/agent/agent_internal.rs` (`send_role_conflict`), `server/Cargo.toml` (`[patch.crates-io]`), `server/Cargo.lock`, `server/src/sfu_e2e.rs`.

### R158 — Portas UDP presas num servidor parado: a PC que falhava por ICE nunca acabava de fechar

**Sintoma.** Depois de uma carga com o host saturado (30 salas × 4, perda de 64%, muitos `sfu create_offer failed error=connection closed`), o servidor já sem ninguém ficou >30 min com 359 sockets UDP abertos, com `delonix_sfu_peer_connections` e `delonix_sfu_subscriptions` a zero. Em corridas limpas os sockets fechavam em 90 s. Cada PC presa segura ~18 portas: o intervalo por omissão (50000–50200, 201 portas) esgota-se à 12.ª.

**Causa raiz.** O handler de `on_peer_connection_state_change` chamava `remove_peer` → `pc.close()` quando o estado passava a `Failed`. O webrtc-rs 0.17.1 (`do_peer_connection_state_change`) segura um `tokio::Mutex` à volta do handler enquanto ele corre; o `close()` chega ao passo 11 (`update_connection_state` → `Closed`) e pede o MESMO mutex. Fica pendurado para sempre — e o peer já tinha saído da sala, por isso nenhum gauge o via e a saída do WebSocket mais tarde já não o encontrava para fechar.

Ao lado, uma armadilha da mesma biblioteca: `RTCRtpSender::read` espera por `Notify::notify_waiters()` sem consultar a bandeira de paragem. Um `stop()` que chegue quando a tarefa não está a ler perde-se, e num sender que nunca enviou o `read_rtcp` seguinte nunca regressa. A tarefa de drenagem de RTCP segurava um `Arc<Publication>` (e com ele a PC do publicador) até esse `read_rtcp` falhar. Não se reproduziu no SFU (200 ciclos de subscrever/dessubscrever sem ficar nada vivo), mas a armadilha está fixada num teste.

**Medição que o prova.** Recenseamento novo em `/metrics` (`delonix_sfu_pc_alive` por `Weak`, `delonix_sfu_pc_unclosed` = `close()` que nunca regressou, peers/publicações/tarefas contados por `Drop`). Servidor real, 8 clientes do gerador de carga congelados com `SIGSTOP` (WebSocket aberto, ICE morto → `Failed`), depois mortos: antes da correcção `peers_in_rooms=0`, `peer_connections=0`, mas `pc_alive=8 pc_unclosed=8`, 8 peers e 8 tarefas de negociação vivos e **144 sockets UDP**, iguais 120 s depois; com a correcção tudo a zero e 0 sockets 20 s depois do `Failed`.

**Regra.**
- NUNCA fechar (nem remover, que fecha) uma `RTCPeerConnection` de dentro de um callback dela. A remoção por `Failed` corre numa tarefa à parte, e só remove o peer se ainda for o mesmo `Arc` (`remove_peer_exact`).
- Uma tarefa que vive de um sender ou receiver do webrtc-rs não pode depender só de a leitura falhar para terminar: segura `Weak`s e confirma periodicamente que a subscrição existe (`subscription_alive`, pergunta ao `subscribed` do peer, que sobrevive à troca de camada por `replace_track`).
- Um `close()` acima de 10 s é um erro no log (`close_pc`), não silêncio.
- Uma fuga de PC vê-se em `delonix_sfu_pc_unclosed - delonix_sfu_peers_in_rooms > 0` sustentado, não nos gauges de negócio.

**Portão.**
- `sfu_e2e::pc_que_falha_por_ice_fecha_e_nao_fica_viva` — o cliente desaparece sem avisar, ICE com timeouts curtos; exige que a PC do SFU feche e deixe de existir. Sem a correcção falha com `pc_alive: 2, pc_unclosed: 2, peers_in_rooms: 1`.
- `sfu_e2e::churn_de_subscricoes_nao_deixa_nada_vivo` — 200 ciclos de interesse de vídeo; depois de todos saírem o censo tem de voltar a zero.
- `sfu_e2e::webrtc_rs_read_rtcp_depois_de_stop_nao_regressa` — fixa a armadilha da biblioteca; se passar a falhar, o upstream corrigiu-a.

**Não validado.** A liveness da tarefa de RTCP depois de uma troca de camada só se exercita quando passam 5 s sem RTCP, o que não acontece nos testes (os interceptors mandam Receiver Reports a cada segundo); está coberta por leitura de código, não por um teste que a force. Kubernetes com relay-only não se correu.

**Ficheiros.** `server/src/sfu.rs` (`Census`, `close_pc`, `remove_peer_exact`, handler de estado, `subscribe_layer`, `subscription_alive`), `server/src/main.rs` (`/metrics`), `server/src/sfu_e2e.rs`.

### R160 — Segredos de integração em claro na base (S5): webhooks, SSO e WebDAV

**Sintoma.** Nenhum visível. Quem lesse um dump, um backup ou uma réplica da base levava, em texto claro, o segredo HMAC de cada webhook (`org_webhooks.secret`), o `client_secret` OIDC de cada organização (`org_sso_configs.client_secret`) e a password do Nextcloud/WebDAV da plataforma (`platform_storage.webdav_password`) — credenciais de terceiros de todos os inquilinos. Auditoria 2026-09-16, S5 (`storage.rs:106`, `org.rs:1144`, `webhooks.rs:268`). A migração 0019 dizia «encriptado em repouso (app-level)» e a 0030 «cifrado se STORAGE_ENCRYPT=1»; nenhuma das duas era verdade.

**Causa raiz.** Os três handlers faziam `bind` do valor recebido directamente na coluna. Não podem ser hash (o servidor volta a usá-los), e a `core::secret_box` — que já cifrava a chave RTMP dos destinos de emissão — não era usada aqui.

**Regra.** Novo `server/src/secrets_at_rest.rs`, único caminho para estas colunas: `seal` na escrita (aad `<tabela>.<coluna>:<id da linha>`; o id do webhook nasce antes do `INSERT`), `open` onde o segredo se usa — assinatura em `webhooks::attempt` (disparo e reenvio), `auth::sso_login`/`sso_callback`, PROPFIND de `storage::test_storage`. Nenhuma resposta devolve o segredo: o `GET /api/orgs/{org_id}/sso` ganha `has_client_secret` (o storage já tinha `webdav_password_set`; o webhook já não serializava `secret`).

**Decisões de compatibilidade — explícitas.**
- Sem `DATA_ENCRYPTION_KEYS` (produção sem chaves): escrever um segredo NÃO vazio é `422 secrets.encryption_unconfigured`, nada é gravado. Criar um webhook SEM segredo (Slack/Teams/Mattermost, ou `generic` sem assinatura), guardar o SSO sem `client_secret` e o storage sem password continuam a funcionar sem chaves.
- O herdado em claro lê-se sempre, com ou sem chaves. Um valor `enc:v1:` sem chaves, ou que não abre (chave retirada, valor copiado de outra linha), é erro interno com log `error` — `500` no teste WebDAV e no OIDC; no webhook a entrega fica `failed` e **não** se envia sem assinatura.
- Migração: `secrets_at_rest::reseal_legacy` corre no arranque e de hora a hora (`lib.rs`). Com chaves, cifra os herdados em lotes de 200 com `UPDATE … WHERE coluna = <valor lido>` (não pisa uma escrita concorrente), idempotente; sem chaves, só avisa quantos continuam em claro.
- Um texto herdado que por acaso comece por `enc:v1:` seria lido como cifrado e falharia. Não se tratou.

**Portão.** `server/tests/secrets_at_rest.rs` (Postgres real + receptor HTTP em 127.0.0.1): coluna `enc:v1:` sem o texto claro nas três; HMAC recebido válido com o segredo ORIGINAL; `Authorization: Basic` do PROPFIND com a password original; nenhuma resposta traz o segredo nem o cifrado; herdado inserido por SQL serve antes e depois da tarefa, que cifra (1,1,1) e na segunda passagem 0; sem chaves 422 nas três escritas com segredo e 200 sem ele, herdado serve, cifrado sem chave falha fechado; cifrado copiado para outro webhook dá `failed` sem envio, e o `client_secret` da org A não abre com o contexto da org B. Prova de que morde: com `seal` a devolver o texto claro (o comportamento anterior), 4 dos 6 testes falham em «não está cifrado».

**Não validado.** O fluxo OIDC completo contra um IdP (o teste prova o aad que `auth.rs` usa, não um `sso_login` real — a discovery exige `https://`). **Fica aberto:** `apikeys.rs` (provisão de org pela integração, `sso.client_secret`) continua a gravar o `client_secret` em claro — não era ficheiro desta sessão; deve chamar `org::seal_sso_client_secret`. Até lá a leitura funciona e a tarefa horária cifra-o.

**Ficheiros.** `server/src/{secrets_at_rest,webhooks,org,auth,storage,lib}.rs`, `server/tests/secrets_at_rest.rs`, `docs/reference/openapi/{bff,v1}.json`.

### R170 — Uma chave `dlx_` era um cheque em branco: sem escopos, sem expiração, e o limite era do IP (S6)

**Sintoma.** Uma chave emitida para o sync de calendário (ler reuniões) também criava salas, punha bots em salas com a sala de espera contornada, listava gravações e cancelava reuniões — e servia para sempre. O `HARNESS.md` chegou a afirmar «hash + scopes». O limite da v1 era por IP: duas integrações da mesma organização atrás do mesmo NAT partilhavam 120 pedidos/min, e a mais faladora deixava a outra a receber `429` com um `Retry-After: 60` constante.

**Causa raiz.** `org_api_keys` não tinha onde guardar escopos nem expiração, e o `ApiKeyAuth` só devolvia `org_id`/`owner_id`. O `v1_rate_limit` corria antes da autenticação e só conhecia o IP.

**Regra.**
- **Catálogo fixo** em `delonix_meet_domain::identity::api_key::Scope`: `org:read`, `rooms:read`, `rooms:write`, `bots:join`, `meetings:read`, `meetings:write`, `recordings:read`. Sem `*`. Uma chave guarda a lista EXPLÍCITA: um escopo novo no catálogo não chega às chaves existentes.
- **Um só ponto de decisão:** `key.require(Scope::…)?` na primeira linha de cada handler v1 → `403 api_key.scope_missing` com o escopo em `details`. O teste `cada_rota_v1_exige_o_seu_escopo` percorre as 11 rotas nas duas direcções (sem o escopo → 403; só com ele → 2xx): um handler que esqueça o `require` falha ali.
- **Expiração:** `expires_at` opcional, futuro e ≤ 2 anos. Expirada → `401 api_key.expired` (distinto de desconhecida/revogada, que continua `401 auth.unauthenticated`).
- **Compatibilidade — decisão explícita:** (1) as chaves anteriores à migração 0046 recebem o catálogo inteiro (o `DEFAULT` só existe durante o `ALTER` e cai logo a seguir); (2) uma chave criada **sem `scopes`**, pela BFF ou pelo provisionamento `POST /api/v1/admin/orgs`, recebe também o catálogo inteiro. O cliente web e o módulo Odoo não enviam `scopes`; tornar o omisso mais restritivo dentro da v1 partia integrações que se criam hoje sem mudar nada do lado delas, e a v1 só quebra com v2. Quem quer menos privilégio pede a lista; `[]` é recusado (`api_key.scopes_empty`).
- **Limite por chave:** o middleware procura a chave UMA vez (segue nas extensões para o extractor). Balde = a chave, se existe e não expirou; o IP em todos os outros casos — um hash do que vier no cabeçalho dava um balde novo por chave inventada e anulava o limite. `429` com `Retry-After` = o que falta da janela, arredondado para cima e nunca 0. A `/api/ice` passa a `ip_rate_limit` (só IP): autentica por sessão, e um balde escolhido por uma `dlx_` que a rota nem lê deixava contorná-lo.
- **`last_used_at`:** no máximo uma escrita por minuto por chave, com a guarda repetida no SQL para dois nós não escreverem os dois.

**Portão.** `server/tests/api_key_scopes.rs` (Postgres real): `cada_rota_v1_exige_o_seu_escopo`, `sem_meetings_write_o_post_e_403_e_com_ele_200`, `criacao_valida_escopos_e_expiracao_e_a_lista_mostra_os`, `chave_expirada_e_401_api_key_expired`, `last_used_at_no_maximo_uma_escrita_por_minuto`, `chave_anterior_a_migracao_continua_a_servir` (desfaz as colunas, grava a chave com o INSERT antigo, aplica o SQL da 0046 e percorre as 11 rotas), `chave_provisionada_serve_os_fluxos_do_odoo`, `limite_por_chave_isola_duas_chaves_do_mesmo_ip`. Unitários no domínio e em `apikeys::tests::migracao_0046_da_as_chaves_antigas_o_catalogo_inteiro`.

**Ficheiros.** `server/crates/delonix-meet-domain/src/identity/api_key.rs`, `server/migrations/0046_api_key_scopes.sql`, `server/src/{apikeys,meetings_v1,rate_limit,lib}.rs`, `server/tests/{api_key_scopes,api_v1,organization}.rs`, `docs/reference/openapi/{bff,v1}.json`.

### R171 — Revogar uma chave que não existia respondia `{"ok": true}`

**Sintoma.** `DELETE /api/orgs/{org}/api-keys/{id}` devolvia `200 {"ok": true}` para uma chave inexistente ou de OUTRA organização — o teste de isolamento chegava a afirmar «responde ok mas não apaga nada». Quem revogava uma chave comprometida com o id errado era informado de que tinha corrido bem.

**Regra.** `204` sem corpo quando apaga; `404 api_key.not_found` quando não há linha com esse id NESTA organização (não se confirma que existe noutra). O `web/src/api.ts` trata `204` desde `b36661f`. As asserções de `tests/organization.rs` mudaram com intenção (`200→204`, `200→404`).

**Ficheiros.** `server/src/apikeys.rs` (`revoke`), `server/tests/{api_key_scopes,api_v1,organization}.rs`.

### R180 — Só os webhooks tinham guarda anti-SSRF; o resto saía para onde o cliente mandasse (S4)

**Sintoma.** A `validate_public_url` só era chamada pelos webhooks. Um admin de organização podia gravar `odoo_url = http://127.0.0.1:8069` (ou `169.254.169.254`) e o login seguinte de qualquer membro mandava a PASSWORD para lá; o emissor OIDC era descoberto com até 5 redirects e sem timeout; o `PROPFIND` do WebDAV e o Ollama também saíam sem guarda. Nos webhooks a validação e a ligação resolviam DNS em separado — um nome podia responder público ao teste e interno à ligação (rebinding). E `::ffff:127.0.0.1`, NAT64 e 6to4 não estavam na lista.

**Regra.** Nenhum `reqwest::Client` fora do `net_guard` (catraca `clientes_reqwest=0`). URL de cliente → `state.outbound.tenant()` + `check_tenant_url` (ao ligar) / `check_tenant_config_url` (ao gravar: 400 com razão); URL do operador → `operator()` (rede privada sim, link-local/metadados não); OIDC → `outbound.oidc()`, que valida CADA pedido do fluxo (o `jwks_uri`
**Ficheiros.** `server/crates/delonix-meet-core/src/egress.rs`, `server/src/net_guard.rs`, `server/src/{auth,apikeys,odoo,odoo_sso,org,storage,webhooks,ai,lib,config}.rs`, `server/tests/egress_guard.rs` (a provisão recusa antes de escrever; o login Odoo NÃO abre ligação a `127.0.0.1` sem allowlist e abre com ela; recusas ao gravar com controlo positivo).

### R181 — Respostas `{"ok": true}` que não diziam nada, e recusas de permissão em 401

**Sintoma.** 18 rotas respondiam `200 {"ok": true}` — incluindo `DELETE` de coisas que não existiam (arquivar um id inventado, apagar um webhook de OUTRA organização, revogar um link que não havia), que diziam «ok» sem ter feito nada. Nas mesmas zonas, a falta de permissão ainda saía como `401` (gravações: partilhas, links, download; reuniões: arrancar, acta, notas, lista de convidados), que o web lê como sessão perdida e tenta renovar (a classe do R153). E o download de uma gravação FALHADA devolvia `400` com o motivo da falha ANTES de verificar o acesso: uma conta de outra organização lia o `failure_reason`. Partilhar com um id de utilizador inexistente dava `500` (chave estrangeira).

**Regra.** `respostas_ok_true=0` na catraca. Apagar → `204`, e `404` quando não havia linha NESTA organização/recurso. `PUT` de configuração (Odoo, SSO, armazenamento) devolve o recurso como o `GET`; `PUT` do RSVP devolve o convidado; acta → `204`; telemetria (`join-timings`, `quality-samples`) → `204`; logout e desactivar MFA → `204`; acknowledge de chamadas perdidas → `{"updated": n}`; partilhar → `201` + `Location` (ou `200` se já estava). Acesso a gravação: `seen_item` primeiro (404 a quem não chega, antes de qualquer outra resposta), depois `403 recording.not_owner` / `recording.download_forbidden`; partilhar e links são do dono ACTIVO (`AccessFacts::can_share`). Reuniões: `404` a quem não é dono nem convidado, `403 meeting.not_host` ao convidado. Quadros: `404` fora da organização, `403 whiteboard.not_manager` dentro. Mantêm `401` só as credenciais que não são a sessão (password do link público, token de sala do directo).

**Ficheiros.** `server/src/{actions,auth,meetings,mfa,odoo,org,presence,recordings,rooms,storage,webhooks,whiteboards}.rs`, `server/crates/delonix-meet-domain/src/content/recording.rs` (`can_share`), `server/tests/{content,identity,organization,recordings_metadata,scheduling,security_identity}.rs` (asserções mudadas com intenção, cada uma com o controlo), `web/src/api.ts`, `web/e2e/mfa.mjs`, `scripts/arquitectura-baseline.txt`.

### R182 — O chat da sala nunca era gravado, a mensagem «privada» ia para a sala toda, e o protocolo da UI nova era descartado em silêncio

**Sintoma.** Três coisas, medidas no levantamento UI↔API de 2026-09-17:
1. `GET /api/rooms/{room_code}/messages` devolvia sempre `[]`. A tabela `room_chat_messages` existia desde a 0018 e nenhum código a escrevia; e a leitura, com `ORDER BY created_at ASC LIMIT 200`, devolvia as primeiras 200 mensagens, não as últimas.
2. A UI nova enviava `chat{to}` para uma conversa privada. `ClientMsg::Chat` só tinha `text`, o `to` era ignorado pelo serde, e a mensagem era difundida à sala toda. É exposição de dados.
3. Cerca de 20 mensagens que a UI nova envia pelo `/ws` existiam só no `server/` da branch da UI e eram descartadas pelo `signaling.rs` desta linha sem erro: fios e reacções, `set-role`, `spotlight`, `admit-all`, sala de espera em runtime, `qa-hide`/`qa-spotlight`, `breakouts-broadcast`, e as páginas, cursores e permissões do quadro. A moderação e o quadro pareciam funcionar e não faziam nada.

Estava corrigido na linha da UI (R122 dessa branch, número já usado aqui; commits `c153b7a`, `cd8458a`, `0c87e62`), mas essa linha tem o seu próprio servidor.

**Regra 1 — a escrita do chat não entra no caminho quente.** O handler corre com o lock da sala (R16). A mensagem vai para uma fila LIMITADA (`room_chat::ChatStore`, `try_send`), consumida por uma tarefa própria e por ordem. Fila cheia descarta e conta (`delonix_chat_persist_dropped_total`); a sala recebe a mensagem na mesma. A retenção prometida na 0018 é cumprida por `room_chat::retention_sweep`, num cron horário (G9).

**Regra 2 — uma privada só existe para o par.** Vai só ao destinatário (`send_to`) e o `chat-sent` volta a quem envia. Responder a uma privada continua privado. Um terceiro não lhe responde nem reage, e recebe o mesmo erro de uma mensagem inexistente. O histórico filtra na CONSULTA (`to_user_id IS NULL OR user_id = me OR to_user_id = me`): nem o anfitrião lê privadas alheias.

**Regra 3 — o que se esconde a um público não lhe é enviado.** Uma pergunta de Q&A escondida não sai para quem não é anfitrião. As duas vistas vão por difusões SEM sobreposição (`broadcast_hosts` + `broadcast_non_hosts`, com o evento Redis `BroadcastNonHosts`), para a ordem de chegada não decidir qual das vistas o anfitrião fica a ver.

**Regra 4 — desligar a sala de espera não abre a porta a quem não tem entrada directa.** O token de sala separa `lobby` (sem entrada directa: espera sempre) de `wr` (a configuração da sala); só o segundo é substituível em runtime. Origem e cargo decidem-se no servidor e viajam assinados no token.

**Adaptações nesta linha.** Router e crons em `lib.rs`. As migrações 0039 e 0048 da UI passam a 0050 e 0051 (a 0049 é a dos convites pendentes, #88). O `PeerRole` do R124 funde-se com o da UI (`role` + `can_admit` EFECTIVO: um co-anfitrião por papel continua a admitir).

**Portão.** `signaling` + `room_chat` (91 testes do hub, com a metade negativa de cada controlo e os 6 da conversa directa); `tests/room_chat.rs` contra Postgres real (a privada não volta a um terceiro; fios e reacções no histórico).

**Ficheiros.** `server/src/{signaling,room_tools,room_chat,rooms,auth,org,users,pubsub,metrics,lib}.rs`, `server/migrations/0050_room_chat_threads_reactions.sql`, `server/migrations/0051_room_chat_direct.sql`, `server/tests/room_chat.rs`.

### R183 — Dois servidores com dois modelos de gravação: a UI nova falava com rotas e campos que a linha da main não tinha

**Sintoma.** Medido no levantamento UI↔API de 2026-09-17: a UI nova lê `duration_ms`, `kind`, `state`, `transcript_status`, contagens, `description`, `tags` e `visibility` de cada gravação e chama `/details`, `/publish`, `/thumbnail`, `/views`, `/participants`, `/transcript` e `/captions/*`. A linha da main devolvia `duration_secs`, `category`, `title` e `processing_state`, e parte dessas rotas não existia: a biblioteca aparecia vazia de metadados e o leitor a falhar em silêncio.

**Estado nesta linha (honesto).** Reconciliado em parte: as rotas do leitor (`recording_meta`, `recording_captions`, edição e geração de capítulos) convivem com o `recordings.rs` da main, que fica como modelo de dados (título/categoria, capítulos e comentários paginados). O que NÃO está reconciliado é o contrato de DADOS do item da biblioteca da UI nova (`state`, `kind`, `visibility`, `description`, `tags`): decidir qual dos dois modelos fica é uma decisão de produto, e o e2e `gravacoes-meta.mjs` está fora do CI por isso (ver `scripts/e2e-fora-do-ci.txt`).

**Regra (o que já vale).** O worker de transcrição entrega os segmentos com tempos e a língua detectada, e é o SERVIDOR que aplica o DLP a tudo o que chega (`ai-worker/job_source.py`, `transcriber.py`) — um worker que gravasse direto contornaria o DLP.

**Portão (segmentos).** `server/tests/grpc.rs` (`transcription_queue_lease_complete_and_dlp`: o DLP corre em cada segmento, os incoerentes saem, a confiança guardada é a média) e as tabelas de `domain::content::transcription`. Os campos novos do `CompleteJobRequest` (`segments`, `language`) são compatíveis no fio, mas partem quem constrói a mensagem em Rust com um literal: `tests/{grpc,notifications}.rs` usam `..Default::default()`.

**Ficheiros.** `ai-worker/{transcriber,job_source,transcribe_worker}.py`, `server/proto/delonix/meet/transcription/v1/transcription.proto`, `server/src/{grpc,transcription,recording_meta,recording_captions,recording_chapters}.rs`, `server/crates/delonix-meet-domain/src/content/transcription.rs`, `server/tests/{grpc,notifications}.rs`, `web/e2e/isolamento.mjs`.

### R184 — Agendar uma reunião «videoaula» ou «gravar automaticamente» era ignorado: a sala nascia sempre normal, sem espera e sem gravação

**Sintoma.** O formulário de agendar mostrava formato (reunião, videoaula, emissão, híbrida), sala de espera, «gravar automaticamente» e qualidade da gravação, mas `meetings::start` criava SEMPRE uma sala `normal`, sem sala de espera e sem gravação: a pessoa marcava uma emissão com gravação a 1080p e entrava numa reunião comum, sem que nada avisasse que os campos tinham sido descartados. Um campo que o cliente escreve e o sistema ignora é pior do que um campo que não existe.

**Regra.** As opções vivem na PRÓPRIA reunião (`meetings.format`, `waiting_room`, `auto_record`, `record_quality`, migração 0063) e passam à sala no arranque (`rooms.auto_record`, `rooms.record_quality`, onde o gravador do servidor as lê). A validação é do servidor (`SessionOptions::validate`): `format` fora de `meeting|training|broadcast|hybrid` e `record_quality` fora de `2160p|1080p|720p|audio` dão 400 antes de gravar — nunca se corta em silêncio para um valor por omissão. As duas listas repetem-se em `CHECK` na base como segunda linha. A resposta «tentativa» ao convite (`meeting_invitees.status`) entra no mesmo passo.

**Não faz** (e o ecrã não o mostra): destinos de emissão e dial-in PSTN por reunião — são recursos da organização, sem `meeting_id`, e um campo para eles seria outro campo ignorado.

**Alterar depois de agendar, e a v1.** `PATCH /api/meetings/{meeting_id}` altera só as opções (só o anfitrião; `403 meeting.not_host` ao convidado, `404` a quem não chega; campos desconhecidos recusados, não ignorados) e a v1 aceita-as no create, no `PATCH` e devolve-as no `GET` e na lista. As duas superfícies chamam `meetings::patch_session_options`, que também as passa à sala já criada. `auto_record` numa sala E2EE é `422 meeting.auto_record_e2ee` — o gravador do servidor não tem a chave; recusa-se em vez de aceitar e não gravar. O `PATCH` da v1 valida o tecto de 200 convidados ANTES de escrever: antes gravava título, datas e opções e só depois respondia `400`. O `external_source` da lista é só o prefixo com forma de identificador (`odoo:…` → `odoo`); uma referência que não declara sistema lê-se `api` em vez de sair inteira pela BFF.

**Portão.** `server/tests/meeting_session_options.rs` (Postgres real: criar pela BFF e pela v1 com as mesmas regras, lista, `tentative`, a sala arrancada herda as opções, os dois `PATCH`, a gravação automática à entrada do anfitrião por `/ws`, e o `PATCH` v1 que valida antes de escrever) e a validação em `SessionOptions::validate`.

**Ficheiros.** `server/src/{meetings,meetings_v1,apikeys,rooms,recorder}.rs`, `server/migrations/0063_meeting_session_options.sql`, `server/tests/meeting_session_options.rs`, `web/src/pages/calendar/ScheduleForm.tsx`.

### R189 — Um merge com dois blocos de conflito foi empurrado com o segundo por resolver

**Sintoma.** Ao propagar a `main` (#89) pela pilha, o `HARNESS.md` da `backend/bw1-protocolo-sala` tinha dois blocos em conflito. O script de resolução tratou o primeiro e o commit seguiu com `<<<<<<< HEAD` … `>>>>>>>` na tabela de infraestrutura. Nenhum portão reparou: o `check-docs-drift.sh` lê as linhas que procura e não o ficheiro inteiro, e num `.md` nada compila. No mesmo passo, a bateria final correu sobre uma árvore com um merge PARADO em conflito, porque o script não parava quando o `git merge` falhava.

**Regra.** Uma linha seguida que comece por `<<<<<<< ` ou `>>>>>>> ` é um conflito por resolver, e o `check-repo-hygiene.sh` falha com o ficheiro e a linha (controlo negativo feito: um bloco acrescentado ao `HARNESS.md` faz o portão falhar). Resolver conflitos por script: iterar até não restar NENHUM marcador, nunca só o primeiro índice. Uma bateria só conta sobre `git status` sem `UU`.

**Ficheiros.** `scripts/check-repo-hygiene.sh`, `HARNESS.md`.

### R210 — Uma chamada de emergência podia ser gravada, bloqueada ou travada pelo limite de canais

**Risco.** O plano de marcação é do cliente: uma regra `1XX` (ramal) antes da de emergência, uma regra `block` larga (`11X`), `record: true` na regra de emergência, ou o tronco no máximo de canais deixavam o 112 gravado, recusado ou sem caminho.

**Regra.** Os números de emergência (`TELEPHONY_EMERGENCY_NUMBERS`, omissão `112,113,115`) resolvem-se ANTES das regras (`domain::telephony::dial_plan::resolve`), nunca gravados e com TODOS os troncos activos como reserva. Recusa ao gravar: `telephony.emergency_never_recorded`, `telephony.emergency_cannot_be_blocked`. A extensão servida ao FreeSWITCH (`telephony_fs_xml`) usa `bridge` sem `limit_execute` e `delonix_record=false`; a ingestão de CDR força `recorded=false`; a base tem `CHECK (NOT (emergency AND record))` nas regras e nos CDRs. O teste rápido e o convite para sala RECUSAM emergência (`telephony.test_call_emergency_refused`, `telephony.emergency_not_invitable`): o invariante é sobre quem marca.

**Prova.** `dial_plan::tests::emergency_*`, `tests/telephony.rs::dial_plan_first_match_emergency_invariants_and_test` (inclui o `UPDATE` directo recusado pela base), e contra o FreeSWITCH real (`web/e2e/telefonia-freeswitch.mjs`): com o tronco a 1/1 canais a chamada normal é recusada e a de emergência passa.

**Ficheiros.** `server/crates/delonix-meet-domain/src/telephony/dial_plan.rs`, `server/src/telephony_fs_xml.rs`, `server/src/telephony_cdr.rs`, `server/migrations/0070_telephony_dial_plan.sql`, `0068_telephony_call_records.sql`.

### R211 — Um CDR reenviado cobrava a chamada duas vezes

**Risco.** O `mod_json_cdr` reenvia quando não recebe `2xx` (e um `2xx` perdido na rede também causa reenvio). Sem chave, cada reenvio era uma chamada e um custo novos.

**Regra.** `UNIQUE (source, source_call_id)` em `telephony_call_records`; o reenvio responde `200 {duplicate: true}` com o id que já existe. A perna A de uma chamada pelo plano (o PBX) leva `delonix_cdr_skip=true` e responde `204`: o custo e o ASR estão nas pernas B, uma por tentativa de tronco.

**Prova.** `tests/telephony.rs::cdr_ingestion_idempotent_priced_at_time_of_call_listed_and_summed`; contra o FreeSWITCH real, o mesmo ficheiro do `log-dir` reenviado dá `200 duplicate` e a lista não cresce.

**Ficheiros.** `server/src/telephony_cdr.rs`, `server/migrations/0072_telephony_call_records.sql`.

### R212 — O custo de uma chamada mudava quando se mudava o preço

**Risco.** Calcular o custo na leitura com «o preço actual» reescrevia o consumo de meses passados, e um preço gravado com início no passado fazia o mesmo.

**Regra.** Preços e taxas de câmbio são histórico (linhas novas, `valid_from`). O custo congela-se na ingestão ao preço em vigor em `started_at` (`cost_e4`, `price_id`), por começo de minuto, só para saída atendida (atendida = `answer_epoch`, porque uma chamada atendida e desligada em menos de 1 s tem `billsec` 0 — medido). Sem preço: `cost` `null` com `cost_reason`, nunca `0` inventado. A API recusa preços e taxas no passado (`telephony.price_backdated`). O total em Kz só existe com taxa para cada moeda (`total_aoa_reason: missing_exchange_rate`).

**Prova.** `cost::tests::cost_uses_price_in_force_when_call_happened`; `tests/telephony.rs` (mudança de preço a 15/09, consumo em USD sem e com taxa); contra o FreeSWITCH real, o CDR atendido fica com `8.9000 AOA` e o falhado com `0.0000`.

**Ficheiros.** `server/crates/delonix-meet-domain/src/telephony/{cost,money}.rs`, `server/src/telephony_{cdr,trunks,calls}.rs`.

### R213 — Um tronco SIP para um endereço interno era SSRF por SIP, e dois inquilinos com o mesmo domínio trocavam de troncos

**Risco.** O host de um tronco é escrito pelo cliente e é o FreeSWITCH que liga a ele: um tronco para `10.0.0.5` ou `169.254.169.254` fazia a plataforma abrir ligações para dentro. E o domínio SIP decide a org de uma chamada que entra pelo PBX: medido contra o FreeSWITCH real, com dois inquilinos em `127.0.0.1`, a chamada de uma org saiu pelos troncos da outra.

**Regra.** O host passa por `net_guard::check_tenant_config_url` ao criar e ao alterar (`telephony.trunk_host_refused`; excepções só por `OUTBOUND_ALLOW_HOSTS`). SRTP diferente de `off` exige TLS (`telephony.srtp_requires_tls`). O domínio SIP é único na base (`lower(domain)`, `409 telephony.sip_domain_taken`).

**Prova.** `tests/telephony.rs::trunks_crud_order_prices_secrets_and_isolation` (host `10.0.0.5` recusado); `web/e2e/telefonia-freeswitch.mjs` (domínio próprio por org, a chamada do PBX sai pelos troncos certos).

**Ficheiros.** `server/src/telephony_trunks.rs`, `server/src/telephony_sip.rs`, `server/migrations/0071_telephony_sip_settings.sql`.

### R214 — Credenciais SIP e de operadora legíveis sem reautenticação

**Risco.** A password de um tronco e a da conta SIP da org são credenciais de terceiros; uma sessão roubada de um admin chegava para as ler, e sem trilha.

**Regra.** Cifradas em repouso (`secret_box`, aad por linha) e NUNCA devolvidas nas listas nem no `GET` (`password_configured` só). A única saída da password SIP é `POST …/sip-settings/reveal-credentials`, com a password da conta (ou código MFA para contas sem password local), 5 falhas em 5 min bloqueiam (inclusive a certa), e sucesso e falha vão para a auditoria. A password dos troncos só sai decifrada para o FreeSWITCH, no listener interno com `VOICE_INTERNAL_SECRET` (`purpose=gateways`).

**Prova.** `tests/telephony.rs::sip_credentials_are_revealed_only_after_reauth_and_audited`, `trunks_crud_order_prices_secrets_and_isolation`; `web/e2e/isolamento.mjs` (A não obtém as credenciais de B com a sua própria password; o tronco de B continua sem a password na resposta).

**Ficheiros.** `server/src/telephony_sip.rs`, `server/src/telephony_trunks.rs`, `server/src/telephony_fs_xml.rs`.

### R230 — O directo multidestino só funcionava no primeiro destino

**Sintoma.** Com 2 ou mais destinos, o segundo em diante era recusado pelo servidor RTMP (`unsupported video codec: 2`, medido com mediamtx a 2160p/16 Mbit pela sessão da frente E). O `montar_argumentos` punha `-c:v copy -c:a aac -b:a 128k -ar 44100` UMA vez, antes da primeira saída — e no ffmpeg as opções de saída valem só para a saída seguinte. As restantes saíam com os codecs por omissão do FLV: vídeo FLV1 re-codificado em software (o custo que o ADR-0003 existe para evitar) e áudio MP3. O teste existente só contava as saídas `flv`, não o que cada uma levava.

**Prova.** ffmpeg real, a mesma entrada Matroska H.264+Opus por cano, dois ficheiros FLV: com os argumentos antigos, `ffprobe` dá `h264 aac` na 1.ª saída e `mp3 flv1` na 2.ª; com os novos, `h264 aac` nas duas.

**Regra.** As opções de codec vêm de `opcoes_de_saida()` e repetem-se antes de CADA `-f flv`. Portão: `broadcast::testes::cada_saida_leva_as_suas_opcoes_de_codec` (1, 2 e 3 destinos; cada saída tem de ter o seu `-c:v copy`, `-c:a aac` e `-ar` desde a saída anterior). Continua por resolver, e é da frente E (ADR-0013): um destino pendurado congela os outros, porque é um só processo.

**Ficheiros.** `server/src/broadcast.rs`.

### R190 — «É admin» era uma string comparada em 52 sítios; não havia dono, papéis nem capacidades (ADR-0008)

**Sintoma.** `org_members.role` só tinha `admin | member`. Quem criava a org não era distinguível de outro admin, um admin podia arquivar o criador, e nenhum papel intermédio (gestor de emissão, formador) existia sem dar tudo. Três módulos decidiam por texto (`voice.rs:444`, `recordings.rs:269`, `odoo_sso.rs:415`).

**Regra.**
- **Catálogo fechado** `delonix_meet_domain::identity::authorization::Capability` (21, `CATALOG_VERSION = 3`), policy pura `can()`, valores `allow | deny | inherit | requires_approval`. Capacidade com `enforced: false` só aceita o valor por omissão (`422 authz.capability_not_enforced`); `org.administer` nunca num papel personalizado.
- **Ponto único** `org::require_capability` (uma query sobre `org_role_effective_capabilities`, calculada pela policy na transacção de cada escrita de papéis). `require_admin` = `org.administer`. Estranho → `404`; sem capacidade → `403 authz.missing_capability` (antes `permission_denied`).
- **Papéis de sistema imutáveis** com a semântica de hoje: `owner`/`admin` tudo, `member` só `sessions.create` e `recordings.record_4k`, `external_guest` nada.
- **Pontos migrados:** membros (`admin.manage_accounts`), auditoria (`admin.view_audit`), destinos de emissão (`broadcast.manage_rtmp_keys`), directo com destinos guardados (`broadcast.public_destinations` + limite do papel), definições (`admin.change_retention`), salas/reuniões BFF e v1 (`sessions.create`), partilhas e link público (`recordings.publish` dentro de `owned_item`: dono activo OU a capacidade; vê sem ela → `403 authz.missing_capability`, não vê → `404`; alargamento intencional a `owner`/`admin`), facto `org_admin` da biblioteca (`recordings.view_others`).
- **Catraca nova** `verificacoes_papel_por_string_fora_de_org_rs` (medida sobre a base com o #90: 2 — `voice.rs:444` e `whiteboards.rs:360`).

**Portão.** `server/tests/rbac.rs` (`migrated_points_keep_their_status_table`, `no_escalation`, `system_roles_and_unenforced_fields_are_locked`, `materialized_decisions_equal_policy`, `department_scoped_role`, `sessions_create_is_enforced`, `recordings_publish_and_view_others`, `new_routes_are_isolated`, `seeded_system_defaults_match_the_domain`) e unitários de tabela em `authorization/tests.rs`.

**Ficheiros.** `server/crates/delonix-meet-domain/src/identity/authorization{.rs,/tests.rs}`, `server/migrations/0055_org_roles.sql`, `server/src/{org,roles,audit,stream_destinations,broadcast,recordings,rooms,meetings,meetings_v1,apikeys}.rs`, `scripts/check-arquitectura-catraca.sh`.

### R191 — Uma escrita herdada de `role = 'member'` esmagava em silêncio um papel personalizado

**Sintoma.** Com `role_id` como fonte, os escritores herdados (`add_employee … DO UPDATE SET role`, `update_employee`, o «nunca despromove» da sincronização Odoo) voltavam a escrever o texto e deixavam `role_id` e `role` a dizer coisas diferentes.

**Regra.** `role` é derivado de `role_id` por gatilho num só sentido (0055). `INSERT` só com `role` recebe o papel de sistema; `UPDATE` que mude `role` sem mudar `role_id` levanta excepção. Os escritores que alteram papel chamam `org::set_system_role`. O último dono activo (com humanos activos) é protegido no serviço (`409 role.last_owner`) e por um gatilho de restrição adiado. O utilizador de serviço nunca é dono nem ocupa lugar.

**Portão.** `tests/rbac.rs::legacy_role_update_cannot_overwrite_role_id`, `last_owner_and_owner_assignment`, `no_legacy_role_updates_in_source` (varre `src/`).

### R192 — «Requer aprovação» tem de criar um pedido e nunca executar; a aprovação serve uma vez e para um alvo

**Regra.** `approval_requests` ligados a `(capacidade, acção, SHA-256 do alvo canónico)`; consumo atómico (`UPDATE … WHERE status = 'approved' AND expires_at > now() RETURNING`); mudar papel, departamento ou estado de quem pediu invalida o pendente e o aprovado; sem auto-aprovação; sweeper de expiração; auditoria de cada passo.

**Portão.** `tests/rbac.rs::requires_approval_flow`, `approval_is_consumed_once_under_concurrency`.

### R193 — O convite é a credencial: token em hash, uso único, sem capturar contas de outra org

**Regra.** Só o SHA-256 do token é guardado; aceitar faz `SELECT … FOR UPDATE` e consome na mesma transacção; o correio da sessão tem de coincidir (defesa em profundidade — o registo não verifica o correio); rate-limit por IP; conta activa noutra org só entra como `external_guest`; reenviar roda o token; `removed`/`odoo_exit` voltam por convite, nunca por «reactivar». Sem SMTP: `delivery_channel: manual`.

**Portão.** `tests/directory.rs::invitation_acceptance_rules`, e o bloco ADR-0008 de `web/e2e/isolamento.mjs`.

### R194 — Duas activações concorrentes passavam o tecto de lugares

**Regra.** O tecto (`organizations.max_seats`, só o operador o fixa) verifica-se com `SELECT … FROM organizations … FOR UPDATE` dentro da transacção que activa (convite aceite, reactivação, `add_employee` novo). O uso mede-se na hora (sem contador): activos humanos que não são `external_guest`.

**Portão.** `tests/directory.rs::seats` (duas reactivações em simultâneo com um lugar livre → uma entra, a outra `seats.limit_reached`).
### R154 — O segredo da API interna de IVR estava escrito no manifesto de um repositório público

**Sintoma.** Nenhum para quem usa o produto. `deploy/k8s/01-config.yaml` trazia `VOICE_INTERNAL_SECRET: "voice-internal-secret-for-pstn"` desde 98f5b28 (2026-07-10), e as rotas `POST /api/voice/ivr/validate` e `POST /api/voice/ivr/cdr` estão no router PÚBLICO, protegidas só pelo cabeçalho `X-Voice-Secret`. Em qualquer deploy que tenha aplicado esse manifesto, quem lesse o GitHub validava PINs de dial-in (limitado a 10 falhas por DID em 5 min) e injectava CDRs — minutos e custo — na org de qualquer sala de voz cujo UUID conhecesse. Medido por leitura (2026-09-16, `origin/main` 4ff5249); não foi explorado contra um cluster.

**Causa raiz.** Duas metades. (1) O Secret de stage estava versionado com valores literais, e o valor de voz ficou lá como se fosse de exemplo. (2) O servidor não tinha chão para este segredo: ao contrário do `JWT_SECRET`/`TURN_SECRET` (`config::secret`, panic sem valor forte), o `VOICE_INTERNAL_SECRET` era lido com `unwrap_or_default()` e qualquer valor não vazio — curto ou publicado — autenticava.

**Regra.**
- O `VOICE_INTERNAL_SECRET` **nunca** está num ficheiro versionado. Em K8s vem do Secret `delonix-voice` por `secretKeyRef` (`optional: true`) no `02-server.yaml`; o `make stage`/`make prod` criam-no aleatório (`make voice-secret-k8s`) e o Ansible `k8s_app` a partir do `voice_secret` gerado em `deploy/ansible/.secrets/`.
- **Fail-closed sem partir o servidor.** `config::voice_secret_refusal` decide UMA vez no arranque: vazio, `< 32` caracteres, ou valor da lista `BURNED_VOICE_SECRETS` → as rotas de IVR dão `503` com a razão, venha o cabeçalho que vier, e o arranque avisa. Não é panic como o JWT: quem não usa voz não perde o servidor. Com `DELONIX_ALLOW_INSECURE=1` aceita-se qualquer valor não vazio (é o `make dev`).
- Ordem: primeiro o segredo configurado (`503`), depois o cabeçalho (`401`). Um cabeçalho «certo» contra um valor publicado não autentica ninguém. Comparação com `apikeys::ct_eq`, não uma cópia local.
- **Não se reescreve o histórico** (repositório público, force-push parte clones e PRs, e não tira o valor a quem já o tem). A decisão e a rotação obrigatória estão em `scripts/leaked-secrets-accepted.txt`, e o `check-repo-hygiene.sh` recusa que um valor desse livro volte a um ficheiro seguido.
- **Armadilha do `kubectl apply`:** tirar uma chave do `stringData` não a apaga do Secret já existente. Um cluster antigo continua com o valor publicado dentro do `delonix-secrets` — é por isso que o servidor tem de o recusar por valor, e não basta mudar o manifesto. Limpeza e rotação em `docs/deployment.md` §6.

**Portão.** `voice::tests` — `ivr_refuses_missing_short_or_burned_secret_with_503`, `ivr_rejects_wrong_or_absent_header_with_401`, `ivr_accepts_the_right_strong_secret`, `insecure_dev_keeps_the_dev_value_but_not_empty` (verificado a falhar com a guarda do segredo desligada); `scripts/check-repo-hygiene.sh` ponto 7 (verificado a falhar com o valor reposto no `01-config.yaml`). Não há teste contra servidor real nem contra um cluster: a camada de media (FreeSWITCH) nunca correu (ver `voice/README.md`).

**Fora deste passo.** O mesmo `01-config.yaml` continua a versionar `PROVISIONING_SECRET`, `JWT_SECRET`, `TURN_SECRET` e a password do Postgres de stage — mesma classe, não tratada aqui. Na linha ADR-0004 (`delonix-meet-backend/backend-enterprise`), o `grpc.rs` e o `odoo.rs` testam só `voice_internal_secret.is_empty()`: ao juntar, têm de passar a respeitar `voice_secret_refusal`.

**Ficheiros.** `server/src/{config,voice}.rs`, `deploy/k8s/{01-config,02-server}.yaml`, `deploy/ansible/roles/k8s_app/templates/app-config.yaml.j2`, `Makefile` (`voice-secret-k8s`), `scripts/{check-repo-hygiene.sh,leaked-secrets-accepted.txt}`, `docs/deployment.md`, `voice/README.md`.

### R231 — A acta e a transcrição chegavam ao LLM e ao webhook sem passar pelo DLP

**Sintoma.** O DLP (`dlp::censor`) corria no que é DIFUNDIDO — chat e legendas ao vivo (`signaling.rs`) — e no que o worker de transcrição entrega (`transcription.rs`). Não corria no que o CLIENTE acumula na sala e envia no fim: `PUT /api/meetings/{meeting_id}/minutes` gravava `minutes` e `transcript` tal como vinham. Um cartão de crédito ou um NIF ditos em voz alta ficavam na base, entravam no prompt de `ai::summarize_minutes` (Ollama local) e saíam da casa no webhook `meeting.mom_ready` para o Odoo. O `ai.rs` não tinha UMA chamada ao DLP: `POST /api/ai/translations` também mandava texto do cliente directo para o modelo.

Vinha assinalado desde o PR #68 (2026-09-16), que nunca foi integrado; o código mudou de sítio entretanto (rotas novas, worker por gRPC) e o buraco ficou.

**Regra.** O DLP corre À ENTRADA, onde o texto é gravado (`meetings::save_minutes`, que serve também o `PUT /api/rooms/{room_code}/minutes`), e OUTRA VEZ no prompt, antes de o texto sair do processo para o LLM — as duas funções de prompt (`ai::caption_prompt`, `ai::minutes_prompt`) censuram e são puras, por isso provam-se sem Ollama. Censurar duas vezes é barato; censurar zero vezes foi isto.

**Portão.** `ai::tests::{o_prompt_da_legenda_vai_censurado, o_prompt_do_resumo_vai_censurado, a_janela_do_resumo_guarda_o_fim}` e `server/tests/dlp_antes_do_llm.rs` contra Postgres real (a acta e a transcrição ficam censuradas na BASE, com controlo positivo de que o resto do texto sobrevive). Mutação verificada: sem a censura em `save_minutes`, o teste falha com o cartão gravado.

**Não fechado aqui.** Injecção de prompt (OWASP LLM01) e a sanitização do Markdown que o LLM devolve, no visualizador.

**Ficheiros.** `server/src/ai.rs`, `server/src/meetings.rs`, `server/tests/dlp_antes_do_llm.rs`.

### R234 — Duas migrações de gravações não fizeram nada, em silêncio, e o servidor lia colunas que não existiam

**Sintoma.** `POST /api/recordings/{id}/chapters/generate` e `PATCH …/chapters/{id}` respondiam `500`. O `recording_chapters.rs` lê `t_ms` e `source` de `recording_chapters`; a tabela tinha `at_secs` e nenhum `source`. Medido a 2026-09-29 contra Postgres 17 real, aplicando as migrações da `main` por ordem a uma base limpa: `\d recording_chapters` dava `at_secs integer NOT NULL` e `recording_comments` dava `author_id`, quando as migrações 0060 e 0061 diziam declarar `t_ms`, `source` e `user_id`.

**Causa raiz.** As 0060 e 0061 escreveram a forma nova como `CREATE TABLE IF NOT EXISTS`. A 0045 já tinha criado as duas tabelas, com o modelo antigo (G5). `IF NOT EXISTS` não é um upsert de esquema: a tabela existia, as instruções não fizeram nada, e não houve erro. As migrações passaram verdes em todas as bases, incluindo as dos testes — nenhum teste tocava nas duas rotas que liam as colunas novas. (Da 0061 só `recording_views` chegou a nascer, por ser tabela nova.)

**Regra.** Uma migração que ALTERA uma tabela existente usa `ALTER`, e converte os dados. `CREATE TABLE IF NOT EXISTS` só para uma tabela nova — num ficheiro que também mexe numa antiga, é um no-op à espera de acontecer. A 0068 converte a sério: `at_secs` → `t_ms` (afastando 1 ms os capítulos que partilhavam o mesmo segundo, em vez de os apagar), `author_id` → `user_id`, `source`, títulos até 200 caracteres, autor opcional, e unicidade por `(recording_id, t_ms)`.

**Portão.** `server/tests/recordings_metadata.rs::{chapters_crud_bounds_and_access, comments_crud_author_only_soft_delete_and_dlp}` passam a escrever e a ler em `t_ms`/`source`/`user_id` contra Postgres real — o que falhava antes da 0068 com «column does not exist».

**Não fechado.** Não se procurou a mesma classe de defeito nas outras 67 migrações; só as duas das gravações foram verificadas coluna a coluna contra o esquema real.

**Ficheiros.** `server/migrations/0068_recording_chapters_comments_ms.sql`, `server/src/{recordings,recording_chapters}.rs`, `server/tests/recordings_metadata.rs`.

### R235 — Publicar uma gravação para a organização não a mostrava a ninguém

**Sintoma.** A dona de uma gravação carregava em «publicar», a consola confirmava, `visibility` passava a `org` e `published_at` ficava preenchido — e nenhum colega a via em lado nenhum. `GET /api/recordings?scope=published` não estava implementado: o parâmetro era ignorado e a biblioteca devolvia sempre a pessoal. O próprio comentário no `recordings.rs` da `main` admitia-o por escrito («o `scope=published` … NÃO está implementado»). Medido a 2026-09-29 contra Postgres real.

**Causa raiz.** A publicação entrou pela migração 0062 e pelas rotas `publish`/`unpublish` (`recording_meta.rs`), mas a LEITURA ficou por fazer quando dois modelos de gravação foram reconciliados (R183). Duas colunas que só se escreviam.

**Regra.** `AccessFacts` ganha `published_to_my_org` (publicada para a organização **e** quem pede é membro ACTIVO de uma organização do autor) e `can_view` passa a contá-lo. `?scope=` escolhe a biblioteca: `mine` (omissão) é a de sempre — carregou, participou, ou foi-lhe partilhada — e `published` são as publicadas que quem pede vê, incluindo as de salas onde nunca esteve. Publicar **não** enche a biblioteca pessoal dos colegas (`listed_in`), **não** dá o ficheiro (`can_download`) nem o poder de gerir, e **não** abre a transcrição nem a lista de presentes (`has_direct_relation`) — publicar é para ser VISTO. Um `scope` desconhecido é `400 recording.invalid_scope`, não «tudo» em silêncio.

**Portão.** `server/tests/recordings_metadata.rs::{published_library_reaches_the_org_and_nobody_else, publishing_does_not_open_transcript_nor_participants, library_scopes_agree_with_access_facts}` e os testes de unidade do domínio. O último percorre cada pessoa contra cada gravação e exige que o SQL que filtra antes de paginar diga o mesmo que `listed_in` — a regra está escrita duas vezes de propósito, e é assim que uma fica para trás.

**Não fechado.** A biblioteca publicada não tem filtro por etiqueta nem por autor, e não há notificação a quem quer que seja quando uma gravação é publicada.

**Ficheiros.** `server/src/recordings.rs`, `server/src/recording_meta.rs`, `server/crates/delonix-meet-domain/src/content/recording.rs`, `server/tests/recordings_metadata.rs`.

### R236 — A duração de uma gravação carregada nunca aparecia

**Sintoma.** Uma gravação carregada pelo browser aparecia sempre sem duração e sem resolução na biblioteca e no leitor — mesmo depois de o servidor as ter medido com `ffprobe` e guardado. Medido a 2026-09-29: o upload escrevia `duration_ms` (migração 0057, via `media_probe::probe_and_store`) e a listagem servia `duration_secs` (migração 0045), uma coluna que só o gravador do servidor preenchia.

**Causa raiz.** Duas colunas para a mesma grandeza, de duas gerações do modelo (G4 e o contrato novo), e cada caminho escolheu uma. A consola já lia `duration_ms` (`web/src/api.ts`), por isso o campo que o servidor mandava nem sequer era lido: a UI mostrava «—» com a duração na base a um campo de distância.

**Regra.** O item da biblioteca é o `RecordingLibraryItem` que a consola lê, e a unidade é o MILISSEGUNDO em todo o contrato — item, capítulos (`t_ms`) e comentários (`t_ms`). `null` continua a ser «não foi possível medir», nunca zero.

**Portão.** `server/tests/recordings_metadata.rs::uploaded_recording_shows_measured_duration_and_resolution`: um webm de 2 s e 320x240 feito pelo `ffmpeg` é carregado, e a duração e a resolução medidas aparecem na resposta do upload, no recurso e na listagem; e o capítulo em `t_ms = 2000` passa enquanto o de `2001` é recusado, o que prova que a duração medida chega às validações.

**Não fechado.** `duration_secs` e `category` continuam na tabela, já sem ninguém que as leia — a limpeza fica para uma migração própria. As gravações ANTIGAS, carregadas antes da 0057, continuam sem `duration_ms`: não foram medidas retroactivamente, e mostram-se sem duração.

**Ficheiros.** `server/src/recordings.rs`, `server/tests/recordings_metadata.rs`.

### R237 — Procurar um instante no leitor puxava a gravação inteira

**Sintoma.** `GET /api/recordings/{id}/content` respondia `200` com o ficheiro todo a um pedido `Range: bytes=0-1023`, e sem `Accept-Ranges`. O `<video>` pede um intervalo, recebe tudo, conclui que o servidor não sabe servir intervalos e desiste de procurar: cada salto na barra volta a descarregar a gravação inteira. Numa gravação de uma hora são centenas de MB por clique. Medido a 2026-09-29.

**Causa raiz.** O handler lia o ficheiro com `tokio::fs::read` e devolvia os bytes. Nunca olhou para o cabeçalho `Range`.

**Regra.** O ficheiro honra `Range` (RFC 9110 §14): `206` com `Content-Range: bytes <início>-<fim>/<total>` para uma faixa única — do princípio, aberta à direita (`bytes=1024-`) ou por sufixo (`bytes=-500`) —, com um fim para lá do ficheiro CORTADO em vez de recusado; `416` com `Content-Range: bytes */<total>` para um intervalo bem escrito mas fora do ficheiro; e o ficheiro inteiro (`200`) sem cabeçalho, com várias faixas (não se serve `multipart/byteranges`) ou com um cabeçalho que não se percebe. `Accept-Ranges: bytes` vai em TODAS as respostas, também nas de `200` — é assim que o leitor sabe que pode pedir um intervalo da próxima vez. O `Range` corre DEPOIS do controlo de acesso: não é um caminho paralelo ao RBAC.

**Portão.** `server/tests/recordings_metadata.rs::content_honours_range_requests`, com as cinco formas de intervalo, o `416`, os três casos que caem no ficheiro inteiro, e a prova de que o `Range` não contorna o RBAC (`403` no download sem permissão, `404` para outra organização).

**Não fechado.** A fatia é lida para memória antes de sair (`read_exact`), como já era o ficheiro inteiro: não há streaming. Para os intervalos que um leitor pede (KB a MB) é menos memória do que antes, mas um pedido de uma faixa enorme continua a alocar essa faixa. Não há `ETag`, `Last-Modified` nem `If-Range`, por isso um cliente não revalida uma fatia em cache.

**Ficheiros.** `server/src/recordings.rs`, `server/tests/recordings_metadata.rs`.

### R232 — Uma chave de API da v1 listava as gravações privadas de qualquer membro da organização

**Sintoma.** `GET /api/v1/recordings` devolvia todas as gravações cujo autor é membro da organização da chave, incluindo as que o autor nunca publicou. Na BFF, um colega só vê as gravações de outra pessoa quando ela as publica para a organização (R235); pela v1, a mesma organização via tudo.

**Causa raiz.** A consulta juntava `recordings` a `org_members` pelo autor e parava aí. Uma chave representa a ORGANIZAÇÃO inteira, não um utilizador com relação directa à gravação — e a regra de «o que a organização vê» (`AccessFacts::listed_in(Published, …)`) não estava na consulta.

**Regra.** A v1 lista só as gravações publicadas para a organização (`visibility = 'org'` e `published_at` preenchido): exactamente o que um colega qualquer vê na biblioteca «publicadas», nunca uma gravação privada de outro membro só porque partilham organização. É uma mudança de comportamento para integrações que contavam com a lista inteira: passam a ver uma gravação quando o autor a publica.

**Portão.** `server/tests/api_v1.rs::v1_recordings_list_scoped_to_org`: a privada fica fora, a publicada aparece, a outra organização continua sem nenhuma.

**Dois achados menores da mesma revisão.** (1) Os segmentos da transcrição eram cortados a 2000 caracteres ANTES de o DLP correr: uma chave ou um cartão a atravessar essa fronteira ficava partido ao meio e a expressão regular deixava de o reconhecer. Censura-se o texto bruto primeiro e corta-se depois (`transcription::complete`; portão `tests/grpc.rs::dlp_runs_before_truncating_a_segment_that_straddles_the_limit`). (2) `PATCH …/chapters/{chapter_id}` marcava sempre `source = 'manual'`, mesmo com um corpo vazio (resave, retry): um capítulo automático perdia a elegibilidade para a geração seguinte sem nenhuma correcção ter acontecido. Só passa a manual quando `t_ms` ou `title` vêm no pedido (portão em `tests/recording_chapter_generation.rs`).

**Ficheiros.** `server/src/{apikeys,transcription,recording_chapters}.rs`, `server/tests/{api_v1,grpc,recording_chapter_generation}.rs`.

### R240 — O `/asr` do whisper aceitava qualquer ligação, sem autenticação nenhuma

**Sintoma.** O `whisper-server` publica `WebSocket /asr?lang=…` e está no MESMO ingress público do resto. A ligação era aceite sem verificar nada: quem alcançasse o endereço tinha transcrição por GPU à borla, e podia esgotar o modelo partilhado com ligações de propósito. A mesma ligação devolvia ao cliente a mensagem crua de qualquer excepção Python (`str(e)[:200]`) — caminhos e nomes internos incluídos, que é reconhecimento grátis para quem provoca o erro de propósito.

**Regra.** O `/asr` valida o MESMO access token do `/rtc` (HS256, claim `typ = access`, `auth.rs`) e fecha com `1008` ANTES do `accept()` — uma ligação nunca aceite não gasta um slot de transcrição. Sem `JWT_SECRET` definido não há degradação para «sem autenticação»: recusa na mesma. O detalhe de uma excepção vai para o log do servidor; ao cliente vai «erro interno». O browser passa o token na query (`media.ts`), como já fazia no `/rtc`.

**Prova.** `whisper-server/app.py` compila; `tsc -b` limpo com a mudança do cliente. **Não corrido contra um whisper-server real nem contra o ingress** — a verificação é de código e de tipos.

**Ficheiros.** `whisper-server/app.py`, `whisper-server/requirements.txt` (PyJWT), `deploy/k8s/09-whisper.yaml` (o `JWT_SECRET` vem do configmap partilhado), `web/src/media.ts`.

**Origem.** Estava no PR #68 (2026-09-16), que nunca foi integrado; o espelho do DLP em Python que vinha no mesmo PR NÃO entra, porque na `main` o worker entrega por gRPC e o servidor censura à chegada (R182, R231).

### R172 — A publicação morria poucos milissegundos depois de nascer, com entradas concorrentes

**Sintoma.** Medido a 2026-09-17 com clientes WebRTC reais (`server/examples/loadgen.rs`, hoje na `main` pelo #122): 8 salas × 4 participantes, entradas a 40 ms, **75–87 de 96** fluxos de vídeo chegavam, 0 % de perda nos que chegavam, e `delonix_sfu_subscriptions` ficava em 165 em vez de 192. Salas inteiras deixavam de ver o mesmo publicador. A causa não era a subscrição: a publicação morria 30–65 ms depois de nascer — `read_rtp` devolvia `buffer: closed` e o servidor fazia `unpublish` para a sala toda com o publicador ainda a enviar.

**Causa.** No webrtc-rs (0.17.1 e também 0.17.2, verificado no código do crate), um pacote de um SSRC **declarado** no SDP remoto que chegue entre a sessão SRTP nascer e o `start_rtp` abrir os receivers faz a sessão criar o stream sozinha e anunciá-lo como media «não declarada». A sonda de simulcast sonda-o, não encontra `rid`, e o fecho no fim da sonda fecha o MESMO `Arc<Stream>` que o `start_rtp` entretanto abriu para o receiver. A assinatura nos logs é `Incoming unhandled RTP ssrc(…) … failed Simulcast probing`, escondida pelo filtro `delonix_server=info`.

**Regra.** Um SSRC declarado no SDP remoto não passa pela sonda: o `server/vendor/webrtc` (0.17.2 vendorizado, patch de ~30 linhas em `peer_connection_internal.rs`) devolve cedo, deixando o stream para o receiver sem o ler nem o fechar. O critério é o mesmo com que o `start_rtp` decide que receivers abrir (`track_details_from_sdp`, SSRC ou repair SSRC). O `sfu.rs` passa a registar a razão por que a bomba de RTP terminou — sem ela, «track unpublished» não distingue o publicador que saiu de um stream fechado por baixo.

**Portão.** `sfu_e2e::entradas_concorrentes_todos_recebem_todos`: 16 clientes em 4 salas, verifica por par (subscritor, publicador, tipo) que o RTP chega, repete a verificação e confere o gauge. O cliente de teste negoceia as extensões RTP de um browser — **sem `sdes:mid` a sonda desiste antes de fechar e o defeito fica invisível ao teste**.

**Escala do teste.** O cenário dimensiona-se pelos núcleos disponíveis: 4 salas × 4 numa máquina de desenvolvimento, 2 salas × 4 num runner de 2 vCPU. O defeito é POR SALA — quatro entradas ao mesmo tempo na mesma sala chegam para o provocar —, e 16 `RTCPeerConnection`s reais num runner pequeno deixavam de medir o SFU para medir a máquina (a 2026-09-29 o CI da `main` ficou vermelho assim, com um subscritor sem receber nada e as 96 subscrições feitas). Subir o prazo não servia: o próprio aviso do teste diz que mais tempo não liga um ICE que já desistiu. **Verificado que a escala menor NÃO enfraquece o portão**: com `taskset -c 0,1` e sem o patch, o teste falha na mesma com `media deixou de chegar`.

**Prova refeita nesta árvore (2026-09-29).** Com o patch: passa. Sem o patch (retirado só o bloco do early-return): falha com `media deixou de chegar: ["…←…:audio"]`. **NÃO foi repetida a corrida do gerador de carga** — os 75/96 → 96/96 são a medição de 17 de setembro, noutra árvore.

**Ficheiros.** `server/vendor/webrtc/` (crate 0.17.2 vendorizado + o patch), `server/Cargo.toml` (`[patch.crates-io]`), `server/src/sfu.rs`, `server/src/sfu_e2e.rs`, `HARNESS.md`.

### R221 — Quem entrava por telefone nunca ouvia a sala, e a sala nunca o ouvia

**Sintoma.** O dial-in PSTN estava ligado desde a Fase 1: o chamador marcava o DID, digitava o PIN, e entrava — numa conferência **local do FreeSWITCH**. Ouvia os outros chamadores PSTN e mais ninguém. Os participantes WebRTC da MESMA reunião não o ouviam nem eram ouvidos. Não era uma falha intermitente: era o comportamento permanente, com a ponte do lado do SFU escrita, testada e desligada (ver R222 para a razão por que estava desligada).

**Regra.** Uma perna de telefone é um **publicador como outro** no SFU: `server/src/phone_bridge/` descodifica G.711 (PCMA/PCMU) do telefone, codifica Opus para a sala, e devolve ao telefone a mistura da sala **menos a própria voz** (mix-minus — sem isso o chamador ouve-se com o atraso da volta, que é o eco clássico). O jitter é absorvido por uma fila com relógio próprio de 20 ms (`phone_bridge::audio`), porque o RTP do telefone não chega alinhado com o do SFU. Um pacote de um IP que não está na allowlist **não entra na sala**, mesmo com o resto do pacote correcto.

**Portão.** `sfu_e2e::ponte_telefone_sala_tom_nos_dois_sentidos`: um «telefone» (socket UDP com RTP G.711 lei μ a 1 kHz) e um participante `RTCPeerConnection` real (webrtc-rs, Opus a 440 Hz) na mesma sala. Exige, com descodificação real dos dois lados: (1) o participante recebe Opus descodificável com o 1 kHz; (2) o telefone recebe G.711 com os 440 Hz; (3) o telefone **não** recebe o próprio 1 kHz; (4) um pacote de outro IP não entra. Imprime o atraso de cada sentido e o CPU de codecs por chamada.

**Âmbito do portão.** O transporte até à ponte é um socket local: sem FreeSWITCH, sem operadora e sem SRTP. A cadeia real é a R222.

**Ficheiros.** `server/src/phone_bridge/{mod,audio,g711,leg,quality}.rs`, `server/src/sfu.rs` (`PubSource::Bridge` — uma publicação deixa de ser sempre uma track remota), `server/src/sfu_e2e.rs`, `server/Cargo.toml` (sai `audiopus`, que se ligava à `libopus` do sistema; entra `opus-rs`, Rust puro — a imagem distroless deixa de precisar da lib).

### R222 — A ponte assentava num mecanismo que o FreeSWITCH não tem, e por isso nunca foi ligada

**Sintoma.** A Abordagem B (`docs/pstn-sfu-bridge-design.md`, `pstn_bridge.rs`, 2026-09-19) fazia o SFU escutar RTP/SRTP **cru** num par UDP, com as chaves entregues por fora no JSON do IVR. O lado do SFU foi escrito e testado. O lado do FreeSWITCH nunca chegou a existir: o `dialin_ivr.lua` ficou com a pergunta escrita no topo — qual é o verbo do FreeSWITCH que manda RTP cifrado com uma chave dada por fora para um endereço arbitrário, sem segundo diálogo SIP — e, sem resposta, a fazer apenas um `consoleLog` antes de cair na conferência local. Isto é o caso em que o código existe, os testes passam, e **o cliente não tem a funcionalidade**.

**Causa.** Não há esse mecanismo num FreeSWITCH 1.11.3 de stock — lido na imagem pelo trabalho que construiu a ponte, e não reverificado ao trazê-la para a `main`. O `mod_audio_fork` manda áudio por WebSocket para STT, não RTP bidireccional; `uuid_deflect`, `snoop` e `unicast` fazem outra coisa. O que o FreeSWITCH faz bem é originar uma **segunda perna SIP** — que é o que a Abordagem B evitava de propósito, para não pôr sinalização nova no SFU.

**Regra.** O SFU ganha o shim que a própria pergunta antecipava: um UA SIP mínimo (`phone_bridge::sip`) que só **atende** — não regista, não origina, não fala SIP para fora. As chaves SRTP passam a ser negociadas **no SDP, por chamada** (SDES), em vez de distribuídas por JSON: uma oferta sem `a=crypto` leva `488`, e uma sem G.711 também. A allowlist de origens é **fail-closed**: vazia, o UA nem arranca (`voice::start_phone_bridge`). O IVR deixa de receber `pstn_bridge` (host, porta, chaves) e passa a receber `room_bridge` (para onde fazer `bridge` e que variáveis pôr antes) — e **executa-o**, com recuo para a conferência local se falhar.

**Portão.** `sfu_e2e::ponte_com_freeswitch_real_tom_nos_dois_sentidos`, contra um **FreeSWITCH 1.11.3 real** (`scripts/fs-canais.sh up`): `originate` por ESL → gateway → «telefone» que atende, grava a chamada e toca 1 kHz → `bridge` SIP → UA da ponte → SFU → participante webrtc-rs a publicar 440 Hz. Exige o `X-Delonix-Call-Id` no `INVITE`, o 1 kHz audível na sala, os **440 Hz da sala dentro da gravação que o FreeSWITCH fez do lado do telefone**, e o `BYE` a tirar a perna da sala. **Fora do CI** (`scripts/e2e-fora-do-ci.txt`): precisa da imagem `delonix-dev/freeswitch:1.11.3`, que o CI não alcança.

**Prova corrida a 2026-09-30.** `originate`→atendida 328 ms; `200 OK` do UA 330 ms; 1 kHz do telefone na sala, mediana 0,1548 por pacote; gravação do telefone de 5,0 s a 8 kHz, dois canais — recebido com 440 Hz a **0,2495** e 1 kHz a **0,0001** (a sala chega-lhe, a própria voz não), enviado com 1 kHz a 0,1554. **Por medir:** uma operadora a sério através do Kamailio, e um browser em vez do cliente webrtc-rs.

**Ficheiros.** `server/src/phone_bridge/{sip,srtp}.rs`, `server/src/voice.rs` (`RoomBridgeResp`, `DialInAdmission`, `start_phone_bridge`), `server/src/lib.rs` (arranque), `voice/freeswitch/scripts/dialin_ivr.lua` (o `bridge` a sério), `voice/freeswitch/canais-prova/`, `scripts/fs-canais.sh`, `docs/adr/0010-ponte-telefone-sala.md`. Sai `server/src/pstn_bridge.rs`.

### R223 — O IVR dependia de módulos que a imagem do FreeSWITCH não tinha, e ninguém lia o Lua

**Sintoma.** Os dialplans do Meet chamam `dialin_ivr.lua` e `ramais_dial.lua`, que fazem `curl` ao control plane. A imagem local `delonix-dev/freeswitch:1.11.3` — a que provou a R222 — foi compilada **sem `mod_lua` nem `mod_curl`**: o IVR não corria nela. A R222 não o apanhou porque o seu dialplan (`canais-prova/`) não passa pelo Lua. A imagem vivia fora do repo (`.worktrees/freeswitch-build/`), com o sofia-sip e o spandsp tirados do HEAD, e não estava publicada. E o `dialin_ivr.lua`, no caminho de quem liga, não tinha portão nenhum: não havia Lua na máquina de desenvolvimento, na imagem, nem no CI.

**Causa.** Três faltas que se escondiam umas às outras. O `modules.conf` não pedia os dois módulos; pedia o `mod_rtp`, que não existe na v1.11.3, e o build saltava-o com um aviso perdido em 8 000 linhas de log. A configuração vanilla carrega o `mod_lua` e tem o `mod_curl` comentado. E sem interpretador de Lua em lado nenhum, a sintaxe dos scripts só era verificada por quem ligava.

**Regra.** A imagem vive no repo (`voice/freeswitch/image/`), com as três fontes fixadas por commit — as mesmas da imagem da R222 — e a base por digest. Um módulo pedido e não compilado **parte o build**. Os `*.lua` debaixo de `voice/` compilam com o `luac5.2` (o Lua que o `mod_lua` liga) no `make fitness` e no CI, e o portão **falha**, não salta, se não houver `luac`. A imagem publica-se só a partir da `main`, com uma tag imutável `1.11.3-<sha8>`, SBOM e proveniência.

**Portão.** `scripts/check-lua-sintaxe.sh` (fitness e CI). `scripts/freeswitch-image-smoke.sh` (workflow `freeswitch-image.yml`): confere os `/REF-*` contra os `ARG`, arranca a imagem **sem rede**, exige que `mod_sofia`, `mod_event_socket`, `mod_conference`, `mod_dptools`, `mod_commands`, `mod_opus`, `mod_lua` e `mod_curl` carreguem, e que o `mod_lua` execute um script que chama a API e vê o `mod_curl`. **Âmbito:** sintaxe e carregamento, não o comportamento do IVR nem a media — a media é a R222.

**Prova corrida a 2026-09-30.** Prova de fumo verde contra a imagem construída. Controlos negativos: um `.lua` partido falha o portão com a linha (`dialin_ivr.lua:191: unexpected symbol near <eof>`), e a imagem antiga chumba em `/REF-freeswitch`, `mod_lua`, `mod_curl` e `luac5.2` — o sofia-sip e o spandsp dela coincidem com os `ARG`, a confirmar que as fontes são as da R222. A **R222 repetida contra a imagem nova** passa com os mesmos números: 440 Hz da sala na gravação do telefone a **0,2496** (0,2495 na corrida de referência), o próprio 1 kHz nesse canal a 0,0001, 1 kHz na sala a 0,1547; `originate`→atendida 564 ms (328 ms antes, com o host a carga ~19). **Por medir:** o IVR a correr de ponta a ponta nesta imagem — a configuração segura que o `fs-canais.sh` monta ainda não está no repo, e não passa pelo Lua.

**Ficheiros.** `voice/freeswitch/image/{Containerfile,modules.conf,README.md}`, `scripts/check-lua-sintaxe.sh`, `scripts/freeswitch-image-smoke.sh`, `.github/workflows/freeswitch-image.yml`, `scripts/fs-canais.sh` (`FS_IMAGE`), `Makefile` (`freeswitch-image`, fitness).

### R224 — O `ForceMute` de um anfitrião não calava quem entrou por telefone, e a sala nem o via

**Sintoma.** Duas metades do mesmo buraco. **Primeira:** uma chamada de telefone entrava na sala como publicador do SFU — ouvia-se — mas **não aparecia no censo da sala**. Para o anfitrião não existia: não estava na lista de participantes, não tinha crachá, não havia em quem carregar. **Segunda:** mesmo que aparecesse, o `ForceMute` não lhe faria nada. Esse comando é uma MENSAGEM ao alvo (`ServerMsg::ForceMuted`) e um browser honra-a silenciando-se a si próprio; **um telefone não tem cliente para a honrar**. A mensagem caía no vazio e o áudio continuava a entrar.

**Regra.** Quem vem de fora da app entra no censo como toda a gente (`signaling::join_external`), com o canal, o número mascarado e os crachás que o distinguem: «sem nome», «vídeo indisponível» e «ligação fraca» — este último **medido** no jitter e na perda do RTP da perna (`phone_bridge::quality`), não adivinhado. E o que um cliente honraria sozinho, para estes **impõe-se no servidor**: o `ForceMute` sobre um lugar de fora da app acciona o interruptor da perna pela porta `signaling::PhoneControl`, implementada pela ponte. O pacote silenciado **conta na mesma** para a estatística e para o RTP simétrico — descartá-lo antes disso faria a perna parecer morta a quem a observa.

**Portão.** `sfu_e2e::force_mute_cala_o_telefone_na_perna`: um telefone manda 1 kHz sem parar e um participante `RTCPeerConnection` real ouve-o; o anfitrião silencia; o tom **desaparece do que a sala recebe** (medido depois de a fila de jitter esvaziar — o que já ia a caminho ainda chega); e **volta** quando o anfitrião desfaz. Sem esse último passo, uma perna morta passaria o teste por outra razão.

**Origem.** A frente dos canais (`origin/delonix-meet-backend/v3-canais`) trazia o modelo — `Seat`, crachás, resumo de canal — mas `join_external` e `update_external` **só eram chamados por testes**, e `muted`/`on_stage` só alguma vez eram escritos como `false`. Nada os ligava, nem lá nem aqui. Portar o modelo sem a ligação poria no protocolo dois campos que mentem: a interface diria «silenciado» e o áudio continuaria a passar. A R221 e a R222 documentam a ponte que este porte completa.

**Ficheiros.** `server/src/signaling.rs` (`Seat`, `join_external`, `update_external`, `channel_summary`, a porta `PhoneControl` e o `ForceMute` a impô-la), `server/src/phone_bridge/{sip,leg}.rs` (registo síncrono de interruptores por perna), `server/src/voice.rs` (a ponte regista-se e os eventos dela alimentam o censo), `server/src/sfu_e2e.rs`.

**O que NÃO entra deste porte, e porquê.** As mensagens `DialOutUpdated` e `SessionCost` e os tipos `DialOutView`/`SessionCostView`: descrevem chamadas de saída e custo, que são da frente da telefonia, e nada na `main` os pode produzir. A porta do WhatsApp Business, pela mesma razão — não tem consumidor. Uma mensagem no protocolo que ninguém produz é uma capacidade anunciada sem código por trás.

### R225 — O anfitrião destacava alguém e o SFU continuava a poder calá-lo

**Sintoma.** O `Spotlight` do anfitrião guardava o destacado na sala e difundia `ServerMsg::Spotlight` — e era só isso. O SFU nunca o soube. Numa sala de cinco ou mais, o selector de oradores encaminha só os três microfones com mais energia (`MAX_ACTIVE_SPEAKERS`), e a pessoa destacada para toda a gente podia ser uma das suprimidas: a interface dizia «em palco» e o áudio não passava. A `Publication` já tinha o campo `pinned` e o selector já o respeitava; **ninguém o escrevia**.

**Regra.** O palco vale no encaminhamento, não só na interface. O `Spotlight` acciona a porta `signaling::StageControl`, implementada sobre o SFU e registada no arranque (`lib.rs`): fixa o áudio do novo destacado (`SfuState::set_audio_pinned`) e **liberta o anterior** — senão ficavam dois fora do concurso para sempre. Quem está fixado passa sempre e não ocupa um dos lugares do top-N; ao fixar, volta a encaminhar já, sem esperar o próximo tique do selector. Vale para qualquer publicador, browser ou perna de telefone; um lugar de fora da app leva além disso o crachá `on_stage`, que a R224 deixara sempre a `false`.

**Portão.** Duas metades. `signaling::b1_sala_tests::destacar_fixa_o_audio_no_sfu_e_liberta_o_anterior` mede o que o `Hub` PEDE ao SFU: destacar fixa, trocar liberta o anterior, limpar liberta, e quem não é anfitrião não fixa nada. `sfu_e2e::palco_impede_o_selector_de_calar_quem_esta_destacado` mede o que um `RTCPeerConnection` real RECEBE: um telefone em silêncio é subscrito, entram três que falam e o selector suprime-o (os pacotes param — sem este passo o teste não mediria nada), e fixado volta a chegar.

**Ficheiros.** `server/src/signaling.rs` (a porta `StageControl`, o `Spotlight` a accioná-la e o crachá `on_stage`), `server/src/sfu.rs` (`set_audio_pinned`), `server/src/lib.rs` (registo no arranque), `server/src/sfu_e2e.rs`.

**O que NÃO está provado.** O caminho inteiro WebSocket → `Spotlight` → SFU num só teste: as duas metades estão medidas em separado e a cola é o adaptador de 15 linhas em `lib.rs`. E nenhum cliente web foi alterado — o destaque já existia na interface.

### R226 — «SRTP obrigatório» estava escrito em parâmetros que o FreeSWITCH não lê, e nada o impunha à entrada

**Sintoma.** Medido a 2026-10-03 com os ficheiros que o `voice/docker-compose.voice.yml` monta, sobre a configuração vanilla da imagem `delonix-meet/freeswitch:1.11.3`: um ramal autenticado fez um `INVITE` **sem `a=crypto`** ao perfil `internal` e **não foi recusado por isso** — chegou ao plano de marcação exactamente como a chamada cifrada (`404`, ver «por corrigir»). A variável global `rtp_secure_media` estava vazia. O perfil dos ramais e o perfil de conferência diziam «SRTP obrigatório» num comentário, ao lado de `<param name="rtp-secure-media" value="mandatory"/>`.

**Causa.** Quatro mecanismos, e só um funciona — nenhum estava a valer:

1. **`rtp-secure-media` não é parâmetro de nada.** Zero ocorrências em `mod_sofia/sofia.c` e em `mod_conference/*.c` no commit `ef32e205` (v1.11.3). O `tls` do perfil de conferência, idem: o mod_conference não tem nenhum parâmetro de segurança de media — a conferência mistura áudio já decifrado.
2. **`require-secure-rtp`, o parâmetro de perfil «a sério», é inerte.** O sofia lê-o (`sofia.c:5451`) para ligar a flag `PFLAG_SECURE`, e mais nada no código a consulta (só `mod_sofia.h:227` e as duas linhas que a escrevem). Com ele a `true` e sem a global, a chamada em claro foi **aceite e atendida**.
3. **O `set rtp_secure_media=mandatory` do dialplan depende do perfil por onde a chamada entra.** Num perfil que negoceia o SDP à chegada — o dos ramais — corre tarde, e a chamada em claro foi **aceite**. Num perfil com `inbound-late-negotiation=true` — o `external` da vanilla, por onde entra o dial-in — a negociação espera pelo `answer` e o mesmo `set` recusa-a com `488`; sem `set` e sem a global, esse perfil também a aceitou. Não é dele que se depende: a exigência ficava presa a um parâmetro de outro ficheiro. *(Corrigido a 2026-10-03: a primeira versão desta entrada dizia que o `set` chegava sempre tarde — só tinha sido medido num perfil sem negociação tardia.)*
4. **A variável global é o que recusa** (`switch_core_media.c:5627`, «Crypto not negotiated but required» → `488`), mas vivia só em `voice/freeswitch/vars.xml.inc`, e esse ficheiro (a) **não é incluído por nada** — o compose monta-o (`:44`) e nenhum `vars.xml` do repo nem o da vanilla o inclui; (b) trazia no cabeçalho, dentro de um comentário, a directiva de include por extenso, e o pré-processador do FreeSWITCH procura `X-PRE-PROCESS` em cada linha sem olhar a comentários (`switch_xml.c:1525`): incluído como o cabeçalho mandava, incluía-se a si próprio e o arranque morria com `Cannot Initialize [[error near line 395]: unclosed <!--]`; (c) lia o ambiente com `cmd="set"` e `$${NOME}`, que lê outra variável global e não o ambiente — URL, segredo e porto ficavam vazios, e o perfil dos ramais ia para o porto 5060.

**Regra.** O SRTP à entrada é a **variável global**, e o `sip_profiles/internal.xml` põe-na ele próprio, com uma directiva de pré-processamento no topo: quem carrega o perfil carrega a exigência, sem depender de alguém ter incluído outro ficheiro. Não «arrumar» isto de volta para um `<param>` do perfil — não há nenhum que o faça. Nunca escrever uma directiva `X-PRE-PROCESS` dentro de um comentário de um ficheiro de configuração do FreeSWITCH. Ler o ambiente é com `cmd="env-set"` e `$NOME`. E «SRTP obrigatório» prova-se com o controlo negativo, não com a leitura do XML.

**Portão estático, no CI.** `scripts/check-fs-xml.sh` (`make fitness`): em todo o `*.xml` e `*.xml.inc` de `voice/`, XML bem formado, nenhuma directiva `X-PRE-PROCESS` dentro de um comentário, nenhum `$${NOME_EM_MAIÚSCULAS}`. Com o `vars.xml.inc` de antes desta entrada falha nas quatro linhas certas. Não prova o que a configuração faz em chamada.

**Portão de comportamento.** `bash scripts/softphone-prova.sh srtp-real` — **no CI desde 2026-10-04** (workflow «Imagem FreeSWITCH», contra a imagem acabada de construir, sempre que `voice/freeswitch/`, `voice/cluster/`, o `compose.yaml` ou o script mudam). *O parágrafo seguinte descreve o `srtp-real` como era quando media o compose antigo; desde 2026-10-04 mede o `compose.yaml`, ver «O compose antigo foi retirado» abaixo.* Tira a configuração das linhas de montagem do próprio compose, numa rede docker sem saída: (1) a global vale; (2) com a password errada o ramal leva `403` — o perfil autentica; (3) com a password certa e SRTP, a chamada passa a autenticação e a negociação; (4) **a mesma chamada sem SRTP leva `488`** e o FreeSWITCH regista a razão; (5) com o `vars.xml.inc` incluído pelo `vars.xml`, o FreeSWITCH arranca, o perfil escuta no porto do ambiente e o URL e o segredo vêm do ambiente. O andaime da prova é um ramal em directório estático (o control plane não corre), sem os perfis SIP de demonstração da vanilla, e o ESL em loopback.

**Prova corrida a 2026-10-03.** Com o `voice/` de `origin/main` (`275ced1`) e o script final: global vazia; sem SRTP `404` em vez de `488`; passo 5 com o `unclosed <!--`. Com a correcção: 10 verificações verdes, `488 Not Acceptable Here` para a chamada em claro, repetido em cinco corridas. As variantes de perfil mediram-se com o `selftest` sem a global: nada → aceite; `require-secure-rtp=true` → aceite; a global posta no ficheiro do perfil → `488`. E com `inbound-late-negotiation=true`, sempre sem a global: `set` antes do `answer` → `488`; sem `set` → aceite.

**Por corrigir — o compose de voz continua sem correr, e isto não o resolve.**
- **Nada inclui o `vars.xml.inc`**: tal como o compose a monta, a configuração deixa o perfil dos ramais no porto **5060** (o do Kamailio, em `network_mode: host`) e o URL e o segredo do control plane vazios. O ADR-0009 já o listava a 2026-09-17.
- **O contexto `delonix_ramais` não existe para o FreeSWITCH**: o ficheiro é montado em `dialplan/default/`, que a vanilla inclui *dentro* do contexto `default` — `Context delonix_ramais not found`, `404`. Por isso o controlo positivo não chega a atender: prova a autenticação e a negociação, não uma chamada estabelecida.
- A vanilla não carrega o `mod_xml_curl` nem o `mod_curl`, de que o directório dos ramais e os dois Lua dependem.
- O compose usa `safarov/freeswitch:latest`, que **não foi medida** (nem descarregada): a prova corre na imagem do repo, que guarda a configuração em `/usr/local/freeswitch/etc/freeswitch` e não em `/etc/freeswitch`.

**O compose antigo foi retirado (2026-10-04).** O `voice/docker-compose.voice.yml` e o `voice/freeswitch/vars.xml.inc` — os quatro pontos acima — saíram da `develop`, com os alvos `make voice-up`/`voice-down`/`voice-certs`: nunca correram, e o `compose.yaml` (`make compose-up`) sobe a voz pelo mesmo arranque do cluster. O `srtp-real` passou a medir o `compose.yaml`; é a mesma prova do `srtp-cluster`, com a lista de ficheiros de cada um.

**A configuração que corre: o cluster local (medido a 2026-10-03).** O `make cluster` não usa o compose: o `voice/cluster/freeswitch-entrypoint.sh` monta a configuração sobre a vanilla, põe a global ele próprio, define as variáveis a partir do ambiente, põe os dialplans do Meet como contextos de topo e carrega o `mod_curl` e o `mod_xml_curl` — o que nos quatro pontos acima falta ao compose. `bash scripts/softphone-prova.sh srtp-cluster` mede essa configuração, com os ficheiros do ConfigMap `freeswitch-meet` (tirados do `scripts/cluster-voice.sh`) e, como andaime, um servidor de directório que responde como o `ramais.rs` a um só ramal. Ramal com a password errada: `403`. Ramal autenticado, com SRTP: passa a negociação e chega ao `ramais_dial.lua`, que a fecha com `404` (o andaime não resolve números). Sem SRTP: `488`. Dial-in pelo perfil `external`, com SRTP: atendido pelo IVR; sem SRTP: `488`. **Sensibilidade da prova:** tirando a global do entrypoint continua verde (fica a do `internal.xml`); tirando as duas, o ramal em claro deixa de ser recusado e a prova falha — e o dial-in continua a dar `488`, pelo `set` do dialplan e a negociação tardia do `external` (ponto 3).

**O que NÃO está provado.** O caminho do dial-in (Kamailio → contexto `public`), TLS na sinalização, e um re-INVITE em claro a meio de uma chamada cifrada. A global vale para **todas** as pernas: uma chamada de entrada de um tronco declarado `srtp=off` seria recusada — o `telephony_fs_xml.rs:230` só põe `rtp_secure_media` por gateway à saída —, e isso não foi medido. Os `set` do dialplan, dos dois Lua e do dialplan que o `ramais.rs` gera ficaram onde estavam, com o comentário corrigido.

**Ficheiros.** `voice/freeswitch/sip_profiles/internal.xml`, `voice/freeswitch/autoload_configs/conference.conf.xml`, `voice/freeswitch/vars.xml.inc`, `voice/freeswitch/dialplan/{public/00_delonix_dialin,default/00_delonix_extensions}.xml` e `voice/freeswitch/scripts/{dialin_ivr,ramais_dial}.lua` (só comentários), `voice/README.md`, `voice/docker-compose.voice.yml` (só um comentário), `scripts/softphone-prova.sh` (modo `srtp-real`), `voice/softphone/README.md`, `.claude/skills/delonix-meet-voip/SKILL.md`.

### R227 — O segredo de voz ia no URL do `mod_xml_curl`, e o FreeSWITCH escrevia-o no log

**Sintoma.** As duas bindings do `mod_xml_curl` (`voice/freeswitch/autoload_configs/xml_curl.conf.xml`) chamavam o servidor com `…/api/voice/ivr/directory?secret=<VOICE_INTERNAL_SECRET>` e `…/dialplan-did?secret=…`. Medido a 2026-10-03 contra o FreeSWITCH 1.11.3: o módulo escreve o URL de cada binding no log **ao arrancar** (`mod_xml_curl.c:562`, nível NOTICE) e outra vez **a cada pedido que falha** (`:311`, `:316`, `:319`). Com um arranque e um pedido falhado, o segredo apareceu **5 vezes** no `freeswitch.log`. E ia na linha do pedido HTTP, que é o que qualquer log de acesso pelo caminho guarda. Quem lê o log da voz ficava com o segredo que autentica toda a API interna de IVR.

**Causa.** O comentário do ficheiro dava a razão: «a build stock do mod_xml_curl não deixa configurar cabeçalhos HTTP arbitrários». É verdade, e não chegava a conclusão nenhuma: o módulo tem `gateway-credentials` e `auth-scheme` (`mod_xml_curl.c:400-402`), que enviam HTTP Basic, e o `check_media_secret` do servidor **já aceitava** o segredo como password do Basic desde a telefonia de troncos (ADR-0009). O `?secret=` era um recurso que ninguém voltou a medir.

**Regra.** O segredo de voz **nunca** vai num URL. O `mod_xml_curl` envia-o por HTTP Basic (`gateway-credentials`, utilizador `freeswitch`, que não conta), e o servidor **deixou de o aceitar** em `?secret=`: `ivr_directory` e `ivr_dialplan_did` chamam o `check_media_secret` como o resto da API interna, sem recurso — um `?secret=` é ignorado, não autentica. Não «repor por compatibilidade»: um segredo que o servidor aceita no URL acaba num URL.

**Portão.** `tests/security_voice_odoo.rs::xml_curl_routes_take_the_secret_by_basic_and_never_in_the_url`, contra Postgres real, nas duas rotas: Basic certo `200`, `X-Voice-Secret` certo `200`, **o segredo certo em `?secret=` `401`** (também ao lado de um Basic errado, e também no corpo do formulário), Basic errado `401`, sem nada `401`. Com o `ramais.rs` de antes desta entrada o teste falha no `?secret=` (`200` em vez de `401`). `voice::tests::ivr_basic_takes_the_secret_only_as_the_password` prende as fronteiras do Basic: só como password, só com o esquema `Basic`, nunca a salvar um `X-Voice-Secret` errado, e o `503` da R154 ganha a um Basic certo.

**Prova corrida a 2026-10-03, contra o FreeSWITCH 1.11.3 real** (um `nc` a capturar o pedido). Configuração antiga: pedido `POST /api/voice/ivr/directory?secret=<o segredo>`, sem `Authorization`, segredo 5 vezes no log. Configuração nova: pedido `POST /api/voice/ivr/directory`, `Authorization: Basic` com `freeswitch:<o segredo>`, segredo **0 vezes** no `freeswitch.log`, em claro ou em base64.

**Os dois Lua também o escreviam, a cada chamada — fechado no cluster local.** `ramais_dial.lua` e `dialin_ivr.lua` chamavam o servidor com `session:execute("curl", …)` e o segredo nos argumentos. Medido a 2026-10-03 na configuração do cluster: o FreeSWITCH escrevia a linha `EXECUTE … curl(… X-Voice-Secret: <segredo> …)` (nível INFO) e o `mod_curl` outra a DEBUG (`mod_curl.c:246`), **duas linhas com o segredo por chamada**; no `dialin_ivr.lua` o corpo levava ainda o PIN de quem liga (`mod_curl.c:262`, `Post data: …`). Os argumentos de uma aplicação vão também para o `app_log` do CDR (lido no código, não medido). Duas correcções, e é preciso as duas:
- os Lua passam a usar a **API** do `mod_curl` (`api:execute("curl", …)`, mesmos argumentos): deixa de haver linha `EXECUTE` e `app_log`;
- o `voice/cluster/freeswitch-entrypoint.sh` tira o nível **DEBUG** do log (`DELONIX_FS_LOG_DEBUG=1` volta a ligá-lo, e volta a pôr lá o segredo e o PIN).

**Portão de comportamento (no CI desde 2026-10-04, workflow «Imagem FreeSWITCH»).** O passo 6 do `bash scripts/softphone-prova.sh srtp-real` e do `srtp-cluster`, com um servidor de andaime que guarda os pedidos: nenhum pedido leva o segredo no URL; o do `mod_xml_curl` leva-o em `Authorization: Basic`; os do `ramais_dial.lua` e do `dialin_ivr.lua` chegam com `X-Voice-Secret`, `Content-Type: application/json` e o corpo certo (no IVR, com o PIN marcado por DTMF e o `+` do DID intacto); e **o segredo e o PIN aparecem 0 vezes no `freeswitch.log`**. Sensibilidade: com os Lua de antes, ou com o DEBUG ligado, o segredo aparece 2 vezes e o PIN 1; com o `xml_curl.conf.xml` de antes, 5 vezes e no URL.

**O `freeswitch.xml.fsxml` saiu do directório de logs (2026-10-04).** O FreeSWITCH grava no directório do `-log` a configuração já pré-processada, com o segredo expandido — antes no URL, agora no `gateway-credentials`. Medido: 2 ocorrências, ao lado do `freeswitch.log`. O arranque passa a dar ao `-log` um directório privado ao lado da configuração (`/conf/.estado`, modo 700) e a fixar o `freeswitch.log`, por caminho explícito, onde sempre esteve. O passo 6 do `srtp-real`/`srtp-cluster` exige que **nenhum ficheiro do directório de logs traga o segredo**; com o arranque anterior falha, com o `freeswitch.xml.fsxml` apontado.

**O PIN lia-se no log dígito a dígito (2026-10-04).** A prova procurava o PIN no corpo do pedido e dava-o por ausente; numa chamada a sério, no laboratório, o log tinha uma linha `RECV DTMF <dígito>` por tecla (`switch_channel.c:529`, nível INFO) — o PIN de cima para baixo. A variável de canal `sensitive_dtmf=true` cala essa linha, e fica posta nos dois planos de marcação antes de o IVR correr. O passo 6 da prova exige agora **zero linhas `RECV DTMF`**. Medido no laboratório com uma chamada pelo PBX: no compose, 7 linhas antes e nenhuma nova depois, com o PIN certo aceite e a chamada na conferência; no cluster, 0 linhas com o IVR a correr.

**Rodar o segredo (2026-10-04).** `make voice-secret-rotate` troca o `VOICE_INTERNAL_SECRET` do `.env` — a única origem: o `compose.yaml` lê-o e o `make cluster` cria a partir dele o Secret `delonix-voice` — e diz o que falta para o pôr a valer. Qualquer instalação que tenha corrido com a configuração de antes desta entrada escreveu o segredo no log: roda-o **depois** de ter estas correcções, e apaga esses logs. O procedimento está em `voice/README.md`.

**Por corrigir.**
- **Uma instalação que monte a configuração à mão, sem o arranque**, fica com o que tiver no `logfile.conf.xml` e no `-log`: com DEBUG, o `mod_curl` escreve o segredo e o PIN a cada chamada, e o `freeswitch.xml.fsxml` fica ao lado do log. O `compose.yaml`, o cluster local e o chart Helm sobem todos pelo `voice/cluster/freeswitch-entrypoint.sh`.
- **O `delonix compose up` não recria contentores que já existem**: depois de rodar o segredo (ou de construir imagens novas), o compose precisa de `make compose-down` e `make compose-up` — os volumes mantêm-se. Feito à mão a 2026-10-04; o `make compose-up` não o faz sozinho.
- **O `make cluster` reaplica o Secret antes do passo do Helm**: se esse passo falhar (aconteceu duas vezes, por DNS), o Secret já tem o segredo novo e os pods ainda o antigo — coerentes entre si até um deles reiniciar. Repete-se o `make cluster` até ao fim.

**A rotação, exercitada (2026-10-04).** No compose e no cluster locais, que corriam a configuração anterior a esta entrada (o segredo 2 vezes no log e em 2 ficheiros do directório de logs, em cada um): depois de os actualizar para a `develop` e rodar, o servidor e o FreeSWITCH usam o segredo novo, com 0 ocorrências no log e 0 ficheiros com ele; os dados mantiveram-se e o `compose-voice-check` e as medições do cluster passam.

**Ordem de actualização.** Primeiro a configuração do FreeSWITCH, depois o servidor. O servidor anterior já aceita Basic; um servidor novo com o `xml_curl.conf.xml` antigo responde `401` a cada registo, e a cada `401` o FreeSWITCH escreve o URL antigo, com o segredo, no log.

**O que NÃO está provado.** O par completo — este FreeSWITCH a registar um ramal contra este servidor — não correu: o cabeçalho foi medido de um lado e a aceitação do outro. A medição do log é uma corrida avulsa, sem portão que a repita. O Basic viaja em claro como viajava o URL: entre o FreeSWITCH e o servidor continua a ser preciso rede privada ou TLS. Não foi visto se o libcurl reenvia o Basic num redireccionamento para outro host (o `mod_xml_curl` segue redireccionamentos).

**Ficheiros.** `voice/freeswitch/autoload_configs/xml_curl.conf.xml`, `server/src/ramais.rs` (sai o `DirectoryQuery` e o `check_media_secret_str`), `server/src/voice.rs` e `server/tests/security_voice_odoo.rs` (testes), `voice/freeswitch/scripts/{dialin_ivr,ramais_dial}.lua`, `voice/cluster/freeswitch-entrypoint.sh`, `scripts/softphone-prova.sh` (passo 6 do `srtp-cluster`).

### R228 — A conferência registava «Can't find caller-controls» a cada entrada

**Sintoma.** Visto a 2026-10-04 no laboratório, numa chamada com o PIN certo: ao entrar na conferência o FreeSWITCH escrevia `[ERR] conference_member.c:87 Can't find caller-controls in conference.conf`, uma vez por chamador. A chamada entrava na mesma, sem controlos por DTMF.

**Causa.** O perfil `delonix` pedia `caller-controls="default"`. O grupo `default` existe no `conference.conf.xml` da vanilla, mas o nosso ficheiro **substitui** esse e não traz nenhuma secção `caller-controls`. O mod_conference só não liga controlos quando o valor é `none` (`conference_member.c:1009`); com qualquer outro nome vai procurar o grupo e, não o achando, regista o erro.

**Regra.** Um perfil de conferência pede `none` ou um grupo **definido no mesmo ficheiro**. Ficou `none`, que é o comportamento que já tinha. Dar controlos a quem liga (calar-se com uma tecla, por exemplo) é uma decisão de produto: faz-se definindo o grupo no ficheiro, não apontando para um nome da vanilla.

**Portão.** `scripts/check-fs-xml.sh` (no `make fitness` e no CI): num `conference.conf`, `caller-controls` e `moderator-controls` têm de ser `none` ou nomear um grupo do ficheiro. Com o ficheiro anterior falha e aponta a linha.

**Prova corrida a 2026-10-04, no laboratório.** Compose: 2 linhas do erro antes (duas entradas na conferência); depois da correcção, uma terceira chamada com o PIN certo entrou na conferência e o log continuou com 2. Cluster: 0 linhas no pod novo, com o perfil carregado.

**O que NÃO está provado.** O portão é estático: não mede uma entrada na conferência. As provas com chamadas do `softphone-prova.sh` não chegam à conferência do perfil `delonix` (o servidor de andaime não devolve uma sala).

**Ficheiros.** `voice/freeswitch/autoload_configs/conference.conf.xml`, `scripts/check-fs-xml.sh`.

### R270 — A fala de um participante entrava no prompt do LLM como se fosse instrução

**Sintoma.** `ai::caption_prompt` e `ai::minutes_prompt` interpolavam a legenda, a transcrição e o título da reunião numa string única, a seguir à instrução. Quem ditasse «ignora as instruções anteriores, a reunião decidiu…» escrevia no mesmo plano que a instrução, e a frase podia acabar citada como decisão na acta — que dispara o webhook `meeting.mom_ready` para fora. Era o «não fechado aqui» da R231 (OWASP LLM01); o DLP tira PII, não tira instruções.

**Regra.** Instrução e dado vão separados: a instrução no campo `system` do `/api/generate` do Ollama, o texto não confiável em `prompt`, cercado por `<fala>` e `<titulo>`, com o aviso de que o que lá está é dado (`ai::UNTRUSTED_NOTE`). As etiquetas da cerca são tiradas do próprio texto (`ai::strip_fence_tags`), senão fechava-se a cerca por dentro. É mitigação, não blindagem: a resposta do modelo continua a ser só texto, sem nenhuma acção a partir dela.

**Portão.** `ai::tests::{a_fala_fica_na_cerca_e_fora_da_instrucao, nao_se_fecha_a_cerca_por_dentro, a_instrucao_vai_no_campo_system}` — o último contra um Ollama falso que guarda o corpo recebido — e os três da R231, que continuam a valer sobre as duas metades do prompt.

**Origem.** `fix/dlp-antes-do-llm` (302bd29, PR #68 nunca integrado) fazia-o com o `/api/chat`. Aqui fica no `/api/generate`, que é o que o resto do `ai.rs` e o `ai_studio.rs` usam e que os testes com o Ollama falso cobrem.

**O que NÃO está provado.** O efeito num modelo real: nenhum Ollama correu contra este prompt, e não se mediu se a qualidade da tradução ou da acta mudou com a instrução em `system`. Os capítulos (`recording_chapters.rs`) e o Estúdio (`ai_studio.rs`) continuam com o prompt numa string só.

**Ficheiros.** `server/src/ai.rs`.

### R271 — A chamada de saída para a ponte da sala ia sem SRTP e sem o id da chamada

**Sintoma.** `telephony_esl::originate_command`, no ramo `AfterAnswer::RoomBridge`, montava o `&bridge(...)` para o UA da ponte sem `rtp_secure_media` e sem `sip_h_X-Delonix-Call-Id`. A ponte recusa com `488` uma oferta sem `a=crypto` (`phone_bridge/sip.rs`) e lê esse cabeçalho para ligar a perna SIP à chamada da telefonia — o lado que recebe estava portado, o lado que envia não. Sem efeito visível hoje: nada em `server/src` constrói um `RoomBridge` fora dos testes.

**Regra.** A perna para a ponte leva `rtp_secure_media=mandatory:<suite>` (`phone_bridge::srtp::SRTP_PROFILE_NAME`) e o cabeçalho `X-Delonix-Call-Id` (`phone_bridge::sip::CALL_ID_HEADER`). `mandatory` e não `optional`: com `optional` uma resposta em claro passava.

**Portão.** `telephony_esl::tests::room_bridge_goes_to_the_bridge_ua_not_the_local_conference` compara o comando inteiro.

**Entrou no mesmo porte, SEM consumidor.** De `delonix-meet-backend/v3-canais`, e contra o que a R224 decidira: as mensagens `ServerMsg::DialOutUpdated` e `SessionCost` com `DialOutView`/`SessionCostView`/`CurrencyTotalView` (só os testes as constroem), a porta `domain::integration::whatsapp` (nenhum adaptador a implementa) e a migração `0086_room_channels.sql` (`room_dial_outs`, `room_phone_pins`, `org_whatsapp_configs` — nenhum código as lê ou escreve). Não são capacidades: nenhuma pode ser anunciada enquanto não tiver quem a produza.

**O que NÃO está provado.** O `originate` com estas variáveis contra um FreeSWITCH real — a R222 mede a ponte com um comando montado no próprio teste (`sfu_e2e.rs`), não com o `originate_command`.

**Ficheiros.** `server/src/telephony_esl.rs`, `server/src/signaling.rs`, `server/crates/delonix-meet-domain/src/integration/whatsapp.rs`, `server/migrations/0086_room_channels.sql`, `server/tests/telephony.rs`.
### R155 — Um convidado sem conta entra pela porta, e a porta não abre mais nada

**Sintoma (antes).** Um externo sem conta não conseguia entrar numa reunião: o `join_room` exige `AuthUser`, e o link levava ao ecrã de login. Era o bloqueio n.º 1 à adopção face ao Zoom e ao Meet (`notas-ui-template/adopcao-vs-meet-teams-zoom.md`, alavanca 1).

**O risco que a correcção cria.** É a primeira rota PÚBLICA que dá acesso a uma reunião. As quatro formas óbvias de a errar: (1) o token do convidado abrir alguma rota `/api/*` (gravações, chat guardado, actas, quadros, convites); (2) o convidado entrar sem ser admitido — basta um `wait: false` mal emitido; (3) o convidado ganhar o papel de anfitrião (`transfer-host`) ou reclamá-lo por reconexão; (4) a rota servir para esgotar o TURN ou inundar a sala de espera de alguém.

**Regra.**
- O token é `typ: "room"` com `origin: "guest"`, o claim `guest: true` e um `sub` gerado que não existe em `users`. Nenhum extractor da API aceita `typ: "room"` — a exclusão é por construção, não por lista. O claim é próprio porque `origin: "guest"` sozinho já é o que o `join_room` dá a quem TEM conta e entrou pelo link sem convite (R182): esse continua a poder receber papéis.
- O `/ws` decide o lugar em `signaling::seat_policy` a partir do claim `guest`: convidado espera sempre (`lobby` forçado — nem o anfitrião a desligar a sala de espera a meio o deixa passar) e não tem papel, seja o que for que venha nos outros campos do token. `Hub::join_with` volta a impô-lo (`JoinExtras::is_guest`), e o lugar reclamado guarda a marca (`ReclaimedSeat::is_guest`). O anfitrião vê-o marcado: `PeerInfo::is_guest` na espera e na sala, `WaitingView::is_guest` na REST.
- `TransferHost`, `PromoteAdmit` e `SetRole` para `cohost` são recusados quando o alvo é um convidado (os papéis de palco, `speaker`/`broadcast`, não); o directo (`/api/rooms/{room_code}/live`) recusa tokens de convidado com `403`.
- `rooms.allow_guests` (0075, por omissão `true`, `PATCH /api/rooms/{room_code}` só pelo dono) → `403` antes de emitir seja o que for.
- Travão por IP (`GUEST_JOIN_PER_IP_PER_MIN`, 10) antes de ler a base e por sala (`GUEST_JOIN_PER_ROOM_PER_MIN`, 30) depois de a sala existir, com `429` + `Retry-After` com o que falta da janela (`ApiError::RateLimited`, do `RateLimiter::acquire`).
- `room.guest_join` na auditoria da org do dono, com o código da sala e o nome marcado «(convidado)». Nem IP nem agente.

**Portão.** Unidade: `guests::tests` (nome, admissão, forma do token, token ≠ acesso, travão) e `signaling::b1_sala_tests::convidado_*` (espera forçada, não admite nem por promoção, não é promovido, reclama o lugar e continua convidado) — no ramo de origem, os dois de promoção/admissão foram verificados a FALHAR com as guardas retiradas. Servidor real: `web/e2e/isolamento.mjs` (secção «convidado sem conta»: 14 rotas autenticadas recusadas com o token de convidado, sala de espera, directo, `allow_guests`, 400/404/422, travões por IP e por sala, auditoria) e `web/e2e/convidado.mjs` (admissão, recusa, reentrada no mesmo lugar, expulsão), os dois no job `isolamento` do CI.

**Fora.** Media do convidado (não há e2e com `RTCPeerConnection` para convidados), salas de grupo (a troca de sala chama `/join`, que exige conta: um convidado não vai para um grupo), e os travões são por pod (memória), como os restantes.

**Ficheiros.** `server/src/guests.rs`, `server/src/signaling.rs`, `server/src/auth.rs`, `server/src/rooms.rs`, `server/src/audit.rs`, `server/src/error.rs`, `server/src/broadcast.rs`, `server/src/lib.rs`, `server/migrations/0075_room_allow_guests.sql`, `scripts/rotas-publicas.txt`, `web/e2e/isolamento.mjs`, `web/e2e/convidado.mjs`, `web/src/api.ts`, `.github/workflows/ci.yml`.

**Porte para o `develop` (2026-10-03).** A entrada nasceu no ramo `delonix-meet-backend/convidado-sem-conta` (2026-09-16) e foi portada por cima do `develop`: a rota vive em `lib.rs`, a migração passou de 0040 a 0075, e o `join_seat` do ramo deu lugar ao `join_with` com `JoinExtras::is_guest`. **Não revalidado no porte:** os dois e2e (`isolamento.mjs`, `convidado.mjs`) não correram contra servidor e Postgres reais; a prova de que os testes falham sem as guardas é a do ramo de origem; e o que o `handle_socket` do `develop` grava com o `user_id` (chat persistido, presenças) nunca foi medido com um `sub` que não existe em `users`.
### R200 — Terminar uma sessão não cortava nada até o JWT expirar

**Sintoma.** A sessão tinha identidade (`refresh_tokens.session_id`, 0065) mas não tinha estado: revogar o refresh deixava o access token (15 min) a abrir a API e o `/rtc` e o `/ws` ligados.

**Regra.** `user_sessions` (0078) guarda o estado da sessão com o MESMO id; o access e o room token levam `sid`; `AuthUser`, `/rtc` e `/ws` recusam uma sessão terminada com `401 auth.session_revoked`. Terminar (`DELETE /api/users/me/sessions/{session_id}`, em `account.rs`, por `sessions::revoke`) revoga os refresh tokens dela e acorda o `shutdown` das ligações dela neste nó e, pelo canal Redis `dlx:session-revoked`, nos outros. O logout termina a sessão.

**Portão.** `server/tests/account_sessions.rs::revoking_a_session_kills_refresh_access_and_websockets` (o `/rtc` e o `/ws` fecham, o room token ainda válido não reabre, a sessão de onde se termina continua). **Não validado:** com duas réplicas reais (o caminho Redis entre nós).

**Ficheiros.** `server/src/{sessions,account,auth,presence,signaling,rooms,pubsub,lib}.rs`, migração `0078_sessoes`.

### R201 — «Terminar todas as outras sessões»

**Regra.** `POST /api/users/me/sessions/revoke-others` termina todas menos a do pedido; um access token sem `sid` (anterior às sessões) recebe `422 sessions.current_unknown` em vez de terminar a própria.

**Portão.** `server/tests/account_sessions.rs::revoke_others_keeps_only_the_current_one`.

### R202 — As sessões são só da própria pessoa, também para o administrador da org

**Regra.** Toda a leitura e revogação filtra pelo `user_id` da sessão — também o `UPDATE` dos refresh tokens, porque `refresh_tokens.session_id` não tem chave estrangeira; uma sessão de outra pessoa dá `404`, igual a uma inexistente. Suspender a conta de outro é outra superfície.

**Portão.** `server/tests/account_sessions.rs::sessions_of_others_are_not_found`; `web/e2e/isolamento.mjs` («A termina uma sessão de B»).

### R203 — Reautenticação recente para alterar factores

**Regra.** `POST /api/users/me/reauthentication` (password pela mesma função do login — `auth::password_matches`, Odoo incluído — ou código TOTP/recuperação) abre 5 min NESSA sessão; o travão `mfa_limiter` conta as falhas e recusa também a prova certa durante o bloqueio. Sem janela: `403 auth.reauthentication_required`.

**Portão.** `server/tests/account_sessions.rs::reauthentication_needs_the_real_password`, `account_passkeys.rs`.

### R204 — Os campos do Odoo não se editam no perfil, e o nome legal vem da sincronização

**Sintoma evitado.** Um perfil editável localmente numa conta gerida divergia do ERP em silêncio; o `PATCH /api/users/me` ignorava um idioma desconhecido (200) e aceitava mudar a password de uma conta cuja password é a do Odoo.

**Regra.** `PATCH /api/users/me/profile` valida tudo antes de escrever; `legal_name`, `email`, `department` → `409 profile.field_managed_by_odoo` (conta gerida) ou `409 profile.field_read_only`; telefone pela `sms::normalize_msisdn` (`422 profile.invalid_phone`) e escrito por `org::set_member_phone` (a regra do SMS: apagar fica `manual`); idioma `400 profile.invalid_locale` (também no `PATCH /api/users/me`, onde antes era ignorado); password de conta gerida `409`. A sincronização (`odoo_sso::upsert_member`) escreve `legal_name`, e um email no lugar do nome não o apaga.

**Portão.** `server/tests/account_profile.rs::{profile_validates_normalizes_and_protects_odoo_fields, legal_name_comes_from_the_odoo_sync}`; `tests/identity.rs::users_me_get_and_patch` mudou com intenção (idioma desconhecido 200→400).

### R205 — Fotografia de perfil pelos bytes, com tecto, e só para quem partilha organização

**Regra.** PNG/JPEG/WebP reconhecidos pela assinatura (um SVG com `Content-Type: image/png` → `422 profile.avatar_unsupported_type`), até 1 MiB (`422 profile.avatar_too_large`; acima de 2 MiB o servidor corta com 413). `GET /api/users/{user_id}/avatar` só com organização activa em comum; senão `404`.

**Portão.** `server/tests/account_profile.rs::avatar_is_sniffed_limited_and_scoped`; `isolamento.mjs`.

### R206 — «Avisar antes de gravar» é imposto pelo servidor

**Regra.** As preferências de entrada vêm no `POST /api/rooms/{room_code}/join`. Um anfitrião com `warn_before_recording` que manda `server-record` sem `confirmed: true` recebe `recording-confirmation-required` e a gravação NÃO começa. Sem a preferência, o pedido antigo grava como sempre.

**Portão.** `server/tests/account_profile.rs::join_preferences_reach_the_join_and_recording_needs_confirmation`, `domain::identity::join_preferences::tests`.

### R207 — Preferências de notificação honestas; guia e «Novo PIN»

**Regra.** Só `in_app` entrega; `email` e `sms` são guardados mas anunciados `not_configured`. Com `in_app` desligado para um tipo, o produtor não cria a notificação. O guia valida os ids contra a lista versionada (`404 tour.unknown_step`). «Novo PIN» troca o PIN da sala de voz activa ligada à sala pessoal (o antigo morre), auditado; sem dial-in `409 personal_room.no_dial_in`.

**Portão.** `server/tests/account_profile.rs::{notification_preferences_are_honest_and_enforced, tour_progress_and_new_pin}`.

### R208 — Chaves de acesso: segundo factor, cerimónia de uso único, último factor

**Regra.** ADR-0011. Registar exige reautenticação; o login com password passa a desafio `methods: ["passkey"]`; a cerimónia é consumida uma vez (replay `404 passkeys.ceremony_not_found`) e uma asserção não serve noutra (`401 passkeys.authentication_failed`); com `organizations.require_mfa`, a última chave e o único TOTP não saem (`409 security.last_factor_required`, antes de gastar o código); sem RP configurado `503 passkeys.not_configured`.

**Portão.** `server/tests/account_passkeys.rs` com o `SoftPasskey` do `webauthn-authenticator-rs` (assina de verdade). **Não validado:** com um autenticador de hardware e um browser real.

### R209 — «Os meus dados» só com dados da própria pessoa

**Regra.** Exportação assíncrona (`202`), uma de cada vez (`409 data_export.already_running`), 3 por 24 h (`429 data_export.rate_limited`). O ZIP leva perfil, preferências, gravações CARREGADAS pela pessoa como links, as transcrições dessas, a actividade em que é actora (alvos de acções sobre terceiros → `target_redacted`) e o uso G3. Link por HMAC, 15 min; assinatura errada, outro id ou vencido → `404`; o ficheiro apaga-se às 48 h. Limite escrito: uma transcrição de reunião contém a fala de outros participantes.

**Portão.** `server/tests/account_data_export.rs`; `isolamento.mjs` (A não lê nem pede link da exportação de B; sem assinatura 404).

### R272 — A dona de uma sala levava `500` ao espreitar a sua sala de espera

**Sintoma.** `GET /api/rooms/{code}/waiting` devolvia `500` («no column found for name: allow_guests») a quem tinha todo o direito de ver a sala de espera — a dona. O porte do convidado sem conta (R155) acrescentou `allow_guests` à sala e à constante `rooms::ROOM_COLUMNS`, mas este handler lia a sala com a lista de colunas **escrita à mão**, sem a coluna nova. É a mesma classe de defeito que já tinha obrigado a uma varredura das listas de colunas.

**Regra.** Uma `Room` lê-se sempre com `rooms::ROOM_COLUMNS`. Acrescentar uma coluna à sala é mexer na constante e em mais nada.

**Portão.** `server/tests/room_waiting.rs`: a dona recebe `200`, outra organização `403`/`404`, e sem sessão `401`; e `nenhuma_query_escreve_as_colunas_da_sala_a_mao` percorre `server/src` e falha se algum `SELECT` enumerar as colunas da sala. Controlo negativo feito: sem a correcção, os dois falham (`rooms.rs:833`).

**Como se soube.** Só o e2e de isolamento do CI o apanhou, na primeira vez que o `develop` passou por ele. O mesmo e2e tinha dois casos do convidado que não mediam nada: liam `/notes`, que não existe (passava por `404`), e escreviam a acta com `POST` numa rota que só tem `GET` e `PUT` (falhava por `405`). Passaram a usar `/minutes` com os métodos certos.

**Ficheiros.** `server/src/rooms.rs` (`room_waiting`), `server/src/application/recording_service.rs`, `server/tests/room_waiting.rs`, `web/e2e/isolamento.mjs`.

### R273 — Um ramal interno não tinha como entrar numa reunião

**Sintoma.** Um ramal registado só podia ligar a outro ramal da sua organização. A ponte telefone↔sala (ADR-0010) existia, mas a única porta para ela era o dial-in PSTN, que identifica a sala por `(DID, PIN)` — e um ramal não marca DID nenhum. O cabeçalho de `ramais.rs` dizia-o: «fase seguinte».

**O risco que a correcção cria.** O PIN de uma sala de voz só é único por DID. Procurar a sala só pelo PIN, sem DID, punha um ramal da org A dentro de uma reunião da org B que tivesse o mesmo PIN — ou a quem o PIN tivesse chegado.

**Regra.**
- Há um número curto RESERVADO, o número de acesso às reuniões: `VOICE_MEETING_ACCESS_NUMBER` (`config.rs`, 3–5 dígitos sem zero à esquerda, `8000` por omissão), o mesmo para todas as organizações. Configura-se num só sítio: o dialplan não o conhece, pergunta-o — `POST /api/voice/ivr/resolve-extension` responde `{"meeting_access": true}` e o `ramais_dial.lua` entrega a chamada a `dialin_ivr.lua ramal`.
- Nenhum ramal pode ter esse número: `POST /api/orgs/{org_id}/extensions` recusa com `409 ramais.extension_reserved`. Um ramal que já o tivesse deixa de ser alcançável por ele (o número reservado ganha) e o servidor avisa no arranque.
- O IVR valida o PIN em `POST /internal/v1/voice/ivr/validate-extension` (listener interno, segredo de voz), com `{sip_username, domain, pin}`. A organização sai do RAMAL (`voice_extensions.org_id`, pelo `sip_username`, que é globalmente único), nunca do pedido; o `domain` tem de ser o domínio SIP dessa org, o ramal tem de estar activo e o membro dono não pode estar arquivado. A sala é a sala de voz ACTIVA dessa org com esse PIN; duas com o mesmo PIN → recusa. Todas as recusas são o mesmo `404`, e contam para o travão de PIN, por ramal.
- O Lua lê a identidade de `sip_auth_username`/`sip_auth_realm` — o que o perfil `internal` autenticou por digest (`auth-calls=true`) — e desliga se faltarem. Não recua para o `From`.
- A resposta é a do dial-in (`room_bridge` incluído) e o resto do caminho é o mesmo código: `bridge` para a ponte, recuo para a conferência local. Quem entra por ramal aparece no censo como qualquer telefone (R224), porque o lugar nasce do `BridgeEvent::Started` da ponte, não do IVR.
- As leituras dos ramais (`GET`/`POST`/`PATCH /api/orgs/{org_id}/extensions…`) trazem `meeting_access_number` em cada ramal.

**Portão.** `server/tests/ramal_entra_na_sala.rs`, contra Postgres real: PIN da própria org → `room_bridge` igual ao do dial-in; ramal de A com o PIN de uma sala de B → `404` (com o controlo positivo do ramal de B); ramal inexistente, inactivo, com o domínio de outra org ou de membro arquivado → `404`; sem o segredo de voz → `401`; PIN em duas salas da org → `404`; ramal com o número reservado → `409 ramais.extension_reserved`; o número vem da configuração. Controlo negativo feito: sem o `vr.org_id = $2` na procura da sala, `ramal_de_outra_org_nao_entra_mesmo_com_o_pin` falha com o ramal de A dentro da sala de B. Unidade: `telephony::extension::tests`. Lua: só `scripts/check-lua-sintaxe.sh`.

**O que NÃO está provado.** Nenhuma chamada real: a imagem do FreeSWITCH do laboratório ainda não traz os sons do IVR, e o modo `ramal` do `dialin_ivr.lua` nunca correu. Por medir contra um FreeSWITCH real: que `sip_auth_username` e `sip_auth_realm` vêm preenchidos num INVITE autenticado do perfil `internal`; que `session:execute("lua", "dialin_ivr.lua ramal")` a partir do `ramais_dial.lua` corre o IVR com `argv[1]`; e a media de ponta a ponta (a R222 mede a ponte, não esta entrada). O censo (R224) também não foi medido por este caminho — decorre de a perna ser a mesma.

**Fora.** Não há CDR da chamada de um ramal para uma sala (o CDR do dial-in cobra a tarifa de entrada PSTN). O ramal entra como «Telefone», anónimo: a ponte não recebe a identidade de quem liga. O gRPC (`IvrService`) não tem o equivalente. Uma organização sem DID não cria salas de voz, logo não tem PIN para marcar. O DID de um ramal (Fase 2) continua a tocar na pessoa, não numa sala.

**Ficheiros.** `server/src/voice.rs` (`validate_pin_for_extension`), `server/src/ramais.rs`, `server/src/config.rs`, `server/src/lib.rs`, `server/crates/delonix-meet-domain/src/telephony/extension.rs`, `server/tests/ramal_entra_na_sala.rs`, `voice/freeswitch/scripts/{ramais_dial,dialin_ivr}.lua`, `voice/freeswitch/dialplan/default/00_delonix_extensions.xml`.

### R274 — Todo o PIN marcado ao telefone era «errado», e o IVR nem chegava a pedi-lo

**Sintoma.** Duas falhas em cadeia, vistas na primeira chamada que chegou ao IVR (2026-10-03, no laboratório do `compose.yaml`). **Primeira:** a imagem do FreeSWITCH não trazia os sons; o `dialin_ivr.lua` falhava a abrir o pedido do PIN, esgotava as três tentativas em milissegundos e desligava — o chamador ouvia silêncio e caía. **Segunda:** com os sons, o PIN certo era recusado. O `mod_curl` registava `content-type: (null)`: os dois scripts montavam o pedido como `post content-type=application/json '<corpo>' '<cabeçalho>'`, e o módulo quer as opções **antes** do método, cada uma com o valor separado por espaço. O pedido saía sem `Content-Type` e sem o segredo, o servidor respondia `415`, e o IVR tratava isso como PIN errado.

**Regra.** A imagem do FreeSWITCH traz os sons que os scripts tocam, fixados por versão e SHA-256, e o build parte se faltar um. Um pedido do IVR ao servidor escreve-se `content-type application/json append_headers 'X-Voice-Secret: …' post '<corpo>'`.

**Portão.** No build da imagem: os cinco ficheiros que os scripts tocam têm de existir nas duas vozes. Em chamada, **só à mão**, no laboratório: `asterisk -rx "channel originate PJSIP/<DID>@meet extension <PIN>@prova-pin"` — o `mod_curl` regista `content-type: application/json`, o servidor valida, e o IVR toca `conf-welcome` e entra na conferência. Não há portão automático do comportamento do IVR.

**O que NÃO está provado.** O telefone dentro da sala WebRTC: no compose a ponte para o SFU não está ligada, e com o PIN certo a chamada entra na conferência local do FreeSWITCH. O `ramais_dial.lua` levou a mesma correcção mas não foi exercitado por nenhuma chamada.

**Ficheiros.** `voice/freeswitch/image/Containerfile`, `voice/freeswitch/scripts/{dialin_ivr,ramais_dial}.lua`, `voice/cluster/freeswitch-entrypoint.sh`, `voice/pbx-cliente/extensions.conf`.

### R275 — As credenciais de um ramal não diziam onde o softphone se liga, e o diálogo sobrepunha os valores

**Sintoma.** Visto numa captura de ecrã da consola (2026-10-03). O diálogo «Credenciais SIP» punha utilizador, password e domínio lado a lado numa grelha de três colunas (`.org-voice__kpis`, feita para três números curtos): os valores longos sobrepunham-se, o domínio era cortado e o diálogo ganhava scroll horizontal. E o «domínio SIP» mostrado, `<slug>.ramais.delonix.meet`, é o realm do digest — um nome lógico, que não resolve em DNS: a API não devolvia o endereço a que o softphone se liga, e ninguém conseguia configurar um a partir do que a consola mostrava. Dois avisos do mesmo ecrã diziam o contrário do código: que a ponte telefone↔sala «ainda não existe» (existe desde o ADR-0010) e que «nenhum ramal entra numa sala» (entra pelo número de acesso, R273).

**Regra.**
- O endereço público do servidor SIP dos ramais é configuração da instalação: `VOICE_RAMAIS_PUBLIC_HOST`, `VOICE_RAMAIS_PUBLIC_PORT` (omissão 5070) e `VOICE_RAMAIS_PUBLIC_TRANSPORT` (`udp`|`tcp`|`tls`, omissão `udp`). As leituras de um ramal (lista, criação, `PATCH`, regeneração) trazem `sip_server: { host, port, transport, uri } | null`. **Sem host configurado — ou com um host mal formado — vai `null`**: o servidor não deriva o endereço do domínio SIP, do `Host` do pedido nem de outra variável. A forma (`is_sip_host`, `SipTransport`, `proxy_uri`) vive no domínio, sem IO.
- O `VOICE_RAMAIS_DOMAIN_SUFFIX` não mudou: entra no HA1, e mudá-lo invalida as passwords de todos os ramais existentes.
- Dados para copiar mostram-se um campo por linha, com quebra (`overflow-wrap: anywhere`) e um botão de copiar por campo — nunca numa grelha de colunas. Sem `sip_server`, a linha do servidor fica e diz que falta.
- Um aviso diz o que existe e a condição, não um estado que o ecrã não mede: a ponte só liga quando a instalação a configura, e a entrada de um ramal numa sala não foi verificada com uma chamada real.

**Portão.** `server/tests/ramal_entra_na_sala.rs`, contra Postgres real: `null` sem configuração; preenchido nas quatro leituras com ela; omissões de porta e transporte; host mal formado → `null`. Unidade: `telephony::extension::tests`. Web: `web/src/pages/admin/ExtensionsCard.test.ts` (ordem dos campos, servidor presente e ausente, número de acesso, nome acessível de cada botão, nenhuma chave crua nas quatro línguas). `scripts/check-openapi.sh`.

**O que NÃO está provado.** O layout: os testes de render não medem sobreposição nem scroll — o diálogo novo não foi visto num browser, a 375 px ou a outra largura. Que o endereço dos laboratórios é alcançável: no compose a porta 5070 só é publicada com `make compose-up LAN_IP=…`, e no cluster o serviço do FreeSWITCH é interno. Nenhum softphone foi configurado com os dados do diálogo.

**Ficheiros.** `server/crates/delonix-meet-domain/src/telephony/extension.rs`, `server/src/config.rs`, `server/src/ramais.rs`, `server/tests/ramal_entra_na_sala.rs`, `web/src/pages/admin/{ExtensionsCard,VoiceCard}.tsx`, `web/src/ui/org.css`, `web/src/locales/*/consola.ts`, `compose.yaml`, `scripts/{cluster,compose-lan}.sh`, `voice/README.md`.

### R276 — O ramal não tinha PIN, não havia ramal sem pessoa, e a lista dos ramais cortava os botões

**Sintoma.** Três faltas e um defeito, do item 3.8 do plano de produção (decisão do dono de 2026-10-04: número do ramal, password SIP e PIN são três coisas separadas). (1) O ramal não tinha PIN nenhum. (2) Só existia «um ramal por pessoa» (`member_id NOT NULL`): recepção, sala e portaria não tinham como ter ramal. (3) Os ramais criavam-se um a um, à mão. (4) A tabela dos ramais na consola tinha scroll horizontal a ~847 px de largura, com «Regenerar password» e «Apagar» cortados.

**Regra.**
- **O PIN** tem seis dígitos, é sorteado pelo servidor (aleatoriedade do SO, sem enviesamento: `pin_candidate` rejeita a cauda em vez de reduzir com o resto) e guarda-se **só em hash** (`auth::hash_password`, Argon2 — o helper das passwords, não uma cópia). Sai **uma vez**, na resposta que o gera. Nenhuma leitura o devolve: as leituras trazem `pin_state` (`unset` | `set` | `locked`).
- **Recusas**, iguais para o PIN gerado e para o escolhido (`telephony::extension_pin::pin_refusal`): não são seis dígitos (`ramais.pin_format`), dígitos todos iguais (`ramais.pin_repeated`), sequência ascendente ou descendente com a volta 9→0 (`ramais.pin_sequence`), e o PIN que **contém** o número do ramal (`ramais.pin_contains_extension`). «Igual ao número do ramal» não pode acontecer à letra — o ramal tem 3 a 5 dígitos e o PIN seis —, por isso recusa-se o que alguém faria para o tornar igual (`001234`, `123400`).
- **Quem vê o PIN de quem.** O de um ramal de PESSOA é dela: gera-o ou escolhe-o em `/api/orgs/{org_id}/my-extension` (`POST …/regenerate-pin`, `PUT …/pin`; na web, Definições → Segurança → «O meu ramal»). O administrador **não o vê nem o define** (`409 ramais.pin_belongs_to_member`): «forçar a regeneração» é `DELETE /api/orgs/{org_id}/extensions/{id}/pin`, que responde `204` sem PIN, deixa-o «por definir» e levanta o bloqueio — e a pessoa gera outro na sua área. O de um ramal da EMPRESA é do administrador: gera-o (`POST …/extensions/{id}/regenerate-pin`, visto uma vez) ou escolhe-o (`PUT …/extensions/{id}/pin`).
- **Verificação** (`extension_pin::verify_pin`): cinco falhas seguidas bloqueiam o PIN durante 15 minutos; bloqueado, nem o PIN certo passa e a tentativa não conta; um acerto zera o contador; ao bloquear, o contador volta a zero. A linha lê-se com `FOR UPDATE`. Cada falha (`ramal.pin_falhado`) e cada bloqueio (`ramal.pin_bloqueado`) ficam na auditoria imutável com o **actor de sistema** (`Uuid::nil()`) e o ramal no alvo — nunca em nome do dono do ramal, que é a vítima de quem adivinha; o PIN tentado nunca entra no registo. Está exposta em `POST /internal/v1/voice/ivr/verify-extension-pin` (listener interno, `X-Voice-Secret`), com resposta sempre `200` e `{valid, reason: invalid|locked|not_set, retry_after_secs, extension_id, member_id, display_name}`. Ramal inexistente, inactivo, de pessoa arquivada ou de outra organização responde como PIN errado.
- **Ramal da empresa:** `member_id` nulo, etiqueta obrigatória (`400 ramais.label_required`, também no `PATCH`; `CHECK` na tabela), sem PIN por omissão. As leituras trazem `member_id`, `member_username` e `member_email` a `null`. O caminho da R273 (`validate_pin_for_extension`) deixou de ler `member_id` como obrigatório.
- **Numeração automática:** intervalo por organização em `voice_extension_ranges` (`GET`/`PUT /api/orgs/{org_id}/extension-range`; por omissão 1000–1999; fora de 100–99999 ou invertido é `400 ramais.range_invalid`). `POST /api/orgs/{org_id}/extensions/assign-missing` dá um ramal a cada pessoa ACTIVA e humana sem ramal, por ordem crescente, saltando os números ocupados e o de acesso às reuniões. É idempotente, cria no máximo 100 por chamada (cada ramal custa um Argon2) e diz `remaining` e `range_exhausted`; a consola repete enquanto houver progresso. A resposta não traz passwords SIP nem PIN.
- **A lista dos ramais não é uma tabela.** Cada ramal é uma linha em grelha que quebra (`.org-ext`): identidade e estado em cima, número PSTN e acções por baixo, com `flex-wrap` e sem largura mínima.

**Portão.** `server/tests/ramal_pin.rs`, contra Postgres real (9 casos): o PIN sai uma vez e na base só há `$argon2…`; as recusas com o seu código; o administrador não gera nem escolhe o PIN de uma pessoa e o `DELETE` não traz PIN; cinco falhas bloqueiam, as falhas ficam com o actor de sistema e não em nome do dono do ramal (que é a vítima), dez palpites errados em paralelo contam exactamente cinco e bloqueiam, duas atribuições em massa em paralelo não repetem números nem pessoas, o PIN certo não passa bloqueado, a auditoria tem cinco `ramal.pin_falhado` e um `ramal.pin_bloqueado` sem o PIN tentado e com a cadeia intacta, e o bloqueio expira; a rota interna dá `401` sem o segredo de voz e não atravessa organizações; ramal da empresa sem etiqueta é recusado; a atribuição em massa é idempotente, salta o `8000` e um número ocupado, ignora arquivados e diz quando o intervalo se esgota. Unidade: `telephony::extension_pin::tests` e `extension_pin::tests`. Isolamento: nove linhas novas em `web/e2e/isolamento.mjs`. Web: `web/src/pages/admin/ExtensionsCard.test.ts` (estado do PIN por linha, botões por tipo de ramal, nenhuma `<table>`, «o meu ramal», quatro línguas sem chave crua).

**Visto num browser, uma vez, à mão** (2026-10-04, servidor de teste sobre a base do `#[sqlx::test]` e `vite` em `localhost`, medido por geometria do DOM — nenhuma captura de ecrã): a 847 px e a 375 px o cartão dos ramais não tem scroll horizontal nem elementos fora da sua caixa, com sete ramais (pessoa e empresa, os três estados do PIN); «Atribuir ramais a todos» criou três ramais; «Gerar PIN» de um ramal da empresa e «Gerar PIN novo» em «O meu ramal» mostraram seis dígitos uma vez; `123456` foi recusado com a frase da sequência. Não é portão: nada disto corre no CI.

**O que NÃO está provado.**
- **Nenhuma chamada usa o PIN.** A rota de verificação não tem consumidor: o `dialin_ivr.lua` não mudou. Identificar quem liga de fora por ramal+PIN, o nome no censo e o anfitrião por telefone são do lote seguinte.
- **Limites conhecidos da verificação — pré-condição do lote que ligar o IVR.** O lote do IVR **não liga a verificação** sem um travão por origem e um bloqueio de duração crescente. Até lá:
  - **(i) O bloqueio serve para negar serviço a um colega.** Cinco PIN errados a cada 15 minutos mantêm um ramal bloqueado indefinidamente, e os números dos ramais são sequenciais (1000, 1001, …): quem quiser bloqueia a organização inteira. Não há travão por origem.
  - **(ii) A adivinhação online não escala o custo.** Cinco palpites por 15 minutos, sem escalada, são 480 por dia por ramal; e em largura — o mesmo PIN em muitos ramais — o bloqueio por ramal não trava nada.
  - **(iii) O contador não tem janela.** Só zera com um acerto (ou ao bloquear): quatro falhas de há um mês e uma de hoje bloqueiam.
  - **(iv) A resposta e o tempo distinguem os casos.** `invalid`, `not_set` e `locked` são respostas diferentes, e só o caminho com PIN definido paga um Argon2. A rota é para o IVR, não para quem liga: **o Lua não pode dar mensagens diferentes a quem liga** — uma só recusa, igual para as três razões.
- **Um membro novo não recebe ramal ao entrar.** Há nove sítios que inserem em `org_members`; ligar a atribuição a todos ficou de fora. Hoje o administrador carrega em «Atribuir ramais a todos» depois de juntar pessoas.
- **Os ramais criados em massa têm uma password SIP que ninguém viu**: o administrador regenera-a por ramal. O QR de provisionamento do Linphone (3.8) resolve isto e não está feito.
- **Argon2 sobre seis dígitos não resiste a quem roube a base** (10⁶ candidatos). O hash protege de uma leitura casual, não de um ataque offline; o que protege o PIN em uso é o bloqueio.
- O `isolamento.mjs` não foi corrido (precisa de servidor e Postgres próprios). A 375 px o diálogo de Definições inteiro é mais largo que o ecrã e o cartão de Membros da consola transborda — os dois já eram assim e não foram tocados.

**Ficheiros.** `server/migrations/0087_ramal_pin_e_ramais_da_empresa.sql`, `server/crates/delonix-meet-domain/src/telephony/extension_pin.rs`, `server/src/{extension_pin,ramais,voice,lib}.rs`, `server/tests/ramal_pin.rs`, `scripts/check-openapi.sh`, `docs/reference/openapi/*.json`, `web/e2e/isolamento.mjs`, `web/src/api.ts`, `web/src/pages/admin/ExtensionsCard.tsx`, `web/src/components/{MyExtensionPanel,PinOnce,SettingsDialog}.tsx`, `web/src/ui/{org,shell}.css`, `web/src/locales/*/{consola,shell}.ts`.

### R277 — O bordo anunciava-se como `0.0.0.0`, e todas as chamadas por ele eram cortadas aos 32 segundos

**Sintoma.** Uma chamada que entrava pelo Kamailio era atendida, o IVR falava, o PIN era aceite — e exactamente 32 s depois o FreeSWITCH desligava-a com `NORMAL_UNSPECIFIED`. Com qualquer origem: uma central FreePBX por tronco TLS e um softphone. Nunca se tinha visto porque a chamada de prova do `compose-voice-check` e do `cluster-voice` dura 4 s (`application Wait 4`), e as provas da ponte (R222) não passam pelo bordo.

**Causa.** O `kamailio.cfg` escutava em `0.0.0.0` (`listen=udp:0.0.0.0:5060` e as outras duas) sem `advertise`. O Kamailio escrevia literalmente `Record-Route: <sip:0.0.0.0:5061;transport=tls;r2=on;lr>` — o endereço para onde quem liga tem de mandar o `ACK` e o `BYE`. O `ACK` não saía (o baresip diz `Invalid argument`), o FreeSWITCH repetia o `200 OK` e, sem confirmação, desligava ao fim do temporizador. Medido com o rasto SIP no FreeSWITCH: onze `200 OK` enviados, zero `ACK` recebidos.

**Regra.** O bordo escuta numa **interface**, não em `0.0.0.0`: `listen=udp:DELONIX_SIP_IFACE:5060` (por omissão `eth0`, a de um contentor ou de um pod — confirmado no docker, no motor delonix e no cluster local), e o Kamailio anuncia o endereço dela. Um bordo atrás de NAT, ou com o endereço público noutro equipamento, dá-o em `DELONIX_SIP_ADVERTISE` (`listen=… advertise <endereço>:<porto>`). As duas variáveis lêem-se do ambiente com `#!trydefenv`; sem nenhuma, o ficheiro funciona tal como está.

**Portão.** `scripts/check-bordo-anuncia.sh` (`make fitness` e CI): estático, recusa um `listen=` em `0.0.0.0`, `*` ou `[::]` sem `advertise`; controlo negativo corrido contra o ficheiro anterior (as três linhas, com o número). `bash scripts/pbx-tronco-prova.sh longa` — **fora do CI**: uma chamada de 40 s por TLS com SRTP; exige o endereço real no `Record-Route`, o `ACK` enviado e nenhum `NORMAL_UNSPECIFIED`.

**Prova corrida a 2026-10-04**, numa réplica do bordo (`voice/pbx-tronco-prova/`). Antes: `Record-Route: <sip:0.0.0.0…>`, chamada cortada aos 32,08 s. Depois: `Record-Route: <sip:172.30.50.14:5061;transport=tls…>`, um `ACK`, chamada de 40 s e de 45 s até o softphone desligar. **Por medir:** o laboratório do `compose.yaml` e o cluster local com a correcção (correm o mesmo ficheiro, mas não foram reiniciados nesta entrega), e um bordo atrás de NAT com `DELONIX_SIP_ADVERTISE` — a variante só foi validada com `kamailio -c`, que mostra `advertise tls:203.0.113.9:5061`.

**Ficheiros.** `voice/kamailio/kamailio.cfg`, `scripts/check-bordo-anuncia.sh`, `scripts/pbx-tronco-prova.sh` (modo `longa`), `Makefile` e `.github/workflows/ci.yml` (o portão).

### R278 — Ninguém via a password SIP dos ramais criados em massa, e quem entrava na organização ficava sem ramal

**Sintoma.** Duas faltas do item 3.8 do plano de produção (lote 3), as duas deixadas em aberto pela R276. (1) A password SIP é longa, aleatória e mostrada uma vez; na atribuição em massa ninguém a vê, e o administrador tinha de a regenerar ramal a ramal e ditá-la. A decisão do dono é que ninguém a digite. (2) Só existia a acção em massa: quem entrava depois dela ficava sem ramal até alguém voltar a carregar no botão.

**Regra — o Linphone configura-se por QR de uso único.**
- **Emissão, com sessão.** A pessoa para o seu ramal (`POST /api/orgs/{org_id}/my-extension/provisioning-ticket`) e o administrador para qualquer ramal da organização (`POST /api/orgs/{org_id}/extensions/{id}/provisioning-ticket` — já pode regenerar a password SIP de qualquer ramal; isto não lhe dá poder novo). São *custom methods*: respondem `200` com `{provisioning_url, expires_at, extension}`, sem `Location` (o URL é um segredo; o tipo não deriva `Debug` para um `{:?}` não o pôr num registo). O bilhete é um token de 256 bits da aleatoriedade do SO; **só o SHA-256 fica na base** (`voice_extension_provisioning_tickets`), vale 10 minutos, e emitir outro apaga o anterior — um bilhete vivo por ramal.
- **O URL é público a sério.** Constrói-se da primeira origem de `CORS_ORIGINS` (a mesma de onde sai o retorno do SSO), nunca do `Host` do pedido, e só se for `https://host[:porta]` e não for um nome que o telefone não resolve (`localhost`, `*.svc`, e qualquer `*.local` — domínio mDNS, que o Android em geral não resolve por DNS; cobre `*.cluster.local`): senão `422 ramais.public_url_missing`. **Consequência:** o `compose.yaml` e o cluster local, que usam `meet.ngolacloud.local`, deixam de emitir QR até `CORS_ORIGINS` apontar a um nome que o telefone resolva (um domínio, `sslip.io` ou um túnel). Sem `VOICE_RAMAIS_PUBLIC_HOST` é `422 ramais.sip_server_missing` — não se emite um QR que não leva a lado nenhum. Ramal inactivo: `422 ramais.extension_inactive`.
- **Resgate, sem sessão** (`GET /api/public/extension-provisioning/{token}`; o telefone não tem conta, e está em `scripts/rotas-publicas.txt` com a razão). Só um bilhete vivo custa um Argon2 (pergunta-se primeiro, sem bloqueio); o segredo calcula-se **antes** de abrir a transacção, e dentro dela tudo se lê e escreve pela mesma ligação — com a linha do ramal bloqueada não se vai ao pool. Gasta o bilhete com um `UPDATE … WHERE consumed_at IS NULL AND expires_at > now()` — de dois resgates em paralelo só um ganha —, grava uma **password SIP nova** (`ramais::SipSecret`: Argon2 para repouso, HA1 para o directório do FreeSWITCH) e devolve a configuração `lpconfig` do Linphone com `Content-Type: application/xml` e `Cache-Control: no-store`. Inexistente, mal formado, já usado, expirado, ramal inactivo ou de quem já saiu: **sempre o mesmo `404 ramais.provisioning_invalid`**. É um `GET` com efeito — é assim que o Linphone descarrega a configuração.
- **Um bilhete morre com o que o tornava válido.** O de um ramal inactivo (ou de quem saiu) **gasta-se** na tentativa — reactivar o ramal não ressuscita um QR antigo. Regenerar a password SIP pelo administrador **apaga** os bilhetes do ramal — senão o QR por ler trocava a password nova outra vez.
- **O que protege a rota pública são os 256 bits e o uso único.** Há um limite por IP (`provisioning_limiter`, 20/min, `429` + `Retry-After`), mas não é ele a barreira — ver «aberto» abaixo.
- **Onde o token fica e onde não fica.** Na auditoria não: `ramal.provisionamento_emitido` tem quem pediu e o número do ramal; `ramal.provisionado` tem o actor de sistema (`Uuid::nil()`), o número do ramal **e o IP de quem resgatou** (`1004 ← 203.0.113.9`). No `tracing` do servidor não: o caminho sai redigido do span HTTP (`extension_provisioning::redact_path`). No registo de acessos dos três nginx do repositório não: `deploy/nginx-delonix.conf`, `deploy/k8s/nginx.conf` e `deploy/compose/edge.conf` têm uma `location` própria com `access_log off`.
- **A configuração** (`telephony::extension_provisioning::linphone_config_xml`, sem IO): uma conta por omissão, identidade `sip:<sip_username>@<domínio SIP>`, registo e rota para o **endereço público** (`sip_server`, nunca o domínio lógico) e `media_encryption=srtp` obrigatório — o perfil dos ramais não aceita áudio em claro.
- **Ler o QR troca a password SIP:** o aparelho antigo deixa de registar. O diálogo «Configurar o Linphone» (em «O meu ramal» e em cada ramal activo da consola) di-lo ANTES de emitir, mostra o QR com o gerador que o MFA já usa (`qrcode`), conta o tempo de validade e diz que o QR se lê **só com o Linphone**. **O URL não aparece em texto copiável** — só como recurso, quando a imagem do QR não se conseguiu desenhar.

**Regra — ramal automático a quem entra por um acto de um administrador ou de um IdP.**
- **Uma definição por organização, desligada por omissão:** `auto_assign_on_join`, na mesma linha e na mesma rota do intervalo (`GET`/`PUT /api/orgs/{org_id}/extension-range`). No `PUT` o campo é opcional e **ausente = manter**: um cliente que só conhece o intervalo não a desliga sem querer. Ligá-la **não** dá ramal a quem já cá está: isso continua a ser `assign-missing`.
- **Um só ponto:** `ramais::assign_on_join(state, org_id, user_id)`, chamado depois do commit. Decide tudo lá dentro: a organização tem-na ligada, a pessoa ocupa lugar (`org::seat_holder_username` — activa, humana, **não convidado externo**; o utilizador de serviço e os convidados sem conta ficam de fora) e ainda não tem ramal. Dá o primeiro número livre do intervalo, saltando ocupados e o de acesso às reuniões, com o mesmo `INSERT` da criação manual — e **um só Argon2**, gerado fora do ciclo que tenta os números (o mesmo vale para o `assign-missing`: um por pessoa, não um por número tentado).
- **Nunca faz falhar a entrada.** Não devolve erro. Intervalo esgotado: o membro entra sem ramal e fica `ramal.atribuicao_automatica_falhou` na auditoria (actor de sistema, a pessoa no alvo). Sucesso: `ramal.atribuido_ao_entrar`.
- **Quem chama — houve um acto de alguém com autoridade:** o administrador junta um colaborador (`org::add_employee`, só pertença nova), o convite aceite (`directory::accept_invitation`), a reactivação em massa, o SSO OIDC com criação de conta (`auth::sso_callback`) e a pertença vinda do Odoo (`org::ensure_odoo_membership`, só quando nasce).
- **Quem NÃO chama, e porquê:**
  - **o auto-registo (`auth::register`).** O registo não verifica o email (aberto conhecido, `delonix-meet-backend` §Segurança). Numa instalação de organização única (`TENANCY_MODE=single`) com `REGISTRATION_MODE=open`, quem se regista entra na organização sem acto de ninguém: com ramal automático, um desconhecido ficava com uma conta SIP — pedia o seu próprio bilhete e registava — e, repetindo, esgotava o intervalo. Enquanto o email não for verificado, o auto-registo não dá ramal; o administrador atribui-o.
  - **o convidado de uma reunião criada pela API v1 (`meetings_v1::resolve_org_user`).** Cria conta e pertença para qualquer endereço do domínio, exista ou não, a pedido de uma chave de API: não é uma entrada decidida por um administrador, e custava um Argon2 por convidado.
  - a criação de uma organização (`org::create_org`, e o administrador que o registo cria com ela) e os dois sítios que inscrevem o utilizador de serviço (`apikeys::v1_provision_org`, `odoo_sso::ensure_org`).

**Portão.** `server/tests/ramal_provisionamento.rs`, contra Postgres real (7 casos): o dono emite e o resgate traz uma password NOVA cujo HA1 é o que `ivr/directory` devolve ao FreeSWITCH e cujo Argon2 guardado a verifica; segundo resgate, token inventado e token mal formado dão o mesmo `404`; a auditoria tem a emissão com o actor real e o resgate com o actor de sistema e o IP, sem o token; o administrador emite para ramal de pessoa e da empresa, e emitir outro mata o QR anterior; administrador da org A não emite para ramal da org B (por nenhum dos dois caminhos), um membro não emite pela rota do administrador nem sem ramal próprio; expirado dá `404`; ramal desactivado depois da emissão dá `404`, o bilhete fica gasto e reactivar o ramal não o ressuscita; regenerar a password mata o QR por ler e a password que o administrador viu continua a registar; dois resgates em paralelo dão exactamente um `200`, e a password na base é a do vencedor; sem servidor SIP ou com origem interna a emissão é `422` e não grava bilhete; o 21.º resgate do mesmo IP num minuto é `429` com `Retry-After`. `server/tests/ramal_ao_entrar.rs` (5 casos): desligada por omissão; ligada, quem o administrador junta recebe o primeiro número livre, sem PIN, e quem já cá estava não; um `PUT` sem o campo mantém-na ligada; não cria segundo ramal; isolada por organização; intervalo esgotado não parte a entrada e fica na auditoria; convite aceite dá ramal e convidado externo não; **convidado de reunião pela v1 não recebe ramal; quem se regista sozinho numa instalação `single` + `open` entra na organização e não recebe ramal nem tem bilhete para pedir**. Unidade: `telephony::extension_provisioning::tests` (forma do bilhete, origem pública, XML e escapes) e `extension_provisioning::tests` (redacção do caminho). Web: `ExtensionsCard.test.ts` (aviso antes de emitir, QR e tempo sem o URL em texto, URL só sem imagem, expirado, uma frase por recusa, botão só em ramal activo, quatro línguas sem chave crua). `check-route-auth.sh`, `check-openapi.sh`, `check-isolamento-cobertura.sh` (duas linhas novas em `web/e2e/isolamento.mjs`).

**Aberto — defeitos que este lote encontra ou alarga e não fecha.**
- **O limite por IP contorna-se com `X-Forwarded-For`** (anterior a este lote; afecta também `/api/auth/*`, o emparelhamento do estúdio e a entrada de convidados). `rate_limit::client_ip` usa o PRIMEIRO elemento do cabeçalho quando o par é um endereço privado, e os proxies do repositório fazem `$proxy_add_x_forwarded_for` — acrescentam ao que o cliente mandou. Quem forjar o cabeçalho escolhe o seu balde a cada pedido. Não se corrigiu aqui: a correcção certa (contar da direita com um número de saltos de confiança por instalação, ou um cabeçalho que só o proxy de borda escreve) muda todas as topologias de uma vez — o cluster tem dois saltos e o `deploy/k8s/nginx.conf` nem põe `X-Real-IP` —, e há um teste de unidade que fixa o comportamento actual. Nesta rota a barreira é o token; nas de autenticação o travão que conta é o que é por conta (`login_limiter`), não o por IP.
- **Quem sai da organização continua a registar com a password antiga.** O directório do FreeSWITCH só filtra `active` do ramal, e arquivar um membro não desactiva o ramal dele. É anterior, mas este lote multiplica-o: com o ramal automático há muito mais ramais de pessoa. O resgate de um QR já recusa o ramal de quem saiu; o registo SIP não. **Trabalho seguinte.**
- ~~O ingress-nginx regista o caminho~~ — **fechado a 2026-10-05, medido no cluster de laboratório.** No cluster o `/api` vai do ingress directo ao servidor, sem passar pelo nginx do web, e o bilhete ficava no registo de acessos do controlador. O chart (`deploy/helm/delonix-meet/templates/ingress.yaml`) e os manifestos (`deploy/k8s/04-ingress.yaml`) têm agora um `Ingress` próprio, `delonix-provisioning`, para `/api/public/extension-provisioning`, com `nginx.ingress.kubernetes.io/enable-access-log: "false"`; `check-helm.sh` e `check-k8s-render.sh` falham se a rota perder esse Ingress ou a anotação (provado por mutação nos dois). **Medido** no cluster local (ingress-nginx): antes, um resgate com bilhete inventado deixava 1 linha no registo do controlador; com o `Ingress` dedicado aplicado, 0 linhas, o servidor continua a responder 404, e dois pedidos de controlo (`/api/openapi.json` e outra rota em `/api/public/`) continuam registados. **Limite:** um ingress que não seja o ingress-nginx ignora a anotação, e um proxy à frente do ingress continua a poder registar o caminho.
- **O QR é uma saída nova de password SIP sem reautenticação** — equivalente ao `regenerate-password`, que também não a pede. A R214 fechou a LEITURA de credenciais SIP de troncos atrás de reautenticação; a troca da password de um ramal (por regeneração ou por QR) continua a bastar-se com a sessão.
- **`GET` com efeito.** Qualquer coisa que abra o URL — a câmara do telemóvel, um leitor de QR genérico, uma pré-visualização de link — gasta o bilhete e troca a password, e o aparelho antigo deixa de registar. O diálogo di-lo e já não mostra o URL em texto; não há como o impedir do lado do servidor sem deixar de servir o Linphone.
- **A password vai em claro no XML** (`passwd`, não `ha1`), sobre `https`. E o transporte SIP por omissão é UDP com SRTP SDES: as chaves de media seguem em claro na sinalização. `VOICE_RAMAIS_PUBLIC_TRANSPORT=tls` é o que fecha isso, e não é a omissão.

**O que NÃO está provado.**
- **Um Linphone leu o QR e ficou a funcionar (laboratório compose, 2026-10-04); o resto do aparelho continua por medir.** A auditoria tem três `ramal.provisionado` (ramal 101 duas vezes, ramal 1000 uma) e, 95 segundos depois do resgate do ramal 1000, esse ramal ligou para uma sala como ramal registado (R279) — logo o `lpconfig` (secções `sip`, `auth_info_0`, `proxy_0`; chaves `reg_proxy`, `reg_route`, `reg_identity`, `media_encryption`), escrito de memória da documentação, foi aceite por um Linphone real, um só resgate bastou, e o aparelho autenticou-se com um realm que não é o host do proxy. O dono confirma que não digitou a password. **Não ficou registado nem medido:** a versão do Linphone e do Android; se `media_encryption_mandatory` é respeitado; se o aparelho faz um segundo `GET` (receberia `404`, e aqui não o impediu de registar); e se o Linphone confia numa raiz instalada pelo utilizador — os dois resgates que serviram um Linphone chegaram por um endereço público (túnel, certificado de AC pública); o terceiro veio da rede local, mas a auditoria não diz que cliente o fez.
- **O diálogo não foi visto num browser** a 375 px nem a 847 px: os testes de render não medem scroll horizontal. O CSS foi escrito para não o ter (`width: min(220px, 100%)`, URL de recurso com `overflow-wrap: anywhere`), e isso é intenção, não medida.
- **As três `location` do nginx não foram carregadas por um nginx** (`nginx -t` precisa dos certificados e dos nomes de cada ambiente).
- **Sem teste:** que o Argon2 se calcula uma vez por ramal e fora da transacção (é estrutura do código, não comportamento observável pela API); a redacção do caminho no span só tem teste de unidade.
- **Caminhos de entrada sem teste:** o SSO OIDC, a pertença do Odoo e a reactivação em massa chamam `assign_on_join` mas nenhum teste o prova.
- **`assign-missing` e `assign_on_join` não escolhem as mesmas pessoas:** a acção em massa (R276) usa `org::active_member_subjects`, que inclui convidados externos; a automática exclui-os. Não foi alinhado aqui.
- O `isolamento.mjs` não foi corrido (precisa de servidor e Postgres próprios).

**Ficheiros.** `server/migrations/{0092_ramal_provisionamento_por_qr,0093_ramal_automatico_ao_entrar}.sql`, `server/crates/delonix-meet-domain/src/telephony/extension_provisioning.rs`, `server/src/{extension_provisioning,ramais,org,auth,directory,meetings_v1,lib}.rs`, `server/tests/{ramal_provisionamento,ramal_ao_entrar,ramal_pin}.rs`, `scripts/rotas-publicas.txt`, `deploy/{nginx-delonix.conf,k8s/nginx.conf,compose/edge.conf}`, `docs/reference/openapi/*.json`, `web/e2e/isolamento.mjs`, `web/src/api.ts`, `web/src/components/{LinphoneQrDialog,MyExtensionPanel}.tsx`, `web/src/pages/admin/ExtensionsCard.tsx`, `web/src/ui/org.css`, `web/src/locales/*/consola.ts`.

### R279 — O PIN do ramal não tinha consumidor, e a verificação servia para bloquear o ramal de um colega

**Sintoma.** A R276 deixou a verificação do PIN (`/internal/v1/voice/ivr/verify-extension-pin`) sem consumidor e com quatro limites escritos como pré-condição de a ligar: (i) cinco PIN errados bloqueavam o ramal de outra pessoa, e os ramais são sequenciais; (ii) quinze minutos fixos para sempre; (iii) o contador só zerava num acerto — quatro enganos de há um mês e um de hoje bloqueavam; (iv) resposta e tempo distinguiam os casos. E quem entrava por telefone era sempre «Telefone», sem nome, mesmo ligando do seu próprio ramal autenticado.

**Regra — a verificação (parte A).**
- **Travão por ORIGEM, antes do ramal.** O pedido passa a exigir `origin` (`caller_number`, `network_ip` — o `caller_id_number` e o `sip_network_ip` do FreeSWITCH); sem ele é `422`. A chave é `<rede>|<número>`; uma chamada sem número partilha uma só chave por rede. A origem é **cobrada antes** de se verificar (`extension_pin::charge_origin`, linha em `FOR UPDATE`) e a falha é **devolvida** num acerto (`refund_origin` — só essa: um acerto não zera o contador). Trava à **terceira** falha em quinze minutos (`ORIGIN_THROTTLE`), antes de a mesma origem poder juntar as cinco de um ramal; o primeiro bloqueio da origem dura **vinte** minutos — mais que a janela do ramal, com folga verificada ao compilar (`ORIGIN_LOCK_MARGIN_SECS`) —, para que as falhas que ela deixou num ramal já tenham saído da janela dele quando volta a poder tentar. Num acerto que cai sobre o bloqueio que ele próprio causou, a origem volta a ficar como estava antes do pedido (nível, memória do bloqueio anterior, início da janela). As origens que já não contam (sem mexer há 48 h e com o último bloqueio esquecido) são apagadas a cada verificação. Travada, a resposta é `origin_locked` e **nem a organização nem o ramal são lidos**: nem o PIN certo passa.
- **Onde vive o estado: Postgres** (`voice_pin_origins`, migração `0091`), não o `RateLimiter` em memória. O servidor corre em três réplicas e o pedido do FreeSWITCH cai em qualquer uma; um contador por processo triplicava as tentativas. O Redis do repo é barramento e estado de sala, opcional, e não tem travão canónico.
- **Janela e duração crescente**, para o ramal e para a origem (`telephony::extension_pin::Throttle`, sem IO): as falhas contam numa janela de quinze minutos, **estrita** (aos 900 s exactos a falha já não conta: as idades vêm da base em segundos inteiros, e «≤» fazia a janela durar um segundo a mais); o bloqueio do ramal é de 15 min, depois 30, 60… até 24 h (o da origem, 20, 40, 80…); o nível esquece-se um dia depois do fim do último bloqueio, e um acerto no ramal zera-o.
- **Auditoria:** `ramal.pin_falhado` e `ramal.pin_bloqueado` levam a origem no alvo (`… — origem +244… via 10.…`) e o nível do bloqueio; `ramal.origem_travada` regista a origem travada. O actor continua a ser o de sistema; o PIN tentado nunca entra.
- **Tempo:** os caminhos sem PIN para verificar (ramal inexistente, de pessoa arquivada, PIN por definir, domínio de ninguém) gastam o mesmo Argon2 que um PIN errado.

**Regra — quem liga, identificado (parte B).**
- **Ramal registado** (`dialin_ivr.lua ramal`): não se pede PIN pessoal. `validate_pin_for_extension` resolve o ramal → pessoa (`COALESCE(display_name, username)`) ou etiqueta do ramal da empresa, e emite um **bilhete**.
- **De fora** (dial-in): depois do PIN da sala o IVR pede o ramal («…depois a tecla sustenido»; só cardinal ou silêncio é continuar anónimo), depois o PIN pessoal, e chama `verify-extension-pin` com a origem e o `voice_room_id`. O domínio vem do `validate` (`org_sip_domain`). Uma sala que não é activa na organização do domínio responde como PIN errado, sem ler o ramal. **Uma só frase de recusa** (`ivr-pin_or_extension_is-invalid.wav`) para todas as razões, duas tentativas por chamada, e a pessoa **entra na mesma**, anónima.
- **O bilhete** (`server/src/voice_caller.rs`, tabela `voice_caller_tickets`): 64 hex, só o hash guardado, 45 segundos, uso único, válido para UMA sala; se a ponte recusar o `INVITE` que o traz (`BridgeEvent::Refused`), é invalidado na hora (`voice_caller::discard`). Vai nas `channel_vars` (`sip_h_X-Delonix-Caller-Ticket`), que o Lua já copia para a perna da ponte sem as conhecer. O UA SIP leva-o no `BridgeEvent::Started`, e `voice::seat_phone_caller` troca-o pelo nome ao sentar a chamada no censo: nome da pessoa e sem o crachá «sem nome». Sem bilhete, gasto, expirado ou de outra sala: «Telefone», anónimo. **O Lua não é a fonte do nome:** a resposta do `verify-extension-pin` leva `display_name`, `member_id` e `extension_id` até ao FreeSWITCH, mas o Lua não os usa nem os regista — o que decide o nome no censo é o bilhete.
- **Os cabeçalhos `X-Delonix-*` só nascem dentro.** O Kamailio tira-os no bordo do tronco (`remove_hf_re("^X-Delonix-")`), e o `dialin_ivr.lua` tira-os da perna que recebe antes do `bridge` (os ramais registam-se directamente no FreeSWITCH, sem passar pelo Kamailio). O DID marcado e o número de quem liga passam por `limpa()` antes de entrarem no JSON do `mod_curl`.
- **Tempo de resposta.** Ramal inexistente, de pessoa arquivada, com o PIN por definir ou **bloqueado** gastam o mesmo Argon2 que um PIN errado. A origem travada (`origin_locked`) **não** gasta, de propósito: não leu ramal nenhum, por isso o tempo dela não diz nada sobre ramais, e é o ramo que tem de ser barato — é o que responde a quem está a abusar, e queimar CPU aí dava a quem abusa um custo que hoje não nos impõe.
- **O segredo de voz nunca vai em query string** (R227), e o Lua não escreve no log o ramal, o PIN pessoal nem as variáveis da perna.

**Portão.** Contra Postgres real: `server/tests/ramal_pin_origem.rs` (9 casos — na fronteira exacta do bloqueio a origem não completa as cinco do ramal; as origens velhas são limpas e as que contam ficam; a devolução repõe nível, memória e janela, e a auditoria diz 20 e 40 min; a mesma origem a falhar em três ramais é travada, o quarto ramal e o PIN certo já não são verificados, e nenhum ramal bloqueia; dez em paralelo da mesma origem: três chegam ao ramal, sete `origin_locked`; um acerto não zera a origem; o bloqueio do ramal dá 900 → 1800 → 3600 s e volta a 900 um dia depois; a janela expira no ramal e na origem; sem `origin` é `422`), `server/tests/ivr_identifica_quem_liga.rs` (4 casos — o bilhete vale 45 s e um recusado pela ponte deixa de valer; ramal registado entra com o nome sem PIN pessoal, a etiqueta no ramal da empresa, bilhete de uso único, só para a sua sala e com prazo; de fora, acerto traz bilhete e o nome no censo, falha e sala de outra organização não trazem nada; três falhas travam a origem e não o ramal) e `server/tests/ramal_pin.rs` (os 9 da R276, cada chamada com origem própria). Unidade: `telephony::extension_pin::tests` (janela, dobra, tecto, esquecimento,) e a invariante «a origem trava antes do ramal», que é uma asserção em tempo de compilação e `phone_bridge::sip::tests::o_bilhete_de_quem_liga_so_tem_uma_forma`. O Lua: `scripts/check-lua-sintaxe.sh`. Os três sons novos estão na verificação de construção da imagem (`voice/freeswitch/image/Containerfile`).

**O que NÃO está provado.**
- **Só o caminho do ramal registado correu numa chamada (laboratório compose, 2026-10-04).** Dois Linphones reais ligaram para a mesma sala: o servidor emitiu um bilhete por cada um (`voice_caller_tickets`, ramais 101 e 1000, com o nome da pessoa) e cada bilhete foi trocado pelo nome 2 segundos depois (`used_at` preenchido com a validade de 45 s intacta — é o `redeem`, não a invalidação). Fica provado, nesse caminho, que o cabeçalho `X-Delonix-Caller-Ticket` chega ao UA da ponte a partir da variável `sip_h_…` no prefixo da dial string e que o nome entra no censo; o dono viu o crachá de telefone com o nome e relata áudio nos dois sentidos (relato, não medição). Os registos dos contentores perderam-se num `compose-down`. **Continua por confirmar contra um FreeSWITCH real o caminho de quem liga de fora:** `session:read(0, 5, …)` a devolver vazio com só cardinal ou silêncio; `caller_id_number` e `sip_network_ip` preenchidos na perna do dial-in; a verificação do PIN pessoal, a recusa e o travão por origem numa chamada; e as frases a soarem bem em sequência. Nos testes, quem faz de IVR e de ponte é o próprio teste.
- **O anfitrião por telefone não ganhou nada novo.** Quem entra por telefone já não passa pela sala de espera (`join_external`, R224) — identificado ou não. A sala ao vivo não tem um estado «à espera do anfitrião» que um telefone pudesse abrir, e dar `is_host` a uma perna sem cliente mexe na transferência de anfitrião e na moderação: ficou por desenhar. O bilhete traz a pessoa (`member_id`), que é o gancho para isso.
- **O número de quem liga pode ser forjado.** Quem rode o número a cada chamada tem uma origem nova de cada vez: aí só o contador do ramal trava, e cinco falhas de origens diferentes continuam a bloquear um ramal (agora com janela, e com a origem de cada uma na auditoria). Atrás do Kamailio o `network_ip` é sempre o do Kamailio: serve de espaço de nomes, não de travão por tronco.
- **O bilhete aparece no `freeswitch.log`** na linha `EXECUTE … bridge([…])` que o próprio FreeSWITCH escreve, ao lado das outras variáveis da perna. Aparece também no CDR (`mod_json_cdr`: o `app_log` e a variável da perna B). É de uso único, vale 45 segundos, é gasto nesse instante e invalidado se a ponte recusar a perna; não é o segredo de voz nem o PIN. **Se o `INVITE` nem chegar à ponte** (ela em baixo, a chamada caída na conferência local), o bilhete fica válido até expirar: não há quem avise.
- **O travão por origem pode negar a IDENTIFICAÇÃO a terceiros** (nunca a entrada: entra-se na mesma, anónimo): (a) quem forje o número da vítima e falhe três vezes trava-a de 20 minutos a 24 horas; (b) um PBX de cliente que apresenta um só número de tronco é UMA origem — três enganos de qualquer colaborador travam todos; (c) as chamadas sem número partilham uma chave por rede, e atrás do Kamailio a rede é uma só: é uma chave global, de todas as organizações. **O número não é normalizado:** `+244…`, `244…` e `00244…` são três chaves.
- **Ficar abaixo do limiar não é travado.** Quatro falhas por janela nunca bloqueiam um ramal: são 384 palpites por dia por ramal, sem escalada. Com a origem rodada e 100 ramais, um acerto esperado em cerca de 26 dias. O ganho é o nome de um colega no censo, sem papel de anfitrião. Fica como trabalho seguinte: um contador de janela longa por ramal, ou um alarme por volume de `ramal.pin_falhado`.
- **O número completo de quem liga fica escrito** — no log do servidor (`tracing`, nas linhas de origem travada), na auditoria (`ramal.pin_falhado`, `ramal.pin_bloqueado`, `ramal.origem_travada`) e na chave de `voice_pin_origins` até a linha ser limpa. É um dado pessoal, guardado para se saber de onde veio um ataque; não há mascaramento nem prazo próprio na auditoria.
- **O Kamailio e o `unset` do Lua não correram numa chamada.** O `kamailio.cfg` passou `kamailio -c` (sintaxe) na imagem 5.8.6; que o `remove_hf_re` tira de facto o cabeçalho de um `INVITE`, e que o `unset` impede o FreeSWITCH de o copiar para a perna B, são pressupostos.
- **A igualdade de tempo não tem portão**: o Argon2 no ramal bloqueado foi lido no código, não medido.
- **A identificação só existe com a ponte.** No recuo para a conferência local do FreeSWITCH não há censo, e o IVR não a oferece.
- Um bilhete não reverifica, ao ser trocado, se a pessoa foi arquivada nos 45 segundos desde que foi emitido.
- `ramal_dial.lua`, o gRPC `IvrService` e a web não mudaram: o gRPC `ValidatePin` não devolve `org_sip_domain`.

**Ficheiros.** `server/migrations/0091_ivr_pin_origem_e_identidade.sql`, `server/crates/delonix-meet-domain/src/telephony/extension_pin.rs`, `server/src/{extension_pin,voice,voice_caller,lib}.rs`, `server/src/phone_bridge/sip.rs`, `server/src/sfu_e2e.rs` (`um_invite_recusado_devolve_o_bilhete_para_invalidar`), `server/tests/{ramal_pin,ramal_pin_origem,ivr_identifica_quem_liga,ramal_entra_na_sala}.rs`, `voice/freeswitch/scripts/dialin_ivr.lua`, `voice/kamailio/kamailio.cfg`, `voice/freeswitch/image/Containerfile`.

### R280 — O bordo não sabia de que organização era a central que lhe ligava

**Sintoma.** O bordo (Kamailio) só filtrava por IP: aceitava quem estivesse no `address_file`, mantido à mão pelo operador, e recusava os outros com `403`. Ligar a central de um inquilino pedia um passo manual do operador; e, entrada a chamada, a sala procurava-se por `(número, PIN)` — a central de uma organização que soubesse o PIN de uma sala de outra entrava nela. O IP não chega para identificar: as centrais do `kind: PbxService` saem de uma célula pelo mesmo endereço. Entretanto a conta SIP da organização («Registo SIP», ADR-0009 §5) guardava-se, mostrava-se sob reautenticação, e **nada se autenticava com ela**.

**Regra** ([ADR-0016](../adr/0016-a-central-de-uma-organizacao-autentica-se-no-bordo.md)).
- Quem não está na allowlist só entra como **central de uma organização**: por TLS, desafiada (`407`, realm = o domínio do `From`) e autenticada por digest com a conta SIP dessa organização. O Kamailio pede o HA1 ao servidor — `POST /internal/v1/telephony/edge/sip-account`, listener interno, segredo da media, resposta em texto — e verifica ele a resposta. Continua sem base de dados.
- Autenticada, a chamada segue com `X-Delonix-Central: <domínio>`. O bordo tira os `X-Delonix-*` a tudo o que vem de fora (`remove_hf_re`), e o IVR só acredita nesse cabeçalho se a chamada veio de um endereço do bordo (`DELONIX_EDGE_CIDRS` → lista `delonix_bordo` do FreeSWITCH). Sem a lista, nenhuma chamada entra como central.
- O IVR valida o PIN em `POST /internal/v1/voice/ivr/validate-central`: a organização sai do domínio autenticado, nunca do pedido, e a sala é a do PIN **dentro dela** — o `room_by_pin_in_org` que o ramal já usava (R273). Sem CDR de dial-in.
- Sem TLS, `403` sem desafio (as chaves SDES viajam no SDP). À décima falha de autenticação em 5 minutos, `403` à origem sem mais perguntas ao servidor. Conta inexistente e password errada levam o mesmo desafio.
- Uma conta SIP sem password não autentica nem abre salas.
- O bordo conta o que deixa entrar e o que recusa (`kamcmd cnt.get script centrais_autenticadas` e `centrais_recusadas`): o Kamailio corre sem `L_INFO` nem `L_NOTICE`, e sem os contadores uma central que entrava não deixava rasto nenhum no bordo.
- **Desligado por omissão**: sem `DELONIX_CONTROL_URL` no Kamailio o bordo comporta-se como antes. O chart liga-o com `voice.centrais.enabled` e recusa-se a renderizar sem `voice.centrais.edgeCidrs`.

**Portão.** `server/tests/central_entra_na_sala.rs`, contra Postgres real (6 casos): o HA1 é o da conta certa e só dela, com o realm tal como veio; conta de outra organização, utilizador noutra caixa, domínio de ninguém e conta sem password → `404`, sem HA1 no corpo; sem o segredo → `401`; PIN da própria organização → `room_bridge` igual ao do dial-in; central de A com o PIN de uma sala de B → `404`, com o controlo positivo da central de B; PIN em duas salas → `404`; o travão de PINs conta por central. `scripts/check-bordo-central.sh` (`make fitness` e CI), estático: o bordo tira os `X-Delonix-*` antes de decidir quem liga, só escreve o cabeçalho depois do digest e por TLS, e o IVR só acredita nele vindo do bordo — quatro controlos negativos corridos (cada regra partida numa cópia → falha).

**Prova corrida a 2026-10-04**, na réplica do bordo (`bash scripts/pbx-tronco-prova.sh central`, Kamailio 5.8.6, FreeSWITCH 1.11.3 da imagem do repo, servidor desta árvore): de endereços **fora da allowlist**, dois telefones com a conta da organização A entram na sala de A **pela ponte do SFU** (ADR-0010) e ouvem-se um ao outro (440 Hz a 0,2499 e 1000 Hz a 0,2500; o próprio tom a 0,0008 e 0,0012), nenhum cai na conferência local, e o PIN não fica no log. Recusas medidas: UDP com a conta certa → `403`; TLS sem credenciais → `407`; password errada e a conta de B no domínio de A → não entram, falha contada; a central de A com o PIN de uma sala de B → o IVR não a deixa entrar (e o mesmo PIN, pela central de B, abre a sala); um `X-Delonix-Central` forjado pela porta do bordo chega ao FreeSWITCH **sem** o cabeçalho, e direito ao FreeSWITCH é rejeitado (`603`); à décima falha a origem leva `403` mesmo com a password certa, e outra origem continua a entrar.

**Por medir, e não o dês por feito.** Um browser na sala (quem ouve a central nesta prova é outro telefone, pela mesma ponte). O chart (`voice.centrais`) só foi lido, nunca instalado; o `check-helm.sh` corre no CI. **O `compose.yaml` liga as centrais** desde a entrega seguinte: o PBX de laboratório ganhou um segundo tronco (`meet-central`, TLS, com a conta SIP da organização), a allowlist do laboratório passou a aceitar só a porta 5060 de origem (senão ninguém do laboratório chegava a ser desafiado), e o `make compose-voice-check` mede-o. Corrido a 2026-10-04: numa cópia isolada no docker (9 de 9; com uma CA errada o tronco fica `Unavail`), depois no laboratório que estava a correr, sob o `delonix compose`, pelo caminho de actualização de um laboratório que já existia (`make bootstrap` acrescentou os dois segredos sem mudar os outros e trocou o certificado do bordo, que não tinha SAN — 9 de 9), e **no cluster local**, pelo passo de voz (`scripts/cluster-voice.sh`, 9 de 9). No cluster o `make cluster` completo parou no passo do Helm por falta de rede, antes da voz; correu-se só esse passo. Restringir uma conta a redes de origem não existe. O caminho `delonix-outbound` (`telephony_fs_xml.rs`) continua a decidir a organização pelo domínio do pedido sem o autenticar.

**Ficheiros.** `voice/kamailio/kamailio.cfg`, `voice/freeswitch/scripts/dialin_ivr.lua`, `voice/cluster/freeswitch-entrypoint.sh`, `server/src/telephony_sip.rs`, `server/src/voice.rs`, `server/src/lib.rs`, `server/tests/central_entra_na_sala.rs`, `scripts/check-bordo-central.sh`, `scripts/pbx-tronco-prova.sh` (modo `central`), `scripts/softphone-prova.sh` (`--dominio`, `--rede`), `voice/pbx-tronco-prova/compose.yaml`, `deploy/helm/delonix-meet/` (`voice.centrais`).

### R281 — Um access token roubado trocava a password, e a sessão de quem roubou sobrevivia à troca do dono

**Sintoma.** `PATCH /api/users/me` aceitava `password` só com o access token: sem a password actual, sem reautenticação recente, e sem tocar nas outras sessões. Quem apanhasse um token trocava a password e ficava com a conta; e quando o dono mudava a password por desconfiar de um roubo, a sessão do ladrão continuava a abrir a API até o refresh token expirar. As rotas que alteram os outros factores (MFA, chaves de acesso) já exigiam `sessions::require_recent`; a password, que é o factor principal, não.

**Regra.**
- Mudar a password pede **prova de identidade**: `current_password` no mesmo pedido (verificada por `auth::check_password_of`, a regra única do login) ou uma reautenticação desta sessão há menos de cinco minutos (`sessions::require_recent`). Sem nenhuma: `403 auth.reauthentication_required`. Prova errada: `401 reauthentication.failed`, com o mesmo travão (cinco em 5 minutos, `reauth:<conta>`) e o mesmo registo (`auth.reauthentication_failed`) da reautenticação — `sessions::failed_proof` é chamado pelas duas.
- A prova corre **antes de qualquer escrita**: um pedido com `username` e `password` em que a prova falha não deixa o username mudado.
- Depois de mudar, **todas as outras sessões da conta terminam** (`sessions::revoke_all_except`, a mesma função do `revoke-others`) e fica `auth.password_changed` na auditoria com quantas terminou. A sessão do pedido continua.
- A conta gerida pelo Odoo continua a recusar primeiro (`409 profile.field_managed_by_odoo`).
- Na consola, o campo «palavra-passe actual» aparece quando se escreve uma nova (`SettingsDialog.tsx`), nas quatro línguas.

**Portão.** `server/tests/account_sessions.rs`: `changing_the_password_needs_proof_and_ends_the_other_sessions` (sem prova `403` e sem escrita parcial; prova errada `401` e a password antiga continua; prova certa `200`, a outra sessão dá `401 auth.session_revoked` e o refresh dela `401`, a própria continua; a antiga deixa de entrar e a nova entra; uma linha de cada evento na auditoria) e `a_recent_reauthentication_is_proof_enough_to_change_the_password`.

**O que NÃO está provado.**
- Nenhum browser abriu o diálogo de definições depois da mudança.
- Um access token **anterior às sessões** (sem `sid`) termina TODAS as sessões ao mudar a password, e ele próprio continua válido até expirar: não tem sessão que se possa terminar.
- O motivo gravado em `user_sessions.revoked_reason` é `user_revoked_others` — o `CHECK` da 0078 não tem um valor próprio para a mudança de password, e acrescentá-lo pedia uma migração. Distingue-se pelo evento de auditoria.
- Não há reposição de password para quem a esqueceu (plano de lacunas, E3).

**Ficheiros.** `server/src/users.rs`, `server/src/sessions.rs`, `server/tests/account_sessions.rs`, `web/src/api.ts`, `web/src/components/SettingsDialog.tsx`, `web/src/locales/*/shell.ts`.

### R282 — O limite por IP usava o valor do `X-Forwarded-For` que o cliente escreveu

**Sintoma.** `rate_limit::client_ip` devolvia o PRIMEIRO valor do `X-Forwarded-For` quando o peer era um proxy privado. Os proxies deste repo acrescentam (`$proxy_add_x_forwarded_for` no compose, no Nginx de VPS e no da web): o que o cliente manda fica à esquerda, o endereço real à direita. Um pedido com `X-Forwarded-For: <qualquer coisa>` escolhia a sua própria chave nos limites de autenticação, de convidado sem conta, de chaves de acesso e de emparelhamento do estúdio — e o IP gravado na sessão e na auditoria era o inventado.

**Regra.** O endereço do cliente é a entrada que o proxy de fora escreveu: a `TRUSTED_PROXY_HOPS`-ésima a contar do **fim** (default 1; `Config::trusted_proxy_hops`, de 1 a 8). Com menos entradas do que saltos usa-se a mais à esquerda que houver; um valor que não é um endereço IP nunca é chave (cai para o IP da ligação); numa ligação directa (peer público) o cabeçalho continua a ser ignorado. O cabeçalho repetido conta como uma lista só.

**Portão.** Unitários em `rate_limit.rs` (`a_forged_xff_does_not_choose_the_rate_limit_key`, `hops_count_from_the_right`, `a_value_that_is_not_an_address_is_never_the_key`, e o `xff_trusted_only_from_proxy` reescrito). Contra servidor real: `server/tests/security.rs::a_forged_x_forwarded_for_does_not_escape_the_auth_rate_limit` — com o limite a 5, o mesmo cliente a inventar um valor por pedido é travado à sexta; clientes diferentes com o mesmo valor forjado não se travam uns aos outros.

**O que NÃO está provado.**
- Nenhum pedido passou por um Nginx ou por um ingress a sério: o teste escreve o cabeçalho como o proxy o entregaria.
- **Um segundo proxy que acrescente** (um balanceador L7 à frente do ingress) com `TRUSTED_PROXY_HOPS=1` faz todos os clientes aparecerem com o endereço do primeiro proxy e partilharem o limite por IP. É falhar para o lado seguro, mas é uma indisponibilidade: o operador tem de subir o valor. Nenhum manifesto do repo monta essa topologia.
- Um balanceador L4 com SNAT esconde o endereço real de todos os clientes; isso não se resolve aqui (pede PROXY protocol).
- Quem já está dentro da rede privada e fala directamente com o servidor escolhe o cabeçalho inteiro.

**Ficheiros.** `server/src/rate_limit.rs`, `server/src/config.rs`, os onze chamadores em `server/src/{auth,guests,passkeys,studio,rate_limit}.rs`, `server/tests/security.rs`, `docs/deployment.md`.

### R283 — A sala de espera não tinha tecto, e um só token enchia a lista de todos os anfitriões

**Sintoma.** `SignalingHub::add_waiting_with` inseria uma entrada por ligação, sem limite e sem olhar a quem era. Um token de sala válido a abrir ligações em ciclo punha milhares de entradas na sala de espera, e cada uma é difundida a todos os anfitriões e devolvida inteira em cada `GET /api/rooms/{code}/waiting`.

**Regra** (`SignalingHub::add_waiting_for`).
- **Uma espera por identidade.** A chave é o `sub` do token de sala (a conta, ou o `guest_id` que cada `guest-join` gera). Uma segunda ligação da mesma identidade substitui a primeira: a entrada antiga sai (`WaitingLeft` para os anfitriões), a nova entra, e a ligação antiga termina como recusada porque o seu `admit_tx` cai sem decisão.
- **Um tecto por sala**, `WAITING_ROOM_MAX = 500` por nó. Cheia, a ligação recebe `Error { "waiting room is full" }` e fecha; não se regista nada e fica um aviso no log.

**Portão.** `signaling.rs`: `one_identity_waits_once_however_many_sockets_it_opens` (mil ligações, uma entrada, fica a mais recente, as outras 999 terminam sem decisão, a que ficou é admitida) e `the_waiting_room_has_a_ceiling` (a 501.ª identidade é recusada; uma que sai liberta o lugar).

**O que NÃO está provado.**
- Só o hub foi exercitado: nenhum WebSocket real abriu mil ligações, e nenhum browser viu a mensagem de sala cheia.
- Dois separadores da MESMA pessoa à espera: o mais antigo mostra «entrada recusada». Fechar em silêncio faria os dois separadores substituírem-se em ciclo ao religar; a recusa é terminal.
- O tecto é por nó. A afinidade por sala (ADR-0001) põe uma sala num só nó; se isso falhar, o tecto multiplica-se pelo número de nós.
- Quinhentas identidades DIFERENTES continuam a caber — o que as trava antes é o limite do `guest-join` por IP e por sala.

**Ficheiros.** `server/src/signaling.rs`.

### R284 — O `JWT_SECRET`, o `TURN_SECRET` e o `PROVISIONING_SECRET` estavam escritos num manifesto de um repositório público

**Sintoma.** `deploy/k8s/01-config.yaml` trazia o Secret `delonix-secrets` com valores literais — `JWT_SECRET`, `TURN_SECRET`, `PROVISIONING_SECRET`, `DATABASE_URL` e `POSTGRES_PASSWORD` — e `deploy/k8s/helm-values/` as passwords do Postgres de stage e de «produção». O `make stage` e o `make prod` aplicavam-nos. A R154 tinha tirado de lá o segredo de voz; os outros ficaram. O servidor só recusava o valor de dev e o comprimento, e os de stage tinham 42 e 21 caracteres: passavam. Com o `JWT_SECRET` publicado assina-se um access token de qualquer conta de qualquer cluster instalado por aquele caminho.

**Regra.**
- **O Secret não vive no repositório.** Sai do `01-config.yaml` (fica só o ConfigMap) e nasce do `.env` da máquina por `scripts/k8s-app-secrets.sh`, a regra única de «o que entra no `delonix-secrets`»: o `make stage`, o `make prod` e o `scripts/cluster.sh` chamam-no (o `cluster.sh` tinha a sua cópia). As passwords do Postgres saem dos ficheiros de valores e passam por `--set` a partir do `.env`.
- **O servidor recusa arrancar** em produção com um valor publicado: `config::BURNED_SECRETS` e `refuse_burned`, aplicados ao `JWT_SECRET`, ao `TURN_SECRET` e ao `PROVISIONING_SECRET`. Com `DELONIX_ALLOW_INSECURE=1` passam. A password de base publicada dá um **aviso** no arranque, não uma recusa (`database_url_uses_burned_password`): rodá-la é um `ALTER USER` com a aplicação parada.
- **Os seis valores entram no livro** (`scripts/leaked-secrets-accepted.txt`), com a razão de cada um, e o portão de higiene recusa que voltem a um ficheiro seguido.

**Portão.** `config.rs` (`mod tests`): o controlo positivo (produção com segredos fortes arranca), uma recusa por cada um dos três valores, desenvolvimento a aceitá-los, e o reconhecimento da password no URL sem confundir o nome do utilizador. `scripts/check-repo-hygiene.sh` verde com os seis valores no livro; `scripts/check-k8s-render.sh` verde (base e os dois overlays). O `k8s-app-secrets.sh` foi corrido contra um `kubectl` que finge o `apply`: sete chaves, `DATABASE_URL` com o host pedido e a password do `.env`, nada de segredos na saída, e recusa com `.env` incompleto.

**O que NÃO está provado.**
- **Nenhum cluster foi instalado.** O `make stage` e o `make prod` não correram; do `make stage` só se viu o `make -n`. O `make prod` continua LEGADO: aponta para um host de base que não é o do chart que instala, e as chaves `auth.*` que recebe são as que o ficheiro de valores já tinha (plano de lacunas, O1).
- O `scripts/cluster.sh` não correu depois da troca da cópia pela chamada ao script.
- A rotação num cluster já instalado (`docs/deployment.md` §6) está escrita, não exercitada.
- O histórico do git continua a ter os valores, de propósito (mesma decisão da R154).
- O Redis destes caminhos continua sem autenticação (plano de lacunas, O7).

**Ficheiros.** `deploy/k8s/01-config.yaml`, `deploy/k8s/kustomization.yaml`, `deploy/k8s/helm-values/postgres-{stage-,}values.yaml`, `scripts/k8s-app-secrets.sh`, `scripts/cluster.sh`, `scripts/bootstrap.sh`, `Makefile`, `server/src/config.rs`, `scripts/leaked-secrets-accepted.txt`, `docs/deployment.md`.

### R285 — Sem `DATA_ENCRYPTION_KEYS` o servidor arrancava em produção, e guardar um segredo falhava em silêncio

**Sintoma.** `Config::from_source` devolvia `secret_box: None` em produção quando a variável faltava. O servidor arrancava, a sonda de saúde ficava verde, e cada escrita de um segredo — o `client_secret` do SSO, o segredo de um webhook, a password de um tronco, a chave de um destino de directo — respondia `422 secrets.encryption_unconfigured`. Só o chart Helm a exigia; o compose, o cluster local, os manifestos de `deploy/k8s`, o Ansible, os do PaaS e o deploy legado não a traziam. A documentação mandava definir `SECRETS_KEY`, que o servidor nunca leu.

**Regra.** Em produção a chave é **obrigatória**: sem ela o arranque falha com a razão e o comando para a gerar. Em desenvolvimento (`DELONIX_ALLOW_INSECURE=1`) continua a derivar-se uma. E todos os caminhos que arrancam o servidor passam a trazê-la:
- `make bootstrap` gera `DATA_ENCRYPTION_KEYS=k1:<base64 de 32 bytes>` no `.env` (acrescenta-a a um `.env` que já exista, sem tocar no resto), e o `make compose-up` recusa um `.env` sem ela;
- `scripts/k8s-app-secrets.sh` põe-na no `delonix-secrets` (stage, prod, cluster local);
- Ansible: `data_encryption_keys` em `group_vars/all.yml`, persistida como os outros segredos, nos três modelos;
- PaaS: `meet-data-encryption-keys` em `deploy/delonix/meet-application.yaml` e no exemplo de segredos;
- `scripts/pbx-tronco-prova.sh` acrescenta-a ao `.env` da réplica, e o `deploy/deploy.sh` legado recusa sem ela.

**Portão.** `config.rs`: `production_without_the_encryption_key_refuses_to_start` e `development_derives_a_key_when_none_is_given`. O bloco do `bootstrap.sh` foi corrido duas vezes sobre um `.env` de ensaio: uma linha só, 32 bytes depois de descodificar.

**O que NÃO está provado.**
- Nenhum dos caminhos arrancou um servidor: nem o compose, nem o cluster, nem o Ansible, nem a réplica do tronco. **O laboratório local tem de correr `make bootstrap` antes do próximo `make compose-up` ou `make cluster`**, senão o servidor não arranca — a mensagem diz porquê.
- O filtro `b64encode` do Ansible sobre 32 caracteres ASCII não foi corrido.
- No PaaS, o `from_secret` continua por injectar pelo expander (aviso 1 do `deploy/delonix/README.md`): a linha nova fica à espera do mesmo que as outras.
- Os testes que esvaziam `secret_box` continuam a provar o `422`, agora como defesa em profundidade.

**Ficheiros.** `server/src/config.rs`, `scripts/bootstrap.sh`, `deploy/compose/env.example`, `scripts/k8s-app-secrets.sh`, `scripts/cluster.sh`, `Makefile`, `deploy/ansible/{group_vars/all.yml,roles/secrets/tasks/main.yml,roles/*/templates/*}`, `deploy/delonix/*`, `scripts/pbx-tronco-prova.sh`, `voice/pbx-tronco-prova/compose.yaml`, `deploy/deploy.sh`, `docs/deployment.md`.

### R286 — O HA1 dos ramais estava em claro na base, e as rotas que o entregam ficavam atrás do ingress

**Sintoma.** Duas coisas, a mesma superfície. (1) `voice_extensions.sip_ha1` guardava o `MD5(utilizador:domínio:password)` de cada ramal em claro. O HA1 é o que o digest SIP usa: quem o tiver regista-se como o ramal sem nunca ter visto a password — uma fuga da tabela era uma fuga das credenciais de todos os ramais. (2) As três rotas de máquina dos ramais — o directório, que devolve esse HA1 ao FreeSWITCH, a resolução de um número marcado e o dialplan por DID — estavam no router PÚBLICO como `/api/voice/ivr/*`, com o comentário «fica no público porque os configs já chamam este caminho». Qualquer ingress que publicasse `/api` publicava o caminho que troca o segredo de voz por credenciais SIP.

**Regra.**
- **O HA1 é cifrado em repouso** (`secrets_at_rest::seal`, aad `voice_extensions.sip_ha1:<id>`), ao criar o ramal e ao regenerar a password. Só à saída para o FreeSWITCH, em `ramais::ivr_directory`, volta a ser o valor do digest. O id do ramal passa a nascer no servidor, para amarrar o valor cifrado à linha. Um HA1 herdado em claro continua a ler-se, e a tarefa de fundo cifra-o (`voice_extensions` é a quarta coluna de `reseal_legacy`; `ResealReport::extension_ha1s`).
- **As três rotas passam para o listener interno**: `/internal/v1/voice/ivr/{directory,resolve-extension,dialplan-did}`, ao lado do resto da API de máquina. Os caminhos antigos deixam de existir. O `xml_curl.conf.xml`, o `ramais_dial.lua`, o arranque do FreeSWITCH (que reescrevia as cópias para apontar ao listener público), o compose, o cluster local, o chart e os scripts de prova mudam no mesmo commit; `DELONIX_API_URL` deixa de existir.

**Portão.** Contra Postgres real: `ramal_entra_na_sala::the_extension_ha1_is_sealed_at_rest_and_still_reaches_freeswitch` (na base está `enc:v1:` e o MD5 não está lá; o directório devolve o HA1 certo; o herdado em claro ainda serve; o cifrado de um ramal copiado para a linha de outro não abre; regenerar grava cifrado) e `…::the_extension_machine_routes_left_the_public_listener` (com `INTERNAL_BIND_ADDR`, o router público dá `404` às três, no caminho novo e no antigo; sem ele, só o novo responde). `secrets_at_rest::legacy_plaintext_keeps_working_and_is_resealed_idempotently` conta e cifra o HA1 herdado. `security_voice_odoo` (o segredo por Basic e nunca no URL) corre nos caminhos novos. `check-lua-sintaxe.sh` e `check-fs-xml.sh` verdes. **Contra um FreeSWITCH 1.11.3 real** (`scripts/softphone-prova.sh srtp-real`, a configuração que o arranque monta com os ficheiros do compose, numa rede sem saída): um ramal autentica-se por digest — o `mod_xml_curl` pediu o directório em `/internal/v1/voice/ivr/directory` com o segredo em Basic —, o `ramais_dial.lua` chegou a `/internal/v1/voice/ivr/resolve-extension` com `X-Voice-Secret`, o IVR do dial-in atendeu, e os controlos negativos mantêm-se (password errada `403`, sem SRTP `488`); dezassete verificações, todas dentro dos limites.

**O que NÃO está provado.**
- **Nenhum ramal se registou contra o servidor a sério.** O que correu foi o FreeSWITCH real com um servidor de andaime (ver o portão): o compose e o cluster não foram levantados, por isso o `make compose-voice-check` e o `scripts/cluster-voice.sh` — que passam a exigir que o directório NÃO responda no listener público — ficam por correr.
- Um FreeSWITCH com a configuração antiga a falar com um servidor novo deixa de registar ramais (pede `/api/voice/ivr/directory`, que já não existe): a configuração e o servidor sobem juntos.
- Sem `DATA_ENCRYPTION_KEYS` não se cria nem se regenera um ramal (`422`) — a R285 torna a chave obrigatória em produção.
- O `sip_password_hash` (Argon2) não mudou; o PIN do ramal também não.

**Ficheiros.** `server/src/ramais.rs`, `server/src/secrets_at_rest.rs`, `server/src/lib.rs`, `voice/freeswitch/autoload_configs/xml_curl.conf.xml`, `voice/freeswitch/scripts/ramais_dial.lua`, `voice/cluster/freeswitch-entrypoint.sh`, `compose.yaml`, `deploy/k8s/cluster/voice.yaml`, `deploy/helm/delonix-meet/templates/voice.yaml`, `voice/pbx-tronco-prova/compose.yaml`, `scripts/{softphone-prova,compose-voice-check,cluster-voice,check-openapi}.sh`, `server/tests/{ramal_entra_na_sala,secrets_at_rest,security_voice_odoo}.rs`.

### R287 — A chave de emissão de cada destino ia no URL do WebSocket do directo

**Sintoma.** `GET /api/rooms/{code}/live` recebia os destinos em JSON na query — e com eles a chave de emissão de cada destino ad hoc (a «stream key» do YouTube, do Facebook, da Twitch). Um URL fica escrito nos logs de acesso do Nginx, do ingress e de qualquer balanceador pelo caminho: a chave de quem emite ficava em texto, em ficheiros que ninguém trata como segredo (RFC-0001, achado B8).

**Regra.**
- **Os destinos vão na primeira trama de texto**, depois do upgrade: `{"tipo":"iniciar","destinos":[…]}`. O URL leva o token de sala e o codec, e mais nada. O servidor espera dez segundos por ela (`iniciar_por_mensagem`); outra coisa na primeira trama é recusada com a forma esperada.
- **No URL continua a caber o que não é segredo:** destinos guardados, só pelo id (`[{"id":…}]`). Um destino com URL e chave na query é recusado com a frase que manda recarregar a página — é o que um cliente antigo em cache recebe.
- A validação é a mesma nos dois caminhos e pela mesma ordem (`preparar_destinos`): malformado, tecto, E2EE, destinos guardados e capacidade do papel, endereços públicos, codec e capacidade do nó.
- Na consola, `urlDoDirecto` deixa de receber destinos e `pedidoDeInicio` monta a trama; o cliente dá a emissão por aceite quando chega o primeiro estado dos destinos (ou ao fim de dois segundos), e nenhum pedaço de media sai antes disso.

**Portão.** `server/tests/broadcast_authz.rs`: `a_chave_de_emissao_nao_se_aceita_no_url` (ad hoc no URL recusado, sem repetir a chave na recusa; o mesmo destino pela primeira trama não leva essa recusa; um id no URL continua a passar a guarda) e `a_primeira_trama_tem_de_ser_o_pedido_de_inicio` (binário, JSON de outro tipo e destinos malformados); os três testes que já lá estavam passam a usar a primeira trama. `web/src/studio/directo.test.ts`: o URL só tem `token` e `codec`; a trama de início sai uma vez, depois de a ligação abrir, e a chave não está no URL; a recusa em resposta ao pedido é a razão que se mostra.

**O que NÃO está provado.**
- **Nenhum browser emitiu.** O `web/e2e/directo-destinos.mjs` (fora do CI: precisa de um servidor RTMP a sério) foi actualizado para a primeira trama e não correu. O `web/e2e/backend-novo-directo.mjs` já estava escrito contra outra linha de backend e não foi tocado.
- **O token de sala continua na query**, como no `/ws`: um WebSocket do browser não manda cabeçalhos. É curto, tem âmbito de uma sala e expira em minutos — mas aparece nos mesmos logs.
- Um cliente antigo em cache recebe a recusa e tem de recarregar; não há negociação de versão.
- Entre o pedido de início e a resposta o servidor consulta a base e resolve nomes: se demorar mais de dois segundos, a consola dá a emissão por aceite e uma recusa tardia aparece como erro da emissão, com a razão do servidor.

**Ficheiros.** `server/src/broadcast.rs`, `server/tests/broadcast_authz.rs`, `web/src/studio/directo.ts`, `web/src/studio/directo.test.ts`, `web/e2e/directo-destinos.mjs`.

### R288 — A palavra-passe de um link de partilha ia no URL, e adivinhava-se sem travão

**Sintoma.** `GET /api/public/recordings/{token}?password=…` e o `…/content?password=…` que o `<video>` e o download usavam: a palavra-passe escolhida por quem partilha — que costuma repetir-se noutros sítios — ficava nos logs de acesso. E não havia limite: adivinhava-se ao ritmo que o Argon2 deixasse.

**Regra.**
- A palavra-passe vai no **corpo** de `POST /api/public/recordings/{token}/access`. Os dois `GET` deixam de a ler: um link com palavra-passe responde `401` ao `GET` dos metadados, com ela no URL ou sem ela.
- A resposta traz o `download_url` já com um **passe de leitura** (`?grant=<expira>.<hmac>`): vale uma hora, só abre aquele link, e deixa de valer quando a palavra-passe do link muda (o hash dela entra no que se assina). O `<video>` não manda cabeçalhos nem corpo, por isso alguma coisa tem de ir no URL — passa a ser uma coisa curta e que não é de ninguém.
- **Cinco palavras-passe erradas em 5 minutos travam o link** (`429`), a certa incluída.
- A rota nova está em `scripts/rotas-publicas.txt`, com a razão.

**Portão.** `server/tests/content.rs`: a palavra-passe CERTA no URL dá `401` nos dois caminhos; no corpo, errada `401` e certa `200`; o `download_url` traz o passe e não a palavra-passe; sem passe, com o MAC trocado, com a validade esticada ou com lixo, `401`; o passe de um link não abre outro; um link sem palavra-passe não ganha passe; cinco erradas e a certa dá `429`. `check-route-auth.sh` verde (273 rotas).

**O que NÃO está provado.**
- Nenhum browser abriu a página de partilha: a `SharePage` passa a usar o `download_url` da resposta, lido e não corrido.
- O passe está num URL e aparece nos logs durante a hora que vale; não é a palavra-passe, mas abre o ficheiro a quem o ler a tempo.
- O travão é por link e vive em memória de cada réplica (`mfa_limiter`): com três réplicas são quinze tentativas, e um reinício zera-o.
- A expiração do passe por tempo não tem teste próprio (só a validade adulterada).

**Ficheiros.** `server/src/recordings.rs`, `server/src/lib.rs`, `scripts/rotas-publicas.txt`, `server/tests/content.rs`, `web/src/api.ts`, `web/src/pages/SharePage.tsx`.

### R289 — O destino de um directo só era validado uma vez, e cada reinício do `ffmpeg` resolvia o nome outra vez

**Sintoma.** `check_tenant_stream_url` corria antes de a emissão arrancar. O supervisor reinicia o `ffmpeg` de um destino até oito vezes ao longo de minutos, e cada reinício resolve o nome por conta do `ffmpeg`, sem ninguém olhar para o resultado. Para apontar uma emissão a um endereço interno bastava um nome que respondesse público à primeira, falhasse a ligação, e passasse a responder `10.x` antes do reinício — sem acertar em janela nenhuma.

**Regra.** O supervisor volta a perguntar à guarda antes de cada reinício (`broadcast::StreamUrlGuard`, a mesma `check_tenant_stream_url` com o DNS de agora). Um destino que deixou de resolver para um endereço público pára de vez, com a razão no estado, e o processo não volta a ser arrancado para ele.

**Portão.** `broadcast.rs`: `um_destino_que_deixa_de_ser_publico_nao_e_reiniciado` (com oito tentativas permitidas, pára à primeira consulta e não arranca outro processo) e `um_destino_que_continua_publico_e_reiniciado` (controlo positivo: uma consulta por reinício, e o destino desiste pelo caminho de sempre).

**O que NÃO está provado, e o que fica aberto.**
- **A janela dentro de um arranque continua aberta.** Entre a resposta da guarda e a resolução que o `ffmpeg` faz a seguir passam milissegundos, e um DNS com TTL zero ainda pode trocar a resposta aí. Fixar o IP no URL parte o RTMPS: o `ffmpeg` não manda SNI para um endereço numérico, e as plataformas que só aceitam RTMPS servem por SNI. **A defesa completa continua a ser de rede** — o processo de emissão não alcançar a rede interna (RFC-0001 §11) — e depende do Channel Engine (plano de lacunas, D3).
- Os testes usam uma guarda de ensaio; nenhum servidor de nomes trocou uma resposta a meio de uma emissão.
- O primeiro arranque não passa por esta guarda: é o `preparar_destinos` que o valida, imediatamente antes.

**Ficheiros.** `server/src/broadcast.rs`, `server/src/net_guard.rs`.

### R290 — Quem abria o link de uma reunião sem conta caía no login

**Sintoma.** O servidor tinha a entrada de convidado sem conta desde a R155 (`POST /api/rooms/{code}/guest-join`), o cliente tinha `guestJoin` com testes, e **nenhum ecrã a chamava**: `App.tsx` mandava quem não tinha sessão para o `Login`, com a sala como destino «depois de entrares». Um convidado externo — o caso em que o Zoom e o Meet ganham — não conseguia entrar. Era o bloqueio nº 1 de adopção, e esteve dado como «feito» porque a rota existia.

**Regra.**
- **Um link de sala sem sessão é a entrada de convidado** (`pages/PortaDeConvidado.tsx`): um nome, «pedir para entrar», e a sala. O login fica a um botão, com a mesma sala como destino. A moderação (`#/lobby/…`) e o telemóvel-câmara continuam a pedir conta.
- **O bilhete do convidado vive num só módulo** (`convidado.ts`): por sala, no `sessionStorage`, com o token de sala, os servidores ICE e o que ele vê da sala. Sobrevive a um F5 e morre com o separador.
- **A sala entra por uma só função** — `entrarNaSala` — que serve os dois: um membro pede `joinRoom` e `iceServers` com a sessão; um convidado usa o bilhete e, se o token expirou (vale cinco minutos), pede outro com o mesmo nome. **Quem tem conta nunca entra como convidado**, mesmo com um bilhete antigo guardado.
- **O próprio nome vem de `participanteLocal()`**, não de `currentUser()`: dezassete sítios da sala escreviam o nome da conta, e um convidado ficava com o retrato sem nome.
- **O convidado não tem o que é da conta:** sem gravar (a gravação guarda-se na biblioteca de quem grava), sem convidar (é pesquisar pessoas da organização), sem presença nem chamadas directas (`PresencaAusente`). O resto da sala é o mesmo dos membros.
- **Sair devolve-o a este ecrã** («saíste da reunião»), e o bilhete e o lugar reservado são esquecidos.
- As recusas dizem o que fazer: sala que só aceita contas → iniciar sessão; código que não existe → não existe; travão → quanto falta.

**Portão.** `web/e2e/convidado-ecra.mjs`, no CI, com dois Chromium e media falsa contra o servidor a sério — 25 verificações: sem sessão o link mostra a entrada de convidado e não o login; o convidado fica na sala de espera; o anfitrião vê o pedido com o nome escrito e admite; vêem-se um ao outro (o retrato remoto é o de QUEM se espera, pelo nome); o convidado não tem o botão de gravar e o anfitrião tem (controlo); nenhum pedido dele leva credenciais; depois de um F5 volta à sala sem nova espera e o anfitrião continua a ver um só; sair mostra «saíste da reunião»; sala fechada a convidados e código inexistente dizem-no. `web/src/convidado.test.ts` (11 casos): o bilhete, a renovação com o token expirado, e a regra de a conta ganhar. `tsc` limpo e `vitest` 1076/1076, com a paridade de chaves das quatro línguas.

**O que NÃO está provado.**
- **A media é a falsa do Chromium**, numa só máquina: nem rede real, nem TURN, nem telemóvel (só a largura de 375 px foi conferida, sem transbordo).
- **Sala com cifra ponta-a-ponta:** o convidado precisa da frase-passe como qualquer membro; o ecrã da frase-passe é o da sala e não foi exercitado sem conta.
- **Mais de cinco minutos parado na pré-entrada:** a renovação do bilhete está nos unitários, com relógio fingido; nenhum browser esperou cinco minutos.
- **O chat anterior à entrada, o relatório de qualidade e as legendas guardadas** pedem sessão: para um convidado esses pedidos dão `401` e são ignorados (já tinham `catch`). Não vê o histórico do chat de antes de entrar.
- **Convite por email com o link** não existe: não há correio (plano de lacunas, E2). O link partilha-se à mão.
- Um instante depois do F5 a lista mostra o lugar reservado da própria pessoa como se fosse outra, até a reclamação do lugar terminar — acontece também a membros, e o teste passou a esperar pelo nome certo em vez de por «dois retratos».

**Ficheiros.** `web/src/convidado.ts`, `web/src/pages/PortaDeConvidado.tsx`, `web/src/pages/auth/EntradaDeConvidado.tsx`, `web/src/App.tsx`, `web/src/components/PresenceProvider.tsx`, `web/src/room/useCallSession.ts`, os ficheiros da sala que liam `currentUser()`, `web/src/room/ControlBar.tsx`, `web/src/room/PeoplePanel.tsx`, `web/src/locales/*/auth.ts`, `web/e2e/convidado-ecra.mjs`, `.github/workflows/ci.yml`.

### R291 — A telefonia de troncos só existia numa pasta de prova: a instalação que corre não registava um tronco nem recebia um CDR

**Sintoma.** O servidor tem troncos, plano de marcação, custo e registos de chamada desde o #136 (ADR-0009), e a consola deixa criar um tronco. O FreeSWITCH que o compose, o cluster e o chart arrancam não sabia de nada disso: o `xml_curl.conf.xml` distribuído só tinha os dois bindings dos ramais, o `mod_json_cdr` não estava carregado, e o perfil `external` não pedia gateways ao servidor. O binding `freeswitch-config`, o `json_cdr.conf.xml` e o perfil com os troncos viviam só em `voice/freeswitch/telefonia-prova/`, montados à mão sobre uma configuração que não está no repo. Um tronco criado na consola nunca se registava em lado nenhum (plano de lacunas, T1).

**Regra.**
- **O binding dos troncos está no `xml_curl.conf.xml` distribuído** (`delonix_telefonia`, `dialplan|directory`), DEPOIS dos dois dos ramais: um «not found» passa a pergunta ao seguinte, e este responde «not found» a tudo o que não seja seu.
- **Os troncos são gateways do perfil `external`** — o que o bordo já usa e o que o servidor nomeia por omissão. O arranque TROCA o `<domain name="all">` da vanilla por `<domain name="delonix-trunks">`. Trocar, não acrescentar: o `all` também chega ao servidor, e com os dois a lista era lida duas vezes (medido: duas linhas «Ignoring duplicate gateway» por tronco e por ciclo). O comentário do `telephony_fs_xml.rs` que dizia que o `all` só lia o directório estático estava errado e foi corrigido.
- **O `mod_json_cdr` carrega no arranque** e entrega os registos em `/internal/v1/telephony/call-records`, com o segredo em Basic. Nada em disco enquanto o servidor responde; o que ele não aceitar fica em `cdr-pendentes`, um directório `700` FORA de `/conf` (que é esvaziado a cada arranque).
- **Só a perna de um tronco deixa registo de chamada.** Um registo do `mod_json_cdr` leva TODAS as variáveis do canal, sem filtro — o SDP com as linhas `a=crypto` e as chaves SRTP da perna incluídas. Na primeira versão desta entrada o FreeSWITCH entregava o registo de todas as chamadas e o servidor escolhia: **medido, 2 de 4 registos de chamadas de ramal e de dial-in levavam chaves SRTP** (achado da segunda revisão de segurança; contraria o ADR-0010, «as chaves nunca aparecem em JSON»). Agora: `log-b-leg` desligado, e uma perna B só é registada se o plano de marcação o pedir — o servidor pede-o (`force_process_cdr=true`) só na perna que sai por um tronco; e os contextos `public` e `delonix_ramais` desligam o registo da perna de quem liga (`process_cdr=false`).
- **O que sobra, o servidor aceita e ignora.** Uma perna A que nunca chegou a um plano de marcação (uma chamada recusada à entrada) ou que nasceu dentro do FreeSWITCH não tem organização nem tronco: `204`. Recusá-la (`422`, como antes) fazia o módulo tentar outra vez e guardá-la em disco. Com QUALQUER gateway no registo — nosso (`dlx-<id>`) ou não — e sem organização continua a ser recusada: é uma saída para a rede pública que não se consegue atribuir.
- **Uma organização tem no máximo 20 troncos** (`MAX_TRUNKS_PER_ORG`), ao criar e ao servir. Os troncos de TODAS as organizações vão ao FreeSWITCH num só documento, e o `mod_xml_curl` deita fora a resposta INTEIRA acima do seu limite (1 MiB por omissão; o binding passa a dizer 16 MiB): sem tecto, uma organização deixava as outras sem troncos no arranque seguinte.
- **Os troncos voltam a ler-se sozinhos**: um ciclo no arranque manda `sofia profile external rescan` de 60 em 60 s (`DELONIX_TRUNKS_RESCAN_SECS` no compose, `voice.freeswitch.trunksRescanSecs` no chart; `0` desliga). O FreeSWITCH só pergunta pelos gateways quando o perfil arranca — se o servidor não respondia nesse instante ficava sem tronco nenhum, e um tronco novo só aparecia reiniciando-o.
- **Texto de um inquilino nunca chega ao FreeSWITCH com um `$`.** O FreeSWITCH passa a resposta do `mod_xml_curl` pelo PRÉ-PROCESSADOR antes de a ler, e esse troca `$${nome}` pelo valor da variável global `nome` — o segredo de voz é uma (`delonix_voice_secret`). O utilizador e a password de um tronco são texto que o administrador de QUALQUER organização escreve. Ligar este caminho sem mais abria isto: um tronco com o utilizador `$${delonix_voice_secret}` punha o FreeSWITCH a registar-se no servidor SIP do atacante com o segredo da plataforma no `From` — e esse segredo abre todo o `/internal/v1/*`. **Foi a revisão de segurança que o encontrou, antes de fundir; reproduzi-o na réplica com o servidor a sério** (o gateway ficou com o segredo como utilizador, e a operadora de ensaio recebeu-o em dois pedidos). `telephony_fs_xml::esc` e `ramais::xml_escape` passam a emitir o `$` como `&#36;`: o pré-processador não o vê, e o leitor de XML devolve-o como o `$` que era. O `xml_escape` dos ramais (etiquetas e nomes, texto livre) tinha a mesma falha e já estava em uso.

**Portão.** `bash scripts/troncos-prova.sh` — fora do CI (precisa de uma imagem do servidor da árvore), 31 verificações numa réplica com o servidor a sério, o FreeSWITCH arrancado pelo `freeswitch-entrypoint.sh` com os ficheiros que o `compose.yaml` monta, e uma operadora de ensaio. Medido a 2026-10-05, host a carga 4 a 8:
- um tronco criado PELA API aparece e fica `REGED` na operadora sem reiniciar nada, e a operadora tem o registo;
- uma chamada pelo plano de marcação sai por ele, e o registo chega: atendida, de saída, 5 s facturáveis, **9,40 AOA** (o preço do tronco, ao minuto), **MOS 4,50**, jitter e perda medidos; a API mostra-o ao administrador;
- ocupado fica `busy` sem custo; o 112 sai, fica marcado como emergência e não é gravado;
- um número sem regra não sai e não deixa registo;
- uma chamada ao IVR do dial-in não deixa registo nem ficheiro em disco — **controlo negativo: com um servidor sem a regra do `204` ficam dois ficheiros, e a prova falha**;
- reiniciar o FreeSWITCH: o tronco volta; reiniciá-lo com o servidor EM BAIXO: arranca sem tronco (controlo), e o tronco regista-se sozinho quando o servidor volta;
- um tronco com o utilizador `$${delonix_voice_secret}` chega ao FreeSWITCH como texto, e a operadora não recebe o segredo em pedido nenhum — **controlo negativo: com o servidor sem o `&#36;` as duas verificações falham**;
- sem servidor o registo de uma chamada fica em disco (controlo do «nada por entregar»); com ele, nenhum; quatro chamadas por tronco, quatro registos — a perna do tronco continua a chegar com o `log-b-leg` desligado; o que fica em disco não leva o segredo de voz, a password do tronco nem chaves SRTP;
- nem o segredo de voz nem a password do tronco aparecem no `freeswitch.log` nem no directório de logs.

No CI: `tests/telephony.rs` (o `204` e que nada fica guardado; o `422` com um gateway que não é nosso; a resposta dos gateways sem um único `$`; o tecto de troncos ao criar e ao servir), os unitários de `esc` e `xml_escape`, `check-fs-xml.sh`, `check-helm.sh` (o que o arranque copia de `/meet` tem de estar no ConfigMap do chart — um ficheiro em falta era um pod em CrashLoop, visto só no cluster), e — no workflow da imagem do FreeSWITCH — `softphone-prova.sh srtp-real` e `srtp-cluster`, que passaram a medir que **nenhuma chamada atendida de ramal ou de dial-in deixa registo**, e que o que ainda chega (a chamada recusada antes do plano de marcação — é por ela que se sabe que o módulo entrega) vem com o segredo em Basic, **sem chave SRTP e sem o PIN marcado**. As duas correram em local com a configuração nova: os ramais e o dial-in continuam a recusar chamadas sem SRTP.

**Revisão.** Três revisões independentes antes de fundir, e foram elas que encontraram o que as minhas verificações não viam. A de deploy não bloqueou; os dois ajustes (a prova do PIN e o portão do chart) entraram. A primeira de segurança encontrou o `$` (bloqueava) e não entregou o resto; a segunda respondeu ao que faltava, por leitura do código e do fonte do FreeSWITCH: as chaves SRTP nos registos (corrigido e medido), o tecto de troncos (corrigido, com teste), e o `204` para um gateway alheio (corrigido, com teste). Três achados dela eram sobre uma verificação de DNS que eu tinha acrescentado ao servir os gateways — punha o DNS de uma organização a consumir um recurso de todas e não cumpria o que prometia; **foi retirada**, e o que ela tentava fechar está abaixo como aberto. Confirmado por ela, por leitura: uma perna de tronco nunca cai no `204` (o sofia põe sempre o nome do gateway), e nem o segredo de voz, nem a password do tronco, nem o HA1 são variáveis de canal.

**O que NÃO está provado.**
- **Ninguém marca.** A chamada da prova nasce dentro do FreeSWITCH (`originate loopback/…/delonix-outbound`), já com a organização. Na configuração distribuída nenhum perfil leva um ramal ou uma central ao contexto `delonix-outbound`: um cliente ainda NÃO consegue fazer uma chamada para a rede pública (T2). O que ficou ligado é o caminho de saída e o registo, não a porta de entrada.
- **Um tronco ALTERADO ou APAGADO não se actualiza.** O `rescan` só acrescenta. Mudar a password ou desactivar um tronco precisa de `killgw`, que o servidor manda pelo ESL — fechado em loopback nesta configuração (T11). Até lá é reiniciar o FreeSWITCH: **um passo manual no caminho do cliente, por fechar.** Pela mesma razão a consola não mostra o estado do registo, e a «chamada de teste» não funciona.
- **O cluster e o chart.** A lista de ficheiros do cluster (`cluster-voice.sh`) passou no `srtp-cluster`, mas nenhuma chamada por tronco correu num cluster, e o chart não foi renderizado em local (não há `helm` nesta máquina — fica para o CI).
- **Uma operadora de verdade**: TLS, SRTP, NAT de media, DTMF, identidade do chamador, e uma chamada que ENTRA por um tronco registado (T3 a T6). A operadora de ensaio é o FreeSWITCH vanilla na mesma rede.
- **O custo do ciclo.** Cada `rescan` escreve uma linha de aviso por tronco que já existe (com cem troncos, cem linhas por minuto), volta a aplicar as definições ao perfil vivo e zera os contadores de chamadas dele (lido no `sofia.c` pelo revisor; a memória ao fim de dias não foi medida). Abrir o ESL ao servidor (T11) deixa o ciclo servir só para o arranque.
- **Os registos pendentes não se reenviam sozinhos.** Sobrevivem a reiniciar o contentor no compose; num pod, um reinício pelo kubelet é um contentor novo e leva-os — não há volume para eles. É um ficheiro por registo, à espera de alguém.
- **O host de um tronco só é verificado quando se grava** (R213) — e um nome que então não resolve é aceite. Agora o FreeSWITCH liga-se mesmo a ele, e resolve-o por conta própria a cada registo: um nome que passe a apontar para dentro leva pedidos SIP do FreeSWITCH a um endereço interno. Verificar outra vez ao servir não o fecha (um gateway já carregado nunca volta a ser verificado, e o DNS pode responder uma coisa ao servidor e outra ao FreeSWITCH). A defesa a sério é a rede do FreeSWITCH não alcançar a rede interna — política de rede do pod, que não existe hoje. **É um risco que este PR torna real e deixa aberto, por decidir com o bordo (T9).**
- **A perna de um tronco COM SRTP leva as chaves dessa chamada no seu registo.** Vão em HTTP para o listener interno do servidor, que não as guarda; se o servidor não responder ficam no ficheiro pendente. O `mod_json_cdr` não sabe filtrar variáveis.
- **Uma chamada que ENTRA por um tronco não deixa registo**: cai no contexto `public`, que desliga o registo da perna (tem o PIN e as chaves). As chamadas recebidas ficam por contar (T12).
- **`${nome}` com um só cifrão.** O `&#36;` fecha o pré-processador; a expansão em execução (`${nome}` no `data` de uma acção ou numa dial string, com recuo para as variáveis globais) é outro mecanismo. Hoje só lá entram dígitos, identificadores gerados e o domínio SIP. A regra fica escrita: texto livre de um inquilino nunca vai para o `data` de uma acção nem para uma dial string.
- **O registo de contas sem verificação de email** agrava tudo o que é «por organização»: quem se regista é administrador da sua e cria troncos.
- **A ordem de instalação.** A configuração nova entrega todos os registos; um servidor ANTERIOR a esta entrada responde `422` aos que não são de tronco, e ficam dois ficheiros em disco por chamada. O servidor actualiza-se primeiro, ou os dois juntos. E o contentor do FreeSWITCH tem de ser RECRIADO (`up -d`), não reiniciado: o arranque novo copia um ficheiro que a montagem antiga não tem.
- **Mais de um FreeSWITCH**: cada réplica registaria o mesmo tronco na operadora (T11).
- **O ADR-0009 continua «Proposto»**: esta entrada liga o que ele descreve, não o aceita.
- A custo: 5 s de chamada custam o minuto inteiro (9,40 AOA). É a regra que já lá estava (`cost.rs`), não foi mexida; o incremento configurável é o T12.

**Ficheiros.** `voice/freeswitch/dialplan/public/00_delonix_dialin.xml`, `voice/freeswitch/dialplan/default/00_delonix_extensions.xml`, `server/crates/delonix-meet-domain/src/telephony/trunk.rs`, `docs/reference/openapi/bff.json`, `server/src/ramais.rs`, `server/src/telephony_trunks.rs`, `scripts/check-helm.sh`, `deploy/helm/delonix-meet/values.yaml`, `voice/cluster/freeswitch-entrypoint.sh`, `voice/freeswitch/autoload_configs/xml_curl.conf.xml`, `voice/freeswitch/autoload_configs/json_cdr.conf.xml`, `compose.yaml`, `scripts/cluster-voice.sh`, `deploy/helm/delonix-meet/templates/voice.yaml`, `deploy/helm/delonix-meet/files/voice/freeswitch/json_cdr.conf.xml`, `voice/pbx-tronco-prova/compose.yaml`, `server/src/telephony_cdr.rs`, `server/src/telephony_fs_xml.rs`, `server/tests/telephony.rs`, `scripts/troncos-prova.sh`, `voice/troncos-prova/`, `scripts/softphone-prova.sh`.

### R292 — Um ramal não conseguia ligar para fora: nem para a rede pública, nem para o 112

**Sintoma.** Com os troncos ligados (R291) uma chamada já saía pelo plano de marcação — mas só nascendo dentro do FreeSWITCH. Um ramal não chegava lá: o contexto `delonix_ramais` só aceitava números de 3 a 5 dígitos e entregava-os a `ramais_dial.lua`, que só conhecia ramais e o número de acesso às reuniões. Um número de nove dígitos nem batia no padrão; **o 112 batia, era procurado como ramal, não existia, e a chamada desligava**. O cliente que comprava telefonia não ligava para ninguém de fora (plano de lacunas, T2).

**Regra.**
- **O servidor decide, por esta ordem** (`ramais::ivr_resolve_extension`): o número de acesso às reuniões; a **emergência** — sai sempre, antes de se procurar um ramal, e basta o ramal estar activo (nem a pessoa arquivada fica sem o 112); outro ramal da organização; e por fim o plano de marcação. Só sai o que uma regra de SAÍDA do plano manda por um tronco: sem regra, bloqueado ou uma regra interna respondem «não existe» — o plano de marcação é uma lista do que se pode marcar.
- **A organização que paga é a do ramal AUTENTICADO.** O Lua manda o que o digest autenticou (`sip_auth_username`, `sip_auth_realm`) e o servidor confere-o (`voice::authenticated_extension`, a mesma regra do R273, agora num só sítio): utilizador activo, e o realm é o domínio da organização dele. Nunca o `From`, nem o `domain` do pedido. A resposta leva a organização; o Lua põe-na no canal e transfere para `delonix-outbound`.
- **À operadora apresenta-se o número curto do ramal**, não o utilizador SIP — que é metade da credencial dele, e era o que o directório punha como identificador.
- **Um número de emergência nunca é de um ramal** (`409 ramais.extension_reserved`), nem a numeração automática o atribui.
- **A perna do tronco volta a ligar o seu próprio registo.** A perna do ramal tem `process_cdr=false` (R291), e o FreeSWITCH copia esse valor para a perna que ela origina DEPOIS de aplicar as variáveis da dial string (`switch_core_session.c`): a chamada saía pelo tronco e **não deixava registo — não se cobrava**. A dial string do tronco leva `execute_on_originate=set process_cdr=true`. Foi a prova com um telefone a sério que o mostrou; a da R291 marcava de dentro do FreeSWITCH e não o via.
- **O DID de um ramal só responde a uma chamada que entra.** O FreeSWITCH pergunta pelo plano em todos os contextos, e a resposta do `dialplan-did` só tem o `public`: dada a uma chamada de ramal ficava sem contexto e sem rota. Com o padrão alargado a 15 dígitos, um ramal que marcasse o DID de outro batia aí.
- **Um ramal não transfere, e a operadora não redirecciona.** Os dois perfis recusam `REFER` (`disable-transfer`): aceite, uma transferência cega pedida pelo ramal punha a perna do TRONCO a passar outra vez pelo plano de marcação — saía uma segunda chamada para onde o ramal mandasse, por conta da organização, e a primeira ficava marcada para o servidor ignorar. E **nenhuma perna que o FreeSWITCH origina segue um 3xx** (`outbound_redirect_fatal`, variável global posta no arranque, e também na dial string do tronco): segui-lo era ligar ao Contact que o outro lado escolhe, sem passar pela guarda de saída (R213). Vale para a perna do tronco, para a chamada de teste pelo ESL e para a que toca num ramal — um telefone que responda `302` (reencaminhamento configurado no aparelho) deixa de ser seguido.
- **O registo da perna do tronco não leva a chave do ramal.** O FreeSWITCH copia para ela o SDP que o ramal ofereceu, com as linhas `a=crypto` (`switch_m_sdp`) — ao originar, **e outra vez sempre que o ramal manda um SDP novo a meio da chamada** (`sofia_glue_pass_sdp`, sem condição: pôr em espera e retomar chega) —, e o registo leva todas as variáveis. A dial string tira-a duas vezes, na própria perna: quando nasce (`execute_on_originate_2=unset switch_m_sdp`) e quando a ponte acaba (`execute_on_post_bridge=unset switch_m_sdp`). **O que não serve, medido:** um `api_hangup_hook=uuid_setvar ${uuid} …` — o texto do plano é expandido na perna de quem marca (duas vezes, com o `limit_execute`) e `${uuid}` sai com o identificador DELA; e com o identificador certo (`origination_uuid` escolhido pelo servidor) o `uuid_setvar` não encontra uma sessão que já desligou (`switch_core_session_perform_read_lock`).
- **A ingestão nunca ignora um registo que saiu por um gateway.** A marca de «ignorar» (`delonix_cdr_skip`) é uma variável de canal, e uma perna de tronco que volte a passar pelo plano fica com ela.
- **A organização que paga nunca sai de um dado do pedido.** O handler do plano de saída tinha um recuo para o host do Request-URI (`sip_req_host`); sem a variável que nós pomos no canal, agora não há rota.
- **Do telefone para a operadora não passam cabeçalhos** `X-…`/`P-…` (`sip_copy_custom_headers=false`) nem as partes de um INVITE multipart (`sip_copy_multipart=false`), o pedido do Lua ao servidor tem tempo-limite (3 s para ligar, 6 s no total) e a limpeza do corpo tira também o `%` (o `mod_curl` descodifica `%XX` depois dela), e o dialplan aceita números de emergência de dois dígitos.

**Portão.** `bash scripts/troncos-prova.sh`, passos 6 e 8 — fora do CI; **45 verificações, 0 falhas**, medidas a 2026-10-05 no **delonix 4.5.0** (as provas deixaram de precisar do docker: `scripts/motor.sh`), num host partilhado com a carga entre 7 e 30 nas várias corridas, em cerca de cinco minutos. Um softphone (baresip) autentica-se por digest no perfil dos ramais com um ramal criado pela API, com SRTP:
- liga para um número de ensaio: atendida; **o ramal ouve os 440 Hz da operadora e a operadora grava os 1000 Hz do ramal (amplitude 0,25)** — áudio nos dois sentidos; o registo chega atendido, de «1001», com custo e MOS; a operadora nunca vê o utilizador SIP do ramal;
- liga para o 112: sai, marcada como emergência, não gravada;
- **pede uma transferência cega a meio de uma chamada: recusada** — nenhuma chamada sai para o número pedido, e a chamada em que foi pedida deixa o seu registo;
- **controlos negativos:** um número sem regra leva `404`; a password errada leva `403`; o ramal de OUTRA organização, sem plano, não sai pelos troncos desta; nenhuma das três deixa registo;
- **põe a chamada em espera e retoma-a, e o servidor morre a meio:** a prova confere primeiro que a renegociação chegou — a perna do tronco voltou a ter o SDP do ramal, com a chave —, e depois que o registo dessa perna, que fica em disco (directório `700`), **não leva a chave**;
- sete chamadas por tronco entregues, sete registos.

**Os controlos das regras que duas revisões de segurança independentes encontraram**, todos com o código de antes de cada correcção:
- a transferência e a chave copiada na origem: 40 ✓ e 4 ✗ — a transferência fazia sair uma chamada e deixava a primeira sem registo, e o registo em disco levava `a=crypto … inline:` (medido no docker);
- a chave copiada outra vez a meio da chamada: 43 ✓ e 2 ✗ com espera e retoma (medido no delonix) — e **as duas primeiras correcções, com o gancho de fim de perna, deram os mesmos 43 ✓ e 2 ✗**; foi o registo em disco que mostrou porquê.

No CI: `tests/telephony.rs` — a decisão do servidor caso a caso (sai com a identidade certa e a organização do ramal; não sai sem identidade, com o realm de outra organização, com o `domain` de outra, sem regra, bloqueado, por regra interna, ou com a pessoa arquivada; o 112 sai nesses casos em que o ramal está activo; `112` não se cria como ramal), o DID só no contexto `public`, o plano de saída sem rota quando só vem o domínio SIP do pedido, e um registo com gateway guardado mesmo marcado para ignorar. `ramal_entra_na_sala`, `ramal_pin` e `security_voice_odoo` continuam verdes com a regra do ramal autenticado extraída. `softphone-prova.sh selftest`, `srtp-real` e `srtp-cluster` passam com o dialplan e os perfis novos — 15, 20 e 20 verificações, no delonix; no docker é o CI que os corre (workflow «Imagem FreeSWITCH»).

**O que NÃO está provado.**
- **Antifraude não existe** (T8): não há tecto de gasto, nem tranca de internacional, nem alarme. Um ramal com a password roubada liga para tudo o que o plano de marcação deixar, até ao limite de canais do tronco. O plano é a única barreira — e é o administrador que o escreve.
- **A identidade apresentada à operadora é o número curto do ramal** («1001»), não um número que a organização possua, e não há `P-Asserted-Identity` nem pedido de privacidade (T6). Uma operadora a sério rejeita-o ou troca-o.
- **A emergência não leva localização**, e nada foi conferido com o que o regulador exige de uma chamada para o 112 a partir de um softphone.
- **Uma central (ADR-0016) não marca para fora**: só o ramal chega ao plano de marcação.
- **Ramal para ramal e o número de acesso às reuniões** não foram medidos aqui com dois telefones; o caminho deles no Lua não mudou, e o `srtp-real` só o leva até ao `404`.
- **Um ramal deixou de poder transferir**, de todo: nem cega nem assistida. É a regra, não um resto — volta quando a transferência for desenhada (quem paga a perna transferida, e com que registo).
- **Um `REFER` ou um 3xx vindos da OPERADORA, e um 3xx de um ramal** não foram medidos: a operadora de ensaio não os manda. A recusa (`disable-transfer` no perfil `external`, `outbound_redirect_fatal`) está lida no fonte do FreeSWITCH (`sofia.c`, e o recuo de uma variável de canal para as globais em `switch_channel.c`), não exercitada.
- **Uma renegociação do ramal ANTES de a chamada ser atendida, seguida de cancelamento**: a perna do tronco nunca chega a ter ponte, e a cópia do SDP que essa renegociação lá ponha não é tirada por nenhum dos dois `unset`. Por leitura; nenhum softphone o fez aqui.
- **Um INVITE com `Replaces`** não é coberto pelo `disable-transfer`, e a perna nova não passa pelo contexto dos ramais (sem `process_cdr=false`, sem Lua). Por leitura (`sofia.c`); a perna do tronco não volta ao plano nem perde o registo, mas o caminho não foi exercitado.
- **O 112 depende do servidor.** O Lua pergunta-lhe o que é o número, e o plano de saída volta a perguntar: com o servidor em baixo, ou a demorar mais de 6 s, a chamada de emergência não sai. Não há caminho estático.
- **Os cabeçalhos e o corpo multipart do telefone** (`sip_copy_custom_headers`, `sip_copy_multipart`) e o tempo-limite do Lua estão no código e não têm uma chamada que os meça.
- **`web/e2e/telefonia-freeswitch.mjs`** (fora do CI, contra `voice/freeswitch/telefonia-prova/`) entrava pelo perfil do PBX e dependia do recuo para o domínio do pedido. Passou a originar por `loopback` com `delonix_org_id`, como esta prova — e **essa alteração não foi corrida**.
- **Os segredos da réplica na linha de comandos.** As provas põem o ambiente dos contentores em ficheiros, mas ainda passam por argumento a password do administrador, a do tronco e o segredo de voz ao `python3` e ao `grep` que as conferem. São gerados para a réplica e morrem com ela.
- O que a R291 já deixava aberto continua: um tronco alterado só se actualiza reiniciando o FreeSWITCH, o host do tronco só é verificado ao gravar, e nada correu num cluster.
- **As provas no docker.** A `troncos-prova.sh` foi reescrita sem compose e só correu no delonix; o caminho do docker dela não foi exercitado depois disso (não corre no CI). O da `softphone-prova.sh` é o CI que o corre.

**Ficheiros.** `server/src/ramais.rs`, `server/src/voice.rs`, `server/src/telephony_fs_xml.rs`, `server/src/telephony_cdr.rs`, `server/tests/telephony.rs`, `voice/freeswitch/scripts/ramais_dial.lua`, `voice/freeswitch/dialplan/default/00_delonix_extensions.xml`, `voice/freeswitch/sip_profiles/internal.xml`, `voice/cluster/freeswitch-entrypoint.sh`, `scripts/motor.sh`, `scripts/troncos-prova.sh`, `scripts/softphone-prova.sh`, `voice/troncos-prova/operadora-dialplan.xml`, `web/e2e/telefonia-freeswitch.mjs`, `voice/freeswitch/telefonia-prova/README.md`.

### R293 — Nunca um browser tinha ouvido um telefone da ponte, e a réplica de prova tinha deixado de arrancar

**Sintoma.** Duas coisas, encontradas ao medir a mesma. (1) Tudo o que estava provado sobre a ponte telefone↔sala (ADR-0010) media a media de um telefone contra um subscritor `webrtc-rs` dentro de um teste (R221, R222) ou contra outro telefone (R280). Um browser — o cliente real — nunca tinha estado na sala a ouvir um telefone; todos os relatórios o diziam como «não validado». (2) O `voice/pbx-tronco-prova/compose.yaml` tinha a chave `DATA_ENCRYPTION_KEYS` duas vezes no ambiente do servidor: duas entregas (ADR-0016 e R286) acrescentaram-na cada uma na sua linha, o git fundiu sem conflito, e o `docker compose` passou a recusar o ficheiro («mapping key already defined»). A réplica deixou de arrancar na `develop` e ninguém soube: só corre à mão.

**Regra.**
- O modo `browser` do `scripts/pbx-tronco-prova.sh` põe um Chromium na sala com a pilha real do cliente (`web/e2e/harness.html`: `SfuCall` e `Signaling`) a tocar 440 Hz, e a central — um softphone, autenticado no bordo com a conta SIP da organização — a tocar 1000 Hz. O softphone mede os 440 Hz no que ouve; o browser mede os 1000 Hz no que descodifica (`web/e2e/telefone-na-sala.mjs`).
- **Mede-se nas amostras, não nas estatísticas.** O `audioLevel` e o `totalAudioEnergy` do `inbound-rtp` vêm a zero neste Chromium, sem saída de áudio real, com o tom presente (medido: energia 0 com 1002 Hz a −26 dB). A medição é o espectro (Web Audio) da faixa remota.
- **Uma fonte de Web Audio por faixa.** Uma fonte feita do fluxo inteiro só lê uma das faixas de áudio, e o cliente tem sempre uma faixa remota muda: quando calhava ser essa, lia-se silêncio com os pacotes a chegar — uma corrida em cada três falhava, e parecia um defeito do produto.
- O arnês aceita `som=cru` (sem cancelamento de eco, supressão de ruído nem ganho automático): quem mede um tom precisa de que ele chegue, e a supressão de ruído trata um tom constante como ruído. O `vite.config.ts` aceita `API_HOST`, para o servidor estar noutro endereço que não a máquina.
- A chave repetida saiu do compose da réplica.

**Portão.** `scripts/check-replicas-compose.sh` (`make fitness` e CI): o próprio `docker compose` lê cada `voice/*/compose.yaml` (`config -q`), com valores de enchimento nas variáveis que eles exigem; não sobe nada. Controlo negativo corrido: com o ficheiro como estava na `develop`, falha com a chave repetida. A prova com o browser fica **fora do CI** (`scripts/e2e-fora-do-ci.txt`).

**Prova corrida a 2026-10-05**, cinco vezes seguidas, na réplica (Kamailio 5.8.6, FreeSWITCH 1.11.3, o servidor desta árvore com a ponte ligada): a central entra na sala pela ponte, sem recuo; a central ouve os 440 Hz do browser (amplitude 0,503) e não o seu próprio tom (0,001); o browser recebe ≈ 602 pacotes em 12 s, sem perdas, e o pico do espectro do que descodifica está nos 1002 Hz, a −26 dB, em 12 de 12 leituras, 82 dB ou mais acima dos 440 Hz. **Controlo negativo:** com a central a tocar 700 Hz (`PBX_PROVA_TOM_CENTRAL=700`) o browser deixa de encontrar os 1000 Hz e a prova falha, com o outro sentido a continuar verde.

**Por medir, e não o dês por feito.** A qualidade do áudio (só a presença do tom); mais do que um browser na sala; vídeo; a mesma medição no `compose.yaml`, no cluster ou com um Chrome a sério em vez do Chromium de testes; um telefone a entrar por dial-in de operadora ou por ramal com um browser na sala (o caminho da ponte é o mesmo, mas quem aqui entrou foi a central).

**Ficheiros.** `web/e2e/telefone-na-sala.mjs`, `web/e2e/harness.ts` (`som=cru`), `web/vite.config.ts` (`API_HOST`), `scripts/pbx-tronco-prova.sh` (modo `browser`), `voice/pbx-tronco-prova/compose.yaml`, `scripts/check-replicas-compose.sh`, `Makefile` e `.github/workflows/ci.yml` (o portão), `scripts/e2e-fora-do-ci.txt`.

### R294 — Um pacote de áudio atrasado somava 24 h 51 min à pista gravada, e em debug matava a thread de escrita

**Sintoma.** O gravador entrega os pacotes RTP Opus ao `OggWriter` do `webrtc-media 0.17.2`, que avança a posição do grânulo com `timestamp - anterior` — uma subtracção de `u32` sem `wrapping` (`ogg_writer/mod.rs:181`). A bomba do SFU escreve pela ordem de chegada. Medido a 2026-10-05, com três pacotes de 20 ms e os timestamps 0, 1920, 960 (o terceiro atrasado pela rede):
- **em release** (o perfil do CI e da imagem): os grânulos ficam `1, 1921, 4294968257`, e o `ffprobe` dá à pista de 60 ms **89 478,5 s**. A pista não recupera — todos os pacotes seguintes ficam 2^32 amostras à frente. Um pacote repetido escrevia-se duas vezes;
- **em debug**: `attempt to subtract with overflow`, a thread de escrita morre, o OGG fica sem página de fecho e o resto da pista não se grava. Sem erro para quem chama: o `try_send` passa a falhar e conta como «disco lento»;
- **a volta legítima do relógio de 32 bits** (os browsers começam o timestamp num valor ao acaso; a 48 kHz dá a volta a cada 24 h 51 min) dá o resultado certo em release, por acaso da aritmética, e é o mesmo pânico em debug.

**O que isto NÃO fazia, medido.** O ficheiro composto não saía com 24 h. Com áudio real (10 s de tom, um pacote por página, os grânulos que o `OggWriter` produz) e o ffmpeg 6.1.1, os três caminhos da composição — remux `-c copy`, só áudio com `adelay`, e `adelay`+`amix` de duas pistas — dão 10,04 s: o ffmpeg vê um salto de mais de 10 s num OGG e desconta-o (`timestamp discontinuity … new offset= -89478465333`). O que ficava era o frame atrasado fora do sítio: no remux, dois pacotes com o mesmo instante e nenhum 20 ms antes. O revisor mediu o mesmo salto por conta própria, com outro ficheiro (6 s), no remux com vídeo e no `adelay`+`amix`: 6,0 s nos dois. A gravação dependia de uma tolerância do ffmpeg que ninguém tinha pedido.

**Regra.**
- **O que não está à frente do último escrito não se escreve.** `recorder::OpusClock` guarda o último timestamp aceite; a comparação é a distância com sinal em 32 bits (`wrapping_sub` lido como `i32`), para a volta do relógio não ser um recuo. Atrasado ou repetido, descarta-se — para a pista é uma perda de pacote, que o gravador já tinha.
- **O timestamp entregue ao `OggWriter` conta a partir do primeiro pacote da pista.** Ele só usa diferenças, e assim a subtracção dele não vê a volta. Não «simplificar» entregando o timestamp tal como chega: os testes de ficheiro passam em release na mesma, e é em debug que rebenta.
- **Um recuo que não passa é um relógio novo.** 50 atrasados SEGUIDOS que avançam entre si (`OPUS_CLOCK_RESYNC_AFTER`: 1 s de Opus contínuo em pacotes de 20 ms, que é o caso da ponte; com DTX são os mesmos 50 pacotes e mais tempo) re-ancoram a pista: o primeiro da série fica no instante do último escrito, e a pista continua daí com o tempo que o relógio novo mediu. Sem isto, uma origem que recomeçasse o timestamp para trás ficava muda na gravação até 12 h 25 min — pior do que antes da correcção, em que se gravava tudo. **Foi o revisor que o apontou, e bloqueava.** Um pacote em dia pelo meio desfaz a contagem; atrasados que não avançam entre si também.
- **Um salto para a FRENTE aceita-se tal como vem**: é tempo que passou (DTX, um telefone calado pelo anfitrião). Não lhe pôr tecto.
- **Um payload vazio não avança o relógio**: o `OggWriter` ignora-o sem mexer no dele.
- **O descarte é contado**, como a fila cheia (R40): `delonix_recording_audio_late_dropped_total`, um aviso ao primeiro e a cada 500, e um aviso próprio quando a pista re-ancora.
- **Não se mexe no crate externo.** A correcção vive em `RecSink::Audio`.

**Portão.** `recorder.rs`, dez testes, verdes em debug e em release:
- `pacote_de_audio_atrasado_nao_avanca_a_pista_um_dia` (0, 1920, 960: grânulos `1, 1921`, fecho escrito, um descartado contado), `depois_de_um_atrasado_ou_repetido_a_pista_continua`, `um_atrasado_do_outro_lado_da_volta_tambem_se_descarta`, `a_volta_do_relogio_de_32_bits_nao_e_um_recuo`;
- `o_relogio_conta_a_partir_do_primeiro_pacote_e_atravessa_a_volta` — é este que guarda a segunda regra no CI, que corre em release;
- `um_recuo_que_nao_passa_e_um_relogio_novo`, `uma_rajada_de_reordenacao_nao_e_um_relogio_novo`, `depois_de_o_relogio_recuar_de_vez_a_pista_volta_a_gravar`;
- `um_payload_vazio_nao_avanca_o_relogio_da_pista`, `o_audio_cifrado_passa_pelo_mesmo_relogio_e_sai_decifrado` (com cifra ponta-a-ponta: o atrasado e o frame que não autentica ficam de fora).

**Controlo negativo.** Os quatro primeiros, escritos ANTES da correcção e corridos contra o código antigo: em debug falham os quatro, com o pânico em `ogg_writer/mod.rs:181`; em release falham três, com os grânulos acima, e passa o da volta — que por isso guarda contra corrigir de mais, não contra o defeito. E quatro mutantes da correcção, corridos em release (o perfil do CI): com o ramo em dia a entregar o timestamp tal como chega falham 5 testes; sem re-ancorar, 2; com a comparação sem sinal, 3; sem o pacote em dia desfazer a contagem, 1 (o da rajada).

**Revisão.** `delonix-meet-webrtc`, por leitura e com medições próprias no ffmpeg 6.1.1. Bloqueou a primeira versão, que só descartava: é dela a re-ancoragem, e são dela os ramos que estavam sem teste (a contagem a partir do primeiro pacote no perfil do CI, o payload vazio, o frame que não autentica, o salto para a frente). Confirmou por leitura que a R40 fica intacta (a escrita, o contador e o aviso correm na thread dedicada; `try_send` e `close` não mudaram), que o vídeo só mudou na forma de receber o pacote, e que uma `TrackRemote` nova dá uma publicação, um `RecWriter` e um relógio novos. Uma segunda passagem, só à re-ancoragem e também por leitura, não bloqueou: deu a aritmética como certa com a volta pelo meio e o timestamp entregue como estritamente crescente depois de re-ancorar; apontou que nenhum teste falhava sem o pacote em dia desfazer a contagem (o teste da rajada foi corrigido, e o quarto mutante é a prova) e acertou seis frases desta entrada.

**O que NÃO está provado.**
- **Nenhuma gravação com um browser ou um telefone a sério.** Os pacotes são 20 ms de silêncio fabricados; a reordenação é a ordem em que o teste os escreve.
- **O ffmpeg da imagem (9.0.2).** As medições da composição são do 6.1.1 desta máquina, sobre ficheiros com os grânulos do `OggWriter` mas montados à mão, não escritos por ele.
- **Que uma perna de telefone recomeça o relógio a meio** (re-INVITE, retenção, transferência). A re-ancoragem responde a essa hipótese; ninguém a viu acontecer.
- **Uma re-ancoragem errada.** 50 pacotes atrasados, por ordem, depois de UM que lhes passou à frente, são lidos como relógio novo, e o resto da pista fica deslocado a distância a que esse pacote passou — 1 s no mínimo, sem tecto (190 frames à frente dão 3,8 s, para sempre). Não se conhece rede que o faça; fica escrito.
- **Um relógio novo em que cada pacote chegue repetido** nunca re-ancora: um atrasado que não avança sobre o anterior recomeça a contagem, e a pista fica muda. A alternativa (ignorá-lo) deixava muda a origem que recuasse duas vezes seguidas. Repetidos não chegam de um browser (o SRTP deita-os fora — não verificado) nem da ponte.
- **Se o `webrtc-rs` entrega uma `TrackRemote` nova quando o SSRC muda no mesmo `mid`.** Se não entregar, o relógio da pista é o mesmo para os dois fluxos, e é a re-ancoragem que os separa.
- **Um timestamp muito à frente** (até 2^31) é aceite, estica a pista esse tempo e deixa os pacotes seguintes atrasados durante 1 s, até re-ancorar. Só o próprio publicador o faz a si mesmo; acima de 10 s, o ffmpeg 6.1.1 desconta o salto.
- **Quem ficou sem áudio não se sabe pelo log.** O aviso leva o nome da pista (`03-audio`), sem sala nem publicador, e o «gravação DEGRADADA» do fecho só conta a fila cheia. Uma falha de decifra continua sem contador (já era assim).
- **Em debug, uma pista cujo relógio avance mais de 2^32 amostras** do primeiro pacote ao último volta ao pânico do `OggWriter`. Em release dá certo.
- **Um buraco de menos de 10 s numa pista era fechado pela mistura** (`adelay`/`amix`, sem `aresample=async`): medido pelo revisor com ficheiros sintéticos, e era o que acontecia a cada pacote descartado ou perdido — 20 ms de cada vez. Com DTX era muito mais. **Confirmado com gravações reais e corrigido na R295**: a composição passou a encher esses buracos com silêncio.
- A bomba do SFU renumera a sequência do áudio depois de gravar (`next_seq`): um pacote atrasado segue para quem ouve com sequência contígua e timestamp para trás. Não foi mexido nem medido.

**Ficheiros.** `server/src/recorder.rs` (`OpusClock`, `RecSink::Audio`, `RecWriter::spawn`), `server/src/metrics.rs`.

### R295 — A fala depois de um silêncio recuava na gravação: os buracos de PTS do DTX iam para o ficheiro final

**Sintoma.** Numa gravação do servidor, o que um participante diz depois de se calar ouve-se antes do tempo. O áudio adianta-se ao vídeo e aos outros participantes, e cada silêncio soma ao anterior. Medido a 2026-10-05 em gravações a sério de 36 s (dois Chromium com `usedtx=1`; um microfone fala 2 s em cada 8, o outro dá um toque por segundo), com o ffmpeg 9.0.2 da imagem do servidor:
- a pista crua de quem se cala tinha 60 a 63 buracos de PTS de 0,40 s — 21,6 a 22,8 s no total;
- o ficheiro final tinha 14,4 s de amostras de áudio para 36 s de gravação (23,6 s para 46,4 s na grelha), com os mesmos buracos nos PTS do contentor;
- o Chromium, a tocá-lo, punha a fala aos 2,8 / 5,8 / 8,8 / 11,8 s do media em vez de 7,7 / 15,7 / 23,7 / 31,7 s — de 3 em 3 s o que foi dito de 8 em 8;
- na grelha, os toques do segundo participante saíam a 0,62 s uns dos outros em vez de 1 s.

Nos TRÊS caminhos do `finalize_inner`: grelha com mistura, só áudio, e um publicador (`-c copy`) — que é a gravação mais comum.

**Causa raiz.** O cliente pede `usedtx=1` (`web/src/webrtc.ts`): em silêncio o Opus manda um pacote a cada 400 ms e o timestamp RTP continua a andar. O `OggWriter` avança o grânulo pelo timestamp, por isso a pista gravada guarda o salto nos PTS e nenhuma amostra no meio. O `adelay` e o `amix` contam amostras, não PTS; o codificador e o `-c copy` levam os PTS com buracos para o webm; e o Chromium ignora esses buracos e toca as amostras seguidas. Um pacote perdido faz o mesmo, 20 ms de cada vez.

**Regra.**
- **Toda a pista de áudio passa por `AUDIO_GAP_FILL` (`aresample=async=1:first_pts=0`) antes de qualquer outro filtro**, em todos os caminhos. O grafo de áudio da composição constrói-se em `audio_mix_graph` e os argumentos de um publicador em `single_publisher_args`; não se escrevem à mão no `finalize_inner`.
- **No caminho de um publicador o vídeo vai em cópia e o áudio NÃO.** `-c copy` (ou `-c:a copy`) no áudio repõe o defeito. O custo de o recodificar, medido numa gravação de 36 s: 0,13 s em vez de 0,01 s, e um ficheiro 3,6 % maior.
- **O áudio de uma gravação mede-se a contar amostras, ou a ouvi-la no Chromium.** A duração do contentor dizia 36 s com 14 s de áudio lá dentro, e descodificar com um filtro que honre os PTS repõe o silêncio que o browser não repõe. A primeira versão do `gravacao-buraco-audio.mjs` media assim e deu os três caminhos por bons com o servidor por corrigir.
- **Não subir o `-dts_delta_threshold` para apanhar buracos maiores.** O `aresample` guarda o silêncio inteiro em memória antes de o entregar — medido: 440 MB para um buraco de 6 min, 3,2 GB para 1 h.

**Portão.** `cargo test --lib recorder`:
- `o_audio_e_enchido_antes_de_qualquer_outro_filtro` — corre no CI: o enchimento está em todas as cadeias, antes do `adelay`, e nenhum caminho leva o áudio em cópia;
- `a_fala_depois_de_um_buraco_nao_recua_na_mistura`, `…_numa_pista_so`, `…_com_um_publicador` — pistas escritas pelo `open_track` de produção, com perda e com o padrão do DTX, compostas pelo ffmpeg com os argumentos de produção, e o resultado medido amostra a amostra. **Precisam de ffmpeg, que o CI não tem: lá dão `ok` sem medir nada** — o aviso é um `eprintln!` que o `cargo test` engole num teste que passa. No CI só o teste de cima guarda esta regra. Correm em local com `FFMPEG_BIN`; passaram com o 6.1.1 da máquina e com o 9.0.2 da imagem.
- Controlo negativo corrido: com o filtro trocado por `anull` falham os quatro (a pista de 6 s sai com 4,0 s).

Fora do CI: `web/e2e/gravacao-buraco-audio.mjs`, a gravação a sério com os três caminhos, ouvida no Chromium. Com o servidor corrigido: zero buracos de PTS, as amostras cobrem a gravação inteira, a fala de 8 em 8 s (7,94 / 8,00 / 8,00), os toques entre 0,98 e 1,00 s, e no Chromium a fala aos 7,8 / 15,9 / 23,9 / 31,9 s. Com o servidor por corrigir falham 15 das suas verificações.

**O que NÃO está provado.**
- **Mais de 10 s sem um único pacote continua a recuar.** O ffmpeg trata esse salto como descontinuidade e tira-o antes de o filtro o ver (medido: um buraco de 9,9 s é enchido, um de 10,5 s desaparece). Com o DTX do browser não acontece, que há um pacote a cada 400 ms. Acontece a quem fica sem rede ou com o browser suspenso mais de 10 s — e, sem avaria nenhuma, a **um telefone calado pelo anfitrião** (R224): com `ForceMute` a perna da ponte não publica nada (`phone_bridge/leg.rs`), e quando volta a falar fica adiantada o tempo todo em que esteve calada. O mesmo para um tronco com supressão de silêncio. Lido no código pelo revisor; não foi gravado um telefone. Só se fecha a escrever o silêncio na própria pista, ao gravar.
- **Os primeiros 20 ms de cada fala ficam ANTES do silêncio.** O demuxer OGG do ffmpeg dá a cada pacote o grânulo da página anterior, por isso o primeiro pacote depois de um buraco cola-se ao último que chegou e o filtro enche a seguir a ele. Medido nas três gravações corrigidas: um fragmento de 10 ms aos 7,785 s e a fala a começar aos 7,930 s (achado do revisor, repetido por mim). Com DTX são até 380 ms de adianto, num pedaço de 20 ms; antes da correcção colava a fala inteira. Não foi ouvido com voz. Fecha-se no mesmo sítio que o ponto de cima: um pacote de silêncio escrito na pista antes do primeiro pacote depois de um salto (simulado pelo revisor sobre as páginas OGG: o fragmento desaparece, e com um marcador a cada 5 s um buraco de 6 min fica inteiro com 18 MB de memória).
- **Perdas isoladas ficam com até 100 ms de desvio.** O `async=1` só repõe quando a soma dos buracos passa de 100 ms (medido: 12 pacotes perdidos, um a cada 500 ms, dão dois silêncios de 120 ms em vez de doze de 20).
- **Só o Chromium foi ouvido.** Firefox, Safari e os leitores de ficheiros não foram medidos, nem antes nem depois.
- **Cada pista de áudio entra 80 ms adiantada**: o `OggWriter` declara 3840 amostras de `pre-skip` e o ffmpeg desconta-as ao início. É constante, já era assim, e não foi corrigido aqui.
- **Perda e atraso de rede a sério** não foram exercitados numa gravação real — só em pistas sintéticas. Nem uma gravação E2EE, nem a pista de um telefone da ponte.
- **O áudio da grelha de uma gravação arrancada À MÃO a meio da chamada** não foi medido, porque essa gravação falha antes: a pista de vídeo começa sem keyframe (o `start_recording` não pede nenhum, e a guarda do `Vp8IvfWriter` lê o bit de keyframe também nos pacotes de continuação e deixa passar quadros delta). Medido: três pistas de vídeo em três que o ffmpeg não descodifica, e a grelha a acabar em `ffmpeg exited with exit status: 69`. O teste usa `auto_record` por isso. Defeito à parte, por corrigir.

**Ficheiros.** `server/src/recorder.rs` (`AUDIO_GAP_FILL`, `audio_mix_graph`, `single_publisher_args`), `web/e2e/gravacao-buraco-audio.mjs`.

### R296 — A voz de um softphone chegava à sala a 8 kHz, e a prova real da perna em Opus encontrou três defeitos que já lá estavam

**Sintoma.** Um softphone falava Opus a 48 kHz com o FreeSWITCH e a perna para a ponte era forçada a PCMA: tudo acima de ~3,4 kHz ficava pelo caminho («baixo e sem qualidade», 2026-10-05). Medido antes de mexer: a voz chegava à ponte a −20 dBFS e a ponte era transparente em nível e em espectro nos dois sentidos — o estrangulamento era a perna a 8 kHz, não os codecs. Decisão no [ADR-0018](../adr/0018-a-perna-da-ponte-negoceia-opus.md).

Pôr a perna em Opus e medi-la contra um FreeSWITCH real mostrou mais do que a banda:

1. **Com o FreeSWITCH autorizado por NOME a ponte nunca era entregue ao IVR.** `voice::room_bridge_for` só olhava para os IPs literais de `PHONE_BRIDGE_FREESWITCH_IPS`; o compose e o chart usam o nome do serviço. O UA arrancava («origens=1») e todas as chamadas caíam na conferência local, com um aviso de «lista vazia» no log.
2. **Quem desligava o telefone continuava na sala.** Com o UA à escuta em `0.0.0.0`, o `200` levava `Contact: <sip:bridge@0.0.0.0:5090>`; o `BYE` do FreeSWITCH ia para lá. A perna, o socket de RTP e a publicação ficavam vivos até o servidor reiniciar.
3. **A sala chegava ao telefone com os agudos dobrados.** O misturador pedia 8 kHz ao descodificador do `opus-rs`, que desce um pacote SILK de banda larga sem filtro: um tom de 6 kHz saía inteiro (−27 dB onde a libopus com filtro dá −72).

**Regra.**
- **A perna da ponte fala o primeiro codec da oferta que souber** — Opus (RFC 7587, PT dinâmico) ou PCMA/PCMU; sem nenhum, `488`. O servidor manda `OPUS,PCMA`; `PHONE_BRIDGE_WIDEBAND=0` repõe `PCMA`, e um valor que não se perceba DESLIGA.
- **Em Opus, telefone → sala não se recodifica.** O payload passa intacto; a ponte valida-o (tecto de 600 bytes, tem de descodificar), mede-lhe o nível, não deixa sair atrasados nem repetidos, e o relógio de saída segue o da origem — também através de um silêncio imposto. A sequência não abre buraco pelo que a ponte reteve.
- **Sala → telefone em Opus é uma mistura a 16 kHz** num só fluxo de banda larga, a taxa constante (em taxa variável o `opus-rs` ignora o alvo).
- **A resposta SDP não pede FEC nem anuncia taxa de captura.** Com `useinbandfec=1` a libopus do FreeSWITCH codifica em banda média para o FEC caber, e o `opus-rs` (0.1.33 e 0.1.34) lê mal a banda média a qualquer taxa — um tom sai 8 dB abaixo, fala sai 15 dB acima e distorcida. O misturador deixa esses pacotes de fora.
- **O misturador descodifica sempre a 16 kHz** e desce a soma para 8 kHz com um passa-baixo, uma vez por perna.
- **Um valor com vírgula na dial string é escapado** no `dialin_ivr.lua`. Sem isso o FreeSWITCH parte a lista e fica só com o primeiro codec.
- **O `Contact` leva o IP onde as pernas abrem o RTP** quando o de escuta é indefinido — o deste processo, não o do Service: o diálogo vive nesta réplica.
- **A ponte é entregue com a lista por IP OU por nome**; sem nenhum dos dois continua a não haver ponte.
- **A perna larga a sala mesmo que a sua tarefa rebente**: o descodificador lê bytes da rede.

**Portão.** No CI: `cargo test --lib phone_bridge::` (61, 23 novos — a negociação, o `Contact`, um minuto de fluxo com perda e troca de ordem nas fronteiras dos dez segundos, o silêncio imposto, o tecto, seis pacotes reais da libopus, banda média fora da mistura, 20 000 pacotes hostis sem um pânico, a resposta do filtro, e os 6 kHz que chegam em Opus e não chegam nem dobram em G.711); `cargo test --lib ponte_em_opus` contra o SFU (5 e 6 kHz nos dois sentidos, payloads byte a byte iguais aos enviados, silenciar e voltar); `cargo test --test ramal_entra_na_sala` contra Postgres (a ordem dos codecs, o interruptor, a ponte por nome, e o controlo negativo da lista vazia).

Fora do CI, medido a 2026-10-05 no compose do laboratório (FreeSWITCH 1.11.3, libopus 1.3.1), com `scripts/softphone-prova.sh par` — dois softphones em dois ramais, na mesma sala, SRTP obrigatório:
- a oferta do FreeSWITCH à ponte traz `opus/48000/2` e `PCMA` (`absolute_codec_string=OPUS\,PCMA` no log dele) e a ponte responde Opus;
- cada softphone ouve o tom do outro a 0,2487 e 0,2452 (enviado a 0,25), estável, e o próprio a 0,001;
- **controlo negativo da banda média:** com a primeira resposta SDP (`useinbandfec=1`) o 1 kHz chegava a 0,092, em quatro corridas, e o registo de depuração do `mod_opus` mostrava 2 583 blocos em `MEDIUMBAND`;
- com `PHONE_BRIDGE_WIDEBAND=0` a perna volta a PCMA e os tons chegam a 0,2504 e 0,2506;
- com o FreeSWITCH só por nome as duas pernas abrem — **controlo negativo:** com a imagem anterior, o mesmo compose registava «lista vazia» e nenhuma perna abria;
- quando os softphones desligam, as duas pernas FECHAM: «ponte: perna fechada» com `rejected=0 srtp_failed=0 decode_errors=0 mix_unsupported=0 opus_rejected=0 opus_late=0` em ~2 000 pacotes por sentido, e o servidor fica só com o socket do UA — **controlo negativo:** com o `Contact` antigo, a mesma corrida deixava as duas pernas e os dois sockets de RTP abertos, sem uma linha no log.

**Revisão.** Três revisões independentes do primeiro commit (media, segurança, Rust), antes de qualquer chamada. Encontraram o que os testes não viam: a vírgula da dial string (bloqueava), a âncora da passagem que voltava a ancorar de dez em dez segundos e apagava a perda nessa fronteira, a frase do ADR sobre a sequência (o SFU renumera-a), a mudança de confiança por nomear, o tecto de payload em falta, e a sala que ficava presa se a tarefa da perna rebentasse. A chamada real encontrou o resto: a ponte por nome, a banda média, e o `Contact`.

**O que NÃO está provado.**
- **A banda larga com um softphone que fale Opus.** Os da prova falam G.711: provam a negociação, o nível, o mix-minus e o fecho — a banda, só os testes com tons de 5 e 6 kHz contra o SFU. Falta um Linphone real e a mesma chamada ouvida num browser.
- **A gravação de uma perna em Opus**, e o palco (R225) nesse codec.
- **A robustez do `opus-rs` é uma amostra, não uma auditoria**: 20 000 pacotes no repo, 300 000 fora dele, zero pânicos. O crate tem `unsafe` e não lê banda média.
- **CPU em release**: os números dos testes são de um binário de debug, numa máquina carregada.
- **O cluster e o chart**: tudo isto foi medido no compose.
- **O defeito do `opus-rs` com a banda média não foi reportado aos autores.**

### R298 — O portão de higiene acusava regressões que existem: o `grep -q` matava o `echo`, e o `pipefail` lia isso como «não encontrado»

**Sintoma.** Medido a 2026-10-05 com o host a carga 20–44: `bash scripts/check-repo-hygiene.sh` falhou em 2 corridas de 4 na mesma árvore, sem nada ter mudado entre elas, com «referência a R99 sem entrada no catálogo» e «referência a R211 sem entrada no catálogo» — e as duas entradas existem. Os números acusados mudam de corrida para corrida (31 diferentes em 50 corridas). Um portão que falha sem razão ensina a correr outra vez até dar verde, e é esse hábito que deixa passar a falha verdadeira.

**Causa raiz.** A secção das referências fazia `echo "$rnums" | grep -qx "$r" || { … fail=1; }`, uma vez por referência (mais de duzentos pipelines por corrida), num ficheiro com `set -o pipefail`. O `echo` do bash escreve para um pipe **uma linha de cada vez** (medido com `strace`: seis linhas, seis `write`); o `grep -q` sai à primeira correspondência; se o `echo` ainda vai a meio da lista, morre com SIGPIPE (estado 141). Com `pipefail` o pipeline dá 141 embora o `grep` tenha dado 0, e o `||` trata isso como «não está no catálogo». Não era uma hipótese lida no código: uma cópia instrumentada que regista o `PIPESTATUS` deu 34 linhas falsas em 50 corridas, e as 34 com `(141, 0)` — o `echo` morto, o `grep` a encontrar o número.

**Regra.**
- **Num script com `pipefail`, um veredicto nunca sai do estado de um pipe cujo último comando pode sair antes de ler tudo** (`grep -q`, `head -n`, `grep -m`). Lê-se para uma variável e dá-se por `<<<`, ou dá-se o ficheiro ao `grep`: `grep -qx "$r" <<<"$rnums"`. Não há produtor para morrer.
- **O erro tem dois sentidos.** Com `||` é um vermelho falso (este). Com `&&` é um **verde falso**: `code | grep -q -- "$forbid" && bad`, no `check-ffmpeg-licenca.sh`, deixava passar uma opção proibida quando o `grep -v` a montante morria. Medido fora da árvore, sob carga, com um Dockerfile de 10 kB e `--enable-gpl` na primeira linha: a forma antiga não o viu em 86 de 500 voltas; a nova, em nenhuma. Com um de 2,5 kB, zero em 500 — abaixo dos 4096 bytes do buffer do stdio o `grep -v` escreve tudo de uma vez. O `Dockerfile.server` tem hoje 2782 bytes fora dos comentários: o portão da licença estava certo por o ficheiro ser pequeno, e deixava de estar quando crescesse.
- **Tirar o `pipefail` não é a correcção**: esconde também o produtor que falha a sério.
- O padrão saiu de quatro portões: `check-repo-hygiene.sh` (as referências, o cabeçalho `MÓDULO DE APOIO` dos e2e e o livro das chaves aceites), `check-ffmpeg-licenca.sh` (cinco sítios), `check-isolamento-cobertura.sh` (um) e `check-bordo-central.sh` (o `linha()`, onde um `head -1` a sair cedo acrescentava um `0` ao número da linha por via do `|| echo 0`).
- **O estado do produtor que interessa lê-se à parte, não se perde.** No `check-ffmpeg-licenca.sh`, com um binário em `FFMPEG_BIN`: um `ffmpeg -L` que falha continua a ser vermelho (a primeira versão desta correcção deixava de olhar para ele — achado da revisão), e um `ffmpeg -version` que escreve `--enable-gpl` e sai com erro passa a ser vermelho — com `… | grep -E … && bad` e `pipefail` era verde, sem SIGPIPE nenhum.

**Portão.** O próprio `scripts/check-repo-hygiene.sh` (`make fitness` e CI) recusa, em qualquer `scripts/check-*.sh`, um `grep -q` (ou `--quiet`) à saída de um pipe **na mesma linha**, fora de comentários — `grep`, `egrep` e `fgrep`, com caminho, com `command`, `xargs` ou uma variável à frente, e o `-q` em qualquer posição entre as opções. O que ele não vê está no «não provado».

**Prova corrida a 2026-10-05**, num host de 32 núcleos partilhado com outras sessões:
- antes, sem carga acrescentada (host a 18–27): **4 falhas em 50 corridas**, sete linhas falsas, sete números diferentes;
- antes, com um `yes` por núcleo (host a 32–72), na cópia instrumentada: **23 corridas de 50** com pelo menos uma linha falsa, 34 linhas, todas com `PIPESTATUS` `(141, 0)`;
- depois, com a mesma carga (host a 56–65): **0 falhas em 50**;
- os controlos negativos do portão continuam a falhar: uma referência a um número sem entrada num `.md`; um `### R59` repetido; um e2e novo sem CI nem razão escrita; `MÓDULO DE APOIO` só depois da linha 20; um caminho tirado do livro das chaves, e o mesmo caminho deixado só em comentário; um `echo x | grep -q x` e um `cat f | grep -E -iq x` num `check-*.sh`. E o que tem de passar, passa: um módulo de apoio declarado no cabeçalho; um comentário, um `<<<` e um `|| grep -q` não disparam o portão novo;
- o portão novo acusa também `grep -m1 -q`, `egrep -q`, `LC_ALL=C grep -q`, `/usr/bin/grep -q`, `grep -e p -q`, `grep x --quiet`, `|& grep -q`, `command grep -q` e `xargs grep -q`; e cala-se com `grep -c`, `grep -oE … | sort -u` e `grep -v … | wc -l`;
- os outros três portões dão a mesma saída e o mesmo estado que a versão anterior, na árvore como está e em nove mutações (`--enable-gpl`, o SHA-256 a zeros e curto, um `ffmpeg` GPL, um LGPL e um que escreve «Lesser» e sai com erro em `FFMPEG_BIN`, o `isolamento.mjs` sem as linhas de um recurso, o `kamailio.cfg` sem `allow_source_address` e sem o digest). A única diferença é a pretendida: o binário GPL cujo `-version` sai com erro era verde e é vermelho.

**O que NÃO está provado.**
- **Só no portão de higiene a falha foi reproduzida na árvore real**, e só na linha das referências. Nas outras duas linhas dele (cabeçalho dos e2e, livro das chaves) o `PIPESTATUS` deu zero nas 50 corridas instrumentadas — o produtor é um `head` ou um `grep` com menos de 4096 bytes para escrever. Os outros três portões foram corrigidos por leitura; o que se mediu neles foi a equivalência com a versão anterior.
- **O portão novo só vê `grep -q` na mesma linha da barra**, e não distingue uma barra dentro de uma cadeia de texto nem um comentário no fim de uma linha de código (falha fechado). Um `| head -1` cujo estado decida alguma coisa, um `grep -m`, um `awk … exit` são revisão. A revisão procurou-os nos 22 portões e não encontrou nenhum cujo estado decida um veredicto; os `| head` que restam ou estão dentro de um `$(…)` sem estado lido, ou só apresentam uma falha já marcada.
- **Os scripts que não são portões não foram tocados.** Há 37 pipes para `grep -q` com `pipefail` em dez deles (`cluster-voice.sh` e `pbx-tronco-prova.sh` com oito cada, `compose-voice-check.sh` com seis, `cluster.sh` e `softphone-prova.sh` com quatro, e outros), quase todos com um `kubectl exec` ou um `docker exec` a montante. Uma prova de voz que falha «às vezes» numa máquina carregada pode ser isto.
- **Não correu no runner do CI**, só nesta máquina (bash 5.2.21, grep do GNU). A carga foi de CPU (um `yes` por núcleo); não se mediu com o disco ou a memória em pressão.

**Ficheiros.** `scripts/check-repo-hygiene.sh`, `scripts/check-ffmpeg-licenca.sh`, `scripts/check-isolamento-cobertura.sh`, `scripts/check-bordo-central.sh`.

### R300 — Um tronco alterado ou apagado ficava no FreeSWITCH até alguém o reiniciar

**Sintoma.** Os troncos chegam ao FreeSWITCH por um ciclo que volta a ler a lista do servidor (R291) — e esse ciclo só ACRESCENTA. Mudar a password de um tronco, o host, ou desactivá-lo não tinha efeito: o FreeSWITCH continuava registado na operadora com os dados antigos. **Um tronco apagado na consola continuava registado na operadora**, até alguém reiniciar o FreeSWITCH. Tirar um gateway pede `killgw`, que o servidor manda pelo Event Socket (ESL); o ESL passou a estar aberto ao servidor no compose e no cluster local (#218), mas o servidor só o usava no botão «reiniciar registo», e o chart do Helm nem passava a password ao FreeSWITCH.

**Regra.**
- **O servidor avisa o FreeSWITCH** quando um tronco é criado, apagado, ou quando MUDA um campo da ligação à operadora (host, porta, transporte, SRTP, registo, utilizador, activo; a password conta sempre que vem) — mudar o nome ou os prefixos não deita um registo abaixo, e repetir num PATCH o valor que já lá estava também não (`telephony_trunks::refresh_gateway`). Não espera pela resposta nem falha o pedido: a base é a verdade, e o ciclo de releitura continua a ser a rede de segurança para o que é NOVO.
- **Os avisos passam por uma fila com um só trabalhador** (`telephony_service::GatewayRefresh`): uma ligação ao ESL de cada vez e um `rescan` por cada 2 s, com os gateways juntos. Cada `rescan` relê o XML inteiro, aloca um gateway novo no perfil e escreve uma linha de aviso por gateway de TODAS as organizações; sem a fila, um administrador a alterar o seu tronco em ciclo abria uma ligação e um `rescan` por pedido, sem tecto.
- **Entre o `killgw` e o `rescan` espera-se que o gateway antigo SAIA do perfil.** Não é o `rescan` que precisa — um gateway marcado deixa logo de ser visto, e o `rescan` recria-o de imediato. É a operadora: o antigo ainda tem de mandar o seu desregisto, e o novo regista-se com o MESMO contacto. Com o `rescan` imediato o FreeSWITCH regista o novo e só depois desregista o antigo (medido); um registrar que apague por contacto ficava sem registo, com o FreeSWITCH a dizer `REGED`. Olha-se às DUAS listas (`gwlist` e `gwlist down`): `gwlist` sem argumento só traz os gateways UP.
- **`DELONIX_ESL_CIDRS` estreita quem entra no ESL.** Sem ela a lista de acesso são todas as redes privadas, e num compose ou num cluster isso é toda a gente: a password é a única barreira. Com ela, só essas redes e loopback. Cada entrada tem de ter UMA `/` e uma máscara só de dígitos, sem zero à esquerda e diferente de zero: o FreeSWITCH lê a máscara com `atoi`, e vazia, `00` ou com letras dá zero bits — a lista «estreita» ficava aberta a toda a gente.
- **O chart do Helm** dá ao FreeSWITCH a `TELEPHONY_ESL_PASSWORD` do Secret quando `server.telephony.eslAddr` está posto, aceita `voice.freeswitch.eslCidrs`, e com `networkPolicy.enabled` fecha o 8021 aos pods do servidor (`freeswitch-esl`), deixando o resto do pod como estava. **Recusa** o ESL com o FreeSWITCH em `hostNetwork` sem `eslCidrs` (a política não se aplica e o ESL escuta nas interfaces do nó) e, em produção, sem política nem `eslCidrs`.
- **A camada do motor confere o que tira** (`m_rm`): o `rm -f` do delonix 4.5.0 deixou duas vezes num dia um contentor em «Dead» — a rede dele ficava de pé e a prova seguinte não conseguia criar a mesma sub-rede.

**Uma afirmação desta entrada que estava errada, e saiu.** A primeira versão dizia que um `rescan` logo a seguir ao `killgw` ainda encontrava o gateway e não o recriava, e esperava por `sofia status gateway`. Uma revisão independente leu no fonte que não (`sofia_reg_find_gateway` não devolve um gateway marcado, e o «Ignoring duplicate gateway» só vale para os não marcados): a espera era inerte, e a prova passava igual com ela. A razão verdadeira para esperar é a da operadora, acima — e essa ficou medida.

**Portão.** `bash scripts/troncos-prova.sh`, passo 10 — fora do CI; **56 verificações, 0 falhas**, medidas a 2026-10-05 no delonix 4.5.0, num host partilhado com a carga entre 4 e 64 nas várias corridas. Com o ESL aberto só ao endereço do servidor:
- a API vê no FreeSWITCH o registo do tronco (`registration=registered`);
- **tronco alterado** (password errada): o registo cai na operadora em 2 s, sem reiniciar nada; reposta a password, volta a registar-se sozinho e leva uma chamada, com registo;
- **rodar a password de um tronco que ESTÁ registado**: 9 s depois continua `REGED` no FreeSWITCH e registado na operadora — e **pela ordem certa**: no log, o gateway antigo sai («Deleted gateway») antes de o novo entrar («Added gateway»);
- **tronco apagado**: o gateway sai do FreeSWITCH e desregista-se da operadora;
- outro contentor da mesma rede, **com a password certa**, não entra no ESL (e o mesmo comando, contra o seu próprio FreeSWITCH, entra); a password de fábrica não entra, nem em loopback; a password do ESL não aparece no `freeswitch.log` nem no log do servidor.

**Os três controlos**, com a mesma prova:
- `SEM_ESL=1` (o ESL fechado, como antes do #218): 50 ✓ e 0 ✗ com as expectativas AO CONTRÁRIO — 32 s depois o tronco alterado continua registado com a password antiga e o apagado continua registado na operadora; a API não sabe do registo; o 8021 não responde a outro contentor;
- o servidor de antes desta regra, com o ESL aberto: 51 ✓ e 4 ✗ — a API já vê o registo, mas o tronco alterado e o apagado ficam como estavam. É este que diz que a medição vem do aviso do servidor, e não de o ESL estar aberto;
- o servidor com o `rescan` imediato: 55 ✓ e 1 ✗ — «Added gateway» antes de «Deleted gateway». É este que diz que a espera faz alguma coisa.

No CI: `tests/telephony.rs` (contra um ESL falso) — a ordem `killgw` → `gwlist` → `gwlist down` → `rescan`; mudar só o nome não toca no registo; mudar a password, criar e apagar avisam. `check-helm.sh` — o FreeSWITCH só recebe a password com `eslAddr`, a política deixa o 8021 SÓ aos pods do servidor (estragada de duas maneiras à mão, o portão falhou nas duas), e as duas recusas. O passo 4 do arranque foi corrido contra a configuração vanilla do fonte do FreeSWITCH: fechado, só a password, estreitado, só a lista (continua fechado), e onze formas de `DELONIX_ESL_CIDRS` — nove recusadas (`/`, `/0`, `/00`, `/08`, `/x`, `//8`, sem máscara, `::/0`, e um CIDR com XML).

**O que NÃO está provado.**
- **Nada correu num cluster.** A política de rede e o ambiente do chart estão conferidos no render, não aplicados; se o CNI a impõe (e o `endPort`, de que a regra «tudo o resto» depende), e se os pods do servidor chegam ao 8021 com ela, ninguém mediu. O tráfego do próprio nó costuma passar ao lado de uma política.
- **No compose e no `make cluster` a lista de acesso continua larga** (todas as redes privadas): os contentores não têm endereço fixo para estreitar. Lá a password é a única barreira, em claro — o ESL não tem TLS — e visível no argumento do `fs_cli` da sonda de prontidão e, na prova, no argumento do `exec` que a passa à operadora.
- **Uma operadora que não seja o FreeSWITCH.** A ordem «desregisto do antigo, registo do novo» está medida no log do nosso lado, com uma margem de um ciclo (cerca de um segundo); o que um registrar de verdade faz com dois pedidos tão próximos, não.
- **Mais de um FreeSWITCH, ou mais de uma réplica do servidor**: o servidor fala com UM endereço de ESL, e a fila dos avisos é por processo. Com dois FreeSWITCH só um é avisado, e cada um registaria o mesmo tronco na operadora. É o resto da linha T11 do plano de lacunas, que continua aberta — esta entrada só fecha «o ESL fora do loopback» e o que dele dependia.
- **Se o aviso falha, só fica um aviso no log**: não há nova tentativa, e um tronco alterado fica com os dados antigos até um reinício (o ciclo não tira gateways). A tarefa da fila também não espera pelo fim ordenado do servidor.
- **A carga de muitos avisos**: a fila limita-os a um `rescan` por 2 s, mas cada um continua a alocar no perfil memória que só se liberta ao parar o perfil, e a escrever uma linha por gateway existente. A grandeza não foi medida.
- **O «reiniciar registo» e a «chamada de teste» da consola** (`/sip-registration/restart`, `originate`) não foram medidos contra um FreeSWITCH real aqui; o primeiro usa o código que a prova exercita ao alterar e apagar. Os erros dessas rotas devolvem ao administrador o texto cru do FreeSWITCH e o endereço interno do ESL — dívida anterior, não tocada.
- **Um tronco DESACTIVADO** (`enabled=false`) segue o mesmo caminho do apagado, por leitura; nenhuma chamada o mediu.
- **Uma corrida da prova falhou sem causa apurada** (48 ✓, 8 ✗, na árvore de integração, com o host a carga 30): depois do reinício do FreeSWITCH nada ENTRAVA no contentor — nem a chamada do softphone, nem o ESL do servidor —, e o que SAÍA funcionava (o tronco registava-se). A repetição, com o mesmo binário e a mesma árvore, passou (56 ✓). A suspeita é o motor ter devolvido o contentor com outro endereço; não ficou confirmada. A prova passou a conferir o endereço depois de cada reinício (`confere_ip`), para a próxima dizer se é o motor ou o Meet.
- A prova só correu no delonix; o caminho do docker dela continua por exercitar (R292).

**Ficheiros.** `server/src/telephony_trunks.rs`, `server/src/telephony_service.rs`, `server/src/telephony_esl.rs`, `server/tests/telephony.rs`, `voice/cluster/freeswitch-entrypoint.sh`, `deploy/helm/delonix-meet/templates/voice.yaml`, `deploy/helm/delonix-meet/templates/_helpers.tpl`, `deploy/helm/delonix-meet/values.yaml`, `deploy/helm/delonix-meet/README.md`, `scripts/check-helm.sh`, `scripts/motor.sh`, `scripts/troncos-prova.sh`.
