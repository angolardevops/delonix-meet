---
gap: A-09
grupo: A
title: Dois formatos de sala que se guardam e não fazem nada
severidade: ALTO — e a dívida mais perto de virar afirmação falsa
estado: IMPLEMENTADO a 2026-10-21 — o formato `broadcast` impõe o palco no SFU; a promoção no ecrã ficou por ver no browser. Ver «Como fechou»
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

## Como fechou (2026-10-21)

**O `broadcast` passou a fazer alguma coisa:** numa sala com este formato, só quem está em palco publica, e isso é imposto **no SFU**.
- Em palco estão `Host`, `Cohost`, `Speaker` e `Broadcast`; quem entra como `Attendee` assiste (`signaling::em_palco`).
- O `hybrid` fica como estava, como o prompt pedia.

**Como funciona:**
- Cada peer do SFU tem um interruptor `on_stage`, partilhado com as publicações dele (`sfu.rs`, `Publication::off_stage`).
- Fora de palco, a bomba de RTP descarta tudo o que ele publica: nada vai para a sala, para a gravação, para a mistura do telefone ou para o selector de oradores, e ninguém lhe é subscrito.
- A publicação **fica retida** em vez de ser recusada no `on_track`. Por isso a promoção é imediata, sem o assistente renegociar nada.
- O assistente é avisado (`ServerMsg::PublishRefused { reason: "stage.not_on_stage" }`), e a métrica `delonix_sfu_publish_offstage_total` conta.
- A sinalização lê o formato da base à entrada, põe o peer fora de palco **antes** de ele poder publicar, e manda-lhe o próprio `peer-role`. O `SetRole` do anfitrião muda o palco pela porta `StageControl`, a mesma do destaque da R225.

| Critério | Estado | Observação |
|---|---|---|
| 1. Sem papel de orador, não publica media: observado no SFU, e a tentativa é **recusada**, não ignorada | **feito, com uma diferença dita** | `sfu_e2e::webinar_quem_esta_fora_de_palco_nao_chega_a_ninguem`, com media a sério: fora de palco, a Ana recebe **0 pacotes** da Bia, a Bia recebe a recusa e a métrica conta. **Controlo negativo medido:** sem o portão, a Ana recebe 173 pacotes. **A diferença:** o critério dizia «zero publicações dessa pessoa no `/metrics`». A publicação existe, retida, para a promoção ser imediata. O que o `/metrics` mostra é a contagem de retidas e zero subscrições, não zero publicações |
| 2. O palco é explícito: promover muda o que o SFU encaminha, despromover corta, nos dois sentidos | **feito no SFU e na sinalização** | O mesmo teste: promovida, a Ana passa a recebê-la sem a Bia renegociar; despromovida, deixa de chegar (zero pacotes numa janela de 2 s). `num_webinar_o_papel_decide_quem_publica_no_sfu`: o `SetRole` mexe no palco só num webinar, e só pelo anfitrião (controlo: numa reunião normal não mexe) |
| 3. O ecrã de um assistente não mostra controlos que ele não tem, nas quatro línguas | **feito, e visto no browser em metade** | Visto: o assistente tem «Watching — the host can give you the floor» no lugar do microfone e da câmara, e não tem botão de partilha; o anfitrião tem tudo. O anfitrião ganhou «Dar a palavra / Tirar a palavra» no painel de pessoas, só num webinar. **NÃO visto:** o assistente a recuperar os controlos depois de promovido. O browser embutido deixou de abrir o WebSocket do assistente pelo proxy do vite depois de várias recargas; directo à API, o mesmo socket respondia. A lógica do ecrã é `aAssistir(formato, papel)`, testada em `room/webinar.test.ts`. O limite fica escrito: sem audiência por HLS, o `broadcast` serve até ao tecto que o `A-04` mediu (40, e com o `B-12` por corrigir) |

**Fora, como o prompt dizia:** a audiência passiva por HLS, o registo e as páginas de evento, o `hybrid`.

**Achado no caminho, por decidir:** num webinar, um membro da organização que não é o anfitrião foi para a **sala de espera** («Waiting for the host to admit you»). É o comportamento que a sala já tinha; num evento talvez se queira que os assistentes entrem directamente. É decisão de produto e não se mudou aqui.

Portões:
- servidor: `cargo fmt --check`, `check-docs-drift`, `check-route-auth`, `check-arquitectura-catraca` e `check-capability-claims` verdes; 22 testes da sinalização e do SFU verdes (palco, papéis, `force_mute` e o do webinar);
- web: `tsc` 0, `vitest` com 120 ficheiros e 1465 testes, `check-frontend-lint` com 0 problemas.

