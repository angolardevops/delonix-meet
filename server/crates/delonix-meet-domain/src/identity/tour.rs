//! O guia de primeira utilização (Tour): o PROGRESSO por pessoa.
//!
//! O conteúdo dos passos (título, texto, onde apontam) é do cliente. O servidor
//! só conhece os IDENTIFICADORES, numa lista versionada: é o que lhe permite
//! recusar um passo inventado e dizer «N de M concluídos» sem que o cliente
//! mande o M.
//!
//! Uma versão nova do guia (passos acrescentados, retirados ou reordenados) é
//! uma constante nova. O progresso guardado com outra versão não se perde —
//! conta só a interseção com a versão actual ([`completed_in_current`]).

use delonix_meet_core::DomainError;

/// Versão actual do guia.
pub const VERSION: &str = "home-2026-09";

/// Os seis passos do guia do Início, por ordem.
///
/// Os três primeiros são os do template Navegavel3 (`DelonixTour`: «Comece por
/// aqui», «Agendar puxa do Odoo», «Uma sala, três modos»). O template só
/// descreve esses três de um total de seis; os três últimos seguem os blocos
/// que o mesmo ecrã mostra (próximas sessões, canais de emissão,
/// armazenamento) e são provisórios até o desenho os escrever — mudar-lhes o
/// id é uma versão nova.
pub const STEPS: [&str; 6] = [
    "home.start-now",
    "home.schedule-from-odoo",
    "home.one-room-three-modes",
    "home.upcoming-sessions",
    "home.stream-channels",
    "home.storage",
];

pub fn validate_step(id: &str) -> Result<&'static str, DomainError> {
    STEPS.iter().find(|s| **s == id).copied().ok_or_else(|| {
        DomainError::not_found("tour.unknown_step")
            .with_message(format!("passo desconhecido na versão {VERSION} do guia"))
    })
}

/// Os passos concluídos que pertencem à versão actual, na ordem do guia.
pub fn completed_in_current<'a>(stored: impl IntoIterator<Item = &'a str>) -> Vec<&'static str> {
    let stored: Vec<&str> = stored.into_iter().collect();
    STEPS
        .iter()
        .filter(|s| stored.contains(s))
        .copied()
        .collect()
}

/// O primeiro passo por concluir, ou `None` se o guia está completo.
pub fn next_step(completed: &[&str]) -> Option<&'static str> {
    STEPS.iter().find(|s| !completed.contains(s)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_validated_against_the_versioned_list() {
        assert_eq!(validate_step("home.start-now").unwrap(), "home.start-now");
        let e = validate_step("home.nope").unwrap_err();
        assert_eq!(e.code, "tour.unknown_step");
        assert!(validate_step("").is_err());
        let mut seen = std::collections::HashSet::new();
        assert!(STEPS.iter().all(|s| seen.insert(*s)), "ids únicos");
    }

    #[test]
    fn progress_counts_only_the_current_version() {
        let done = completed_in_current(["home.storage", "old.step", "home.start-now"]);
        assert_eq!(
            done,
            vec!["home.start-now", "home.storage"],
            "na ordem do guia"
        );
        assert_eq!(next_step(&done), Some("home.schedule-from-odoo"));
        assert_eq!(next_step(&STEPS), None);
    }
}
