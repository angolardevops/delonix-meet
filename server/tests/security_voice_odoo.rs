//! Ataques à telefonia (dial-in PSTN) e à integração Odoo, contra Postgres
//! real. Cada recusa vem com o controlo positivo (R51/R94): o mesmo pedido
//! funciona para quem tem direito — senão o teste mediria uma avaria.
mod common;

use common::{assert_denied, Account, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const VOICE_SECRET: &str = "segredo-da-media-0123456789";

/// DID dedicado de uma org, semeado por SQL (como `tests/grpc.rs`): o caminho
/// HTTP de criação de DIDs é ele próprio objecto de teste abaixo.
async fn seed_did(app: &TestApp, org: &str, e164: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO voice_did (org_id, e164) VALUES ($1::uuid, $2) RETURNING id")
        .bind(org)
        .bind(e164)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

async fn ivr_validate(app: &TestApp, did: &str, pin: &str) -> (u16, Value) {
    let r = app
        .raw(
            reqwest::Method::POST,
            "/api/voice/ivr/validate",
            &[("x-voice-secret", VOICE_SECRET)],
            Some(json!({"did_e164": did, "pin": pin})),
        )
        .await;
    (r.status, r.json())
}

async fn voice_room(app: &TestApp, who: &Account, code: &str) -> (u16, Value) {
    app.post(
        "/api/voice/rooms",
        Some(&who.token),
        json!({ "room_code": code }),
    )
    .await
}

/// R140 — o admin da org A ligava um DID+PIN da SUA org ao código de sala da
/// org B: o IVR punha então chamadores PSTN dentro da reunião de B.
#[sqlx::test(migrations = "./migrations")]
async fn voice_room_for_another_orgs_room_code_is_refused(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("VOICE_INTERNAL_SECRET", VOICE_SECRET)]).await;
    let a = app.new_org("alfa-voz.ao").await;
    let b = app.new_org("beta-voz.ao").await;
    seed_did(&app, a.org(), "+244222100001").await;
    seed_did(&app, b.org(), "+244222200002").await;
    let room_b = app.new_room(&b, "Conselho da B").await;
    let code_b = room_b["code"].as_str().unwrap();

    // Controlo positivo: B liga o dial-in à sua própria sala, e o IVR encontra-a.
    let (st, own) = voice_room(&app, &b, code_b).await;
    assert_eq!(st, 200, "{own}");
    let (st, ivr) = ivr_validate(
        &app,
        own["dial_in_number"].as_str().unwrap(),
        own["pin"].as_str().unwrap(),
    )
    .await;
    assert_eq!(st, 200, "{ivr}");
    assert_eq!(ivr["room_code"], code_b);

    // O ataque: A pede um dial-in para o código da sala de B.
    let (st, attack) = voice_room(&app, &a, code_b).await;
    assert_denied("A cria dial-in para a sala de B", st, &attack, "pin");
    assert_eq!(st, 404, "não revela que a sala existe: {attack}");
    assert_eq!(attack["code"], "voice.room_not_found", "{attack}");

    // A recusa é a mesma de um código inexistente (não distingue).
    let (st, missing) = voice_room(&app, &a, "nao-existe-nenhuma").await;
    assert_eq!((st, &missing["code"]), (404, &attack["code"]), "{missing}");

    // E nada ficou gravado para A.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_room WHERE org_id = $1::uuid")
        .bind(a.org())
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0, "a org A ficou com uma sala de voz para a sala de B");

    // Controlo positivo do lado de A: a sua própria sala continua a funcionar.
    let room_a = app.new_room(&a, "Sala da A").await;
    let (st, own_a) = voice_room(&app, &a, room_a["code"].as_str().unwrap()).await;
    assert_eq!(st, 200, "{own_a}");
}
