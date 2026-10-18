---
gap: A-09
grupo: A
title: Dois formatos de sala que se guardam e não fazem nada
severidade: ALTO — e a dívida mais perto de virar afirmação falsa
estado: NÃO EXISTE (as peças existem todas)
medido: 2026-10-18
base: develop @ f21e6fc47
---

Objectivo: o formato `broadcast` passa a COMPOR as peças que já existem num modo de evento — ou sai da lista de formatos aceites.

Esta é a lacuna com melhor relação esforço/venda da lista, porque **nada do que a compõe falta**: falta o modo.

Done when:

* Numa sala de formato `broadcast`, quem entra sem papel de orador **não publica media** — observado no SFU, não na interface: `/metrics` mostra zero publicações dessa pessoa, e o `isolamento.mjs` (ou um teste equivalente contra servidor real) prova que a tentativa de publicar é **recusada**, não ignorada
* O palco é explícito: o anfitrião promove alguém a orador e isso muda o que o SFU encaminha; despromover corta o fluxo. Medido nos dois sentidos, com controlo negativo — um assistente que tente publicar depois de despromovido é recusado
* O ecrã de um assistente **não mostra** controlos que ele não tem (microfone, câmara, partilha), e os textos existem nas quatro línguas. Se a audiência passiva (HLS) não entrar nesta fatia, o limite fica escrito: o `broadcast` serve até ao tecto de participantes que o `A-04` vier a medir, e **não** 500 ouvintes

Fora de âmbito: **a audiência passiva por HLS/LL-HLS** — é o que torna 500 ouvintes baratos, depende do `broadcast.rs`, e é uma fatia própria (`grep hls` no servidor = 0 hoje); o registo de participantes e as páginas de evento, que é produto e não media; o `hybrid`, que fica como está até o `broadcast` estar provado; e o `A-04`, de que este herda o tecto.

Evidência já lida (2026-10-18, `f21e6fc47`):
- `grep -ri webinar` em `server/` e `web/src` dá **4 ocorrências e nenhuma é código de produto**: duas fixtures de teste e duas num teste de exportador UML
- `server/src/rooms.rs:123` — `ROOM_FORMATS: &["normal", "training", "broadcast", "hybrid"]`; `server/src/meetings.rs:172` tem o equivalente. São **aceites e persistidos**
- `server/src/rooms.rs:38` — «`training` (só este permite salas de grupo)». Confirmado por grep: o `broadcast` e o `hybrid` **não são lidos para decidir nada**
- as peças, todas medidas e a funcionar: sala de espera com `WaitingPeer`/`AdmitAll` (`signaling.rs:342,1473-1578`), **palco que fixa o áudio no SFU** (`signaling.rs:1738`, `sfu.rs:1115`, R225), papel `Role::Attendee` (`signaling.rs:417`), sondagens e Q&A moderado com destaque (`room_tools.rs`, `QaSpotlight`), RTMP multi-destino (`broadcast.rs`), e o SFU que já só reencaminha 3 áudios (`sfu.rs:474`)
- `docs/competitive-positioning.md:134` admite a ausência em voz alta

Nota sobre a honestidade: hoje o `broadcast` é um valor inerte, e isso é apenas feio. No dia em que alguém o puser num comparativo ou num ecrã de criação de sala como «modo de evento», passa a ser mentira. Esta fatia fecha isso num dos dois sentidos.

Portões em risco: `check-capability-claims.sh` (não anunciar «webinar» antes das observações acima); `check-isolamento-cobertura.sh`; o portão de i18n para os textos do assistente; e o OpenAPI, se o papel entrar no contrato.
