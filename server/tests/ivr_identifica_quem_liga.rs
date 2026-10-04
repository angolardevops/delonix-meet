//! R277 — o IVR identifica quem liga, e a pessoa entra na sala com o seu nome.
//! Contra Postgres real.
//!
//! O que se mede é a REGRA do servidor e o censo da sala. Nenhum FreeSWITCH
//! corre aqui e nenhuma chamada é feita: o teste faz de IVR (com o segredo de
//! voz) e de ponte (`seat_phone_caller`, que é o que o UA SIP chama quando
//! atende a perna). O `dialin_ivr.lua` só está verificado por sintaxe.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const VOICE_SECRET: &str = "segredo-da-media-0123456789";
const TICKET_VAR: &str = "sip_h_X-Delonix-Caller-Ticket";

async fn spawn(db: sqlx::PgPool) -> TestApp {
    TestApp::spawn_with(
        db,
        &[
            ("VOICE_INTERNAL_SECRET", VOICE_SECRET),
            // A ponte «configurada», para o `room_bridge` vir na resposta. O UA
            // SIP não arranca nos testes — só se lê a configuração.
            ("PHONE_BRIDGE_SIP_BIND", "127.0.0.1:5099"),
            ("PHONE_BRIDGE_FREESWITCH_IPS", "127.0.0.1"),
            ("PHONE_BRIDGE_SIP_ADVERTISE", "ponte.teste:5099"),
        ],
    )
    .await
}

async fn ivr(app: &TestApp, path: &str, body: Value) -> (u16, Value) {
    let r = app
        .raw(
            reqwest::Method::POST,
            &format!("/internal/v1/voice/ivr/{path}"),
            &[("x-voice-secret", VOICE_SECRET)],
            Some(body),
        )
        .await;
    (r.status, r.json())
}

/// Uma org com DID, uma sala e a sala de voz dela: `(código, PIN, id da sala
/// de voz, id da sala)`.
async fn sala(app: &TestApp, owner: &Account, e164: &str) -> (String, String, String, Uuid) {
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
    let voice_room_id: String = sqlx::query_scalar(
        "SELECT id::text FROM voice_room WHERE room_code = $1 AND status = 'active'",
    )
    .bind(&code)
    .fetch_one(&app.db)
    .await
    .unwrap();
    let room_id = Uuid::parse_str(room["id"].as_str().unwrap()).unwrap();
    (
        code,
        vr["pin"].as_str().unwrap().into(),
        voice_room_id,
        room_id,
    )
}

/// Cria um ramal; devolve `(sip_username, sip_domain)`.
async fn ramal(app: &TestApp, admin: &Account, body: Value) -> (String, String) {
    let (st, r) = app
        .post(
            &format!("/api/orgs/{}/extensions", admin.org()),
            Some(&admin.token),
            body,
        )
        .await;
    assert_eq!(st, 200, "criar ramal: {r}");
    (
        r["sip_username"].as_str().unwrap().into(),
        r["sip_domain"].as_str().unwrap().into(),
    )
}

async fn gerar_pin(app: &TestApp, quem: &Account) -> String {
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/my-extension/regenerate-pin", quem.org()),
            Some(&quem.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    body["pin"].as_str().unwrap().to_string()
}

fn errado(certo: &str) -> &'static str {
    if certo == "584930" {
        "730518"
    } else {
        "584930"
    }
}

/// O que a ponte faz ao atender a perna; devolve `(nome no censo, anónimo)`.
async fn senta(app: &TestApp, room_id: Uuid, code: &str, ticket: Option<&str>) -> (String, bool) {
    let leg = Uuid::new_v4();
    delonix_server::seat_phone_caller(&app.state, room_id, code, leg, ticket).await;
    let nome = app
        .state
        .hub
        .roster_named(room_id)
        .into_iter()
        .find(|(id, _, _)| *id == leg)
        .expect("a perna está no censo")
        .1;
    let seat = app.state.hub.seat_of(room_id, leg).expect("lugar");
    (nome, seat.anonymous)
}

/// O bilhete que um ramal registado recebe ao validar o PIN da sala.
async fn bilhete_do_ramal(app: &TestApp, user: &str, dom: &str, pin_sala: &str) -> String {
    let (st, r) = ivr(
        app,
        "validate-extension",
        json!({"sip_username": user, "domain": dom, "pin": pin_sala}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    r["room_bridge"]["channel_vars"][TICKET_VAR]
        .as_str()
        .expect("o bilhete vem nas variáveis da ponte")
        .to_string()
}

/// Um RAMAL REGISTADO entra com o nome da pessoa (ou a etiqueta do ramal da
/// empresa), sem PIN pessoal: o aparelho já está autenticado.
#[sqlx::test(migrations = "./migrations")]
async fn um_ramal_registado_entra_com_o_nome_sem_pin_pessoal(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-nome.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let (code, pin_sala, _, room_id) = sala(&app, &a, "+244222300101").await;
    let (user, dom) = ramal(
        &app,
        &a,
        json!({"extension": "1001", "member_id": ana.user_id}),
    )
    .await;
    let (user_rec, _) = ramal(&app, &a, json!({"extension": "1002", "label": "Recepção"})).await;
    sqlx::query("UPDATE users SET display_name = 'Ana Paula' WHERE id = $1::uuid")
        .bind(&ana.user_id)
        .execute(&app.db)
        .await
        .unwrap();

    // Só o PIN da SALA: o pedido não leva PIN pessoal nenhum.
    let (st, r) = ivr(
        &app,
        "validate-extension",
        json!({"sip_username": user, "domain": dom, "pin": pin_sala}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    let ticket = r["room_bridge"]["channel_vars"][TICKET_VAR]
        .as_str()
        .expect("o bilhete vem nas variáveis da ponte")
        .to_string();
    assert_eq!(ticket.len(), 64, "{ticket}");
    // As outras variáveis da ponte continuam lá.
    assert_eq!(
        r["room_bridge"]["channel_vars"]["rtp_secure_media"],
        "mandatory:AES_CM_128_HMAC_SHA1_80"
    );
    // Na base só o hash.
    let guardados: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM voice_caller_tickets")
        .fetch_all(&app.db)
        .await
        .unwrap();
    assert_eq!(guardados.len(), 1);
    assert_ne!(guardados[0], ticket);

    assert_eq!(
        senta(&app, room_id, &code, Some(&ticket)).await,
        ("Ana Paula".to_string(), false)
    );
    // Uso único: o mesmo bilhete outra vez não identifica ninguém.
    assert_eq!(
        senta(&app, room_id, &code, Some(&ticket)).await,
        ("Telefone".to_string(), true)
    );
    // Sem bilhete, ou com um inventado: anónimo, como sempre.
    assert_eq!(
        senta(&app, room_id, &code, None).await,
        ("Telefone".to_string(), true)
    );
    assert_eq!(
        senta(&app, room_id, &code, Some(&"ab".repeat(32))).await,
        ("Telefone".to_string(), true)
    );

    // O ramal da empresa entra com a etiqueta.
    let (_, r) = ivr(
        &app,
        "validate-extension",
        json!({"sip_username": user_rec, "domain": dom, "pin": pin_sala}),
    )
    .await;
    let ticket = r["room_bridge"]["channel_vars"][TICKET_VAR]
        .as_str()
        .unwrap();
    assert_eq!(
        senta(&app, room_id, &code, Some(ticket)).await,
        ("Recepção".to_string(), false)
    );

    // Um bilhete vale para UMA sala, e expira.
    let outra = app.new_room(&a, "Outra sala").await;
    let outra_id = Uuid::parse_str(outra["id"].as_str().unwrap()).unwrap();
    let t = bilhete_do_ramal(&app, &user, &dom, &pin_sala).await;
    assert_eq!(
        senta(&app, outra_id, outra["code"].as_str().unwrap(), Some(&t)).await,
        ("Telefone".to_string(), true),
        "um bilhete de uma sala identificou alguém noutra"
    );
    // ...e não ficou gasto por isso: na sala dele ainda serve.
    assert_eq!(senta(&app, room_id, &code, Some(&t)).await.0, "Ana Paula");
    let t = bilhete_do_ramal(&app, &user, &dom, &pin_sala).await;
    sqlx::query("UPDATE voice_caller_tickets SET expires_at = now() - interval '1 second' WHERE used_at IS NULL")
        .execute(&app.db)
        .await
        .unwrap();
    assert_eq!(
        senta(&app, room_id, &code, Some(&t)).await,
        ("Telefone".to_string(), true),
        "um bilhete expirado identificou alguém"
    );
}

/// Quem liga DE FORA: PIN da sala, depois ramal + PIN pessoal com a origem.
/// Acerto → bilhete; falha → nenhuma identidade, e a entrada na sala não
/// depende disso.
#[sqlx::test(migrations = "./migrations")]
async fn quem_liga_de_fora_identifica_se_por_ramal_e_pin(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-fora.ao").await;
    let b = app.new_org("beta-fora.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let (code, pin_sala, voice_room, room_id) = sala(&app, &a, "+244222300201").await;
    let (_, _, voice_room_b, _) = sala(&app, &b, "+244222300202").await;
    ramal(
        &app,
        &a,
        json!({"extension": "1001", "member_id": ana.user_id}),
    )
    .await;
    let pin = gerar_pin(&app, &ana).await;

    // 1. O PIN da sala: a resposta traz o domínio da organização, que é com o
    //    que o IVR pede a identificação — e ainda NÃO traz bilhete.
    let (st, v) = ivr(
        &app,
        "validate",
        json!({"did_e164": "+244222300201", "pin": pin_sala}),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["voice_room_id"], voice_room.as_str());
    let dom = v["org_sip_domain"]
        .as_str()
        .expect("org_sip_domain")
        .to_string();
    assert!(
        v["room_bridge"]["channel_vars"].get(TICKET_VAR).is_none(),
        "{v}"
    );

    let pedido = |ext: &str, pin: &str, sala: &str, numero: &str| {
        json!({
            "domain": dom, "extension": ext, "pin": pin, "voice_room_id": sala,
            "origin": {"caller_number": numero, "network_ip": "10.9.0.1"},
        })
    };

    // 2. Falhas: PIN errado, ramal que não existe, e a sala de OUTRA
    //    organização com o PIN certo. Nenhuma traz bilhete nem nome.
    for (i, corpo) in [
        pedido("1001", errado(&pin), &voice_room, "+244923000001"),
        pedido("1999", &pin, &voice_room, "+244923000002"),
        pedido("1001", &pin, &voice_room_b, "+244923000003"),
        pedido("1001", &pin, &Uuid::new_v4().to_string(), "+244923000004"),
    ]
    .into_iter()
    .enumerate()
    {
        let (st, r) = ivr(&app, "verify-extension-pin", corpo).await;
        assert_eq!(st, 200, "caso {i}: {r}");
        assert_eq!(r["valid"], false, "caso {i}: {r}");
        assert_eq!(r["reason"], "invalid", "caso {i}: {r}");
        assert!(r.get("channel_vars").is_none(), "caso {i}: {r}");
        assert!(r.get("display_name").is_none(), "caso {i}: {r}");
    }
    // A sala de outra organização não chegou a ler o ramal: só o PIN errado
    // contou nele.
    let falhas: i32 = sqlx::query_scalar(
        "SELECT pin_failed_attempts FROM voice_extensions WHERE extension = '1001'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(falhas, 1);
    // Quem falhou entra na mesma — anónimo.
    assert_eq!(
        senta(&app, room_id, &code, None).await,
        ("Telefone".to_string(), true)
    );

    // 3. Acerto: o bilhete vem na resposta, e a pessoa entra com o nome.
    let (_, r) = ivr(
        &app,
        "verify-extension-pin",
        pedido("1001", &pin, &voice_room, "+244923000005"),
    )
    .await;
    assert_eq!(r["valid"], true, "{r}");
    assert_eq!(r["member_id"], json!(ana.user_id));
    let ticket = r["channel_vars"][TICKET_VAR].as_str().expect("bilhete");
    let (nome, anonimo) = senta(&app, room_id, &code, Some(ticket)).await;
    assert!(nome.starts_with("ana"), "{nome}");
    assert!(!anonimo);

    // 4. Sem `voice_room_id` a verificação continua a valer, sem bilhete.
    let (_, r) = ivr(
        &app,
        "verify-extension-pin",
        json!({"domain": dom, "extension": "1001", "pin": pin,
               "origin": {"caller_number": "+244923000006", "network_ip": "10.9.0.1"}}),
    )
    .await;
    assert_eq!(r["valid"], true, "{r}");
    assert!(r.get("channel_vars").is_none(), "{r}");
}

/// As três falhas de uma chamada (o IVR dá duas tentativas; aqui três, para
/// chegar ao travão) não bloqueiam o ramal de ninguém, e a quarta já nem é
/// verificada — mesmo com o PIN certo.
#[sqlx::test(migrations = "./migrations")]
async fn falhar_a_identificacao_trava_a_origem_e_nao_o_ramal(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-trava.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let (_, pin_sala, voice_room, _) = sala(&app, &a, "+244222300301").await;
    ramal(
        &app,
        &a,
        json!({"extension": "1001", "member_id": ana.user_id}),
    )
    .await;
    let pin = gerar_pin(&app, &ana).await;
    let (_, v) = ivr(
        &app,
        "validate",
        json!({"did_e164": "+244222300301", "pin": pin_sala}),
    )
    .await;
    let dom = v["org_sip_domain"].as_str().unwrap().to_string();
    let pedido = |pin: &str| {
        json!({
            "domain": dom, "extension": "1001", "pin": pin, "voice_room_id": voice_room,
            "origin": {"caller_number": "+244930000111", "network_ip": "10.9.0.1"},
        })
    };
    for _ in 0..3 {
        let (_, r) = ivr(&app, "verify-extension-pin", pedido(errado(&pin))).await;
        assert_eq!(r["reason"], "invalid", "{r}");
    }
    let (_, r) = ivr(&app, "verify-extension-pin", pedido(&pin)).await;
    assert_eq!(r["reason"], "origin_locked", "{r}");
    assert!(r.get("channel_vars").is_none(), "{r}");
    let (estado, bilhetes): (bool, i64) = sqlx::query_as(
        "SELECT COALESCE(pin_locked_until > now(), false),
                (SELECT count(*) FROM voice_caller_tickets)
           FROM voice_extensions WHERE extension = '1001'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(!estado, "o ramal ficou bloqueado por uma só origem");
    assert_eq!(bilhetes, 0);
    // O PIN da sala continua a servir a essa origem: falhar a identificação
    // não tira ninguém da reunião.
    let (st, _) = ivr(
        &app,
        "validate",
        json!({"did_e164": "+244222300301", "pin": pin_sala}),
    )
    .await;
    assert_eq!(st, 200);
}
