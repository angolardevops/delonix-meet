//! R277 — a verificação do PIN do ramal, endurecida antes de o IVR a usar:
//! travão por ORIGEM da chamada, bloqueio de duração crescente e contador com
//! janela. Contra Postgres real.
//!
//! O que faz de IVR é o próprio teste, com o segredo de voz. A origem é o que
//! o `dialin_ivr.lua` lê ao FreeSWITCH: `caller_id_number` e `sip_network_ip`.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

const VOICE_SECRET: &str = "segredo-da-media-0123456789";
const REDE: &str = "10.9.0.1";

async fn spawn(db: sqlx::PgPool) -> TestApp {
    TestApp::spawn_with(db, &[("VOICE_INTERNAL_SECRET", VOICE_SECRET)]).await
}

async fn ramal_com_pin(app: &TestApp, admin: &Account, quem: &Account, number: &str) -> String {
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/extensions", admin.org()),
            Some(&admin.token),
            json!({"extension": number, "member_id": quem.user_id}),
        )
        .await;
    assert_eq!(st, 200, "criar ramal {number}: {body}");
    // O PIN é da pessoa: só ela o gera, na sua área.
    let id: String = body["id"].as_str().unwrap().into();
    let pin = gerar(app, quem).await;
    assert!(!id.is_empty());
    pin
}

async fn gerar(app: &TestApp, quem: &Account) -> String {
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/my-extension/regenerate-pin", quem.org()),
            Some(&quem.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    body["pin"].as_str().unwrap().to_string()
}

async fn dominio(app: &TestApp, org: &str) -> String {
    let slug: String = sqlx::query_scalar("SELECT slug FROM organizations WHERE id = $1::uuid")
        .bind(org)
        .fetch_one(&app.db)
        .await
        .unwrap();
    format!("{slug}.{}", app.state.config.voice_ramais_domain_suffix)
}

async fn verificar(app: &TestApp, domain: &str, ext: &str, pin: &str, numero: &str) -> Value {
    let r = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/voice/ivr/verify-extension-pin",
            &[("x-voice-secret", VOICE_SECRET)],
            Some(json!({
                "domain": domain, "extension": ext, "pin": pin,
                "origin": {"caller_number": numero, "network_ip": REDE},
            })),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    r.json()
}

/// Um PIN de seis dígitos que NÃO é `certo` e que não cai em nenhuma recusa.
fn errado(certo: &str) -> &'static str {
    if certo == "584930" {
        "730518"
    } else {
        "584930"
    }
}

/// `(falhas, nível, bloqueado agora)` do contador de um ramal.
async fn contador(app: &TestApp, org: &str, ext: &str) -> (i32, i32, bool) {
    sqlx::query_as(
        "SELECT pin_failed_attempts, pin_lock_level, COALESCE(pin_locked_until > now(), false)
           FROM voice_extensions WHERE org_id = $1::uuid AND extension = $2",
    )
    .bind(org)
    .bind(ext)
    .fetch_one(&app.db)
    .await
    .unwrap()
}

async fn auditoria(app: &TestApp, org: &str, action: &str) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT actor_id::text, target FROM audit_logs
          WHERE org_id = $1::uuid AND action = $2 ORDER BY seq",
    )
    .bind(org)
    .bind(action)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

/// O caso da negação de serviço (R276 (i)): a mesma origem a falhar em
/// vários ramais é travada, e NENHUM desses ramais fica bloqueado — nem para
/// a origem que falhou, nem para o dono.
#[sqlx::test(migrations = "./migrations")]
async fn a_mesma_origem_em_varios_ramais_e_travada_e_nenhum_ramal_bloqueia(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-origem.ao").await;
    let dom = dominio(&app, a.org()).await;
    let mut pins = Vec::new();
    for (i, nome) in ["ana", "beto", "carla", "dino"].iter().enumerate() {
        let quem = app.add_member(&a, nome, "member").await;
        let numero = format!("100{i}");
        let pin = ramal_com_pin(&app, &a, &quem, &numero).await;
        pins.push((numero, pin));
    }
    let atacante = "+244930000999";

    // Três falhas, cada uma num ramal diferente: a terceira trava a origem.
    for (ext, pin) in &pins[..3] {
        let r = verificar(&app, &dom, ext, errado(pin), atacante).await;
        assert_eq!(r["reason"], "invalid", "{ext}: {r}");
    }
    // Travada: nem o quarto ramal nem um PIN CERTO chegam a ser verificados.
    let (ext4, pin4) = &pins[3];
    let r = verificar(&app, &dom, ext4, errado(pin4), atacante).await;
    assert_eq!(r["reason"], "origin_locked", "{r}");
    assert!(r["retry_after_secs"].as_i64().unwrap() > 0, "{r}");
    let r = verificar(&app, &dom, ext4, pin4, atacante).await;
    assert_eq!(
        r["reason"], "origin_locked",
        "o PIN certo passou com a origem travada: {r}"
    );
    // E martelar um só ramal também não o bloqueia.
    for _ in 0..10 {
        let r = verificar(&app, &dom, "1000", errado(&pins[0].1), atacante).await;
        assert_eq!(r["reason"], "origin_locked", "{r}");
    }

    // Os ramais: no máximo uma falha cada, nenhum bloqueado, o quarto intacto.
    for (ext, _) in &pins[..3] {
        assert_eq!(contador(&app, a.org(), ext).await, (1, 0, false), "{ext}");
    }
    assert_eq!(contador(&app, a.org(), ext4).await, (0, 0, false));
    // Os donos continuam a identificar-se — de outra origem.
    for (i, (ext, pin)) in pins.iter().enumerate() {
        let r = verificar(&app, &dom, ext, pin, &format!("+24492300000{i}")).await;
        assert_eq!(r["valid"], true, "{ext}: {r}");
    }

    // A trilha: cada falha diz de onde veio, com o actor de sistema, e a
    // origem travada fica registada uma vez.
    let falhas = auditoria(&app, a.org(), "ramal.pin_falhado").await;
    assert_eq!(falhas.len(), 3, "{falhas:?}");
    for (actor, alvo) in &falhas {
        assert_eq!(actor, "00000000-0000-0000-0000-000000000000");
        assert!(
            alvo.contains(&format!("origem {atacante} via {REDE}")),
            "{alvo}"
        );
    }
    let travadas = auditoria(&app, a.org(), "ramal.origem_travada").await;
    assert_eq!(travadas.len(), 1, "{travadas:?}");
    assert_eq!(travadas[0].0, "00000000-0000-0000-0000-000000000000");
    assert!(travadas[0].1.contains(atacante), "{travadas:?}");
    assert_eq!(
        auditoria(&app, a.org(), "ramal.pin_bloqueado").await.len(),
        0
    );
    // O PIN tentado não está em lado nenhum.
    for (_, alvo) in falhas.iter().chain(&travadas) {
        for (_, pin) in &pins {
            assert!(
                !alvo.contains(errado(pin)) && !alvo.contains(pin.as_str()),
                "{alvo}"
            );
        }
    }
}

/// Dez palpites em paralelo da MESMA origem: a origem é cobrada antes de
/// verificar, por isso só três chegam ao ramal — e o ramal não bloqueia.
#[sqlx::test(migrations = "./migrations")]
async fn dez_em_paralelo_da_mesma_origem_so_tres_chegam_ao_ramal(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-origem-paralelo.ao").await;
    let quem = app.add_member(&a, "colega", "member").await;
    let pin = ramal_com_pin(&app, &a, &quem, "1004").await;
    let dom = dominio(&app, a.org()).await;
    let mau = errado(&pin);

    let respostas = futures_util::future::join_all(
        (0..10).map(|_| verificar(&app, &dom, "1004", mau, "+244930000777")),
    )
    .await;
    let por = |razao: &str| respostas.iter().filter(|r| r["reason"] == razao).count();
    assert_eq!(por("invalid"), 3, "{respostas:?}");
    assert_eq!(por("origin_locked"), 7, "{respostas:?}");
    assert_eq!(contador(&app, a.org(), "1004").await, (3, 0, false));
    assert_eq!(
        verificar(&app, &dom, "1004", &pin, "+244923000123").await["valid"],
        true
    );
}

/// Um acerto devolve à origem a falha desse pedido, e só essa: «dois
/// palpites, um acerto» não deixa ninguém adivinhar para sempre.
#[sqlx::test(migrations = "./migrations")]
async fn um_acerto_nao_zera_a_origem(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-origem-acerto.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let beto = app.add_member(&a, "beto", "member").await;
    let pin_ana = ramal_com_pin(&app, &a, &ana, "1001").await;
    let pin_beto = ramal_com_pin(&app, &a, &beto, "1002").await;
    let dom = dominio(&app, a.org()).await;
    let quem = "+244930000555";

    for _ in 0..2 {
        let r = verificar(&app, &dom, "1002", errado(&pin_beto), quem).await;
        assert_eq!(r["reason"], "invalid");
    }
    assert_eq!(
        verificar(&app, &dom, "1001", &pin_ana, quem).await["valid"],
        true
    );
    // O acerto não apagou as duas falhas: a seguinte trava.
    let r = verificar(&app, &dom, "1002", errado(&pin_beto), quem).await;
    assert_eq!(r["reason"], "invalid", "{r}");
    let r = verificar(&app, &dom, "1001", &pin_ana, quem).await;
    assert_eq!(r["reason"], "origin_locked", "{r}");
    // O acerto que cai em cima do bloqueio que ele próprio causou desfá-lo.
    let outro = "+244930000556";
    for _ in 0..2 {
        verificar(&app, &dom, "1002", errado(&pin_beto), outro).await;
    }
    assert_eq!(
        verificar(&app, &dom, "1001", &pin_ana, outro).await["valid"],
        true
    );
    let (falhas, travada): (i32, bool) = sqlx::query_as(
        "SELECT failures, COALESCE(locked_until > now(), false) FROM voice_pin_origins
          WHERE origin = $1",
    )
    .bind(format!("{REDE}|{outro}"))
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!((falhas, travada), (2, false));
}

/// O bloqueio do RAMAL cresce a cada reincidência e esquece-se ao fim de um
/// dia sem bloqueios. As falhas vêm de origens sempre novas: é o caso de quem
/// ataca de muitas origens, que só o contador do ramal trava.
#[sqlx::test(migrations = "./migrations")]
async fn o_bloqueio_do_ramal_cresce_e_esquece_se(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-origem-cresce.ao").await;
    let quem = app.add_member(&a, "colega", "member").await;
    let pin = ramal_com_pin(&app, &a, &quem, "1004").await;
    let dom = dominio(&app, a.org()).await;
    let mau = errado(&pin);
    let mut n = 0u32;
    let mut cinco_falhas = || {
        let nums: Vec<String> = (0..5)
            .map(|_| {
                n += 1;
                format!("+24494{n:07}")
            })
            .collect();
        nums
    };
    let acabou = |intervalo: &'static str| {
        let db = app.db.clone();
        async move {
            sqlx::query(&format!(
                "UPDATE voice_extensions SET pin_locked_until = now() - interval '{intervalo}'
                  WHERE extension = '1004'"
            ))
            .execute(&db)
            .await
            .unwrap();
        }
    };

    for (ronda, esperado) in [(1, 900), (2, 1800), (3, 3600)] {
        let nums = cinco_falhas();
        for num in &nums[..4] {
            assert_eq!(
                verificar(&app, &dom, "1004", mau, num).await["reason"],
                "invalid",
                "ronda {ronda}"
            );
        }
        let r = verificar(&app, &dom, "1004", mau, &nums[4]).await;
        assert_eq!(r["reason"], "locked", "ronda {ronda}: {r}");
        assert_eq!(r["retry_after_secs"], esperado, "ronda {ronda}: {r}");
        assert_eq!(contador(&app, a.org(), "1004").await, (0, ronda, true));
        acabou("1 second").await;
    }
    // Um dia depois do último bloqueio, sem outro, recomeça nos 15 minutos.
    acabou("25 hours").await;
    let nums = cinco_falhas();
    for num in &nums[..4] {
        verificar(&app, &dom, "1004", mau, num).await;
    }
    let r = verificar(&app, &dom, "1004", mau, &nums[4]).await;
    assert_eq!(r["retry_after_secs"], 900, "{r}");
    // A trilha diz o nível do bloqueio.
    let bloqueios = auditoria(&app, a.org(), "ramal.pin_bloqueado").await;
    assert_eq!(bloqueios.len(), 4, "{bloqueios:?}");
    assert!(
        bloqueios[1].1.contains("30 min (bloqueio n.º 2)"),
        "{bloqueios:?}"
    );
    assert!(
        bloqueios[2].1.contains("60 min (bloqueio n.º 3)"),
        "{bloqueios:?}"
    );
    assert!(
        bloqueios[3].1.contains("15 min (bloqueio n.º 1)"),
        "{bloqueios:?}"
    );
    // Um acerto zera o nível.
    acabou("1 second").await;
    assert_eq!(
        verificar(&app, &dom, "1004", &pin, "+244923000999").await["valid"],
        true
    );
    assert_eq!(contador(&app, a.org(), "1004").await, (0, 0, false));
}

/// O contador tem janela (R276 (iii)): quatro falhas de há mais de quinze
/// minutos e uma de agora não bloqueiam. O mesmo para a origem.
#[sqlx::test(migrations = "./migrations")]
async fn a_janela_expira(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-origem-janela.ao").await;
    let quem = app.add_member(&a, "colega", "member").await;
    let pin = ramal_com_pin(&app, &a, &quem, "1004").await;
    let dom = dominio(&app, a.org()).await;
    let mau = errado(&pin);

    for i in 0..4 {
        let num = format!("+24495000000{i}");
        assert_eq!(
            verificar(&app, &dom, "1004", mau, &num).await["reason"],
            "invalid"
        );
    }
    assert_eq!(contador(&app, a.org(), "1004").await, (4, 0, false));
    sqlx::query(
        "UPDATE voice_extensions SET pin_failure_window_at = now() - interval '16 minutes'
          WHERE extension = '1004'",
    )
    .execute(&app.db)
    .await
    .unwrap();
    let r = verificar(&app, &dom, "1004", mau, "+244950000009").await;
    assert_eq!(r["reason"], "invalid", "as falhas velhas contaram: {r}");
    assert_eq!(contador(&app, a.org(), "1004").await, (1, 0, false));
    let falhas = auditoria(&app, a.org(), "ramal.pin_falhado").await;
    assert!(
        falhas.last().unwrap().1.contains("tentativa 1 de 5"),
        "{falhas:?}"
    );

    // A origem: duas falhas velhas não se juntam à de agora.
    let quem_liga = "+244930000444";
    for _ in 0..2 {
        verificar(&app, &dom, "1004", mau, quem_liga).await;
    }
    sqlx::query(
        "UPDATE voice_pin_origins SET window_started_at = now() - interval '16 minutes'
          WHERE origin = $1",
    )
    .bind(format!("{REDE}|{quem_liga}"))
    .execute(&app.db)
    .await
    .unwrap();
    assert_eq!(
        verificar(&app, &dom, "1004", mau, quem_liga).await["reason"],
        "invalid"
    );
    assert_eq!(
        verificar(&app, &dom, "1004", &pin, quem_liga).await["valid"],
        true,
        "a origem ficou travada por falhas fora da janela"
    );
}

/// Sem a origem no pedido, a verificação não corre: o travão não é opcional.
#[sqlx::test(migrations = "./migrations")]
async fn sem_origem_o_pedido_e_recusado(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-origem-falta.ao").await;
    let quem = app.add_member(&a, "colega", "member").await;
    let pin = ramal_com_pin(&app, &a, &quem, "1004").await;
    let dom = dominio(&app, a.org()).await;
    let r = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/voice/ivr/verify-extension-pin",
            &[("x-voice-secret", VOICE_SECRET)],
            Some(json!({"domain": dom, "extension": "1004", "pin": pin})),
        )
        .await;
    assert_eq!(r.status, 422, "{}", r.text);
    assert!(!r.text.contains("\"valid\""), "{}", r.text);
}
