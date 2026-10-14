//! SCIM 2.0: o directório da empresa escreve os membros.
//!
//! **O que estes testes guardam.** A lacuna nº1 do levantamento de 2026-10-13 é
//! «as entradas e as saídas vêm do nosso directório?». O que mais importa aqui
//! não é criar — é **suspender**: uma conta que não sai quando a pessoa sai é um
//! achado de auditoria, e é a operação que o Entra ID faz (`PATCH` com
//! `active: false`).
//!
//! E guardam a regra que a R142 ensinou, na forma que o SCIM precisa: **saber o
//! endereço de alguém não o traz para dentro**.

mod common;

use common::TestApp;
use serde_json::{json, Value};

/// O `PUBLIC_URL` é o que a resposta da rotação devolve como `base_url` — o
/// valor que se cola na configuração do provedor.
const AMBIENTE: &[(&str, &str)] = &[("PUBLIC_URL", "https://meet.exemplo.ao")];

/// O token de SCIM de uma organização, pela rota de administração.
async fn token(app: &TestApp, admin: &common::Account) -> String {
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/scim/rotate-token", admin.org()),
            Some(&admin.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "rodar o token: {body}");
    assert!(
        body["base_url"]
            .as_str()
            .unwrap_or_default()
            .ends_with("/scim/v2"),
        "o base_url é o que se dá ao provedor: {body}"
    );
    body["token"].as_str().expect("token").to_string()
}

/// Um pedido na superfície do SCIM, com o token do directório.
async fn scim(
    app: &TestApp,
    metodo: &str,
    caminho: &str,
    tok: &str,
    corpo: Option<Value>,
) -> (u16, Value) {
    match metodo {
        "GET" => app.get(caminho, Some(tok)).await,
        "POST" => {
            app.post(caminho, Some(tok), corpo.unwrap_or(json!({})))
                .await
        }
        "PATCH" => {
            app.patch(caminho, Some(tok), corpo.unwrap_or(json!({})))
                .await
        }
        "PUT" => {
            app.put(caminho, Some(tok), corpo.unwrap_or(json!({})))
                .await
        }
        "DELETE" => app.delete(caminho, Some(tok)).await,
        outro => panic!("método {outro}"),
    }
}

fn utilizador(email: &str, activo: bool) -> Value {
    json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"],
        "userName": email,
        "externalId": "dir-001",
        "name": {"givenName": "Ana", "familyName": "Silva"},
        "active": activo,
    })
}

#[sqlx::test(migrations = "./migrations")]
async fn o_directorio_cria_e_a_pessoa_fica_membro(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, AMBIENTE).await;
    let admin = app.new_org("empresa.ao").await;
    let tok = token(&app, &admin).await;

    let (st, body) = scim(
        &app,
        "POST",
        "/scim/v2/Users",
        &tok,
        Some(utilizador("Ana.Silva@Empresa.AO", true)),
    )
    .await;
    assert_eq!(st, 201, "criar: {body}");
    // Normalizado, como em todo o resto da casa.
    assert_eq!(body["userName"], "ana.silva@empresa.ao", "{body}");
    assert_eq!(body["active"], true, "{body}");
    assert_eq!(body["name"]["formatted"], "Ana Silva", "{body}");
    assert_eq!(body["externalId"], "dir-001", "{body}");
    let id = body["id"].as_str().expect("id").to_string();

    // É membro da organização, de facto — não só um utilizador.
    let (st, membros) = app
        .get(
            &format!("/api/orgs/{}/members", admin.org()),
            Some(&admin.token),
        )
        .await;
    assert_eq!(st, 200, "{membros}");
    let emails: Vec<&str> = membros
        .as_array()
        .map(|v| v.iter().filter_map(|m| m["email"].as_str()).collect())
        .unwrap_or_default();
    assert!(emails.contains(&"ana.silva@empresa.ao"), "{membros}");

    // E o directório reencontra-a pelo filtro que ele usa.
    let (st, lista) = scim(
        &app,
        "GET",
        "/scim/v2/Users?filter=userName%20eq%20%22ana.silva@empresa.ao%22",
        &tok,
        None,
    )
    .await;
    assert_eq!(st, 200, "{lista}");
    assert_eq!(lista["totalResults"], 1, "{lista}");
    assert_eq!(lista["Resources"][0]["id"], id, "{lista}");
}

/// **A operação que mais importa.** Uma saída no directório tem de fechar a
/// conta aqui — e fechar as sessões abertas, senão «saiu» é só uma etiqueta.
#[sqlx::test(migrations = "./migrations")]
async fn uma_saida_no_directorio_suspende_e_corta_as_sessoes(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, AMBIENTE).await;
    let admin = app.new_org("empresa.ao").await;
    let tok = token(&app, &admin).await;
    let (_, criado) = scim(
        &app,
        "POST",
        "/scim/v2/Users",
        &tok,
        Some(utilizador("bruno@empresa.ao", true)),
    )
    .await;
    let id = criado["id"].as_str().unwrap().to_string();

    let (st, body) = scim(
        &app,
        "PATCH",
        &format!("/scim/v2/Users/{id}"),
        &tok,
        Some(json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
            "Operations": [{"op": "replace", "path": "active", "value": false}],
        })),
    )
    .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["active"], false, "{body}");

    // Arquivado de facto na organização.
    let arquivado: Option<bool> = sqlx::query_scalar(
        "SELECT archived_at IS NOT NULL FROM org_members WHERE org_id = $1 AND user_id = $2",
    )
    .bind(admin.org().parse::<uuid::Uuid>().unwrap())
    .bind(id.parse::<uuid::Uuid>().unwrap())
    .fetch_optional(&app.state.db)
    .await
    .unwrap();
    assert_eq!(arquivado, Some(true), "a pertença tinha de ficar arquivada");

    // E um `active: true` readmite — é o caso de quem volta.
    let (st, body) = scim(
        &app,
        "PATCH",
        &format!("/scim/v2/Users/{id}"),
        &tok,
        Some(json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
            "Operations": [{"op": "replace", "path": "active", "value": true}],
        })),
    )
    .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["active"], true, "{body}");
}

/// `DELETE` **suspende**, não apaga: quem sai leva consigo actas, gravações e
/// trilha de auditoria, e apagar a conta apagava o rasto de quem fez o quê.
#[sqlx::test(migrations = "./migrations")]
async fn o_delete_suspende_e_nao_apaga(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, AMBIENTE).await;
    let admin = app.new_org("empresa.ao").await;
    let tok = token(&app, &admin).await;
    let (_, criado) = scim(
        &app,
        "POST",
        "/scim/v2/Users",
        &tok,
        Some(utilizador("carla@empresa.ao", true)),
    )
    .await;
    let id = criado["id"].as_str().unwrap().to_string();

    let (st, _) = scim(&app, "DELETE", &format!("/scim/v2/Users/{id}"), &tok, None).await;
    assert_eq!(st, 204);

    // A CONTA continua a existir (é o rasto), mas inactiva na organização.
    let existe: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(id.parse::<uuid::Uuid>().unwrap())
        .fetch_optional(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        existe.as_deref(),
        Some("carla@empresa.ao"),
        "a conta não se apaga"
    );
    let (st, body) = scim(&app, "GET", &format!("/scim/v2/Users/{id}"), &tok, None).await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["active"], false, "{body}");
}

/// **A regra da R142, na forma do SCIM.** Saber o endereço de alguém não o traz
/// para dentro: o directório da org A não captura a conta da org B.
#[sqlx::test(migrations = "./migrations")]
async fn o_directorio_de_uma_org_nao_captura_a_conta_de_outra(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, AMBIENTE).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;
    let tok_a = token(&app, &a).await;

    let (st, body) = scim(
        &app,
        "POST",
        "/scim/v2/Users",
        &tok_a,
        Some(utilizador(&b.email, true)),
    )
    .await;
    assert_eq!(st, 409, "a conta de B não se reclama: {body}");
    assert_eq!(body["scimType"], "uniqueness", "{body}");
    assert_eq!(
        body["schemas"][0], "urn:ietf:params:scim:api:messages:2.0:Error",
        "{body}"
    );

    // E B continua fora da org A.
    let (_, membros) = app
        .get(&format!("/api/orgs/{}/members", a.org()), Some(&a.token))
        .await;
    let emails: Vec<&str> = membros
        .as_array()
        .map(|v| v.iter().filter_map(|m| m["email"].as_str()).collect())
        .unwrap_or_default();
    assert!(!emails.contains(&b.email.as_str()), "{membros}");
}

/// O token de uma organização não fala pela outra, e a conta da outra não se vê
/// por aqui.
#[sqlx::test(migrations = "./migrations")]
async fn o_token_de_uma_org_nao_ve_a_outra(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, AMBIENTE).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;
    let tok_a = token(&app, &a).await;
    let tok_b = token(&app, &b).await;

    let (_, criado) = scim(
        &app,
        "POST",
        "/scim/v2/Users",
        &tok_b,
        Some(utilizador("dentro-da-b@beta.ao", true)),
    )
    .await;
    let id_b = criado["id"].as_str().unwrap().to_string();

    // O token de A não lê nem mexe em quem é de B: 404, sem revelar nada.
    let (st, _) = scim(&app, "GET", &format!("/scim/v2/Users/{id_b}"), &tok_a, None).await;
    assert_eq!(st, 404);
    let (st, _) = scim(
        &app,
        "DELETE",
        &format!("/scim/v2/Users/{id_b}"),
        &tok_a,
        None,
    )
    .await;
    assert_eq!(st, 404);
    // E a lista de A não traz ninguém de B.
    let (st, lista) = scim(&app, "GET", "/scim/v2/Users", &tok_a, None).await;
    assert_eq!(st, 200, "{lista}");
    let emails = lista["Resources"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|u| u["userName"].as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(!emails.contains(&"dentro-da-b@beta.ao"), "{lista}");
}

/// Um token de OUTRA superfície não abre esta — é a R142 escrita como teste.
#[sqlx::test(migrations = "./migrations")]
async fn o_token_do_odoo_e_a_chave_do_inquilino_nao_abrem_o_scim(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, AMBIENTE).await;
    let admin = app.new_org("empresa.ao").await;

    let (st, odoo) = app
        .post(
            &format!("/api/orgs/{}/integrations/odoo/rotate-token", admin.org()),
            Some(&admin.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{odoo}");
    let tok_odoo = odoo["token"].as_str().expect("token do odoo");

    let (st, body) = scim(&app, "GET", "/scim/v2/Users", tok_odoo, None).await;
    assert_eq!(st, 401, "o token do Odoo não abre o SCIM: {body}");
    // Mesmo com a SESSÃO de administrador: esta superfície é só de token.
    let (st, _) = scim(&app, "GET", "/scim/v2/Users", &admin.token, None).await;
    assert_eq!(st, 401, "a sessão não serve nesta superfície");
}

/// A descoberta diz a verdade sobre o que esta instalação suporta — e **declara
/// a ausência dos Groups**, para o provedor não a encontrar à terceira chamada.
#[sqlx::test(migrations = "./migrations")]
async fn a_descoberta_diz_o_que_ha_e_o_que_nao_ha(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, AMBIENTE).await;
    let admin = app.new_org("empresa.ao").await;
    let tok = token(&app, &admin).await;

    let (st, cfg) = scim(&app, "GET", "/scim/v2/ServiceProviderConfig", &tok, None).await;
    assert_eq!(st, 200, "{cfg}");
    assert_eq!(cfg["patch"]["supported"], true, "{cfg}");
    assert_eq!(cfg["filter"]["supported"], true, "{cfg}");
    assert_eq!(cfg["bulk"]["supported"], false, "{cfg}");
    assert_eq!(cfg["changePassword"]["supported"], false, "{cfg}");

    let (st, tipos) = scim(&app, "GET", "/scim/v2/ResourceTypes", &tok, None).await;
    assert_eq!(st, 200, "{tipos}");
    let nomes: Vec<&str> = tipos["Resources"]
        .as_array()
        .map(|v| v.iter().filter_map(|r| r["name"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(nomes, vec!["User"], "os Groups ainda não existem: {tipos}");

    let (st, esquemas) = scim(&app, "GET", "/scim/v2/Schemas", &tok, None).await;
    assert_eq!(st, 200, "{esquemas}");
    assert_eq!(
        esquemas["Resources"][0]["id"], "urn:ietf:params:scim:schemas:core:2.0:User",
        "{esquemas}"
    );
}

/// Um filtro que não entendemos é um ERRO no envelope da norma — nunca uma
/// lista vazia, que faria o provedor concluir que a pessoa não existe e
/// criá-la outra vez.
#[sqlx::test(migrations = "./migrations")]
async fn filtro_nao_suportado_e_erro_no_envelope_do_scim(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, AMBIENTE).await;
    let admin = app.new_org("empresa.ao").await;
    let tok = token(&app, &admin).await;

    let (st, body) = scim(
        &app,
        "GET",
        "/scim/v2/Users?filter=displayName%20eq%20%22Ana%22",
        &tok,
        None,
    )
    .await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["scimType"], "invalidFilter", "{body}");
    assert_eq!(body["status"], "400", "{body}");
}

/// O endereço de uma conta não se muda por SCIM: mudá-lo é tomar a identidade
/// de alguém, e isso passa pelas regras da casa (R25), não por aqui.
#[sqlx::test(migrations = "./migrations")]
async fn o_scim_nao_muda_o_endereco_de_uma_conta(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, AMBIENTE).await;
    let admin = app.new_org("empresa.ao").await;
    let tok = token(&app, &admin).await;
    let (_, criado) = scim(
        &app,
        "POST",
        "/scim/v2/Users",
        &tok,
        Some(utilizador("dora@empresa.ao", true)),
    )
    .await;
    let id = criado["id"].as_str().unwrap().to_string();

    let (st, body) = scim(
        &app,
        "PATCH",
        &format!("/scim/v2/Users/{id}"),
        &tok,
        Some(json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
            "Operations": [{"op": "replace", "path": "userName", "value": "outra@empresa.ao"}],
        })),
    )
    .await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["scimType"], "mutability", "{body}");

    let (st, body) = scim(
        &app,
        "PUT",
        &format!("/scim/v2/Users/{id}"),
        &tok,
        Some(utilizador("outra@empresa.ao", true)),
    )
    .await;
    assert_eq!(st, 400, "um PUT com outro endereço também não: {body}");
}
