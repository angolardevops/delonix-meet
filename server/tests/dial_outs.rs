//! «Ligar a…» a partir da sala, fatia F1 (docs/ligar-a-partir-da-sala.md): contra
//! Postgres real e um FreeSWITCH ESL FALSO (um servidor TCP que fala o protocolo
//! do Event Socket, como em `telephony.rs`).
//!
//! O que um verde aqui prova: o contrato HTTP, a máquina de estados a partir dos
//! eventos do `originate`, o comando ESL que sai (perna `user/`, ponte da sala,
//! nunca um tronco), a capacidade `sessions.dial_out`, o isolamento entre
//! organizações e as recusas. O que NÃO prova: que um FreeSWITCH real faz tocar um
//! Linphone e que a ponte aceita a perna — ver «Não validado» na PR.
mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use common::TestApp;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

const ESL_PW: &str = "esl-de-teste-com-comprimento-suficiente-0123";

// ============================================================
//  ESL falso
// ============================================================

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

/// O comportamento do ramal chamado vem do `sip_username` no `user/…`:
/// `ramal_ocupado` dá `USER_BUSY`, `ramal_naoreg` dá `USER_NOT_REGISTERED`,
/// `ramal_mudo` toca e nunca atende, e qualquer outro atende ao fim de 150 ms e
/// é desligado pelo outro lado 700 ms depois (com 1 s de conversa).
fn script(cmd: &str) -> Vec<(u64, String)> {
    let one = |b: &str| vec![(0, api(b))];
    if cmd.starts_with("hupall ") {
        return one("+OK\n");
    }
    if cmd.starts_with("event ") || cmd.starts_with("filter ") {
        return vec![(0, reply("+OK"))];
    }
    let Some(rest) = cmd.strip_prefix("bgapi originate ") else {
        return one("-ERR command not found\n");
    };
    let job = rest
        .split("Job-UUID: ")
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string();
    let chan = format!("chan-{job}");
    let ev = |name: &str, extra: &[(&str, &str)], body: Option<&str>| {
        let mut h: Vec<(&str, &str)> = vec![
            ("Event-Name", name),
            ("Unique-ID", &chan),
            ("variable_delonix_call_id", &job),
        ];
        h.extend_from_slice(extra);
        event(&h, body)
    };
    let job_end = |body: &str| {
        event(
            &[("Event-Name", "BACKGROUND_JOB"), ("Job-UUID", &job)],
            Some(body),
        )
    };
    let mut out = vec![(0, reply(&format!("+OK Job-UUID: {job}")))];
    out.push((20, ev("CHANNEL_CREATE", &[], None)));
    for (nome, causa) in [
        ("ramal_ocupado", "USER_BUSY"),
        ("ramal_naoreg", "USER_NOT_REGISTERED"),
    ] {
        if rest.contains(&format!("user/{nome}@")) {
            out.push((30, ev("CHANNEL_HANGUP", &[("Hangup-Cause", causa)], None)));
            out.push((0, job_end(&format!("-ERR {causa}\n"))));
            return out;
        }
    }
    out.push((20, ev("CHANNEL_PROGRESS", &[], None)));
    if rest.contains("user/ramal_mudo@") {
        // Toca até ao tempo esgotado do cliente ESL; nunca atende.
        return out;
    }
    out.push((150, ev("CHANNEL_ANSWER", &[], None)));
    out.push((0, job_end(&format!("+OK {chan}\n"))));
    out.push((
        700,
        ev(
            "CHANNEL_HANGUP_COMPLETE",
            &[
                ("Hangup-Cause", "NORMAL_CLEARING"),
                ("variable_billsec", "1"),
            ],
            None,
        ),
    ));
    out
}

async fn spawn_esl() -> FakeEsl {
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
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                w.write_all(b"Content-Type: auth/request\n\n").await.ok();
                let Some(auth) = read_cmd(&mut r).await else {
                    return;
                };
                if auth != format!("auth {ESL_PW}") {
                    w.write_all(reply("-ERR invalid").as_bytes()).await.ok();
                    return;
                }
                w.write_all(reply("+OK accepted").as_bytes()).await.ok();
                while let Some(cmd) = read_cmd(&mut r).await {
                    let cmd = cmd.strip_prefix("api ").unwrap_or(&cmd).to_string();
                    log.lock().unwrap().push(cmd.clone());
                    for (delay, msg) in script(&cmd) {
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

// ============================================================
//  Ajudas
// ============================================================

async fn app_com_ponte(db: sqlx::PgPool, esl: &FakeEsl) -> TestApp {
    TestApp::spawn_with(
        db,
        &[
            ("TELEPHONY_ESL_ADDR", esl.addr.as_str()),
            ("TELEPHONY_ESL_PASSWORD", ESL_PW),
            ("PHONE_BRIDGE_SIP_BIND", "127.0.0.1:5090"),
            ("PHONE_BRIDGE_SIP_ADVERTISE", "127.0.0.1:5090"),
            ("PHONE_BRIDGE_FREESWITCH_IPS", "127.0.0.1"),
        ],
    )
    .await
}

async fn ramal(app: &TestApp, org: &str, numero: &str, sip: &str, activo: bool) -> String {
    sqlx::query_scalar(
        "INSERT INTO voice_extensions
             (org_id, extension, sip_username, sip_password_hash, sip_ha1, label, active)
         VALUES ($1::uuid, $2, $3, 'x', 'x', 'Recepção', $4) RETURNING id::text",
    )
    .bind(org)
    .bind(numero)
    .bind(sip)
    .bind(activo)
    .fetch_one(&app.db)
    .await
    .unwrap()
}

fn d(code: &str, rest: &str) -> String {
    format!("/api/rooms/{code}/dial-outs{rest}")
}

async fn esperar_estado(app: &TestApp, token: &str, code: &str, id: &str, alvo: &str) -> Value {
    let mut ultimo = Value::Null;
    for _ in 0..60 {
        let (st, lista) = app.get(&d(code, ""), Some(token)).await;
        assert_eq!(st, 200, "{lista}");
        ultimo = lista["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == id)
            .cloned()
            .unwrap_or(Value::Null);
        if ultimo["status"] == alvo {
            return ultimo;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("o pedido nunca chegou a «{alvo}»: {ultimo}");
}

fn codigo(room: &Value) -> String {
    room["code"].as_str().unwrap().to_string()
}

// ============================================================
//  Testes
// ============================================================

#[sqlx::test(migrations = "./migrations")]
async fn um_ramal_toca_atende_e_a_perna_vai_para_a_ponte_da_sala(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    let app = app_com_ponte(db, &esl).await;
    let a = app.new_org("dialout-a.ao").await;
    let room = app.new_room(&a, "Sala").await;
    let code = codigo(&room);
    let ok = ramal(&app, a.org(), "201", "ramal_ok", true).await;

    let (st, v) = app
        .post(&d(&code, ""), Some(&a.token), json!({"extension_id": ok}))
        .await;
    assert_eq!(st, 202, "{v}");
    assert!(
        matches!(v["status"].as_str(), Some("queued" | "dialing")),
        "{v}"
    );
    assert_eq!(v["extension"], "201");
    assert_eq!(v["display_name"], "Recepção");
    let id = v["id"].as_str().unwrap().to_string();

    // Um segundo pedido para o MESMO ramal enquanto este vive: recusado.
    let (st, dup) = app
        .post(&d(&code, ""), Some(&a.token), json!({"extension_id": ok}))
        .await;
    assert_eq!(st, 409, "{dup}");
    assert_eq!(dup["code"], "dial_out.already_active", "{dup}");

    let em_chamada = esperar_estado(&app, &a.token, &code, &id, "in_call").await;
    assert!(!em_chamada["answered_at"].is_null(), "{em_chamada}");

    // O comando que saiu: perna `user/` do ramal (nunca um tronco) e a ponte.
    {
        let log = esl.log.lock().unwrap();
        let orig = log
            .iter()
            .find(|c| c.starts_with("bgapi originate "))
            .unwrap();
        assert!(orig.contains("]user/ramal_ok@"), "{orig}");
        assert!(orig.contains("delonix_dial_out=extension"), "{orig}");
        assert!(!orig.contains("sofia/gateway"), "{orig}");
        assert!(
            orig.contains(&format!("sofia/external/room-{code}@127.0.0.1:5090)")),
            "{orig}"
        );
        assert!(orig.contains("rtp_secure_media=mandatory:"), "{orig}");
        assert!(
            !orig.contains("record_session"),
            "sem gravação do chamado: {orig}"
        );
    }

    // O outro lado desliga: `ended`, com o tempo de conversa.
    let fim = esperar_estado(&app, &a.token, &code, &id, "ended").await;
    assert_eq!(fim["billsec"], 1, "{fim}");
    assert!(!fim["ended_at"].is_null());

    // Já não está vivo: pode voltar a ligar-se ao mesmo ramal.
    let (st, outra) = app
        .post(&d(&code, ""), Some(&a.token), json!({"extension_id": ok}))
        .await;
    assert_eq!(st, 202, "{outra}");
    // E o histórico lista os dois, o mais recente primeiro.
    let (_, lista) = app.get(&d(&code, ""), Some(&a.token)).await;
    assert_eq!(lista["items"].as_array().unwrap().len(), 2, "{lista}");
    assert_eq!(lista["items"][0]["id"], outra["id"]);

    // Auditado, com o id (nunca o número) como alvo.
    let alvos: Vec<String> = sqlx::query_scalar(
        "SELECT target FROM audit_logs WHERE org_id = $1::uuid AND action = 'room.dial_out.requested'",
    )
    .bind(a.org())
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert!(alvos.contains(&id), "{alvos:?}");
}

#[sqlx::test(migrations = "./migrations")]
async fn as_causas_do_freeswitch_dao_estados_estaveis(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    let app = app_com_ponte(db, &esl).await;
    let a = app.new_org("dialout-b.ao").await;
    let code = codigo(&app.new_room(&a, "Sala").await);
    for (sip, numero, estado, causa) in [
        ("ramal_ocupado", "202", "declined", None),
        ("ramal_naoreg", "203", "failed", Some("USER_NOT_REGISTERED")),
    ] {
        let r = ramal(&app, a.org(), numero, sip, true).await;
        let (st, v) = app
            .post(&d(&code, ""), Some(&a.token), json!({"extension_id": r}))
            .await;
        assert_eq!(st, 202, "{v}");
        let fim = esperar_estado(&app, &a.token, &code, v["id"].as_str().unwrap(), estado).await;
        assert_eq!(fim["failure_code"].as_str(), causa, "{fim}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn desligar_enquanto_toca_cancela_e_e_idempotente(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    let app = app_com_ponte(db, &esl).await;
    let a = app.new_org("dialout-c.ao").await;
    let code = codigo(&app.new_room(&a, "Sala").await);
    let mudo = ramal(&app, a.org(), "204", "ramal_mudo", true).await;
    let (st, v) = app
        .post(&d(&code, ""), Some(&a.token), json!({"extension_id": mudo}))
        .await;
    assert_eq!(st, 202, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    esperar_estado(&app, &a.token, &code, &id, "ringing").await;

    let (st, h) = app
        .post(
            &d(&code, &format!("/{id}/hangup")),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{h}");
    assert_eq!(h["status"], "cancelled", "{h}");
    {
        let log = esl.log.lock().unwrap();
        assert!(
            log.iter()
                .any(|c| c.starts_with("hupall NORMAL_CLEARING delonix_call_id ")),
            "{log:?}"
        );
    }
    // Repetir não muda nada e não volta a mandar `hupall`.
    let antes = esl.log.lock().unwrap().len();
    let (st, h2) = app
        .post(
            &d(&code, &format!("/{id}/hangup")),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(
        (st, h2["status"].as_str()),
        (200, Some("cancelled")),
        "{h2}"
    );
    assert_eq!(esl.log.lock().unwrap().len(), antes);
    // Um id que não é desta sala: 404.
    let (st, _) = app
        .post(
            &d(&code, &format!("/{}/hangup", uuid::Uuid::new_v4())),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404);
}

#[sqlx::test(migrations = "./migrations")]
async fn quem_pode_ligar_a_quem(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    let app = app_com_ponte(db, &esl).await;
    let a = app.new_org("dialout-d.ao").await;
    let b = app.new_org("dialout-e.ao").await;
    let code = codigo(&app.new_room(&a, "Sala A").await);
    let ra = ramal(&app, a.org(), "205", "ramal_a", true).await;
    let rb = ramal(&app, b.org(), "205", "ramal_b", true).await;
    let inactivo = ramal(&app, a.org(), "206", "ramal_inactivo", false).await;
    let corpo = |r: &str| json!({"extension_id": r});

    // Sem sessão.
    let (st, _) = app.post(&d(&code, ""), None, corpo(&ra)).await;
    assert_eq!(st, 401);
    // Outra organização: a sala e o ramal respondem como se não existissem.
    let (st, v) = app.post(&d(&code, ""), Some(&b.token), corpo(&rb)).await;
    assert_eq!(st, 404, "B não vê a sala de A: {v}");
    let code_b = codigo(&app.new_room(&b, "Sala B").await);
    let (st, v) = app.post(&d(&code_b, ""), Some(&b.token), corpo(&ra)).await;
    assert_eq!(st, 404, "B não alcança o ramal de A: {v}");
    let (st, _) = app.get(&d(&code, ""), Some(&b.token)).await;
    assert_eq!(st, 404);
    // Um colega sem `sessions.dial_out` (papel `member`), mesmo na sua sala: 403.
    let m = app.add_member(&a, "colega", "member").await;
    let code_m = codigo(&app.new_room(&m, "Sala do colega").await);
    let (st, v) = app.post(&d(&code_m, ""), Some(&m.token), corpo(&ra)).await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(v["code"], "authz.missing_capability", "{v}");
    // Um colega de A que NÃO gere a sala de A (não é anfitrião nem co-anfitrião): 403.
    let (st, v) = app.post(&d(&code, ""), Some(&m.token), corpo(&ra)).await;
    assert_eq!(st, 403, "{v}");
    // Ramal desactivado.
    let (st, v) = app
        .post(&d(&code, ""), Some(&a.token), corpo(&inactivo))
        .await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["code"], "dial_out.extension_inactive", "{v}");
    // Ramal que não existe.
    let (st, _) = app
        .post(
            &d(&code, ""),
            Some(&a.token),
            corpo(&uuid::Uuid::new_v4().to_string()),
        )
        .await;
    assert_eq!(st, 404);
    // Nada disto chegou ao ESL.
    assert!(
        !esl.log
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("bgapi originate")),
        "uma recusa nunca origina"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn sala_cifrada_e_ponte_desligada_recusam(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    let app = app_com_ponte(db.clone(), &esl).await;
    let a = app.new_org("dialout-f.ao").await;
    let (st, sala) = app
        .post(
            "/api/rooms",
            Some(&a.token),
            json!({"name": "Cifrada", "topology": "sfu", "e2ee": true}),
        )
        .await;
    assert_eq!(st, 200, "{sala}");
    let r = ramal(&app, a.org(), "207", "ramal_e2ee", true).await;
    let (st, v) = app
        .post(
            &d(&codigo(&sala), ""),
            Some(&a.token),
            json!({"extension_id": r}),
        )
        .await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["code"], "dial_out.room_e2ee", "{v}");
    drop(app);

    // Sem a ponte configurada: recusa com causa estável, nunca «a tocar».
    let app = TestApp::spawn_with(
        db,
        &[
            ("TELEPHONY_ESL_ADDR", esl.addr.as_str()),
            ("TELEPHONY_ESL_PASSWORD", ESL_PW),
        ],
    )
    .await;
    let a = app.login("admin@dialout-f.ao").await;
    let (_, orgs) = app.get("/api/orgs", Some(&a.token)).await;
    let org = orgs[0]["id"].as_str().unwrap().to_string();
    let code = codigo(&app.new_room(&a, "Sem ponte").await);
    let r = ramal(&app, &org, "208", "ramal_sem_ponte", true).await;
    let (st, v) = app
        .post(&d(&code, ""), Some(&a.token), json!({"extension_id": r}))
        .await;
    assert_eq!(st, 422, "{v}");
    assert_eq!(v["code"], "dial_out.bridge_not_configured", "{v}");
    drop(app);
}
