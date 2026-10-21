//! ADR-0023 — «ligar a partir da sala» a um ramal móvel com a app morta. Contra Postgres real e um
//! FreeSWITCH ESL FALSO (um servidor TCP que fala o protocolo do Event Socket, como em `dial_outs.rs`).
//!
//! O que se mede: com `PUSH_WAIT_SECS` > 0 e o ramal sem registo, o servidor acorda os aparelhos (fornecedor
//! `lab`) e só origina depois de o ramal se registar; sem aparelhos, ou com a funcionalidade desligada (o
//! valor por omissão), liga já, como antes; e um pedido cancelado durante a espera nunca chega a originar.
//! O que NÃO prova: que um FreeSWITCH real faz o `sofia_contact` que aqui se simula, nem um push real.
mod common;

use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use axum::{extract::State, routing::post, Json, Router};
use common::{Account, TestApp};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

const ESL_PW: &str = "esl-de-teste-com-comprimento-suficiente-0123";

// ---------- ESL falso, só o que esta prova precisa ----------

struct FakeEsl {
    addr: String,
    log: Arc<Mutex<Vec<String>>>,
    /// O que o `sofia_contact` responde: o ramal está registado?
    registado: Arc<AtomicBool>,
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

/// A chamada que o ramal atende ao fim de 150 ms e desliga 700 ms depois (como em `dial_outs.rs`).
fn originate_script(rest: &str) -> Vec<(u64, String)> {
    let job = rest
        .split("Job-UUID: ")
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string();
    let chan = format!("chan-{job}");
    let ev = |name: &str, extra: &[(&str, &str)]| {
        let mut h: Vec<(&str, &str)> = vec![
            ("Event-Name", name),
            ("Unique-ID", &chan),
            ("variable_delonix_call_id", &job),
        ];
        h.extend_from_slice(extra);
        event(&h, None)
    };
    vec![
        (0, reply(&format!("+OK Job-UUID: {job}"))),
        (20, ev("CHANNEL_CREATE", &[])),
        (20, ev("CHANNEL_PROGRESS", &[])),
        (150, ev("CHANNEL_ANSWER", &[])),
        (
            0,
            event(
                &[("Event-Name", "BACKGROUND_JOB"), ("Job-UUID", &job)],
                Some(&format!("+OK {chan}\n")),
            ),
        ),
        (
            700,
            ev(
                "CHANNEL_HANGUP_COMPLETE",
                &[
                    ("Hangup-Cause", "NORMAL_CLEARING"),
                    ("variable_billsec", "1"),
                ],
            ),
        ),
    ]
}

async fn spawn_esl() -> FakeEsl {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let log = Arc::new(Mutex::new(Vec::new()));
    let registado = Arc::new(AtomicBool::new(false));
    let (log2, registado2) = (log.clone(), registado.clone());
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                return;
            };
            let (log, registado) = (log2.clone(), registado2.clone());
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
                    let saida: Vec<(u64, String)> = if cmd.starts_with("sofia_contact ") {
                        let b = if registado.load(Ordering::SeqCst) {
                            "sofia/internal/sip:ramal@127.0.0.1:5060"
                        } else {
                            "error/user_not_registered"
                        };
                        vec![(0, api(b))]
                    } else if cmd.starts_with("hupall ") {
                        vec![(0, api("+OK\n"))]
                    } else if cmd.starts_with("event ") || cmd.starts_with("filter ") {
                        vec![(0, reply("+OK"))]
                    } else if let Some(rest) = cmd.strip_prefix("bgapi originate ") {
                        originate_script(rest)
                    } else {
                        vec![(0, api("-ERR command not found\n"))]
                    };
                    for (delay, msg) in saida {
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
    FakeEsl {
        addr,
        log,
        registado,
    }
}

impl FakeEsl {
    fn consultas(&self) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("sofia_contact "))
            .count()
    }
    fn originates(&self) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("bgapi originate "))
            .count()
    }
    /// Quantas consultas ao registo havia quando saiu o primeiro `originate` (`None` se ainda não saiu).
    fn consultas_antes_do_originate(&self) -> Option<usize> {
        let log = self.log.lock().unwrap();
        let i = log.iter().position(|c| c.starts_with("bgapi originate "))?;
        Some(
            log[..i]
                .iter()
                .filter(|c| c.starts_with("sofia_contact "))
                .count(),
        )
    }
}

// ---------- o fornecedor `lab`: ao receber o pedido, o «aparelho» regista-se ----------

struct Lab {
    url: String,
    pedidos: Arc<Mutex<Vec<Value>>>,
    recebidos: Arc<AtomicUsize>,
}

/// `registar_apos`: quanto tempo depois do pedido o ramal «se regista» no ESL falso (`None`: nunca).
async fn lab(registado: Arc<AtomicBool>, registar_apos: Option<Duration>) -> Lab {
    #[derive(Clone)]
    struct Est {
        pedidos: Arc<Mutex<Vec<Value>>>,
        recebidos: Arc<AtomicUsize>,
        registado: Arc<AtomicBool>,
        apos: Option<Duration>,
    }
    async fn entrega(State(e): State<Est>, Json(v): Json<Value>) {
        e.pedidos.lock().unwrap().push(v);
        e.recebidos.fetch_add(1, Ordering::SeqCst);
        if let Some(d) = e.apos {
            let r = e.registado.clone();
            tokio::spawn(async move {
                tokio::time::sleep(d).await;
                r.store(true, Ordering::SeqCst);
            });
        }
    }
    let est = Est {
        pedidos: Arc::default(),
        recebidos: Arc::default(),
        registado,
        apos: registar_apos,
    };
    let (pedidos, recebidos) = (est.pedidos.clone(), est.recebidos.clone());
    let app = Router::new().route("/push", post(entrega)).with_state(est);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Lab {
        url: format!("http://127.0.0.1:{port}/push"),
        pedidos,
        recebidos,
    }
}

// ---------- ajudas ----------

async fn app_com(db: sqlx::PgPool, esl: &FakeEsl, lab: &Lab, espera: Option<&str>) -> TestApp {
    let mut extra = vec![
        ("TELEPHONY_ESL_ADDR", esl.addr.as_str()),
        ("TELEPHONY_ESL_PASSWORD", ESL_PW),
        ("PHONE_BRIDGE_SIP_BIND", "127.0.0.1:5090"),
        ("PHONE_BRIDGE_SIP_ADVERTISE", "127.0.0.1:5090"),
        ("PHONE_BRIDGE_FREESWITCH_IPS", "127.0.0.1"),
        ("PUSH_LAB_URL", lab.url.as_str()),
        ("OUTBOUND_ALLOW_HOSTS", "127.0.0.1"),
    ];
    if let Some(e) = espera {
        extra.push(("PUSH_WAIT_SECS", e));
    }
    TestApp::spawn_with(db, &extra).await
}

/// Um ramal de uma pessoa (criado pelo administrador) e o aparelho `lab` dessa pessoa.
async fn ramal_com_aparelho(
    app: &TestApp,
    admin: &Account,
    local: &str,
    numero: &str,
    com_aparelho: bool,
) -> String {
    let pessoa = app.add_member(admin, local, "member").await;
    let (st, r) = app
        .post(
            &format!("/api/orgs/{}/extensions", admin.org()),
            Some(&admin.token),
            json!({"extension": numero, "member_id": pessoa.user_id}),
        )
        .await;
    assert_eq!(st, 200, "{r}");
    if com_aparelho {
        let id = uuid::Uuid::new_v4();
        let (st, r) = app
            .put(
                &format!("/api/orgs/{}/my-extension/devices/{id}", admin.org()),
                Some(&pessoa.token),
                json!({"platform": "android", "provider": "lab", "push_token": format!("tok-{local}")}),
            )
            .await;
        assert_eq!(st, 201, "{r}");
    }
    r["id"].as_str().unwrap().to_string()
}

fn d(code: &str, rest: &str) -> String {
    format!("/api/rooms/{code}/dial-outs{rest}")
}

async fn esperar_estado(app: &TestApp, token: &str, code: &str, id: &str, alvo: &str) -> Value {
    let mut ultimo = Value::Null;
    for _ in 0..120 {
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

async fn ligar(app: &TestApp, a: &Account, ramal: &str) -> (String, String) {
    let room = app.new_room(a, "Sala").await;
    let code = room["code"].as_str().unwrap().to_string();
    let (st, v) = app
        .post(
            &d(&code, ""),
            Some(&a.token),
            json!({"extension_id": ramal}),
        )
        .await;
    assert_eq!(st, 202, "{v}");
    (code, v["id"].as_str().unwrap().to_string())
}

// ---------- testes ----------

#[sqlx::test(migrations = "./migrations")]
async fn um_ramal_morto_e_acordado_e_so_se_origina_depois_do_registo(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    let lab = lab(esl.registado.clone(), Some(Duration::from_millis(700))).await;
    let app = app_com(db, &esl, &lab, Some("10")).await;
    let a = app.new_org("acordar-a.ao").await;
    let ramal = ramal_com_aparelho(&app, &a, "ana", "1004", true).await;

    let (code, id) = ligar(&app, &a, &ramal).await;
    esperar_estado(&app, &a.token, &code, &id, "in_call").await;

    assert_eq!(lab.recebidos.load(Ordering::SeqCst), 1, "um só push");
    let push = lab.pedidos.lock().unwrap()[0].clone();
    assert_eq!(push["kind"], "incoming_call");
    assert!(
        push["call_uuid"].as_str().is_some_and(|c| c.len() == 36),
        "o call_uuid é o id da chamada: {push}"
    );
    assert!(
        !push.to_string().contains("tok-ana"),
        "o token não vai no push: {push}"
    );
    // A consulta ao registo repetiu-se até o ramal «se registar», e só então saiu o originate.
    let antes = esl
        .consultas_antes_do_originate()
        .expect("saiu um originate");
    assert!(
        antes >= 2,
        "o originate saiu sem esperar pelo registo ({antes} consultas)"
    );
    assert_eq!(esl.originates(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn sem_aparelhos_liga_ja_como_antes(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    let lab = lab(esl.registado.clone(), Some(Duration::from_millis(50))).await;
    let app = app_com(db, &esl, &lab, Some("10")).await;
    let a = app.new_org("acordar-b.ao").await;
    let ramal = ramal_com_aparelho(&app, &a, "ana", "1004", false).await;

    let (code, id) = ligar(&app, &a, &ramal).await;
    esperar_estado(&app, &a.token, &code, &id, "in_call").await;
    assert_eq!(lab.recebidos.load(Ordering::SeqCst), 0, "ninguém a acordar");
    assert_eq!(esl.consultas(), 1, "uma só consulta e liga já");
    assert_eq!(esl.consultas_antes_do_originate(), Some(1));
}

#[sqlx::test(migrations = "./migrations")]
async fn com_a_funcionalidade_desligada_nada_muda(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    let lab = lab(esl.registado.clone(), Some(Duration::from_millis(50))).await;
    // Sem PUSH_WAIT_SECS: o valor por omissão (0) desliga.
    let app = app_com(db, &esl, &lab, None).await;
    let a = app.new_org("acordar-c.ao").await;
    let ramal = ramal_com_aparelho(&app, &a, "ana", "1004", true).await;

    let (code, id) = ligar(&app, &a, &ramal).await;
    esperar_estado(&app, &a.token, &code, &id, "in_call").await;
    assert_eq!(
        lab.recebidos.load(Ordering::SeqCst),
        0,
        "não acordou ninguém"
    );
    assert_eq!(esl.consultas(), 0, "nem sequer perguntou pelo registo");
}

#[sqlx::test(migrations = "./migrations")]
async fn um_ramal_ja_registado_nao_acorda_ninguem(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    esl.registado.store(true, Ordering::SeqCst);
    let lab = lab(esl.registado.clone(), None).await;
    let app = app_com(db, &esl, &lab, Some("10")).await;
    let a = app.new_org("acordar-d.ao").await;
    let ramal = ramal_com_aparelho(&app, &a, "ana", "1004", true).await;

    let (code, id) = ligar(&app, &a, &ramal).await;
    esperar_estado(&app, &a.token, &code, &id, "in_call").await;
    assert_eq!(lab.recebidos.load(Ordering::SeqCst), 0);
    assert_eq!(esl.consultas(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn cancelar_durante_a_espera_nunca_origina(db: sqlx::PgPool) {
    let esl = spawn_esl().await;
    // O aparelho nunca acorda: a chamada fica à espera até ao limite ou ao cancelamento.
    let lab = lab(esl.registado.clone(), None).await;
    let app = app_com(db, &esl, &lab, Some("20")).await;
    let a = app.new_org("acordar-e.ao").await;
    let ramal = ramal_com_aparelho(&app, &a, "ana", "1004", true).await;

    let (code, id) = ligar(&app, &a, &ramal).await;
    // Dá tempo ao pedido de acordar e de entrar na espera.
    for _ in 0..50 {
        if lab.recebidos.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(lab.recebidos.load(Ordering::SeqCst), 1);
    let (st, h) = app
        .post(
            &d(&code, &format!("/{id}/hangup")),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{h}");
    assert_eq!(h["status"], "cancelled", "{h}");
    // O aparelho acorda TARDE, já depois do cancelamento: sem a verificação do cancelamento, a espera via o
    // registo e originava. A espera dá-se conta (poll de 500 ms) e não origina.
    esl.registado.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(esl.originates(), 0, "originou depois de cancelado");
    let (_, lista) = app.get(&d(&code, ""), Some(&a.token)).await;
    let item = lista["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id)
        .unwrap();
    assert_eq!(item["status"], "cancelled", "{item}");
}
