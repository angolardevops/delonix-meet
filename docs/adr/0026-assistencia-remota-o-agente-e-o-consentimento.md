# ADR-0026 — Assistência remota: o agente é o consentimento, e a sessão vive no servidor

**Estado:** Proposto · **Data:** 2026-10-10 ·
**Contexto:** um técnico de IT tem de assistir um utilizador num posto gerido —
entrar no ecrã dele, mostrar o rosto de quem assiste, e falar nos dois sentidos —
sem que o utilizador tenha de entrar numa sala e partilhar o ecrã a cada vez ·
**Assenta em:** [ADR-0011](0011-chaves-de-acesso-e-sessoes.md) (chaves de acesso
e sessões) e [ADR-0008](0008-papeis-capacidades-e-ambito.md) (papéis,
capacidades e âmbito).

## Contexto

Medido na `origin/develop` (`8ad5d373`) a 2026-10-10, antes de desenhar:

1. **O controlo remoto já existe, e é um relay SEM ESTADO.**
   `ClientMsg::RemoteControl { to, action, payload }`
   (`server/src/signaling.rs:294`) é reencaminhado para o par de destino
   (`:3179`). O servidor não guarda sessão nenhuma: não sabe que há uma
   assistência em curso, quem a conduz, desde quando, nem o que já foi feito.

2. **O portão é «estar a apresentar», e só na primeira mensagem.** Um `request`
   só é entregue se `room.presenter == Some(to)` (`:3190`); as respostas passam
   sempre. O consentimento não é uma funcionalidade — é um efeito de o
   utilizador ter escolhido partilhar o ecrã.

3. **Por isso trocar de vista perde tudo.** Como não há sessão, mudar de
   separador, re-partilhar ou reconectar refaz o par e o estado de apresentador:
   o `request` tem de acontecer outra vez. Não há nada para «recuperar», e é
   esta a causa do sintoma — não um descuido do cliente.

4. **O Meet é só web.** A raiz do repositório não tem Electron nem Tauri
   (`git ls-tree origin/develop`): `server`, `web`, `voice`, `sms-gateway`,
   `whisper-server`, `ai-worker`. Não há cliente nativo em lado nenhum.

5. **O transporte do que falta já está construído.** O SFU em Rust leva a
   partilha de ecrã como track separada, simulcast, e áudio nos dois sentidos.
   O vídeo do rosto do técnico e o áudio não precisam de desenho novo: são mais
   um publicador.

6. **Uma decisão de sala é do servidor** (R7, revisor `delonix-meet-webrtc`), e
   o controlo remoto está nomeado nessa regra. O desenho abaixo respeita-a: nada
   do que decide quem vê o quê fica no cliente.

## O que o browser NÃO pode fazer, e é o que decide este ADR

O pedido é entrar no posto **sem o utilizador ter de partilhar o ecrã**. Isso
não é trabalho por fazer — é fora do alcance de uma página web, por desenho dos
browsers:

- `getDisplayMedia` exige um gesto do utilizador e um selector. Não há forma de
  uma página começar a capturar o ecrã sozinha.
- Nenhuma API web injecta rato ou teclado no sistema operativo. O controlo
  remoto de hoje só funciona porque o alvo é uma track de vídeo que o próprio
  utilizador ofereceu, e os eventos são aplicados pelo cliente dele.
- Uma notificação de sistema persistente, fora do browser, não é de um separador.

**Logo a assistência remota não é uma extensão do cliente web: é um agente
nativo instalado no posto, que usa o SFU do Meet como transporte.** Qualquer
desenho que tente o contrário entrega metade da funcionalidade e chama-lhe
pronta.

## Decisão

### 1. A instalação do agente é o consentimento

O utilizador comum não autoriza sessão a sessão. **Autoriza uma vez, quando o
posto é inscrito** — e essa inscrição é um acto explícito, com identidade de
quem inscreve, auditado, e revogável pelo dono do posto e pela organização.

É isto que torna legítimo o «entra directo» que o pedido quer: o consentimento
existe, foi dado por quem tinha autoridade para o dar, e está escrito. O que
muda face ao modelo de hoje é **quando** se pede, não **se** se pede.

Fora de um posto inscrito, nada disto existe: o caminho continua a ser o de hoje
— sala, partilha de ecrã, `request`.

### 2. O pedido por sessão é OPCIONAL, e a omissão é entrar directo

Na inscrição, o agente leva `--exigir-autorizacao` (nome provisório). Com ela, o
técnico abre a sessão e o posto mostra um pedido que o utilizador aceita ou
recusa — o comportamento de hoje. Sem ela, a sessão abre.

**A omissão é entrar directo**, como pedido. Duas condições que não se negoceiam
em troca:

- a escolha vive no **posto**, decidida na inscrição, e não num parâmetro que o
  técnico controla na altura de entrar;
- um posto só aceita sessão de técnicos da organização que o inscreveu.

### 3. A sessão existe no servidor, e é ela que sobrevive às vistas

Nasce a `AssistSession` (`asi_<16 hex>`). O nome não é `SupportSession` de
propósito: o `delonix-paas` já tem uma com esse nome (ADR 0056 de lá), que é
impersonação só-de-leitura da consola web e não tem nada a ver com um posto de
trabalho. Duas coisas diferentes com um nome só acabam numa confusão de produção.

```
id          asi_<16 hex>
posto       o posto inscrito
tecnico     a identidade REAL de quem assiste (nunca um papel genérico)
aberta_em   quando
expira_em   TTL, renovável enquanto houver actividade
estado      pedida | activa | suspensa | terminada
```

A sessão é **do servidor**, não do par. Trocar de vista, re-partilhar ou perder
a rede passa a `suspensa` e **recupera sem novo pedido** enquanto não expirar —
que é o ponto 1 do pedido, e deixa de ser um remendo no cliente para passar a
ser consequência de haver uma sessão.

Terminar é explícito: o técnico fecha, o utilizador fecha, o TTL expira, ou a
organização revoga.

### 4. O aviso é do servidor, persistente, e com nome

Enquanto houver `AssistSession` activa, o agente mostra no posto um aviso
**permanente** — não um alerta que passa — com:

- o nome real do técnico, vindo da sessão no servidor e nunca do cliente dele;
- o vídeo do rosto, se o técnico o publicar;
- áudio nos dois sentidos;
- um botão de **terminar** que o utilizador pode premir sempre, sem excepção e
  sem pedir nada a ninguém.

O vídeo e o áudio são mais um publicador no SFU que já existe (contexto 5). O
que é novo é o aviso ser **imposto pelo servidor**: um cliente que não o mostre
não recebe a sessão.

### 5. Auditoria

Abrir, suspender, recuperar, terminar e revogar são eventos de auditoria com o
técnico real, o posto e o instante. A trilha é do servidor; o agente não a
escreve nem a pode calar.

## Consequências

**O que isto custa:** um agente nativo por sistema operativo, com captura de
ecrã, injecção de input, notificação de sistema e ciclo de inscrição e
actualização. É um produto, não uma funcionalidade — e é o grosso do trabalho.

**O que isto evita:** prometer «assistência remota» em cima do cliente web e
descobrir no primeiro piloto que o utilizador tem de partilhar o ecrã à mão de
cada vez, que é exactamente o que o pedido quer eliminar.

**O que fica por decidir e não se decide aqui:** que sistemas operativos
primeiro; se o agente fala com o SFU directamente ou por um bordo próprio; e
como se distribui e actualiza. Cada um é seu ADR.

**Risco assumido, escrito para não se perder:** o valor por omissão é entrar sem
pedir. Isso é o normal em ferramentas de gestão de parque, e aqui é sustentado
pela inscrição (§1), pelo aviso permanente com nome (§4) e pelo botão de
terminar que é sempre do utilizador. Se algum dia estes postos deixarem de ser
de parque gerido e passarem a ser de inquilinos da plataforma, **esta omissão
tem de ser reavaliada** — nesse caso a autoridade é a `ngolacloud-identidade`,
cuja regra 8 exige impersonação explícita, temporária, limitada e auditada.
