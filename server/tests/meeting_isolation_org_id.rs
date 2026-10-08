//! TDD do isolamento de reuniões por `org_id` explícito.
//!
//! Auditoria de leitura de código, 2026-10-08 (achado "T1"): a tabela
//! `meetings` (migração 0004) só guardava `owner_id`, nunca a organização em
//! cujo contexto nasceu. `meetings_v1::meeting_in_org` — usada por
//! `GET`/`PATCH`/`DELETE`/`ring` de `/api/v1/meetings/{id}` — e as duas
//! queries equivalentes em `apikeys.rs` (`v1_meetings`, `v1_meeting_notes`)
//! decidiam se uma reunião "pertence" à organização da chave `dlx_` só
//! verificando se o DONO tinha uma linha em `org_members` para essa
//! organização: sem filtrar `archived_at IS NULL` e sem nenhum registo
//! explícito da organização em que a reunião foi criada.
//!
//! Isto fica aberto a um utilizador que já foi membro da organização B —
//! arquivado — e cuja reunião é, de facto, assunto doutra organização (aqui
//! chamada A): a chave `dlx_` da B continuava a conseguir listá-la, ler-lhe
//! a ata/transcrição, alterá-la, apagá-la e tocar nela, para sempre, mesmo
//! depois de o afastamento da B.
//!
//! A mesma identidade nunca pode estar ACTIVA em duas organizações ao mesmo
//! tempo (R25), e `add_employee` recusa um email de domínio diferente do da
//! organização — por isso este teste insere a pertença arquivada
//! directamente na base (como os e2e já fazem com `archive_member`), em vez
//! de tentar reproduzir esse estado pela BFF: o que importa provar é que o
//! CÓDIGO nunca devia ter confiado só na pertença (activa ou arquivada) do
//! dono para decidir a quem pertence a reunião — e com o `org_id` explícito
//! da migração 0095 deixa de confiar.
//!
//! Antes da correcção, as quatro chamadas abaixo (GET, PATCH, DELETE, ring) e
//! a listagem/notas de `apikeys.rs` deviam falhar (o que ESTE teste prova
//! primeiro) e não falhavam — a reunião saía inteira à chave da B.
mod common;

use common::TestApp;
use serde_json::json;

fn auth_header(key: &str) -> (&'static str, String) {
    ("Authorization", format!("Bearer {key}"))
}

#[sqlx::test(migrations = "./migrations")]
async fn chave_de_outra_org_nao_alcanca_reuniao_de_dono_com_pertenca_arquivada(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;

    // Duas organizações reais, cada uma com a sua chave `dlx_` (escopos
    // completos — `api_key()` não passa `scopes`, que por omissão é o
    // catálogo inteiro).
    let a = app.new_org("alfa-t1.test").await;
    let b = app.new_org("beta-t1.test").await;
    let (_, key_a) = app.api_key(&a).await;
    let (_, key_b) = app.api_key(&b).await;

    // Carlos: membro ACTIVO da A (anfitrião legítimo de reuniões da A) e,
    // independentemente disso, uma linha ARQUIVADA em `org_members` para a
    // B — o estado que a auditoria descreve ("foi membro da B, já
    // arquivado"). Inserida directamente: nem a BFF nem a v1 oferecem um
    // caminho para o mesmo email pertencer a duas organizações de domínios
    // diferentes, e não é isso que está em causa — está em causa o que o
    // CÓDIGO faz quando a linha já existe, seja como for que lá chegou.
    let carlos = app.add_member(&a, "carlos", "member").await;
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, role, archived_at)
         VALUES ($1::uuid, $2::uuid, 'member', now())
         ON CONFLICT (org_id, user_id) DO UPDATE SET archived_at = now()",
    )
    .bind(b.org())
    .bind(&carlos.user_id)
    .execute(&app.db)
    .await
    .unwrap();

    // A chave da A cria a reunião — claramente um recurso da organização A,
    // com Carlos como anfitrião.
    let starts = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
    let created = app
        .raw(
            reqwest::Method::POST,
            "/api/v1/meetings",
            &[auth_header(&key_a)],
            Some(json!({
                "title": "reunião da A",
                "starts_at": starts,
                "host_email": carlos.email,
            })),
        )
        .await;
    assert_eq!(created.status, 200, "{}", created.text);
    let meeting_id = created.json()["id"].as_str().unwrap().to_string();

    // Controlo positivo: a própria A lê a sua reunião.
    let get_a = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/v1/meetings/{meeting_id}"),
            &[auth_header(&key_a)],
            None,
        )
        .await;
    assert_eq!(get_a.status, 200, "controlo positivo falhou: {}", get_a.text);

    // O DEFEITO: a chave da B — que nunca criou, nem é dona, desta reunião —
    // só porque Carlos tem (arquivada) uma linha em `org_members` para a B.
    let get_b = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/v1/meetings/{meeting_id}"),
            &[auth_header(&key_b)],
            None,
        )
        .await;
    assert_eq!(
        get_b.status, 404,
        "GET: a chave da B leu uma reunião da A (pertença arquivada bastou): {}",
        get_b.text
    );

    let minutes_b = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/v1/meetings/{meeting_id}/minutes"),
            &[auth_header(&key_b)],
            None,
        )
        .await;
    assert_eq!(
        minutes_b.status, 404,
        "minutes: a chave da B leu a ata/transcrição de uma reunião da A: {}",
        minutes_b.text
    );

    let patch_b = app
        .raw(
            reqwest::Method::PATCH,
            &format!("/api/v1/meetings/{meeting_id}"),
            &[auth_header(&key_b)],
            Some(json!({"title": "sequestrada pela B"})),
        )
        .await;
    assert_eq!(
        patch_b.status, 404,
        "PATCH: a chave da B alterou uma reunião da A: {}",
        patch_b.text
    );

    let ring_b = app
        .raw(
            reqwest::Method::POST,
            &format!("/api/v1/meetings/{meeting_id}/ring"),
            &[auth_header(&key_b)],
            None,
        )
        .await;
    assert_eq!(
        ring_b.status, 404,
        "ring: a chave da B tocou numa reunião da A: {}",
        ring_b.text
    );

    // A listagem (`GET /api/v1/meetings`) da B também não pode trazer a
    // reunião da A — mesma fonte de verdade (`m.org_id`), outro caminho de
    // código (`apikeys::v1_meetings`).
    let list_b = app
        .raw(
            reqwest::Method::GET,
            "/api/v1/meetings",
            &[auth_header(&key_b)],
            None,
        )
        .await;
    assert_eq!(list_b.status, 200, "{}", list_b.text);
    let ids: Vec<&str> = list_b.json()["meetings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(
        !ids.contains(&meeting_id.as_str()),
        "listagem: a reunião da A apareceu na listagem da B: {ids:?}"
    );

    // E o recurso da A tem de sobreviver: o DELETE de cima é 404, não um
    // apagar silencioso.
    let get_a_ainda = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/v1/meetings/{meeting_id}"),
            &[auth_header(&key_a)],
            None,
        )
        .await;
    assert_eq!(
        get_a_ainda.status, 200,
        "a reunião da A desapareceu depois dos pedidos da B: {}",
        get_a_ainda.text
    );

    // DELETE por fim (depois do resto, para não destruir o fixture a meio).
    let delete_b = app
        .raw(
            reqwest::Method::DELETE,
            &format!("/api/v1/meetings/{meeting_id}"),
            &[auth_header(&key_b)],
            None,
        )
        .await;
    assert_eq!(
        delete_b.status, 404,
        "DELETE: a chave da B apagou uma reunião da A: {}",
        delete_b.text
    );
    let get_a_final = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/v1/meetings/{meeting_id}"),
            &[auth_header(&key_a)],
            None,
        )
        .await;
    assert_eq!(
        get_a_final.status, 200,
        "a reunião da A não sobreviveu ao DELETE recusado da B: {}",
        get_a_final.text
    );
}

/// O caso simples que a auditoria também descreve, sem pertença alguma na
/// B: a chave de uma organização que nunca teve nada a ver com o dono não
/// alcança a reunião. Controlo de que a correcção não ficou demasiado
/// larga (ex.: `org_id IS NULL` a abrir para toda a gente).
#[sqlx::test(migrations = "./migrations")]
async fn chave_de_org_sem_qualquer_relacao_nao_alcanca_a_reuniao(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa-t1b.test").await;
    let b = app.new_org("beta-t1b.test").await;
    let (_, key_a) = app.api_key(&a).await;
    let (_, key_b) = app.api_key(&b).await;

    let starts = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
    let created = app
        .raw(
            reqwest::Method::POST,
            "/api/v1/meetings",
            &[auth_header(&key_a)],
            Some(json!({"title": "reunião só da A", "starts_at": starts, "host_email": a.email})),
        )
        .await;
    assert_eq!(created.status, 200, "{}", created.text);
    let meeting_id = created.json()["id"].as_str().unwrap().to_string();

    let get_b = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/v1/meetings/{meeting_id}"),
            &[auth_header(&key_b)],
            None,
        )
        .await;
    assert_eq!(get_b.status, 404, "{}", get_b.text);
}
