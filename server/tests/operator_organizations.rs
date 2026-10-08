//! Rotas de operador para o backoffice (PR2): listar organizações e gerir as
//! suas quotas de plano — contra Postgres real.
//!
//! Mesmo padrão de `security_voice_odoo.rs::shared_did_pool_requires_platform_admin`:
//! o operador regista-se num servidor de arranque, cai fora, e o servidor do
//! teste já o declara em `PLATFORM_ADMIN_USER_IDS` (o utilizador tem de
//! existir ANTES da configuração que o declara).
mod common;

use common::TestApp;
use serde_json::json;

#[sqlx::test(migrations = "./migrations")]
async fn operador_lista_e_gere_quotas_de_tenants(db: sqlx::PgPool) {
    let boot = TestApp::spawn(db.clone()).await;
    let operator = boot.new_org("operador-orgs.ao").await;
    drop(boot);
    let app = TestApp::spawn_with(db, &[("PLATFORM_ADMIN_USER_IDS", &operator.user_id)]).await;

    let alfa = app.new_org("alfa-operador.ao").await;
    let beta = app.new_org("beta-operador.ao").await;
    // Um membro a mais em alfa, para o `member_count` não coincidir por acaso.
    app.add_member(&alfa, "membro", "member").await;

    // --- Listagem: as duas aparecem, cada uma com o seu member_count, SEM
    //     uso agregado (isso é só o detalhe). ---
    let (st, page) = app
        .get(
            "/api/operator/v1/organizations?page_size=100",
            Some(&operator.token),
        )
        .await;
    assert_eq!(st, 200, "{page}");
    let items = page["items"].as_array().expect("items é array");
    let alfa_row = items
        .iter()
        .find(|o| o["id"] == alfa.org())
        .cloned()
        .unwrap_or_else(|| panic!("alfa não está na lista: {page}"));
    let beta_row = items
        .iter()
        .find(|o| o["id"] == beta.org())
        .cloned()
        .unwrap_or_else(|| panic!("beta não está na lista: {page}"));
    assert_eq!(alfa_row["member_count"], 2, "admin + membro: {alfa_row}");
    assert_eq!(beta_row["member_count"], 1, "só o admin: {beta_row}");
    assert!(
        alfa_row.get("seats").is_none() && alfa_row.get("storage").is_none(),
        "a listagem não traz uso agregado: {alfa_row}"
    );

    // --- Admin de tenant (não operador): 403 nas três rotas novas. ---
    let (st, e) = app
        .get("/api/operator/v1/organizations", Some(&alfa.token))
        .await;
    assert_eq!(st, 403, "{e}");
    let (st, e) = app
        .get(
            &format!("/api/operator/v1/organizations/{}", alfa.org()),
            Some(&alfa.token),
        )
        .await;
    assert_eq!(st, 403, "{e}");
    let (st, e) = app
        .put(
            &format!("/api/operator/v1/organizations/{}/quotas", alfa.org()),
            Some(&alfa.token),
            json!({"max_rooms": 5}),
        )
        .await;
    assert_eq!(st, 403, "{e}");

    // --- Detalhe: traz uso real (lugares e armazenamento). ---
    let (st, detail) = app
        .get(
            &format!("/api/operator/v1/organizations/{}", alfa.org()),
            Some(&operator.token),
        )
        .await;
    assert_eq!(st, 200, "{detail}");
    assert_eq!(detail["org"]["id"], alfa.org());
    assert_eq!(detail["org"]["member_count"], 2, "{detail}");
    assert_eq!(detail["seats"]["used"].as_i64(), Some(2), "{detail}");
    assert_eq!(detail["storage"]["org_id"], alfa.org(), "{detail}");

    // --- Altera as quotas de ALFA: não afecta BETA. ---
    let (st, updated) = app
        .put(
            &format!("/api/operator/v1/organizations/{}/quotas", alfa.org()),
            Some(&operator.token),
            json!({"max_groups": 2, "max_rooms": 7, "max_meetings": -1,
                   "voice_media_backend": "provider", "voice_did_model": "dedicated"}),
        )
        .await;
    assert_eq!(st, 200, "{updated}");
    assert_eq!(updated["max_groups"], 2, "{updated}");
    assert_eq!(updated["max_rooms"], 7, "{updated}");
    assert!(
        updated["max_meetings"].is_null(),
        "negativo = ilimitado: {updated}"
    );

    let (backend, did_model): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT voice_media_backend, voice_did_model FROM organizations WHERE id = $1::uuid",
    )
    .bind(alfa.org())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(backend.as_deref(), Some("provider"), "gravado na base");
    assert_eq!(did_model.as_deref(), Some("dedicated"), "gravado na base");

    // Um valor de voz fora do enum não apaga o actual (COALESCE, como
    // `org::update_settings`).
    let (st, _) = app
        .put(
            &format!("/api/operator/v1/organizations/{}/quotas", alfa.org()),
            Some(&operator.token),
            json!({"voice_media_backend": "bogus"}),
        )
        .await;
    assert_eq!(st, 200);
    let (backend2,): (Option<String>,) =
        sqlx::query_as("SELECT voice_media_backend FROM organizations WHERE id = $1::uuid")
            .bind(alfa.org())
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(
        backend2.as_deref(),
        Some("provider"),
        "enum inválido mantém o actual"
    );

    let (_, beta_detail) = app
        .get(
            &format!("/api/operator/v1/organizations/{}", beta.org()),
            Some(&operator.token),
        )
        .await;
    assert!(
        beta_detail["org"]["max_rooms"].is_null(),
        "beta não foi tocada pelas quotas de alfa: {beta_detail}"
    );

    // --- Os irmãos `seats`/`concurrency` (já existentes) também só tocam
    //     em ALFA: a mesma garantia de isolamento, pelas rotas vizinhas. ---
    let (st, s) = app
        .put(
            &format!("/api/operator/v1/organizations/{}/seats", alfa.org()),
            Some(&operator.token),
            json!({"max_seats": 10}),
        )
        .await;
    assert_eq!(st, 200, "{s}");
    let (st, c) = app
        .put(
            &format!("/api/operator/v1/organizations/{}/concurrency", alfa.org()),
            Some(&operator.token),
            json!({"max_concurrent_participants": 50}),
        )
        .await;
    assert_eq!(st, 200, "{c}");

    let (_, alfa_detail2) = app
        .get(
            &format!("/api/operator/v1/organizations/{}", alfa.org()),
            Some(&operator.token),
        )
        .await;
    assert_eq!(alfa_detail2["org"]["max_seats"], 10, "{alfa_detail2}");
    assert_eq!(
        alfa_detail2["org"]["max_concurrent_participants"], 50,
        "{alfa_detail2}"
    );
    let (_, beta_detail2) = app
        .get(
            &format!("/api/operator/v1/organizations/{}", beta.org()),
            Some(&operator.token),
        )
        .await;
    assert!(
        beta_detail2["org"]["max_seats"].is_null(),
        "beta não foi tocada pelo seats de alfa: {beta_detail2}"
    );
    assert!(
        beta_detail2["org"]["max_concurrent_participants"].is_null(),
        "beta não foi tocada pela concurrency de alfa: {beta_detail2}"
    );
}
