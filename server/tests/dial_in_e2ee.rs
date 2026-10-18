//! A-01 — uma sala cifrada de ponta a ponta NÃO aceita um telefone, na
//! ENTRADA. Contra Postgres real.
//!
//! O produto já recusava nos outros dois caminhos: o dial-OUT com uma razão
//! escrita (`dial_outs.rs`, `dial_out.room_e2ee`, provado em
//! `tests/dial_outs.rs`) e a auto-gravação (`recorder.rs`,
//! `auto_record AND NOT r.e2ee`). Só a entrada não chegava a olhar: medido a
//! 2026-10-18, `grep -c e2ee server/src/voice.rs` dava **zero**.
//!
//! O que se mede aqui é a REGRA do servidor, não a chamada: nenhum FreeSWITCH
//! corre nestes testes. O PIN continua a ser aceite — quem liga cai na
//! conferência local, que é o recuo que o `room_bridge_for` já usa quando a
//! ponte não está configurada — e o que desaparece é a PONTE para a sala
//! cifrada. É isso que impede o telefone de entrar.

mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

const VOICE_SECRET: &str = "segredo-da-media-0123456789";
const ADVERTISE: &str = "ponte.teste:5099";

/// A ponte «configurada», para o `room_bridge` poder vir na resposta. Sem
/// isto o teste passava por falta de configuração e não pela regra — e seria
/// um falso verde dos que esta casa persegue.
async fn spawn(db: sqlx::PgPool) -> TestApp {
    TestApp::spawn_with(
        db,
        &[
            ("VOICE_INTERNAL_SECRET", VOICE_SECRET),
            ("PHONE_BRIDGE_SIP_BIND", "127.0.0.1:5099"),
            ("PHONE_BRIDGE_FREESWITCH_IPS", "127.0.0.1"),
            ("PHONE_BRIDGE_SIP_ADVERTISE", ADVERTISE),
        ],
    )
    .await
}

/// O que o `dialin_ivr.lua` envia no caminho do DID.
async fn ivr(app: &TestApp, did: &str, pin: &str) -> (u16, Value) {
    let r = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/voice/ivr/validate",
            &[("x-voice-secret", VOICE_SECRET)],
            Some(json!({"did_e164": did, "pin": pin})),
        )
        .await;
    (r.status, r.json())
}

/// Uma sala (cifrada ou não) com DID e PIN. Devolve `(did, pin)`.
async fn sala_com_did(app: &TestApp, owner: &Account, e164: &str, e2ee: bool) -> (String, String) {
    sqlx::query("INSERT INTO voice_did (org_id, e164) VALUES ($1::uuid, $2)")
        .bind(owner.org())
        .bind(e164)
        .execute(&app.db)
        .await
        .unwrap();
    let (st, room) = app
        .post(
            "/api/rooms",
            Some(&owner.token),
            json!({"name": if e2ee { "Cifrada" } else { "Normal" }, "topology": "sfu", "e2ee": e2ee}),
        )
        .await;
    assert_eq!(st, 200, "criar sala (e2ee={e2ee}): {room}");
    assert_eq!(room["e2ee"], e2ee, "a sala não nasceu como pedido: {room}");
    let code = room["code"].as_str().unwrap().to_string();
    let (st, vr) = app
        .post(
            &format!("/api/orgs/{}/voice/rooms", owner.org()),
            Some(&owner.token),
            json!({"room_code": code}),
        )
        .await;
    assert_eq!(st, 200, "criar sala de voz: {vr}");
    (e164.to_string(), vr["pin"].as_str().unwrap().to_string())
}

/// O caso que o A-01 descreve: DID e PIN certos, sala cifrada, **sem ponte**.
#[sqlx::test(migrations = "./migrations")]
async fn uma_sala_cifrada_nao_devolve_a_ponte_ao_ivr(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("cifrada-dialin.ao").await;
    let (did, pin) = sala_com_did(&app, &a, "+244222500001", true).await;

    let (st, body) = ivr(&app, &did, &pin).await;

    // O PIN é válido: o IVR fica a saber a sala de voz e atende a chamada.
    assert_eq!(st, 200, "{body}");
    assert!(
        body["room_code"].as_str().is_some_and(|c| !c.is_empty()),
        "o PIN certo tem de continuar a ser aceite — quem liga cai na conferência local, não leva a porta na cara: {body}"
    );
    // E o que NÃO vem é a ponte. Sem ela o FreeSWITCH não tem para onde
    // transferir: o telefone não entra na sala cifrada.
    assert!(
        body["room_bridge"].is_null(),
        "A-01: uma sala cifrada de ponta a ponta devolveu a ponte telefone↔sala — a ponte publica no SFU EM CLARO e a garantia que o ecrã promete deixa de ser verdade: {body}"
    );
}

/// CONTROLO NEGATIVO, e é o que dá valor ao teste de cima: a MESMA montagem,
/// a mesma org, o mesmo caminho — só sem `e2ee` — devolve a ponte. Sem isto, o
/// teste acima passaria por a sala não existir, por a ponte não estar
/// configurada, ou por o PIN estar errado, e ninguém saberia.
#[sqlx::test(migrations = "./migrations")]
async fn a_mesma_sala_sem_e2ee_devolve_a_ponte(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("normal-dialin.ao").await;
    let (did, pin) = sala_com_did(&app, &a, "+244222500002", false).await;

    let (st, body) = ivr(&app, &did, &pin).await;

    assert_eq!(st, 200, "{body}");
    let ponte = &body["room_bridge"];
    assert!(
        !ponte.is_null(),
        "controlo negativo quebrado: sem e2ee a ponte TEM de vir, senão o teste do e2ee não prova nada: {body}"
    );
    // E é a ponte a sério, com o URI que o IVR transfere.
    let uri = ponte["sip_uri"].as_str().unwrap_or_default();
    assert!(
        uri.starts_with("sip:room-") && uri.contains(ADVERTISE),
        "o URI da ponte não é o esperado: {ponte}"
    );
}

/// A segunda camada: o `INVITE` que chega à ponte é resolvido OUTRA VEZ, e
/// também tem de recusar. Não é redundância — é o que apanha um `INVITE` que
/// nunca passou pelo IVR, porque a allowlist de origens diz de onde um INVITE
/// pode vir, não para que sala.
///
/// Aqui mede-se pela fonte, não por comportamento: o `DialInAdmission` é um
/// `impl` privado sem rota que o exercite, e inventar uma só para o teste era
/// mudar o desenho para a medição. O que se trava é o regresso silencioso da
/// consulta sem `e2ee`.
#[test]
fn o_admit_da_ponte_tambem_le_o_e2ee() {
    let src = include_str!("../src/voice.rs");
    let i = src
        .find("impl crate::phone_bridge::sip::BridgeAdmission for DialInAdmission")
        .expect("o DialInAdmission mudou de nome — este teste ficou cego");
    let corpo = &src[i..i + 1200];
    assert!(
        corpo.contains("SELECT id, e2ee FROM rooms WHERE code = $1"),
        "o admit da ponte voltou a resolver a sala SEM ler o e2ee: um INVITE directo entra numa sala cifrada"
    );
    assert!(
        corpo.contains("if e2ee"),
        "o admit lê o e2ee e não o usa para recusar"
    );
}
