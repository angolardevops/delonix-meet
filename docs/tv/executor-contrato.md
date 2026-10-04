# Executor do Channel Engine — contrato de controlo (rascunho)

**Estado:** Proposto · **Data:** 2026-10-04 · **Contexto:** [ADR-0015](../adr/0015-channel-engine-ingest-e-distribuicao-de-tv.md) e RFC-0001 §13.
Só desenho, **sem código**. Cobre o *plano de controlo* do executor: quem tem o canal, o que escreve em cada tabela, e como recupera. **Não decide** a composição do sinal, a ingestão nem o HLS: isso depende do spike do MediaMTX, que está incompleto.

## 1. O que já existe (medido na `develop` `9fdf6f0b`)

| Peça | Onde | O que dá |
|---|---|---|
| Intenção e lease | `tv_broadcast_sessions` (0089), `tv_broadcasts.rs` | `claim`, `renew`, `report` com `fencing_token`; estados `requested → starting → live → ending → ended`, qualquer vivo → `failed`; uma só sessão por terminar por canal |
| Registo observado | `live_sessions` + `live_session_destinations` + `live_session_events` (0074, 0094) | estado por destino, eventos com `seq`; **nenhum código as escreve ainda** |
| Resiliência por destino | `domain::content::live_output` | máquina pura: `Input → (estado, Effect)`, backoff com jitter, tentativas contadas |
| Ligação | `tv_broadcast_sessions.live_session_id` (0094) | a emissão que o pedido produziu |

Falta: o **supervisor** que corre a máquina, escreve as tabelas e lança processos. É isto que este contrato descreve.

## 2. Quem escreve o quê

| Tabela / coluna | Escreve | Quando |
|---|---|---|
| `tv_broadcast_sessions.desired_state` | plano de controlo (HTTP) | `POST …/broadcasts` e `…/stop` |
| `tv_broadcast_sessions.state`, `failure_reason`, `started_at`, `ended_at` | **só o executor**, via `report` com o token | a cada transição |
| `tv_broadcast_sessions.executor_id/lease/fencing_token` | **só o executor**, via `claim`/`renew` | ao tomar e renovar |
| `live_sessions` | executor | **cria-se quando a sessão passa a `live` pela primeira vez** |
| `live_session_destinations`, `live_session_events` | executor (o supervisor de cada destino) | a cada `Input` da máquina `live_output` |

Regra: **o plano de controlo nunca escreve estado observado.** Foi o que impediu, desde a #166, que a interface mostrasse «no ar» sem prova.

## 3. Ciclo do executor

1. **Descobrir trabalho.** Cada nó corre um ciclo curto que procura sessões por terminar com `desired_state = 'live'` e sem dono válido (`executor_id IS NULL` ou lease expirado). Reclama-as com `claim` — que já é atómico, por isso dois nós a competir resolvem-se na base e **só um** recebe o token.
2. **`requested → starting`.** Com o token, `report(Starting)`. Lança o que for preciso para o sinal existir.
3. **`starting → live`.** Só quando há media a sair para pelo menos um destino. Nesse instante cria a `live_session` (`status = 'live'`, `channel_id`, `node_id`), liga-a por `live_session_id` e abre um supervisor por destino com a máquina `live_output`. **Antes disto não existe `live_session`**: um pedido que falha em `starting` acaba `failed` sem registo de emissão.
4. **Manter.** Renova o lease a cada `LEASE_SECS/3`. Cada destino cai e volta sozinho pela sua máquina; um destino em `lost` **não** derruba o canal (CA-07).
5. **Parar.** Ao ver `desired_state = 'stopped'`: `report(Ending)`, `Stop` a cada destino, fecha a `live_session` (`status = 'ended'`, `ended_at`, `end_reason`), `report(Ended)`.
6. **Falhar.** Sem fonte e sem reserva, ou erro que o executor não recupera: `report(Failed, razão)` e fecha a `live_session` como `interrupted`.

Mapeamento de estados: `requested`/`starting` → sem `live_session`; `live` → `live`; `ending` → `live` até fechar; `ended` → `ended`; `failed` → `interrupted`. O estado `recording_only` da `live_sessions` não tem equivalente no canal e **fica fora** deste contrato.

## 4. Recuperação (CA-04, CA-05, CA-08)

- **O browser do realizador não faz parte do caminho**: o pedido é uma linha na base e o executor é um processo do servidor. Fechar o browser, ou terminar uma reunião, não toca no lease (CA-04, CA-05).
- **Reinício ou queda do nó.** O lease expira (30 s sem renovação); outro nó faz `claim` e o `fencing_token` **sobe**. O executor antigo, se acordar, já não consegue `renew` nem `report` (provado nos testes da #166). O novo lê a `live_session` ligada, **não cria outra**, e relança os destinos: a máquina de cada um entra em `interrupted → retrying` com o backoff normal. Não há duplo executor activo no controlo (CA-08).
- **Sessão a decorrer sem dono.** Se `desired_state = 'stopped'` e ninguém a tem (o caso que a #166 deixou em aberto), o primeiro nó que a reclamar fecha-a (`ending → ended`) sem lançar media.

## 5. Limite que o lease não resolve

O `fencing_token` trava **escritas na base**. **Não trava o processo de saída**: um executor congelado (GC, pausa do host) pode continuar a empurrar RTMP durante até um lease, e o novo executor arrancar outro — **duas emissões para o mesmo destino**. Mitigação exigida ao executor, **não provada**:

- o executor mata os processos de saída se um `renew` falhar, ou se o último `renew` bem-sucedido tiver mais de `LEASE_SECS/2`;
- o `kill_on_drop` actual (`broadcast.rs`) cobre a morte do processo, não a pausa.

Até haver um teste com um executor suspenso (`SIGSTOP`) e um segundo a arrancar, **a garantia de «uma só emissão» é só para o plano de controlo**.

## 6. Em aberto, de propósito

1. **Actor de sistema.** `live_sessions.started_by` é `NOT NULL`; um playout sem pessoa por trás precisa de um. Decide-se com o agendador (RF-14/15); para emissões pedidas por uma pessoa, usa-se `requested_by`.
2. **Composição e fonte.** O que gera o sinal (D1) e como entra (D2) não está aqui. O contrato só exige que o executor saiba dizer «há media a sair» para chegar a `live`.
3. **Onde corre.** Um processo por nó dentro do `delonix-server`, ou um serviço próprio (RFC-0001 §6 prevê a imagem do Channel Engine). Afecta o deploy, não o contrato de estados.
4. **Admissão por capacidade.** Quantos canais por nó, medido (RNF-20). Hoje só há o tecto por contagem.

## 7. Como se prova (plano de testes)

Tudo com um `OutputRunner` falso (um *trait*), sem ffmpeg:
1. **Mapeamento puro** estado do canal ↔ estado da `live_session` (tabela de transições, testada linha a linha).
2. **Dois executores em corrida** sobre a mesma sessão: um só recebe o token.
3. **Morte do dono** a meio de `live`: o segundo reclama, **não cria segunda `live_session`**, relança os destinos.
4. **Executor obsoleto** acorda depois: nenhuma escrita passa (já existe para `report`/`renew`; estender a `live_sessions`).
5. **Paragem sem dono** (§4) fecha a sessão sem media.
6. **Um destino em `lost`** não muda o estado do canal.
7. Controlo negativo para cada um: o mutante (sem token, sem o `kill`) tem de falhar.

O que estes testes **não** provam: a emissão real. Isso exige o ffmpeg e um destino a sério (o spike) e continua por fazer.
