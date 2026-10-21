# ADR-0032 — Web Push para tocar com a aplicação fechada: desligado por omissão, aviso sem assunto, só serviços de push conhecidos

**Estado:** **Proposto** a 2026-10-21. As três decisões abaixo foram tomadas pelo agente que fechou o `A-03`, com a razão escrita, e **ficam por confirmar pelo dono**. ·
**Numeração:** é 0032 à data da PR; escolhe-se outra vez ao abrir, se a `develop` tiver ganho uma. ·
**Contexto:** a lacuna `A-03` da fila de paridade ([`.claude/paridade/A-03-push-com-a-app-fechada.md`](../../.claude/paridade/A-03-push-com-a-app-fechada.md)) ·
**Assenta em:** [ADR-0023](0023-push-para-acordar-o-ramal.md) (o push dos ramais móveis, que é outra coisa), e no `net_guard` (a saída de pedidos do servidor).

## Contexto

Medido na `origin/develop` @ `bbf7a5fa` a 2026-10-21, antes de desenhar:

1. **Um toque de reunião só chegava por WebSocket.** `presence::ring_users` envia pelo socket de presença. Quem não tem a aba aberta fica em `offline` na resposta do `ring` (`meetings_v1.rs:946`) e com uma chamada perdida para ver depois.
2. **Não havia Web Push em lado nenhum.** `grep -rliE "vapid|pushManager|web-push|webpush"` no código de `server`, `web`, `mobile` e `deploy` dá `exit=1`. O `web/public/sw.js` não tem handler de `push`.
3. **O push dos ramais (ADR-0023) não serve isto.** Acorda um softphone para uma chamada SIP, pelo serviço `delonix-push` ou pelo `lab`; não chega a um browser.

## Decisão

**Web Push (RFC 8030), cifrado (RFC 8291) e assinado com VAPID (RFC 8292), para o browser da pessoa, enviado a partir do `ring` a quem não tem socket.** O código está em `server/src/web_push.rs`, a migração é a `0128_web_push_subscriptions`, e o handler está no `web/public/sw.js`. Um aviso ACEITE por um serviço de push passa a pessoa de `offline` a `ringing`. Aceite não é visto: é o que o servidor pode saber, tal como quando escreve num socket.

Três decisões tomadas com isto:

### 1. Desligado por omissão

Sem `WEB_PUSH_VAPID_PRIVATE_KEY` e `WEB_PUSH_SUBJECT` nada sai, e o ecrã não oferece a funcionalidade (`GET /api/users/me/push` devolve `enabled: false`).

**Porquê:** é a **primeira saída do produto para serviços de terceiros que não foram escritos por um cliente**: o FCM da Google, o autopush da Mozilla, o WNS da Microsoft e o APNs da Apple. O aviso vai cifrado de ponta a ponta, por isso o serviço não o lê. Mas vê que houve um aviso, para que browser e a que horas. Numa instalação soberana, quem abre essa porta é o operador, com uma linha de configuração, e não o produto por omissão. Uma chave mal colada faz o servidor recusar arrancar (`web_push::validar_config`), em vez de desligar o Web Push em silêncio.

### 2. O aviso não diz o assunto nem quem chama

O aviso é `{"kind":"ring","title":"Delonix Meet","body":<texto genérico na língua da pessoa>,"room_code","link":"/#/r/<código>"}`. A função que o compõe (`web_push::aviso`) **nem recebe** o título da reunião nem o nome de quem chama.

**Porquê:** um aviso no ecrã de bloqueio é uma fuga de DLP. O título de uma reunião («Despedimento colectivo — Q4») não se mostra a quem pega no telemóvel de outra pessoa. O teste `quem_nao_tem_socket_e_avisado_e_passa_a_ringing` decifra o aviso com as chaves do browser e confirma que o título e o nome não estão lá. **O que se perde:** a pessoa não sabe que reunião é antes de abrir a aplicação. Se o dono quiser o assunto, a forma certa é uma opção por organização, desligada por omissão, e não mudar esta função.

### 3. Só serviços de push conhecidos

O endpoint de uma subscrição vem do browser, ou seja do cliente. Só se aceitam hosts de `fcm.googleapis.com`, `android.googleapis.com`, `push.services.mozilla.com`, `notify.windows.com` e `push.apple.com` (exactos ou por sufixo com ponto), e só por HTTPS. Também valem os nomes que o operador declarou em `OUTBOUND_ALLOW_HOSTS`. Por cima disto, a guarda de saída do `net_guard` corre ao registar e outra vez em cada envio.

**Porquê:** sem a lista, registar `https://intranet.cliente/…` punha o servidor a fazer POSTs para onde alguém quisesse. É um SSRF com o servidor a assinar. A guarda de saída já recusa endereços internos; a lista recusa também os públicos que não são serviços de push.

## O que fica por fazer, e porquê não está aqui

- **Tentar outra vez.** Um aviso perdido é aceitável, porque fica a chamada perdida. Uma fila de retentativas é outra fatia.
- **A condição do iOS.** O Safari em iOS só recebe Web Push (16.4+) com a PWA **instalada no ecrã principal**. O ecrã di-lo e não oferece o botão fora disso; não há forma de o contornar a partir de uma página.
- **Avisos que não sejam toques** (menções no chat, gravação pronta). O mecanismo serve, mas cada um precisa da sua decisão de DLP.

## O que não está provado

**Nenhuma notificação foi vista num aparelho real com o browser fechado.** Os testes provam:
- a rota;
- a cifra, decifrando o aviso com as chaves do browser;
- a assinatura VAPID com a chave pública anunciada;
- a passagem de `offline` a `ringing`;
- a limpeza num 410.

Contra um serviço de push falso. O critério 1 do `A-03`, a notificação no ecrã com o browser fechado, observa-se num aparelho, com a chave VAPID configurada numa instalação alcançável por HTTPS.
