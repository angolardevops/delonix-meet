//! Paginação por cursor opaco (ADR-0004 §4).
//!
//! - `page_size` por omissão 50, máximo 100: nenhuma listagem sem limite, e
//!   nenhum limite silencioso — quem pede 500 recebe 100 *e* um
//!   `next_page_token`.
//! - O token é opaco para o cliente (base64url de JSON). Não é um segredo nem
//!   uma autorização: a consulta volta a aplicar o filtro de inquilino.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::error::DomainError;

pub const DEFAULT_PAGE_SIZE: u32 = 50;
pub const MAX_PAGE_SIZE: u32 = 100;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PageRequest {
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

impl PageRequest {
    /// Tamanho efectivo, preso a `1..=MAX_PAGE_SIZE`.
    pub fn size(&self) -> u32 {
        self.page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE)
    }

    /// O cursor descodificado, se houver. Um token corrompido é erro do
    /// cliente (400), nunca «primeira página» em silêncio.
    pub fn cursor<C: DeserializeOwned>(&self) -> Result<Option<C>, DomainError> {
        match self.page_token.as_deref() {
            None | Some("") => Ok(None),
            Some(t) => decode_cursor(t).map(Some),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

impl<T> Page<T> {
    /// Constrói a página a partir de `size + 1` linhas lidas: se veio a linha a
    /// mais, há página seguinte e o cursor sai da última linha MOSTRADA.
    pub fn from_overfetch<C: Serialize>(
        mut rows: Vec<T>,
        size: u32,
        cursor_of: impl Fn(&T) -> C,
    ) -> Self {
        let size = size as usize;
        let next_page_token = if rows.len() > size {
            rows.truncate(size);
            rows.last().map(|last| encode_cursor(&cursor_of(last)))
        } else {
            None
        };
        Page {
            items: rows,
            next_page_token,
        }
    }

    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Page<U> {
        Page {
            items: self.items.into_iter().map(f).collect(),
            next_page_token: self.next_page_token,
        }
    }
}

pub fn encode_cursor<C: Serialize>(c: &C) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(c).unwrap_or_default())
}

pub fn decode_cursor<C: DeserializeOwned>(token: &str) -> Result<C, DomainError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| DomainError::invalid("page.invalid_token", "page_token inválido"))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| DomainError::invalid("page.invalid_token", "page_token inválido"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Cur {
        at: i64,
        id: u32,
    }

    #[test]
    fn size_is_bounded() {
        assert_eq!(PageRequest::default().size(), 50);
        let r = PageRequest {
            page_size: Some(500),
            page_token: None,
        };
        assert_eq!(r.size(), 100);
        let r = PageRequest {
            page_size: Some(0),
            page_token: None,
        };
        assert_eq!(r.size(), 1);
    }

    #[test]
    fn overfetch_yields_token_from_last_shown() {
        let rows = vec![1u32, 2, 3];
        let p = Page::from_overfetch(rows, 2, |x| Cur { at: 0, id: *x });
        assert_eq!(p.items, vec![1, 2]);
        let c: Cur = decode_cursor(p.next_page_token.as_deref().unwrap()).unwrap();
        assert_eq!(c, Cur { at: 0, id: 2 });
        let p = Page::from_overfetch(vec![1u32, 2], 2, |x| Cur { at: 0, id: *x });
        assert!(p.next_page_token.is_none());
    }

    #[test]
    fn garbage_token_is_client_error() {
        let r = PageRequest {
            page_size: None,
            page_token: Some("%%%".into()),
        };
        let e = r.cursor::<Cur>().unwrap_err();
        assert_eq!(e.code, "page.invalid_token");
    }
}
