//! OAuth 2.1 para clientes MCP contra Postgres e servidor reais (ADR-0028).
//!
//! O que se prova, por ordem do que mais importa:
//!
//! 1. **o token do agente não é uma sessão web** — `typ: "mcp"` com `aud` do
//!    recurso não abre uma única rota do BFF;
//! 2. **terminar a sessão mata o agente** — a autorização aparece em
//!    «Sessões activas» e, terminada, a renovação morre;
//! 3. **retirar o consentimento mata-o também**, no mesmo instante;
//! 4. **o que não bate é sempre o mesmo erro** — código gasto, PKCE errado,
//!    cliente trocado: `oauth.invalid_grant`, sem dizer qual foi;
//! 5. **um refresh reapresentado revoga a família** (RFC 9700 §4.14.2);
//! 6. **o `redirect_uri` não serve de redirector aberto**: o erro só volta
//!    pelo redirect quando o endereço está provado como daquele cliente;
//! 7. **isolamento** — a concessão de outra pessoa dá `404`, e a organização
//!    do agente é a de quem consentiu, nunca uma escolha do cliente.
mod common;

use common::{jwt_claims, TestApp};
use serde_json::{json, Value};
use sqlx::PgPool;

/// Par PKCE do RFC 7636 Appendix B — o mesmo que prova
/// `core::crypto::pkce_s256_matches`.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const PUBLIC_URL: &str = "https://meet.test";
const REDIRECT: &str = "http://127.0.0.1:33418/callback";

fn resource() -> String {
    format!("{PUBLIC_URL}/api/mcp")
}

async fn app(db: PgPool) -> TestApp {
    TestApp::spawn_with(db, &[("PUBLIC_URL", PUBLIC_URL)]).await
}

/// Registo dinâmico (RFC 7591). Devolve o `client_id`.
async fn register(app: &TestApp, nome: &str) -> String {
    let (st, body) = app
        .post(
            "/api/oauth/register",
            None,
            json!({"client_name": nome, "redirect_uris": [REDIRECT]}),
        )
        .await;
    assert_eq!(st, 201, "registo: {body}");
    assert!(
        body.get("client_secret").is_none(),
        "um cliente público não recebe segredo: {body}"
    );
    body["client_id"].as_str().unwrap().to_string()
}

fn pedido(client_id: &str, scope: &str) -> Value {
    json!({
        "response_type": "code",
        "client_id": client_id,
        "redirect_uri": REDIRECT,
        "scope": scope,
        "code_challenge": CHALLENGE,
        "code_challenge_method": "S256",
        "resource": resource(),
        "state": "xyz-123",
    })
}

fn query_de(p: &Value) -> String {
    let o = p.as_object().unwrap();
    o.iter()
        .map(|(k, v)| format!("{k}={}", urlencoding(v.as_str().unwrap_or_default())))
        .collect::<Vec<_>>()
        .join("&")
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// O código que sai do consentimento aprovado.
async fn aprovar(app: &TestApp, token: &str, p: &Value) -> String {
    let mut body = p.clone();
    body["approve"] = json!(true);
    let (st, r) = app
        .post("/api/oauth/authorizations", Some(token), body)
        .await;
    assert_eq!(st, 200, "consentimento: {r}");
    let url = r["redirect_to"].as_str().unwrap();
    let u = url::Url::parse(url).unwrap();
    assert_eq!(
        u.query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.to_string()),
        Some("xyz-123".into()),
        "o `state` do cliente volta tal e qual"
    );
    u.query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .expect("o redirect traz o código")
}

async fn trocar(app: &TestApp, client_id: &str, code: &str) -> (u16, Value) {
    app.post(
        "/api/oauth/token",
        None,
        json!({
            "grant_type": "authorization_code",
            "client_id": client_id,
            "code": code,
            "code_verifier": VERIFIER,
            "redirect_uri": REDIRECT,
            "resource": resource(),
        }),
    )
    .await
}

// ---------------------------------------------------------------------------

#[sqlx::test]
async fn metadata_descreve_o_servidor_e_o_recurso(db: PgPool) {
    let app = app(db).await;

    let (st, as_md) = app
        .get("/.well-known/oauth-authorization-server", None)
        .await;
    assert_eq!(st, 200, "{as_md}");
    assert_eq!(as_md["issuer"], PUBLIC_URL);
    assert_eq!(as_md["code_challenge_methods_supported"], json!(["S256"]));
    assert_eq!(
        as_md["token_endpoint_auth_methods_supported"],
        json!(["none"]),
        "cliente público: quem prova é o PKCE"
    );
    assert!(
        !as_md["grant_types_supported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g == "password" || g == "implicit"),
        "OAuth 2.1 não tem password nem implícito: {as_md}"
    );

    let (st, prm) = app.get("/.well-known/oauth-protected-resource", None).await;
    assert_eq!(st, 200, "{prm}");
    assert_eq!(prm["resource"], resource());
    assert_eq!(prm["authorization_servers"], json!([PUBLIC_URL]));
}

#[sqlx::test]
async fn sem_public_url_o_oauth_esta_fechado(db: PgPool) {
    // Fail-closed e em voz alta: sem nome canónico não há público para o
    // token, e emitir um sem público é pior do que não emitir nenhum.
    let app = TestApp::spawn(db).await;
    let (st, body) = app.get("/.well-known/oauth-protected-resource", None).await;
    assert_eq!(st, 503, "{body}");
    let (st, _) = app
        .post(
            "/api/oauth/register",
            None,
            json!({"client_name": "X", "redirect_uris": [REDIRECT]}),
        )
        .await;
    // O registo não depende do recurso; o que não se consegue é autorizar.
    assert_eq!(st, 201);
    let (st, body) = app
        .get(
            &format!(
                "/api/oauth/authorize?{}",
                query_de(&pedido("dlxc_x", "meet.read"))
            ),
            None,
        )
        .await;
    assert_eq!(st, 503, "{body}");
}

#[sqlx::test]
async fn registo_recusa_um_endereco_de_retorno_em_claro_fora_de_loopback(db: PgPool) {
    let app = app(db).await;
    for mau in [
        "http://exemplo.ao/cb",     // em claro na rede
        "http://localhost:7777/cb", // por nome: resolve-se onde o cliente estiver
        "app://retorno",            // esquema que não verificamos
        "https://*.exemplo.ao/cb",
    ] {
        let (st, body) = app
            .post(
                "/api/oauth/register",
                None,
                json!({"client_name": "Mau", "redirect_uris": [mau]}),
            )
            .await;
        assert_eq!(st, 400, "{mau} devia ser recusado: {body}");
        assert_eq!(body["code"], "oauth.invalid_redirect_uri", "{mau}: {body}");
    }
}

#[sqlx::test]
async fn registo_dinamico_desligado_recusa(db: PgPool) {
    let app = TestApp::spawn_with(
        db,
        &[
            ("PUBLIC_URL", PUBLIC_URL),
            ("OAUTH_DYNAMIC_REGISTRATION", "off"),
        ],
    )
    .await;
    let (st, body) = app
        .post(
            "/api/oauth/register",
            None,
            json!({"client_name": "X", "redirect_uris": [REDIRECT]}),
        )
        .await;
    assert_eq!(st, 403, "{body}");
    assert_eq!(body["code"], "oauth.registration_disabled");
    let (_, md) = app
        .get("/.well-known/oauth-authorization-server", None)
        .await;
    assert!(
        md["registration_endpoint"].is_null(),
        "desligado, a metadata não anuncia o endpoint: {md}"
    );
}

#[sqlx::test]
async fn authorize_manda_ao_consentimento_e_nao_e_um_redirector_aberto(db: PgPool) {
    let app = app(db).await;
    let client = register(&app, "Claude Desktop").await;

    // Pedido bom: 303 para o ecrã de consentimento da web, com os parâmetros.
    let res = app
        .raw(
            reqwest::Method::GET,
            &format!(
                "/api/oauth/authorize?{}",
                query_de(&pedido(&client, "meet.read"))
            ),
            &[],
            None,
        )
        .await;
    assert_eq!(res.status, 303, "{}", res.text);
    let loc = res.header("location").expect("location");
    assert!(
        loc.starts_with(&format!("{PUBLIC_URL}/#/oauth/consent?")),
        "o destino é o ecrã de consentimento, por hash: {loc}"
    );
    assert!(loc.contains("code_challenge=E9Melhoa"), "{loc}");

    // Pedido mau COM endereço provado: o erro volta ao cliente, como o RFC manda.
    let mut mau = pedido(&client, "meet.read");
    mau["code_challenge_method"] = json!("plain");
    let res = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/oauth/authorize?{}", query_de(&mau)),
            &[],
            None,
        )
        .await;
    assert_eq!(res.status, 303, "{}", res.text);
    let loc = res.header("location").unwrap();
    assert!(loc.starts_with("http://127.0.0.1:33418/callback?"), "{loc}");
    assert!(loc.contains("error=invalid_request"), "{loc}");
    assert!(
        loc.contains("state=xyz-123"),
        "o state volta no erro: {loc}"
    );

    // Pedido mau com endereço NÃO registado: responde aqui. Mandar um erro
    // para um endereço não verificado seria um redirector aberto nosso.
    let mut forjado = pedido(&client, "meet.read");
    forjado["redirect_uri"] = json!("https://atacante.example/cb");
    let res = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/oauth/authorize?{}", query_de(&forjado)),
            &[],
            None,
        )
        .await;
    assert_eq!(
        res.status, 400,
        "um endereço não registado nunca recebe redirect: {}",
        res.text
    );
    assert!(
        res.header("location").is_none(),
        "nenhum Location para um endereço não registado"
    );
}

#[sqlx::test]
async fn o_ecra_de_consentimento_diz_quem_pede_e_o_que_abre(db: PgPool) {
    let app = app(db).await;
    let dono = app.new_org("alfa.ao").await;
    let client = register(&app, "Claude Code").await;
    let p = pedido(&client, "meet.read captions.write");

    let (st, info) = app
        .get(
            &format!("/api/oauth/authorization-info?{}", query_de(&p)),
            Some(&dono.token),
        )
        .await;
    assert_eq!(st, 200, "{info}");
    assert_eq!(info["client_name"], "Claude Code");
    assert_eq!(info["registered_dynamically"], json!(true));
    assert_eq!(info["organization_id"], dono.org());
    assert_eq!(info["already_granted"], json!(false));
    let escopos = info["scopes"].as_array().unwrap();
    assert_eq!(escopos.len(), 2, "{info}");
    for e in escopos {
        assert!(
            !e["describes"].as_str().unwrap_or_default().is_empty(),
            "cada escopo traz o que abre, em linguagem de pessoa: {e}"
        );
    }
    // A ordem é a do catálogo: ler antes de escrever.
    assert_eq!(escopos[0]["scope"], "meet.read");
    assert_eq!(escopos[0]["read_only"], json!(true));
    assert_eq!(escopos[1]["scope"], "captions.write");
    assert_eq!(escopos[1]["read_only"], json!(false));

    // Sem sessão não há ecrã: é a pessoa que decide.
    let (st, _) = app
        .get(
            &format!("/api/oauth/authorization-info?{}", query_de(&p)),
            None,
        )
        .await;
    assert_eq!(st, 401);

    // Negar devolve `access_denied` e nenhum código.
    let mut nega = p.clone();
    nega["approve"] = json!(false);
    let (st, r) = app
        .post("/api/oauth/authorizations", Some(&dono.token), nega)
        .await;
    assert_eq!(st, 200, "{r}");
    let loc = r["redirect_to"].as_str().unwrap();
    assert!(loc.contains("error=access_denied"), "{loc}");
    assert!(!loc.contains("code="), "negar não emite código: {loc}");
}

#[sqlx::test]
async fn o_token_do_agente_nao_abre_o_bff(db: PgPool) {
    let app = app(db).await;
    let dono = app.new_org("beta.ao").await;
    let client = register(&app, "Agente").await;
    let p = pedido(&client, "meet.read");
    let code = aprovar(&app, &dono.token, &p).await;
    let (st, tok) = trocar(&app, &client, &code).await;
    assert_eq!(st, 200, "{tok}");
    let mcp = tok["access_token"].as_str().unwrap();

    // A forma do token: outro `typ`, e o público do recurso.
    let claims = jwt_claims(mcp);
    assert_eq!(claims["typ"], "mcp");
    assert_eq!(claims["aud"], resource());
    assert_eq!(claims["scope"], "meet.read");
    assert_eq!(claims["sub"], dono.user_id);
    assert!(claims["sid"].is_string(), "leva a sessão: {claims}");
    assert_eq!(tok["token_type"], "Bearer");

    // E não abre o BFF. É a razão de o `typ` ser outro.
    for rota in [
        "/api/users/me",
        "/api/meetings",
        "/api/recordings",
        "/api/orgs",
    ] {
        let (st, body) = app.get(rota, Some(mcp)).await;
        assert_eq!(st, 401, "um token de MCP não abre {rota}: {body}");
    }

    // Ao contrário: o token da sessão web não serve de token de agente.
    let claims_web = jwt_claims(&dono.token);
    assert_eq!(claims_web["typ"], "access");
}

#[sqlx::test]
async fn o_codigo_e_de_uso_unico_e_o_que_nao_bate_da_sempre_o_mesmo_erro(db: PgPool) {
    let app = app(db).await;
    let dono = app.new_org("gama.ao").await;
    let client = register(&app, "Agente").await;
    let outro = register(&app, "Outro agente").await;

    // PKCE errado.
    let code = aprovar(&app, &dono.token, &pedido(&client, "meet.read")).await;
    let (st, body) = app
        .post(
            "/api/oauth/token",
            None,
            json!({"grant_type": "authorization_code", "client_id": client,
                   "code": code, "code_verifier": "x".repeat(43), "redirect_uri": REDIRECT}),
        )
        .await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "oauth.invalid_grant");

    // Cliente trocado.
    let code = aprovar(&app, &dono.token, &pedido(&client, "meet.read")).await;
    let (st, body) = trocar(&app, &outro, &code).await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "oauth.invalid_grant");

    // Uso único: o segundo pedido com o MESMO código falha.
    let code = aprovar(&app, &dono.token, &pedido(&client, "meet.read")).await;
    let (st, _) = trocar(&app, &client, &code).await;
    assert_eq!(st, 200);
    let (st, body) = trocar(&app, &client, &code).await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "oauth.invalid_grant");

    // Código inventado.
    let (st, body) = trocar(&app, &client, "dlxac_inventado").await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "oauth.invalid_grant");
}

#[sqlx::test]
async fn o_refresh_roda_e_reapresentar_um_usado_revoga_a_familia(db: PgPool) {
    let app = app(db).await;
    let dono = app.new_org("delta.ao").await;
    let client = register(&app, "Agente").await;
    let code = aprovar(&app, &dono.token, &pedido(&client, "meet.read")).await;
    let (_, tok) = trocar(&app, &client, &code).await;
    let r1 = tok["refresh_token"].as_str().unwrap().to_string();

    // Roda: usar emite outro, e o primeiro morre.
    let (st, tok2) = app
        .post(
            "/api/oauth/token",
            None,
            json!({"grant_type": "refresh_token", "client_id": client, "refresh_token": r1}),
        )
        .await;
    assert_eq!(st, 200, "{tok2}");
    let r2 = tok2["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(r1, r2, "o refresh é rotativo");

    // Reapresentar o PRIMEIRO é roubo: revoga a família, e o segundo também cai.
    let (st, body) = app
        .post(
            "/api/oauth/token",
            None,
            json!({"grant_type": "refresh_token", "client_id": client, "refresh_token": r1}),
        )
        .await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "oauth.invalid_grant");

    let (st, body) = app
        .post(
            "/api/oauth/token",
            None,
            json!({"grant_type": "refresh_token", "client_id": client, "refresh_token": r2}),
        )
        .await;
    assert_eq!(
        st, 400,
        "a família inteira cai quando se detecta reutilização: {body}"
    );
}

#[sqlx::test]
async fn o_agente_e_uma_sessao_e_terminar_a_sessao_mata_o(db: PgPool) {
    let app = app(db).await;
    let dono = app.new_org("epsilon.ao").await;
    let client = register(&app, "Claude Desktop").await;
    let code = aprovar(&app, &dono.token, &pedido(&client, "meet.read")).await;
    let (_, tok) = trocar(&app, &client, &code).await;
    let sid = jwt_claims(tok["access_token"].as_str().unwrap())["sid"]
        .as_str()
        .unwrap()
        .to_string();
    let refresh = tok["refresh_token"].as_str().unwrap().to_string();

    // Aparece na lista de sessões activas, com o nome do cliente.
    let (st, sessoes) = app.get("/api/users/me/sessions", Some(&dono.token)).await;
    assert_eq!(st, 200, "{sessoes}");
    let agente = sessoes
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == sid)
        .unwrap_or_else(|| panic!("a sessão do agente tem de aparecer: {sessoes}"));
    assert!(
        agente.to_string().contains("Claude Desktop"),
        "a pessoa tem de saber QUAL é o agente: {agente}"
    );

    // Terminar corta a renovação do agente — é a máquina da 0078, não outra.
    let (st, _) = app
        .delete(&format!("/api/users/me/sessions/{sid}"), Some(&dono.token))
        .await;
    assert_eq!(st, 204);
    let (st, body) = app
        .post(
            "/api/oauth/token",
            None,
            json!({"grant_type": "refresh_token", "client_id": client, "refresh_token": refresh}),
        )
        .await;
    assert!(
        st >= 400,
        "sessão terminada, o agente não renova: {st} {body}"
    );
}

#[sqlx::test]
async fn retirar_o_consentimento_mata_o_agente_e_a_concessao_de_outro_da_404(db: PgPool) {
    let app = app(db).await;
    let a = app.new_org("zeta.ao").await;
    let b = app.new_org("eta.ao").await;
    let client = register(&app, "Agente").await;

    let code = aprovar(&app, &a.token, &pedido(&client, "meet.read")).await;
    let (_, tok) = trocar(&app, &client, &code).await;
    let refresh = tok["refresh_token"].as_str().unwrap().to_string();

    let (st, grants) = app.get("/api/users/me/oauth-grants", Some(&a.token)).await;
    assert_eq!(st, 200, "{grants}");
    let grant_id = grants[0]["id"].as_str().unwrap().to_string();
    assert_eq!(grants[0]["client_name"], "Agente");

    // A de outra pessoa não existe para quem não é dono — mesma resposta que
    // um id inventado.
    let (st, _) = app
        .delete(
            &format!("/api/users/me/oauth-grants/{grant_id}"),
            Some(&b.token),
        )
        .await;
    assert_eq!(st, 404);
    let (st, suas) = app.get("/api/users/me/oauth-grants", Some(&b.token)).await;
    assert_eq!(st, 200);
    assert_eq!(suas.as_array().unwrap().len(), 0, "{suas}");

    // O dono retira: a concessão sai e o agente deixa de renovar.
    let (st, _) = app
        .delete(
            &format!("/api/users/me/oauth-grants/{grant_id}"),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 204);
    let (st, body) = app
        .post(
            "/api/oauth/token",
            None,
            json!({"grant_type": "refresh_token", "client_id": client, "refresh_token": refresh}),
        )
        .await;
    assert!(
        st >= 400,
        "consentimento retirado, nada renova: {st} {body}"
    );
    let (_, grants) = app.get("/api/users/me/oauth-grants", Some(&a.token)).await;
    assert_eq!(grants.as_array().unwrap().len(), 0, "{grants}");
}

#[sqlx::test]
async fn o_recurso_e_o_escopo_sao_fechados(db: PgPool) {
    let app = app(db).await;
    let dono = app.new_org("teta.ao").await;
    let client = register(&app, "Agente").await;

    // Recurso de outro servidor: recusado. É o que dá público ao token.
    let mut outro = pedido(&client, "meet.read");
    outro["resource"] = json!("https://outro.example/api/mcp");
    outro["approve"] = json!(true);
    let (st, body) = app
        .post("/api/oauth/authorizations", Some(&dono.token), outro)
        .await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "oauth.invalid_target");

    // Escopo que não existe: recusado, não ignorado.
    let mut inventado = pedido(&client, "meet.read tudo");
    inventado["approve"] = json!(true);
    let (st, body) = app
        .post("/api/oauth/authorizations", Some(&dono.token), inventado)
        .await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "oauth.invalid_scope");

    // E um escopo das chaves `dlx_` não vale aqui: são catálogos diferentes.
    let mut dlx = pedido(&client, "rooms:read");
    dlx["approve"] = json!(true);
    let (st, body) = app
        .post("/api/oauth/authorizations", Some(&dono.token), dlx)
        .await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "oauth.invalid_scope");
}
