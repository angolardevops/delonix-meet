---
gap: A-05
grupo: A
title: Quem vive no Outlook ou no Google Calendar copia o link à mão
severidade: ALTO — bloqueio de adopção diária
estado: NÃO EXISTE (o .ics existe e está bom)
medido: 2026-10-18
base: develop @ f21e6fc47
---

Objectivo: uma reunião criada no Delonix aparece na agenda do Outlook ou do Google de quem foi convidado, e uma alteração feita LÁ reflecte-se aqui.

Done when:

* Uma reunião criada aqui aparece no calendário ligado **sem ninguém copiar nada**, e remarcar aqui move o evento lá. Observado na agenda do fornecedor — não no nosso log de saída nem no 200 da API
* Apagar ou recusar **no fornecedor** reflecte-se aqui: o estado do convidado muda. É o sentido que distingue «mandámos um `.ics`» de «integração», e é o que tem de ter teste
* O token do fornecedor é segredo em repouso como os outros, e revogar a ligação **corta o acesso** — observado: uma chamada posterior à API do fornecedor falha com credencial inexistente, não com credencial válida e sem efeito

Fora de âmbito: CalDAV genérico (decidir se entra depois do primeiro fornecedor funcionar, não antes); o convite por `.ics`, que **já existe e está bom** e não se toca; as salas de recursos e a disponibilidade (*free/busy*), que é a fatia seguinte desta frente; o `A-06` (canais de equipa), que é o outro lado da adopção diária.

Evidência já lida (2026-10-18, `f21e6fc47`):
- `grep -ri "graph.microsoft|googleapis|caldav"` em `.rs/.ts/.tsx` = **0**. Os únicos cinco resultados da árvore são **documentos a declarar a ausência** (`docs/levantamento-2026-10-13-...:101`, `docs/competitive-positioning.md:127`)
- o que já está feito e é melhor do que parece: `server/src/mail.rs:412` — `ContentType::parse("text/calendar; charset=utf-8; method={method}")`, multipart com `text/plain` **antes** do calendário (`:725`); `server/src/meetings.rs:1929` sobe o `SEQUENCE` ao remarcar e `:2033` emite `REQUEST` com o mesmo `UID`; `server/src/notifications.rs:384-402` trata `METHOD:CANCEL` e explica que «um `CANCEL` sem o `SEQUENCE` actual é ignorado pelo Outlook»
- agenda própria em `web/src/pages/Calendar.tsx`; integração com Odoo em `server/src/odoo.rs` — **há precedente de integração externa neste repositório**, com `net_guard` e segredos em repouso
- `docs/plano-paridade-2026-10-13.md:29` afirma que o `mail.rs:466` é texto simples sem `text/calendar`: **está desactualizado**, tem

Nota de desenho: o `net_guard` (`server/src/net_guard.rs`) é o sítio por onde esta saída passa, e é também o que mantém a promessa de soberania verdadeira — uma integração de calendário é a **primeira** saída para a nuvem de um terceiro neste produto. A decisão de produto que acompanha a fatia: isto é opt-in por organização, e o ecrã diz o que sai daqui para lá.

Portões em risco: `check-route-auth.sh` (rotas de OAuth do fornecedor e de *callback*); `rotas-publicas.txt` se alguma for pública, **com razão escrita**; `check-docs-drift.sh` (variáveis de ambiente novas); e o portão de segredos.
