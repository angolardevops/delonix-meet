//! Quem liga por telefone, IDENTIFICADO (plano de produção, item 3.8, lote 2
//! — R279).
//!
//! O IVR sabe quem liga em dois casos: a chamada vem de um ramal registado
//! (o FreeSWITCH autenticou-o por digest), ou quem liga de fora marcou o seu
//! ramal e o PIN (`extension_pin::verify_from_call`). Em ambos é o SERVIDOR
//! que conclui a identidade — o Lua não é fonte de verdade de quem é quem.
//!
//! O que viaja até à ponte telefone↔sala não é o nome nem a pessoa: é um
//! bilhete opaco, de uso único e de vida curta, num cabeçalho SIP da perna
//! que o FreeSWITCH origina (`X-Delonix-Caller-Ticket`). A ponte troca-o pela
//! identidade ao sentar a chamada no censo. Só o hash fica na base.
//!
//! O bilhete vale para UMA sala (`room_code`): apresentado noutra, não
//! identifica ninguém. Sem bilhete, ou com um bilhete gasto, expirado ou de
//! outra sala, a chamada entra como sempre entrou — «Telefone», anónimo.

use uuid::Uuid;

use crate::{error::ApiError, AppState};

/// Quanto tempo o bilhete vale. Entre o IVR o receber e a ponte atender o
/// `INVITE` passam as frases de boas-vindas — segundos. Quarenta e cinco
/// cobrem um FreeSWITCH lento; mais do que isso era só tempo em que um
/// bilhete que não chegou à ponte (ela em baixo, a chamada caída na
/// conferência local) continuava a valer, escrito no log e no CDR do
/// FreeSWITCH. Se a ponte RECUSAR o `INVITE`, o bilhete é invalidado na hora
/// ([`discard`]).
const TICKET_TTL_SECS: f64 = 45.0;

/// A variável de canal que faz o FreeSWITCH pôr o cabeçalho na perna para a
/// ponte (`sip_h_<Cabeçalho>`). O Lua copia as `channel_vars` que o servidor
/// lhe dá sem as conhecer pelo nome.
pub(crate) fn channel_var() -> String {
    format!("sip_h_{}", crate::phone_bridge::sip::CALLER_TICKET_HEADER)
}

/// Quem o bilhete identifica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerIdentity {
    /// O nome com que entra no censo: o da pessoa, ou a etiqueta do ramal da
    /// empresa.
    pub display_name: String,
    /// A pessoa. `None` num ramal da empresa.
    pub member_id: Option<Uuid>,
}

/// Emite um bilhete para `room_code`. Devolve o valor em claro — sai UMA vez,
/// para o IVR.
pub(crate) async fn issue(
    state: &AppState,
    org_id: Uuid,
    room_code: &str,
    extension_id: Uuid,
    who: &CallerIdentity,
) -> Result<String, ApiError> {
    let token = delonix_meet_core::crypto::random_hex(32);
    // Os gastos e os expirados não ficam a acumular.
    sqlx::query("DELETE FROM voice_caller_tickets WHERE expires_at < now() - interval '1 hour'")
        .execute(&state.db)
        .await?;
    sqlx::query(
        "INSERT INTO voice_caller_tickets
             (token_hash, org_id, room_code, extension_id, member_id, display_name, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(secs => $7))",
    )
    .bind(delonix_meet_core::crypto::sha256_hex(&token))
    .bind(org_id)
    .bind(room_code)
    .bind(extension_id)
    .bind(who.member_id)
    .bind(who.display_name.chars().take(120).collect::<String>())
    .bind(TICKET_TTL_SECS)
    .execute(&state.db)
    .await?;
    Ok(token)
}

/// Troca o bilhete pela identidade, UMA vez, e só para a sala dele.
pub(crate) async fn redeem(
    db: &sqlx::PgPool,
    token: &str,
    room_code: &str,
) -> Option<CallerIdentity> {
    let row: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "UPDATE voice_caller_tickets SET used_at = now()
          WHERE token_hash = $1 AND room_code = $2 AND used_at IS NULL AND expires_at > now()
          RETURNING display_name, member_id",
    )
    .bind(delonix_meet_core::crypto::sha256_hex(token))
    .bind(room_code)
    .fetch_optional(db)
    .await
    .map_err(|e| tracing::warn!(error = %e, "bilhete de identidade da chamada: leitura falhou"))
    .ok()
    .flatten();
    row.map(|(display_name, member_id)| CallerIdentity {
        display_name,
        member_id,
    })
}

/// Invalida um bilhete que não entrou em sala nenhuma (a ponte recusou o
/// `INVITE` que o trazia). Idempotente; um bilhete que não existe não é erro.
pub(crate) async fn discard(db: &sqlx::PgPool, token: &str) {
    if let Err(e) = sqlx::query(
        "UPDATE voice_caller_tickets SET used_at = now(), expires_at = now()
          WHERE token_hash = $1 AND used_at IS NULL",
    )
    .bind(delonix_meet_core::crypto::sha256_hex(token))
    .execute(db)
    .await
    {
        tracing::warn!(error = %e, "bilhete de identidade da chamada: invalidação falhou");
    }
}
