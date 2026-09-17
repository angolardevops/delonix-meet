//! Os recursos pesquisáveis traduzidos para SQL: a visibilidade (a MESMA regra
//! do endpoint normal) e a expressão de cada campo da lista branca.
//!
//! As regras de pertença vêm de `org.rs` (ADR-0004 §5 regra 1); aqui só se
//! compõem. `viewer.id`/`viewer.org_id`/`viewer.tz` são os valores ligados à
//! cabeça da consulta (ver `sql.rs`).

use std::sync::LazyLock;

use delonix_meet_domain::{compliance, content, organization, scheduling};

use super::sql::ResourceSql;
use crate::org;

static RECORDINGS_FROM: LazyLock<String> = LazyLock::new(|| {
    // `AccessFacts::can_view` em forma de semi-junção: o conjunto visível
    // parte de quem pede (as suas gravações, as salas onde esteve, as
    // partilhas) em vez de avaliar três EXISTS por cada gravação da base.
    // O teste `recordings_visibility_matches_can_view` compara as duas.
    [
        " CROSS JOIN recordings r JOIN rooms rm ON rm.id = r.room_id \
         WHERE r.id IN (\
           SELECT r1.id FROM recordings r1 WHERE r1.uploader_id = viewer.id \
           UNION SELECT r2.id FROM recordings r2 JOIN room_participants p ON p.room_id = r2.room_id \
                  WHERE p.user_id = viewer.id \
           UNION SELECT s.recording_id FROM recording_shares s WHERE s.user_id = viewer.id) \
         AND NOT ",
        &org::sql_viewer_departed_from("r.uploader_id"),
    ]
    .concat()
});

pub static RECORDINGS: ResourceSql = ResourceSql {
    schema: &content::search::RECORDINGS,
    from: || RECORDINGS_FROM.clone(),
    id: "r.id",
    fields: &[
        ("title", "COALESCE(r.title, r.filename)"),
        ("filename", "r.filename"),
        ("uploader", "r.uploader_id"),
        ("room_code", "rm.code"),
        ("category", "r.category"),
        ("status", "r.status"),
        ("transcribed", "(r.transcribed_at IS NOT NULL)"),
        (
            "shared_with_me",
            "EXISTS (SELECT 1 FROM recording_shares ws WHERE ws.recording_id = r.id AND ws.user_id = viewer.id)",
        ),
        ("duration_secs", "r.duration_secs"),
        ("size_bytes", "r.size_bytes"),
        ("width", "r.width"),
        ("created_at", "r.created_at"),
    ],
    group_labels: &[(
        "uploader",
        "(SELECT lu.username FROM users lu WHERE lu.id = g.k::uuid)",
    )],
    fts: Some("r.search_vector"),
    trigram: &["dlx_fold(coalesce(r.title, '') || ' ' || r.filename)"],
};

pub static MEETINGS: ResourceSql = ResourceSql {
    schema: &scheduling::search::MEETINGS,
    // A regra do `meetings::list`: dono ou convidado. Em semi-junção a partir
    // de quem pede: com `m.owner_id = viewer.id OR mi.user_id IS NOT NULL` o
    // Postgres percorria as 150 k reuniões (1,3 s medidos); assim, 0,6 ms.
    // O LEFT JOIN fica só para o `my_status`.
    from: || {
        " CROSS JOIN meetings m \
         LEFT JOIN meeting_invitees mi ON mi.meeting_id = m.id AND mi.user_id = viewer.id \
         WHERE m.id IN (SELECT m1.id FROM meetings m1 WHERE m1.owner_id = viewer.id \
                        UNION ALL SELECT i1.meeting_id FROM meeting_invitees i1 WHERE i1.user_id = viewer.id)"
            .to_string()
    },
    id: "m.id",
    fields: &[
        ("title", "m.title"),
        ("description", "m.description"),
        ("owner", "m.owner_id"),
        ("kind", "m.kind"),
        ("starts_at", "m.starts_at"),
        ("duration_min", "m.duration_min"),
        (
            "my_status",
            "CASE WHEN m.owner_id = viewer.id THEN 'owner' ELSE COALESCE(mi.status, 'pending') END",
        ),
        (
            "recurring",
            "(m.recurrence_freq IS NOT NULL OR m.recurrence_parent_id IS NOT NULL)",
        ),
        ("has_minutes", "(m.minutes <> '')"),
        ("meeting_room", "m.room_ref"),
        ("created_at", "m.created_at"),
    ],
    group_labels: &[
        (
            "owner",
            "(SELECT lu.username FROM users lu WHERE lu.id = g.k::uuid)",
        ),
        (
            "meeting_room",
            "(SELECT lr.name FROM meeting_rooms lr WHERE lr.id = g.k::uuid)",
        ),
    ],
    fts: Some("m.search_vector"),
    trigram: &["dlx_fold(m.title)"],
};

pub static MEMBERS: ResourceSql = ResourceSql {
    schema: &organization::search::MEMBERS,
    from: || org::SQL_MEMBERS_SEARCH_FROM.to_string(),
    id: "m.user_id",
    fields: &[
        ("username", "u.username"),
        ("email", "u.email"),
        ("title", "m.title"),
        ("role", "m.role"),
        ("branch", "m.branch_id"),
        ("joined_at", "m.created_at"),
    ],
    group_labels: &[(
        "branch",
        "(SELECT lb.name FROM branches lb WHERE lb.id = g.k::uuid)",
    )],
    fts: None,
    trigram: &[
        "dlx_fold(u.username || ' ' || u.email)",
        "dlx_fold(m.title)",
    ],
};

static WHITEBOARDS_FROM: LazyLock<String> = LazyLock::new(|| {
    [
        " CROSS JOIN whiteboards w WHERE w.org_id IN ",
        org::SQL_VIEWER_ACTIVE_ORGS,
    ]
    .concat()
});

pub static WHITEBOARDS: ResourceSql = ResourceSql {
    schema: &content::search::WHITEBOARDS,
    // A regra do `whiteboards::list`: membro activo da org do quadro.
    from: || WHITEBOARDS_FROM.clone(),
    id: "w.id",
    fields: &[
        ("title", "w.title"),
        ("room_code", "w.room_code"),
        ("owner", "w.owner_id"),
        ("is_public", "w.is_public"),
        ("created_at", "w.created_at"),
    ],
    group_labels: &[(
        "owner",
        "(SELECT lu.username FROM users lu WHERE lu.id = g.k::uuid)",
    )],
    fts: None,
    trigram: &["dlx_fold(w.title || ' ' || w.room_code)"],
};

pub static AUDIT_EVENTS: ResourceSql = ResourceSql {
    schema: &compliance::search::AUDIT_EVENTS,
    from: || org::SQL_AUDIT_SEARCH_FROM.to_string(),
    id: "a.id",
    fields: &[
        ("action", "a.action"),
        ("category", "split_part(a.action, '.', 1)"),
        ("target", "a.target"),
        ("actor", "a.actor_id"),
        ("created_at", "a.created_at"),
    ],
    group_labels: &[(
        "actor",
        "(SELECT lu.username FROM users lu WHERE lu.id = g.k::uuid)",
    )],
    fts: None,
    trigram: &["dlx_fold(a.action || ' ' || a.target || ' ' || a.actor_name)"],
};

#[cfg(test)]
pub fn all() -> [&'static ResourceSql; 5] {
    [
        &MEETINGS,
        &RECORDINGS,
        &MEMBERS,
        &WHITEBOARDS,
        &AUDIT_EVENTS,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use delonix_meet_core::query::FieldType;

    /// Cada campo da lista branca tem expressão, e nenhuma expressão sobra.
    #[test]
    fn every_schema_field_has_exactly_one_expression() {
        for r in all() {
            for f in r.schema.fields {
                let n = r.fields.iter().filter(|(name, _)| *name == f.name).count();
                assert_eq!(n, 1, "{}.{}", r.schema.resource, f.name);
            }
            assert_eq!(
                r.fields.len(),
                r.schema.fields.len(),
                "{}",
                r.schema.resource
            );
            for (name, _) in r.group_labels {
                let f = r.schema.field(name).expect("rótulo de campo inexistente");
                assert!(f.groupable && matches!(f.ty, FieldType::User | FieldType::Ref));
            }
        }
        // O registo do domínio e o do SQL são o mesmo conjunto.
        let sql: Vec<_> = all().iter().map(|r| r.schema.resource).collect();
        let dom: Vec<_> = delonix_meet_domain::search::schemas()
            .iter()
            .map(|s| s.resource)
            .collect();
        assert_eq!(sql, dom);
    }

    /// A ordenação por data não usa COALESCE: a coluna tem de ser NOT NULL.
    #[test]
    fn sortable_datetimes_are_not_null() {
        for r in all() {
            for f in r
                .schema
                .fields
                .iter()
                .filter(|f| f.sortable && f.ty == FieldType::Datetime)
            {
                let e = r.expr(f.name);
                assert!(
                    [
                        "r.created_at",
                        "m.starts_at",
                        "m.created_at",
                        "w.created_at",
                        "a.created_at"
                    ]
                    .contains(&e),
                    "{}.{} = {e}: confirma NOT NULL e acrescenta aqui",
                    r.schema.resource,
                    f.name
                );
            }
        }
    }
}
