-- ════════════════════════════════════════════════════════════════════════
--  `meetings.org_id` — a reunião passa a saber, ela própria, de que
--  organização é.
--
--  Até aqui (0004) `meetings` só tinha `owner_id`. A superfície pública v1
--  (chaves `dlx_`, `meetings_v1::meeting_in_org`, e as duas queries
--  equivalentes em `apikeys.rs`: `v1_meetings` e `v1_meeting_notes`) decidia
--  se uma reunião "pertence" à organização da chave verificando SÓ se o DONO
--  tinha uma linha em `org_members` para essa organização — nunca se essa
--  reunião foi criada mesmo ali, e nunca se essa pertença ainda estava activa.
--
--  Isso abre duas fugas:
--    1. Um utilizador que já foi membro da organização B, agora ARQUIVADO lá
--       (`org_members.archived_at` preenchido), continua a ter a linha em
--       `org_members` — só arquivada, não apagada. Uma reunião sua, criada
--       noutro contexto qualquer, continuava "visível" à chave de B para
--       sempre, mesmo depois de a empresa o desligar.
--    2. Sem um registo explícito de em que organização a reunião nasceu, a
--       pertença do dono é a ÚNICA fonte — e pertença não é posse: o dono
--       pode (tivesse sido possível) pertencer a duas organizações, e nada
--       aqui dizia qual das duas é dona da reunião.
--
--  A correcção grava a organização na CRIAÇÃO (o contexto em que a chave de
--  API, ou a sessão, estava a operar) e os três sítios acima passam a
--  perguntar directamente `meetings.org_id = <org da chave>` — não mais
--  inferência via pertença do dono.
--
--  NULLABLE, não NOT NULL: a BFF (`meetings::create`, sessão) escolhe o
--  `org_id` pela MESMA heurística que já usa para a quota de reuniões
--  (`org::orgs_of_user(...).first()` — a primeira organização activa do
--  criador) porque um utilizador de sessão não traz um "contexto de
--  organização" explícito no pedido; não há garantia de que essa heurística
--  encontre sempre uma organização (conta órfã, sem pertença nenhuma), e
--  nesse caso a coluna fica NULL em vez de inventar um valor. Uma reunião com
--  `org_id` NULL é, depois desta migração, invisível a qualquer chave `dlx_`
--  (fecha em segurança, não em conveniência) — o que é correcto: ninguém
--  reivindicou aquela reunião.
--
--  O `ON DELETE SET NULL` (em vez de CASCADE) é deliberado: apagar uma
--  organização não deve apagar o histórico de reuniões de quem lá passou —
--  só deixa de haver dono-de-organização para elas.
ALTER TABLE meetings ADD COLUMN org_id UUID REFERENCES organizations(id) ON DELETE SET NULL;

CREATE INDEX meetings_org_id_idx ON meetings (org_id);

-- Backfill das reuniões existentes: heurística, não garantia (dito no
-- comentário acima e repetido aqui porque é a frase que mais importa desta
-- migração). Escolhe-se a pertença do dono ordenada por "activa primeiro,
-- depois a mais antiga" — mesmo critério informal que `org::orgs_of_user`
-- já usava (sem ORDER BY explícito, mas lendo a primeira linha). Uma reunião
-- cujo dono não tem NENHUMA pertença (conta órfã) fica com `org_id` NULL, e
-- portanto deixa de ser alcançável por qualquer chave `dlx_` — o que é a
-- escolha seguro-por-omissão descrita acima, não um efeito secundário.
UPDATE meetings m
SET org_id = sub.org_id
FROM (
    SELECT DISTINCT ON (om.user_id) om.user_id, om.org_id
    FROM org_members om
    ORDER BY om.user_id, (om.archived_at IS NULL) DESC, om.created_at ASC
) sub
WHERE m.owner_id = sub.user_id;
