---
gap: A-03
grupo: A
title: Ninguém é avisado de uma reunião sem ter a aba do Delonix aberta
severidade: BLOQUEIA A ADOPÇÃO
estado: NÃO EXISTE
medido: 2026-10-18
base: develop @ f21e6fc47
---

Objectivo: uma pessoa com o Delonix FECHADO recebe o aviso de que a reunião começou, no telemóvel e no desktop.

Done when:

* Com o browser fechado (não minimizado — **fechado**), um `POST /api/v1/meetings/{id}/ring` faz aparecer uma notificação do sistema no aparelho alvo. A observação é **a notificação no ecrã**, não o 200 da rota nem uma linha de log: gravar o ecrã ou fotografar, e dizer qual o aparelho, browser e versão
* O `GET` que hoje devolve `{ringing: [], offline: [uuid]}` para quem não tem socket passa a devolver esse uuid em `ringing` — e há um teste que o prova com o socket FECHADO, não aberto
* Um aparelho que revogou a permissão, ou cujo *endpoint* o servidor de push rejeitou com 404/410, é **removido** da tabela em vez de acumular falhas: teste negativo com um endpoint morto, e a contagem de aparelhos desce

Fora de âmbito: o cliente móvel nativo (`B-01` — isto resolve-se no PWA, que é o caminho real); o chat e as menções (só o toque de reunião nesta fatia); a entrega garantida e as tentativas (uma notificação perdida é aceitável, uma fila de retentativas é outra sessão).

Evidência já lida (2026-10-18, `f21e6fc47`):
- `grep -ri "vapid|pushManager|web-push"` em `.rs/.ts/.tsx/.js/.toml/.sql`, fora de `node_modules` e `dist` = **0 resultados em todo o repositório**
- `web/public/sw.js` — 132 linhas, **sem** `addEventListener('push')` e sem `notificationclick`
- `server/src/notifications.rs:21-24` — a notificação é «empurrada pelo `PresenceHub::notify`», ou seja só por WebSocket
- `server/src/meetings_v1.rs:946` — devolve literalmente `offline: [uuid]` para quem não tem socket aberto
- `server/src/voice_devices.rs` (1013 linhas, migração 0105) **já tem** registo de aparelho com token FCM/APNs — a tabela existe, o envio não
- `server/tests/voice_push_wake.rs:368` — «Um aparelho FCM: aceita-se o registo, mas o fornecedor ainda não envia nada — **não finge**». Só o provider `delonix` está ligado, e **só para acordar SIP**

Notas de desenho, porque a decisão está aberta:
- Web Push (VAPID) cobre Chrome, Edge, Firefox e **Safari 16.4+ em iOS, mas só se a PWA estiver instalada no ecrã inicial**. Isso é uma condição de produto, não um detalhe: se o caminho for Web Push, o ecrã tem de pedir a instalação e **dizer porquê**, senão o iOS fica de fora em silêncio — que é exactamente o modo de falha que este repositório não tolera
- A chave VAPID é um segredo: entra pelo `secrets_at_rest` como os outros, nunca no `config.example.yml`
- O corpo da notificação **não** leva assunto da reunião por omissão: um aviso no ecrã de bloqueio é uma fuga de DLP. Ver `dlp.rs` e decidir, com a decisão escrita

Portões em risco: `check-route-auth.sh` (a rota de subscrição é nova e precisa de extractor ou de linha no `rotas-publicas.txt` COM razão); `check-capability-claims.sh` (não anunciar «notificações push» em locale nenhum antes de a observação acima existir); e o portão de i18n — a frase da permissão nas quatro línguas.
