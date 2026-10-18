---
gap: A-04
grupo: A
title: Quarenta participantes por sala, contra 500 do Meet e 1000 do Zoom
severidade: BLOQUEIA A VENDA
estado: TECTO ARQUITECTURAL
medido: 2026-10-18
base: develop @ f21e6fc47
---

Objectivo: **primeiro saber onde a sala quebra**, e só depois escolher o caminho — porque hoje ninguém sabe, e a decisão de arquitectura sem o número é um palpite caro.

Esta fatia NÃO implementa escala. Produz a medição que decide entre as três saídas.

Done when:

* A rampa de carga continua de 40 para cima até **a sala reprovar o critério** (fluxos ≥ 98 %, perda < 2 %, jitter p95 < 30 ms, nenhuma PeerConnection falhada), e o relatório diz o degrau exacto em que reprovou e **qual recurso saturou** — núcleos, memória, largura de banda ou descritores. Medido de fora: `/metrics` e o `top` do nó, não a opinião do cliente de teste
* O custo por participante acima das 30 pessoas fica escrito como número (hoje infere-se 0,28 núcleos/participante de duas linhas de uma tabela), com a máquina e a carga do nó declaradas — a mesma disciplina do aviso do próprio repositório de que uma medição de 2026-09-17 e outra de 10-05 não se comparam entre máquinas
* As três saídas ficam avaliadas contra esse número, cada uma com o que PARTE: (a) **SFU em cascata** entre pods, que o ADR-0001 já diz onde tem o *seam* e que obriga a mover o RTP entre processos; (b) **caminho de audiência passiva** (HLS/LL-HLS a partir do `broadcast.rs`, `grep hls` no servidor = 0 hoje) — resolve 500 ouvintes e **não** resolve 120 participantes activos; (c) **aceitar o tecto** e vender 40 como limite honesto, mudando o material comercial. Uma recomendação, não três opções

Fora de âmbito: implementar qualquer das três (cada uma é um projecto com o seu ADR); o `B-06` (a entrada em série que faz 1,7 s com 40 — é vizinho e some-se a este, mas mede-se à parte); o `B-05` (a composição que rouba CPU ao media no mesmo pod, e que **contamina qualquer medição de carga se correr ao mesmo tempo** — desligar durante a rampa e dizê-lo).

Evidência já lida (2026-10-18, `f21e6fc47`):
- `docs/ops/teste-de-carga-2026-10-05.md:29` — 40 participantes, 1560/1560 fluxos, 2834 Mbps, 0,03 % / 1,25 % de perda, **11,17 de 16 núcleos**, 2929 MB, entrada a 1,7 s
- o mesmo ficheiro, `:68` — a rampa «acabou por esgotar os degraus pedidos — **não é o tecto**»; onde a sala quebra não se sabe
- `docs/ops/teste-de-carga-2026-09-17.md:69` — o tecto medido então era 30; e `:85-89` — 20 composições em paralelo levaram as chamadas vivas a 64 % de perda
- `deploy/k8s/01-config.yaml:23,27` — o deploy de referência envia `NODE_PEER_CAPACITY: "50"` e `ORG_MAX_PARTICIPANTS: "40"`
- `server/src/signaling.rs:4130,4225` — os dois tectos são opcionais e aplicam-se no `join`
- ADR-0001 — «todos os pares de uma mesma sala têm de ser servidos pelo MESMO pod», porque o RTP nunca passa pelo Redis; a sala é a unidade de *sharding*
- não há `max_participants` no SFU; o tecto é o pod

Portões em risco: nenhum de código. Mas o **material comercial** depende do resultado: se a saída for (c), o `docs/competitive-positioning.md` e qualquer página de preços têm de passar a dizer 40 — e o `check-capability-claims.sh` existe precisamente para impedir que se anuncie o que não se mede.
