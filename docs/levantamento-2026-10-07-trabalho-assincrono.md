# Levantamento — o trabalho assíncrono do backend e como falha

**Data:** 2026-10-07. **Medido contra** `origin/develop` `e521b9e8`
(«Merge pull request #264 … gravações nos objectos»).
**Âmbito:** o que o servidor faz fora do pedido HTTP que o pediu, onde esse
trabalho guarda estado, e o que acontece quando o processo morre a meio.
**Não é âmbito:** a maquinaria de sessão (SFU, sinalização, presença, ponte de
telefone em chamada) — essa é in-memory por pod *de propósito* e um reinício
mata a chamada, que é o que o cliente já vê e o `drenar()` já trata.

Isto é o **passo 1 da frente 1**, antes de qualquer código.

## 0. O que existe hoje, em números

| Medida | Valor | Onde |
|---|---|---|
| `tokio::spawn`/`spawn_blocking` | **114** em 29 ficheiros (15 só no `sfu_e2e.rs`, que é teste) | `server/src/**`, `server/crates/**` |
| Ciclos de fundo no arranque (`interval`) | **14** | `lib.rs:1752–2029` |
| Desses, que param no shutdown | **1** (`run_quarantine_sweeper`) | `lib.rs:1800`, `2089` |
| Reivindicação atómica entre pods (`FOR UPDATE SKIP LOCKED`) | 6 sítios | `data_exports`, `sms`×2, `sms_notify`, `transcription`, `webhooks` |
| Abstracção de fila partilhada | **nenhuma** | não há `job.rs`/`queue.rs`; cada fluxo escreve a sua |
| Broker (Redis Streams, NATS, SQS) | **nenhum**, por desenho | migração `0090`: «o livro de entregas já é a fila: não há broker» |

**O padrão da casa é tabela + `interval` + `SKIP LOCKED`.** Está escrito em
nenhum sítio e por isso é cumprido a cinco níveis de maturidade diferentes.

## 1. Os cinco níveis de maturidade

Ordenados do melhor para o pior. **A lição é que o repo já sabe fazer isto
bem** — três fluxos são exemplares, e o trabalho da frente 1 é levar os outros
ao nível deles, não inventar mecanismo novo.

### Nível A — fila durável completa (o modelo a copiar)

| Fluxo | Estado persistente | Reivindicação | Tentativas | Recuperação |
|---|---|---|---|---|
| **Entrega de webhooks** | `webhook_deliveries` (0043/0090/0095) | `SKIP LOCKED`, `retry_at` limpo na mesma transacção que insere a tentativa seguinte | `MAX_AUTO_ATTEMPTS=5`, atrasos `[30, 120, 600, 3600]s` com espalhamento ±20% | `retry_due` a cada 15 s (`lib.rs:1920`) + varredor horário fecha as `pending` abandonadas |
| **Transcrição** | colunas `transcription_lease_*` e `transcription_attempts` em `recordings` | `SKIP LOCKED` + `lease_token` | `MAX_ATTEMPTS=5` (`domain/content/transcription.rs:8`) | reserva expirada volta à fila sozinha (`transcription.rs:27`) |
| **Destinos de directo** | `live_sessions` (0094) | — | `backoff(attempt, sample)` exponencial com tecto e jitter (`domain/content/live_output.rs:255`) | reconexão própria |

Os webhooks têm o detalhe que mais falta aos outros: **classificação da falha**
(`Outcome::Http` / `Transport` / `Permanent`) para não gastar tentativas numa
falha estável, e **`event_id`** para o receptor deduplicar um «pelo menos uma
vez».

### Nível B — fila durável sem tentativas

**Exportações «os meus dados»** (`data_exports.rs:363`). Tem tudo o que uma
fila precisa — `SKIP LOCKED`, `LEASE_SECS = 15 min`, requeue do abandonado no
topo do `run_queue`, cron a cada minuto (`lib.rs:2017`) e arranque imediato
depois do `POST` (`:187`). **Falta só uma coisa:** o `Err(e)` do `build` escreve
`status='failed'` com a mensagem *«não foi possível gerar a exportação; peça
outra»*. Não há contador de tentativas. Uma falha transitória (o Postgres a
reiniciar, disco cheio por um minuto) custa o trabalho e **o cliente repete à
mão** — é literalmente o que a mensagem lhe diz para fazer.

### Nível C — auto-cura por re-selecção, sem travão

**Capítulos automáticos** (`recording_chapters.rs:564`), cron de 5 min
(`lib.rs:1829`). Não tem fila: a consulta re-selecciona quem tem
`chapters_generated_at IS NULL`, o que dá idempotência de graça. Tem até duas
coisas boas pensadas: pára a volta inteira se o LLM estiver em baixo
(`LlmUnavailable → return`) e exclui a pílula envenenada
(`NOT EXISTS … error_code = 'ai.bad_response'`).

**O que falha:** o braço `Err(e)` e o `ai.timeout` não contam nada. Uma gravação
cujo texto faça o modelo estourar o prazo é repetida **a cada 5 min para
sempre**, sem backoff e sem tecto, a ocupar 1 das 2 vagas por volta e a
atrasar as gravações atrás dela.

### Nível D — estado persistente, recuperação só a pedido de uma pessoa

| Fluxo | Estado | Como «recupera» |
|---|---|---|
| **Legendas traduzidas** | `recording_captions.status = 'generating'` (0059) | `STALE_GENERATING_MINUTES = 30`: passados 30 min a API deixa **pedir outra vez**. Ninguém repete sozinho. |
| **Geração de capítulos a pedido** | `recording_chapter_generations` (0076) | a própria migração assume: «um trabalho `running` que deixou de dar sinal (o pod morreu a meio) é tratado como interrompido pela API a partir de `updated_at`, **sem precisar de um varredor**» |
| **Chamadas de saída / ramais** | `dial_outs`, `telephony_calls` | `finish_stale` fecha as `dialing`/`in_call` penduradas — mas **é chamado dos handlers de leitura** (`dial_outs.rs:331,500`; `telephony_calls.rs:149,196`), **não de um cron**. Se ninguém abrir o ecrã, a linha fica `in_call` a bloquear o ramal e a contar para o limite de concorrência. |
| **SMS** | `sms_message` (`queued`→`claimed`→`sent`/`failed`) | `fail_stale` a cada 60 s, `STALE_MINUTES = 10` → `failed` com *«o envio não foi confirmado; não se reenvia para não duplicar»*. **Decisão deliberada e honesta**, mas é trabalho perdido sem ninguém avisado. |

### Nível E — dispara e esquece: a falha é invisível ou o trabalho morre

Aqui está o que a frente 1 tem de resolver.

#### E1. Composição de gravações — perde-se a cada rollout

`recorder::finalize` (`:946`) é um `tokio::spawn` nu. A linha nasce em
`processing` (`insert_processing`, `:993`) — bom, a pessoa vê progresso — mas
**a linha não guarda ponteiro para o directório dos segmentos**: o
`session.dir` (`recordings_dir/tmp-{id}`, `:864`) só existe na memória da
tarefa. Depois de um reinício **nada no mundo sabe retomar aquilo**;
`fail_stale_processing` (`:1135`) só pode marcar `failed` com o texto
*«O processamento foi interrompido (o servidor reiniciou a meio)»*.

Os números medidos fazem disto uma certeza, não um risco:

| | Valor | Onde |
|---|---|---|
| Orçamento do ffmpeg | `FFMPEG_TIMEOUT_SECS` = **3600 s** | `config.rs:696` |
| Orçamento do drain | `DRAIN_READINESS_SECS` 12 + `DRAIN_GRACE_SECS` 40 = **52 s** | `config.rs:692,694` |
| SIGKILL do K8s | **60 s** | `deploy/k8s/02-server.yaml:34`, `helm/values.yaml:89` |
| Até a pessoa saber que falhou | `ffmpeg_timeout + 600` = **4200 s ≈ 70 min** | `recorder.rs:1146` |

E o `drenar()` (`lib.rs:2147`) **só espera pelos peers do WebSocket**
(`hub.peers_ligados()`). Não sabe que há uma composição a correr. Portanto:
**toda a gravação que esteja a compor durante um rollout morre**, e quem a
pediu fica a olhar para «a compor, 37 %» durante 70 minutos antes de lhe
dizerem que falhou. Não há como voltar a pedir — a reunião acabou.

Dois danos colaterais do mesmo sítio: o `remove_dir_all(&session.dir)` (`:959`)
só corre no caminho feliz, e **não há varredor de `tmp-*` órfãos** — cada
crash deixa segmentos RTP em disco para sempre; e o `kind`/nome do ficheiro
vêm de `room_rec_info` lido na tarefa, não guardados, pelo que nem o nome se
reconstrói.

#### E2. Resumo da acta pelo LLM — degrada em silêncio e nunca volta

`ai::spawn_mom_summary` (`:402`) é chamado de **um único sítio**
(`meetings.rs:1013`, ao fechar a reunião). Se o Ollama estiver em baixo ou o
pod reiniciar, sai pelo `return` com um `warn` e **fica lá**: não há coluna de
estado, não há varredor que apanhe `minutes_ai_at IS NULL`, e **não há rota
para pedir outra vez**. A ata por regras fica (o comentário diz, com razão,
«nunca se perde nada») — mas o Odoo, que lê `minutes_ai_at` para saber que o
MoM é a versão final (`apikeys.rs:756`), passa a ver para sempre uma reunião
sem ata final, sem ninguém poder corrigir.

#### E3. O primeiro registo da entrega de webhook não é transaccional

A fila dos webhooks é exemplar **a partir da primeira linha**. O problema é
nascer essa linha: `fire()` (`:120`) é um `tokio::spawn` que consulta
`org_webhooks` e só depois insere a `pending`. O código assume-o por escrito:

> «O registo não é condição do envio: se a base falhar aqui, a entrega segue
> sem linha (e fica o aviso no log).»

Entre o `COMMIT` do evento de negócio (gravação pronta, reunião começada) e o
`INSERT` na `webhook_deliveries` há uma janela em que **um SIGTERM apaga o
evento sem deixar rasto nenhum** — nem entrega, nem linha falhada, nem nada
para reenviar na consola. É o padrão *outbox* a faltar exactamente no ponto
onde o outbox serve para algo.

#### E4. Eventos de chamada num canal sem limite

`dial_outs.rs:424` abre um `mpsc::unbounded_channel` para os eventos do ESL e
gasta-os noutra tarefa. Sem tecto, um ESL verboso cresce em memória sem
contrapressão. (Dívida do mesmo tipo que a `delonix-meet-rust` persegue:
«filas limitadas».)

## 2. As quatro falhas estruturais, transversais a tudo

1. **Nenhum trabalho de fundo participa no shutdown.** 13 dos 14 ciclos não
   têm `CancellationToken` nem `JoinHandle`; o `drenar()` conta peers e mais
   nada. A forma normal de perder trabalho neste sistema é **fazer deploy**.
2. **Não há abstracção.** Cinco maturidades porque cada fluxo reinventa
   claim/lease/retry. A catraca da arquitectura
   (`check-arquitectura-catraca.sh`) conta cópia de regras e não vê isto,
   porque cada cópia tem forma diferente.
3. **A fila está no browser onde devia estar no servidor.** O `ExportsPanel`
   diz-se a si mesmo: «O que NÃO está, porque é do servidor e o servidor ainda
   não o tem: a fila de transcodificação partilhada e o seu nó/CPU»
   (`web/src/studio/exports/ExportsPanel.tsx:10`). A fila de exportação corre
   no separador, uma de cada vez; fechar o separador perde-a. O mesmo para as
   cenas (`studio/cenas.ts`) e os projectos de edição (`studio/edit/bd.ts`),
   que é o que resta do ADR-0020 — o `arquivo.ts` é explícito: «É a fila de
   espera até haver rede».
4. **O cliente é o mecanismo de retry.** «peça outra» (exportações), «pedir
   outra vez» (legendas), «volta-se a pedir à mão» (capítulos), «não se
   reenvia» (SMS). Pela regra da casa — *um passo manual no caminho do cliente
   é um bloqueio* — são quatro bloqueios.

## 3. Ordem proposta de trabalho

Por raio de dano medido, não por dificuldade.

| # | Trabalho | Prova a medir |
|---|---|---|
| 1 | **Composição retomável**: guardar `segments_dir` e o necessário na linha `recordings`; `lease`+`attempts` como a transcrição; o `drenar()` espera pela composição em curso ou larga-a *reivindicável*; varredor de `tmp-*` órfãos | reiniciar o servidor a meio de uma composição e a gravação **ficar `ready`**, não `failed`; `tmp-*` a zero depois do varredor |
| 2 | **Um `jobs.rs` só**: claim/lease/attempts/backoff/poison numa peça, com o `webhook_deliveries` como referência; migrar E2 e o nível B/C para ela | a catraca da arquitectura a contar «fluxos assíncronos sem a peça comum» e a descer |
| 3 | **Outbox transaccional** para `fire()`: a linha `pending` nasce na **mesma transacção** do evento de negócio; o worker é que a envia | matar o processo entre o commit e o envio e o evento sair **depois** do arranque |
| 4 | **Estado e rota de repetição** para o resumo da acta (E2) e para os capítulos com `ai.timeout` | uma reunião com Ollama em baixo ganhar ata final quando ele voltar, sozinha |
| 5 | **Cron para o `finish_stale`** da telefonia, hoje preso a handlers de leitura | um `dial_out` pendurado fechar sem ninguém abrir o ecrã |
| 6 | **Fila de exportação no servidor** (o que o `ExportsPanel:10` já declara em falta) e as cenas/projectos do Estúdio fora do IndexedDB | fechar o separador e a exportação continuar |

**Fora de âmbito desta frente:** a maquinaria de sessão (SFU/sinalização/
presença), que é in-memory por desenho; e o canal sem tecto do E4, que é
revisão de Rust (`delonix-meet-rust`) e não de filas.
