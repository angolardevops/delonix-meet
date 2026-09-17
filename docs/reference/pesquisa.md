# Pesquisa, filtros e agrupamentos — contrato

> **Autoridade:** [ADR-0007](../adr/0007-pesquisa-filtros-e-agrupamentos.md). Superfície: BFF
> (`/api`, sessão). Envelope de erro plano do ADR-0006 §3; paginação do `core::page`.
> Os caminhos usam os nomes da reorganização de 2026-09-16
> ([`api-routes.md`](api-routes.md)); onde a rota herdada ainda tem outro nome, vai entre
> parênteses.
>
> **Estado de cada peça:** «implementado» = servido e provado nesta linha
> (`server/tests/search.rs`, `web/e2e/isolamento.mjs`); «fase 2» = mesmo mecanismo, a seguir.
> **Caminhos:** as colecções respondem HOJE nos caminhos herdados (`/api/orgs/{org_id}/employees`,
> `/api/orgs/{org_id}/audit`); mudam para `members`/`audit-events` com a reorganização de rotas,
> sem alteração dos parâmetros nem da resposta.

## 0. Resumo para a UI

| Precisas de… | Chamas |
|---|---|
| Ctrl+K | `GET /api/search?q=orc&types=meetings,recordings&limit=5` |
| Montar o painel de uma lista | `GET /api/search/schemas/recordings` (uma vez por ecrã; cacheável) |
| A lista com pesquisa/filtros/agrupamento | `GET /api/recordings?q=…&filter=…&filters=mine,this_week&group_by=created_at:month&order_by=-created_at&page_size=50` |
| Abrir um grupo | a mesma lista com o `filter` que o grupo trouxe e o `group_by` restante |
| Favoritos | `GET/POST /api/users/me/saved-searches`, `GET/PATCH/DELETE /api/users/me/saved-searches/{saved_search_id}` |

As páginas e acções da app (menu, «Nova reunião», definições) **ficam no cliente**: o Ctrl+K
junta-as localmente ao resultado do servidor.

---

## 1. Pesquisa global — `GET /api/search`

### Parâmetros

| Nome | Tipo | Regra |
|---|---|---|
| `q` | string | obrigatório, 1–200 caracteres, pelo menos uma letra ou dígito (`search.invalid_query`) |
| `types` | lista separada por vírgulas | omissão = todos os tipos que a pessoa pode ver; tipo desconhecido → `400 search.invalid_types` |
| `limit` | inteiro | resultados **por tipo**; omissão 5, preso a 1..20 |

### Tolerância

- **Acentos e maiúsculas:** `orcamento` encontra «Orçamento» (`unaccent`).
- **Enquanto se escreve:** cada termo é prefixo (`reuni` encontra «reunião»); termos juntam-se
  com E.
- **Subcadeias:** trigramas (`pg_trgm`) sobre títulos, nomes, emails e códigos — `ab-cd`
  encontra a sala `xab-cdy`. Só termos ≥ 3 caracteres usam o índice de trigramas.
- **Erros de escrita — só quando a pesquisa exacta não encontra nada.** Primeiro corre a exacta
  (prefixos + subcadeias); se der zero, corre a aproximada (`word_similarity ≥ 0.4`) —
  `orcamneto` encontra «Orçamento». Sem esta ordem, «orcamento» traria «planeamento» ao lado
  (medido). Nas listas, a resposta diz qual foi com `text_match: "exact" | "fuzzy"`, e a UI deve
  dizer «resultados aproximados».
- **Chinês e outras escritas sem espaços:** encontrados por subcadeia (trigramas). Um termo
  CJK de 2 caracteres é correcto mas não usa índice.
- **Sem stemming:** «reuniões» não encontra «reunião» a não ser como prefixo. Decisão do
  ADR-0007 §4.

### Tipos

| `type` | Onde procura (peso) | Quem vê (a MESMA regra do endpoint normal) | `target` para abrir | Estado |
|---|---|---|---|---|
| `meetings` | título (A), descrição (B), acta (C) | dono ou convidado (`GET /api/meetings`) | `{meeting_id, room_code, starts_at}` | implementado |
| `recordings` | título/ficheiro (A), transcrição (B), capítulos (C, com `at_secs`), comentários não apagados (C, com `at_secs`) | a biblioteca: quem carregou, participante da sala ou partilhada, e não arquivado (`AccessFacts::can_view`) | `{recording_id, at_secs}` | implementado (legendas e segmentos com tempo: contrato — só existem na linha da UI) |
| `people` | nome de utilizador, email | colegas **activos** de uma organização comum (`GET /api/users?q=`) | `{user_id}` | implementado |
| `whiteboards` | título, código da sala | membros activos da org do quadro (`GET /api/whiteboards`) | `{whiteboard_id}` | implementado |
| `rooms` | código, nome | só as salas que a pessoa **já conhece**: dela, onde esteve, convidada para uma reunião nessa sala, co-anfitriã. Mais restrito do que o `room_access` (que deixa qualquer colega pedir para entrar): o Ctrl+K não revela códigos de salas de colegas | `{room_code}` | implementado |
| `messages` | texto das mensagens de chat persistidas (ordenadas por recência, `score` 0) | dono da sala; ou **participante** da sala que ainda passa no `room_access` (quem saiu da org deixa de ver) | `{room_code, message_id, created_at}` | implementado |
| `stream_destinations` | nome, tipo | admin activo da org (`GET /api/orgs/{org_id}/stream-destinations`) | `{org_id, stream_destination_id}` | implementado |
| `webhooks` | tipo e **só o anfitrião** do URL (o caminho de um webhook do Slack é segredo) | admin activo da org | `{org_id, webhook_id}` | implementado |
| `audit_events` | acção, alvo, nome do actor | admin activo da org — só quando pedido explicitamente em `types`; só os eventos COM org (os sem org, p.ex. logins, ficam na lista da org) | `{org_id, audit_event_id}` | implementado |

Um tipo que a pessoa não pode ver **não aparece**. Se foi pedido explicitamente em `types`,
aparece em `skipped` com a razão (`search.forbidden`) — sem dizer nada sobre o que existe.

### Resposta `200`

```json
{
  "query": "orcamento",
  "took_ms": 23,
  "groups": [
    {
      "type": "recordings",
      "count": 3,
      "count_kind": "exact",
      "more_href": "/api/recordings?q=orcamento",
      "items": [
        {
          "type": "recordings",
          "id": "0b5d…",
          "title": "Revisão do orçamento 2027",
          "subtitle": "admin-alfa · 2026-09-12",
          "highlight": [
            {"text": "…e o ", "match": false},
            {"text": "orçamento", "match": true},
            {"text": " do segundo trimestre…", "match": false}
          ],
          "matched_in": "transcript",
          "score": 0.61,
          "target": {"recording_id": "0b5d…", "at_secs": null},
          "href": "/api/recordings/0b5d…",
          "occurred_at": "2026-09-12T10:00:00Z"
        }
      ]
    }
  ],
  "skipped": [{"type": "webhooks", "code": "search.forbidden"}]
}
```

- `highlight` vem **partido em segmentos**, não com marcas no texto: a UI escapa cada `text`
  e realça os `match: true`. Nunca há HTML no resultado.
- `matched_in`: `title` | `description` | `minutes` | `transcript` | `chapter` | `comment` |
  `name` | `email` | `code` | `message` | `url_host` | `action`.
- `count` é exacto até 1000 (`count_kind: "exact"`); acima disso `1000` com
  `"at_least"`.
- Tipos sem `tsvector` (pessoas, quadros, salas, destinos, webhooks, auditoria) realçam por
  subcadeia em minúsculas — sem dobrar acentos: «orcamento» não realça «Orçamento» nesses tipos
  (o resultado aparece; o realce não).
- A transcrição ainda não tem tempos nesta linha: `at_secs` é `null` quando `matched_in` é
  `transcript`; capítulos e comentários trazem-no.
- Ordem dos grupos: fixa (a da tabela). Ordem dentro do grupo: `score` descendente, depois
  data descendente, depois `id`.
- `score` só compara resultados do MESMO tipo.

---

## 2. Pesquisa de lista (estilo Odoo)

Os mesmos parâmetros em todas as colecções da secção 4. **Sem nenhum deles, a colecção
herdada responde como antes** (array); com qualquer um, responde o envelope da §2.3.

### 2.1 Parâmetros

| Nome | Forma | Exemplo |
|---|---|---|
| `q` | texto livre; pesquisa nos campos de texto do recurso com as regras da §1 | `q=orcamento 2027` |
| `filter` | JSON (URL-encoded) — o **domínio** (§2.2) | `filter=[["status","eq","ready"],{"or":[["category","eq","lecture"],["duration_secs","gte",3600]]}]` |
| `filters` | nomes de filtros pré-definidos do schema, separados por vírgulas | `filters=mine,this_week` |
| `group_by` | até 3 campos agrupáveis; datas com granularidade `:day`/`:week`/`:month`/`:quarter`/`:year` | `group_by=created_at:month,uploader` |
| `order_by` | até 3 campos ordenáveis; `-` = descendente; `_score` = relevância (só com `q`) | `order_by=-duration_secs,title` · `order_by=-_score` |
| `page_size` | 1..100, omissão 50 (`core::page`) | |
| `page_token` | cursor opaco de `next_page_token` | |
| `groups_page_token` | cursor opaco de `next_groups_page_token` | |

**Combinação:** `q` E `filter` E (filtros pré-definidos). Filtros pré-definidos do **mesmo
`group`** do schema combinam com OU; de grupos diferentes, com E (como no painel do Odoo:
«As minhas» OU «Partilhadas comigo», E «Esta semana»).

**Ordem por omissão:** a do schema (`default_order`); com `q` e sem `order_by`, relevância
descendente — excepto onde o schema diz `relevance_default: false` (gravações: a biblioteca
continua por data, como no contrato da 0045; relevância com `order_by=-_score`). O `id` é sempre o último desempate — a paginação é estável mesmo com empates.

**Keyset:** o `page_token` guarda os valores da última linha e uma impressão digital de
`q`/`filter`/`filters`/`order_by`. Reutilizá-lo com outra pesquisa → `400
search.page_token_mismatch`. Não há `OFFSET`.

### 2.2 O domínio (`filter`)

```
nó        := condição | {"and": [nó, …]} | {"or": [nó, …]} | {"not": nó}
condição  := [campo, operador] | [campo, operador, valor]
topo      := nó | [nó, …]          (lista no topo = E)
```

Limites: profundidade ≤ 4, ≤ 20 condições, `in`/`not_in` ≤ 100 valores, `filter` ≤ 4 KiB.

**Operadores por tipo de campo:**

| Tipo | Operadores | Valor |
|---|---|---|
| `text` | `eq`, `ne`, `contains`, `not_contains`, `starts_with`, `in`, `not_in`, `is_set`, `is_not_set` | string; `contains`/`starts_with` ignoram maiúsculas e acentos; `%` e `_` são literais |
| `enum` | `eq`, `ne`, `in`, `not_in` | um dos `options[].value` do schema (senão `search.invalid_value`) |
| `number` | `eq`, `ne`, `lt`, `lte`, `gt`, `gte`, `between`, `is_set`, `is_not_set` | número; `between` = `[mín, máx]` inclusivo |
| `datetime` | `lt`, `lte`, `gt`, `gte`, `between`, `in_period`, `is_set`, `is_not_set` | RFC 3339; `between` = `[de, até]` inclusivo; `in_period` = um período (abaixo) |
| `bool` | `eq` | `true`/`false` |
| `user` | `eq`, `ne`, `in`, `not_in`, `is_set`, `is_not_set` | UUID ou `"me"` (quem pede) |
| `ref` | `eq`, `ne`, `in`, `not_in`, `is_set`, `is_not_set` | UUID |

**Períodos (`in_period`)**, calculados no fuso da organização (`timezone` do schema,
omissão `Africa/Luanda`): `today`, `yesterday`, `this_week` (segunda a domingo),
`last_week`, `this_month`, `last_month`, `this_quarter`, `last_quarter`, `this_year`,
`last_year`, `last_7_days`, `last_30_days`, `next_7_days`, `past`, `future`.

`is_set`/`is_not_set` não levam valor. Um texto vazio conta como não definido.

### 2.3 Resposta `200`

```json
{
  "items": [ { "…": "a mesma forma de item da colecção" , "search": {"score": 0.42, "highlight": [ … ]} } ],
  "next_page_token": "eyJ…",
  "total": 1234,
  "total_kind": "exact",
  "groups": [
    {
      "key": "2026-09",
      "label": "2026-09",
      "count": 12,
      "aggregates": {"duration_secs": {"sum": 36000}, "size_bytes": {"sum": 9876543}},
      "range": {"from": "2026-08-31T23:00:00Z", "to": "2026-09-30T23:00:00Z"},
      "filter": {"and": [["created_at", "gte", "2026-08-31T23:00:00Z"], ["created_at", "lt", "2026-09-30T23:00:00Z"]]},
      "group_by": ["uploader"]
    }
  ],
  "next_groups_page_token": null,
  "text_match": "exact"
}
```

- **Compatível com `Page<T>`:** `items` e `next_page_token` são os de sempre; `total`,
  `total_kind`, `groups` e `next_groups_page_token` acrescentam.
- `total`: exacto até 10 000 (`"exact"`); acima, `10000` com `"at_least"`.
- `search` só aparece com `q` (o `highlight` segue a forma da §1). As gravações mantêm
  também o `snippet` herdado (marcas «»).
- `text_match` só aparece com `q`: `exact` ou `fuzzy` (§1).
- `range` vem em UTC (`Z`): é o mesmo instante que o início do dia/semana/… no fuso da org.
- **`groups`** só com `group_by`, e só do **primeiro** campo (agrupamento preguiçoso):
  - `key`: o valor do grupo — UUID (user/ref), valor do enum, texto, `true`/`false`, ou a
    chave de data `2026-09-17` (dia) · `2026-W38` (semana ISO) · `2026-09` (mês) ·
    `2026-Q3` (trimestre) · `2026` (ano). `null` = sem valor.
  - `label`: nome de utilizador, nome da referência, rótulo do enum; para datas repete a
    chave (a UI formata-a no idioma dela).
  - `filter`: o nó a JUNTAR ao `filter` corrente para abrir o grupo (para `null`,
    `[campo, "is_not_set"]`); `group_by`: o que falta agrupar.
  - `aggregates`: os do schema, sobre o conjunto filtrado inteiro do grupo.
  - Ordem dos grupos: pelo rótulo (nome da pessoa/referência; para o resto, a própria chave —
    as datas ficam cronológicas), `null` no fim; até 100 grupos por página, e
    `next_groups_page_token` para os seguintes.
  - Com `group_by`, `items` continua a trazer a primeira página **não agrupada** (a UI
    pode pedir `page_size=1` se só quer os grupos).

### 2.4 Erros (`400`, envelope plano)

| `code` | Quando | `details[].field` |
|---|---|---|
| `search.invalid_filter` | `filter` não é JSON, ou a forma do nó está errada | `filter` ou o caminho (`filter[1].or[0]`) |
| `search.filter_too_complex` | profundidade, número de condições, tamanho ou `in` acima do limite | `filter` |
| `search.unknown_field` | campo fora da lista branca do recurso | o caminho |
| `search.field_not_filterable` / `search.field_not_sortable` / `search.field_not_groupable` | o campo existe mas não serve para isso | `filter…` / `order_by` / `group_by` |
| `search.invalid_operator` | operador que não existe ou não serve para o tipo | o caminho |
| `search.invalid_value` | valor com tipo errado, enum fora das opções, data inválida, período desconhecido, operador sem valor | o caminho |
| `search.unknown_filter` | nome em `filters` que o schema não tem | `filters` |
| `search.invalid_group_by` | granularidade num campo que não é data, granularidade desconhecida, mais de 3 campos, repetido | `group_by` |
| `search.invalid_order_by` | mais de 3 campos ou repetido | `order_by` |
| `search.invalid_query` | `q` sem letras nem dígitos, ou > 200 caracteres (nas gravações o código continua `recording.invalid_query`, contrato da 0045) | `q` |
| `search.page_token_mismatch` | `page_token`/`groups_page_token` de outra pesquisa | `page_token` |
| `page.invalid_token` | token corrompido (`core::page`) | — |

`404 search.unknown_resource` em `GET /api/search/schemas/{resource}` desconhecido. Um
recurso de outra organização continua a ser `404`/`403` como no endpoint normal.

---

## 3. Descrição — `GET /api/search/schemas` e `GET /api/search/schemas/{resource}`

A lista devolve `{"items": [schema, …]}` só com os recursos que a pessoa pode listar em
alguma organização (`members` exige pertença activa; `audit_events` exige ser admin activo). O individual devolve um schema:

```json
{
  "resource": "recordings",
  "label": "Gravações",
  "collection": "/api/recordings",
  "org_scoped": false,
  "timezone": "Africa/Luanda",
  "text_search": {"fields": ["title", "filename", "transcript"], "typo_tolerant": true},
  "fields": [
    {"name": "created_at", "label": "Criada em", "type": "datetime",
     "operators": ["lt", "lte", "gt", "gte", "between", "in_period", "is_set", "is_not_set"],
     "filterable": true, "sortable": true, "groupable": true,
     "granularities": ["day", "week", "month", "quarter", "year"], "aggregates": []},
    {"name": "category", "label": "Categoria", "type": "enum",
     "operators": ["eq", "ne", "in", "not_in"], "filterable": true, "sortable": false, "groupable": true,
     "options": [{"value": "meeting", "label": "Reunião"}, {"value": "lecture", "label": "Aula"}]}
  ],
  "filters": [
    {"name": "mine", "label": "As minhas", "group": "owner", "filter": [["uploader", "eq", "me"]]}
  ],
  "group_by": [{"value": "created_at:month", "label": "Criada em: mês"}],
  "default_order": ["-created_at"],
  "relevance_default": false,
  "periods": ["today", "yesterday", "this_week", "…"]
}
```

`label` vem em português; a UI traduz pelo `name` (`search.<resource>.fields.<name>`,
`search.<resource>.filters.<name>`). `org_scoped: true` = a colecção tem `{org_id}` no
caminho.

---

## 4. Recursos cobertos

Legenda: **F** filtra · **O** ordena · **A** agrupa · Σ agregado.

### 4.1 `recordings` — `GET /api/recordings` · implementado

Visibilidade: a da biblioteca (`AccessFacts::can_view`). `q`: título/ficheiro (A) +
transcrição (B).

| Campo | Tipo | F | O | A | Σ |
|---|---|---|---|---|---|
| `title` (título, ou o nome do ficheiro) | text | ✓ | ✓ | | |
| `filename` | text | ✓ | | | |
| `uploader` | user | ✓ | | ✓ | |
| `room_code` | text | ✓ | | ✓ | |
| `category` | enum `meeting`·`lecture`·`broadcast`·`other` | ✓ | | ✓ | |
| `status` | enum `ready`·`failed` | ✓ | | ✓ | |
| `transcribed` | bool | ✓ | | ✓ | |
| `shared_with_me` | bool | ✓ | | | |
| `duration_secs` | number | ✓ | ✓ | | soma, média |
| `size_bytes` | number | ✓ | ✓ | | soma |
| `width` (largura em px; `null` se não se sabe) | number | ✓ | | | |
| `created_at` | datetime | ✓ | ✓ | ✓ | |

Filtros: `mine` «As minhas» e `shared_with_me` «Partilhadas comigo» (grupo `owner`);
`transcribed` «Com transcrição» e `without_transcript` «Sem transcrição» (`content`);
`failed` «Falhadas» (`status`); `today`, `this_week`, `this_month` (`period`);
`long` «Mais de 1 hora» (`duration`); `uhd` «4K» (`width ≥ 3840`, `quality`). Ordem: `-created_at`.

### 4.2 `meetings` — `GET /api/meetings` · implementado

Visibilidade: dono ou convidado. `q`: título (A), descrição (B), acta (C).

| Campo | Tipo | F | O | A | Σ |
|---|---|---|---|---|---|
| `title` | text | ✓ | ✓ | | |
| `description` | text | ✓ | | | |
| `owner` | user | ✓ | | ✓ | |
| `kind` | enum `video`·`voice` | ✓ | | ✓ | |
| `starts_at` | datetime | ✓ | ✓ | ✓ | |
| `duration_min` | number | ✓ | ✓ | | soma |
| `my_status` | enum `owner`·`pending`·`accepted`·`declined` | ✓ | | ✓ | |
| `recurring` | bool | ✓ | | ✓ | |
| `has_minutes` | bool | ✓ | | ✓ | |
| `meeting_room` | ref (sala física da org) | ✓ | | ✓ | |
| `created_at` | datetime | ✓ | ✓ | | |

Filtros: `mine` «Organizadas por mim», `invited` «Convidado» (`owner`); `pending_response`
«Por responder», `accepted` «Aceites», `declined` «Recusadas» (`response`); `video` «Vídeo»,
`voice` «Voz» (`kind`); `upcoming` «Próximas», `past` «Passadas», `today`,
`this_week`, `next_7_days` (`period`); `recurring` «Recorrentes», `with_minutes` «Com acta»
(`content`). Ordem: `starts_at`.

### 4.3 `members` — `GET /api/orgs/{org_id}/members` (hoje `/employees`) · implementado

Visibilidade: membro activo da org; lista só membros activos. `q`: nome, email, cargo.

| Campo | Tipo | F | O | A |
|---|---|---|---|---|
| `username` | text | ✓ | ✓ | |
| `email` | text | ✓ | ✓ | |
| `title` (cargo) | text | ✓ | ✓ | ✓ |
| `role` | enum `admin`·`member` | ✓ | | ✓ |
| `branch` | ref (filial) | ✓ | | ✓ |
| `joined_at` | datetime | ✓ | ✓ | ✓ |

Filtros: `admins` «Administradores», `members` «Membros» (`role`); `without_branch` «Sem
filial» (`branch`); `joined_this_month` «Entraram este mês» (`period`). Ordem: `username`.

### 4.4 `whiteboards` — `GET /api/whiteboards` · implementado

Visibilidade: membro activo da org do quadro. `q`: título, código da sala.

| Campo | Tipo | F | O | A |
|---|---|---|---|---|
| `title` | text | ✓ | ✓ | |
| `room_code` | text | ✓ | | ✓ |
| `owner` | user | ✓ | | ✓ |
| `is_public` | bool | ✓ | | ✓ |
| `created_at` | datetime | ✓ | ✓ | ✓ |

Filtros: `mine` «Os meus» (`owner`); `public` «Com link público» (`sharing`); `this_week`,
`this_month` (`period`). Ordem: `-created_at`.

### 4.5 `audit_events` — `GET /api/orgs/{org_id}/audit-events` (hoje `/audit`) · implementado

Visibilidade: admin activo da org; os eventos da org e os sem org cujo actor é (ou foi)
membro — a regra do `audit::list`. `q`: acção, alvo, nome do actor.

| Campo | Tipo | F | O | A |
|---|---|---|---|---|
| `action` | text | ✓ | ✓ | ✓ |
| `category` (antes do primeiro `.`: `auth`, `member`, `webhook`…) | text | ✓ | | ✓ |
| `target` | text | ✓ | | |
| `actor` | user | ✓ | | ✓ |
| `created_at` | datetime | ✓ | ✓ | ✓ |

Filtros: `logins` «Inícios de sessão», `security` «Segurança» (`auth.*`), `members`
«Membros», `integrations` «Integrações» (`apikey.*`, `webhook.*`, `stream_destination.*`,
`sms.*`) (`category`); `mine` «As minhas acções» (`actor`); `today`, `last_7_days`,
`last_30_days` (`period`). Ordem: `-created_at`. O `id` é inteiro (`BIGSERIAL`).

### 4.6 A seguir, com o mesmo mecanismo · fase 2

| Recurso | Colecção | Campos previstos |
|---|---|---|
| `stream_destinations` | `GET /api/orgs/{org_id}/stream-destinations` | `label` (text), `kind` (enum), `state` (enum), `created_by` (user), `created_at` |
| `webhooks` | `GET /api/orgs/{org_id}/webhooks` | `kind` (enum), `active` (bool), `created_at` |
| `sms_messages` | `GET /api/orgs/{org_id}/sms/messages` | `status`, `to`, `created_at` |
| `call_records` | `GET /api/orgs/{org_id}/voice/call-records` | `direction`, `duration`, `started_at` Σ duração |
| `contacts` | quando o recurso chegar à linha do backend (`sms_contactos` está só num ramo) | `name`, `phone`, `tags` |
| `room_messages` | `GET /api/rooms/{room_code}/messages` | `author` (user), `created_at`, `q` |

---

## 5. Favoritos — `/api/users/me/saved-searches`

| Método e caminho | Resposta |
|---|---|
| `GET /api/users/me/saved-searches?resource=&page_size=&page_token=` | `200` `Page` — as minhas e as partilhadas na minha org |
| `POST /api/users/me/saved-searches` | `201` + `Location` + o favorito |
| `GET /api/users/me/saved-searches/{saved_search_id}` | `200` |
| `PATCH /api/users/me/saved-searches/{saved_search_id}` | `200` |
| `DELETE /api/users/me/saved-searches/{saved_search_id}` | `204` |

Corpo do `POST` (o `PATCH` aceita qualquer subconjunto menos `resource`):

```json
{
  "resource": "recordings",
  "name": "Aulas longas deste mês",
  "query": {"q": "", "filter": [["category", "eq", "lecture"]], "filters": ["this_month", "long"],
            "group_by": ["uploader"], "order_by": ["-duration_secs"]},
  "shared": false,
  "is_default": false
}
```

Item:

```json
{
  "id": "…", "resource": "recordings", "name": "Aulas longas deste mês",
  "query": { "…": "…" }, "shared": false, "is_default": false,
  "owner": {"id": "…", "username": "admin-alfa"}, "editable": true,
  "valid": true, "invalid_code": null,
  "created_at": "…", "updated_at": "…"
}
```

Regras:

- A `query` valida-se contra o schema **ao gravar** (os mesmos erros da §2.4). Se o schema
  mudar e deixar de validar, o favorito volta com `valid: false` e `invalid_code`.
- `name` 1–80 caracteres, único por pessoa e recurso → `409 saved_search.duplicate_name`.
- `shared: true` = visível aos membros **activos** da organização de quem o criou (a org é a
  pertença activa, nunca vem do corpo). Sem organização activa → `422
  saved_search.no_organization`. Quem sai da org deixa de ver os partilhados.
- `is_default: true` desmarca o anterior da mesma pessoa e recurso (a UI aplica-o ao abrir
  o ecrã).
- Só o dono altera ou apaga: um partilhado de outra pessoa → `403 saved_search.not_owner`;
  um que não vês → `404`.
- ≤ 100 favoritos por pessoa → `422 saved_search.limit_reached`.
- `name` vazio ou > 80 → `400 saved_search.invalid_name`; `resource` sem pesquisa → `400
  search.unknown_resource`; não existe ou não o vês → `404 saved_search.not_found`.
- A «organização principal» (fuso das listas sem `{org_id}` e destino de um partilhado) é a
  pertença activa mais antiga de quem pede.

---

## 6. Isolamento — o que se garante e como se prova

1. Nenhum resultado (lista, Ctrl+K, grupo, contagem, agregado ou favorito) inclui algo que
   a pessoa não abriria pelo endpoint normal. As contagens e os agregados passam pelo
   MESMO filtro de visibilidade que os itens — um `count` de outra org também é fuga.
2. O `org_id` vem do caminho (verificado com `require_member`/`require_admin`) ou da
   pertença activa; nunca do corpo, do `filter` ou de um favorito.
3. Um `filter` sobre `uploader`/`owner`/`actor` com o UUID de alguém de outra org devolve
   zero linhas — não é uma forma de sondar.
4. Provas: testes de integração por recurso em `server/tests/search.rs` (org A não vê a
   org B, membro arquivado deixa de ver, a pesquisa concorda com a biblioteca pessoa a
   pessoa) e a secção «pesquisa» de `web/e2e/isolamento.mjs`.
