//! Ataques directos a regras de acesso, com controlo positivo (R51/R94): cada
//! recusa é precedida pela prova de que o mesmo pedido funciona para quem tem
//! direito — senão o teste mediria uma avaria, não a regra.
mod common;

use common::TestApp;
use serde_json::json;

/// R125 — `PATCH /api/action-items/{id}` com corpo vazio devolvia o item (o
/// quê, porquê, quem, recursos) a QUALQUER conta autenticada: a verificação
/// de pertença só corria quando o pedido trazia `status`.
#[sqlx::test(migrations = "./migrations")]
async fn action_item_patch_does_not_leak_to_other_org(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;

    let starts_at = (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339();
    let (st, meeting) = app
        .post(
            "/api/meetings",
            Some(&a.token),
            json!({"title": "Conselho", "starts_at": starts_at, "duration_min": 30}),
        )
        .await;
    assert_eq!(st, 200, "{meeting}");
    let meeting_id = meeting["id"].as_str().unwrap();
    let (st, item) = app
        .post(
            &format!("/api/meetings/{meeting_id}/action-plan/items"),
            Some(&a.token),
            json!({"what": "fusão confidencial com a Gama", "why": "segredo da empresa A", "status": "todo"}),
        )
        .await;
    assert_eq!(st, 200, "{item}");
    let item_id = item["id"].as_str().unwrap();
    let path = format!("/api/action-items/{item_id}");

    // Controlo positivo: o dono lê o item por PATCH vazio.
    let (st, own) = app.patch(&path, Some(&a.token), json!({})).await;
    assert_eq!(st, 200, "{own}");
    assert_eq!(own["what"], "fusão confidencial com a Gama");

    // O ataque: outra organização, corpo vazio.
    let (st, leaked) = app.patch(&path, Some(&b.token), json!({})).await;
    assert!(!(200..300).contains(&st), "fuga: {st} {leaked}");
    let text = leaked.to_string();
    assert!(
        !text.contains("confidencial") && !text.contains("segredo"),
        "{text}"
    );
}

/// R150 — `POST /api/orgs/{org}/members` sem `password` criava a conta com
/// a password FIXA `changeme123`: quem soubesse o email de um colaborador
/// recém-adicionado (e que ainda não tivesse mudado a password) entrava como
/// ele.
#[sqlx::test(migrations = "./migrations")]
async fn added_employee_without_password_does_not_get_a_known_password(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("alfa.ao").await;
    let (st, emp) = app
        .post(
            &format!("/api/orgs/{}/members", admin.org()),
            Some(&admin.token),
            json!({"email": "novo@alfa.ao", "title": "Analista"}),
        )
        .await;
    assert_eq!(st, 200, "{emp}");
    assert_eq!(emp["email"], "novo@alfa.ao");

    // O ataque: a password por omissão conhecida.
    let (st, body) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": "novo@alfa.ao", "password": "changeme123"}),
        )
        .await;
    assert_ne!(st, 200, "a password por omissão abre a conta: {body}");

    // Controlo positivo: a password temporária devolvida UMA vez ao admin abre.
    let temp = emp["temporary_password"]
        .as_str()
        .expect("a resposta traz a password temporária para o admin entregar");
    assert!(temp.len() >= 16);
    let (st, _) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": "novo@alfa.ao", "password": temp}),
        )
        .await;
    assert_eq!(st, 200);

    // Com password indicada pelo admin, nada de temporária.
    let (st, emp2) = app
        .post(
            &format!("/api/orgs/{}/members", admin.org()),
            Some(&admin.token),
            json!({"email": "outro@alfa.ao", "password": "UmaPasswordForte123!"}),
        )
        .await;
    assert_eq!(st, 200, "{emp2}");
    assert!(emp2.get("temporary_password").is_none());
}
