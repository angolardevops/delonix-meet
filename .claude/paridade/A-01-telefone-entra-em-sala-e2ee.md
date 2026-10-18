---
gap: A-01
grupo: A
title: Um telefone entra numa sala cifrada de ponta a ponta, e a ponte publica em claro
severidade: ALTO — a garantia que o ecrã dá não se cumpre
estado: ABERTO (uma sessão tentou a 2026-10-18 e terminou sem entregar nada)
medido: 2026-10-18
base: develop @ e48658f06
---

Objectivo: uma sala com `e2ee = true` recusa a entrada de um telefone, como já recusa a saída — com uma frase traduzida, não um código.

Done when:

* Numa sala com `e2ee = true`, marcar o DID e acertar o PIN **não** liga à sala: a chamada recua para a conferência local (ou desliga) e quem ligou ouve uma frase própria. A observação é o que se OUVE na chamada e o que o SFU mostra — zero publicações `PubSource::Bridge` nessa sala em `/metrics` — não a mensagem que o handler devolve
* Um teste negativo em `server/tests/ramal_entra_na_sala.rs`: PIN **certo** numa sala E2EE não devolve `room_bridge`. Medido o controlo negativo — o mesmo teste na mesma sala **sem** E2EE devolve, senão o teste passa por a sala não existir e não por a regra funcionar
* Fica escrito o que acontece a quem **já está** na ponte quando a sala liga a E2EE a meio. Hoje ninguém o define. Se a decisão for expulsar, há observação disso; se for deixar ficar, a razão está escrita no ADR — o que não pode é continuar indefinido

Fora de âmbito: salas de hardware e vídeo por SIP (`C-04`); a E2EE em codecs que não VP8, que `web/src/e2ee.ts:2` assume por desenho; o `A-07` (`require_mfa`), que é o outro achado de identidade da mesma revisão.

Evidência já lida (2026-10-18, confirmada duas vezes, a segunda por mim contra `e48658f06`):
- **O dial-OUT recusa, com razão escrita** — `server/src/dial_outs.rs:307`: `if room.e2ee { return Err(refuse("dial_out.room_e2ee", "uma sala cifrada de ponta a ponta não aceita um telefone: a ponte não decifra")) }`
- **O dial-IN não chega a olhar** — `grep -c e2ee server/src/voice.rs` = **0**. A sala resolve-se com um `SELECT id FROM rooms WHERE code = $1` em `server/src/voice.rs:1208` e `:1430`
- `server/src/signaling.rs:2346` (`join_external`) tem o mesmo lapso
- **A regra existe no produto, e é isso que prova o lapso:** a auto-gravação também recusa em sala E2EE (`server/src/recorder.rs:1750`, `auto_record AND NOT r.e2ee`). Dois dos três caminhos verificam; um não
- A ponte publica como `PubSource::Bridge` **em claro** (`server/src/sfu.rs:988`): os browsers, que esperam frames cifrados, decifram ruído; e a mistura da sala chega ao telefone pela rede da operadora, fora da garantia que o ecrã promete
- Pré-condição do cenário: **só o DID e o PIN.** Nenhuma credencial de conta

Estado da prova: **confirmado por leitura, NÃO explorado.** Ninguém fez uma chamada real a uma sala E2EE. Se houver DID de teste, reproduzir antes de fechar vale mais do que o teste unitário.

Nota de histórico, para não se repetir: uma sessão foi aberta para isto a 2026-10-18 (ramo `integra/dial-in-e2ee`) e **terminou com zero commits** — o ramo ficou igual à `develop` e não houve PR. A lacuna nunca foi tocada. Quem pegar nisto começa do zero, e não de meio caminho.

Portões em risco: o portão de i18n (a frase nova nas quatro línguas, e **não** um código cru no ecrã — ver o padrão do PR #376); `check-docs-drift.sh` se o ADR mudar; e a catraca do clippy, que o `voice.rs` conta.
