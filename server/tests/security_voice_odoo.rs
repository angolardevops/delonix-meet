//! Ataques à telefonia (dial-in PSTN) e à integração Odoo, contra Postgres
//! real. Cada recusa vem com o controlo positivo (R51/R94): o mesmo pedido
//! funciona para quem tem direito — senão o teste mediria uma avaria.
mod common;

use common::{assert_denied, Account, TestApp, PASSWORD};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use uuid::Uuid;

const VOICE_SECRET: &str = "segredo-da-media-0123456789";

/// DID dedicado de uma org, semeado por SQL (como `tests/grpc.rs`): o caminho
/// HTTP de criação de DIDs é ele próprio objecto de teste abaixo.
async fn seed_did(app: &TestApp, org: &str, e164: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO voice_did (org_id, e164) VALUES ($1::uuid, $2) RETURNING id")
        .bind(org)
        .bind(e164)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

async fn ivr_validate(app: &TestApp, did: &str, pin: &str) -> (u16, Value) {
    let r = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/voice/ivr/validate",
            &[("x-voice-secret", VOICE_SECRET)],
            Some(json!({"did_e164": did, "pin": pin})),
        )
        .await;
    (r.status, r.json())
}

/// Cria a sala de voz pelo caminho da org de `who` (a sala de voz vive debaixo
/// da organização).
async fn voice_room(app: &TestApp, who: &Account, code: &str) -> (u16, Value) {
    voice_room_in(app, who, who.org(), code).await
}

async fn voice_room_in(app: &TestApp, who: &Account, org: &str, code: &str) -> (u16, Value) {
    app.post(
        &format!("/api/orgs/{org}/voice/rooms"),
        Some(&who.token),
        json!({ "room_code": code }),
    )
    .await
}

/// R140 — o admin da org A ligava um DID+PIN da SUA org ao código de sala da
/// org B: o IVR punha então chamadores PSTN dentro da reunião de B.
#[sqlx::test(migrations = "./migrations")]
async fn voice_room_for_another_orgs_room_code_is_refused(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("VOICE_INTERNAL_SECRET", VOICE_SECRET)]).await;
    let a = app.new_org("alfa-voz.ao").await;
    let b = app.new_org("beta-voz.ao").await;
    seed_did(&app, a.org(), "+244222100001").await;
    seed_did(&app, b.org(), "+244222200002").await;
    let room_b = app.new_room(&b, "Conselho da B").await;
    let code_b = room_b["code"].as_str().unwrap();

    // Controlo positivo: B liga o dial-in à sua própria sala, e o IVR encontra-a.
    let (st, own) = voice_room(&app, &b, code_b).await;
    assert_eq!(st, 200, "{own}");
    let (st, ivr) = ivr_validate(
        &app,
        own["dial_in_number"].as_str().unwrap(),
        own["pin"].as_str().unwrap(),
    )
    .await;
    assert_eq!(st, 200, "{ivr}");
    assert_eq!(ivr["room_code"], code_b);

    // O ataque: A pede um dial-in para o código da sala de B.
    let (st, attack) = voice_room(&app, &a, code_b).await;
    assert_denied("A cria dial-in para a sala de B", st, &attack, "pin");
    assert_eq!(st, 404, "não revela que a sala existe: {attack}");
    assert_eq!(attack["code"], "voice.room_not_found", "{attack}");

    // A recusa é a mesma de um código inexistente (não distingue).
    let (st, missing) = voice_room(&app, &a, "nao-existe-nenhuma").await;
    assert_eq!((st, &missing["code"]), (404, &attack["code"]), "{missing}");

    // Pelo caminho da org B (de que A não é membro): 404 antes de tudo.
    let (st, body) = voice_room_in(&app, &a, b.org(), code_b).await;
    assert_denied("A cria dial-in pelo caminho da org B", st, &body, "pin");
    assert_eq!(st, 404, "{body}");

    // Leitura da sala de voz da B: B lê; A não, nem pelo caminho da B nem
    // pelo seu com o id da B.
    let own_id = own["id"].as_str().unwrap();
    for suffix in ["", "/participants"] {
        let (st, body) = app
            .get(
                &format!("/api/orgs/{}/voice/rooms/{own_id}{suffix}", b.org()),
                Some(&b.token),
            )
            .await;
        assert_eq!(st, 200, "B lê a sua sala de voz{suffix}: {body}");
        for org in [b.org(), a.org()] {
            let (st, body) = app
                .get(
                    &format!("/api/orgs/{org}/voice/rooms/{own_id}{suffix}"),
                    Some(&a.token),
                )
                .await;
            assert_eq!(st, 404, "A lê a sala de voz da B{suffix} via {org}: {body}");
            assert_denied("sala de voz da B", st, &body, "pin");
        }
    }

    // E nada ficou gravado para A.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_room WHERE org_id = $1::uuid")
        .bind(a.org())
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0, "a org A ficou com uma sala de voz para a sala de B");

    // Controlo positivo do lado de A: a sua própria sala continua a funcionar.
    let room_a = app.new_room(&a, "Sala da A").await;
    let (st, own_a) = voice_room(&app, &a, room_a["code"].as_str().unwrap()).await;
    assert_eq!(st, 200, "{own_a}");
}

/// R141 — qualquer membro encerrava a sala de voz de outro colega.
#[sqlx::test(migrations = "./migrations")]
async fn voice_room_close_requires_creator_or_org_admin(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("VOICE_INTERNAL_SECRET", VOICE_SECRET)]).await;
    let admin = app.new_org("gama-voz.ao").await;
    let creator = app.add_member(&admin, "criadora", "member").await;
    let other = app.add_member(&admin, "outro", "member").await;
    seed_did(&app, admin.org(), "+244222300003").await;

    let room = app.new_room(&creator, "Sala da criadora").await;
    let code = room["code"].as_str().unwrap();
    let (st, vr1) = voice_room(&app, &creator, code).await;
    assert_eq!(st, 200, "{vr1}");
    let id1 = vr1["id"].as_str().unwrap();

    // O ataque: um colega que não criou a sala nem é admin.
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/voice/rooms/{id1}/close", admin.org()),
            Some(&other.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 403, "um membro qualquer encerrou a sala de voz: {body}");
    assert_eq!(body["code"], "voice.room_close_forbidden", "{body}");
    let status: String = sqlx::query_scalar("SELECT status FROM voice_room WHERE id = $1::uuid")
        .bind(id1)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(status, "active", "a recusa não pode ter encerrado a sala");

    // Controlo positivo 1: a criadora encerra a sua (204, sem corpo).
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/voice/rooms/{id1}/close", admin.org()),
            Some(&creator.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 204, "{body}");

    // Controlo positivo 2: o admin da org encerra a de outra pessoa.
    let (st, vr2) = voice_room(&app, &creator, code).await;
    assert_eq!(st, 200, "{vr2}");
    let id2 = vr2["id"].as_str().unwrap();
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/voice/rooms/{id2}/close", admin.org()),
            Some(&admin.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 204, "{body}");

    // Outra org continua a receber 404 (não revela a existência): pelo
    // caminho da org dona e pelo caminho da sua própria org.
    let foreign = app.new_org("delta-voz.ao").await;
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/voice/rooms/{id2}/close", foreign.org()),
            Some(&foreign.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404, "{body}");
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/voice/rooms/{id2}/close", admin.org()),
            Some(&foreign.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 404, "{body}");
}

/// R141 — um admin de org punha números no pool PARTILHADO (`org_id NULL`),
/// que todas as organizações passam a usar no dial-in.
#[sqlx::test(migrations = "./migrations")]
async fn shared_did_pool_requires_platform_admin(db: sqlx::PgPool) {
    // O administrador da plataforma é declarado por UUID no arranque; a conta
    // só nasce depois. Regista-se com um servidor, e o teste corre num segundo
    // servidor, sobre a mesma base, que já a declara. O primeiro larga a vaga
    // do semáforo antes de o segundo a pedir.
    let boot = TestApp::spawn(db.clone()).await;
    let operator = boot.new_org("operador-voz.ao").await;
    drop(boot);
    let app = TestApp::spawn_with(db, &[("PLATFORM_ADMIN_USER_IDS", &operator.user_id)]).await;
    let tenant = app.new_org("epsilon-voz.ao").await;
    let dids = format!("/api/orgs/{}/voice/dids", tenant.org());

    // O ataque: admin de org, modelo partilhado, sem `org_scoped` → pool.
    let (st, body) = app
        .post(&dids, Some(&tenant.token), json!({"e164": "+244222400004"}))
        .await;
    assert_eq!(
        st, 403,
        "um admin de org escreveu no pool partilhado: {body}"
    );
    assert_eq!(
        body["code"], "voice.shared_did_requires_platform_admin",
        "{body}"
    );
    let (st, body) = app
        .post(
            &dids,
            Some(&tenant.token),
            json!({"e164": "+244222400005", "model": "shared", "org_scoped": false}),
        )
        .await;
    assert_eq!(st, 403, "{body}");
    let pool: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_did WHERE org_id IS NULL")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(pool, 0, "a recusa não pode ter gravado no pool");

    // Controlo positivo: o admin de org cria DIDs da SUA org.
    let (st, body) = app
        .post(
            &dids,
            Some(&tenant.token),
            json!({"e164": "+244222400006", "org_scoped": true}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["org_id"], tenant.org());
    let (st, body) = app
        .post(
            &dids,
            Some(&tenant.token),
            json!({"e164": "+244222400007", "model": "dedicated"}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["org_id"], tenant.org());

    // Controlo positivo: o administrador da plataforma escreve no pool.
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/voice/dids", operator.org()),
            Some(&operator.token),
            json!({"e164": "+244222400008"}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["org_id"], Value::Null, "{body}");
}

/// R142 — a chave de API do inquilino (`dlx_`) abria as rotas da integração
/// Odoo, que são do token `dlxo_`.
#[sqlx::test(migrations = "./migrations")]
async fn odoo_integration_routes_refuse_tenant_api_key(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("zeta-odoo.ao").await;
    let (_, dlx) = app.api_key(&admin).await;
    assert!(dlx.starts_with("dlx_"), "{dlx}");

    // A chave está boa: abre a SUA superfície (controlo positivo da chave).
    let (st, body) = app.get("/api/v1/organization", Some(&dlx)).await;
    assert_eq!(st, 200, "{body}");

    // O ataque: a mesma chave nas rotas da integração.
    let (st, body) = app.get("/api/integrations/odoo/v1/users", Some(&dlx)).await;
    assert_denied("dlx_ lista o directório Odoo", st, &body, &admin.email);
    assert_eq!(st, 401, "{body}");
    let r = app
        .raw(
            reqwest::Method::GET,
            "/api/integrations/odoo/v1/users",
            &[("x-integration-token", &dlx)],
            None,
        )
        .await;
    assert_eq!(r.status, 401, "{}", r.text);
    let (st, body) = app
        .post(
            "/api/integrations/odoo/v1/provision",
            Some(&dlx),
            json!({"company": "Capturada", "admin_email": admin.email, "users": []}),
        )
        .await;
    assert_eq!(st, 401, "dlx_ provisiona pelo caminho do Odoo: {body}");

    // Controlo positivo: o token de integração `dlxo_` abre as duas.
    let (st, tok) = app
        .post(
            &format!("/api/orgs/{}/integrations/odoo/rotate-token", admin.org()),
            Some(&admin.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{tok}");
    let dlxo = tok["token"].as_str().unwrap();
    let (st, body) = app.get("/api/integrations/odoo/v1/users", Some(dlxo)).await;
    assert_eq!(st, 200, "{body}");
    assert!(body.to_string().contains(&admin.email), "{body}");
    let (st, body) = app
        .post(
            "/api/integrations/odoo/v1/provision",
            Some(dlxo),
            json!({"company": "Org zeta-odoo.ao", "admin_email": admin.email, "users": []}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
}

/// R143 — `GET /api/integrations/odoo/v1/users` devolvia ao Odoo membros
/// ARQUIVADOS (saídos da empresa) como se ainda lá estivessem.
#[sqlx::test(migrations = "./migrations")]
async fn odoo_list_users_excludes_archived_members(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let admin = app.new_org("eta-odoo.ao").await;
    let gone = app.add_member(&admin, "saiu", "member").await;
    let stays = app.add_member(&admin, "fica", "member").await;
    let (st, tok) = app
        .post(
            &format!("/api/orgs/{}/integrations/odoo/rotate-token", admin.org()),
            Some(&admin.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{tok}");
    let dlxo = tok["token"].as_str().unwrap();

    // Controlo positivo: antes de sair, está na lista.
    let (st, body) = app.get("/api/integrations/odoo/v1/users", Some(dlxo)).await;
    assert_eq!(st, 200, "{body}");
    assert!(body.to_string().contains(&gone.email), "{body}");

    app.archive_member(admin.org(), &gone.user_id).await;

    let (st, body) = app.get("/api/integrations/odoo/v1/users", Some(dlxo)).await;
    assert_eq!(st, 200, "{body}");
    let text = body.to_string();
    assert!(
        !text.contains(&gone.email),
        "o membro arquivado continua no directório: {body}"
    );
    assert!(text.contains(&stays.email), "{body}");
    assert!(text.contains(&admin.email), "{body}");
}

// ---------------------------------------------------------------------------
//  Sincronização do directório: fallback sem telefone (Odoo sem módulo `hr`)
// ---------------------------------------------------------------------------

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Um «Odoo» falso que autentica QUALQUER credencial e conta as chamadas
/// `call_kw`: a primeira pede `mobile_phone`/`work_phone` e é recusada (como
/// um Odoo sem o módulo `hr`); a segunda, sem esses campos, tem sucesso — é o
/// fallback de `odoo_sso::search_users`.
async fn odoo_falso_sem_modulo_hr() -> (u16, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let call_kw_count = Arc::new(AtomicUsize::new(0));
    let conta = call_kw_count.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let conta = conta.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let (head_end, content_length) = loop {
                    let n = match s.read(&mut tmp).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..pos]);
                        let cl = head
                            .lines()
                            .find_map(|l| {
                                let lower = l.to_ascii_lowercase();
                                lower.strip_prefix("content-length:").map(|v| {
                                    v.trim().parse::<usize>().unwrap_or(0)
                                })
                            })
                            .unwrap_or(0);
                        break (pos + 4, cl);
                    }
                };
                while buf.len() < head_end + content_length {
                    match s.read(&mut tmp).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let head_text = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let path = head_text
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("")
                    .to_string();
                let body_text =
                    String::from_utf8_lossy(&buf[head_end..head_end + content_length]).to_string();

                let corpo = if path == "/web/session/authenticate" {
                    r#"{"jsonrpc":"2.0","id":1,"result":{"uid":1,"company_id":1,"name":"Carla",
                        "is_admin":true,
                        "user_companies":{"allowed_companies":{"1":{"name":"Alfa"}}}}}"#
                        .to_string()
                } else if path == "/web/dataset/call_kw" {
                    conta.fetch_add(1, Ordering::SeqCst);
                    if body_text.contains("mobile_phone") {
                        r#"{"jsonrpc":"2.0","id":1,"error":{"code":200,"message":"Invalid field mobile_phone in res.users"}}"#.to_string()
                    } else {
                        r#"{"jsonrpc":"2.0","id":1,"result":[{"id":7,"login":"carla@alfa.test","name":"Carla","email":"carla@alfa.test"}]}"#.to_string()
                    }
                } else {
                    "{}".to_string()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nset-cookie: session_id=fake-session\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{corpo}",
                    corpo.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    (port, call_kw_count)
}

/// Aponta a org de `who` para o Odoo em `url`, como `egress_guard::apontar_odoo`.
async fn apontar_odoo(app: &TestApp, org: &str, user_id: &str, url: &str) {
    sqlx::query(
        "UPDATE organizations SET odoo_enabled = TRUE, odoo_url = $1, odoo_db = 'prod' WHERE id = $2::uuid",
    )
    .bind(url)
    .bind(org)
    .execute(&app.db)
    .await
    .unwrap();
    sqlx::query("UPDATE users SET odoo_org_id = $1::uuid WHERE id = $2::uuid")
        .bind(org)
        .bind(user_id)
        .execute(&app.db)
        .await
        .unwrap();
}

/// Um Odoo sem o módulo `hr` recusa `mobile_phone`/`work_phone` em
/// `res.users` — `search_users` tem de repetir sem esses campos em vez de
/// deixar a sincronização do directório inteira por fazer.
#[sqlx::test(migrations = "./migrations")]
async fn directory_sync_falls_back_without_phone_fields_when_odoo_refuses(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, &[("OUTBOUND_ALLOW_HOSTS", "127.0.0.1")]).await;
    let a = app.new_org("hr-odoo.ao").await;
    let (port, call_kw_count) = odoo_falso_sem_modulo_hr().await;
    apontar_odoo(&app, a.org(), &a.user_id, &format!("http://127.0.0.1:{port}")).await;

    let (st, _) = app
        .post(
            "/api/auth/login",
            None,
            json!({"email": a.email, "password": PASSWORD}),
        )
        .await;
    assert_eq!(st, 200, "login por Odoo tinha de ter sucesso");

    // `spawn_directory_sync` corre em segundo plano: espera as DUAS tentativas
    // de `call_kw` (com telefones, recusada; sem eles, aceite).
    for _ in 0..50 {
        if call_kw_count.load(Ordering::SeqCst) >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(
        call_kw_count.load(Ordering::SeqCst),
        2,
        "search_users tinha de tentar com telefones e depois sem eles"
    );
}
