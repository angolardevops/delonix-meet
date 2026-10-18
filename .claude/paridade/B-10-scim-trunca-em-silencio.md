---
gap: B-10
grupo: B
title: O SCIM trunca em silêncio acima de 200 pessoas, e mente no totalResults
severidade: ALTO — e é a correcção mais barata de alto valor da lista
estado: PARCIAL (90 % feito)
medido: 2026-10-18
base: develop @ f21e6fc47
---

Objectivo: um ciclo de provisionamento do Entra ID ou do Okta numa organização de 500 pessoas lê **as 500**, em vez de ler 200 e ser informado de que são 200.

Done when:

* `GET /Users?startIndex=201&count=100` devolve o segundo bloco, e `totalResults` traz o **total real da organização**, não o tamanho da página. Observado de fora: um ciclo completo paginado soma exactamente o número de linhas que o `SELECT count(*)` da organização dá — a comparação é com a base de dados, não com a resposta anterior
* Um provedor real completa um ciclo inicial numa organização com **mais de 200** membros. Se não houver conta de Entra ID/Okta disponível, isto é um **SKIP explícito** com o que fica por provar escrito — nunca «deve funcionar»; e o substituto mínimo é um cliente que respeite o protocolo de paginação do RFC 7644, não um `curl` solto
* `GET /Groups` existe, ou a descoberta **continua a declarar honestamente que não existe**. Hoje o `resource_types` já só declara `User`, o que está correcto: se os grupos não entrarem nesta fatia, a decisão fica escrita e a descoberta fica como está — o que não pode acontecer é anunciar `Group` sem rota

Fora de âmbito: SAML 2.0 (`A-11`); o mapeamento atributo→papel, que o próprio `scim.rs:33-35` declara como decisão de produto em aberto; `ETag`, `Bulk` e `sort`, já declarados como não suportados; e o `require_mfa` que não impõe nada (`A-07`), que é vizinho de identidade mas outro defeito.

Evidência já lida (2026-10-18, `f21e6fc47`):
- `server/src/scim.rs:344-352` — `ListParams` só aceita `filter`. **`startIndex` e `count` são ignorados**
- `server/src/scim.rs:64` — `MAX_RESULTS: i64 = 200`
- `server/src/scim.rs:399-414` — `list_users` devolve `"totalResults": recursos.len()`. Numa org de 500, o Entra lê 200 e **o campo confirma-lhe que são 200**: truncatura silenciosa, não erro. É isto o bloqueador nº1
- o que JÁ está bom e não se mexe: rotas registadas (`lib.rs:456-466`, `.nest("/scim/v2")` em `:599`), token próprio `dlxs_` com prefixo exigido (`:148`), `scim_enabled` + `scim_last_seen_at` (`:160-170`), `DELETE` = suspender (`:669`), pertença delegada a `org.rs` pela regra 1 do ADR-0004, filtro `userName eq` com `invalidFilter` no envelope certo (`:356-383`), e **dez** `#[sqlx::test]` em `server/tests/scim.rs` incluindo três negativos de organização cruzada
- `server/src/org.rs` já tem o caminho de suspensão que o `DELETE`/`active:false` usa — generaliza-se, não se copia

Nota de prioridade: isto é 90 % de um SCIM a sério. A paginação é um `LIMIT/OFFSET` e um `count(*)`; o que a torna urgente é que a falha é **silenciosa e do lado do cliente** — o administrador vê o provisionamento «com sucesso» e faltam 300 pessoas.

Portões em risco: `check-isolamento-cobertura.sh` (as rotas de SCIM têm escopo de organização; qualquer rota nova tem de ser exercitada pelo `isolamento.mjs`); e o OpenAPI regenerado no mesmo commit, pela regra de não deixar as tabelas a mentir.
