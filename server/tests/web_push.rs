//! Web Push (A-03, ADR-0032): o toque de uma reunião chega a quem tem a
//! aplicação fechada.
//!
//! **O que estes testes guardam.** O critério do A-03 é que um `ring` a quem não
//! tem socket deixe de devolver essa pessoa em `offline` — e que o que a leva
//! para `ringing` seja um aviso que um serviço de push ACEITOU, não uma linha de
//! log. Por isso o serviço de push aqui é um servidor HTTP falso que guarda o
//! que recebe, e o aviso é DECIFRADO com as chaves do browser: é assim que se
//! prova que vai cifrado (RFC 8291), assinado (RFC 8292) e sem o assunto da
//! reunião.
//!
//! **O que NÃO provam:** que um browser real, fechado, mostra a notificação.
//! Isso só se observa num aparelho — ver o «Como fechou» do prompt do A-03.

mod common;

use std::sync::{
    atomic::{AtomicU16, Ordering},
    Arc, Mutex,
};

use base64::Engine as _;
use chrono::{Duration, Utc};
use common::TestApp;
use serde_json::{json, Value};
use web_push_native::{p256::SecretKey, Auth};

const SUBJECT: &str = "mailto:ops@meet.test";
const TITULO_SECRETO: &str = "Despedimento colectivo Q4";

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// A chave VAPID da «instalação» de teste: um escalar fixo, para o teste ser
/// reprodutível.
fn vapid_privada() -> String {
    b64(&[9u8; 32])
}

/// As chaves de um «browser»: o par ECDH P-256 e o segredo de autenticação.
struct Browser {
    secreta: SecretKey,
    auth: [u8; 16],
}

impl Browser {
    fn novo(semente: u8) -> Self {
        Browser {
            secreta: SecretKey::from_slice(&[semente; 32]).unwrap(),
            auth: [semente.wrapping_add(1); 16],
        }
    }
    fn subscricao(&self, endpoint: &str) -> Value {
        json!({
            "endpoint": endpoint,
            "expirationTime": null,
            "keys": {
                "p256dh": b64(&self.secreta.public_key().to_sec1_bytes()),
                "auth": b64(&self.auth),
            }
        })
    }
    /// Decifra um aviso como o browser decifraria.
    fn decifra(&self, corpo: Vec<u8>) -> Value {
        let claro =
            web_push_native::decrypt(corpo, &self.secreta, &Auth::clone_from_slice(&self.auth))
                .expect("o aviso não se decifra com as chaves do browser");
        serde_json::from_slice(&claro).expect("o aviso decifrado não é JSON")
    }
}

/// Um pedido que o serviço de push falso recebeu.
#[derive(Clone)]
struct Recebido {
    cabecalhos: Vec<(String, String)>,
    corpo: Vec<u8>,
}

impl Recebido {
    fn cabecalho(&self, nome: &str) -> Option<&str> {
        self.cabecalhos
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(nome))
            .map(|(_, v)| v.as_str())
    }
}

/// Um serviço de push falso: aceita POSTs em `/push/{qualquer}`, guarda-os e
/// responde com o estado que o teste escolher (201 por omissão).
struct ServicoFalso {
    base: String,
    recebidos: Arc<Mutex<Vec<Recebido>>>,
    estado: Arc<AtomicU16>,
}

impl ServicoFalso {
    async fn arranca() -> Self {
        let recebidos: Arc<Mutex<Vec<Recebido>>> = Arc::default();
        let estado = Arc::new(AtomicU16::new(201));
        let (r, e) = (recebidos.clone(), estado.clone());
        let app = axum::Router::new().route(
            "/push/{id}",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, corpo: axum::body::Bytes| {
                    let (r, e) = (r.clone(), e.clone());
                    async move {
                        r.lock().unwrap().push(Recebido {
                            cabecalhos: headers
                                .iter()
                                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                                .collect(),
                            corpo: corpo.to_vec(),
                        });
                        axum::http::StatusCode::from_u16(e.load(Ordering::SeqCst)).unwrap()
                    }
                },
            ),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        ServicoFalso {
            base,
            recebidos,
            estado,
        }
    }
    fn endpoint(&self, id: &str) -> String {
        format!("{}/push/{id}", self.base)
    }
    fn recebidos(&self) -> Vec<Recebido> {
        self.recebidos.lock().unwrap().clone()
    }
}

/// Uma instalação COM Web Push configurado. O `127.0.0.1` em
/// `OUTBOUND_ALLOW_HOSTS` é o que deixa o serviço falso ser destino — a mesma
/// porta por onde um operador declara um serviço próprio.
async fn com_push(db: sqlx::PgPool) -> TestApp {
    let chave = vapid_privada();
    TestApp::spawn_with(
        db,
        &[
            ("WEB_PUSH_VAPID_PRIVATE_KEY", chave.as_str()),
            ("WEB_PUSH_SUBJECT", SUBJECT),
            ("OUTBOUND_ALLOW_HOSTS", "127.0.0.1"),
        ],
    )
    .await
}

async fn v1(
    app: &TestApp,
    method: reqwest::Method,
    path: &str,
    key: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let auth = format!("Bearer {key}");
    let r = app
        .raw(
            method,
            &format!("/api/v1{path}"),
            &[("Authorization", &auth)],
            body,
        )
        .await;
    (r.status, r.json())
}

/// Uma reunião da org do `admin`, com `convidado` convidado, e o `ring` dela
/// pela API pública. Devolve `(id, room_code)`.
async fn reuniao(app: &TestApp, chave: &str, anfitriao: &str, convidado: &str) -> (String, String) {
    let (st, m) = v1(
        app,
        reqwest::Method::POST,
        "/meetings",
        chave,
        Some(json!({
            "title": TITULO_SECRETO,
            "starts_at": (Utc::now() + Duration::hours(1)).to_rfc3339(),
            "duration_min": 30,
            "host_email": anfitriao,
            "invitees": [{"email": convidado}],
        })),
    )
    .await;
    assert_eq!(st, 200, "{m}");
    (
        m["id"].as_str().unwrap().to_string(),
        m["room_code"].as_str().unwrap().to_string(),
    )
}

async fn ring(app: &TestApp, chave: &str, id: &str) -> Value {
    let (st, r) = v1(
        app,
        reqwest::Method::POST,
        &format!("/meetings/{id}/ring"),
        chave,
        None,
    )
    .await;
    assert_eq!(st, 200, "{r}");
    r
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

async fn subscricoes(app: &TestApp, user_id: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM web_push_subscriptions WHERE user_id = $1::uuid")
        .bind(user_id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

/// Sem chave VAPID, o Web Push não existe — e o servidor DIZ-O, para o ecrã não
/// oferecer o que não se envia.
#[sqlx::test(migrations = "./migrations")]
async fn sem_configuracao_nao_ha_push_e_o_ecra_sabe(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("sem-push.test").await;
    let (st, s) = app.get("/api/users/me/push", Some(&a.token)).await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["enabled"], false, "{s}");
    assert!(s["public_key"].is_null(), "{s}");

    let (st, e) = app
        .post(
            "/api/users/me/push/subscriptions",
            Some(&a.token),
            Browser::novo(5).subscricao("https://fcm.googleapis.com/fcm/send/x"),
        )
        .await;
    assert_eq!(st, 409, "{e}");
    assert_eq!(e["code"], "push.not_configured", "{e}");
}

/// O critério do A-03: quem não tem socket e tem um browser subscrito passa de
/// `offline` a `ringing` — e o aviso que chega ao serviço de push vai cifrado,
/// assinado, urgente, com o link da sala e SEM o assunto da reunião.
///
/// Controlo negativo dentro do teste: o mesmo `ring`, ANTES de a pessoa
/// subscrever, devolve-a em `offline` e não manda nada ao serviço.
#[sqlx::test(migrations = "./migrations")]
async fn quem_nao_tem_socket_e_avisado_e_passa_a_ringing(db: sqlx::PgPool) {
    let app = com_push(db).await;
    let servico = ServicoFalso::arranca().await;
    let a = app.new_org("alfa.test").await;
    let carla = app.add_member(&a, "carla", "member").await;
    let (_, chave) = app.api_key(&a).await;
    let (id, code) = reuniao(&app, &chave, &a.email, &carla.email).await;

    // Antes de subscrever: offline, e o serviço não recebe nada.
    let r = ring(&app, &chave, &id).await;
    assert_eq!(ids(&r["offline"]), vec![carla.user_id.clone()], "{r}");
    assert!(ids(&r["ringing"]).is_empty(), "{r}");
    assert!(servico.recebidos().is_empty());

    let (st, estado) = app.get("/api/users/me/push", Some(&carla.token)).await;
    assert_eq!(st, 200);
    assert_eq!(estado["enabled"], true, "{estado}");
    let publica = estado["public_key"]
        .as_str()
        .expect("public_key")
        .to_string();

    let browser = Browser::novo(5);
    let (st, sub) = app
        .post(
            "/api/users/me/push/subscriptions",
            Some(&carla.token),
            browser.subscricao(&servico.endpoint("carla")),
        )
        .await;
    assert_eq!(st, 201, "{sub}");

    // Depois: ringing, e o serviço recebeu UM aviso.
    let r = ring(&app, &chave, &id).await;
    assert_eq!(ids(&r["ringing"]), vec![carla.user_id.clone()], "{r}");
    assert!(ids(&r["offline"]).is_empty(), "{r}");
    let recebidos = servico.recebidos();
    assert_eq!(recebidos.len(), 1);
    let aviso = &recebidos[0];
    assert_eq!(aviso.cabecalho("content-encoding"), Some("aes128gcm"));
    assert_eq!(aviso.cabecalho("urgency"), Some("high"));
    assert_eq!(aviso.cabecalho("ttl"), Some("120"));
    let vapid = aviso.cabecalho("authorization").unwrap_or_default();
    assert!(
        vapid.starts_with("vapid t=") && vapid.ends_with(&format!("k={publica}")),
        "assinatura VAPID com a chave que o GET anuncia: {vapid}"
    );

    // Decifrado com as chaves do browser: o link da sala, e NADA do assunto.
    let corpo = browser.decifra(aviso.corpo.clone());
    assert_eq!(corpo["link"], format!("/#/r/{code}"), "{corpo}");
    assert_eq!(corpo["kind"], "ring", "{corpo}");
    let texto = corpo.to_string();
    assert!(
        !texto.contains(TITULO_SECRETO),
        "o assunto vazou no aviso: {texto}"
    );
    assert!(
        !texto.contains("admin"),
        "o nome de quem chama vazou no aviso: {texto}"
    );
    // E o corpo em claro nunca viajou: o assunto não está nos bytes enviados.
    assert!(!String::from_utf8_lossy(&aviso.corpo).contains("/#/r/"));
}

/// Um browser que revogou a permissão: o serviço responde 410 e a subscrição
/// SAI da tabela, em vez de acumular falhas a cada toque. E a pessoa fica em
/// `offline`, porque ninguém a avisou.
#[sqlx::test(migrations = "./migrations")]
async fn um_endpoint_morto_sai_da_tabela(db: sqlx::PgPool) {
    let app = com_push(db).await;
    let servico = ServicoFalso::arranca().await;
    let a = app.new_org("alfa.test").await;
    let carla = app.add_member(&a, "carla", "member").await;
    let (_, chave) = app.api_key(&a).await;
    let (id, _) = reuniao(&app, &chave, &a.email, &carla.email).await;
    let (st, _) = app
        .post(
            "/api/users/me/push/subscriptions",
            Some(&carla.token),
            Browser::novo(6).subscricao(&servico.endpoint("morto")),
        )
        .await;
    assert_eq!(st, 201);
    assert_eq!(subscricoes(&app, &carla.user_id).await, 1);

    servico.estado.store(410, Ordering::SeqCst);
    let r = ring(&app, &chave, &id).await;
    assert_eq!(ids(&r["offline"]), vec![carla.user_id.clone()], "{r}");
    assert_eq!(servico.recebidos().len(), 1, "tentou uma vez");
    assert_eq!(
        subscricoes(&app, &carla.user_id).await,
        0,
        "o 410 tinha de retirar a subscrição"
    );

    // CONTROLO: uma falha que não é 404/410 conta-se e a subscrição FICA.
    let (st, _) = app
        .post(
            "/api/users/me/push/subscriptions",
            Some(&carla.token),
            Browser::novo(6).subscricao(&servico.endpoint("lento")),
        )
        .await;
    assert_eq!(st, 201);
    servico.estado.store(503, Ordering::SeqCst);
    let r = ring(&app, &chave, &id).await;
    assert_eq!(ids(&r["offline"]), vec![carla.user_id.clone()], "{r}");
    assert_eq!(
        subscricoes(&app, &carla.user_id).await,
        1,
        "um 503 não é uma subscrição morta"
    );
}

/// O endpoint vem do cliente: só serviços de push conhecidos (ou o que o
/// operador declarou), nada interno, e chaves válidas.
#[sqlx::test(migrations = "./migrations")]
async fn o_endpoint_tem_de_ser_de_um_servico_de_push(db: sqlx::PgPool) {
    let app = com_push(db).await;
    let a = app.new_org("alfa.test").await;
    let b = Browser::novo(7);
    for (endpoint, codigo) in [
        (
            "https://intranet.cliente.ao/push/x",
            "push.endpoint_not_allowed",
        ),
        (
            "https://fcm.googleapis.com.evil.example/x",
            "push.endpoint_not_allowed",
        ),
        (
            "http://fcm.googleapis.com/fcm/send/x",
            "push.endpoint_not_https",
        ),
    ] {
        let (st, e) = app
            .post(
                "/api/users/me/push/subscriptions",
                Some(&a.token),
                b.subscricao(endpoint),
            )
            .await;
        assert_eq!(st, 400, "{endpoint}: {e}");
        assert_eq!(e["code"], codigo, "{endpoint}: {e}");
    }
    // Um IP interno é recusado pela guarda de saída, antes da lista.
    let (st, e) = app
        .post(
            "/api/users/me/push/subscriptions",
            Some(&a.token),
            b.subscricao("https://169.254.169.254/push/x"),
        )
        .await;
    assert_eq!(st, 400, "{e}");

    let mut mas = b.subscricao("https://fcm.googleapis.com/fcm/send/x");
    mas["keys"]["auth"] = json!("curto");
    let (st, e) = app
        .post("/api/users/me/push/subscriptions", Some(&a.token), mas)
        .await;
    assert_eq!(st, 400, "{e}");
    assert_eq!(e["code"], "push.keys_invalid", "{e}");

    // CONTROLO POSITIVO: um endpoint real da Google passa.
    let (st, e) = app
        .post(
            "/api/users/me/push/subscriptions",
            Some(&a.token),
            b.subscricao("https://fcm.googleapis.com/fcm/send/abc123"),
        )
        .await;
    assert_eq!(st, 201, "{e}");
}

/// Um browser partilhado: quem subscreve por último passa a ser o dono — a
/// pessoa anterior deixa de receber os avisos de quem entrou depois dela. E
/// ninguém retira a subscrição de outra pessoa.
#[sqlx::test(migrations = "./migrations")]
async fn o_mesmo_browser_passa_para_quem_subscreveu_por_ultimo(db: sqlx::PgPool) {
    let app = com_push(db).await;
    let servico = ServicoFalso::arranca().await;
    let a = app.new_org("alfa.test").await;
    let carla = app.add_member(&a, "carla", "member").await;
    let endpoint = servico.endpoint("partilhado");
    let b = Browser::novo(8);

    let (st, _) = app
        .post(
            "/api/users/me/push/subscriptions",
            Some(&a.token),
            b.subscricao(&endpoint),
        )
        .await;
    assert_eq!(st, 201);
    let (st, _) = app
        .post(
            "/api/users/me/push/subscriptions",
            Some(&carla.token),
            b.subscricao(&endpoint),
        )
        .await;
    assert_eq!(st, 201);
    assert_eq!(subscricoes(&app, &a.user_id).await, 0);
    assert_eq!(subscricoes(&app, &carla.user_id).await, 1);

    // O admin tenta retirar a subscrição que agora é da Carla: nada muda, e a
    // resposta é a mesma de quem retira a sua.
    let (st, _) = app
        .post(
            "/api/users/me/push/subscriptions/unsubscribe",
            Some(&a.token),
            json!({"endpoint": endpoint}),
        )
        .await;
    assert_eq!(st, 204);
    assert_eq!(subscricoes(&app, &carla.user_id).await, 1);
    // A Carla retira a dela.
    let (st, _) = app
        .post(
            "/api/users/me/push/subscriptions/unsubscribe",
            Some(&carla.token),
            json!({"endpoint": endpoint}),
        )
        .await;
    assert_eq!(st, 204);
    assert_eq!(subscricoes(&app, &carla.user_id).await, 0);
}
