//! ADR-0023, S-01 — os aparelhos de um ramal móvel e o *wake* por push. Contra Postgres real.
//!
//! O que se mede é a REGRA do servidor: quem regista e revoga aparelhos, que o token nunca volta numa
//! resposta nem fica em claro na base, que o *wake* só acorda aparelhos DESSE ramal e DESSA organização,
//! que terminar a sessão desliga o aparelho, e que o FreeSWITCH só é mandado esperar (`awaiting`) quando
//! há de facto alguém a acordar. O fornecedor `lab` fala com um receptor de papel nesta máquina;
//! **nenhum push real (FCM, APNs) é enviado**, e nenhum FreeSWITCH corre nestes testes.
mod common;

use axum::{extract::State, routing::post, Json, Router};
use common::{jwt_claims, Account, TestApp};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const VOICE_SECRET: &str = "segredo-da-media-0123456789";
const TOKEN_DO_APARELHO: &str = "fcm:token-secreto-do-telemovel-123456";

/// O «fornecedor de laboratório»: guarda o que o servidor lhe entrega.
struct Receptor {
    url: String,
    pedidos: Arc<Mutex<Vec<Value>>>,
}

async fn receptor() -> Receptor {
    let pedidos: Arc<Mutex<Vec<Value>>> = Arc::default();
    async fn entrega(State(p): State<Arc<Mutex<Vec<Value>>>>, Json(v): Json<Value>) {
        p.lock().unwrap().push(v);
    }
    let app = Router::new()
        .route("/push", post(entrega))
        .with_state(pedidos.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Receptor {
        url: format!("http://127.0.0.1:{port}/push"),
        pedidos,
    }
}

async fn spawn(db: sqlx::PgPool, lab: Option<&Receptor>) -> TestApp {
    let mut extra = vec![
        ("VOICE_INTERNAL_SECRET", VOICE_SECRET),
        ("OUTBOUND_ALLOW_HOSTS", "127.0.0.1"),
    ];
    if let Some(r) = lab {
        extra.push(("PUSH_LAB_URL", r.url.as_str()));
    }
    TestApp::spawn_with(db, &extra).await
}

async fn novo_ramal(app: &TestApp, admin: &Account, member: &Account, number: &str) -> Value {
    let (st, resp) = app
        .post(
            &format!("/api/orgs/{}/extensions", admin.org()),
            Some(&admin.token),
            json!({"extension": number, "label": "Ramal", "member_id": member.user_id}),
        )
        .await;
    assert_eq!(st, 200, "criar ramal {number}: {resp}");
    resp
}

fn dominio(app: &TestApp, slug: &str) -> String {
    format!("{slug}.{}", app.state.config.voice_ramais_domain_suffix)
}

async fn slug_de(app: &TestApp, org: &str) -> String {
    sqlx::query_scalar("SELECT slug FROM organizations WHERE id = $1::uuid")
        .bind(org)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

fn caminho_aparelho(quem: &Account, id: &str) -> String {
    format!("/api/orgs/{}/my-extension/devices/{id}", quem.org())
}

fn corpo(provider: &str, platform: &str, token: &str) -> Value {
    json!({"platform": platform, "provider": provider, "push_token": token, "app_version": "0.1.0"})
}

async fn registar(app: &TestApp, quem: &Account, id: &str, body: Value) -> (u16, Value) {
    app.put(&caminho_aparelho(quem, id), Some(&quem.token), body)
        .await
}

fn novo_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

async fn acordar(app: &TestApp, dominio: &str, sip_user: &str, call: &str) -> (u16, Value) {
    acordar_de(app, dominio, sip_user, call, "").await
}

/// Como `acordar`, mas diz quem liga (o utilizador SIP do chamador, como o FreeSWITCH o autenticou).
async fn acordar_de(
    app: &TestApp,
    dominio: &str,
    sip_user: &str,
    call: &str,
    chamador_sip: &str,
) -> (u16, Value) {
    let res = app
        .http
        .post(app.url("/internal/v1/voice/push/wake"))
        .header("x-voice-secret", VOICE_SECRET)
        .json(&json!({
            "domain": dominio,
            "sip_username": sip_user,
            "call_uuid": call,
            "caller_sip_username": chamador_sip
        }))
        .send()
        .await
        .unwrap();
    let st = res.status().as_u16();
    (st, res.json().await.unwrap_or(Value::Null))
}

#[sqlx::test(migrations = "./migrations")]
async fn registar_um_aparelho_guarda_o_token_cifrado_e_nunca_o_devolve(db: sqlx::PgPool) {
    let app = spawn(db, None).await;
    let a = app.new_org("alfa-push.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    novo_ramal(&app, &a, &ana, "1004").await;
    let id = novo_id();

    let res = app
        .raw(
            reqwest::Method::PUT,
            &caminho_aparelho(&ana, &id),
            &[("authorization", &format!("Bearer {}", ana.token))],
            Some(corpo("fcm", "android", TOKEN_DO_APARELHO)),
        )
        .await;
    assert_eq!(res.status, 201, "{}", res.text);
    assert_eq!(
        res.header("location").as_deref(),
        Some(caminho_aparelho(&ana, &id).as_str())
    );
    assert!(
        !res.text.contains(TOKEN_DO_APARELHO),
        "o token voltou na resposta: {}",
        res.text
    );

    // Em repouso: cifrado, e o hash serve só a unicidade.
    let (guardado, hash): (String, String) =
        sqlx::query_as("SELECT push_token, push_token_hash FROM voice_devices WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_ne!(
        guardado, TOKEN_DO_APARELHO,
        "o token ficou em claro na base"
    );
    assert!(!guardado.contains(TOKEN_DO_APARELHO));
    assert_eq!(hash.len(), 64);

    // Repetir com o mesmo id renova (200), não duplica.
    let (st, _) = registar(
        &app,
        &ana,
        &id,
        corpo("fcm", "android", "fcm:token-novo-654321"),
    )
    .await;
    assert_eq!(st, 200);
    let (st, lista) = app
        .get(
            &format!("/api/orgs/{}/my-extension/devices", ana.org()),
            Some(&ana.token),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(lista.as_array().unwrap().len(), 1, "{lista}");
    assert!(!lista.to_string().contains("token-novo"), "{lista}");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_forma_do_pedido_e_validada(db: sqlx::PgPool) {
    let app = spawn(db, None).await;
    let a = app.new_org("alfa-forma.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    novo_ramal(&app, &a, &ana, "1004").await;
    for (nome, body, codigo) in [
        (
            "plataforma",
            corpo("fcm", "windows", "abc"),
            "devices.platform_invalid",
        ),
        (
            "fornecedor",
            corpo("onesignal", "android", "abc"),
            "devices.provider_invalid",
        ),
        (
            "fornecedor e plataforma",
            corpo("apns_voip", "android", "abc"),
            "devices.provider_platform_mismatch",
        ),
        (
            "token vazio",
            corpo("fcm", "android", ""),
            "devices.token_invalid",
        ),
        (
            "token com espaço",
            corpo("fcm", "android", "com espaço"),
            "devices.token_invalid",
        ),
    ] {
        let (st, resp) = registar(&app, &ana, &novo_id(), body).await;
        assert_eq!(st, 400, "{nome}: {resp}");
        assert_eq!(resp["code"], codigo, "{nome}: {resp}");
    }
    // Quem não tem ramal não regista aparelhos.
    let bruno = app.add_member(&a, "bruno", "member").await;
    let (st, resp) = registar(&app, &bruno, &novo_id(), corpo("fcm", "android", "abc")).await;
    assert_eq!(st, 404, "{resp}");
    assert_eq!(resp["code"], "ramais.no_extension");
}

#[sqlx::test(migrations = "./migrations")]
async fn um_aparelho_revogado_nao_ressuscita_e_ha_um_maximo(db: sqlx::PgPool) {
    let app = spawn(db, None).await;
    let a = app.new_org("alfa-max.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    novo_ramal(&app, &a, &ana, "1004").await;
    let id = novo_id();
    assert_eq!(
        registar(&app, &ana, &id, corpo("fcm", "android", "tok-1"))
            .await
            .0,
        201
    );
    let (st, _) = app
        .delete(&caminho_aparelho(&ana, &id), Some(&ana.token))
        .await;
    assert_eq!(st, 204);
    // Apagar outra vez: já não há aparelho activo.
    assert_eq!(
        app.delete(&caminho_aparelho(&ana, &id), Some(&ana.token))
            .await
            .0,
        404
    );
    // Voltar a pô-lo com o mesmo id: recusado (o administrador pode tê-lo revogado de propósito).
    let (st, resp) = registar(&app, &ana, &id, corpo("fcm", "android", "tok-1")).await;
    assert_eq!(st, 409, "{resp}");
    assert_eq!(resp["code"], "devices.revoked");

    // O máximo de aparelhos activos por ramal.
    for n in 0..8 {
        let (st, resp) = registar(
            &app,
            &ana,
            &novo_id(),
            corpo("fcm", "android", &format!("tok-m{n}")),
        )
        .await;
        assert_eq!(st, 201, "aparelho {n}: {resp}");
    }
    let (st, resp) = registar(
        &app,
        &ana,
        &novo_id(),
        corpo("fcm", "android", "tok-excesso"),
    )
    .await;
    assert_eq!(st, 409, "{resp}");
    assert_eq!(resp["code"], "devices.too_many");
}

#[sqlx::test(migrations = "./migrations")]
async fn o_wake_acorda_o_aparelho_certo_uma_so_vez_e_sem_o_token(db: sqlx::PgPool) {
    let lab = receptor().await;
    let app = spawn(db, Some(&lab)).await;
    let a = app.new_org("alfa-wake.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let ramal = novo_ramal(&app, &a, &ana, "1004").await;
    let bruno = app.add_member(&a, "bruno", "member").await;
    let ramal_bruno = novo_ramal(&app, &a, &bruno, "1005").await;
    let sip_bruno = ramal_bruno["sip_username"].as_str().unwrap().to_string();
    let sip = ramal["sip_username"].as_str().unwrap().to_string();
    let dom = dominio(&app, &slug_de(&app, a.org()).await);
    let id = novo_id();
    assert_eq!(
        registar(&app, &ana, &id, corpo("lab", "android", TOKEN_DO_APARELHO))
            .await
            .0,
        201
    );

    // Quem liga é o ramal 1005 (o Lua manda o utilizador SIP dele): o telemóvel recebe o NÚMERO, nunca
    // o utilizador SIP.
    let (st, r) = acordar_de(
        &app,
        &dom,
        &sip,
        "11111111-aaaa-bbbb-cccc-000000000001",
        &sip_bruno,
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r, json!({"awaiting": true, "devices": 1}));
    {
        let recebidos = lab.pedidos.lock().unwrap();
        assert_eq!(recebidos.len(), 1, "{recebidos:?}");
        assert_eq!(recebidos[0]["device_id"], id);
        assert_eq!(
            recebidos[0]["call_uuid"],
            "11111111-aaaa-bbbb-cccc-000000000001"
        );
        assert_eq!(recebidos[0]["caller"], "1005");
        assert!(
            !recebidos[0].to_string().contains(&sip_bruno),
            "o utilizador SIP do chamador foi para o telemóvel de outra pessoa: {}",
            recebidos[0]
        );
        assert!(
            !recebidos[0].to_string().contains(TOKEN_DO_APARELHO),
            "o token foi no pedido ao fornecedor: {}",
            recebidos[0]
        );
    }
    // O mesmo call_uuid: continua à espera, mas o telemóvel não é acordado outra vez.
    let (_, r) = acordar(&app, &dom, &sip, "11111111-aaaa-bbbb-cccc-000000000001").await;
    assert_eq!(r, json!({"awaiting": true, "devices": 1}));
    assert_eq!(
        lab.pedidos.lock().unwrap().len(),
        1,
        "acordou duas vezes pela mesma chamada"
    );
    // Auditoria: o número do ramal, nunca o token.
    let (alvo,): (String,) = sqlx::query_as(
        "SELECT target FROM audit_logs WHERE org_id = $1::uuid AND action = 'ramal.acordado_por_push'",
    )
    .bind(a.org())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(alvo, "1004");
}

#[sqlx::test(migrations = "./migrations")]
async fn o_wake_diz_que_nao_ha_ninguem_sem_explicar_porque(db: sqlx::PgPool) {
    let lab = receptor().await;
    let app = spawn(db, Some(&lab)).await;
    let a = app.new_org("alfa-ninguem.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let bruno = app.add_member(&a, "bruno", "member").await;
    let ramal_ana = novo_ramal(&app, &a, &ana, "1004").await;
    let ramal_bruno = novo_ramal(&app, &a, &bruno, "1005").await;
    let dom = dominio(&app, &slug_de(&app, a.org()).await);
    let sip_ana = ramal_ana["sip_username"].as_str().unwrap();
    let sip_bruno = ramal_bruno["sip_username"].as_str().unwrap();
    let nobody = json!({"awaiting": false, "devices": 0});

    // Sem aparelhos.
    assert_eq!(
        acordar(&app, &dom, sip_ana, "c-sem-aparelhos").await.1,
        nobody
    );
    // Um aparelho FCM: aceita-se o registo, mas o fornecedor ainda não envia nada — não finge.
    assert_eq!(
        registar(&app, &ana, &novo_id(), corpo("fcm", "android", "tok-fcm"))
            .await
            .0,
        201
    );
    assert_eq!(acordar(&app, &dom, sip_ana, "c-fcm").await.1, nobody);
    assert!(lab.pedidos.lock().unwrap().is_empty());
    // Domínio e ramal desconhecidos: a mesma resposta.
    assert_eq!(
        acordar(&app, "nao-existe.ramais.delonix.meet", sip_ana, "c-dom")
            .await
            .1,
        nobody
    );
    assert_eq!(
        acordar(&app, &dom, "ramal_inexistente", "c-user").await.1,
        nobody
    );
    // Ramal inactivo.
    let id = novo_id();
    assert_eq!(
        registar(&app, &bruno, &id, corpo("lab", "android", "tok-lab-b"))
            .await
            .0,
        201
    );
    let (st, _) = app
        .patch(
            &format!(
                "/api/orgs/{}/extensions/{}",
                a.org(),
                ramal_bruno["id"].as_str().unwrap()
            ),
            Some(&a.token),
            json!({"active": false}),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(acordar(&app, &dom, sip_bruno, "c-inactivo").await.1, nobody);
    assert!(
        lab.pedidos.lock().unwrap().is_empty(),
        "acordou um ramal que não devia"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn o_wake_exige_o_segredo_de_voz(db: sqlx::PgPool) {
    let app = spawn(db, None).await;
    for cabecalho in [None, Some("errado-errado-errado-errado")] {
        let mut req = app.http.post(app.url("/internal/v1/voice/push/wake")).json(&json!({
            "domain": "x.ramais.delonix.meet", "sip_username": "u", "call_uuid": "abc", "caller_extension": ""
        }));
        if let Some(h) = cabecalho {
            req = req.header("x-voice-secret", h);
        }
        let st = req.send().await.unwrap().status().as_u16();
        assert!(st == 401 || st == 403, "sem o segredo certo deu {st}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn duas_organizacoes_nunca_se_alcancam(db: sqlx::PgPool) {
    let lab = receptor().await;
    let app = spawn(db, Some(&lab)).await;
    let a = app.new_org("alfa-iso.ao").await;
    let b = app.new_org("beta-iso.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let bia = app.add_member(&b, "bia", "member").await;
    let ramal_a = novo_ramal(&app, &a, &ana, "1004").await;
    novo_ramal(&app, &b, &bia, "1004").await; // o MESMO número noutra organização
    let sip_a = ramal_a["sip_username"].as_str().unwrap();
    let dom_a = dominio(&app, &slug_de(&app, a.org()).await);
    let dom_b = dominio(&app, &slug_de(&app, b.org()).await);
    let id_a = novo_id();
    assert_eq!(
        registar(&app, &ana, &id_a, corpo("lab", "android", "tok-a"))
            .await
            .0,
        201
    );

    // A bia não toma o id do aparelho da ana, nem o vê, nem o apaga.
    let (st, _) = registar(&app, &bia, &id_a, corpo("lab", "android", "tok-b")).await;
    assert_eq!(st, 404, "a org B alcançou um aparelho da org A");
    let (st, lista_b) = app
        .get(
            &format!("/api/orgs/{}/my-extension/devices", bia.org()),
            Some(&bia.token),
        )
        .await;
    assert_eq!(st, 200);
    assert!(lista_b.as_array().unwrap().is_empty(), "{lista_b}");
    assert_eq!(
        app.delete(&caminho_aparelho(&bia, &id_a), Some(&bia.token))
            .await
            .0,
        404
    );
    // E o administrador da org B não lista nem apaga aparelhos de um ramal da org A.
    let (st, _) = app
        .get(
            &format!(
                "/api/orgs/{}/extensions/{}/devices",
                b.org(),
                ramal_a["id"].as_str().unwrap()
            ),
            Some(&b.token),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .delete(
            &format!(
                "/api/orgs/{}/extensions/{}/devices/{id_a}",
                b.org(),
                ramal_a["id"].as_str().unwrap()
            ),
            Some(&b.token),
        )
        .await;
    assert_eq!(st, 404);
    // Nem alcança a org A pelo URL da org A (não é membro).
    let (st, _) = app
        .get(
            &format!(
                "/api/orgs/{}/extensions/{}/devices",
                a.org(),
                ramal_a["id"].as_str().unwrap()
            ),
            Some(&b.token),
        )
        .await;
    assert!(st == 403 || st == 404, "{st}");

    // O wake pelo domínio da org B com o utilizador SIP do ramal da org A: ninguém.
    let (_, r) = acordar(&app, &dom_b, sip_a, "c-cruzado").await;
    assert_eq!(r, json!({"awaiting": false, "devices": 0}));
    assert!(lab.pedidos.lock().unwrap().is_empty());
    // Pelo domínio certo, acorda.
    let (_, r) = acordar(&app, &dom_a, sip_a, "c-certo").await;
    assert_eq!(r, json!({"awaiting": true, "devices": 1}));
    assert_eq!(lab.pedidos.lock().unwrap().len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn terminar_a_sessao_desliga_o_aparelho(db: sqlx::PgPool) {
    let lab = receptor().await;
    let app = spawn(db, Some(&lab)).await;
    let a = app.new_org("alfa-sessao.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let ramal = novo_ramal(&app, &a, &ana, "1004").await;
    let sip = ramal["sip_username"].as_str().unwrap();
    let dom = dominio(&app, &slug_de(&app, a.org()).await);

    // O telemóvel é a SESSÃO `telemovel`; o portátil é outra sessão da mesma pessoa.
    let telemovel = app.login(&ana.email).await;
    let portatil = app.login(&ana.email).await;
    let sid = jwt_claims(&telemovel.token)["sid"]
        .as_str()
        .unwrap()
        .to_string();
    let id = novo_id();
    let (st, resp) = app
        .put(
            &format!("/api/orgs/{}/my-extension/devices/{id}", a.org()),
            Some(&telemovel.token),
            corpo("lab", "android", "tok-tel"),
        )
        .await;
    assert_eq!(st, 201, "{resp}");
    assert_eq!(
        acordar(&app, &dom, sip, "c-antes").await.1,
        json!({"awaiting": true, "devices": 1})
    );

    // Terminar a sessão do telemóvel pelo portátil: o aparelho deixa de ser acordado.
    let (st, _) = app
        .delete(
            &format!("/api/users/me/sessions/{sid}"),
            Some(&portatil.token),
        )
        .await;
    assert!(st == 200 || st == 204, "terminar a sessão: {st}");
    assert_eq!(
        acordar(&app, &dom, sip, "c-depois").await.1,
        json!({"awaiting": false, "devices": 0}),
        "o aparelho continua a ser acordado depois de a sessão terminar"
    );
    assert_eq!(lab.pedidos.lock().unwrap().len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn o_administrador_lista_e_desliga_os_aparelhos_de_um_ramal(db: sqlx::PgPool) {
    let app = spawn(db, None).await;
    let a = app.new_org("alfa-admin.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let bruno = app.add_member(&a, "bruno", "member").await;
    let ramal = novo_ramal(&app, &a, &ana, "1004").await;
    let ext = ramal["id"].as_str().unwrap();
    let id = novo_id();
    assert_eq!(
        registar(&app, &ana, &id, corpo("fcm", "android", "tok-x"))
            .await
            .0,
        201
    );

    let (st, lista) = app
        .get(
            &format!("/api/orgs/{}/extensions/{ext}/devices", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(lista.as_array().unwrap().len(), 1);
    assert!(!lista.to_string().contains("tok-x"));
    // Um membro que não é administrador não gere aparelhos alheios.
    let (st, _) = app
        .get(
            &format!("/api/orgs/{}/extensions/{ext}/devices", a.org()),
            Some(&bruno.token),
        )
        .await;
    assert_eq!(st, 403);
    let (st, _) = app
        .delete(
            &format!("/api/orgs/{}/extensions/{ext}/devices/{id}", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 204);
    let (st, lista) = app
        .get(
            &format!("/api/orgs/{}/my-extension/devices", a.org()),
            Some(&ana.token),
        )
        .await;
    assert_eq!(st, 200);
    assert!(lista.as_array().unwrap().is_empty(), "{lista}");
}

#[sqlx::test(migrations = "./migrations")]
async fn ha_um_limite_de_wakes_por_ramal_e_por_minuto(db: sqlx::PgPool) {
    let lab = receptor().await;
    let app = spawn(db, Some(&lab)).await;
    let a = app.new_org("alfa-limite.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let ramal = novo_ramal(&app, &a, &ana, "1004").await;
    let sip = ramal["sip_username"].as_str().unwrap();
    let dom = dominio(&app, &slug_de(&app, a.org()).await);
    assert_eq!(
        registar(&app, &ana, &novo_id(), corpo("lab", "android", "tok-l"))
            .await
            .0,
        201
    );
    for n in 0..12 {
        let (_, r) = acordar(&app, &dom, sip, &format!("chamada-{n}")).await;
        assert_eq!(r["awaiting"], true, "chamada {n}: {r}");
    }
    // A 13.ª em menos de um minuto: o servidor deixa de acordar (e o FreeSWITCH deixa de esperar).
    let (_, r) = acordar(&app, &dom, sip, "chamada-13").await;
    assert_eq!(r, json!({"awaiting": false, "devices": 0}));
    assert_eq!(lab.pedidos.lock().unwrap().len(), 12);
}

#[sqlx::test(migrations = "./migrations")]
async fn um_chamador_que_nao_e_ramal_desta_organizacao_vai_sem_nome(db: sqlx::PgPool) {
    let lab = receptor().await;
    let app = spawn(db, Some(&lab)).await;
    let a = app.new_org("alfa-chamador.ao").await;
    let b = app.new_org("beta-chamador.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let bia = app.add_member(&b, "bia", "member").await;
    let ramal_ana = novo_ramal(&app, &a, &ana, "1004").await;
    let ramal_bia = novo_ramal(&app, &b, &bia, "1004").await;
    let sip_ana = ramal_ana["sip_username"].as_str().unwrap();
    let sip_bia = ramal_bia["sip_username"].as_str().unwrap();
    let dom = dominio(&app, &slug_de(&app, a.org()).await);
    assert_eq!(
        registar(&app, &ana, &novo_id(), corpo("lab", "android", "tok-c"))
            .await
            .0,
        201
    );

    // Um chamador de OUTRA organização (o utilizador SIP existe, mas não aqui) e um inventado: sem nome.
    acordar_de(&app, &dom, sip_ana, "chamada-outra-org", sip_bia).await;
    acordar_de(
        &app,
        &dom,
        sip_ana,
        "chamada-inventada",
        "ramal_que_nao_existe",
    )
    .await;
    let recebidos = lab.pedidos.lock().unwrap();
    assert_eq!(recebidos.len(), 2, "{recebidos:?}");
    for r in recebidos.iter() {
        assert_eq!(r["caller"], "", "{r}");
        assert!(!r.to_string().contains(sip_bia));
        assert!(!r.to_string().contains("ramal_que_nao_existe"));
    }
}

/// O serviço `delonix-push` de papel: guarda a chave com que o chamaram e o corpo.
async fn delonix_push_de_papel(status: u16) -> (String, Arc<Mutex<Vec<(String, Value)>>>) {
    type Vistos = Arc<Mutex<Vec<(String, Value)>>>;
    let vistos: Vistos = Arc::default();
    let app =
        Router::new()
            .route(
                "/v1/messages",
                post(
                    move |State(v): State<Vistos>,
                          h: axum::http::HeaderMap,
                          Json(b): Json<Value>| async move {
                        let auth = h
                            .get("authorization")
                            .and_then(|x| x.to_str().ok())
                            .unwrap_or("")
                            .to_string();
                        v.lock().unwrap().push((auth, b));
                        axum::http::StatusCode::from_u16(status).unwrap()
                    },
                ),
            )
            .with_state(vistos.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (format!("http://127.0.0.1:{port}"), vistos)
}

#[sqlx::test(migrations = "./migrations")]
async fn o_fornecedor_delonix_acorda_pelo_delonix_push_sem_o_utilizador_sip(db: sqlx::PgPool) {
    let (url, vistos) = delonix_push_de_papel(202).await;
    let app = TestApp::spawn_with(
        db,
        &[
            ("VOICE_INTERNAL_SECRET", VOICE_SECRET),
            ("OUTBOUND_ALLOW_HOSTS", "127.0.0.1"),
            ("PUSH_DELONIX_URL", url.as_str()),
            ("PUSH_DELONIX_KEY", "dpk_chave-do-projecto"),
        ],
    )
    .await;
    let a = app.new_org("delonix-push.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let ramal = novo_ramal(&app, &a, &ana, "1010").await;
    let bruno = app.add_member(&a, "bruno", "member").await;
    let ramal_bruno = novo_ramal(&app, &a, &bruno, "1011").await;
    let sip = ramal["sip_username"].as_str().unwrap().to_string();
    let sip_bruno = ramal_bruno["sip_username"].as_str().unwrap().to_string();
    let dom = dominio(&app, &slug_de(&app, a.org()).await);
    let id_push = novo_id(); // o device_id que o delonix-push devolveu
    assert_eq!(
        registar(&app, &ana, &novo_id(), corpo("delonix", "ios", &id_push))
            .await
            .0,
        201,
        "o fornecedor delonix serve iPhone e Android"
    );

    let (_, r) = acordar_de(
        &app,
        &dom,
        &sip,
        "22222222-aaaa-bbbb-cccc-000000000001",
        &sip_bruno,
    )
    .await;
    assert_eq!(r, json!({"awaiting": true, "devices": 1}));
    let v = vistos.lock().unwrap();
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].0, "Bearer dpk_chave-do-projecto");
    assert_eq!(v[0].1["device_id"], id_push);
    assert_eq!(v[0].1["priority"], "high");
    assert_eq!(v[0].1["payload"]["caller"], "1011");
    assert_eq!(v[0].1["payload"]["kind"], "incoming_call");
    assert_eq!(
        v[0].1["idempotency_key"],
        "wake:22222222-aaaa-bbbb-cccc-000000000001"
    );
    assert!(
        !v[0].1.to_string().contains(&sip_bruno),
        "o utilizador SIP do chamador foi para o delonix-push: {}",
        v[0].1
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn o_fornecedor_delonix_sem_configuracao_nao_acorda(db: sqlx::PgPool) {
    // Sem URL/chave: aceita-se o registo, ninguém é acordado e o FreeSWITCH não fica à espera.
    let app = spawn(db.clone(), None).await;
    let a = app.new_org("delonix-push-b.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let ramal = novo_ramal(&app, &a, &ana, "1020").await;
    let sip = ramal["sip_username"].as_str().unwrap().to_string();
    let dom = dominio(&app, &slug_de(&app, a.org()).await);
    registar(
        &app,
        &ana,
        &novo_id(),
        corpo("delonix", "android", &novo_id()),
    )
    .await;
    let (_, r) = acordar(&app, &dom, &sip, "33333333-aaaa-bbbb-cccc-000000000001").await;
    assert_eq!(r["awaiting"], false, "{r}");
}
