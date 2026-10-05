//! «Ligar a…» a partir da sala, esquema (docs/ligar-a-partir-da-sala.md, F1; migração 0095).
//! Contra Postgres real. Ainda não há rota: mede só o que a migração promete.
mod common;

use common::TestApp;
use sqlx::PgPool;

async fn novo_pedido(
    db: &PgPool,
    org: &str,
    room: &str,
    code: &str,
    ramal: &str,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO room_dial_outs
             (id, org_id, room_id, room_code, kind, channel, extension_id, status)
         VALUES (gen_random_uuid(), $1::uuid, $2::uuid, $3, 'voice', 'phone', $4::uuid, $5)",
    )
    .bind(org)
    .bind(room)
    .bind(code)
    .bind(ramal)
    .bind(status)
    .execute(db)
    .await
    .map(|_| ())
}

#[sqlx::test(migrations = "./migrations")]
async fn a_capacidade_vem_semeada_e_o_ramal_e_destino_valido(db: PgPool) {
    // owner/admin permitem; member/external_guest recusam (fail-closed).
    let mut v: Vec<(String, String)> = sqlx::query_as(
        "SELECT system_key, value FROM system_role_capability_defaults WHERE capability = 'sessions.dial_out'",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    v.sort();
    let esperado = [
        ("admin", "allow"),
        ("external_guest", "deny"),
        ("member", "deny"),
        ("owner", "allow"),
    ];
    assert_eq!(
        v,
        esperado
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .to_vec()
    );

    let app = TestApp::spawn(db.clone()).await;
    let a = app.new_org("dialout.test").await;
    let room = app.new_room(&a, "Sala").await;
    let (room_id, code) = (
        room["id"].as_str().unwrap().to_string(),
        room["code"].as_str().unwrap().to_string(),
    );
    let ramal: String = sqlx::query_scalar(
        "INSERT INTO voice_extensions (org_id, extension, sip_username, sip_password_hash, sip_ha1, label)
         VALUES ($1::uuid, '201', 'ramal_dialout', 'x', 'x', 'Recepção') RETURNING id::text",
    )
    .bind(a.org())
    .fetch_one(&db)
    .await
    .unwrap();

    // Um pedido para um ramal, sem número, é aceite.
    novo_pedido(&db, a.org(), &room_id, &code, &ramal, "queued")
        .await
        .unwrap();
    // Um segundo pedido VIVO para o mesmo ramal na mesma sala é recusado…
    for vivo in ["queued", "dialing", "ringing", "in_call"] {
        let e = novo_pedido(&db, a.org(), &room_id, &code, &ramal, vivo)
            .await
            .unwrap_err();
        assert!(
            e.to_string().contains("room_dial_outs_ramal_vivo_uidx"),
            "{vivo}: {e}"
        );
    }
    // …mas um já terminado não conta, e o histórico não impede um novo pedido.
    novo_pedido(&db, a.org(), &room_id, &code, &ramal, "ended")
        .await
        .unwrap();
    novo_pedido(&db, a.org(), &room_id, &code, &ramal, "failed")
        .await
        .unwrap();

    // Apagar o ramal não parte (ON DELETE SET NULL) e deixa o histórico.
    sqlx::query("DELETE FROM voice_extensions WHERE id = $1::uuid")
        .bind(&ramal)
        .execute(&db)
        .await
        .expect("apagar um ramal com pedidos não pode falhar");
    let (total, sem_ramal): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE extension_id IS NULL) FROM room_dial_outs WHERE room_id = $1::uuid",
    )
    .bind(&room_id)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!((total, sem_ramal), (3, 3));
}
