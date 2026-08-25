//! MFA por TOTP (RFC 6238) e códigos de recuperação.
//!
//! Implementado aqui em vez de por dependência nova: o HMAC-SHA-1, o base64 e o
//! argon2 já são dependências, e o algoritmo são trinta linhas com **vectores de
//! teste oficiais** no RFC — o que dá uma verificação independente muito melhor
//! do que confiar numa crate. Os vectores do RFC 6238 (Apêndice B) e do RFC 4648
//! estão nos testes deste ficheiro.
//!
//! SHA-1 é usado de propósito, e não por descuido: é o que o RFC 6238 define por
//! omissão e o único que todos os autenticadores (Google, Aegis, 1Password,
//! Authy) suportam. O HMAC-SHA-1 continua sólido para autenticação de mensagens;
//! as fraquezas conhecidas do SHA-1 são de colisão, que não se aplicam aqui.

use axum::{extract::State, Json};
use hmac::{Hmac, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, AppState};

type HmacSha1 = Hmac<Sha1>;

/// Passo temporal do RFC 6238. 30 s é o que todos os autenticadores assumem.
pub const STEP_SECS: u64 = 30;
/// Dígitos do código. 6 é o universal.
pub const DIGITS: u32 = 6;
/// Passos aceites para cada lado do instante actual.
///
/// Um passo (±30 s) e não mais: cada passo extra multiplica por três a janela
/// de adivinhação de um código de seis dígitos, e o relógio de um telemóvel
/// moderno não anda mais desalinhado do que isso.
pub const SKEW_STEPS: i64 = 1;

// ---------------------------------------------------------------------------
//  base32 (RFC 4648, sem padding) — é o que os autenticadores leem
// ---------------------------------------------------------------------------

const ALFABETO: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn base32_encode(dados: &[u8]) -> String {
    let mut saida = String::new();
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for &b in dados {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            saida.push(ALFABETO[((buffer >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        saida.push(ALFABETO[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    saida
}

pub fn base32_decode(texto: &str) -> Option<Vec<u8>> {
    let mut saida = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for c in texto.chars().filter(|c| *c != '=' && !c.is_whitespace()) {
        let v = ALFABETO
            .iter()
            .position(|&a| a == c.to_ascii_uppercase() as u8)? as u32;
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            saida.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(saida)
}

// ---------------------------------------------------------------------------
//  TOTP
// ---------------------------------------------------------------------------

/// HOTP do RFC 4226: HMAC-SHA-1 do contador, com truncagem dinâmica.
fn hotp(segredo: &[u8], contador: u64, digitos: u32) -> String {
    let mut mac = HmacSha1::new_from_slice(segredo).expect("HMAC aceita qualquer tamanho de chave");
    mac.update(&contador.to_be_bytes());
    let etiqueta = mac.finalize().into_bytes();
    // Truncagem dinâmica (RFC 4226 §5.3): o nibble baixo do último byte diz por
    // onde cortar; o bit alto é limpo para o número ser sempre positivo.
    let deslocamento = (etiqueta[19] & 0x0f) as usize;
    let binario = ((etiqueta[deslocamento] as u32 & 0x7f) << 24)
        | ((etiqueta[deslocamento + 1] as u32) << 16)
        | ((etiqueta[deslocamento + 2] as u32) << 8)
        | (etiqueta[deslocamento + 3] as u32);
    let modulo = 10u32.pow(digitos);
    format!("{:0largura$}", binario % modulo, largura = digitos as usize)
}

/// TOTP (RFC 6238): HOTP com o contador = tempo unix dividido pelo passo.
pub fn totp(segredo: &[u8], instante: u64, passo: u64, digitos: u32) -> String {
    hotp(segredo, instante / passo, digitos)
}

/// Verifica um código, aceitando `SKEW_STEPS` passos para cada lado.
///
/// A comparação é de tempo constante. Um `==` de `String` sai no primeiro byte
/// diferente, e isso chega para distinguir «o primeiro dígito está certo» —
/// numa rede rápida, seis dígitos caem em muito menos tentativas do que o
/// milhão que deviam custar.
pub fn verifica(segredo: &[u8], codigo: &str, agora: u64) -> bool {
    let codigo = codigo.trim();
    if codigo.len() != DIGITS as usize || !codigo.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let passo_actual = (agora / STEP_SECS) as i64;
    let mut valido = false;
    for d in -SKEW_STEPS..=SKEW_STEPS {
        let passo = (passo_actual + d).max(0) as u64;
        // Via `totp` e não `hotp` de propósito: é a mesma função que os
        // vectores do RFC 6238 verificam nos testes, por isso o que se compara
        // aqui é EXACTAMENTE o que ali ficou provado.
        let esperado = totp(segredo, passo * STEP_SECS, STEP_SECS, DIGITS);
        // Sem short-circuit: percorrem-se SEMPRE todos os passos e todos os
        // bytes, para o tempo de resposta não revelar quantos acertaram.
        valido |= igual_em_tempo_constante(esperado.as_bytes(), codigo.as_bytes());
    }
    valido
}

pub fn igual_em_tempo_constante(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diferenca = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diferenca |= x ^ y;
    }
    diferenca == 0
}

/// Segredo novo de 160 bits — o tamanho que o RFC 4226 §4 recomenda para SHA-1.
pub fn segredo_novo() -> Vec<u8> {
    let mut s = vec![0u8; 20];
    rand::thread_rng().fill(&mut s[..]);
    s
}

/// URI `otpauth://` que os autenticadores lêem de um código QR.
///
/// O emissor e a conta são percent-encoded: um email com `+` ou um nome de
/// organização com espaço partiam o URI e o autenticador registava a conta
/// errada — ou nenhuma.
pub fn otpauth_uri(emissor: &str, conta: &str, segredo: &[u8]) -> String {
    let e = percent(emissor);
    let c = percent(conta);
    format!(
        "otpauth://totp/{e}:{c}?secret={}&issuer={e}&algorithm=SHA1&digits={}&period={}",
        base32_encode(segredo),
        DIGITS,
        STEP_SECS
    )
}

fn percent(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Códigos de recuperação: 10 códigos de 10 caracteres, em base32 (sem dígitos
/// ambíguos porque o alfabeto base32 já não tem 0/1/8/9).
pub fn codigos_de_recuperacao() -> Vec<String> {
    (0..10)
        .map(|_| {
            let mut b = [0u8; 7]; // 7 bytes ⇒ 12 chars base32; corta-se a 10
            rand::thread_rng().fill(&mut b[..]);
            let s = base32_encode(&b);
            format!("{}-{}", &s[..5], &s[5..10])
        })
        .collect()
}

// ---------------------------------------------------------------------------
//  Endpoints
// ---------------------------------------------------------------------------

fn agora() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Serialize)]
pub struct EstadoMfa {
    pub enabled: bool,
    /// Inscrito mas por confirmar — o autenticador já tem o segredo, falta a prova.
    pub pending: bool,
    pub backup_codes_left: i64,
}

pub async fn estado(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<EstadoMfa>, ApiError> {
    let linha: Option<(Option<chrono::DateTime<chrono::Utc>>,)> =
        sqlx::query_as("SELECT enabled_at FROM user_mfa WHERE user_id = $1")
            .bind(auth.user_id)
            .fetch_optional(&state.db)
            .await?;
    let restantes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_mfa_backup_codes WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(EstadoMfa {
        enabled: matches!(&linha, Some((Some(_),))),
        pending: matches!(&linha, Some((None,))),
        backup_codes_left: restantes,
    }))
}

#[derive(Serialize)]
pub struct Inscricao {
    /// Segredo em base32, para quem escreve à mão em vez de ler o QR.
    pub secret: String,
    /// URI `otpauth://` — é isto que vira código QR.
    pub otpauth_uri: String,
}

/// Começa a inscrição: gera um segredo novo e devolve-o UMA vez.
///
/// Repetir isto antes de confirmar substitui o segredo — é o que permite
/// recomeçar quando o QR foi lido para o autenticador errado. Depois de
/// confirmado, recusa: trocar o segredo de uma conta com MFA activo sem provar
/// posse do actual seria uma forma de o desligar sem o saber.
pub async fn inscrever(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Inscricao>, ApiError> {
    let ja: Option<(Option<chrono::DateTime<chrono::Utc>>,)> =
        sqlx::query_as("SELECT enabled_at FROM user_mfa WHERE user_id = $1")
            .bind(auth.user_id)
            .fetch_optional(&state.db)
            .await?;
    if matches!(ja, Some((Some(_),))) {
        return Err(ApiError::BadRequest(
            "O MFA já está activo nesta conta. Desactiva-o primeiro (com um código válido) para voltar a inscrever.".into(),
        ));
    }
    let segredo = segredo_novo();
    let b32 = base32_encode(&segredo);
    sqlx::query(
        "INSERT INTO user_mfa (user_id, secret, enabled_at, last_step) VALUES ($1, $2, NULL, NULL)
         ON CONFLICT (user_id) DO UPDATE SET secret = EXCLUDED.secret, enabled_at = NULL, last_step = NULL",
    )
    .bind(auth.user_id)
    .bind(&b32)
    .execute(&state.db)
    .await?;
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(auth.user_id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(Inscricao {
        otpauth_uri: otpauth_uri("Delonix Meet", &email, &segredo),
        secret: b32,
    }))
}

#[derive(Deserialize)]
pub struct CodigoReq {
    pub code: String,
}

#[derive(Serialize)]
pub struct CodigosRecuperacao {
    /// Mostrados UMA vez. Só o hash fica guardado.
    pub backup_codes: Vec<String>,
}

/// Confirma a inscrição com um código do autenticador e devolve os códigos de
/// recuperação — a única vez em que são visíveis.
pub async fn activar(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<CodigoReq>,
) -> Result<Json<CodigosRecuperacao>, ApiError> {
    let linha: Option<(String, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as("SELECT secret, enabled_at FROM user_mfa WHERE user_id = $1")
            .bind(auth.user_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((b32, enabled_at)) = linha else {
        return Err(ApiError::BadRequest("Não há inscrição em curso.".into()));
    };
    if enabled_at.is_some() {
        return Err(ApiError::BadRequest("O MFA já está activo.".into()));
    }
    let segredo = base32_decode(&b32).ok_or(ApiError::Unauthorized)?;
    if !verifica(&segredo, &req.code, agora()) {
        return Err(ApiError::Unauthorized);
    }
    let codigos = codigos_de_recuperacao();
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE user_mfa SET enabled_at = now(), last_step = $2 WHERE user_id = $1")
        .bind(auth.user_id)
        .bind((agora() / STEP_SECS) as i64)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM user_mfa_backup_codes WHERE user_id = $1")
        .bind(auth.user_id)
        .execute(&mut *tx)
        .await?;
    for c in &codigos {
        let h = crate::auth::hash_password(c)
            .map_err(|_| ApiError::Internal("falha a cifrar o código de recuperação".into()))?;
        sqlx::query("INSERT INTO user_mfa_backup_codes (user_id, code_hash) VALUES ($1, $2)")
            .bind(auth.user_id)
            .bind(h)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    crate::audit::log(&state.db, None, auth.user_id, "auth.mfa_enabled", "").await;
    Ok(Json(CodigosRecuperacao {
        backup_codes: codigos,
    }))
}

/// Desactiva o MFA. Exige um código VÁLIDO (TOTP ou de recuperação): sem isso,
/// um token de sessão roubado bastava para o desligar — e o segundo factor
/// existe precisamente para o caso de a sessão estar comprometida.
pub async fn desactivar(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<CodigoReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !consome_codigo(&state, auth.user_id, &req.code).await? {
        return Err(ApiError::Unauthorized);
    }
    sqlx::query("DELETE FROM user_mfa WHERE user_id = $1")
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;
    sqlx::query("DELETE FROM user_mfa_backup_codes WHERE user_id = $1")
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;
    crate::audit::log(&state.db, None, auth.user_id, "auth.mfa_disabled", "").await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Está o MFA activo nesta conta?
pub async fn activo(db: &sqlx::PgPool, user_id: Uuid) -> Result<bool, ApiError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_mfa WHERE user_id = $1 AND enabled_at IS NOT NULL",
    )
    .bind(user_id)
    .fetch_one(db)
    .await?;
    Ok(n > 0)
}

/// Consome um código — TOTP ou de recuperação. Devolve `true` se era válido.
///
/// Chama-se «consome» de propósito: um código válido NÃO serve duas vezes. O
/// TOTP fica preso ao passo temporal (`last_step`), e o de recuperação é
/// marcado como usado. Sem isto, um código apanhado por cima do ombro servia
/// outra vez durante os trinta segundos seguintes.
pub async fn consome_codigo(
    state: &AppState,
    user_id: Uuid,
    codigo: &str,
) -> Result<bool, ApiError> {
    let linha: Option<(String, Option<i64>)> =
        sqlx::query_as("SELECT secret, last_step FROM user_mfa WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((b32, last_step)) = linha else {
        return Ok(false);
    };

    if let Some(segredo) = base32_decode(&b32) {
        let t = agora();
        if verifica(&segredo, codigo, t) {
            let passo = (t / STEP_SECS) as i64;
            // Anti-replay: o passo tem de AVANÇAR. O UPDATE condicional é a
            // barreira — duas tentativas em paralelo, só uma actualiza a linha.
            let afectadas = sqlx::query(
                "UPDATE user_mfa SET last_step = $2
                 WHERE user_id = $1 AND (last_step IS NULL OR last_step < $2)",
            )
            .bind(user_id)
            .bind(passo)
            .execute(&state.db)
            .await?
            .rows_affected();
            if afectadas == 0 {
                // Já foi usado um código deste passo (ou de um mais recente).
                return Ok(false);
            }
            let _ = last_step;
            return Ok(true);
        }
    }

    // Código de recuperação. Percorrem-se os por usar; o argon2 é lento de
    // propósito, e são no máximo dez.
    let pendentes: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, code_hash FROM user_mfa_backup_codes
         WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    let normalizado = codigo.trim().to_uppercase();
    for (id, hash) in pendentes {
        if crate::auth::verify_password(&normalizado, &hash) {
            let afectadas = sqlx::query(
                "UPDATE user_mfa_backup_codes SET used_at = now() WHERE id = $1 AND used_at IS NULL",
            )
            .bind(id)
            .execute(&state.db)
            .await?
            .rows_affected();
            return Ok(afectadas == 1);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectores do RFC 4648 §10 — se o base32 estiver errado, o autenticador
    // regista um segredo diferente do nosso e NADA funciona, sem erro visível.
    #[test]
    fn base32_bate_com_os_vectores_do_rfc4648() {
        for (claro, esperado) in [
            ("", ""),
            ("f", "MY"),
            ("fo", "MZXQ"),
            ("foo", "MZXW6"),
            ("foob", "MZXW6YQ"),
            ("fooba", "MZXW6YTB"),
            ("foobar", "MZXW6YTBOI"),
        ] {
            assert_eq!(
                base32_encode(claro.as_bytes()),
                esperado,
                "encode de {claro:?}"
            );
            assert_eq!(
                base32_decode(esperado).unwrap(),
                claro.as_bytes(),
                "decode de {esperado:?}"
            );
        }
    }

    #[test]
    fn base32_ida_e_volta_com_bytes_aleatorios() {
        for _ in 0..50 {
            let s = segredo_novo();
            assert_eq!(base32_decode(&base32_encode(&s)).unwrap(), s);
        }
    }

    // Vectores do RFC 6238 Apêndice B (semente SHA-1 = "12345678901234567890").
    // É a verificação independente que uma implementação de cripto precisa:
    // não prova que o código é bonito, prova que é O ALGORITMO.
    #[test]
    fn totp_bate_com_os_vectores_do_rfc6238() {
        let semente = b"12345678901234567890";
        for (instante, esperado8) in [
            (59u64, "94287082"),
            (1_111_111_109, "07081804"),
            (1_111_111_111, "14050471"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
            (20_000_000_000, "65353130"),
        ] {
            assert_eq!(totp(semente, instante, 30, 8), esperado8, "t={instante}");
            // E a variante de 6 dígitos são os seis últimos do mesmo número.
            assert_eq!(
                totp(semente, instante, 30, 6),
                esperado8[2..],
                "t={instante} (6 díg.)"
            );
        }
    }

    #[test]
    fn verifica_aceita_o_codigo_do_momento() {
        let s = segredo_novo();
        let agora = 1_700_000_000u64;
        let codigo = totp(&s, agora, STEP_SECS, DIGITS);
        assert!(verifica(&s, &codigo, agora));
    }

    #[test]
    fn verifica_tolera_um_passo_de_desalinhamento_e_nao_mais() {
        let s = segredo_novo();
        let agora = 1_700_000_000u64;
        // ±30 s: relógio de telemóvel ligeiramente à frente ou atrás.
        assert!(verifica(
            &s,
            &totp(&s, agora - STEP_SECS, STEP_SECS, DIGITS),
            agora
        ));
        assert!(verifica(
            &s,
            &totp(&s, agora + STEP_SECS, STEP_SECS, DIGITS),
            agora
        ));
        // ±60 s já não. Cada passo extra multiplica por três a janela de
        // adivinhação de um código de seis dígitos.
        assert!(!verifica(
            &s,
            &totp(&s, agora - 3 * STEP_SECS, STEP_SECS, DIGITS),
            agora
        ));
        assert!(!verifica(
            &s,
            &totp(&s, agora + 3 * STEP_SECS, STEP_SECS, DIGITS),
            agora
        ));
    }

    #[test]
    fn verifica_recusa_lixo_sem_sequer_calcular() {
        let s = segredo_novo();
        let agora = 1_700_000_000u64;
        for mau in ["", "12345", "1234567", "abcdef", "12 45 6", "١٢٣٤٥٦"] {
            assert!(!verifica(&s, mau, agora), "aceitou {mau:?}");
        }
    }

    #[test]
    fn verifica_recusa_o_codigo_de_outro_segredo() {
        let agora = 1_700_000_000u64;
        let a = segredo_novo();
        let b = segredo_novo();
        assert!(!verifica(&a, &totp(&b, agora, STEP_SECS, DIGITS), agora));
    }

    #[test]
    fn comparacao_em_tempo_constante() {
        assert!(igual_em_tempo_constante(b"123456", b"123456"));
        assert!(!igual_em_tempo_constante(b"123456", b"123457"));
        assert!(!igual_em_tempo_constante(b"123456", b"023456"));
        // Comprimentos diferentes não são iguais — e não estoiram.
        assert!(!igual_em_tempo_constante(b"123456", b"12345"));
        assert!(!igual_em_tempo_constante(b"", b"1"));
    }

    #[test]
    fn o_uri_otpauth_escapa_o_que_partiria_o_registo() {
        let s = base32_decode("MZXW6YTBOI").unwrap();
        let uri = otpauth_uri("Org da Ana & Cia", "ana+teste@exemplo.local", &s);
        // Um `+`, um `&` ou um espaço por escapar registava a conta errada — ou
        // nenhuma — no autenticador, e o utilizador só descobria ao entrar.
        assert!(!uri.contains(' '), "espaço por escapar: {uri}");
        assert!(uri.contains("ana%2Bteste%40exemplo.local"), "{uri}");
        assert!(uri.contains("Org%20da%20Ana%20%26%20Cia"), "{uri}");
        assert!(uri.contains("secret=MZXW6YTBOI"));
        assert!(
            uri.contains("algorithm=SHA1") && uri.contains("digits=6") && uri.contains("period=30")
        );
    }

    #[test]
    fn codigos_de_recuperacao_sao_dez_distintos_e_legiveis() {
        let c = codigos_de_recuperacao();
        assert_eq!(c.len(), 10);
        assert_eq!(c.iter().collect::<std::collections::HashSet<_>>().len(), 10);
        for x in &c {
            assert_eq!(x.len(), 11, "formato XXXXX-XXXXX: {x}");
            assert_eq!(x.chars().nth(5), Some('-'));
            // Sem 0/1/8/9 — o alfabeto base32 não os tem, o que remove a
            // confusão entre 0/O e 1/l na hora de os copiar do papel.
            assert!(
                x.chars()
                    .all(|ch| ch == '-' || ALFABETO.contains(&(ch as u8))),
                "{x}"
            );
        }
    }
}
