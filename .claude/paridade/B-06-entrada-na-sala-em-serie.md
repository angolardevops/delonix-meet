---
gap: B-06
grupo: B
title: A entrada na sala é O(n) em série, e passa de 0,3 s a 1,7 s
severidade: MÉDIO — e é a correcção mais barata da lista
estado: ATRÁS (defeito localizado, causa conhecida)
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
