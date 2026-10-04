//! R278 — o Linphone configurado por QR de uso único. Contra Postgres real.
//!
//! O que se mede aqui é a REGRA do servidor: quem emite o bilhete, que o
//! resgate dá uma password que o directório do FreeSWITCH aceita, e que o
//! bilhete só serve uma vez. Nenhum Linphone nem FreeSWITCH corre nestes
//! testes: o que faz de directório é a própria rota `ivr/directory`.
mod common;

use common::{Account, TestApp};
use md5::{Digest, Md5};
use serde_json::{json, Value};

const VOICE_SECRET: &str = "segredo-da-media-0123456789";
const ORIGIN: &str = "https://meet.exemplo.ao";

async fn spawn(db: sqlx::PgPool) -> TestApp {
    TestApp::spawn_with(
        db,
        &[
            ("VOICE_INTERNAL_SECRET", VOICE_SECRET),
            ("VOICE_RAMAIS_PUBLIC_HOST", "sip.exemplo.ao"),
            ("CORS_ORIGINS", ORIGIN),
        ],
    )
    .await
}

async fn novo_ramal(
    app: &TestApp,
    admin: &Account,
    member: Option<&Account>,
    number: &str,
) -> Value {
    let mut body = json!({"extension": number, "label": "Recepção"});
    if let Some(m) = member {
        body["member_id"] = json!(m.user_id);
    }
    let (st, resp) = app
        .post(
            &format!("/api/orgs/{}/extensions", admin.org()),
            Some(&admin.token),
            body,
        )
        .await;
    assert_eq!(st, 200, "criar ramal {number}: {resp}");
    resp
}

async fn emitir_meu(app: &TestApp, quem: &Account) -> (u16, Value) {
    app.post(
        &format!("/api/orgs/{}/my-extension/provisioning-ticket", quem.org()),
        Some(&quem.token),
        json!({}),
    )
    .await
}

async fn emitir_admin(app: &TestApp, admin: &Account, org: &str, id: &str) -> (u16, Value) {
    app.post(
        &format!("/api/orgs/{org}/extensions/{id}/provisioning-ticket"),
        Some(&admin.token),
        json!({}),
    )
    .await
}

/// O caminho do URL emitido (o teste fala com o servidor local, não com a
/// origem pública que vai no QR).
fn caminho(ticket: &Value) -> String {
    let url = ticket["provisioning_url"].as_str().unwrap();
    assert!(
        url.starts_with(&format!("{ORIGIN}/api/public/extension-provisioning/")),
        "{url}"
    );
    url.strip_prefix(ORIGIN).unwrap().to_string()
}

async fn resgatar(app: &TestApp, path: &str) -> common::RawResponse {
    app.raw(reqwest::Method::GET, path, &[], None).await
}

fn entrada(xml: &str, section: &str, name: &str) -> String {
    let sec = xml
        .split(&format!("<section name=\"{section}\">"))
        .nth(1)
        .unwrap_or_else(|| panic!("sem secção {section}: {xml}"));
    let sec = sec.split("</section>").next().unwrap();
    let start = format!("<entry name=\"{name}\" overwrite=\"true\">");
    sec.split(&start)
        .nth(1)
        .unwrap_or_else(|| panic!("sem {section}.{name}: {xml}"))
        .split("</entry>")
        .next()
        .unwrap()
        .to_string()
}

/// O que o FreeSWITCH recebe do directório quando o aparelho regista.
async fn a1_do_directorio(app: &TestApp, user: &str, domain: &str) -> String {
    let res = app
        .http
        .post(app.url("/api/voice/ivr/directory"))
        .header("x-voice-secret", VOICE_SECRET)
        .form(&[("user", user), ("domain", domain)])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let xml = res.text().await.unwrap();
    xml.split("name=\"a1-hash\" value=\"")
        .nth(1)
        .unwrap_or_else(|| panic!("directório sem a1-hash: {xml}"))
        .split('"')
        .next()
        .unwrap()
        .to_string()
}

async fn auditoria(app: &TestApp, org: &str, action: &str) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT actor_id::text, target FROM audit_logs WHERE org_id = $1::uuid AND action = $2 ORDER BY seq",
    )
    .bind(org)
    .bind(action)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn o_dono_emite_e_o_resgate_da_uma_password_que_regista(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-qr.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let ramal = novo_ramal(&app, &a, Some(&ana), "1004").await;
    let antiga = ramal["sip_password"].as_str().unwrap().to_string();

    let (st, ticket) = emitir_meu(&app, &ana).await;
    assert_eq!(st, 200, "{ticket}");
    assert_eq!(ticket["extension"], "1004");
    let path = caminho(&ticket);
    let token = path.rsplit('/').next().unwrap().to_string();
    assert_eq!(token.len(), 64);

    // Na base, só o hash do token.
    let guardado: String =
        sqlx::query_scalar("SELECT token_hash FROM voice_extension_provisioning_tickets")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_ne!(guardado, token);

    let r = resgatar(&app, &path).await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert!(r
        .header("content-type")
        .unwrap()
        .starts_with("application/xml"));
    assert_eq!(r.header("cache-control").as_deref(), Some("no-store"));
    let xml = r.text;
    let user = entrada(&xml, "auth_info_0", "username");
    let pass = entrada(&xml, "auth_info_0", "passwd");
    let domain = entrada(&xml, "auth_info_0", "domain");
    assert_eq!(user, ramal["sip_username"].as_str().unwrap());
    assert_eq!(domain, ramal["sip_domain"].as_str().unwrap());
    assert_ne!(pass, antiga, "a password tem de ser NOVA");
    assert_eq!(
        entrada(&xml, "proxy_0", "reg_proxy"),
        "&lt;sip:sip.exemplo.ao:5070;transport=udp&gt;"
    );
    assert_eq!(entrada(&xml, "sip", "media_encryption"), "srtp");
    assert!(entrada(&xml, "proxy_0", "reg_identity").contains("ana"));

    // A password do XML é a que o directório do FreeSWITCH aceita.
    let want = hex::encode(Md5::digest(format!("{user}:{domain}:{pass}")));
    assert_eq!(a1_do_directorio(&app, &user, &domain).await, want);
    let argon: String =
        sqlx::query_scalar("SELECT sip_password_hash FROM voice_extensions WHERE id = $1::uuid")
            .bind(ramal["id"].as_str().unwrap())
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(delonix_meet_core::crypto::verify_password(&pass, &argon));
    assert!(!delonix_meet_core::crypto::verify_password(&antiga, &argon));

    // Segundo resgate: a mesma recusa que um token que nunca existiu.
    let de_novo = resgatar(&app, &path).await;
    assert_eq!(de_novo.status, 404);
    assert_eq!(de_novo.json()["code"], "ramais.provisioning_invalid");
    let inventado = resgatar(
        &app,
        &format!("/api/public/extension-provisioning/{}", "0".repeat(64)),
    )
    .await;
    assert_eq!(inventado.status, 404);
    assert_eq!(inventado.json()["code"], de_novo.json()["code"]);
    let mal_formado = resgatar(&app, "/api/public/extension-provisioning/abc").await;
    assert_eq!(mal_formado.status, 404);
    assert_eq!(mal_formado.json()["code"], "ramais.provisioning_invalid");

    // Auditoria: a emissão em nome de quem a pediu, o resgate com o actor de
    // sistema, o ramal no alvo — e o token em lado nenhum.
    let emitidos = auditoria(&app, a.org(), "ramal.provisionamento_emitido").await;
    assert_eq!(emitidos, vec![(ana.user_id.clone(), "1004".to_string())]);
    let resgatados = auditoria(&app, a.org(), "ramal.provisionado").await;
    assert_eq!(
        resgatados,
        vec![(uuid::Uuid::nil().to_string(), "1004".to_string())]
    );
    let com_token: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_logs WHERE target LIKE '%' || $1 || '%'")
            .bind(&token)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(com_token, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn o_admin_emite_para_qualquer_ramal_da_sua_org_e_so_da_sua(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-qr-adm.ao").await;
    let b = app.new_org("beta-qr-adm.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let rui = app.add_member(&a, "rui", "member").await;
    let de_ana = novo_ramal(&app, &a, Some(&ana), "1004").await;
    let recepcao = novo_ramal(&app, &a, None, "1000").await;
    let de_b = novo_ramal(&app, &b, None, "1000").await;
    let id_ana = de_ana["id"].as_str().unwrap();

    // O admin emite para um ramal de pessoa e para um da empresa.
    let (st, t1) = emitir_admin(&app, &a, a.org(), id_ana).await;
    assert_eq!(st, 200, "{t1}");
    let (st, t2) = emitir_admin(&app, &a, a.org(), recepcao["id"].as_str().unwrap()).await;
    assert_eq!(st, 200, "{t2}");
    assert_eq!(resgatar(&app, &caminho(&t2)).await.status, 200);

    // Emitir outro apaga o anterior: o primeiro QR já não serve.
    let (_, t3) = emitir_admin(&app, &a, a.org(), id_ana).await;
    assert_eq!(resgatar(&app, &caminho(&t1)).await.status, 404);
    assert_eq!(resgatar(&app, &caminho(&t3)).await.status, 200);

    // Admin da org A não alcança o ramal da org B, nem pelo caminho da B nem
    // pelo da A com o id da B.
    let (st, body) = emitir_admin(&app, &a, b.org(), de_b["id"].as_str().unwrap()).await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = emitir_admin(&app, &a, a.org(), de_b["id"].as_str().unwrap()).await;
    assert_eq!(st, 404, "{body}");

    // Um membro não emite pela rota do admin, nem para o ramal de outro.
    let (st, body) = emitir_admin(&app, &rui, a.org(), id_ana).await;
    assert_eq!(st, 403, "{body}");
    // E sem ramal próprio, «o meu ramal» não tem nada para emitir.
    let (st, body) = emitir_meu(&app, &rui).await;
    assert_eq!(st, 404, "{body}");
    assert_eq!(body["code"], "ramais.no_extension");
    // Nem «o meu ramal» numa organização a que não pertence.
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/my-extension/provisioning-ticket", b.org()),
            Some(&ana.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404, "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn expirado_ou_ramal_inactivo_da_404(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-qr-exp.ao").await;
    let ramal = novo_ramal(&app, &a, None, "1000").await;
    let id = ramal["id"].as_str().unwrap();

    let (_, t) = emitir_admin(&app, &a, a.org(), id).await;
    sqlx::query(
        "UPDATE voice_extension_provisioning_tickets SET expires_at = now() - interval '1 second'",
    )
    .execute(&app.db)
    .await
    .unwrap();
    let r = resgatar(&app, &caminho(&t)).await;
    assert_eq!(r.status, 404);
    assert_eq!(r.json()["code"], "ramais.provisioning_invalid");

    // Emitido com o ramal activo, resgatado depois de o desactivarem.
    let (_, t) = emitir_admin(&app, &a, a.org(), id).await;
    let (st, _) = app
        .patch(
            &format!("/api/orgs/{}/extensions/{id}", a.org()),
            Some(&a.token),
            json!({"active": false}),
        )
        .await;
    assert_eq!(st, 200);
    let r = resgatar(&app, &caminho(&t)).await;
    assert_eq!(r.status, 404);
    assert_eq!(r.json()["code"], "ramais.provisioning_invalid");
    // Inactivo, nem se emite.
    let (st, body) = emitir_admin(&app, &a, a.org(), id).await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "ramais.extension_inactive");
}

#[sqlx::test(migrations = "./migrations")]
async fn dois_resgates_em_paralelo_so_um_ganha(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-qr-par.ao").await;
    let ramal = novo_ramal(&app, &a, None, "1000").await;
    let (_, t) = emitir_admin(&app, &a, a.org(), ramal["id"].as_str().unwrap()).await;
    let path = caminho(&t);
    let (r1, r2) = tokio::join!(resgatar(&app, &path), resgatar(&app, &path));
    let mut st = [r1.status, r2.status];
    st.sort();
    assert_eq!(st, [200, 404], "{} / {}", r1.text, r2.text);

    // A password que ficou na base é a do vencedor.
    let vencedor = if r1.status == 200 { r1.text } else { r2.text };
    let user = entrada(&vencedor, "auth_info_0", "username");
    let domain = entrada(&vencedor, "auth_info_0", "domain");
    let pass = entrada(&vencedor, "auth_info_0", "passwd");
    assert_eq!(
        a1_do_directorio(&app, &user, &domain).await,
        hex::encode(Md5::digest(format!("{user}:{domain}:{pass}")))
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn sem_servidor_sip_ou_origem_publica_nao_se_emite(db: sqlx::PgPool) {
    // Sem VOICE_RAMAIS_PUBLIC_HOST.
    let app = TestApp::spawn_with(db.clone(), &[("CORS_ORIGINS", ORIGIN)]).await;
    let a = app.new_org("alfa-qr-sem.ao").await;
    let ramal = novo_ramal(&app, &a, Some(&a), "1000").await;
    let (st, body) = emitir_meu(&app, &a).await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "ramais.sip_server_missing");
    drop(app);

    // Com o servidor SIP mas a origem pública interna: também não.
    let app = TestApp::spawn_with(
        db,
        &[
            ("VOICE_RAMAIS_PUBLIC_HOST", "sip.exemplo.ao"),
            (
                "CORS_ORIGINS",
                "https://delonix-server.meet.svc.cluster.local:8080",
            ),
        ],
    )
    .await;
    let a = app.login("admin@alfa-qr-sem.ao").await;
    let org = ramal["org_id"].as_str().unwrap();
    let (st, body) = app
        .post(
            &format!(
                "/api/orgs/{org}/extensions/{}/provisioning-ticket",
                ramal["id"].as_str().unwrap()
            ),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 422, "{body}");
    assert_eq!(body["code"], "ramais.public_url_missing");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_extension_provisioning_tickets")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
}
