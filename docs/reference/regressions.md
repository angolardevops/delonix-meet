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
