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
