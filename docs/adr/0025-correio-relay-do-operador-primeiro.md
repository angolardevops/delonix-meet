# ADR-0025 — Correio: relay do operador primeiro, SMTP por organização depois

**Estado:** Aceite · **Data:** 2026-10-09 · **Decisor:** o dono do produto

## Contexto

O servidor **não enviava correio nenhum**. Medido a 2026-10-09 na `develop`: a
única ocorrência de `smtp` em todo o `server/` era um comentário a dizer que não
enviava, e não havia `lettre` nem nada equivalente no `Cargo.toml`.

Isso travava três itens do plano de lacunas de 2026-10-04:

- **E2** — entrega dos convites, lembretes e alteração de hora;
- **E3** — reposição de password **pela própria pessoa** (a metade do
  administrador fechou no #288, sem correio: o token entrega-se à mão);
- **E7** — calendário com participantes e actualização por correio.

O plano nomeia isto como a decisão **D7**: «fornecedor de correio (SMTP próprio
do cliente, relay, ou ambos)».

## A medição que decidiu

**O `net_guard` não cobre SMTP.** O guarda de saída deste repo valida **URLs
HTTP** (`check_tenant_url`, `check_tenant_config_url`, `check_tenant_stream_url`
e as variantes de operador). Uma ligação SMTP não é um URL: é `host:porta` em
TCP puro, e nada no repo a valida.

Consequência: **um host de SMTP escolhido pelo inquilino é uma ligação de saída
arbitrária a partir dos nossos servidores.** Serve para varrer portas da rede
interna, para chegar a um serviço de metadados do provedor
(`169.254.169.254`), e para nos usar como sonda — tudo com a conversa SMTP a
dar sinal de vida ou não por cada tentativa.

Isto inverte o custo aparente das três opções do D7: o trabalho caro de
«SMTP do cliente» **não é o SMTP**, é o guarda de saída em TCP que teria de o
preceder, e que ainda não existe.

## Decisão

1. **Relay do operador primeiro.** Um só fornecedor, configurado pelo operador
   em variáveis de ambiente (`SMTP_HOST`, `SMTP_PORT`, `SMTP_USERNAME`,
   `SMTP_PASSWORD`, `SMTP_FROM`, `SMTP_STARTTLS`). Sem `SMTP_HOST` e `SMTP_FROM`
   o correio fica **desligado**, e quem tenta enfileirar recebe
   `mail.disabled` em vez de encher uma caixa de saída que ninguém esvazia.
2. **SMTP por organização fica para depois**, e só **atrás** de um guarda de
   saída que valide `host:porta` antes de abrir a ligação — com o mesmo rigor
   com que o `net_guard` trata um URL de webhook. Enquanto esse guarda não
   existir, não se aceita um host de SMTP escolhido pelo inquilino.
3. **O endereço de email passa a ser provado antes do E3.** A reposição por
   administrador (#288) entrega a conta a quem tem o **token**; a reposição por
   correio entrega-a a quem controla o **endereço**. E hoje nenhum endereço é
   provado: zero `email_verified`/`verified_at` no repo — o `users.email` é
   afirmado pelo registo ou escrito à mão por um administrador. Um
   `joao@gmai.com` mal escrito seria uma porta para uma conta que já tem dados.

## Consequências

- O E2, o E3 e o E7 destravam numa onda, sem esperar por uma peça de segurança
  nova.
- Um cliente que **exija** enviar pelo SMTP dele — comum em banca e no Estado —
  **não tem resposta nossa** até o ponto 2 existir. É o preço desta decisão e
  fica dito, não escondido.
- Todo o correio sai do nosso IP e da nossa reputação de remetente: SPF, DKIM e
  DMARC do domínio do relay passam a ser trabalho de operação.
- A caixa de saída (`mail_messages`, migração 0108) **não tem coluna de
  fornecedor**. Quando o ponto 2 entrar, acrescenta-se a referência à
  configuração da organização; o livro de entregas não muda de forma.

## Alternativas consideradas

- **Só relay, para sempre.** Mais simples e mais seguro — nunca abriríamos uma
  ligação para um host escolhido pelo inquilino. Recusada por fechar a porta a
  um requisito real de clientes grandes.
- **Só SMTP por organização.** Nada sairia até o guarda de TCP estar feito e
  provado, e o E2, o E3 e o E7 ficariam todos atrás dele. Recusada por travar
  três itens numa peça que ainda não tem desenho.
- **Ambos ao mesmo tempo.** Uma onda maior, com a espinha, o relay e o guarda
  novo no mesmo lote. Recusada por juntar numa só revisão a parte rotineira e a
  parte perigosa.

## Como se mede

- `SMTP_HOST` vazio: `mail::enabled` é falso, `enqueue` devolve `mail.disabled`,
  e as duas filas (`mail_send`, `mail_sweep`) são no-op.
- Com relay configurado: mensagem entregue numa caixa de ensaio.
- Relay em baixo: a mensagem fica `failed` com `retry_at` no futuro, e a
  tentativa seguinte sai sozinha.
- Endereço inválido: falha **sem** agendar repetição (`jobs::Failure::Permanent`).
- A reivindicação passa pela peça comum (`check-filas-reivindicacao.sh`), e a
  árvore continua com **uma só** crypto provider do rustls
  (`check-crypto-provider.sh`) — o `lettre` entra com `ring`, como o resto do repo.
