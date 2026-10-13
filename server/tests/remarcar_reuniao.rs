//! Remarcar uma reunião, contra Postgres real.
//!
//! **PORQUE A ROTA EXISTE.** Nada no servidor mudava o `starts_at`: o PATCH só
//! mexe nas opções de sessão e não havia nenhum `UPDATE … starts_at` em todo o
//! `server/src`. Uma reunião criada tinha hora fixa.
//!
//! **O que estes testes guardam, por ordem de importância:**
//!
//! 1. **só o anfitrião**, e quem não é nem dono nem convidado não fica a saber
//!    que a reunião existe;
//! 2. **os convidados são avisados** com as DUAS horas — a que sai da agenda e
//!    a que entra;
//! 3. **o `SEQUENCE` do `.ics` sobe** — sem isso o Outlook e o Google Calendar
//!    ignoram a versão nova em silêncio;
//! 4. **o lembrete por SMS volta à fila**, senão remarcar para mais tarde
//!    comia-o;
//! 5. as recusas com razão: recorrente, já começada, hora no passado.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

fn daqui_a(horas: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::hours(horas)).to_rfc3339()
}

/// O `.ics` da reunião. Mesma forma do `reuniao_tem_sala_ao_agendar.rs`: o
/// `raw` quer um `Method` e os cabeçalhos à mão, porque isto não é JSON.
async fn ics(app: &TestApp, dono: &Account, id: &str) -> String {
    let auth = format!("Bearer {}", dono.token);
    let r = app
        .raw(
            reqwest::Method::GET,
            &format!("/api/meetings/{id}/calendar.ics"),
            &[("Authorization", &auth)],
            None,
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text);
    r.text
}

async fn remarcar(app: &TestApp, who: &Account, id: &str, body: Value) -> (u16, Value) {
    app.post(
        &format!("/api/meetings/{id}/reschedule"),
        Some(&who.token),
        body,
    )
    .await
}

/// O caminho completo: a hora muda, o `sequence` sobe, e os convidados ficam
/// avisados com as duas horas.
#[sqlx::test(migrations = "./migrations")]
async fn remarcar_muda_a_hora_e_avisa_os_convidados(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("remarcar.ao").await;
    let convidado = app.add_member(&dono, "bruno", "member").await;
    let r = app
        .new_meeting(&dono, "Conselho", &[&convidado.user_id])
        .await;
    let id = r["id"].as_str().unwrap().to_string();

    let nova = daqui_a(48);
    let (st, m) = remarcar(
        &app,
        &dono,
        &id,
        json!({"starts_at": nova, "duration_min": 45}),
    )
    .await;
    assert_eq!(st, 200, "remarcar: {m}");
    assert_eq!(m["duration_min"], 45);
    assert!(
        m["starts_at"].as_str().unwrap().starts_with(&nova[..13]),
        "a hora tinha de ser a nova: {m}"
    );

    // O convidado foi avisado, e a notificação leva as duas horas.
    // Pergunta-se PELO TIPO e não «a mais recente»: o convite e a remarcação
    // nascem no mesmo teste e um `ORDER BY created_at` entre as duas é um
    // empate à espera de acontecer.
    let (body, data): (String, Value) = sqlx::query_as(
        "SELECT body, data FROM notifications
          WHERE user_id = $1::uuid AND kind = 'meeting.rescheduled'",
    )
    .bind(&convidado.user_id)
    .fetch_one(&app.state.db)
    .await
    .expect("o convidado tinha de ter sido avisado da hora nova");
    assert!(body.contains("Conselho"), "{body}");
    assert!(
        data["was_at"].is_string() && data["starts_at"].is_string(),
        "a notificação leva as DUAS horas: {data}"
    );

    // O anfitrião NÃO se avisa a si mesmo: foi ele que mudou.
    let n_dono: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notifications WHERE user_id = $1::uuid AND kind = 'meeting.rescheduled'",
    )
    .bind(&dono.user_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(n_dono, 0, "o anfitrião não se avisa a si mesmo");
}

/// **O `SEQUENCE` do `.ics`.** Sem ele, um cliente de calendário que já conhece
/// o evento ignora a versão nova **em silêncio** — a hora mudava aqui e não
/// mudava na agenda de ninguém.
#[sqlx::test(migrations = "./migrations")]
async fn o_ics_sobe_de_revisao_a_cada_remarcacao(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("ics.ao").await;
    let r = app.new_meeting(&dono, "Com agenda", &[]).await;
    let id = r["id"].as_str().unwrap().to_string();

    let antes = ics(&app, &dono, &id).await;
    assert!(
        antes.contains("SEQUENCE:0"),
        "a primeira versão é a 0: {antes}"
    );

    assert_eq!(
        remarcar(&app, &dono, &id, json!({"starts_at": daqui_a(72)}))
            .await
            .0,
        200
    );
    let depois = ics(&app, &dono, &id).await;
    assert!(
        depois.contains("SEQUENCE:1"),
        "a revisão tinha de subir: {depois}"
    );

    assert_eq!(
        remarcar(&app, &dono, &id, json!({"starts_at": daqui_a(96)}))
            .await
            .0,
        200
    );
    let outra = ics(&app, &dono, &id).await;
    assert!(
        outra.contains("SEQUENCE:2"),
        "e a subir a cada vez: {outra}"
    );
}

/// O lembrete por SMS volta à fila: se já tinha saído para a hora antiga, tem
/// de sair outra vez para a nova.
#[sqlx::test(migrations = "./migrations")]
async fn o_lembrete_volta_a_fila(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("lembrete.ao").await;
    let r = app.new_meeting(&dono, "Com lembrete", &[]).await;
    let id = r["id"].as_str().unwrap().to_string();

    // Como o varredor deixa a reunião depois de mandar o lembrete.
    sqlx::query(
        "UPDATE meetings SET sms_reminder_min = 30, sms_reminder_done_at = now()
          WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&app.state.db)
    .await
    .unwrap();

    assert_eq!(
        remarcar(&app, &dono, &id, json!({"starts_at": daqui_a(72)}))
            .await
            .0,
        200
    );
    let feito: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT sms_reminder_done_at FROM meetings WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(
        feito.is_none(),
        "o lembrete tinha de voltar à fila para a hora nova"
    );
}

/// Só o anfitrião. Um convidado leva 403 (já sabia que a reunião existe); quem
/// não é nem dono nem convidado leva 404 e não fica a saber nada.
#[sqlx::test(migrations = "./migrations")]
async fn so_o_anfitriao_remarca(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("anfitriao.ao").await;
    let convidado = app.add_member(&dono, "carla", "member").await;
    let estranho = app.add_member(&dono, "diogo", "member").await;
    let r = app
        .new_meeting(&dono, "Fechada", &[&convidado.user_id])
        .await;
    let id = r["id"].as_str().unwrap().to_string();

    let (st, body) = remarcar(&app, &convidado, &id, json!({"starts_at": daqui_a(48)})).await;
    assert_eq!(st, 403, "um convidado não remarca: {body}");
    assert_eq!(body["code"], "meeting.not_host");

    let (st, body) = remarcar(&app, &estranho, &id, json!({"starts_at": daqui_a(48)})).await;
    assert_eq!(
        st, 404,
        "quem não é da reunião não fica a saber que existe: {body}"
    );
    assert!(
        !body.to_string().contains("Fechada"),
        "o título não podia vazar: {body}"
    );

    // E a hora não mudou.
    let (_, m) = app
        .get(&format!("/api/meetings/{id}"), Some(&dono.token))
        .await;
    assert!(m["starts_at"]
        .as_str()
        .unwrap()
        .starts_with(&daqui_a(1)[..10]));
}

/// As três recusas com razão.
#[sqlx::test(migrations = "./migrations")]
async fn recusa_recorrente_comecada_e_passado(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("recusas.ao").await;

    // Hora no passado.
    let r = app.new_meeting(&dono, "Normal", &[]).await;
    let id = r["id"].as_str().unwrap().to_string();
    let (st, body) = remarcar(
        &app,
        &dono,
        &id,
        json!({"starts_at": (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339()}),
    )
    .await;
    assert_eq!(st, 400, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("starts_in_past"),
        "{body}"
    );

    // Já começada.
    sqlx::query("UPDATE meetings SET started_at = now() WHERE id = $1::uuid")
        .bind(&id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let (st, body) = remarcar(&app, &dono, &id, json!({"starts_at": daqui_a(48)})).await;
    assert_eq!(st, 409, "{body}");
    assert_eq!(body["code"], "meeting.already_started");

    // Recorrente.
    let (st, rec) = app
        .post(
            "/api/meetings",
            Some(&dono.token),
            json!({"title": "Semanal", "kind": "video", "starts_at": daqui_a(24),
                   "duration_min": 30, "invitee_ids": [], "recurrence_freq": "weekly"}),
        )
        .await;
    assert_eq!(st, 200, "criar recorrente: {rec}");
    let (st, body) = remarcar(
        &app,
        &dono,
        rec["id"].as_str().unwrap(),
        json!({"starts_at": daqui_a(48)}),
    )
    .await;
    assert_eq!(st, 409, "uma série não se remarca assim: {body}");
    assert_eq!(body["code"], "meeting.recurring_unsupported");
}

/// A duração fora de 5–1440 é recusada, como na criação.
#[sqlx::test(migrations = "./migrations")]
async fn duracao_tem_os_mesmos_limites_da_criacao(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let dono = app.new_org("duracao.ao").await;
    let r = app.new_meeting(&dono, "Normal", &[]).await;
    let id = r["id"].as_str().unwrap().to_string();
    for d in [4, 1441] {
        let (st, body) = remarcar(
            &app,
            &dono,
            &id,
            json!({"starts_at": daqui_a(48), "duration_min": d}),
        )
        .await;
        assert_eq!(st, 400, "duração {d}: {body}");
    }
    // E omissa mantém a que está.
    let (st, m) = remarcar(&app, &dono, &id, json!({"starts_at": daqui_a(48)})).await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["duration_min"], 30, "a duração não mudou");
}
