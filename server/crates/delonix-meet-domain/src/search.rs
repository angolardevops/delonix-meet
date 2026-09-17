//! Registo dos recursos pesquisáveis (ADR-0007). Cada schema vive no contexto
//! do seu recurso; aqui só se juntam, pela ordem em que a UI os mostra.

use delonix_meet_core::query::SearchSchema;

use crate::{compliance, content, organization, scheduling};

/// Todos os recursos com pesquisa de lista.
pub fn schemas() -> [&'static SearchSchema; 5] {
    [
        &scheduling::search::MEETINGS,
        &content::search::RECORDINGS,
        &organization::search::MEMBERS,
        &content::search::WHITEBOARDS,
        &compliance::search::AUDIT_EVENTS,
    ]
}

pub fn schema(resource: &str) -> Option<&'static SearchSchema> {
    schemas().into_iter().find(|s| s.resource == resource)
}

#[cfg(test)]
mod tests {
    use super::*;
    use delonix_meet_core::query::{compile, parse_filter, Ctx, FieldType, ListParams};
    use uuid::Uuid;

    /// Um filtro pré-definido que não validasse dava 400 ao primeiro clique.
    #[test]
    fn every_named_filter_validates_against_its_schema() {
        let ctx = Ctx { me: Uuid::nil() };
        for s in schemas() {
            for nf in s.filters {
                parse_filter(s, nf.filter, ctx)
                    .unwrap_or_else(|e| panic!("{}.{}: {e}", s.resource, nf.name));
            }
        }
    }

    #[test]
    fn default_order_and_groupings_compile() {
        let ctx = Ctx { me: Uuid::nil() };
        for s in schemas() {
            compile(s, &ListParams::default(), ctx).unwrap();
            for f in s.fields.iter().filter(|f| f.groupable) {
                let p = ListParams {
                    group_by: Some(f.name.to_string()),
                    ..Default::default()
                };
                compile(s, &p, ctx).unwrap_or_else(|e| panic!("{}.{}: {e}", s.resource, f.name));
            }
        }
    }

    #[test]
    fn names_are_unique_and_enums_have_options() {
        for s in schemas() {
            let mut names: Vec<_> = s.fields.iter().map(|f| f.name).collect();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), s.fields.len(), "{}", s.resource);
            for f in s.fields {
                assert_eq!(
                    f.ty == FieldType::Enum,
                    !f.options.is_empty(),
                    "{}.{}",
                    s.resource,
                    f.name
                );
                assert!(f.aggregates.is_empty() || f.ty == FieldType::Number);
            }
        }
        assert!(schema("recordings").is_some());
        assert!(schema("users; drop").is_none());
    }
}
