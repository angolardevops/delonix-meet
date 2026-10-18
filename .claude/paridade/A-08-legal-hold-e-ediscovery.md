---
gap: A-08
grupo: A
title: Legal hold e eDiscovery não existem — e é a porta que a soberania abre
severidade: BLOQUEIA O SEGMENTO REGULADO
estado: NÃO EXISTE
medido: 2026-10-18
base: develop @ f21e6fc47
---

Objectivo: uma organização pode suspender a retenção de um conjunto de dados por ordem legal, e um investigador autorizado encontra o que um regulador pede — sem ser administrador da base de dados.

A ironia a resolver: a soberania abre a porta da banca, dos seguros e do Estado, e é exactamente nesse segmento que a falta disto fecha a venda.

Done when:

* Com um *hold* activo, **a varredura de retenção não apaga** o que está retido. Observado de fora do código que faz a afirmação: correr a varredura com o relógio adiantado além do prazo e confirmar na **base de dados** que as linhas continuam lá; e o controlo negativo — sem o *hold*, a mesma varredura apaga
* Uma pesquisa de eDiscovery devolve o que está no âmbito **independentemente da visibilidade de cada pessoa** — que é precisamente o oposto da pesquisa normal — e **só** a quem tem a capacidade própria. Observado: a mesma consulta por um administrador comum é recusada, e por um investigador autorizado devolve. Cada acesso fica na auditoria encadeada
* A exportação sai num formato que um regulador aceita, e **a sua integridade é verificável**: manifesto com SHA-256 por item e um total, verificável depois de descarregado. Não um ZIP sem manifesto

Fora de âmbito: certificações (`A-10` — SOC 2 e ISO são auditoria de processo, não código, e não se compram com uma fatia); o DLP, que **já existe** e não se toca; a exportação pessoal de «os meus dados», que já existe e é outra coisa; retenção por classe de dados, que é a fatia seguinte.

Evidência já lida (2026-10-18, `f21e6fc47`):
- `grep -ri "legal_hold|ediscovery"` em `server/src` e `server/migrations` = **0** (só documentos)
- o que existe e é a base para construir: retenção por dias com varredura agendada (`recorder.rs:2486`, `room_chat.rs:188`, `notifications.rs:779`, agendadas em `lib.rs:2179,2248,2337`); **auditoria encadeada com verificação** (`audit.rs:163-205`, gatilhos *append-only* em `migrations/0037_*.sql:76,86`); DLP de três expressões (`dlp.rs:1-133`), aplicado à transcrição **antes** de entrar na base
- `server/src/search/global.rs:1-7` — a pesquisa global **respeita a visibilidade de cada pessoa**. É o desenho certo para o produto e **não serve** um regulador: daí precisar de um caminho próprio, não de uma bandeira neste
- `server/src/data_exports.rs:19-22` — exportação **pessoal** assinada, com `target_redacted`. Não existe equivalente ao nível da organização
- `grep -c "recording_tracks"` … (não relacionado) — mas vale a nota: este repositório já tem o hábito de pôr a capacidade na base antes das rotas (`A-02`). Aqui **não repetir isso**: a coluna de *hold* entra com o caminho que a respeita, no mesmo commit

Nota sobre a ordem: fazer o *hold* primeiro e o eDiscovery depois. O *hold* é barato (uma tabela, uma condição em cada varredura, um teste negativo por varredura) e é o que impede perda irreversível; a pesquisa é o projecto grande.

Portões em risco: `check-isolamento-cobertura.sh` (rotas novas com escopo de organização); `check-route-auth.sh`; e a capacidade nova tem de entrar no `authorization.rs` — um investigador não é um administrador, e misturar os dois é o erro clássico desta funcionalidade.
