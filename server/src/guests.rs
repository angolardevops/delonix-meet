//! Entrada de CONVIDADO SEM CONTA numa reunião.
//!
//! `POST /api/rooms/{code}/guest-join` com `{"display_name": "…"}`. É a única
//! rota pública que dá acesso a uma reunião, e o desenho é o de uma porta com
//! campainha, não o de uma porta aberta:
//!
//! - **Sempre pela sala de espera.** O token sai com `wait: true` e
//!   `origin: "guest"`, e o `signaling::seat_policy` força a espera a partir da
//!   ORIGEM — não confia só no `wait`. Não há caminho em que um convidado entre
//!   sem que alguém com poder de admitir o deixe.
//! - **Sem conta.** O `sub` do token é um UUID gerado para ESTA entrada, que não
//!   existe em `users`. O token é `typ: "room"`, que nenhum extractor da API
//!   aceita (`AuthUser` exige `access`): gravações, chat guardado, actas,
//!   quadros, convites e o resto de `/api/*` ficam fora de alcance por
//!   construção, não por uma lista de exclusões.
//! - **Nunca anfitrião.** O hub recusa passar o papel a um convidado
//!   (`TransferHost`) e o directo recusa tokens de convidado.
//! - **A sala decide.** `rooms.allow_guests = false` → `403` antes de emitir
//!   seja o que for.
//! - **Travão por IP e por sala** (`GUEST_JOIN_PER_IP_PER_MIN`,
//!   `GUEST_JOIN_PER_ROOM_PER_MIN`), com `429` + `Retry-After`.
//! - **Auditado** como `room.guest_join` na org do dono da sala, com o código
//!   da sala e o nome — e nada mais sobre a pessoa.
//!
//! Salas com E2EE: a frase-passe continua a ser precisa e continua a nunca
//! passar pelo servidor. Um convidado sem ela entra na sala e vê frames que não
//! consegue decifrar — exactamente o que acontece a um membro sem ela.

use axum::{
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{net::SocketAddr, sync::Arc};
use uuid::Uuid;

use crate::{
    auth::{sign_jwt, Claims, ORIGIN_GUEST},
    error::ApiError,
    rooms::Room,
    AppState,
};

/// Comprimento máximo do nome, em caracteres (não bytes: «João» são 4).
pub const DISPLAY_NAME_MAX_CHARS: usize = 60;

/// Janela dos travões, em segundos. É também o `Retry-After`.
const LIMIT_WINDOW_SECS: u64 = 60;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestJoinReq {
    pub display_name: String,
}

/// O que um convidado vê da sala. Menos do que `Room`: o `owner_id` e o
/// `created_at` não são da conta de quem só tem o link.
#[derive(Debug, Serialize)]
pub struct GuestRoomView {
    pub code: String,
    pub name: String,
    pub topology: String,
    pub e2ee: bool,
    pub format: String,
}

impl From<&Room> for GuestRoomView {
    fn from(r: &Room) -> Self {
        Self {
            code: r.code.clone(),
            name: r.name.clone(),
            topology: r.topology.clone(),
            e2ee: r.e2ee,
            format: r.format.clone(),
        }
    }
}

/// Valida e normaliza o nome que o convidado escreveu.
///
/// Recusa (não limpa em silêncio) o que é controlo ou formatação invisível:
/// um `U+202E` inverte a leitura do nome no ecrã do anfitrião e deixa alguém
/// apresentar-se como «oirártsinimdA», e um `\n` parte a linha da auditoria.
/// Espaços repetidos colapsam-se — não enganam ninguém, só estragam o layout.
pub fn validate_display_name(raw: &str) -> Result<String, ApiError> {
    if raw.chars().any(is_forbidden_char) {
        return Err(ApiError::BadRequest(
            "display_name não pode ter caracteres de controlo ou invisíveis".into(),
        ));
    }
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let len = name.chars().count();
    if len == 0 || len > DISPLAY_NAME_MAX_CHARS {
        return Err(ApiError::BadRequest(format!(
            "display_name tem de ter 1–{DISPLAY_NAME_MAX_CHARS} caracteres"
        )));
    }
    Ok(name)
}

/// Controlo (C0/C1, que inclui `\n` e `\t`) e formatação invisível: marcas e
/// sobreposições de direcção, larguras zero, separadores de linha/parágrafo e
/// o BOM.
fn is_forbidden_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{2069}'
                | '\u{FEFF}'
                | '\u{061C}'
        )
}

/// A sala aceita esta entrada? Separado do handler para a regra ser testada
/// sem base de dados.
pub fn admission(room: &Room) -> Result<(), ApiError> {
    if !room.allow_guests {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

/// Os claims de um convidado. Tudo o que dá poder vai a `false`, e a espera a
/// `true` — o `signaling` volta a impô-lo a partir da origem.
pub fn guest_claims(room: &Room, guest_id: Uuid, name: &str, now: i64, ttl: i64) -> Claims {
    Claims {
        sub: guest_id,
        typ: "room".into(),
        iat: now,
        exp: now + ttl,
        room: Some(room.id),
        name: Some(name.to_string()),
        topo: Some(room.topology.clone()),
        owner: false,
        wait: true,
        adm: false,
        is_bot: false,
        origin: Some(ORIGIN_GUEST.into()),
    }
}

fn rate_limited() -> ApiError {
    ApiError::RateLimited {
        retry_after_secs: LIMIT_WINDOW_SECS,
    }
}

/// `POST /api/rooms/{code}/guest-join` — público por desenho.
pub async fn guest_join(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Json(req): Json<GuestJoinReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Travão por IP ANTES de qualquer leitura: é também o que torna caro
    // adivinhar códigos de sala por esta rota.
    let ip = crate::rate_limit::client_ip(&headers, addr.ip());
    if !state.guest_ip_limiter.check(&ip) {
        return Err(rate_limited());
    }
    let name = validate_display_name(&req.display_name)?;
    let room = crate::rooms::find_room(&state.db, &code)
        .await?
        .ok_or(ApiError::NotFound)?;
    admission(&room)?;
    // Por sala DEPOIS de saber que a sala existe: a chave do travão é o id de
    // uma sala real, e um atacante não enche o mapa com códigos inventados.
    if !state.guest_room_limiter.check(&room.id.to_string()) {
        return Err(rate_limited());
    }

    let guest_id = Uuid::new_v4();
    let ttl = state.config.room_token_ttl_secs;
    let room_token = sign_jwt(
        &state.config.jwt_secret,
        &guest_claims(&room, guest_id, &name, Utc::now().timestamp(), ttl),
    )?;
    let ice_servers = crate::rooms::ice_config(&state)?;

    crate::audit::log_guest(
        &state.db,
        Some(&state.metrics),
        room.owner_id,
        guest_id,
        &name,
        "room.guest_join",
        &room.code,
    )
    .await;
    tracing::info!(room = %room.code, %guest_id, "guest join token issued");

    Ok(Json(json!({
        "room": GuestRoomView::from(&room),
        "room_token": room_token,
        "ws_path": format!("/ws?token={room_token}"),
        "expires_in": ttl,
        "guest": { "id": guest_id, "display_name": name },
        "ice_servers": ice_servers,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::verify_jwt;
    use crate::rate_limit::RateLimiter;

    fn room(allow_guests: bool) -> Room {
        Room {
            id: Uuid::new_v4(),
            code: "abc-defg-hij".into(),
            name: "Reunião".into(),
            owner_id: Uuid::new_v4(),
            topology: "sfu".into(),
            waiting_room: false,
            e2ee: false,
            format: "normal".into(),
            allow_guests,
            created_at: Utc::now(),
        }
    }

    // ---- nome ----

    #[test]
    fn nome_valido_e_normalizado() {
        assert_eq!(
            validate_display_name("  Ana   Maria ").unwrap(),
            "Ana Maria"
        );
        assert_eq!(validate_display_name("João").unwrap(), "João");
        // 60 caracteres (não bytes) passam: «ã» são dois bytes.
        let sessenta = "ã".repeat(60);
        assert_eq!(validate_display_name(&sessenta).unwrap(), sessenta);
    }

    #[test]
    fn nome_vazio_ou_longo_e_recusado() {
        for n in ["", "   ", &"a".repeat(61)] {
            assert!(
                matches!(validate_display_name(n), Err(ApiError::BadRequest(_))),
                "{n:?} devia ser recusado"
            );
        }
    }

    #[test]
    fn nome_com_controlo_ou_invisiveis_e_recusado() {
        // `\n` partia a linha da auditoria; U+202E invertia o nome no ecrã do
        // anfitrião; U+200B faz dois nomes iguais parecerem diferentes.
        for n in [
            "Ana\nAdmin",
            "Ana\tB",
            "\u{202E}nimdA",
            "An\u{200B}a",
            "\u{FEFF}Ana",
            "A\u{0007}",
            "A\u{2066}B",
        ] {
            assert!(
                matches!(validate_display_name(n), Err(ApiError::BadRequest(_))),
                "{n:?} devia ser recusado"
            );
        }
    }

    // ---- admissão ----

    #[test]
    fn sala_sem_convidados_da_403() {
        assert!(matches!(admission(&room(false)), Err(ApiError::Forbidden)));
        assert!(admission(&room(true)).is_ok());
    }

    // ---- token ----

    #[test]
    fn o_token_de_convidado_espera_e_nao_da_poder() {
        let r = room(true);
        let g = Uuid::new_v4();
        let c = guest_claims(&r, g, "Ana", 1_000, 300);
        assert!(c.is_guest());
        assert!(c.wait, "um convidado passa SEMPRE pela sala de espera");
        assert!(!c.owner && !c.adm && !c.is_bot);
        assert_eq!(c.sub, g);
        assert_ne!(c.sub, r.owner_id);
        assert_eq!(c.room, Some(r.id));
        assert_eq!(c.exp - c.iat, 300, "efémero: o TTL do token de sala");
    }

    #[test]
    fn o_token_de_convidado_nao_abre_a_api() {
        // Gravações, chat guardado, actas, quadros e convites estão atrás do
        // `AuthUser`, que só aceita `typ: "access"`. Se isto passasse, o
        // convidado lia tudo isso com o token que recebeu à porta.
        let secret = "segredo-de-teste-com-mais-de-32-bytes!!";
        let now = Utc::now().timestamp();
        let tok = sign_jwt(
            secret,
            &guest_claims(&room(true), Uuid::new_v4(), "Ana", now, 300),
        )
        .unwrap();
        assert!(verify_jwt(secret, &tok, "access").is_err());
        let back = verify_jwt(secret, &tok, "room").unwrap();
        assert!(back.is_guest(), "a origem sobrevive à ida e volta pelo JWT");
    }

    #[test]
    fn um_token_de_membro_nao_e_convidado() {
        // Tokens antigos (sem `origin`) continuam a ser de membros.
        let json = r#"{"sub":"00000000-0000-0000-0000-000000000001","typ":"room","exp":1,"iat":0}"#;
        let c: Claims = serde_json::from_str(json).unwrap();
        assert!(!c.is_guest());
    }

    // ---- travão ----

    #[test]
    fn o_travao_por_ip_e_por_sala_sao_independentes() {
        // O mesmo `RateLimiter` que o handler usa, com as chaves que o handler
        // usa. Um IP esgotado não bloqueia outro; uma sala esgotada não
        // bloqueia outra.
        let por_ip = RateLimiter::new(3, std::time::Duration::from_secs(60));
        for _ in 0..3 {
            assert!(por_ip.check("203.0.113.7"));
        }
        assert!(
            !por_ip.check("203.0.113.7"),
            "o 4.º pedido do mesmo IP é travado"
        );
        assert!(por_ip.check("203.0.113.8"), "outro IP continua a passar");

        let por_sala = RateLimiter::new(2, std::time::Duration::from_secs(60));
        let (a, b) = (Uuid::new_v4().to_string(), Uuid::new_v4().to_string());
        assert!(por_sala.check(&a) && por_sala.check(&a));
        assert!(!por_sala.check(&a));
        assert!(por_sala.check(&b));
    }

    #[test]
    fn o_travao_responde_429_com_retry_after() {
        use axum::response::IntoResponse;
        let res = rate_limited().into_response();
        assert_eq!(res.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            res.headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok()),
            Some("60")
        );
    }

    #[test]
    fn a_vista_do_convidado_nao_traz_o_dono() {
        let v = serde_json::to_value(GuestRoomView::from(&room(true))).unwrap();
        assert!(v.get("owner_id").is_none());
        assert!(v.get("allow_guests").is_none());
        assert_eq!(v["code"], "abc-defg-hij");
    }
}
