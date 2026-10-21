# Desenho — uma peça de fila só, em vez de sete

**Data:** 2026-10-07. **Medido contra** `origin/develop` `8c0b33ec`.
**Trabalho nº2** do [levantamento](levantamento-2026-10-07-trabalho-assincrono.md).
**Regra 0 da `delonix-meet-backend`:** mapear e classificar antes de mexer. Este
documento é esse passo; **não há código antes dele**.

## 1. Os sete sítios, medidos

Todos têm **a mesma forma**:

```sql
UPDATE <tabela> SET <marcas da reivindicação>
 WHERE id IN (SELECT id FROM <tabela> WHERE <pronto?>
               ORDER BY <ordem> LIMIT <lote> FOR UPDATE SKIP LOCKED)
 RETURNING <o que o worker precisa>
```

E **nenhum partilha uma linha de código**. O que difere, e o que falta:

| Sítio | Tabela | Tentativas | Reserva | Backoff | Pílula envenenada | Justiça entre orgs |
|---|---|---|---|---|---|---|
| `webhooks::retry_due` | `webhook_deliveries` | **5** | — | **`[30,120,600,3600]` ±20 %** | **`Outcome::Permanent`** | **`row_number() PARTITION BY org_id`** |
| `transcription::claim` | `recordings` | **5** | **sim** (pedida pelo worker) | — | `transcription_failed_at` | — |
| `recorder::resume_due` (#268) | `recordings` | **3** | **sim** (3 min, renovada) | — | `is_retryable` | a jusante (`fair_slots`) |
| `data_exports::run_queue` | `data_exports` | — | **sim** (15 min, requeue no topo) | — | — | — |
| `sms::agent_claim` | `sms_message` | — | `claimed_at` + 10 min → `failed` | — | — | — |
| `sms::dispatch_operator_batch` | `sms_message` | — | idem | — | — | — |
| `sms_notify::remind_due` | `meetings` | — | marca-e-esquece | — | — | — |

**A leitura que importa:** cada coluna tem **um** dono que a fez bem e seis que
não a têm. O backoff com espalhamento existe uma vez. A justiça entre
organizações existe uma vez — e é a melhor ideia do repositório nesta matéria,
porque é a que impede uma organização com mil entregas falhadas de empurrar as
outras para trás da fila. Generalizá-la vale mais do que qualquer das outras.

**O que NENHUM tem:** parar no shutdown. Dos catorze ciclos de fundo do
`run()`, **um** tem `CancellationToken` (`run_quarantine_sweeper`) — e a
`delonix-meet-backend` já o diz pelo nome: «*não*: mais um `tokio::spawn` solto
em `run()` — já lá estão catorze».

## 2. Porque a catraca não apanhou isto

O `check-arquitectura-catraca.sh` conta **oito padrões sintácticos** (cópia de
`org_members`, `Bearer ` lido à mão, `reqwest::Client::new`, …). Sete filas com
a mesma semântica e SQL diferente não batem em nenhum deles. É o limite que a
própria skill admite: «a catraca conta padrões, não semântica».

**Consequência para este trabalho:** a prova de que a peça comum é usada **não
pode ser a catraca actual**. Tem de ser uma medida nova — contar as reivindicações
que NÃO passam pela peça — senão a oitava cópia entra sem ninguém ver.

## 3. Onde a peça vive — e o precedente que decide

A `delonix-meet-backend` manda a regra pura para `delonix-meet-domain` «no
contexto certo». **Mas isto não tem contexto certo:** atravessa `content`
(transcrição, composição), `identity` (exportações), `integration` (webhooks),
`conferencing` (lembretes) e `telephony` (SMS). Cinco dos oito.

O molde certo já existe no repositório: **`core::query`** (ADR-0007) — «a parte
PURA, sem SQL», em que cada nome de campo é um `&'static str` de uma lista
branca, e «a tradução para SQL (no adaptador de Postgres) só conhece esses
nomes e liga todos os valores por *bind*». O `core` é a camada 0 e é onde vivem
exactamente as primitivas transversais: `page`, `error`, `egress`, `edition`.

Daí a divisão em três, cada parte na camada que o `check-crate-deps.sh` permite:

| Parte | Onde | Camada | O que leva |
|---|---|---|---|
| **A especificação e a aritmética** | `delonix-meet-core::jobs` | 0 (proibido `tokio` e `sqlx`) | a forma da fila com nomes em lista branca, tecto de tentativas, tabela de backoff com espalhamento, classificação da falha, contas da reserva, chave de justiça |
| **A reivindicação** | `delonix-meet-store::jobs` | 3 (`sqlx` permitido) | **um** `claim()` que rende a forma única e liga tudo por *bind* |
| **O ciclo** | `server/src/jobs.rs` | 6 | o runner com `CancellationToken` e `JoinSet`, no molde do `quarantine_sweeper` |

Isto também avança o **passo 6 do ADR-0004** (workspace crate a crate) sem
saltar passos: não cria crate novo, põe fatias nos dois que já existem.

## 4. Ordem de migração — cada commit deixa a árvore verde

A Regra 0 exige-o, e a ordem é por **risco crescente**, não por facilidade:

1. **A peça, sem consumidor** — `core::jobs` + `store::jobs` + `jobs.rs`, com
   testes próprios. Nada muda de comportamento.
2. **`data_exports`** — o mais fácil e o que mais ganha: hoje não tem tentativas
   nenhumas e a mensagem de falha diz à pessoa «peça outra».
3. **`sms_notify::remind_due`** e as duas de `sms` — marcam-e-esquecem; ganham
   tentativas e a reserva deixa de ser um prazo fixo de dez minutos.
4. **`recording_chapters::auto_chapters_sweep`** — ganha tecto e backoff, que
   lhe faltam: hoje um `ai.timeout` repete a cada 5 min **para sempre**.
5. **`webhooks::retry_due`** — **o último, e talvez nunca.** É a implementação
   de referência: migrá-la é risco puro sem ganho de comportamento. Só se
   migra se a peça provar que faz tudo o que ela faz, incluindo a justiça entre
   organizações e o `event_id`.
6. **`recorder::resume_due`** — depois de a #268 fundir, não antes. Migrar uma
   coisa que ainda está em revisão é pedir conflito.
7. **`transcription::claim`** — a reserva é pedida pelo *worker* por gRPC, o que
   a peça comum tem de acomodar sem lhe torcer o contrato.

## 5. O que este trabalho NÃO é

- **Não é um broker.** A migração 0090 fixou a postura: «o livro de entregas já
  é a fila: não há broker, e um reinício não perde o que está agendado». Isto
  junta sete implementações do padrão da casa; não o troca.
- **Não é apagar código que funciona.** A Regra 0: «uma proposta que comece por
  apagar código que funciona é recusada na revisão». Os `webhooks` ficam como
  estão até à peça os igualar.
- **Não resolve os trabalhos 3 a 6** do levantamento (outbox transaccional,
  resumo da acta, cron do `finish_stale`, fila de exportação no servidor).

## 6. A prova a medir

| O quê | Como |
|---|---|
| A peça faz o que o `webhooks` faz | testes do `core::jobs` com os mesmos atrasos, o mesmo espalhamento e a mesma classificação |
| Dois nós não levam o mesmo trabalho | teste de integração com duas reivindicações concorrentes sobre o mesmo lote |
| Uma organização ruidosa não empurra as outras | teste da chave de justiça com 1 org a 1000 itens e 3 orgs a 1 |
| O runner pára no shutdown | teste que cancela o token a meio e mede que o ciclo sai |
| A oitava cópia não entra | **medida nova** na catraca: reivindicações que não passam pela peça |
