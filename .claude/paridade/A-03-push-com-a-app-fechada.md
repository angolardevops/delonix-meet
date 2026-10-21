---
gap: A-03
grupo: A
title: Ninguém é avisado de uma reunião sem ter a aba do Delonix aberta
severidade: BLOQUEIA A ADOPÇÃO
estado: IMPLEMENTADO a 2026-10-21 — critérios 2 e 3 provados; o 1 (a notificação num aparelho real) fica por observar. Ver «Como fechou»
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

## Como fechou (2026-10-21)

Web Push (RFC 8030), cifrado (RFC 8291) e assinado com VAPID (RFC 8292), enviado no `presence::ring_users` a quem não tem socket. Módulo `server/src/web_push.rs`, migração 0128, `web/public/sw.js` e um painel nas definições da conta. As três decisões de desenho estão no [ADR-0032](../../docs/adr/0032-web-push-para-tocar-com-a-aplicacao-fechada.md), em **Proposto**, à espera do dono:
- desligado por omissão;
- o aviso não leva o assunto nem quem chama;
- só serviços de push conhecidos.

| Critério | Estado | Observação |
|---|---|---|
| 1. A notificação aparece no ecrã com o browser **fechado** | **NÃO OBSERVADO** | Precisa de um aparelho real, de uma instalação por HTTPS e da chave VAPID configurada. O browser embutido desta sessão nega notificações por política (`Notification.permission = denied`): o painel mostrou correctamente o estado «bloqueado», e com a permissão simulada mostrou «ligar» e explicou a recusa sem registar nada (só `GET /api/users/me/push` na rede). **Como observar:** configurar `WEB_PUSH_VAPID_PRIVATE_KEY` e `WEB_PUSH_SUBJECT` (receita no `docs/deployment.md`, testada nesta sessão: o servidor arrancou com a chave gerada), ligar os avisos nas definições num Chrome ou Android, fechar o browser e chamar a pessoa para uma reunião |
| 2. Sem socket, o uuid passa de `offline` a `ringing`, com teste de socket FECHADO | **feito** | `tests/web_push.rs::quem_nao_tem_socket_e_avisado_e_passa_a_ringing`. O serviço de push falso recebe UM aviso com `Content-Encoding: aes128gcm`, `Urgency: high`, `TTL: 120` e VAPID com a chave anunciada. **Decifrado com as chaves do browser:** leva o link da sala e não leva o assunto nem o nome de quem chama. **Controlo negativo dentro do teste:** antes de subscrever, `offline`. **Controlo negativo medido:** com o envio desligado no `ring`, o teste falha |
| 3. Um endpoint morto (404/410) sai da tabela | **feito** | `um_endpoint_morto_sai_da_tabela`: um 410 retira a subscrição (de 1 para 0) e a pessoa fica `offline`. **Controlo:** um 503 conta como falha e a subscrição FICA |

Também provado: o endpoint só pode ser de um serviço de push conhecido (`o_endpoint_tem_de_ser_de_um_servico_de_push`, com IP interno, host de intranet e HTTP recusados, e a Google aceite). Um browser partilhado passa a avisar quem subscreveu por último, e ninguém retira a subscrição de outra pessoa (`o_mesmo_browser_passa_para_quem_subscreveu_por_ultimo`). O logout retira a subscrição deste browser.

Fica de fora, como o prompt dizia: as retentativas, o cliente nativo (`B-01`) e os avisos que não sejam toques.

**Achado no caminho, por corrigir noutra fatia:** com a aplicação ABERTA mas a aba escondida, o `PresenceProvider` mostra uma notificação com o **assunto** da reunião (`e.title`). É a mesma questão de DLP que o ADR-0032 §2 resolve para o Web Push.

Portões:
- servidor: `--test web_push` 5/5, unitários do `web_push` 3/3, `cargo fmt --check`, `check-openapi` (441/441), `check-docs-drift`, `check-route-auth`, `check-arquitectura-catraca`, `check-capability-claims`, `check-isolamento-cobertura` e `check-dep-audit`, todos verdes;
- web: `tsc` 0, `vitest` com 120 ficheiros e 1469 testes, `check-frontend-lint` com 0 problemas, `vite build` verde.

