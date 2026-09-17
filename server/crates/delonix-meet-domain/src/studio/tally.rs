//! Tally: que fonte está no ar (PROGRAMA), em pré-visualização (PRÉ) ou livre.
//!
//! O operador fixa os dois conjuntos; cada telefone recebe só o SEU estado. Uma
//! fonte em PROGRAMA e em PRÉ ao mesmo tempo (a mesma câmara nos dois
//! barramentos, que acontece numa transição) fica PROGRAMA: a luz vermelha é a
//! que não pode mentir.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_PER_BUS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Tally {
    Program,
    Preview,
    #[default]
    Free,
}

impl Tally {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Program => "program",
            Self::Preview => "preview",
            Self::Free => "free",
        }
    }
}

/// O estado de uma fonte, dados os dois barramentos.
pub fn state_of(source: Uuid, program: &[Uuid], preview: &[Uuid]) -> Tally {
    if program.contains(&source) {
        Tally::Program
    } else if preview.contains(&source) {
        Tally::Preview
    } else {
        Tally::Free
    }
}

/// Os barramentos vêm do cliente: sem repetidos e com tecto.
pub fn validate_buses(program: &[Uuid], preview: &[Uuid]) -> Result<(), &'static str> {
    if program.len() > MAX_PER_BUS || preview.len() > MAX_PER_BUS {
        return Err("studio.tally_too_many");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programa_ganha_a_pre() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert_eq!(state_of(a, &[a], &[a, b]), Tally::Program);
        assert_eq!(state_of(b, &[a], &[a, b]), Tally::Preview);
        assert_eq!(state_of(Uuid::new_v4(), &[a], &[b]), Tally::Free);
    }

    #[test]
    fn serializa_como_o_contrato() {
        assert_eq!(
            serde_json::to_string(&Tally::Program).unwrap(),
            "\"program\""
        );
        assert_eq!(Tally::default(), Tally::Free);
    }

    #[test]
    fn barramentos_com_tecto() {
        let v: Vec<Uuid> = (0..17).map(|_| Uuid::new_v4()).collect();
        assert!(validate_buses(&v, &[]).is_err());
        assert!(validate_buses(&v[..16], &v[..16]).is_ok());
    }
}
