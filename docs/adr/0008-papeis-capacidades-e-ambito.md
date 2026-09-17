# ADR-0008 — Papéis, capacidades e âmbito

**Estado:** Proposto (para revisão da sessão dona da linha de backend) · **Data:** 2026-09-17 ·
**Contexto:** ecrãs «Papéis e permissões» (`DelonixRBAC`) e «Utilizadores e convites»
(`DelonixUsers`) do template Navegavel3 — frente A do backend v3 ·
**Assenta em:** [ADR-0004](0004-organizacao-alvo-do-backend.md) (camadas, catraca, pertença em
`org.rs`) e [ADR-0006](0006-backend-enterprise-contextos-edicoes-e-entrega.md) (contextos de
domínio, edições, envelope de erro).

## Contexto

### O modelo de hoje, medido na base `origin/seg/ssrf-saida` (`2f830bc`)

1. **Papel na organização = uma string com dois valores.** `org_members.role` é
   `'admin' | 'member'` (migração 0005). Não há dono: quem cria a org fica `admin`
   (`auth.rs:384`, `org.rs:442`); as orgs vindas do Odoo têm como `created_by` o utilizador
   de serviço `provisioning@delonix.internal` (`apikeys.rs:935`), que também é `admin`.
2. **Uma regra de acesso, 52 chamadas.** `org::require_admin`/`require_admin_pub` é chamado
   em 12 módulos: `org` (11), `sms` (10), `webhooks` (7), `stream_destinations` (6), `voice`
   (4), `apikeys` (3), `audit` (2+1 import), `odoo` (3), `whiteboards` (2), `broadcast`,
   `meetings`, `usage` (1 cada). Todos significam o mesmo: «é `admin` activo».
   `require_member` significa «é membro activo». Ambos passam por `org::role_in_org`, a
   única fonte de pertença (filtra `archived_at`, S3/R121).
3. **Três verificações de papel por string fora de `org.rs`:**
   - `voice.rs:444` — `role != "admin"` (encerrar uma sala PSTN, R141);
   - `recordings.rs:269` — `me.role = 'admin'` dentro do SQL de visibilidade da biblioteca;
   - `odoo_sso.rs:415` — `org_members.role = 'admin'` no «nunca despromove» da sincronização.
4. **Papéis na sala são outra coisa e ficam onde estão.** Anfitrião = `rooms.created_by`;
   co-anfitriões de admissão persistentes em `room_admitters` (0017); os restantes poderes
   da sala vivem no `signaling` em memória. São regras da SALA, não da organização.
5. **Máquinas têm escopos, não papéis.** `identity::api_key::Scope` (S6/R170): catálogo
   fechado de 7 escopos, `key.require(Scope::…)?`, parse que recusa desconhecidos com
   código estável. É só para chaves `dlx_`.
6. **Administrador da plataforma** é `PLATFORM_ADMIN_USER_IDS` (S1/R121), nunca derivado de
   `org_members`. Não muda aqui.
7. **Não existe:** convite de organização (o `REGISTRATION_MODE=invite` não tem entidade de
   convite), departamento, suspensão distinta do arquivo, lugares/licenças, mapeamento de
   grupo Odoo. A sincronização do Odoo só conhece o booleano «é admin no Odoo».
8. **Catraca da arquitectura:** `pertenca_org_fora_de_org_rs=22`.

### O que os ecrãs pedem

Sete papéis (quatro de sistema bloqueados, três personalizados), uma matriz capacidade ×
papel com quatro valores (permitido, negado, herdado, requer aprovação), âmbito por
departamento, limites por papel, atribuição automática por grupo do Odoo, alerta de
segregação de funções, simular utilizador, duplicar/eliminar/exportar CSV — e, do lado dos
utilizadores, departamento, origem, estado (activo/convidado/suspenso), lugares, convites
pendentes e regras de entrada.

## Decisão

### 1. Catálogo FECHADO de capacidades, no domínio

`delonix_meet_domain::identity::authorization::Capability` é um `enum` fechado, versionado
(`CATALOG_VERSION = 1`). Copia o PADRÃO dos escopos S6 (catálogo no domínio, `as_str`,
`parse` que recusa desconhecidos com `authz.unknown_capability`), mas **não os reutiliza**:
escopos são de máquinas, capacidades são de pessoas. Uma chave `dlx_` continua só com
escopos e **não herda** as capacidades de quem a criou.

| Grupo | Código | Ecrã | Imposta no servidor nesta frente |
|---|---|---|---|
| Sessões | `sessions.create` | Criar e agendar sessões | não |
| | `sessions.admit_waiting_room` | Admitir da sala de espera (inclui convidados por telefone) | não |
| | `sessions.mute_remove` | Silenciar e remover pessoas | não |
| | `sessions.breakout_rooms` | Abrir salas paralelas | não |
| Gravação e biblioteca | `recordings.record_4k` | Gravar em 4K | não |
| | `recordings.view_others` | Ver gravações de outros (só dentro do âmbito) | não |
| | `recordings.publish` | Publicar gravação | não |
| | `recordings.delete` | Apagar gravação (irreversível) | não |
| Emissão | `broadcast.public_destinations` | Emitir para destinos públicos | **sim** — destinos guardados no `/api/rooms/{room_code}/live` |
| | `broadcast.manage_rtmp_keys` | Gerir chaves RTMP (credenciais de terceiros) | **sim** — `stream-destinations` (6 rotas) |
| | `broadcast.highlight_questions` | Destacar perguntas no palco | não |
| Estúdio e quadro | `studio.edit_timeline` | Editar na linha de tempo | não |
| | `studio.generate_captions` | Gerar legendas e dobragem (consome nó de inferência) | não |
| | `studio.export_4k` | Exportar em 4K | não |
| Administração | `admin.manage_accounts` | Convidar e suspender contas | **sim** — membros, convites, acções em massa, importação, lugares |
| | `admin.manage_roles` | Editar papéis e permissões | **sim** — papéis, matriz, SoD, conflitos, mapeamento Odoo |
| | `admin.view_audit` | Ver registo de auditoria | **sim** — `audit-events` e `verification` |
| | `admin.change_retention` | Mudar retenção e residência (afecta toda a organização) | **sim** — `PATCH /api/orgs/{org_id}` |
| | `org.administer` | *(não está no ecrã)* Administração técnica: integrações, SSO, chaves de API, webhooks, voz, SMS, filiais, salas físicas, estatísticas | **sim** — todas as restantes chamadas a `require_admin` |

- **`org.administer` é só de sistema.** Existe para que `require_admin` passe a ser um
  wrapper de uma capacidade sem mudar de significado. Nunca pode ser dada a um papel
  personalizado (`authz.system_only_capability`): seria a porta de escalada para tudo o que
  o catálogo ainda não parte em capacidades finas.
- **«Imposta: não» é dito na API.** `GET /api/capabilities` devolve, por capacidade,
  `enforced: bool` e os pontos onde é imposta. Uma capacidade que a UI mostra e o servidor
  ainda não impõe fica marcada como tal — não se finge. As capacidades de sessão descrevem
  poderes sobre sessões de OUTROS e da organização; os direitos do anfitrião na própria sala
  continuam a ser regras da sala (contexto 4).
- **Acrescentar uma capacidade** é mudar o `enum`, subir `CATALOG_VERSION` e escrever a
  migração que a semeia nos papéis de sistema. Os papéis personalizados recebem-na como
  `inherit`.

### 2. Valores e a policy pura

`CapabilityValue = allow | deny | inherit | requires_approval`.

```rust
pub fn can(subject: &Subject, capability: Capability, scope: ResourceScope) -> Decision
// Decision = Allow | RequiresApproval | Deny(DenyReason), sempre com Explanation (o porquê)
```

Sem IO, testada por tabela. `Subject` = o papel da pertença, a cadeia de herança já
carregada, o departamento da pertença e o papel `member` de sistema da mesma org. As regras:

1. **Herança.** Um papel personalizado tem `inherits_from` (por omissão, `member` de
   sistema). `inherit` resolve-se no pai; o valor explícito mais próximo ganha — um `deny`
   explícito no filho **ganha** ao `allow` herdado do pai. A raiz sem valor dá
   `Deny(not_granted)`. Profundidade máxima 5; ciclos recusados ao gravar
   (`role.inheritance_cycle`).
2. **Âmbito (ABAC mínimo).** O departamento é um **atributo da pertença**
   (`org_members.department_id`). `ResourceScope = Organization | Department(id)`. Um papel
   com `scope = department` só se aplica quando o recurso está em
   `Department(<departamento da pertença>)`; fora disso, a pessoa é avaliada como `member`
   («Fora dele, a pessoa é Membro»). Um recurso sem departamento é `Organization`. Nunca é
   um filtro de UI.
3. **`requires_approval`** não executa: devolve `RequiresApproval` e o adaptador cria (ou
   reutiliza) um pedido pendente (§6).
4. **Suspenso, arquivado ou não membro** não chega à policy: é `404` antes (R153).

### 3. Papéis

- **Papéis de SISTEMA semeados por organização e IMUTÁVEIS**: `owner` (Proprietário),
  `admin` (Administrador), `member` (Membro), `external_guest` (Convidado externo). Não se
  apagam, não mudam de nome, de âmbito nem de matriz (`role.system_immutable`). Semântica:
  - `owner` e `admin`: `allow` em todo o catálogo, incluindo `org.administer` — exactamente o
    `admin` de hoje. O `owner` só acrescenta a protecção de dono (§5);
  - `member`: o `member` de hoje — `sessions.create` e `recordings.record_4k` `allow`, o resto
    `deny` (nenhuma das capacidades impostas era de membro);
  - `external_guest`: `deny` em tudo.
- **Papéis PERSONALIZADOS por organização:** nome (único na org), descrição, `inherits_from`,
  `scope` (`organization` | `department`, com `department_id` quando é departamento), matriz
  (`inherit` por omissão), limites (§8) e `odoo_group` (§9). Duplicar copia tudo menos o nome
  e o grupo Odoo. Eliminar exige `reassign_to` quando há pessoas
  (`role.reassignment_required`) e reatribui na mesma transacção.
- **Uma pertença, um papel** (`org_members.role_id`). A contagem por papel do ecrã
  (1+4+14+38+9+74+2 = 142) confirma-o.
- **A coluna herdada `org_members.role` fica, sincronizada por gatilho**: `owner`/`admin` →
  `'admin'`, qualquer outro → `'member'`. É o que mantém intactos o SQL da biblioteca
  (`recordings.rs:269`), a regra de encerrar sala (`voice.rs:444`) e o «nunca despromove»
  da sincronização, e todos os escritores herdados (registo, `add_employee`, `odoo_sso`,
  `meetings_v1`, provisionamento), que continuam a escrever `'admin'`/`'member'`: um
  `INSERT`/`UPDATE` só com `role` recebe o `role_id` de sistema correspondente, e um
  `UPDATE` de `role_id` reescreve `role`. O gatilho nunca despromove um `owner` por uma
  escrita herdada de `'admin'`.
- **Migração dos dados:** cada org recebe os quatro papéis de sistema; cada pertença recebe
  o `role_id` do seu `role`; o `owner` é o `created_by` se for admin activo, senão o admin
  activo mais antigo que não seja o utilizador de serviço. Uma org sem humano admin (só o
  utilizador de serviço, caso do Odoo antes do primeiro login) fica sem dono até ao primeiro
  admin humano entrar, que é promovido pelo gatilho. Esta é a única excepção ao invariante
  «≥ 1 dono», e é dita no relatório de lugares (`owner_missing: true`).

### 4. Um só ponto de imposição, em `org.rs`

```rust
pub(crate) async fn require_capability(state, org_id, user_id, Capability, ResourceScope)
    -> Result<Grant, ApiError>
```

- Carrega a pertença por `role_in_org` (a ÚNICA fonte; nada de `FROM org_members` fora de
  `org.rs` — a catraca `pertenca_org_fora_de_org_rs=22` não sobe) e os papéis da org, e
  chama `authorization::can`.
- **Estados (R153):** não membro activo / outra org / inexistente → `404`; autenticado sem a
  capacidade → `403 authz.missing_capability` (com a capacidade em `details`);
  `requires_approval` sem aprovação válida → `403 authz.approval_required` (com o id do
  pedido em `details`); sem sessão → `401` (o extractor).
- **`require_admin` e `require_member` passam a wrappers**:
  `require_admin = require_capability(org.administer, Organization)`,
  `require_member = role_in_org(...).is_some()`. As 52 chamadas não mudam de sítio nem de
  significado. **Muda uma coisa, com intenção:** o `code` do 403 passa de
  `permission_denied` para `authz.missing_capability` (o estado continua 403; nem o web nem
  os testes lêem o código antigo — medido com `grep`).
- **Pontos migrados para a capacidade fina** (equivalentes por construção: antes de haver
  papéis personalizados, só `owner`/`admin` têm estas capacidades, que é o `admin` de hoje):

  | Ponto | Antes | Depois |
  |---|---|---|
  | `org::add_employee`, `update_employee`, `remove_employee` | `require_admin` | `admin.manage_accounts` |
  | `audit::list`, `audit::verify` | `require_admin_pub` | `admin.view_audit` |
  | `stream_destinations::{list,create,get_one,update,rotate_key,delete}` | `require_admin_pub` | `broadcast.manage_rtmp_keys` |
  | `broadcast::ws_directo` (destinos guardados) | `require_admin_pub` | `broadcast.public_destinations` + limite de destinos |
  | `org::update_settings` | `require_admin` | `admin.change_retention` |

- **Ficam como `org.administer`** (sem capacidade fina no catálogo): `apikeys` (3), `odoo`
  (3), `org` SSO (3), filiais, salas físicas, estatísticas, `sms` (10), `voice` (4),
  `webhooks` (7), `usage`, `meetings::quarantine_analytics`, `whiteboards` (2, o «admin
  apaga/partilha»).
- **Ficam como estão, e contam na catraca nova:** `voice.rs:444` e `recordings.rs:269`
  (lêem a coluna herdada, que o gatilho mantém fiel) e `odoo_sso.rs:415`. Nova medida
  `verificacoes_papel_por_string_fora_de_org_rs = 3`, que só pode descer. A migração de
  `recordings.rs:269` para `recordings.view_others` pede a capacidade dentro do SQL de
  visibilidade (filtra antes de paginar) e fica para quem tocar na biblioteca.
- **Testes de não-regressão:** para cada ponto migrado, a tabela owner/admin/member/
  arquivado/outra org → estado HTTP, antes igual a depois, contra Postgres real.

### 5. Invariantes

1. **Sempre ≥ 1 dono activo** (excepção da §3). Despromover, suspender, arquivar ou remover
   o último `owner` → `409 role.last_owner`. Imposto no serviço (todas as escritas desta
   frente e as herdadas de `org.rs`) e com um gatilho de restrição adiado na base como
   segunda linha.
2. **Papéis de sistema** não se apagam nem mudam (§3).
3. **Sem escalada.** Quem edita:
   - só dá `allow`/`requires_approval` a capacidades que ELE tem como `allow` no âmbito da
     organização (`authz.escalation`);
   - só atribui a alguém (a si incluído) um papel cujas capacidades efectivas estão todas no
     seu próprio `allow`;
   - só um `owner` atribui ou retira o papel `owner`;
   - `org.administer` nunca vai para um papel personalizado;
   - mapear um grupo Odoo para um papel exige ter as capacidades desse papel.
4. **Requer aprovação cria um pedido, não executa** (§6).
5. **Toda a escrita administrativa audita** com actor, alvo e diff (`audit::log`, alvo em
   JSON compacto).

### 6. Pedidos de aprovação

- `approval_requests(org_id, requester_id, capability, action, target, status
  pending|approved|rejected|consumed|expired, decided_by, decided_at, reason, expires_at)`.
- Criado pelo `require_capability` quando a decisão é `RequiresApproval`; um pedido pendente
  igual (mesma pessoa, capacidade, acção e alvo) é reutilizado.
- **Aprovar:** quem aprova tem `allow` nessa capacidade e não é quem pediu
  (`approval.self_approval`). A aprovação é uma **licença de uso único** válida 24 h: a
  repetição do mesmo pedido pela mesma pessoa consome-a e executa. Nada é re-executado em
  nome de ninguém.

### 7. Suspensão e lugares

- **Suspender = arquivar** (`archived_at`, `archived_by`) com `archived_reason`
  (`suspended` | `removed` | `odoo_exit` | `inactive` | `guest_expired`). Reutiliza a regra S3
  inteira — as 22 leituras fora de `org.rs` que filtram `archived_at` passam a tratar o
  suspenso como quem saiu, sem mudar uma linha. Reactivar limpa `archived_at` e passa pela
  regra dos lugares. O directório mostra os arquivados como `suspended`.
- **Lugares seguem o padrão da `storage_quota`:** regra pura
  `organization::seats::{check_activation, remaining}` no domínio, tecto em
  `organizations.max_seats` (`NULL` = ilimitado), uso MEDIDO no adaptador (membros activos
  menos o utilizador de serviço). **Não há contador.** O tecto é do operador
  (`/api/operator/v1`), nunca do admin da org. As edições não são licenciamento (ADR-0006
  §2): `enterprise` e `personal` nascem sem tecto. «Libertar lugares» = suspender, por item,
  quem não entra há N dias (nunca o último dono nem quem pede).
- **Último acesso:** `users.last_access_at`, escrito por gatilho a cada `auth.login*` na
  auditoria e preenchido a partir dela na migração.

### 8. Limites por papel

`max_session_minutes`, `max_resolution_p`, `max_simultaneous_destinations`,
`max_external_guests_per_month` (`NULL` = sem limite; herdam do pai quando `NULL` num
personalizado). **Impostos nesta frente:** destinos em simultâneo (no directo, com destinos
guardados) e convidados externos por mês (na criação de convites externos, contados por quem
convida). **Guardados e ditos não impostos** (`enforced: false` na resposta): duração e
resolução — pedem o SFU e o gravador, que não são desta frente.

### 9. Grupos do Odoo

- Um papel personalizado ou `admin` pode ter `odoo_group` (id externo, ex.
  `delonix_comunicacao_emissao`). **Nunca `owner`** (`role.odoo_group_owner_forbidden`).
- Aplicado **só na sincronização do directório** (`odoo_sso`: a leitura periódica e o
  `provision`, que já passa por `odoo_sso::upsert_member`), e só a contas cuja autoridade é
  ESTA org (`users.odoo_org_id`, R25) e cuja pertença está activa (R143).
- Quem entra no grupo recebe o papel (`role_source = odoo_group`); quem sai volta a `member`
  na sincronização seguinte — só se o papel tinha vindo do grupo. Um papel dado à mão, ou
  dois grupos a apontar para papéis diferentes, **não se decide sozinho**: nasce um
  `role_conflicts` pendente que um admin resolve (`keep_current` | `apply_proposed`).
- A sincronização grava, por org, o resultado da última corrida (criados, actualizados,
  ignorados, suspensos, conflitos) — é o que o ecrã mostra, sem números inventados.

### 10. Segregação de funções

- Regras por org: nome, ≥ 2 capacidades, papéis isentos (o `owner` por omissão).
- Violação = pessoa activa cujo `allow` efectivo (âmbito organização ∪ o seu departamento)
  cobre todas as capacidades da regra e cujo papel não está isento.
- «Aceitar risco» grava a justificação (obrigatória, auditada) por (regra, pessoa); deixa de
  valer quando o papel da pessoa muda.
- Gravar a matriz de um papel que viola uma regra NÃO é recusado: a resposta traz `warnings`.

### 11. Convites, departamentos e regras de entrada (o que os ecrãs precisam do modelo)

- **Departamento** é entidade da org (`departments`: nome, `source = odoo | manual`); os
  vindos do Odoo não se editam à mão (`department.managed_by_odoo`).
- **Convite de organização** (`org_invitations`): correio, papel, departamento, externo ou
  não, expiração, estado, token guardado só como hash. **Não há transporte de correio no
  servidor** (medido: zero SMTP): criar e reenviar devolvem o link UMA vez, com
  `delivery: "manual"`, e a UI entrega-o. Aceitar exige sessão cujo correio é o do convite.
- **Regras de entrada** por org: criar conta na primeira entrada só para domínios aprovados,
  suspender ao sair do Odoo, validade dos convidados externos, e o registo de presenças
  `hr.attendance` — que fica `available: false` e recusa ligar-se
  (`entry_rules.attendance_unavailable`) enquanto não houver integração com esse modelo.

### 12. Edição `personal`

O mesmo código. A única pessoa é `owner`. Não há ecrãs de papéis: a UI decide pelo `edition`
do `GET /api/public/settings`, que já existe; o servidor não esconde rotas por edição
(ADR-0006 §2: as edições não fecham funcionalidade no código).

## Fora deste ADR

- Poderes na sala (anfitrião, co-anfitrião, admissão) — continuam regras da sala.
- Administração da plataforma (S1) e RLS (ADR-0002).
- Impor as capacidades marcadas «não»: cada uma entra quando o seu módulo for tocado, com o
  teste de tabela ao lado e `enforced: true` no catálogo.

## Consequências

- **+** Uma regra de autorização, num sítio, testada sem base de dados; `require_admin` deixa
  de ser uma string comparada em 52 sítios.
- **+** Nenhuma rota existente muda de estado HTTP; as orgs existentes avaliam exactamente
  como hoje até alguém criar um papel personalizado.
- **+** O que a UI mostra como «permitido» é o que o servidor faz — e o que ainda não faz vem
  marcado.
- **−** A coluna `org_members.role` fica como sombra sincronizada por gatilho até as três
  leituras por string saírem.
- **−** Muda o `code` do 403 de `require_admin` (`permission_denied` →
  `authz.missing_capability`).
- **−** Capacidades de departamento só têm efeito onde o recurso tem departamento — hoje, o
  directório de pessoas. Salas, gravações e destinos não têm departamento.
