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
