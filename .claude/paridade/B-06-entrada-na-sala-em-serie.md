---
gap: B-06
grupo: B
title: A entrada na sala é O(n) em série, e passa de 0,3 s a 1,7 s
severidade: MÉDIO — e é a correcção mais barata da lista
estado: FECHADO a 2026-10-21 — a premissa era um artefacto do gerador, e o laço em série medido não é a causa. Ver «Como fechou»
medido: 2026-10-18
base: develop @ f21e6fc47
---

Objectivo: entrar numa sala com 40 pessoas custa o que custa entrar numa com 8.

Done when:

* O `join_ms` medido pelo arnês que **já existe** fica plano entre 8 e 40 participantes — ou a subida fica justificada por outra causa nomeada. A observação vem de `web/e2e/tempos.mjs`, que grava em `call_timings` e expõe `delonix_join_total`, não de um cronómetro novo escrito para a ocasião
* O `reevaluate_room` deixa de percorrer os pares em série com `.await` dentro do laço, e a correcção **não introduz um lock atravessado por um `.await`** — que é a armadilha óbvia desta mudança neste código. Revisto pelo `delonix-meet-rust` com essa pergunta explícita
* Nenhuma regressão de correcção: a bateria do servidor verde, e em particular os testes que dependem da ordem de reavaliação dos pares (se existirem, nomeá-los; se não existirem, **dizer que não existem** em vez de deixar supor cobertura)

Fora de âmbito: a escala da sala (`A-04` — isto não sobe o tecto de 40, torna os 40 aceitáveis); a composição que rouba CPU (`B-05`); o simulcast do ecrã, declarado em falta no `webrtc.ts:172-174`.

Evidência já lida (2026-10-18, `f21e6fc47`):
- `server/src/sfu.rs:1689-1703` — `reevaluate_room` percorre os pares **em série com `.await`**
- `docs/levantamento-2026-10-13-concorrencia-escala-stack.md:239` — a consequência medida: a entrada sobe de **0,3 s com 8 pessoas para 1,7 s com 40**
- `docs/ops/teste-de-carga-2026-10-05.md:29` — confirma 1,7 s no degrau de 40
- `web/e2e/tempos.mjs:85-106` — o arnês já mede `join_ms`, `ws_ms`, `first_audio_ms`, `ice_restarts`, `reconnects`, com valores absurdos presos (`:101`). **Não é preciso instrumentar nada**
- `[CG]` o Meet entra em menos de 1 s; é a referência prática

Nota de prioridade: de todas as linhas em que estamos atrás, esta é a que tem **causa localizada, medição pronta e âmbito pequeno**. É a primeira a fazer se o objectivo for ganhar credibilidade rápido numa demonstração com 30 ou 40 pessoas.

Portões em risco: a catraca do clippy (`sfu.rs` conta para ela); e o portão de arquitectura, se a correcção mover código entre camadas.

## Como fechou (2026-10-21) — sem alteração ao SFU

Medido. O relatório completo está em [`docs/ops/teste-de-carga-2026-10-21-entrada.md`](../../docs/ops/teste-de-carga-2026-10-21-entrada.md).

1. **A premissa deste prompt era um artefacto.** O «0,3 s com 8 / 1,7 s com 40» é o `entrada_s` do loadgen: o tempo que o GERADOR leva a lançar os clientes, com 40 ms entre cada um (8 × 40 ms = 0,32 s, 40 × 40 ms = 1,6 s). Não é o tempo de entrada de ninguém. O tempo que uma pessoa espera até ver alguém é o `primeiro_video_ms`.
2. **O laço em série não é a causa medida.** O A/B foi intercalado (base, `join_all`, base, `join_all`), com os binários conferidos por sha256:
   - até 16 pessoas, o primeiro vídeo chega em ~70–90 ms, nas duas variantes;
   - aos 40, a variante concorrente **piora** (p95 de 5,4 s e 6,0 s, contra 4,0 s e 3,0 s da base), com a máquina a 92–98 %;
   - a subida acompanha a saturação da máquina, não o número de reavaliações em série.

   A alteração não foi entregue.
3. **Uma armadilha nova, documentada no `loadgen.rs` e no `loadgen-rampa.sh`.** Desde 2026-10-05, o `MAX_WS_PER_USER` (16) corta a rampa no 17.º cliente com 429, porque o gerador usa uma só conta. Sem `MAX_WS_PER_USER=0`, um degrau de 40 parece colapsar aos 24.

Critérios do prompt:

| Critério | Estado |
|---|---|
| O tempo de entrada fica plano entre 8 e 40, **ou a subida fica justificada por outra causa nomeada** | **Justificada:** é plana até 16; acima disso acompanha a saturação da máquina (servidor + gerador + carga alheia). Separar o servidor do gerador precisa de um gerador noutra máquina, e isso é trabalho do `A-04` |
| Sem lock atravessado por `.await` | Não se aplica: nada mudou no SFU. A análise feita para a variante (ordem `peer.subscribed` → `publication.subscribers`, sem caminho inverso) está no relatório |
| Sem regressão | Não se aplica: nada mudou no SFU |

**O que fica por saber, e passa para o `A-04`:** quanto da subida aos 32–40 é do servidor, com um gerador fora da máquina e o host limpo. E um desenho diferente, reavaliar só os peers que uma entrada afecta em vez da sala inteira, não foi medido.

