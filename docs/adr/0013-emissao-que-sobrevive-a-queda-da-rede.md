# ADR-0013 — Emissão que sobrevive à queda da rede

**Estado:** Proposto · **Data:** 2026-09-17 · **Estende:** [ADR-0003](0003-directo-para-plataformas.md) (a decisão central — o browser codifica, o servidor copia o vídeo — mantém-se)
**Pedido que o origina:** ecrã `DelonixRecovery` do Navegavel4 (`notas-ui-template/v4/DelonixRecovery.*`): «A ligação caiu. Continue a falar — nada se perdeu.»
**Revisto antes do código** pela dona da linha de backend (delonix-meet-44), com quatro exigências que este ADR incorpora: o envio em diferido não se promete a destinos RTMP ao vivo, as falhas isolam-se por destino com números medidos, o registo é escrito à medida que acontece, e «baixar para 1080p» não pode tocar na gravação.

## Contexto

O ecrã promete seis coisas: a gravação continua quando a Internet cai; cada destino cai e volta sozinho, com tentativas contadas; os segundos perdidos chegam depois; a sala e o telefone não dão pela queda; o anfitrião pode repor, baixar a qualidade ou largar a emissão e ficar só a gravar; e fica um registo minuto a minuto anexo à gravação.

### O que existia, medido a 2026-09-17 contra `origin/seg/ssrf-saida` (`d3ffd8f`)

`server/src/broadcast.rs` lança **um** `ffmpeg` com N saídas `-f flv`. O `stderr` vai para um cano que ninguém lê. Não há estado por destino, nem reinício, nem gravação do fluxo emitido. A gravação do servidor (`recorder.rs`) é outra coisa: grava o RTP de cada participante a partir do SFU e não passa pelo directo.

Medições feitas nesta máquina (32 vCPU partilhados; **load average entre 35 e 72 durante as medições**, porque correm outras sessões — os valores de CPU são por processo e contam o tempo de CPU efectivamente gasto, mas a folga de tempo real está pior do que num nó dedicado). Fonte: ficheiro H.264 3840×2160 a 30 fps, 16 Mbit/s, Opus, lido com `-re` e empurrado em Matroska por um cano, que é o que o browser envia. Destinos: quatro `bluenviron/mediamtx` em contentor.

| # | Medição | Resultado |
|---|---|---|
| M1 | O comando de hoje com 2 destinos: o que recebe o destino 2? | **Nada utilizável.** As opções `-c:v copy -c:a aac` do ffmpeg valem só para a saída seguinte; a segunda saída recebe o codificador por omissão do FLV — `h264 -> flv1` e `opus -> mp3` (`Stream mapping` do ffmpeg). O mediamtx fecha com `unsupported video codec: 2`. Com um só destino o defeito não se vê. Ver R230. |
| M2 | Um ffmpeg, 2 saídas correctas; o destino 2 fica sem rede (`docker pause`, pacotes deixam de ter resposta) aos 10 s | O destino **saudável** também pára: recebeu **9,1 s** de media em 50 s. Um destino pendurado congela todos. |
| M3 | O mesmo com `-f tee` e `onfail=ignore` em cada saída | O destino saudável recebeu **9,8 s** em 50 s. O `tee` isola uma saída que **fecha**, não uma que **pendura**, e não reinicia nenhuma. |
| M4 | CPU de 4 destinos a 2160p/16 Mbit/s durante 40 s (`pidstat`) | um ffmpeg com 4 saídas: **6,0 %** de um core · `tee`: **4,8 %** · **um ffmpeg por destino: 25,9 %** (quatro processos, ~6,5 % cada — cada um desmultiplexa o Matroska e codifica AAC) |
| M5 | 2160p → 1080p para um destino (`libx264 -preset veryfast -tune zerolatency`, 4,5 Mbit/s) | com `-threads 1`: 65 % de um core e **só 0,5×** o tempo real (15 fps) — não acompanha. Com `-threads 4`: **147 %** (1,47 cores) a 1,01×. |

Leitura: M2 e M3 tiram o `tee` e o processo único de cima da mesa por **isolamento**, não por custo; M4 diz que o preço do isolamento é ~6,5 % de um core por destino a 2160p, dentro do `limits.cpu: 1000m` com o tecto `MAX_DESTINOS_POR_DIRECTO` (4 por omissão); M5 diz que baixar a resolução no servidor custa **mais de um core inteiro** e não cabe no pod de base.

### Trabalho anterior que não estava na linha de backend

Nos ramos locais `frontend/l2-*` (commits `54e8cd8` e `ea6fc31`, 2026-09-16, **nunca empurrados**, sobre uma `main` anterior ao `lib.rs`) já existia «um ffmpeg por destino» com supervisor, `stderr` drenado, `-progress`, backoff e reentrada a meio do fluxo Matroska, provado contra mediamtx. Este ADR **reaproveita** essas peças (a detecção de Cluster com CRC-32, a drenagem do `stderr`, a classificação de falhas, as filas com orçamento em bytes) em vez de as reescrever, e acrescenta o que elas não tinham. A integração da linha l2 tem de ficar com o `broadcast.rs` deste ramo; a migração `0060_stream_destinations` desse ramo colide com a frente A e com a `0042` que já está na linha de backend.

## Decisão

### 1. A gravação nunca depende da rede

- O fluxo que o browser envia é escrito **primeiro no disco do nó** (`RECORDINGS_DIR`), por um escritor próprio — uma thread, uma fila limitada, `write(2)` para um ficheiro. **Não é um ffmpeg** e não partilha nada com os processos de saída: nenhum destino, nenhuma mudança de perfil e nenhuma tentativa de reposição lhe toca.
- A fila é limitada pela mesma razão do `RecWriter`: um disco que não acompanha não pode virar memória sem fim. Cheia, perde bytes — e isso **conta-se e regista-se** como evento `recording.degraded`, nunca em silêncio.
- No fim da sessão, o ficheiro entra na biblioteca (`recordings`) como gravação do servidor, com a duração lida dos Clusters e a resolução lida do cabeçalho Matroska (`PixelWidth`/`PixelHeight`).
- **O que não existe e fica dito:** não há envio para armazenamento de objectos (MinIO/S3). O `storage.rs` configura NFS/WebDAV como volume, não sobe ficheiros. «Escreve primeiro no disco e só depois sobe» é verdade para o disco; o «sobe» é o volume onde `RECORDINGS_DIR` está montado.

### 2. Um processo por destino

Pelos números M2–M4: **um ffmpeg por destino**, cada um com um supervisor. O repartidor (`escrever`) é síncrono e não espera por ninguém: põe cada pedaço na fila de cada destino e na do gravador, com orçamento em bytes. Um destino que não acompanha enche a sua fila, é morto e entra no ciclo de reposição. O custo, medido: ~6,5 % de um core por destino a 2160p.

### 3. Máquina de estados por destino

A máquina é **pura** (`delonix-meet-domain::content::live_output`), com tabela de transições testada; o supervisor do `broadcast.rs` só a executa.

| Estado (fio) | Significado |
|---|---|
| `connecting` | processo lançado, ainda não saiu nenhum byte |
| `live` | o `-progress` do ffmpeg reporta bytes a sair |
| `degraded` | no ar, mas com atraso acumulado na fila acima do limiar (ver §6) |
| `interrupted` | caiu; espera o backoff antes da tentativa `n` (`next_attempt`, `retry_at`) |
| `retrying` | tentativa `n` de `N` em curso |
| `lost` | esgotou as `N` tentativas; só volta com «reconectar à mão» |
| `stopped` | parado pela pessoa, ou porque a emissão terminou |

| De | Evento | Para | Efeito |
|---|---|---|---|
| `connecting`, `retrying` | bytes a sair | `live` | evento `destination.live` (e `destination.recovered` se vinha de uma queda) |
| `live` | atraso ≥ limiar | `degraded` | `destination.degraded` |
| `degraded` | atraso ≤ metade do limiar | `live` | `destination.recovered` |
| `connecting`, `live`, `degraded` | processo morreu / não arrancou a tempo / fila cheia | `interrupted(1)` | agenda backoff; `destination.interrupted` |
| `retrying(n)` | idem, `n < N` | `interrupted(n+1)` | agenda backoff; `destination.retry_failed` |
| `retrying(N)` | idem | `lost` | `destination.lost` |
| `interrupted(n)` | backoff cumprido | `retrying(n)` | lança o processo |
| `interrupted(n)` | «repor agora» | `retrying(n)` | cancela o backoff, lança já |
| `lost` | «reconectar à mão» | `retrying(1)` | contador a zero, lança já |
| qualquer excepto `stopped` | parar | `stopped` | mata o processo |
| `live`, `degraded` | mudar de perfil | `connecting` | reinicia o processo **sem contar tentativa** |

- **Backoff exponencial com tecto e jitter:** `min(inicial × 2^(n-1), tecto) × (1 ± jitter)`. Por omissão: 8 tentativas, 2 s inicial, tecto 15 s, jitter ±20 % (`LIVE_RETRY_MAX_ATTEMPTS`, `LIVE_RETRY_INITIAL_MS`, `LIVE_RETRY_MAX_MS`). É o «tentativa 3 de 8, a cada 15 s» do ecrã, com o jitter a impedir que N destinos da mesma sala — e N salas do mesmo nó — batam na rede no mesmo milissegundo quando ela volta.
- Um destino que fica `live` durante `stable_after` (30 s) volta a ter o contador a zero.
- **Um processo que não põe bytes a sair em `connect_timeout` conta como queda** (lição da l2: sem isto, um ffmpeg à espera de um servidor que aceita a ligação e não responde fica «a ligar» para sempre).
- «Repor agora» num destino `retrying` ou `live` é `409` com código estável; não se lança um segundo processo para a mesma chave.

### 4. Envio em diferido: não, para nenhum destino ao vivo

**O ecrã promete «os 48 segundos que os espectadores perderam são enviados em diferido». O backend não expõe essa promessa**, porque não a consegue cumprir:

- Um directo RTMP não tem backfill. Uma sessão RTMP nova começa uma nova linha de tempo; o YouTube, o Facebook e o Twitch descartam ou recusam timestamps que não sobem dentro da ingestão, e o leitor do espectador está no presente. (O comportamento de cada plataforma é o que a revisão afirma e o que a documentação pública de ingestão diz; **não foi validado contra as plataformas reais** neste trabalho — só contra mediamtx.) Enviar 48 s «antigos» numa ligação nova, no melhor caso, mostra-os como se fossem agora e empurra toda a emissão 48 s para trás; no pior, a plataforma corta a sessão.
- Acelerar o conteúdo para apanhar o tempo real (mudar a velocidade de reprodução) obriga a descodificar e voltar a codificar — M5 mostra que isso custa mais do que um core por destino.

O que é recuperado, por tipo de destino:

| Tipo (`stream_destinations.kind`) | Ao vivo, depois da reposição | Os segundos perdidos |
|---|---|---|
| `youtube`, `facebook`, `linkedin` | retoma **no presente**, no próximo Cluster Matroska (keyframe) | **não** chegam ao directo nem ao VOD da plataforma (o VOD fica com o buraco); ficam na **gravação do servidor** (§1) |
| `rtmp` (genérico) | idem | idem — não sabemos o que o servidor do outro lado faz com uma sessão nova |
| `internal` (o nosso mediamtx) | idem | idem. **Não foi provado** nenhum DVR/HLS próprio que aceite backfill; se vier a existir, prova-se e declara-se só para esse tipo |

A API diz isto em cada destino e no agregado: `live_resumes_at_present: true`, `lost_seconds_backfilled_live: false`, `lost_seconds_in_recording: true|false` (falso quando a sessão não tem gravação activa). O `scripts/check-capability-claims.sh` passa a recusar texto de interface que prometa envio em diferido/backfill do directo.

> Nota sobre os nomes: a revisão escreveu «`lost_seconds_in_recording: false` (os segundos perdidos ficam na gravação)». Os dois pedaços contradizem-se; adoptou-se o sentido da frase — os segundos perdidos **estão** na gravação, `true` — e acrescentou-se `lost_seconds_backfilled_live: false`, que é a negação que a revisão queria expor.

Os testes de integração provam isto, e não o contrário: a gravação tem os segundos da queda sem descontinuidade, e o destino volta a receber media **do presente**.

### 5. Separar as falhas

Três coisas distintas, que não partilham processo nem lock:

| Ligação | Onde vive | O que a queda da saída pública lhe faz |
|---|---|---|
| Saída pública (RTMP) | um ffmpeg por destino, supervisor, fila própria | cai, repõe-se |
| Sala e rede local (SFU/sinalização) | `sfu.rs`, `signaling.rs` | nada: o repartidor do directo nunca espera e não segura locks da sala |
| Telefone (SBC/FreeSWITCH) | `voice.rs`, fora do processo | nada: não passa pelo directo |

Os eventos dizem qual caiu (`output.lost` é da saída; a sala e o telefone têm contagens próprias, lidas das suas fontes, §7).

### 6. Adaptação automática: a métrica que existe, com o nome que tem

- O ecrã diz «perda de pacotes acima de 4 %». **Numa saída RTMP (TCP) o ffmpeg não expõe perda de pacotes** — as retransmissões do TCP escondem-na. O que se mede de verdade, por destino:
  - `backlog_ms`: media aceite para o destino e ainda não escrita no ffmpeg, convertida em tempo pelo débito de entrada;
  - `drop_ratio`: fracção dos bytes do browser que não chegaram ao ffmpeg desse destino (fila cheia, a reentrar);
  - `speed` e `total_size` do `-progress` do ffmpeg (débito de saída medido).
- `degraded` quando `backlog_ms ≥ LIVE_DEGRADED_BACKLOG_MS` (2 000 por omissão). **A UI não pode chamar a isto «perda de pacotes»**: é «a saída não acompanha».
- **Reduzir o débito automaticamente exige transcodificar** (o vídeo vem copiado). Por isso a adaptação só age quando o perfil `1080p` está disponível (§7). Sem ele, o evento diz `adaptation_unavailable` e o destino continua a ser reposto pelo ciclo normal.

### 7. Acções e perfil `1080p`

- **Repor agora** e **reconectar à mão** são a mesma rota (a máquina decide pelo estado).
- **Baixar para 1080p** muda o perfil **do destino**: o processo desse destino reinicia com `-vf scale=-2:1080 -c:v libx264 -preset veryfast -tune zerolatency -b:v 4500k`. A gravação não é um ffmpeg (§1) e não reinicia.
- M5: custa 1,47 cores por destino a partir de 2160p. **Fica desligado por omissão** (`LIVE_TRANSCODE_MAX=0`): num pod de 1 core a opção seria uma mentira que degrada a sala. Um operador on-premise com folga liga-o (`LIVE_TRANSCODE_MAX=1`, `LIVE_TRANSCODE_THREADS=4`). Com o tecto a zero ou esgotado, a rota responde `409 broadcast.transcode_unavailable` e o estado publica `profiles_available: ["source"]`, para a UI desligar o botão em vez de o mostrar a falhar.
- **Terminar a emissão e ficar só a gravar:** pára todos os destinos (`stopped`), mantém a ingestão do browser e o gravador. A sessão passa a `recording_only`.

### 8. Onde vive o estado, e o que sobrevive a um reinício

- A emissão vive no pod que recebeu o WebSocket `/api/rooms/{room_code}/live`. Esse WebSocket **não tem afinidade por sala** no ingress (só o `/ws` tem, ADR-0001) — nem precisa: a media vem do browser, não do SFU.
- **Tudo o que se mostra é escrito na base à medida que acontece**: a sessão (`live_sessions`), o último estado de cada destino (`live_session_destinations`, a cada transição) e cada evento (`live_session_events`). As leituras REST vêm da base e servem de qualquer pod; o pod da emissão acrescenta os números vivos (débito, bytes escritos) quando é ele a responder, e a base guarda um retrato a cada 5 s com `observed_at` — um retrato velho vê-se que é velho.
- **As acções** (repor, perfil, terminar) chegam ao pod da emissão pelo Redis (`RedisRoomEvent::LiveCommand`, o mesmo barramento do ADR-0001); sem Redis, executam-se localmente. São `202 Accepted`: o efeito observa-se no estado.
- **Drain ou reinício:** a memória perde-se, a base não. No arranque, as sessões abertas **deste nó** são fechadas com o evento `session.node_restarted`; o ficheiro de gravação parcial fica no disco e entra na biblioteca (é Matroska em streaming: legível até ao último Cluster completo).
- **Retenção:** o registo pertence à sessão, a sessão à gravação (`ON DELETE CASCADE`). Quando o `retention_sweep` das gravações (`organizations.retention_days`) apaga uma gravação, o registo vai com ela; sessões sem gravação seguem a mesma regra pela organização de quem emitiu.

### 9. O registo cronológico

- Cada evento: `offset_ms` (desde o início da sessão), `at`, `kind` ∈ `normal | warning | failure | safe | recovering`, `code` estável (`destination.lost`, `recording.continued`…), `title` e `detail` em português, destino opcional.
- `now` não se grava: é atribuído na leitura ao último evento `recovering` de um destino que continua em `retrying`.
- **A chave RTMP nunca entra num evento, num log, nem numa resposta.** O `detail` de uma falha vem do `stderr` do ffmpeg — que põe o URL completo, com a chave — e passa por redacção (o valor da chave e o caminho do URL) antes de ser guardado, com teste que o prova.
- É exposto por REST (sala e gravação), emitido pelo WebSocket da sala **só para anfitriões** (`broadcast_hosts`) e fica anexo à gravação.

### 10. «O que está seguro» e «quem perdeu o quê» — contagens reais ou `null` com razão

| Número | Fonte | Quando não há |
|---|---|---|
| duração gravada, GB escritos, resolução | o gravador do §1 | `null` sem gravação activa |
| faixas por fonte/voz | as faixas da gravação do SFU desta sala (`recorder`), quando está a gravar | `null` com razão |
| mensagens de chat | `room_chat_messages` desde o início da sessão | — |
| sondagens | `room:{id}:polls` no Redis (é onde vivem) | `null` sem Redis |
| perguntas (Q&A) | não são persistidas no servidor | `null`, razão `not_persisted` |
| páginas do quadro | `whiteboards` com o código da sala | — |
| espaço em disco e tempo restante | `statvfs` do `RECORDINGS_DIR` e o débito de escrita medido nos últimos 30 s | tempo `null` sem escrita |
| na sala / rede local | participantes no hub de sinalização do pod da sala | — |
| por telefone | `voice_participant` PSTN activos na sala de voz ligada ao código | — |
| por WhatsApp | frente D (canais) | `null`, razão `not_available` |
| espectadores públicos | só a API de cada plataforma os tem | `null`, razão `external` — **nunca** um número inventado |

### 11. Métricas e alertas

`delonix_live_reconnect_attempts_total`, `delonix_live_destinations_lost_total`, `delonix_live_offair_seconds_total` (segundos de destino fora do ar — o que o ecrã chamaria «segundos em diferido», que não existem), `delonix_live_buffered_bytes` (bytes em fila para ffmpegs de saída e gravador), `delonix_live_recording_dropped_bytes_total`, `delonix_node_recordings_disk_free_bytes`. Alertas e runbook: `docs/ops/emissao-resiliente.md`.

## Consequências

- **+** Uma queda de Internet custa espectadores e não custa a aula: a gravação não tem processo, rede nem destino em comum com a saída.
- **+** Um destino mau deixa de levar os outros; o defeito M1 (destinos 2..N transcodificados para FLV1) desaparece por construção.
- **+** O anfitrião vê a verdade: o que se repõe, o que se perdeu e onde está.
- **−** N processos: ~6,5 % de um core por destino a 2160p, e N ligações TCP de saída.
- **−** O ecrã tem de mudar o texto do aviso: «quando a rede voltar, a emissão retoma no presente; os segundos perdidos ficam na gravação». O botão «Baixar para 1080p» fica desligado nos nós sem orçamento de transcodificação.
- **−** A gravação do fluxo emitido ocupa disco do nó ao débito da emissão (16 Mbit/s ≈ 7,2 GB/h a 2160p). O estado publica o tempo restante e o alerta dispara antes de faltar.

## O que este ADR não decide

- DVR/HLS próprio que aceite backfill (§4) — um ADR próprio, com prova.
- Envio da gravação para armazenamento de objectos.
- Espectadores em espera por plataforma (exige OAuth às APIs do YouTube/Facebook/LinkedIn).

## Portão de aceitação

| # | Linha | Estado |
|---|---|---|
| 1 | Com a saída a cair a meio, a gravação final passa no `ffprobe` sem descontinuidade e com a duração da sessão | por medir |
| 2 | Um destino a cair não afecta outro destino da mesma emissão (bytes do outro a crescer durante a queda) | por medir |
| 3 | O destino passa por `interrupted → retrying → live` e volta a receber media do presente | por medir |
| 4 | Com o destino permanentemente em baixo, acaba em `lost` sem afectar os outros | por medir |
| 5 | Baixar um destino para 1080p não reinicia nem descontinua a gravação | por medir |
| 6 | O registo sobrevive a um reinício do servidor a meio | por medir |
| 7 | Outra organização não lê nem actua sobre a emissão | por medir |
| 8 | A chave RTMP não aparece em eventos, logs nem respostas | por medir |
