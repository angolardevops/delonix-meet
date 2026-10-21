---
gap: A-01
grupo: A
title: Um telefone entra numa sala cifrada de ponta a ponta, e a ponte publica em claro
severidade: ALTO — a garantia que o ecrã dá não se cumpre
estado: FECHADO a 2026-10-18 — ver «Como fechou» no fim
medido: 2026-10-18
base: develop @ e48658f06
---

Objectivo: uma sala com `e2ee = true` recusa a entrada de um telefone, como já recusa a saída.

Done when:

* Numa sala com `e2ee = true`, marcar o DID e acertar o PIN **não** liga à sala: a chamada recua para a conferência local (ou desliga) e quem ligou ouve uma frase própria. A observação é o que se OUVE na chamada e o que o SFU mostra — zero publicações `PubSource::Bridge` nessa sala em `/metrics` — não a mensagem que o handler devolve
* Um teste negativo em `server/tests/ramal_entra_na_sala.rs`: PIN **certo** numa sala E2EE não devolve `room_bridge`. Medido o controlo negativo — o mesmo teste na mesma sala **sem** E2EE devolve, senão o teste passa por a sala não existir e não por a regra funcionar
* ~~Fica escrito o que acontece a quem **já está** na ponte quando a sala liga a E2EE a meio.~~ **RETIRADO: o cenário não existe.** Medido a 2026-10-18: `rooms.e2ee` só se escreve no `INSERT` (`rooms.rs:165,245`) e **não há um único `UPDATE` que lhe toque** em `server/src` nem nas migrações (`0008_e2ee.sql:1` apenas adiciona a coluna, `NOT NULL DEFAULT FALSE`). Uma sala não liga a E2EE a meio. Eu escrevi este critério sem medir essa premissa; fica aqui para ninguém gastar uma sessão a resolver o que não acontece. Em troca, o que entrou foi a **visibilidade**: a recusa sai num `tracing::warn!` que nomeia a sala e a razão, no padrão dos três recuos que a função já tinha

Fora de âmbito: salas de hardware e vídeo por SIP (`C-04`); a E2EE em codecs que não VP8, que `web/src/e2ee.ts:2` assume por desenho; o `A-07` (`require_mfa`), que é o outro achado de identidade da mesma revisão.

Evidência já lida (2026-10-18, confirmada duas vezes, a segunda por mim contra `e48658f06`):
- **O dial-OUT recusa, com razão escrita** — `server/src/dial_outs.rs:307`: `if room.e2ee { return Err(refuse("dial_out.room_e2ee", "uma sala cifrada de ponta a ponta não aceita um telefone: a ponte não decifra")) }`
- **O dial-IN não chega a olhar** — `grep -c e2ee server/src/voice.rs` = **0**. A sala resolve-se com um `SELECT id FROM rooms WHERE code = $1` em `server/src/voice.rs:1208` e `:1430`
- `server/src/signaling.rs:2346` (`join_external`) tem o mesmo lapso
- **A regra existe no produto, e é isso que prova o lapso:** a auto-gravação também recusa em sala E2EE (`server/src/recorder.rs:1750`, `auto_record AND NOT r.e2ee`). Dois dos três caminhos verificam; um não
- A ponte publica como `PubSource::Bridge` **em claro** (`server/src/sfu.rs:988`): os browsers, que esperam frames cifrados, decifram ruído; e a mistura da sala chega ao telefone pela rede da operadora, fora da garantia que o ecrã promete
- Pré-condição do cenário: **só o DID e o PIN.** Nenhuma credencial de conta

Estado da prova **quando isto foi escrito**: confirmado por leitura, não explorado. Ver «Como fechou» no fim para o que passou a estar medido.

## Como fechou (2026-10-18)

Nos **dois** pontos onde a sala se resolve, e no padrão que o código já tinha:

- `voice::room_bridge_for` — o `e2ee` entra na MESMA consulta que confirma a sala (duas consultas eram duas oportunidades de alguém acrescentar uma e esquecer a outra, que é como o buraco nasceu), e com `e2ee` devolve `None` + `warn!`. O IVR cai na conferência local, que é o recuo que aquela função já usava três vezes. **Quem liga continua a ser atendido** — o PIN é válido; o que desaparece é a ponte.
- `DialInAdmission::admit` — a mesma leitura. Não é redundância: é o que apanha um `INVITE` que **nunca passou pelo IVR**, porque a allowlist de origens diz de onde um INVITE pode vir, não para que sala. Responde `404`, que é o que o UA já sabia dar.

Nada de i18n: quem liga **ouve** WAV do FreeSWITCH (`conference/conf-bad-pin.wav`), não lê chaves de tradução. O prompt original pedia «frase traduzida nas quatro línguas» e isso não se aplicava a este caminho.

Prova em `server/tests/dial_in_e2ee.rs`, contra Postgres real, com os dois sentidos medidos:

| Caso | Resultado |
|---|---|
| sala cifrada → `room_bridge` é `null` | passa |
| **a mesma sala sem `e2ee`** → a ponte vem, com `sip:room-…@ponte` | passa (controlo negativo) |
| guarda do `room_bridge_for` retirada | **falha**, e a mensagem mostra a ponte entregue: `sip:room-euc-ckmp-ngv@ponte.teste:5099` |
| guarda do `admit` retirada | **falha** |

Vizinhos sem regressão: `ramal_entra_na_sala` (8) e `dial_outs` (17) verdes.

**O que fica por provar:** ninguém fez uma chamada real. Não há DID de teste nesta máquina, e nenhum FreeSWITCH corre nos testes — o estado da prova é «confirmado por teste de integração, não por chamada».

Nota de histórico: uma sessão foi aberta para isto mais cedo a 2026-10-18 (mesmo ramo) e **terminou com zero commits**. Quem ler isto não precisa de a procurar.

Portões que isto mexeu: a catraca do clippy (o `voice.rs` conta para ela) e o `check-docs-drift.sh`. **Nenhum de i18n** — o prompt original dizia que havia, e não há: quem liga ouve WAV, não lê o ecrã.
