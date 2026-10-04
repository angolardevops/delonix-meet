//! ADR-0016 — a central (PBX) de uma organização liga-se ao bordo, autentica-se
//! com a conta SIP dessa organização e entra numa sala pelo IVR. Contra
//! Postgres real.
//!
//! O que se mede aqui é a REGRA do servidor: que conta o bordo consegue
//! verificar, e em que salas o IVR deixa a central entrar. Não se mede a
//! chamada: nem o Kamailio nem o FreeSWITCH correm nestes testes — isso é do
//! `scripts/pbx-tronco-prova.sh`, fora do CI.
mod common;

use common::{Account, TestApp};
use md5::{Digest as _, Md5};
use serde_json::{json, Value};

const VOICE_SECRET: &str = "segredo-da-media-0123456789";
const ADVERTISE: &str = "ponte.teste:5099";

async fn spawn(db: sqlx::PgPool) -> TestApp {
    TestApp::spawn_with(
        db,
        &[
            ("VOICE_INTERNAL_SECRET", VOICE_SECRET),
            // A ponte «configurada», para o `room_bridge` vir na resposta. O UA
            // SIP não arranca nos testes — só se lê a configuração.
            ("PHONE_BRIDGE_SIP_BIND", "127.0.0.1:5099"),
            ("PHONE_BRIDGE_FREESWITCH_IPS", "127.0.0.1"),
            ("PHONE_BRIDGE_SIP_ADVERTISE", ADVERTISE),
        ],
    )
    .await
}

fn ha1(username: &str, realm: &str, password: &str) -> String {
    let mut h = Md5::new();
    h.update(format!("{username}:{realm}:{password}").as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// O «Registo SIP» da organização, como o administrador o grava.
async fn conta_sip(app: &TestApp, admin: &Account, domain: &str, user: &str, password: &str) {
    let (st, body) = app
        .put(
            &format!("/api/orgs/{}/telephony/sip-settings", admin.org()),
            Some(&admin.token),
            json!({"domain": domain, "transport": "tls", "srtp": "mandatory",
                   "username": user, "password": password}),
        )
        .await;
    assert_eq!(st, 200, "gravar a conta SIP de {domain}: {body}");
}

/// O que o bordo (Kamailio, `http_client`) pergunta: devolve o estado e o
/// corpo em texto.
async fn bordo(app: &TestApp, secret: Option<&str>, domain: &str, user: &str) -> (u16, String) {
    let headers: Vec<(&str, &str)> = secret.map(|s| ("x-voice-secret", s)).into_iter().collect();
    let r = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/telephony/edge/sip-account",
            &headers,
            Some(json!({"domain": domain, "username": user})),
        )
        .await;
    (r.status, r.text)
}

/// O que o `dialin_ivr.lua` envia quando a chamada traz `X-Delonix-Central`.
async fn ivr(app: &TestApp, secret: Option<&str>, domain: &str, pin: &str) -> (u16, Value) {
    let headers: Vec<(&str, &str)> = secret.map(|s| ("x-voice-secret", s)).into_iter().collect();
    let r = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/voice/ivr/validate-central",
            &headers,
            Some(json!({"domain": domain, "pin": pin})),
        )
        .await;
    (r.status, r.json())
}

/// Uma org com DID, uma sala e a sala de voz dela: devolve `(código, PIN)`.
async fn sala_com_pin(app: &TestApp, owner: &Account, e164: &str) -> (String, String) {
    sqlx::query("INSERT INTO voice_did (org_id, e164) VALUES ($1::uuid, $2)")
        .bind(owner.org())
        .bind(e164)
        .execute(&app.db)
        .await
        .unwrap();
    let room = app.new_room(owner, "Sala com telefone").await;
    let code = room["code"].as_str().unwrap().to_string();
    let (st, vr) = app
        .post(
            &format!("/api/orgs/{}/voice/rooms", owner.org()),
            Some(&owner.token),
            json!({"room_code": code}),
        )
        .await;
    assert_eq!(st, 200, "criar sala de voz: {vr}");
    (code, vr["pin"].as_str().unwrap().to_string())
}

fn outro_pin(pin: &str) -> &'static str {
    if pin == "000000" {
        "000001"
    } else {
        "000000"
    }
}

/// O bordo recebe o HA1 da conta certa — e só dela. É com ele que o Kamailio
/// verifica o digest: se aqui saísse o de outra conta, a central de uma
/// organização autenticava-se como outra.
#[sqlx::test(migrations = "./migrations")]
async fn o_bordo_recebe_o_ha1_da_conta_sip_e_so_dela(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-central.ao").await;
    let b = app.new_org("beta-central.ao").await;
    conta_sip(
        &app,
        &a,
        "pbx.alfa.example",
        "central-alfa",
        "password-da-alfa-1",
    )
    .await;
    conta_sip(
        &app,
        &b,
        "pbx.beta.example",
        "central-beta",
        "password-da-beta-2",
    )
    .await;

    let (st, body) = bordo(&app, Some(VOICE_SECRET), "pbx.alfa.example", "central-alfa").await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        body.trim(),
        ha1("central-alfa", "pbx.alfa.example", "password-da-alfa-1")
    );
    assert!(
        !body.contains("password-da-alfa"),
        "a password saiu em claro: {body}"
    );

    // O realm entra no HA1 tal como veio: quem ligou calculou a resposta com o
    // realm do desafio, letra por letra.
    let (st, body) = bordo(&app, Some(VOICE_SECRET), "PBX.Alfa.Example", "central-alfa").await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(
        body.trim(),
        ha1("central-alfa", "PBX.Alfa.Example", "password-da-alfa-1")
    );

    // Todas as recusas são o mesmo 404, sem HA1 nenhum no corpo.
    for (domain, user, porque) in [
        (
            "pbx.alfa.example",
            "central-beta",
            "o utilizador de outra conta",
        ),
        (
            "pbx.beta.example",
            "central-alfa",
            "o domínio de outra conta",
        ),
        (
            "pbx.alfa.example",
            "Central-Alfa",
            "o utilizador noutra caixa",
        ),
        (
            "pbx.ninguem.example",
            "central-alfa",
            "um domínio de ninguém",
        ),
        ("pbx.alfa.example", "", "sem utilizador"),
    ] {
        let (st, body) = bordo(&app, Some(VOICE_SECRET), domain, user).await;
        assert_eq!(st, 404, "{porque}: {body}");
        assert!(
            !body.contains(&ha1(
                "central-alfa",
                "pbx.alfa.example",
                "password-da-alfa-1"
            )) && !body.contains(&ha1(
                "central-beta",
                "pbx.beta.example",
                "password-da-beta-2"
            )),
            "{porque}: a recusa traz um HA1: {body}"
        );
    }

    // Sem o segredo da media, ninguém lê um HA1.
    let (st, body) = bordo(&app, None, "pbx.alfa.example", "central-alfa").await;
    assert_eq!(st, 401, "{body}");
    let (st, body) = bordo(
        &app,
        Some("outro-segredo-qualquer-012345"),
        "pbx.alfa.example",
        "central-alfa",
    )
    .await;
    assert_eq!(st, 401, "{body}");
}

/// Uma conta sem password não autentica ninguém; apagar a password fecha a
/// porta a uma central que já entrava.
#[sqlx::test(migrations = "./migrations")]
async fn conta_sem_password_nao_autentica_nem_entra(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-central.ao").await;
    let (_code, pin) = sala_com_pin(&app, &a, "+244222400001").await;
    conta_sip(
        &app,
        &a,
        "pbx.alfa.example",
        "central-alfa",
        "password-da-alfa-1",
    )
    .await;

    // Controlo positivo: com a conta completa, autentica e entra.
    let (st, _) = bordo(&app, Some(VOICE_SECRET), "pbx.alfa.example", "central-alfa").await;
    assert_eq!(st, 200);
    let (st, body) = ivr(&app, Some(VOICE_SECRET), "pbx.alfa.example", &pin).await;
    assert_eq!(st, 200, "{body}");

    // `""` apaga a password (contrato do PUT).
    conta_sip(&app, &a, "pbx.alfa.example", "central-alfa", "").await;
    let (st, body) = bordo(&app, Some(VOICE_SECRET), "pbx.alfa.example", "central-alfa").await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = ivr(&app, Some(VOICE_SECRET), "pbx.alfa.example", &pin).await;
    assert_eq!(st, 404, "uma conta sem password ainda abre salas: {body}");
}

/// O PIN certo de uma sala da organização da central devolve a ponte
/// telefone↔sala — a mesma que o dial-in por DID devolve.
#[sqlx::test(migrations = "./migrations")]
async fn central_com_o_pin_da_sua_org_recebe_a_ponte(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-central.ao").await;
    let (code, pin) = sala_com_pin(&app, &a, "+244222400002").await;
    conta_sip(
        &app,
        &a,
        "pbx.alfa.example",
        "central-alfa",
        "password-da-alfa-1",
    )
    .await;

    let (st, body) = ivr(&app, Some(VOICE_SECRET), "pbx.alfa.example", &pin).await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["room_code"], code.as_str());
    assert_eq!(
        body["room_bridge"]["sip_uri"],
        format!("sip:room-{code}@{ADVERTISE}"),
        "{body}"
    );

    // É a MESMA ponte do dial-in: o caminho por DID devolve o mesmo objecto.
    let did = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/voice/ivr/validate",
            &[("x-voice-secret", VOICE_SECRET)],
            Some(json!({"did_e164": "+244222400002", "pin": pin})),
        )
        .await;
    assert_eq!(did.status, 200);
    assert_eq!(did.json()["room_bridge"], body["room_bridge"]);

    // O domínio noutra caixa é o mesmo domínio.
    let (st, body) = ivr(&app, Some(VOICE_SECRET), "PBX.Alfa.Example", &pin).await;
    assert_eq!(st, 200, "{body}");

    // PIN errado: recusado. Sem o segredo: recusado.
    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        "pbx.alfa.example",
        outro_pin(&pin),
    )
    .await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = ivr(&app, None, "pbx.alfa.example", &pin).await;
    assert_eq!(st, 401, "{body}");
}

/// O isolamento: a central da org A não entra numa sala da org B, mesmo
/// sabendo o PIN. Controlo positivo: o MESMO PIN abre a sala à central de B.
#[sqlx::test(migrations = "./migrations")]
async fn central_de_outra_org_nao_entra_mesmo_com_o_pin(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-central.ao").await;
    let b = app.new_org("beta-central.ao").await;
    let (code_b, pin_b) = sala_com_pin(&app, &b, "+244222400003").await;
    conta_sip(
        &app,
        &a,
        "pbx.alfa.example",
        "central-alfa",
        "password-da-alfa-1",
    )
    .await;
    conta_sip(
        &app,
        &b,
        "pbx.beta.example",
        "central-beta",
        "password-da-beta-2",
    )
    .await;

    let (st, body) = ivr(&app, Some(VOICE_SECRET), "pbx.beta.example", &pin_b).await;
    assert_eq!(st, 200, "controlo positivo: {body}");
    assert_eq!(body["room_code"], code_b.as_str());

    let (st, body) = ivr(&app, Some(VOICE_SECRET), "pbx.alfa.example", &pin_b).await;
    assert_eq!(st, 404, "a central de A entrou numa sala de B: {body}");
    assert!(
        !body.to_string().contains(&code_b),
        "a recusa revela a sala de B: {body}"
    );

    // Um domínio de ninguém não entra em lado nenhum.
    let (st, body) = ivr(&app, Some(VOICE_SECRET), "pbx.ninguem.example", &pin_b).await;
    assert_eq!(st, 404, "{body}");
}

/// O PIN só é único por DID. Com o mesmo PIN em duas salas da organização, a
/// central (que não traz DID) não tem como desempatar: recusa-se, não se
/// escolhe uma.
#[sqlx::test(migrations = "./migrations")]
async fn pin_em_duas_salas_da_org_e_recusado_a_central(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-central.ao").await;
    let (code1, pin) = sala_com_pin(&app, &a, "+244222400004").await;
    conta_sip(
        &app,
        &a,
        "pbx.alfa.example",
        "central-alfa",
        "password-da-alfa-1",
    )
    .await;

    let (st, body) = ivr(&app, Some(VOICE_SECRET), "pbx.alfa.example", &pin).await;
    assert_eq!(st, 200, "controlo positivo, uma só sala com o PIN: {body}");

    // Uma segunda sala, noutro DID, com o MESMO PIN.
    let did2: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO voice_did (org_id, e164) VALUES ($1::uuid, '+244222400005') RETURNING id",
    )
    .bind(a.org())
    .fetch_one(&app.db)
    .await
    .unwrap();
    let room2 = app.new_room(&a, "Outra sala").await;
    sqlx::query(
        "INSERT INTO voice_room (org_id, room_code, pin, did_id, created_by)
         VALUES ($1::uuid, $2, $3, $4, $5::uuid)",
    )
    .bind(a.org())
    .bind(room2["code"].as_str().unwrap())
    .bind(&pin)
    .bind(did2)
    .bind(&a.user_id)
    .execute(&app.db)
    .await
    .unwrap();

    let (st, body) = ivr(&app, Some(VOICE_SECRET), "pbx.alfa.example", &pin).await;
    assert_eq!(st, 404, "o IVR escolheu uma de duas salas: {body}");
    assert!(!body.to_string().contains(&code1), "{body}");
}

/// O travão de adivinhar PINs conta as falhas por central, e não deixa a
/// central de uma organização gastar as tentativas de outra.
#[sqlx::test(migrations = "./migrations")]
async fn adivinhar_pins_trava_a_central_e_so_essa(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-central.ao").await;
    let b = app.new_org("beta-central.ao").await;
    let (_code_a, pin_a) = sala_com_pin(&app, &a, "+244222400006").await;
    let (_code_b, pin_b) = sala_com_pin(&app, &b, "+244222400007").await;
    conta_sip(
        &app,
        &a,
        "pbx.alfa.example",
        "central-alfa",
        "password-da-alfa-1",
    )
    .await;
    conta_sip(
        &app,
        &b,
        "pbx.beta.example",
        "central-beta",
        "password-da-beta-2",
    )
    .await;

    let errado = outro_pin(&pin_a);
    let mut travou = false;
    for _ in 0..12 {
        let (st, _) = ivr(&app, Some(VOICE_SECRET), "pbx.alfa.example", errado).await;
        if st == 429 {
            travou = true;
            break;
        }
        assert_eq!(st, 404);
    }
    assert!(travou, "doze PINs errados seguidos e nenhum 429");

    // A central de B continua a entrar.
    let (st, body) = ivr(&app, Some(VOICE_SECRET), "pbx.beta.example", &pin_b).await;
    assert_eq!(st, 200, "o travão de A apanhou a central de B: {body}");
}
