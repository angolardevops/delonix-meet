//! Regras de campo partilhadas (ADR-0004, Fase 2; contexto identity, ADR-0006). Antes disto, email e
//! password eram validados de forma diferente em três sítios —
//! `auth::register` exigia `8..=128` na password e email com `@`+254
//! caracteres; `users::update_me` só verificava `>= 8` (sem tecto);
//! `org::add_employee` repetia o `8..=128` da password mas validava o email
//! sem o limite de 254. Nenhuma das três tinha uma razão para divergir das
//! outras — só divergiram porque cada handler escreveu a sua própria regra.
//!
//! Funções puras de propósito: sem `AppState`, sem SQL, testáveis sem
//! servidor. Devolvem `String` como mensagem de erro; quem chama mapeia para
//! `ApiError::BadRequest`.

pub const PASSWORD_MIN: usize = 8;
pub const PASSWORD_MAX: usize = 128;
const EMAIL_MAX: usize = 254;

/// `trim` + minúsculas — a normalização que todos os sítios já faziam antes
/// de validar. Não valida; só canoniza.
pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Assume um email já normalizado (`normalize_email`). Regra: tem de conter
/// `@` e caber em 254 caracteres (RFC 5321).
pub fn validate_email(email: &str) -> Result<(), String> {
    if !email.contains('@') || email.len() > EMAIL_MAX {
        return Err("email inválido".into());
    }
    Ok(())
}

/// Regra adicional do registo público: o domínio do email tem de parecer
/// corporativo (não vazio, com pelo menos um ponto) — é o que se torna o
/// domínio da organização. Devolve o domínio extraído.
pub fn require_corporate_domain(email: &str) -> Result<String, String> {
    let domain = email.split('@').nth(1).unwrap_or("").to_string();
    if domain.is_empty() || !domain.contains('.') {
        return Err("email corporativo inválido".into());
    }
    Ok(domain)
}

/// Política de password única para toda a app: 8-128 caracteres. O tecto
/// existe também para não entregar strings arbitrariamente grandes ao
/// Argon2 (custo de hashing cresce com o tamanho da entrada).
pub fn validate_password(password: &str) -> Result<(), String> {
    if !(PASSWORD_MIN..=PASSWORD_MAX).contains(&password.len()) {
        return Err(format!(
            "password deve ter {PASSWORD_MIN}-{PASSWORD_MAX} caracteres"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_and_lowercases() {
        assert_eq!(normalize_email("  Ana@Empresa.PT "), "ana@empresa.pt");
    }

    #[test]
    fn email_needs_at_sign() {
        assert!(validate_email("sem-arroba.pt").is_err());
        assert!(validate_email("a@b.pt").is_ok());
    }

    #[test]
    fn email_boundary_254() {
        let local = "a".repeat(254 - "@b.pt".len());
        let ok = format!("{local}@b.pt");
        assert_eq!(ok.len(), 254);
        assert!(validate_email(&ok).is_ok());
        let too_long = format!("x{ok}");
        assert!(validate_email(&too_long).is_err());
    }

    #[test]
    fn corporate_domain_needs_a_dot() {
        assert!(require_corporate_domain("a@localhost").is_err());
        assert!(require_corporate_domain("a@empresa.pt").is_ok());
        assert_eq!(
            require_corporate_domain("a@empresa.pt").unwrap(),
            "empresa.pt"
        );
    }

    #[test]
    fn password_boundary_8_and_128() {
        assert!(validate_password(&"a".repeat(7)).is_err());
        assert!(validate_password(&"a".repeat(8)).is_ok());
        assert!(validate_password(&"a".repeat(128)).is_ok());
        assert!(validate_password(&"a".repeat(129)).is_err());
    }
}
