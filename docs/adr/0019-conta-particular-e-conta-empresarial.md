# ADR-0019 — Conta particular e conta empresarial

**Estado:** Aceite · **Data:** 2026-10-06 · **Decisor:** o dono do produto

## Contexto

Até aqui, registar-se no Delonix Meet obrigava a **criar uma organização**: o ecrã pedia o
nome da empresa como campo obrigatório (`FormularioEntrada.tsx`, `required`) e o domínio
decidia o registo (`registration::plan`, `TenancyMode::Multi`). Quem só queria uma conta
para falar com três pessoas tinha de inventar uma empresa.

Três coisas medidas a 2026-10-06, antes de decidir:

1. **O modelo de dados já distinguia os dois casos.** `organizations.kind` existe e é
   escrita com `company` ou `personal`, e o domínio tem `OrgKind::Personal` — que não
   exige nome de empresa (gera «Espaço de ‹nome›») nem domínio de email. **Só a edição
   `Personal`/`single` o usava.**
2. **A flag `hide_org_creation` não era o meio-termo.** Quando está activa, o ecrã esconde
   o **registo inteiro** e deixa só o login. Havia dois extremos — registo com empresa
   obrigatória, ou nenhum registo — e nada no meio.
3. **O `require_corporate_domain` não recusava emails públicos**: só exigia que o domínio
   tivesse um ponto. Com isso, o primeiro a registar uma «empresa» com `@gmail.com` ficava
   com `email_domain = 'gmail.com'`, e **a partir daí qualquer outra pessoa com gmail que
   tentasse criar a sua empresa recebia `registration.domain_taken`** e a mensagem «pede ao
   teu administrador para te adicionar» — a falar de um estranho.

## Decisão

**Duas formas de conta, como o mercado as conhece: particular (Zoom) e empresarial
(Teams). A organização pode ser alojada por nós (SaaS) ou pelo cliente (self-hosting), e
isso não muda o modelo de conta.**

1. **Registo sem nome de organização → conta PARTICULAR.** Em `TenancyMode::Multi`, um
   registo sem `org_name` cria a conta e uma organização `kind = 'personal'`, sem domínio,
   chamada «Espaço de ‹nome›». Qualquer email serve, incluindo um público. **Do ponto de
   vista de quem se inscreve, não se cria organização nenhuma** — cria-se o seu espaço.
2. **A organização existe por baixo, e isso é deliberado.** Tudo o que o produto serve —
   salas, gravações, quotas, papéis, auditoria — pende de uma `org_id`. Uma conta sem
   organização nenhuma obrigaria a um segundo caminho em todas essas rotas, e é assim que
   nascem as segundas portas que a R306 fechou. O que não existe numa conta particular é
   uma **empresa**: nem directório, nem domínio, nem colegas a juntarem-se por email.
3. **Registo com nome de organização → conta EMPRESARIAL**, e exige **domínio próprio**. Um
   email público (`gmail.com`, `outlook.pt`, `icloud.com`, … — lista em
   `validation::PUBLIC_EMAIL_DOMAINS`) **nunca** pode ser o domínio de uma organização.
   Quem tenta recebe `registration.corporate_email_required` com a saída à vista: cria a
   conta sem nome de empresa e passa a empresarial quando tiver um domínio.
4. **A passagem de particular para empresarial faz-se em qualquer altura**, por
   `POST /api/orgs/{org_id}/upgrade`, e **não é um registo novo**: a conta, as salas, as
   gravações e os convites ficam onde estão. A organização muda de `kind`, ganha nome de
   empresa e passa a ter o domínio do email de quem converte.
5. **A conversão exige o mesmo que o registo empresarial, e recusa pelos mesmos códigos** —
   `registration.invalid_org_name`, `registration.corporate_email_required`,
   `registration.domain_taken` — mais `organization.already_company` quando já é empresa,
   para que um pedido repetido não mude nada em silêncio. Corre na mesma transacção e com o
   mesmo trinco do registo (`pg_advisory_xact_lock('delonix.registration')`): duas
   conversões para o mesmo domínio ao mesmo tempo não passam as duas.
6. **Quem converte é o administrador da organização** (`org.administer`). Numa conta
   particular é a própria pessoa, que é a administradora do seu espaço.

## O que isto NÃO decide

- **Não há conceito de contrato no servidor.** `contract` e `invoice` não aparecem em
  nenhum ficheiro; há `quota` e `seats`. A conversão é, hoje, **self-service**, como o
  `POST /api/orgs` já era. Se amanhã passar a depender de um contrato assinado, o sítio é
  esta rota — e o ADR que o decidir sucede a este ponto.
- **O que uma conta particular pode fazer** continua a ser o que qualquer organização pode:
  não se definiram limites próprios (salas, duração, participantes, gravação). Enquanto não
  se definirem, uma conta particular tem o mesmo que uma empresa de uma pessoa.
- **O caminho inverso** (empresa → particular) não existe, e não é simétrico: uma empresa
  com colegas, papéis e domínio não se desfaz com um `UPDATE`.
- **Quem já tem uma organização com `email_domain = 'gmail.com'`** (criada antes desta
  decisão) fica como está: a lista de domínios públicos guarda os registos NOVOS. Uma
  limpeza do histórico é trabalho à parte, e nenhum foi medido.

## Consequências

- O ecrã de registo deixa de ter o nome da empresa como campo obrigatório, e passa a
  explicar as duas escolhas.
- `registration::plan` deixa de ser a única porta do assunto: a conversão tem a sua
  (`upgrade_to_company`), com as regras no domínio e os mesmos códigos de erro.
- O teste `saas_keeps_the_historic_flow` muda de veredicto no caso «sem nome de
  organização»: era `registration.invalid_org_name`, passa a ser uma conta particular. Era
  esse erro que obrigava a inventar uma empresa.
