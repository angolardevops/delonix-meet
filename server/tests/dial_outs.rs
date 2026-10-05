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
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use tokio_tungstenite::tungstenite::Message;

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
    if rest.contains("user/ramal_lento@") {
        // O canal só aparece 400 ms depois: dá tempo ao anfitrião de cancelar.
        out.clear();
        out.push((0, reply(&format!("+OK Job-UUID: {job}"))));
        out.push((400, ev("CHANNEL_CREATE", &[], None)));
        out.push((20, ev("CHANNEL_PROGRESS", &[], None)));
        out.push((50, ev("CHANNEL_ANSWER", &[], None)));
        out.push((0, job_end(&format!("+OK {chan}\n"))));
        return out;
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
async fn cancelar_antes_de_o_canal_existir_nao_deixa_a_perna_a_tocar(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    let app = app_com_ponte(db, &esl).await;
    let a = app.new_org("dialout-g.ao").await;
    let code = codigo(&app.new_room(&a, "Sala").await);
    let lento = ramal(&app, a.org(), "209", "ramal_lento", true).await;
    let (st, v) = app
        .post(
            &d(&code, ""),
            Some(&a.token),
            json!({"extension_id": lento}),
        )
        .await;
    assert_eq!(st, 202, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    // Cancela já, antes de o FreeSWITCH criar o canal.
    let (st, h) = app
        .post(
            &d(&code, &format!("/{id}/hangup")),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!((st, h["status"].as_str()), (200, Some("cancelled")), "{h}");
    // Ou nunca se origina, ou, se se originou, o sinal de vida que chega depois
    // volta a mandar desligar: tem de haver um `hupall` DEPOIS do `originate`.
    let mut ok = false;
    for _ in 0..60 {
        {
            let log = esl.log.lock().unwrap();
            let origem = log.iter().position(|c| c.starts_with("bgapi originate "));
            ok = match origem {
                None => true,
                Some(i) => log[i..].iter().any(|c| c.starts_with("hupall ")),
            };
        }
        if ok {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        ok,
        "a perna cancelada ficou a tocar: {:?}",
        esl.log.lock().unwrap()
    );
    // E o estado continua `cancelled`: a chamada tardia não o reescreve.
    tokio::time::sleep(Duration::from_millis(800)).await;
    let (_, lista) = app.get(&d(&code, ""), Some(&a.token)).await;
    assert_eq!(lista["items"][0]["status"], "cancelled", "{lista}");
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

#[sqlx::test(migrations = "./migrations")]
async fn a_perna_de_um_dial_out_entra_com_o_nome_do_ramal_e_a_pessoa(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    let app = app_com_ponte(db, &esl).await;
    let a = app.new_org("dialout-h.ao").await;
    let colega = app.add_member(&a, "colega", "member").await;
    let sala = app.new_room(&a, "Sala").await;
    let code = codigo(&sala);
    // Um ramal que é de uma PESSOA (member_id), com a etiqueta «Recepção».
    let ext: String = sqlx::query_scalar(
        "INSERT INTO voice_extensions
             (org_id, extension, sip_username, sip_password_hash, sip_ha1, label, member_id)
         VALUES ($1::uuid, '210', 'ramal_pessoa', 'x', 'x', 'Recepção', $2::uuid) RETURNING id::text",
    )
    .bind(a.org())
    .bind(&colega.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    let (st, v) = app
        .post(&d(&code, ""), Some(&a.token), json!({"extension_id": ext}))
        .await;
    assert_eq!(st, 202, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    esperar_estado(&app, &a.token, &code, &id, "in_call").await;
    let call_id: uuid::Uuid =
        sqlx::query_scalar("SELECT telephony_call_id FROM room_dial_outs WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    let room_id: uuid::Uuid = sala["id"].as_str().unwrap().parse().unwrap();

    // O que a ponte faz quando a perna atende: senta-a no censo.
    let leg = uuid::Uuid::new_v4();
    delonix_server::seat_phone_caller(&app.state, room_id, &code, leg, None, Some(call_id)).await;
    let seat = app
        .state
        .hub
        .seat_of(room_id, leg)
        .expect("a perna está na sala");
    assert!(!seat.anonymous, "já não é «Telefone» sem nome");
    assert_eq!(seat.call_id, Some(call_id));
    let colega_id: uuid::Uuid = colega.user_id.parse().unwrap();
    assert_eq!(
        app.state.hub.phone_legs_of(room_id, colega_id),
        vec![(leg, Some(call_id))]
    );
    // Um `call_id` que não é de um dial-out desta sala (vem de um cabeçalho que ninguém assina)
    // nunca se guarda: senão decidia o alvo do `hupall`.
    let solta = uuid::Uuid::new_v4();
    let leg2 = uuid::Uuid::new_v4();
    delonix_server::seat_phone_caller(&app.state, room_id, &code, leg2, None, Some(solta)).await;
    let seat2 = app.state.hub.seat_of(room_id, leg2).unwrap();
    assert_eq!((seat2.call_id, seat2.anonymous), (None, true));
    // Outra sala com o mesmo call_id não identifica ninguém.
    let outra = codigo(&app.new_room(&a, "Outra").await);
    assert!(
        delonix_server::dial_outs_caller_of_call(&app.db, call_id, &outra)
            .await
            .is_none()
    );
}

/// Espera por uma mensagem do tipo `tipo` no socket (as outras ignoram-se).
async fn esperar_msg<S>(ws: &mut S, tipo: &str, ms: u64) -> Option<Value>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let fim = tokio::time::Instant::now() + Duration::from_millis(ms);
    loop {
        let resto = fim.saturating_duration_since(tokio::time::Instant::now());
        let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(resto, ws.next()).await else {
            return None;
        };
        let v: Value = serde_json::from_str(&t).unwrap();
        if v["type"] == tipo {
            return Some(v);
        }
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_mesma_pessoa_no_browser_e_ao_telefone_escolhe_onde_continuar(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    let app = app_com_ponte(db, &esl).await;
    let a = app.new_org("dialout-i.ao").await;
    let sala = app.new_room(&a, "Sala").await;
    let code = codigo(&sala);
    // A anfitriã tem um ramal seu (ex.: o Linphone dela) e chama-o a partir da sala.
    let ext: String = sqlx::query_scalar(
        "INSERT INTO voice_extensions
             (org_id, extension, sip_username, sip_password_hash, sip_ha1, label, member_id)
         VALUES ($1::uuid, '211', 'ramal_dela', 'x', 'x', 'Ana (Linphone)', $2::uuid) RETURNING id::text",
    )
    .bind(a.org())
    .bind(&a.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();

    // Ela está na sala pelo browser (WebSocket).
    let (st, join) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{join}");
    let url = format!(
        "{}/ws?token={}",
        app.base.replacen("http", "ws", 1),
        join["room_token"].as_str().unwrap()
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.expect("/ws");
    assert!(esperar_msg(&mut ws, "joined", 10_000).await.is_some());

    let (st, v) = app
        .post(&d(&code, ""), Some(&a.token), json!({"extension_id": ext}))
        .await;
    assert_eq!(st, 202, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    esperar_estado(&app, &a.token, &code, &id, "in_call").await;
    let call_id: uuid::Uuid =
        sqlx::query_scalar("SELECT telephony_call_id FROM room_dial_outs WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    let room_id: uuid::Uuid = sala["id"].as_str().unwrap().parse().unwrap();

    // A perna atende e senta-se na sala: o browser dela é avisado, com `can_hangup`.
    let leg = uuid::Uuid::new_v4();
    delonix_server::seat_phone_caller(&app.state, room_id, &code, leg, None, Some(call_id)).await;
    let aviso = esperar_msg(&mut ws, "duplicate-device", 5_000)
        .await
        .expect("o browser da mesma pessoa tem de ser avisado");
    assert_eq!(aviso["phone_id"], leg.to_string());
    assert_eq!(aviso["can_hangup"], true);

    // Outra conta da mesma organização, na mesma sala, com o id VERDADEIRO da perna: recusado.
    let colega = app.add_member(&a, "intruso", "member").await;
    let (st, j2) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&colega.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{j2}");
    let url2 = format!(
        "{}/ws?token={}",
        app.base.replacen("http", "ws", 1),
        j2["room_token"].as_str().unwrap()
    );
    let (mut ws2, _) = tokio_tungstenite::connect_async(url2)
        .await
        .expect("/ws do colega");
    ws2.send(Message::Text(
        json!({"type": "device-choice", "phone_id": leg, "keep": "meet"})
            .to_string(),
    ))
    .await
    .unwrap();
    // Quem está na sala de espera nem chega ao tratamento; quem chegasse receberia «gone».
    // Em qualquer caso, nunca se actua na perna de outra pessoa.
    if let Some(r2) = esperar_msg(&mut ws2, "duplicate-resolved", 1_500).await {
        assert_eq!(
            r2["outcome"], "gone",
            "a perna de outra pessoa nunca se toca: {r2}"
        );
    }
    assert!(
        !esl.log
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("hupall ")),
        "o hupall de outra pessoa nunca sai"
    );

    // Uma perna que não é dela não faz nada.
    ws.send(Message::Text(
        json!({"type": "device-choice", "phone_id": uuid::Uuid::new_v4(), "keep": "meet"})
            .to_string(),
    ))
    .await
    .unwrap();
    let r = esperar_msg(&mut ws, "duplicate-resolved", 5_000)
        .await
        .expect("responde, para o diálogo não ficar preso");
    assert_eq!(r["outcome"], "gone", "{r}");
    assert!(
        !esl.log
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("hupall ")),
        "uma perna alheia nunca se desliga"
    );

    // «Continuar só no Meet»: a chamada que a sala fez tocar desliga-se.
    ws.send(Message::Text(
        json!({"type": "device-choice", "phone_id": leg, "keep": "meet"})
            .to_string(),
    ))
    .await
    .unwrap();
    let r = esperar_msg(&mut ws, "duplicate-resolved", 5_000)
        .await
        .expect("resposta");
    assert_eq!(
        (r["outcome"].as_str(), r["phone_id"].as_str()),
        (Some("hung_up"), Some(leg.to_string().as_str()))
    );
    assert!(
        esl.log
            .lock()
            .unwrap()
            .iter()
            .any(|c| c == &format!("hupall NORMAL_CLEARING delonix_call_id {call_id}")),
        "{:?}",
        esl.log.lock().unwrap()
    );

    // «Nos dois»: nada se desliga.
    ws.send(Message::Text(
        json!({"type": "device-choice", "phone_id": leg, "keep": "both"})
            .to_string(),
    ))
    .await
    .unwrap();
    let r = esperar_msg(&mut ws, "duplicate-resolved", 5_000)
        .await
        .expect("resposta");
    assert_eq!(r["outcome"], "both");
}
