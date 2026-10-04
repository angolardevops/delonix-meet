//! Telefonia (ADR-0009) contra Postgres real e um FreeSWITCH ESL FALSO (um
//! servidor TCP que fala o protocolo do Event Socket).
//!
//! O que um verde aqui prova: o contrato HTTP, a persistência, o isolamento
//! entre organizações, as invariantes de emergência de ponta a ponta, a
//! ingestão idempotente de CDR com custo ao preço em vigor, e o NOSSO cliente
//! ESL (enquadramento, autenticação, +OK/-ERR, tempos). O que NÃO prova: que um
//! FreeSWITCH real responde assim — ver ADR-0009 §EXTERNAL.
mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::Engine as _;
use common::TestApp;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

const SECRET: &str = "segredo-interno-de-teste-com-entropia-suficiente";

fn t(org: &str, rest: &str) -> String {
    format!("/api/orgs/{org}/telephony{rest}")
}

// ============================================================
//  ESL falso
// ============================================================

/// Resposta do ESL falso a um comando: mensagens cruas, cada uma com um
/// atraso antes de a escrever.
type Responder = Arc<dyn Fn(&str) -> Vec<(u64, String)> + Send + Sync>;

struct FakeEsl {
    addr: String,
    log: Arc<Mutex<Vec<String>>>,
}

async fn read_cmd(r: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> Option<String> {
    let mut line = String::new();
    loop {
        let mut l = String::new();
        if r.read_line(&mut l).await.ok()? == 0 {
            return None;
        }
        if l == "\n" {
            if line.is_empty() {
                continue;
            }
            return Some(line);
        }
        if !line.is_empty() {
            line.push('\n');
        }
        line.push_str(l.trim_end());
    }
}

fn api(body: &str) -> String {
    format!(
        "Content-Type: api/response\nContent-Length: {}\n\n{body}",
        body.len()
    )
}

fn reply(text: &str) -> String {
    format!("Content-Type: command/reply\nReply-Text: {text}\n\n")
}

fn event(headers: &[(&str, &str)], job_body: Option<&str>) -> String {
    let mut body: String = headers
        .iter()
        .map(|(k, v)| format!("{k}: {}\n", v.replace(' ', "%20")))
        .collect();
    if let Some(j) = job_body {
        body.push_str(&format!("Content-Length: {}\n\n{j}", j.len()));
    } else {
        body.push('\n');
    }
    format!(
        "Content-Length: {}\nContent-Type: text/event-plain\n\n{body}",
        body.len()
    )
}

async fn spawn_esl(password: &'static str, responder: Responder) -> FakeEsl {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let log = Arc::new(Mutex::new(Vec::new()));
    let log2 = log.clone();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                return;
            };
            let log = log2.clone();
            let responder = responder.clone();
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                w.write_all(b"Content-Type: auth/request\n\n").await.ok();
                let Some(auth) = read_cmd(&mut r).await else {
                    return;
                };
                if auth != format!("auth {password}") {
                    w.write_all(reply("-ERR invalid").as_bytes()).await.ok();
                    return;
                }
                w.write_all(reply("+OK accepted").as_bytes()).await.ok();
                while let Some(cmd) = read_cmd(&mut r).await {
                    let cmd = cmd.strip_prefix("api ").unwrap_or(&cmd).to_string();
                    log.lock().unwrap().push(cmd.clone());
                    for (delay, msg) in responder(&cmd) {
                        if delay > 0 {
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                        }
                        if w.write_all(msg.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                }
            });
        }
    });
    FakeEsl { addr, log }
}

/// Um FreeSWITCH «saudável»: gateways registados, 3 canais no limite; um
/// `bgapi originate` toca, é atendido ao fim de 150 ms e desliga 200 ms
/// depois — excepto números `…000`, que dão `USER_BUSY`.
fn healthy_fs() -> Responder {
    Arc::new(|cmd: &str| {
        let one = |b: &str| vec![(0, api(b))];
        if cmd == "version" {
            one("FreeSWITCH Version 1.10.11-release+git~20231213 (git 64bit)\n")
        } else if cmd == "uptime s" {
            one("3600\n")
        } else if cmd == "show channels count" {
            one("\n3 total.\n")
        } else if cmd.starts_with("global_getvar outbound_codec_prefs") {
            one("OPUS,G722,PCMA\n")
        } else if let Some(gw) = cmd.strip_prefix("sofia xmlstatus gateway ") {
            one(&format!("<gateway><name>{gw}</name><state>REGED</state><status>UP</status><pingtime>12.0</pingtime></gateway>\n"))
        } else if cmd.starts_with("limit_usage hash delonix_trunk ") {
            one("3\n")
        } else if cmd.starts_with("sofia profile ") || cmd.starts_with("hupall ") {
            one("+OK\n")
        } else if cmd.starts_with("event ") || cmd.starts_with("filter ") {
            vec![(0, reply("+OK"))]
        } else if let Some(rest) = cmd.strip_prefix("bgapi originate ") {
            let job = rest
                .split("Job-UUID: ")
                .nth(1)
                .unwrap_or("")
                .trim()
                .to_string();
            let trunk = rest
                .split("[delonix_trunk_id=")
                .nth(1)
                .and_then(|x| x.split(']').next())
                .unwrap_or("")
                .to_string();
            let chan = format!("chan-{job}");
            let base = |name: &'static str| {
                vec![
                    ("Event-Name", name.to_string()),
                    ("Unique-ID", chan.clone()),
                    ("variable_delonix_call_id", job.clone()),
                    ("variable_delonix_trunk_id", trunk.clone()),
                ]
            };
            let ev = |h: Vec<(&str, String)>, extra: &[(&str, &str)], body: Option<&str>| {
                let mut all: Vec<(&str, &str)> = h.iter().map(|(k, v)| (*k, v.as_str())).collect();
                all.extend_from_slice(extra);
                event(&all, body)
            };
            let mut out = vec![(0, reply(&format!("+OK Job-UUID: {job}")))];
            out.push((20, ev(base("CHANNEL_CREATE"), &[], None)));
            if rest.contains("000 &") {
                out.push((
                    30,
                    ev(
                        base("CHANNEL_HANGUP"),
                        &[("Hangup-Cause", "USER_BUSY")],
                        None,
                    ),
                ));
                out.push((
                    0,
                    ev(
                        vec![
                            ("Event-Name", "BACKGROUND_JOB".into()),
                            ("Job-UUID", job.clone()),
                        ],
                        &[],
                        Some("-ERR USER_BUSY\n"),
                    ),
                ));
            } else {
                out.push((20, ev(base("CHANNEL_PROGRESS"), &[], None)));
                out.push((150, ev(base("CHANNEL_ANSWER"), &[], None)));
                out.push((
                    0,
                    ev(
                        vec![
                            ("Event-Name", "BACKGROUND_JOB".into()),
                            ("Job-UUID", job.clone()),
                        ],
                        &[],
                        Some(&format!("+OK {chan}\n")),
                    ),
                ));
                out.push((
                    200,
                    ev(
                        base("CHANNEL_HANGUP_COMPLETE"),
                        &[
                            ("Hangup-Cause", "NORMAL_CLEARING"),
                            ("variable_billsec", "1"),
                        ],
                        None,
                    ),
                ));
            }
            out
        } else {
            one("-ERR command not found\n")
        }
    })
}

// ============================================================
//  Ajudas
// ============================================================

async fn create_trunk(app: &TestApp, token: &str, org: &str, body: Value) -> Value {
    let (st, v) = app.post(&t(org, "/trunks"), Some(token), body).await;
    assert_eq!(st, 201, "criar tronco: {v}");
    v
}

fn unitel() -> Value {
    json!({"name": "Unitel", "short_code": "uni", "host": "sip.unitel.ao", "port": 5061,
           "transport": "tls", "srtp": "mandatory", "username": "delonix",
           "password": "senha-da-operadora-unitel", "prefixes": ["9"], "max_channels": 60,
           "price_per_min": {"amount": "9,40", "currency": "AOA"}})
}

fn africell() -> Value {
    json!({"name": "Africell", "short_code": "AFR", "host": "sip.africell.ao", "port": 5060,
           "transport": "tls", "srtp": "optional", "prefixes": ["95"], "max_channels": 30,
           "price_per_min": {"amount": "8.90", "currency": "AOA"}})
}

async fn screen_plan(app: &TestApp, token: &str, org: &str, uni: &str, afr: &str) -> Value {
    let (st, v) = app
        .put(
            &t(org, "/dial-plan"),
            Some(token),
            json!({"rules": [
                {"pattern": "9XXXXXXXX", "description": "Móvel nacional", "action": "external",
                 "trunk_id": uni, "fallback_trunk_id": afr, "record": true},
                {"pattern": "84209", "description": "Sala por PIN — entra na sessão", "action": "room_pin", "record": true},
                {"pattern": "1XX", "description": "Ramal interno", "action": "extension"},
                {"pattern": "0800X.", "description": "Números verdes", "action": "block"},
                {"pattern": "112,113,115", "description": "Emergência — nunca gravado", "action": "external",
                 "trunk_id": uni, "fallback_trunk_id": afr, "emergency": true}
            ]}),
        )
        .await;
    assert_eq!(st, 200, "PUT plano: {v}");
    v
}

fn basic(secret: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("freeswitch:{secret}"))
    )
}

// ============================================================
//  Troncos
// ============================================================

#[sqlx::test(migrations = "./migrations")]
async fn trunks_crud_order_prices_secrets_and_isolation(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa-tel.ao").await;
    let b = app.new_org("beta-tel.ao").await;

    let uni = create_trunk(&app, &a.token, a.org(), unitel()).await;
    let uni_id = uni["id"].as_str().unwrap().to_string();
    assert_eq!(uni["short_code"], "UNI");
    assert_eq!(uni["role"], "primary");
    assert_eq!(uni["password_configured"], true);
    assert!(uni.get("password").is_none());
    assert!(!uni.to_string().contains("senha-da-operadora"), "{uni}");
    assert_eq!(
        uni["current_price_per_min"],
        json!({"amount": "9.4000", "currency": "AOA"})
    );
    // Sem SIP configurado: estado honesto, sem números inventados.
    assert_eq!(uni["status"]["state"], "unknown");
    assert!(
        uni["status"]["reasons"]
            .to_string()
            .contains("sip_not_configured"),
        "{uni}"
    );
    assert_eq!(uni["status"]["registration"], Value::Null);
    assert_eq!(uni["status"]["channels_in_use"], Value::Null);
    assert_eq!(uni["status"]["asr"], Value::Null);
    assert_eq!(uni["status"]["asr_reason"], "no_calls_in_window");

    let sealed: String =
        sqlx::query_scalar("SELECT password_sealed FROM telephony_trunks WHERE id = $1::uuid")
            .bind(&uni_id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(
        sealed.starts_with("enc:v1:") && !sealed.contains("senha"),
        "{sealed}"
    );

    let afr = create_trunk(&app, &a.token, a.org(), africell()).await;
    let afr_id = afr["id"].as_str().unwrap().to_string();
    assert_eq!(
        (afr["role"].as_str(), afr["reserve_rank"].as_i64()),
        (Some("reserve"), Some(1))
    );

    // Regras de forma e segurança.
    let mut bad = unitel();
    bad["name"] = json!("Outra");
    bad["transport"] = json!("udp");
    let (st, e) = app.post(&t(a.org(), "/trunks"), Some(&a.token), bad).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("telephony.srtp_requires_tls"))
    );
    let mut ssrf = unitel();
    ssrf["name"] = json!("Interna");
    ssrf["host"] = json!("10.0.0.5");
    let (st, e) = app.post(&t(a.org(), "/trunks"), Some(&a.token), ssrf).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("telephony.trunk_host_refused")),
        "{e}"
    );
    let (st, e) = app
        .post(&t(a.org(), "/trunks"), Some(&a.token), unitel())
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("telephony.trunk_name_taken"))
    );

    // Ordem inteira: exacta, e muda os papéis.
    let (st, e) = app
        .put(
            &t(a.org(), "/trunk-order"),
            Some(&a.token),
            json!({"trunk_ids": [afr_id]}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("telephony.invalid_trunk_order"))
    );
    let (st, order) = app
        .put(
            &t(a.org(), "/trunk-order"),
            Some(&a.token),
            json!({"trunk_ids": [afr_id, uni_id]}),
        )
        .await;
    assert_eq!(st, 200, "{order}");
    assert_eq!(order["items"][0]["name"], "Africell");
    assert_eq!(order["items"][0]["role"], "primary");
    assert_eq!(order["items"][1]["role"], "reserve");

    // PATCH: password write-only, diff na auditoria sem a password.
    let (st, up) = app
        .patch(
            &t(a.org(), &format!("/trunks/{uni_id}")),
            Some(&a.token),
            json!({"max_channels": 80, "password": "nova-senha-secreta"}),
        )
        .await;
    assert_eq!(st, 200, "{up}");
    assert_eq!(up["max_channels"], 80);
    let audit: String = sqlx::query_scalar(
        "SELECT target FROM audit_logs WHERE action = 'telephony.trunk.updated' ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(
        audit.contains("max_channels: 60 → 80") && !audit.contains("nova-senha"),
        "{audit}"
    );

    // Preços: histórico, nunca retroactivo.
    let (st, e) = app
        .post(
            &t(a.org(), &format!("/trunks/{uni_id}/prices")),
            Some(&a.token),
            json!({"price_per_min": {"amount": "9.90", "currency": "AOA"}, "valid_from": "2020-01-01T00:00:00Z"}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("telephony.price_backdated"))
    );
    let (st, _) = app
        .post(
            &t(a.org(), &format!("/trunks/{uni_id}/prices")),
            Some(&a.token),
            json!({"price_per_min": {"amount": "9.90", "currency": "AOA"}, "valid_from": "2099-01-01T00:00:00Z"}),
        )
        .await;
    assert_eq!(st, 201);
    let (st, prices) = app
        .get(
            &t(a.org(), &format!("/trunks/{uni_id}/prices")),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 200);
    let items = prices["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0]["in_force"], false,
        "o futuro ainda não está em vigor"
    );
    assert_eq!(items[1]["in_force"], true);

    // Isolamento: B não vê nem mexe em nada de A; um membro de A não é admin.
    for path in [
        t(a.org(), "/trunks"),
        t(a.org(), &format!("/trunks/{uni_id}")),
        t(a.org(), &format!("/trunks/{uni_id}/prices")),
        t(a.org(), "/exchange-rates"),
    ] {
        let (st, body) = app.get(&path, Some(&b.token)).await;
        assert!(st == 404 || st == 403, "{path}: {st} {body}");
    }
    let (st, _) = app
        .get(&t(b.org(), &format!("/trunks/{uni_id}")), Some(&b.token))
        .await;
    assert_eq!(st, 404, "o id de A pelo caminho de B");
    let (st, _) = app
        .patch(
            &t(b.org(), &format!("/trunks/{uni_id}")),
            Some(&b.token),
            json!({"enabled": false}),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .put(
            &t(b.org(), "/trunk-order"),
            Some(&b.token),
            json!({"trunk_ids": [uni_id]}),
        )
        .await;
    assert_eq!(st, 400, "um tronco de A não entra na ordem de B");
    let member = app.add_member(&a, "zeca", "member").await;
    let (st, _) = app.get(&t(a.org(), "/trunks"), Some(&member.token)).await;
    assert_eq!(st, 403);

    // Apagar: 409 enquanto o plano o usa.
    screen_plan(&app, &a.token, a.org(), &uni_id, &afr_id).await;
    let (st, e) = app
        .delete(&t(a.org(), &format!("/trunks/{afr_id}")), Some(&a.token))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("telephony.trunk_in_use"))
    );
    let (st, _) = app
        .delete(&t(b.org(), &format!("/trunks/{afr_id}")), Some(&b.token))
        .await;
    assert_eq!(st, 404);
    app.put(
        &t(a.org(), "/dial-plan"),
        Some(&a.token),
        json!({"rules": []}),
    )
    .await;
    let (st, _) = app
        .delete(&t(a.org(), &format!("/trunks/{afr_id}")), Some(&a.token))
        .await;
    assert_eq!(st, 204);
}

// ============================================================
//  Plano de marcação
// ============================================================

#[sqlx::test(migrations = "./migrations")]
async fn dial_plan_first_match_emergency_invariants_and_test(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa-plan.ao").await;
    let b = app.new_org("beta-plan.ao").await;
    let uni = create_trunk(&app, &a.token, a.org(), unitel()).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let afr = create_trunk(&app, &a.token, a.org(), africell()).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let b_trunk = create_trunk(&app, &b.token, b.org(), unitel()).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (st, empty) = app.get(&t(a.org(), "/dial-plan"), Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(empty["version"], 0);
    assert_eq!(empty["emergency_numbers"], json!(["112", "113", "115"]));

    // Invariantes recusadas ao gravar, sem escrita parcial.
    let cases = [
        (
            json!({"pattern": "112,113,115", "description": "Emergência", "action": "external",
                "trunk_id": uni, "emergency": true, "record": true}),
            "telephony.emergency_never_recorded",
        ),
        (
            json!({"pattern": "11X", "description": "Bloquear 11X", "action": "block"}),
            "telephony.emergency_cannot_be_blocked",
        ),
        (
            json!({"pattern": "9XXXXXXXX", "description": "Móvel", "action": "external", "trunk_id": b_trunk}),
            "telephony.unknown_trunk",
        ),
        (
            json!({"pattern": "9XX.X", "description": "Mal", "action": "external", "trunk_id": uni}),
            "telephony.invalid_pattern",
        ),
    ];
    for (rule, code) in cases {
        let (st, e) = app
            .put(
                &t(a.org(), "/dial-plan"),
                Some(&a.token),
                json!({"rules": [rule]}),
            )
            .await;
        assert_eq!((st, e["code"].as_str()), (400, Some(code)), "{e}");
    }
    let (_, still) = app.get(&t(a.org(), "/dial-plan"), Some(&a.token)).await;
    assert_eq!(still["version"], 0);

    let plan = screen_plan(&app, &a.token, a.org(), &uni, &afr).await;
    assert_eq!(plan["version"], 1);
    assert_eq!(plan["rules"].as_array().unwrap().len(), 5);

    let test = |number: &'static str| {
        let app = &app;
        let a = &a;
        async move {
            let (st, v) = app
                .post(
                    &t(a.org(), "/dial-plan/test"),
                    Some(&a.token),
                    json!({"number": number}),
                )
                .await;
            assert_eq!(st, 200, "{number}: {v}");
            v
        }
    };
    let v = test("+244 923 447 108").await;
    assert_eq!(v["outcome"], "route");
    assert_eq!(v["matched_rule"]["position"], 0);
    assert_eq!(v["trunk"]["name"], "Unitel");
    assert_eq!(v["fallbacks"][0]["name"], "Africell");
    assert_eq!(v["recorded"], true);
    assert_eq!(
        v["estimated_price_per_min"],
        json!({"amount": "9.4000", "currency": "AOA"})
    );

    let v = test("84209").await;
    assert_eq!(
        (v["outcome"].as_str(), v["action"].as_str()),
        (Some("internal"), Some("room_pin"))
    );
    assert_eq!(v["price_reason"], "not_external");

    let v = test("08001234").await;
    assert_eq!(v["outcome"], "blocked");

    // 112 casa «1XX» (posição 2) antes da regra de emergência: o invariante ganha.
    let v = test("112").await;
    assert_eq!(v["emergency"], true);
    assert_eq!(v["recorded"], false);
    assert_eq!(v["outcome"], "route");
    assert_eq!(v["matched_rule"]["position"], 4);
    assert_eq!(v["overridden_rule_position"], 2);
    assert_eq!(v["trunk"]["name"], "Unitel");

    // Com a Unitel e a Africell desligadas não há caminho — diz-se, não se inventa.
    for id in [&uni, &afr] {
        app.patch(
            &t(a.org(), &format!("/trunks/{id}")),
            Some(&a.token),
            json!({"enabled": false}),
        )
        .await;
    }
    let v = test("113").await;
    assert_eq!(v["outcome"], "no_available_trunk");
    assert_eq!(v["emergency"], true);

    let v = test("555").await;
    assert_eq!(v["outcome"], "no_match");
    let (st, e) = app
        .post(
            &t(a.org(), "/dial-plan/test"),
            Some(&a.token),
            json!({"number": "abc"}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("telephony.invalid_number"))
    );

    // Isolamento.
    let (st, _) = app.get(&t(a.org(), "/dial-plan"), Some(&b.token)).await;
    assert!(st == 403 || st == 404);
    let (st, _) = app
        .put(
            &t(a.org(), "/dial-plan"),
            Some(&b.token),
            json!({"rules": []}),
        )
        .await;
    assert!(st == 403 || st == 404);
    let (st, _) = app
        .post(
            &t(a.org(), "/dial-plan/test"),
            Some(&b.token),
            json!({"number": "112"}),
        )
        .await;
    assert!(st == 403 || st == 404);

    // A base também recusa emergência gravada (defesa em profundidade).
    let r = sqlx::query(
        "UPDATE telephony_dial_rules SET record = true WHERE emergency AND org_id = $1::uuid",
    )
    .bind(a.org())
    .execute(&app.db)
    .await;
    assert!(r.is_err(), "o CHECK da base tem de recusar");
}

// ============================================================
//  CDR: ingestão idempotente, custo com mudança de preço, lista e consumo
// ============================================================

fn cdr(uuid: &str, org: &str, trunk: &str, start: i64, billsec: i64, cause: &str) -> Value {
    json!({"variables": {
        "uuid": uuid, "direction": "outbound",
        "start_epoch": start.to_string(), "answer_epoch": if billsec > 0 { (start + 3).to_string() } else { "0".into() },
        "end_epoch": (start + 3 + billsec).to_string(), "duration": (3 + billsec).to_string(),
        "billsec": billsec.to_string(), "hangup_cause": cause,
        "caller_id_number": "%2B244222000000", "destination_number": "244923447108",
        "sip_gateway_name": format!("dlx-{trunk}"), "delonix_org_id": org,
        "delonix_record": "true", "delonix_emergency": "false",
        "rtp_audio_in_packet_count": "990", "rtp_audio_in_skip_packet_count": "10",
        "rtp_audio_in_jitter_max_variance": "36"
    }})
}

async fn ingest(app: &TestApp, auth: Option<&str>, body: &Value) -> (u16, Value) {
    let mut rb = app
        .http
        .post(app.url("/internal/v1/telephony/call-records"))
        .json(body);
    if let Some(a) = auth {
        rb = rb.header("authorization", a);
    }
    let res = rb.send().await.unwrap();
    let st = res.status().as_u16();
    (st, res.json().await.unwrap_or(Value::Null))
}

#[sqlx::test(migrations = "./migrations")]
async fn cdr_ingestion_idempotent_priced_at_time_of_call_listed_and_summed(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("VOICE_INTERNAL_SECRET", SECRET)]).await;
    let a = app.new_org("alfa-cdr.ao").await;
    let b = app.new_org("beta-cdr.ao").await;
    let mut uni_body = unitel();
    uni_body.as_object_mut().unwrap().remove("price_per_min");
    let uni = create_trunk(&app, &a.token, a.org(), uni_body).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let b_trunk = create_trunk(&app, &b.token, b.org(), unitel()).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    // Histórico de preços no passado (a API não deixa retroagir; a base sim, para o teste).
    sqlx::query(
        "INSERT INTO telephony_trunk_prices (id, org_id, trunk_id, currency, price_per_min_e4, valid_from)
         VALUES (gen_random_uuid(), $1::uuid, $2::uuid, 'AOA', 94000, '2026-09-01T00:00:00Z'),
                (gen_random_uuid(), $1::uuid, $2::uuid, 'AOA', 99000, '2026-09-15T00:00:00Z')",
    )
    .bind(a.org())
    .bind(&uni)
    .execute(&app.db)
    .await
    .unwrap();

    let sep14 = 1_789_430_340; // 2026-09-14T23:59:00Z
    let sep15 = 1_789_430_400; // 2026-09-15T00:00:00Z
    let auth = basic(SECRET);

    let first = cdr("call-a", a.org(), &uni, sep14, 125, "NORMAL_CLEARING");
    let (st, r1) = ingest(&app, Some(&auth), &first).await;
    assert_eq!(st, 201, "{r1}");
    assert_eq!(r1["duplicate"], false);
    // Reenvio: o mesmo registo, sem duplicar custo.
    let (st, r2) = ingest(&app, Some(&auth), &first).await;
    assert_eq!(st, 200);
    assert_eq!(
        (r2["duplicate"].as_bool(), r2["id"].clone()),
        (Some(true), r1["id"].clone())
    );
    let (st, _) = ingest(
        &app,
        Some(&auth),
        &cdr("call-b", a.org(), &uni, sep15, 125, "NORMAL_CLEARING"),
    )
    .await;
    assert_eq!(st, 201);
    let (st, _) = ingest(
        &app,
        Some(&auth),
        &cdr("call-c", a.org(), &uni, sep15 + 60, 0, "USER_BUSY"),
    )
    .await;
    assert_eq!(st, 201);

    let costs: Vec<(String, Option<i64>, String)> = sqlx::query_as(
        "SELECT source_call_id, cost_e4, outcome FROM telephony_call_records ORDER BY source_call_id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        costs,
        vec![
            ("call-a".into(), Some(282_000), "answered".into()), // 3 min × 9,40
            ("call-b".into(), Some(297_000), "answered".into()), // 3 min × 9,90
            ("call-c".into(), Some(0), "busy".into()),
        ]
    );

    // Autenticação e org.
    let (st, _) = ingest(&app, None, &first).await;
    assert_eq!(st, 401);
    let (st, _) = ingest(&app, Some(&basic("errado")), &first).await;
    assert_eq!(st, 401);
    let mut orphan = cdr("call-x", a.org(), &uni, sep15, 10, "NORMAL_CLEARING");
    orphan["variables"]
        .as_object_mut()
        .unwrap()
        .remove("delonix_org_id");
    let (st, e) = ingest(&app, Some(&auth), &orphan).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (422, Some("telephony.cdr_org_unresolved"))
    );
    // Sem organização E sem tronco não é uma chamada da telefonia (ramal para
    // ramal, o IVR do dial-in): aceita-se, para o FreeSWITCH não a reenviar
    // nem a guardar em disco, e não se regista nada.
    let mut interna = orphan.clone();
    interna["variables"]["uuid"] = json!("call-interna");
    interna["variables"]
        .as_object_mut()
        .unwrap()
        .remove("sip_gateway_name");
    let (st, _) = ingest(&app, Some(&auth), &interna).await;
    assert_eq!(st, 204);
    let guardadas: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM telephony_call_records WHERE source_call_id IN ('call-x', 'call-interna')",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(guardadas, 0);
    // Um tronco de B num CDR de A não é atribuído a A.
    let (st, _) = ingest(
        &app,
        Some(&auth),
        &cdr("call-d", a.org(), &b_trunk, sep15, 30, "NORMAL_CLEARING"),
    )
    .await;
    assert_eq!(st, 201);
    let (trunk, reason): (Option<uuid::Uuid>, Option<String>) = sqlx::query_as(
        "SELECT trunk_id, cost_reason FROM telephony_call_records WHERE source_call_id = 'call-d'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!((trunk, reason.as_deref()), (None, Some("no_trunk")));
    // Emergência nunca gravada, mesmo que a variável diga o contrário.
    let mut em = cdr("call-e", a.org(), &uni, sep15, 40, "NORMAL_CLEARING");
    em["variables"]["delonix_emergency"] = json!("true");
    em["variables"]["destination_number"] = json!("112");
    ingest(&app, Some(&auth), &em).await;
    let rec: bool = sqlx::query_scalar(
        "SELECT recorded FROM telephony_call_records WHERE source_call_id = 'call-e'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(!rec);

    // Lista: mascarada, filtrada, paginada, isolada.
    let (st, page) = app
        .get(
            &format!("{}?page_size=2", t(a.org(), "/call-records")),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 200, "{page}");
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    let token = page["next_page_token"].as_str().unwrap().to_string();
    assert!(
        !page.to_string().contains("923447108"),
        "número completo não sai: {page}"
    );
    assert!(page.to_string().contains("+244 923 ***108"), "{page}");
    let (st, e) = app
        .get(
            &format!(
                "{}?page_size=2&outcome=busy&page_token={token}",
                t(a.org(), "/call-records")
            ),
            Some(&a.token),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("search.page_token_mismatch"))
    );
    let (st, busy) = app
        .get(
            &format!("{}?outcome=busy", t(a.org(), "/call-records")),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(busy["items"].as_array().unwrap().len(), 1);
    assert_eq!(busy["items"][0]["trunk_name"], "Unitel");
    let (st, _) = app.get(&t(a.org(), "/call-records"), Some(&b.token)).await;
    assert!(st == 403 || st == 404);
    let (st, bpage) = app.get(&t(b.org(), "/call-records"), Some(&b.token)).await;
    assert_eq!(st, 200);
    assert_eq!(
        bpage["items"].as_array().unwrap().len(),
        0,
        "B não vê os CDRs de A"
    );

    // Consumo de setembro de 2026.
    let (st, u) = app
        .get(
            &format!("{}?month=2026-09", t(a.org(), "/usage")),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 200, "{u}");
    // call-a 28,20 + call-b 29,70 + call-e (1 min a 9,90); call-c não atendida;
    // call-d sem tronco da org → sem preço, fora dos totais e contada.
    assert_eq!(
        u["totals"],
        json!([{"amount": "67.8000", "currency": "AOA"}])
    );
    assert_eq!(
        u["total_aoa"],
        json!({"amount": "67.8000", "currency": "AOA"})
    );
    assert_eq!(u["calls"], 4);
    assert_eq!(u["unpriced_calls"], 1);
    let (st, e) = app
        .get(
            &format!("{}?month=2026-13", t(a.org(), "/usage")),
            Some(&a.token),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("telephony.invalid_month"))
    );
    let (st, _) = app.get(&t(a.org(), "/usage"), Some(&b.token)).await;
    assert!(st == 403 || st == 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn usage_in_usd_needs_an_exchange_rate(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("VOICE_INTERNAL_SECRET", SECRET)]).await;
    let a = app.new_org("alfa-usd.ao").await;
    let mut intl = unitel();
    intl["name"] = json!("Operadora internacional");
    intl["scope"] = json!("international");
    intl["price_per_min"] = json!({"amount": "0.04", "currency": "USD"});
    let intl = create_trunk(&app, &a.token, a.org(), intl).await;
    assert_eq!(intl["role"], "international");
    let id = intl["id"].as_str().unwrap().to_string();
    let now = chrono::Utc::now().timestamp();
    let (st, _) = ingest(
        &app,
        Some(&basic(SECRET)),
        &cdr("usd-1", a.org(), &id, now + 1, 120, "NORMAL_CLEARING"),
    )
    .await;
    assert_eq!(st, 201);

    let (_, u) = app.get(&t(a.org(), "/usage"), Some(&a.token)).await;
    assert_eq!(
        u["totals"],
        json!([{"amount": "0.0800", "currency": "USD"}])
    );
    assert_eq!(u["total_aoa"], Value::Null, "sem taxa não se inventa Kz");
    assert_eq!(u["total_aoa_reason"], "missing_exchange_rate");
    assert_eq!(u["by_trunk"][0]["share_pct"], Value::Null);

    sqlx::query(
        "INSERT INTO telephony_exchange_rates (id, org_id, currency, aoa_per_unit_e6, valid_from)
         VALUES (gen_random_uuid(), $1::uuid, 'USD', 912500000, now() - interval '1 day')",
    )
    .bind(a.org())
    .execute(&app.db)
    .await
    .unwrap();
    let (_, u) = app.get(&t(a.org(), "/usage"), Some(&a.token)).await;
    assert_eq!(
        u["total_aoa"],
        json!({"amount": "73.0000", "currency": "AOA"})
    );
    assert_eq!(u["by_trunk"][0]["share_pct"], 100.0);

    // ASR medido dos CDRs das últimas 24 h.
    let (_, tr) = app
        .get(&t(a.org(), &format!("/trunks/{id}")), Some(&a.token))
        .await;
    assert_eq!(tr["status"]["asr"], 1.0);
    assert_eq!(tr["status"]["asr_attempts"], 1);

    let (st, e) = app
        .post(
            &t(a.org(), "/exchange-rates"),
            Some(&a.token),
            json!({"currency": "AOA", "aoa_per_unit": "1"}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("telephony.invalid_currency"))
    );
    let (st, r) = app
        .post(
            &t(a.org(), "/exchange-rates"),
            Some(&a.token),
            json!({"currency": "usd", "aoa_per_unit": "915,25"}),
        )
        .await;
    assert_eq!(st, 201, "{r}");
    assert_eq!(r["aoa_per_unit"], "915.250000");
}

// ============================================================
//  ESL falso: estado, reiniciar, teste rápido
// ============================================================

#[sqlx::test(migrations = "./migrations")]
async fn quick_test_call_and_sip_status_through_fake_esl(db: sqlx::PgPool) {
    let esl = spawn_esl("ClueCon-teste", healthy_fs()).await;
    let app = TestApp::spawn_with(
        db,
        &[
            ("TELEPHONY_ESL_ADDR", esl.addr.as_str()),
            ("TELEPHONY_ESL_PASSWORD", "ClueCon-teste"),
        ],
    )
    .await;
    let a = app.new_org("alfa-esl.ao").await;
    let b = app.new_org("beta-esl.ao").await;
    let uni = create_trunk(&app, &a.token, a.org(), unitel()).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let afr = create_trunk(&app, &a.token, a.org(), africell()).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    screen_plan(&app, &a.token, a.org(), &uni, &afr).await;

    // Estado medido no «FreeSWITCH».
    let (st, tr) = app
        .get(&t(a.org(), &format!("/trunks/{uni}")), Some(&a.token))
        .await;
    assert_eq!(st, 200);
    assert_eq!(tr["status"]["state"], "up", "{tr}");
    assert_eq!(tr["status"]["registration"], "registered");
    assert_eq!(tr["status"]["channels_in_use"], 3);
    let (st, reg) = app
        .get(&t(a.org(), "/sip-registration"), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{reg}");
    assert_eq!(reg["media"]["software"], "FreeSWITCH");
    assert_eq!(reg["media"]["version"], "1.10.11-release+git~20231213");
    assert_eq!(reg["codecs_offered"], json!(["OPUS", "G722", "PCMA"]));
    assert_eq!(reg["channels"], json!({"in_use": 6, "max": 90}));
    assert_eq!(reg["trunks"]["up"], 2);
    assert_eq!(reg["sbc"], Value::Null);
    assert!(reg["reasons"].to_string().contains("sbc_not_configured"));
    assert_eq!(reg["quality"]["reason"], "no_calls_in_window");
    assert_eq!(reg["state"], "healthy");

    // Reiniciar registo: comandos certos, auditado.
    let (st, rr) = app
        .post(
            &t(a.org(), "/sip-registration/restart"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!((st, rr["gateways"].as_i64()), (202, Some(2)));
    {
        let log = esl.log.lock().unwrap();
        assert!(
            log.iter()
                .any(|c| c == &format!("sofia profile external killgw dlx-{uni}")),
            "{log:?}"
        );
        assert!(log.iter().any(|c| c == "sofia profile external rescan"));
    }

    // Teste rápido: resultado real (latência medida no ESL).
    let (st, call) = app
        .post(
            &t(a.org(), "/test-calls"),
            Some(&a.token),
            json!({"number": "923 447 108"}),
        )
        .await;
    assert_eq!(st, 202, "{call}");
    assert_eq!(call["status"], "dialing");
    assert_eq!(call["to_masked"], "+244 923 ***108");
    let id = call["id"].as_str().unwrap().to_string();
    let mut done = Value::Null;
    for _ in 0..50 {
        let (_, c) = app
            .get(&t(a.org(), &format!("/test-calls/{id}")), Some(&a.token))
            .await;
        if !c["finished_at"].is_null() {
            done = c;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(done["status"], "answered", "{done}");
    assert!(done["answer_latency_ms"].as_i64().unwrap() >= 150, "{done}");
    assert_eq!(
        (done["hangup_cause"].as_str(), done["billsec"].as_i64()),
        (Some("NORMAL_CLEARING"), Some(1)),
        "{done}"
    );
    assert!(!done["answered_at"].is_null());
    {
        let log = esl.log.lock().unwrap();
        let orig = log
            .iter()
            .find(|c| c.starts_with("bgapi originate "))
            .unwrap();
        assert!(orig.contains(&format!("delonix_trunk_id={uni}]sofia/gateway/dlx-{uni}/244923447108|[delonix_trunk_id={afr}]sofia/gateway/dlx-{afr}/244923447108")), "{orig}");
        assert!(orig.contains(&format!("delonix_org_id={}", a.org())));
        assert!(orig.contains("record_session"), "a regra 0 grava");
    }
    // Ocupado.
    let (_, call) = app
        .post(
            &t(a.org(), "/test-calls"),
            Some(&a.token),
            json!({"number": "923447000"}),
        )
        .await;
    let id2 = call["id"].as_str().unwrap().to_string();
    let mut busy = Value::Null;
    for _ in 0..50 {
        let (_, c) = app
            .get(&t(a.org(), &format!("/test-calls/{id2}")), Some(&a.token))
            .await;
        if !c["finished_at"].is_null() {
            busy = c;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        (busy["status"].as_str(), busy["hangup_cause"].as_str()),
        (Some("busy"), Some("USER_BUSY"))
    );

    // Recusas.
    for (number, code) in [
        ("112", "telephony.test_call_emergency_refused"),
        ("84209", "telephony.destination_not_external"),
        ("08001234", "telephony.number_blocked"),
        ("555", "telephony.no_matching_rule"),
    ] {
        let (st, e) = app
            .post(
                &t(a.org(), "/test-calls"),
                Some(&a.token),
                json!({"number": number}),
            )
            .await;
        assert_eq!((st, e["code"].as_str()), (422, Some(code)), "{number}: {e}");
    }
    assert_eq!(
        esl.log
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("bgapi originate "))
            .count(),
        2,
        "nenhuma recusa chegou ao FreeSWITCH"
    );
    let (st, list) = app
        .get(&t(a.org(), "/test-calls?page_size=1"), Some(&a.token))
        .await;
    assert_eq!(st, 200);
    assert_eq!(list["items"][0]["id"], id2, "o mais recente primeiro");

    // Isolamento.
    let (st, _) = app
        .get(&t(b.org(), &format!("/test-calls/{id}")), Some(&b.token))
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .get(&t(a.org(), &format!("/test-calls/{id}")), Some(&b.token))
        .await;
    assert!(st == 403 || st == 404);
    let (st, _) = app
        .post(
            &t(a.org(), "/test-calls"),
            Some(&b.token),
            json!({"number": "923447108"}),
        )
        .await;
    assert!(st == 403 || st == 404);
    let (st, _) = app
        .post(
            &t(a.org(), "/sip-registration/restart"),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert!(st == 403 || st == 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn media_server_down_or_unconfigured_is_reported_honestly(db: sqlx::PgPool) {
    // Sem ESL.
    let app = TestApp::spawn(db.clone()).await;
    let a = app.new_org("alfa-nocfg.ao").await;
    let (st, reg) = app
        .get(&t(a.org(), "/sip-registration"), Some(&a.token))
        .await;
    assert_eq!(st, 200);
    assert_eq!(reg["state"], "not_configured");
    assert_eq!(reg["media"], Value::Null);
    assert_eq!(reg["channels"]["in_use"], Value::Null);
    let (st, e) = app
        .post(
            &t(a.org(), "/test-calls"),
            Some(&a.token),
            json!({"number": "923447108"}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (422, Some("telephony.not_configured"))
    );
    let (st, e) = app
        .post(
            &t(a.org(), "/sip-registration/restart"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (422, Some("telephony.not_configured"))
    );
    drop(app);

    // ESL com a password errada: media em baixo, estado `down`, sem inventar.
    let esl = spawn_esl("certa", healthy_fs()).await;
    let app = TestApp::spawn_with(
        db,
        &[
            ("TELEPHONY_ESL_ADDR", esl.addr.as_str()),
            ("TELEPHONY_ESL_PASSWORD", "errada"),
        ],
    )
    .await;
    let a = app.new_org("alfa-down.ao").await;
    create_trunk(&app, &a.token, a.org(), unitel()).await;
    let (st, reg) = app
        .get(&t(a.org(), "/sip-registration"), Some(&a.token))
        .await;
    assert_eq!(st, 200);
    assert_eq!(reg["state"], "down", "{reg}");
    assert!(
        reg["media_error"].as_str().unwrap().contains("password"),
        "{reg}"
    );
    let (_, trunks) = app.get(&t(a.org(), "/trunks"), Some(&a.token)).await;
    assert_eq!(trunks["items"][0]["status"]["state"], "unknown");
    assert!(trunks["items"][0]["status"]["reasons"]
        .to_string()
        .contains("sip_status_unavailable"));
}

// ============================================================
//  mod_xml_curl
// ============================================================

#[sqlx::test(migrations = "./migrations")]
async fn xml_curl_dialplan_matches_test_endpoint_and_serves_gateways(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("VOICE_INTERNAL_SECRET", SECRET)]).await;
    let a = app.new_org("alfa-xml.ao").await;
    let uni = create_trunk(&app, &a.token, a.org(), unitel()).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let afr = create_trunk(&app, &a.token, a.org(), africell()).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    screen_plan(&app, &a.token, a.org(), &uni, &afr).await;

    let post = |form: String, auth: Option<String>| {
        let app = &app;
        async move {
            let mut rb = app
                .http
                .post(app.url("/internal/v1/telephony/freeswitch-config"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(form);
            if let Some(a) = auth {
                rb = rb.header("authorization", a);
            }
            let res = rb.send().await.unwrap();
            (res.status().as_u16(), res.text().await.unwrap())
        }
    };
    let org = a.org().to_string();
    let form = |n: &str| {
        format!("section=dialplan&Caller-Context=delonix-outbound&Hunt-Destination-Number={n}&variable_delonix_org_id={org}")
    };
    let (st, _) = post(form("923447108"), None).await;
    assert_eq!(st, 401);
    let (st, x) = post(form("923447108"), Some(basic(SECRET))).await;
    assert_eq!(st, 200);
    assert!(
        x.contains(&format!(
            "hash delonix_trunk {uni} 60 bridge [delonix_trunk_id={uni}]sofia/gateway/dlx-{uni}/244923447108"
        )),
        "{x}"
    );
    assert!(x.find(&format!("dlx-{uni}")).unwrap() < x.find(&format!("dlx-{afr}")).unwrap());
    assert!(x.contains("record_session"));

    let (_, x) = post(form("112"), Some(basic(SECRET))).await;
    assert!(
        !x.contains("record_session") && !x.contains("limit_execute"),
        "{x}"
    );
    assert!(x.contains(&format!("sofia/gateway/dlx-{uni}/112")));
    let (_, x) = post(form("08001234"), Some(basic(SECRET))).await;
    assert!(x.contains("403 Forbidden"));
    let (_, x) = post(form("abc"), Some(basic(SECRET))).await;
    assert!(
        x.contains("404 Not Found"),
        "número inválido nunca cai no plano por omissão: {x}"
    );
    // Outra secção ou contexto: «not found» (o FreeSWITCH usa o seu).
    let (_, x) = post(
        "section=dialplan&Caller-Context=public&Hunt-Destination-Number=1".into(),
        Some(basic(SECRET)),
    )
    .await;
    assert!(x.contains(r#"status="not found""#));

    let (_, g) = post(
        "section=directory&purpose=gateways".into(),
        Some(basic(SECRET)),
    )
    .await;
    assert!(g.contains(&format!(r#"<gateway name="dlx-{uni}">"#)), "{g}");
    assert!(
        g.contains("senha-da-operadora-unitel"),
        "o FreeSWITCH precisa da password decifrada"
    );
    assert!(g.contains(r#"value="sip.unitel.ao:5061;transport=tls""#));
}

// ============================================================
//  Credenciais SIP e SMS
// ============================================================

#[sqlx::test(migrations = "./migrations")]
async fn sip_credentials_are_revealed_only_after_reauth_and_audited(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa-sip.ao").await;
    let b = app.new_org("beta-sip.ao").await;
    let (st, s) = app.get(&t(a.org(), "/sip-settings"), Some(&a.token)).await;
    assert_eq!((st, s["configured"].as_bool()), (200, Some(false)));

    let (st, e) = app
        .put(&t(a.org(), "/sip-settings"), Some(&a.token),
             json!({"domain": "sip.delonix.co.ao", "transport": "tls", "srtp": "mandatory",
                    "codecs": ["opus", "G722", "BANANA"], "username": "alfa", "password": "sip-secreta-123"}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("telephony.invalid_codec"))
    );
    let (st, s) = app
        .put(&t(a.org(), "/sip-settings"), Some(&a.token),
             json!({"domain": "sip.delonix.co.ao", "sbc_host": "sbc-01.lad", "transport": "tls", "srtp": "mandatory",
                    "codecs": ["opus", "G722", "PCMA"], "username": "alfa", "password": "sip-secreta-123"}))
        .await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["codecs"], json!(["OPUS", "G722", "PCMA"]));
    assert_eq!(s["password_configured"], true);
    assert!(!s.to_string().contains("sip-secreta"));

    // O domínio decide a org das chamadas que entram: único (R213).
    let (st, e) = app
        .put(
            &t(b.org(), "/sip-settings"),
            Some(&b.token),
            json!({"domain": "SIP.delonix.co.ao", "transport": "tls", "srtp": "mandatory"}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("telephony.sip_domain_taken")),
        "{e}"
    );

    let path = t(a.org(), "/sip-settings/reveal-credentials");
    let (st, e) = app
        .post(&path, Some(&a.token), json!({"password": "errada"}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (403, Some("telephony.reauth_failed"))
    );
    let (st, c) = app
        .post(&path, Some(&a.token), json!({"password": common::PASSWORD}))
        .await;
    assert_eq!(st, 200, "{c}");
    assert_eq!(c["password"], "sip-secreta-123");
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_logs WHERE action LIKE 'telephony.sip_credentials.%' ORDER BY created_at",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        actions,
        vec![
            "telephony.sip_credentials.reveal_denied",
            "telephony.sip_credentials.revealed"
        ]
    );

    // Força bruta trava (5 falhas), inclusive a password certa.
    for _ in 0..5 {
        app.post(&path, Some(&a.token), json!({"password": "x"}))
            .await;
    }
    let (st, e) = app
        .post(&path, Some(&a.token), json!({"password": common::PASSWORD}))
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (429, Some("telephony.reauth_rate_limited"))
    );

    // Isolamento.
    let (st, _) = app
        .post(&path, Some(&b.token), json!({"password": common::PASSWORD}))
        .await;
    assert!(st == 403 || st == 404);
    let (st, _) = app.get(&t(a.org(), "/sip-settings"), Some(&b.token)).await;
    assert!(st == 403 || st == 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn sms_overview_reports_only_what_the_agent_reports(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa-sms.ao").await;
    let b = app.new_org("beta-sms.ao").await;
    let (st, gw) = app
        .post(
            &format!("/api/orgs/{}/sms/gateways", a.org()),
            Some(&a.token),
            json!({"name": "Servidor Luanda"}),
        )
        .await;
    assert_eq!(st, 201, "{gw}");
    let token = gw["token"].as_str().unwrap().to_string();
    let dev = |key: &str, kind: &str, extra: Value| {
        let mut d = json!({"device_key": key, "vendor_id": "12d1", "product_id": "1506",
            "manufacturer": "Huawei", "product": "E3372", "serial": null, "kind": kind,
            "transport": "modemmanager", "port": null, "capable": true, "reason": null,
            "operator_name": "UNITEL", "signal_percent": 70});
        for (k, v) in extra.as_object().unwrap() {
            d[k] = v.clone();
        }
        d
    };
    let (st, _) = app
        .put("/api/integrations/sms-agent/v1/devices", Some(&token),
             json!({"devices": [
                 dev("modem-1", "modem", json!({})),
                 dev("phone-1", "android_adb", json!({"battery_percent": 84, "balance": {"amount": "2140", "currency": "AOA"}}))
             ]}))
        .await;
    assert_eq!(st, 200);
    let (st, o) = app
        .get(
            &format!("/api/orgs/{}/sms/overview", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 200, "{o}");
    let ch = o["channels"].as_array().unwrap();
    let modem = ch.iter().find(|c| c["kind"] == "usb_modem").unwrap();
    assert_eq!(modem["state"], "active");
    assert_eq!(modem["battery_percent"], Value::Null);
    assert_eq!(modem["balance_reason"], "not_reported_by_gateway");
    let phone = ch.iter().find(|c| c["kind"] == "phone_gateway").unwrap();
    assert_eq!(phone["battery_percent"], 84);
    assert_eq!(
        phone["balance"],
        json!({"amount": "2140.0000", "currency": "AOA"})
    );
    let apis: Vec<&Value> = ch.iter().filter(|c| c["kind"] == "operator_api").collect();
    assert_eq!(apis.len(), 3);
    assert!(apis
        .iter()
        .all(|c| c["state"] == "not_configured" && c["balance"].is_null()));
    assert_eq!(o["totals"]["sent_today"], 0);

    // Um relatório sem saldo não apaga o último saldo conhecido.
    app.put(
        "/api/integrations/sms-agent/v1/devices",
        Some(&token),
        json!({"devices": [dev("phone-1", "android_adb", json!({"battery_percent": 80}))]}),
    )
    .await;
    let (_, o) = app
        .get(
            &format!("/api/orgs/{}/sms/overview", a.org()),
            Some(&a.token),
        )
        .await;
    let phone = o["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "phone_gateway")
        .cloned()
        .unwrap();
    assert_eq!(
        (
            phone["battery_percent"].as_i64(),
            phone["balance"]["amount"].as_str()
        ),
        (Some(80), Some("2140.0000"))
    );

    let (st, _) = app
        .get(
            &format!("/api/orgs/{}/sms/overview", a.org()),
            Some(&b.token),
        )
        .await;
    assert!(st == 403 || st == 404);
}
