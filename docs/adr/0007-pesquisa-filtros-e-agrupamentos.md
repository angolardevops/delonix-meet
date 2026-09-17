# ADR-0007 — Pesquisa, filtros e agrupamentos

**Estado:** Proposto · **Data:** 2026-09-17 · **Estende:** [ADR-0004 §4](0004-organizacao-alvo-do-backend.md) (listagens) e [ADR-0006 §1/§3](0006-backend-enterprise-contextos-edicoes-e-entrega.md)
**Contrato:** [`docs/reference/pesquisa.md`](../reference/pesquisa.md)
**Pedido que o origina (dono do produto, 2026-09-17, resumido):** «Ctrl+K faz uma busca
completa, não se limita ao menu, como a pesquisa profunda do Odoo, em qualquer ecrã. Todas
as listas com paginação e filtro de pesquisa, filtros e agrupamentos avançados inspirados
no Odoo. É o diferencial face aos concorrentes: usar todo o potencial do PostgreSQL.»

## Contexto

Medido a 2026-09-17 na linha `delonix-meet-backend/g10-media-nodes` (`33f26f2`):

1. **Só uma listagem pesquisa.** `GET /api/recordings?q=` usa `recordings.search_vector`
   (`tsvector` gerado, configuração `simple`, GIN, migração 0045) com prefixos. Não tolera
   acentos (`reuniao` não encontra `reunião`) nem erros de escrita.
2. **As outras listagens não têm limite nem filtro:** `meetings::list`, `whiteboards::list`
   (`LIMIT 200` silencioso), `org::list_employees`, `audit::list` (`limit` até 500, sem
   cursor). Nenhuma agrupa.
3. **A visibilidade está espalhada em SQL escrito à mão por handler**
   (`recordings::LIBRARY_VISIBLE`, `rooms::room_access`, `users::search`). Uma pesquisa
   global que reescrevesse essas regras seria a 4.ª cópia de cada uma — e a primeira a
   divergir seria uma fuga entre organizações.
4. **A imagem `postgres:17-alpine`** (dev, CI e `deploy/`) traz `unaccent` 1.1 e `pg_trgm`
   1.6. As duas são *trusted* desde o PG 13: o dono da base cria-as sem superutilizador.

## Decisão

### 1. Três superfícies, um motor

| Superfície | Rota | Para quê |
|---|---|---|
| Pesquisa global (Ctrl+K) | `GET /api/search` | procurar em todos os tipos de uma vez, agrupado por tipo |
| Pesquisa de lista (estilo Odoo) | parâmetros uniformes na rota de colecção de cada recurso | `q` + `filter` + `filters` + `group_by` + `order_by` + keyset |
| Descrição | `GET /api/search/schemas[/{resource}]` | a UI constrói o painel sem conhecer campos à mão |
| Favoritos | `/api/users/me/saved-searches[/{saved_search_id}]` | pesquisas guardadas por pessoa, opcionalmente partilhadas na org |

A pesquisa de lista **não é uma rota nova por recurso**: são parâmetros na colecção que já
existe. Sem nenhum deles, a colecção herdada mantém a forma antiga (o mesmo mecanismo do
`GET /api/recordings` desde a 0045); com qualquer um, responde o envelope novo.

### 2. O filtro é uma árvore tipada contra uma lista branca — nunca SQL

- O cliente manda `filter` como JSON: condições `[campo, operador, valor]` combinadas com
  `{"and": […]}`, `{"or": […]}`, `{"not": …}` (lista no topo = AND). Profundidade ≤ 4,
  ≤ 20 condições, ≤ 4 KiB.
- Cada recurso declara uma **lista branca** de campos (`SearchSchema`): tipo, operadores,
  se filtra, ordena, agrupa e que agregados tem. Campo ou operador fora dela → `400` com
  código estável (`search.unknown_field`, `search.invalid_operator`, …).
- A tradução para SQL usa **só** expressões escritas no código para cada campo da lista
  branca; todo o valor vindo do cliente entra por *bind* (`QueryBuilder::push_bind`).
  Nenhum `format!` de SQL com input. O teste de injecção tenta nomes de campo e valores
  hostis e verifica que o SQL gerado não os contém.
- Os filtros pré-definidos («As minhas», «Esta semana», «Com transcrição») são árvores
  declaradas no schema e passam pela MESMA validação. Filtros do mesmo grupo combinam com
  OR e grupos diferentes com AND — como no painel do Odoo.

### 3. Onde vive cada parte (ADR-0006 §1)

| Parte | Sítio | Porquê |
|---|---|---|
| AST do filtro, operadores por tipo, validação de `group_by`/`order_by`, cursor keyset (sobre `core::page`) | `delonix-meet-core::query` | puro, sem IO; reutiliza o `page` — um só cursor opaco e um só limite (1..100) |
| Schema de pesquisa de cada recurso (campos, filtros, agrupamentos) | contexto de domínio do recurso (`content`, `scheduling`, `organization`, `compliance`) | é linguagem do domínio: «Com transcrição» é uma regra de gravações, não de SQL |
| Tradução para SQL (tsvector, unaccent, pg_trgm, keyset, `GROUP BY`) e a visibilidade por recurso | monólito, `server/src/search/` (futuro `delonix-meet-store`) | é o único sítio com `sqlx`; `check-crate-deps.sh` proíbe-o no core e no domínio |
| Handlers | `server/src/search/` (global, schemas, favoritos) e os handlers de colecção existentes (um ramo de poucas linhas) | a rota de colecção é a mesma |

**A visibilidade não se reescreve — mas escreve-se em semi-junção.** Com ≥ 100 k linhas, a
forma «EXISTS por linha» (`m.owner_id = me OR convidado`) percorre a tabela inteira (1,3 s
medidos nas reuniões); a mesma regra escrita como `id IN (as minhas ∪ onde fui convidado)`
parte de quem pede (0,6 ms). Onde a forma muda, um teste compara pessoa a pessoa com o
endpoint herdado (`recordings_visibility_matches_the_library`).

**A visibilidade não se reescreve.** Cada recurso tem UMA função SQL de visibilidade, e a
lista, o Ctrl+K e o endpoint por id concordam — o teste de isolamento prova-o por recurso
e o `web/e2e/isolamento.mjs` fá-lo contra servidor real. O `org_id` vem do caminho (com
`require_member`/`require_admin`) ou da pertença activa de quem pede; nunca do corpo nem
de um parâmetro de pesquisa.

### 4. Texto: `simple` + `unaccent` para todas as línguas, trigramas para o resto

- **Configuração `dlx_search`** = cópia da `simple` com o dicionário `unaccent` antes do
  `simple`. Porquê não `portuguese`: as reuniões e as transcrições saem em pt, en e fr na
  mesma organização (a 0045 já o mediu), e o stemmer português estraga as outras —
  «meetings» deixa de se encontrar a si próprio. O custo (`reunião` não encontra
  `reuniões`) paga-se com prefixos (`reuni:*`) enquanto se escreve.
- **Chinês (zh)** não tem espaços nem stemming: o parser junta um bloco de caracteres Han
  num só token, e o `tsvector` só acerta pelo início do bloco. Para zh (e para códigos,
  emails e nomes) o que funciona é **trigrama** (`pg_trgm`) sobre `dlx_unaccent(lower(…))`:
  subcadeias e erros de escrita (`word_similarity`). O índice trigrama só serve para
  termos ≥ 3 caracteres; um termo CJK de 2 caracteres é correcto mas não indexado — está
  escrito no contrato.
- **`unaccent()` não é `IMMUTABLE`**, por isso nem coluna gerada nem índice a aceitam. A
  0115 cria `dlx_unaccent(text)` IMMUTABLE com o dicionário fixado
  (`public.unaccent('public.unaccent'::regdictionary, $1)`), que é o padrão documentado.
- **Gravações alinham com a 0045:** a coluna `recordings.search_vector` e o índice
  `idx_recordings_search` mantêm nome e papel; só a configuração passa de `simple` para
  `dlx_search` (a coluna é gerada, por isso sai e volta na mesma migração). As duas
  consultas do `recordings::library` passam a `dlx_search`. Não há segunda coluna nem
  segundo índice.
- **Erros de escrita só como recurso.** A pesquisa exacta (prefixos + subcadeias) corre
  primeiro; a aproximada (`word_similarity ≥ 0.4`) só quando a exacta não encontra nada, e a
  resposta diz `text_match: fuzzy`. Medido com 200 k gravações: com as duas juntas,
  «orcamento» trazia «planeamento» (semelhança 0,45).
- Relevância: `ts_rank_cd` com pesos (A título/nome, B transcrição/descrição, C
  capítulos/comentários) somado a `word_similarity` do trigrama; trecho com `ts_headline`
  **só nas linhas da página devolvida** (relê o texto inteiro — antes do `LIMIT` seria por
  cada candidata, a lição da 0045).

### 5. Paginação keyset e o envelope

- `page_size` + `page_token` são os do `core::page` (50 por omissão, 100 máximo, token
  opaco). O cursor leva os valores das chaves de ordenação **e o `id`** como desempate
  final, e uma impressão digital da pesquisa: um token usado com outra `q`/`filter`/ordem
  dá `400 search.page_token_mismatch` em vez de uma página errada em silêncio. Sem
  `OFFSET`: a página 2000 custa o mesmo que a primeira.
- Todas as expressões ordenáveis são não-nulas (`COALESCE` declarado no schema), senão a
  comparação de linha salta registos.
- **Extensão compatível do `Page<T>`**: `{items, next_page_token}` mantém-se tal e qual;
  acrescentam-se `total` e `total_kind` e, com `group_by`, `groups`. Um cliente que só lê
  `items`/`next_page_token` não muda. Porquê `total` apesar do cursor: o painel do Odoo
  mostra «1-80 / 12 345» e a UI pediu-o; conta-se exacto até 10 000 (`total_kind: "exact"`)
  e acima disso devolve `10000` com `total_kind: "at_least"` — nunca um número inventado.
- **Agrupamento preguiçoso (como o `read_group(lazy=True)` do Odoo):** só o primeiro campo
  de `group_by` é agregado; cada grupo traz o `filter` a juntar e o `group_by` restante para
  a UI o expandir, com paginação própria (`groups_page_token`). Datas por
  `day|week|month|quarter|year` **no fuso da organização** (`organizations.timezone`, nova,
  omissão `Africa/Luanda`), porque «esta semana» em Luanda não começa à mesma hora UTC.

### 6. Favoritos

Tabela `saved_searches` (dono, org derivada da pertença activa, recurso, nome, consulta em
JSONB validada contra o schema ao gravar, `shared`, `is_default`). Partilhada = visível aos
membros **activos** da mesma organização; só o dono altera ou apaga. Uma consulta guardada
que deixe de validar (campo removido do schema) volta com `valid: false` e o código do erro,
em vez de desaparecer.

## Consequências

- **+** Uma listagem nova ganha pesquisa, filtros e agrupamentos declarando um schema e uma
  função de visibilidade — não um handler.
- **+** Nenhum SQL do cliente; a lista branca é testável sem base de dados.
- **+** O Ctrl+K não inventa regras de acesso: chama as mesmas funções das listas.
- **−** Índices GIN e trigramas custam escrita e disco (medido no relatório da
  implementação com ≥ 100 k linhas).
- **−** `simple`+`unaccent` não faz stemming: «reuniões» ≠ «reunião» sem prefixo.
- **−** Agregar por grupo percorre o conjunto filtrado inteiro; acima de ~1 M de linhas por
  organização o `group_by` precisa de uma vista materializada — fica medido, não prometido.
- **Fora deste ADR:** pesquisa semântica (embeddings/pgvector), pesquisa em legendas e
  segmentos com tempo enquanto `recording_captions` e `transcript_segments` não chegarem à
  linha do backend (estão só na linha da UI), e a v1 pública (a pesquisa nasce na BFF).
