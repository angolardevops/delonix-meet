//! R273 — um ramal interno marca o número de acesso às reuniões e entra numa
//! sala pelo IVR. Contra Postgres real.
//!
//! O que se mede aqui é a REGRA do servidor: quem o IVR deixa entrar, e onde.
//! Não se mede a chamada: nenhum FreeSWITCH corre nestes testes.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

const VOICE_SECRET: &str = "segredo-da-media-0123456789";
const ADVERTISE: &str = "ponte.teste:5099";

async fn spawn(db: sqlx::PgPool, extra: &[(&str, &str)]) -> TestApp {
    let mut vars = vec![
        ("VOICE_INTERNAL_SECRET", VOICE_SECRET),
        // A ponte «configurada», para o `room_bridge` vir na resposta. O UA
        // SIP não arranca nos testes — só se lê a configuração.
        ("PHONE_BRIDGE_SIP_BIND", "127.0.0.1:5099"),
        ("PHONE_BRIDGE_FREESWITCH_IPS", "127.0.0.1"),
        ("PHONE_BRIDGE_SIP_ADVERTISE", ADVERTISE),
    ];
    vars.extend_from_slice(extra);
    TestApp::spawn_with(db, &vars).await
}

/// O que o `dialin_ivr.lua ramal` envia.
async fn ivr(
    app: &TestApp,
    secret: Option<&str>,
    user: &str,
    domain: &str,
    pin: &str,
) -> (u16, Value) {
    let headers: Vec<(&str, &str)> = secret.map(|s| ("x-voice-secret", s)).into_iter().collect();
    let r = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/voice/ivr/validate-extension",
            &headers,
            Some(json!({"sip_username": user, "domain": domain, "pin": pin})),
        )
        .await;
    (r.status, r.json())
}

struct Ramal {
    id: String,
    sip_username: String,
    sip_domain: String,
}

async fn novo_ramal(app: &TestApp, admin: &Account, member: &Account, number: &str) -> Ramal {
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/extensions", admin.org()),
            Some(&admin.token),
            json!({"member_id": member.user_id, "extension": number}),
        )
        .await;
    assert_eq!(st, 200, "criar ramal {number}: {body}");
    Ramal {
        id: body["id"].as_str().unwrap().into(),
        sip_username: body["sip_username"].as_str().unwrap().into(),
        sip_domain: body["sip_domain"].as_str().unwrap().into(),
    }
}

/// Uma org com DID, uma sala e a sala de voz dela: devolve `(código, PIN)`.
async fn sala_com_pin(app: &TestApp, owner: &Account, e164: &str) -> (String, String) {
    sqlx::query("INSERT INTO voice_did (org_id, e164) VALUES ($1::uuid, $2)")
        .bind(owner.org())
        .bind(e164)
        .execute(&app.db)
        .await
        .unwrap();
    let room = app.new_room(owner, "Sala com telefone").await;
    let code = room["code"].as_str().unwrap().to_string();
    let (st, vr) = app
        .post(
            &format!("/api/orgs/{}/voice/rooms", owner.org()),
            Some(&owner.token),
            json!({"room_code": code}),
        )
        .await;
    assert_eq!(st, 200, "criar sala de voz: {vr}");
    (code, vr["pin"].as_str().unwrap().to_string())
}

/// O PIN certo de uma sala da MESMA org devolve a ponte telefone↔sala — a
/// mesma que o dial-in por DID devolve.
#[sqlx::test(migrations = "./migrations")]
async fn ramal_com_o_pin_da_sua_org_recebe_a_ponte(db: sqlx::PgPool) {
    let app = spawn(db, &[]).await;
    let a = app.new_org("alfa-ramal.ao").await;
    let (code, pin) = sala_com_pin(&app, &a, "+244222300001").await;
    let ramal = novo_ramal(&app, &a, &a, "101").await;

    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        &ramal.sip_username,
        &ramal.sip_domain,
        &pin,
    )
    .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["room_code"], code.as_str());
    assert_eq!(
        body["room_bridge"]["sip_uri"],
        format!("sip:room-{code}@{ADVERTISE}"),
        "{body}"
    );
    assert_eq!(
        body["room_bridge"]["channel_vars"]["rtp_secure_media"],
        "mandatory:AES_CM_128_HMAC_SHA1_80",
        "{body}"
    );

    // É a MESMA ponte do dial-in: o caminho por DID devolve o mesmo objecto.
    let did = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/voice/ivr/validate",
            &[("x-voice-secret", VOICE_SECRET)],
            Some(json!({"did_e164": "+244222300001", "pin": pin})),
        )
        .await;
    assert_eq!(did.status, 200);
    // A única diferença é o bilhete de identidade de quem liga (R279): o
    // ramal já vem identificado, o dial-in ainda não.
    const TICKET: &str = "sip_h_X-Delonix-Caller-Ticket";
    let mut do_ramal = body["room_bridge"].clone();
    let bilhete = do_ramal["channel_vars"]
        .as_object_mut()
        .unwrap()
        .remove(TICKET);
    assert!(bilhete.is_some(), "o ramal não trouxe bilhete: {body}");
    let do_did = did.json()["room_bridge"].clone();
    assert!(do_did["channel_vars"].get(TICKET).is_none(), "{do_did}");
    assert_eq!(do_did, do_ramal);

    // PIN errado: recusado.
    let wrong = if pin == "000000" { "000001" } else { "000000" };
    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        &ramal.sip_username,
        &ramal.sip_domain,
        wrong,
    )
    .await;
    assert_eq!(st, 404, "{body}");
}

/// O isolamento: um ramal da org A não entra numa sala da org B, mesmo
/// sabendo o PIN. Controlo positivo: o MESMO PIN abre a sala a um ramal de B.
/// O que o IVR recebe para a perna da ponte, com a configuração dada.
async fn ponte_com(db: sqlx::PgPool, extra: &[(&str, &str)]) -> Value {
    let app = spawn(db, extra).await;
    let a = app.new_org("alfa-ramal.ao").await;
    let (_code, pin) = sala_com_pin(&app, &a, "+244222300001").await;
    let ramal = novo_ramal(&app, &a, &a, "101").await;
    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        &ramal.sip_username,
        &ramal.sip_domain,
        &pin,
    )
    .await;
    assert_eq!(st, 200, "{body}");
    body["room_bridge"].clone()
}

/// ADR-0017 — por omissão a perna oferece Opus à frente de PCMA: é a ordem que
/// o UA da ponte segue.
#[sqlx::test(migrations = "./migrations")]
async fn a_perna_da_ponte_oferece_opus_a_frente_de_pcma(db: sqlx::PgPool) {
    let ponte = ponte_com(db, &[]).await;
    assert_eq!(
        ponte["channel_vars"]["absolute_codec_string"], "OPUS,PCMA",
        "{ponte}"
    );
}

/// `PHONE_BRIDGE_WIDEBAND=0` repõe o caminho G.711, sem reconstruir nada.
#[sqlx::test(migrations = "./migrations")]
async fn com_a_banda_larga_desligada_a_perna_oferece_so_pcma(db: sqlx::PgPool) {
    let ponte = ponte_com(db, &[("PHONE_BRIDGE_WIDEBAND", "0")]).await;
    assert_eq!(
        ponte["channel_vars"]["absolute_codec_string"], "PCMA",
        "{ponte}"
    );
}

/// Quem escreve um valor que não se percebe queria mexer no interruptor: fica
/// DESLIGADO, não no «ligado» por omissão.
#[sqlx::test(migrations = "./migrations")]
async fn um_valor_que_nao_se_percebe_desliga_a_banda_larga(db: sqlx::PgPool) {
    let ponte = ponte_com(db, &[("PHONE_BRIDGE_WIDEBAND", "talvez")]).await;
    assert_eq!(
        ponte["channel_vars"]["absolute_codec_string"], "PCMA",
        "{ponte}"
    );
}

/// **A ponte é entregue quando o FreeSWITCH vem por NOME.** O compose e o chart
/// configuram `PHONE_BRIDGE_FREESWITCH_IPS` com o nome do serviço; a entrega
/// da ponte ao IVR só olhava para os IPs literais, e com a lista por nome
/// devolvia `room_bridge: null` — o UA à escuta e todas as chamadas a caírem na
/// conferência local (medido no laboratório a 2026-10-05).
#[sqlx::test(migrations = "./migrations")]
async fn a_ponte_e_entregue_com_o_freeswitch_por_nome(db: sqlx::PgPool) {
    let ponte = ponte_com(db, &[("PHONE_BRIDGE_FREESWITCH_IPS", "freeswitch.interno")]).await;
    assert!(
        ponte["sip_uri"]
            .as_str()
            .is_some_and(|u| u.starts_with("sip:room-")),
        "sem ponte com a lista por nome: {ponte}"
    );
}

/// E o controlo negativo: sem lista nenhuma continua a não haver ponte
/// (fail-closed) — a correcção de cima não a abriu a toda a gente.
#[sqlx::test(migrations = "./migrations")]
async fn sem_lista_de_origens_nao_ha_ponte(db: sqlx::PgPool) {
    let ponte = ponte_com(db, &[("PHONE_BRIDGE_FREESWITCH_IPS", "")]).await;
    assert!(
        ponte.is_null(),
        "ponte entregue sem origens autorizadas: {ponte}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn ramal_de_outra_org_nao_entra_mesmo_com_o_pin(db: sqlx::PgPool) {
    let app = spawn(db, &[]).await;
    let a = app.new_org("alfa-ramal.ao").await;
    let b = app.new_org("beta-ramal.ao").await;
    let (code_b, pin_b) = sala_com_pin(&app, &b, "+244222300002").await;
    let ramal_a = novo_ramal(&app, &a, &a, "101").await;
    let ramal_b = novo_ramal(&app, &b, &b, "101").await;

    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        &ramal_b.sip_username,
        &ramal_b.sip_domain,
        &pin_b,
    )
    .await;
    assert_eq!(st, 200, "controlo positivo: {body}");
    assert_eq!(body["room_code"], code_b.as_str());

    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        &ramal_a.sip_username,
        &ramal_a.sip_domain,
        &pin_b,
    )
    .await;
    assert_eq!(st, 404, "um ramal de A entrou numa sala de B: {body}");
    assert!(
        !body.to_string().contains(&code_b),
        "a recusa revela a sala de B: {body}"
    );

    // Dizer-se do domínio de B não muda a org do ramal de A: o par
    // (utilizador, domínio) não existe.
    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        &ramal_a.sip_username,
        &ramal_b.sip_domain,
        &pin_b,
    )
    .await;
    assert_eq!(st, 404, "{body}");
}

/// Quem não é um ramal activo de um membro activo não entra, mesmo com o PIN
/// certo da sua org.
#[sqlx::test(migrations = "./migrations")]
async fn ramal_inexistente_inactivo_ou_de_membro_arquivado_e_recusado(db: sqlx::PgPool) {
    let app = spawn(db, &[]).await;
    let a = app.new_org("alfa-ramal.ao").await;
    let (_code, pin) = sala_com_pin(&app, &a, "+244222300003").await;
    let colega = app.add_member(&a, "colega", "member").await;
    let ramal = novo_ramal(&app, &a, &a, "101").await;
    let ramal_colega = novo_ramal(&app, &a, &colega, "102").await;

    // Controlo positivo: os dois entram.
    for r in [&ramal, &ramal_colega] {
        let (st, body) = ivr(
            &app,
            Some(VOICE_SECRET),
            &r.sip_username,
            &r.sip_domain,
            &pin,
        )
        .await;
        assert_eq!(st, 200, "{body}");
    }

    // Um AOR que ninguém criou, no domínio certo.
    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        "ramal_0000000000000000",
        &ramal.sip_domain,
        &pin,
    )
    .await;
    assert_eq!(st, 404, "{body}");

    // Ramal desactivado pelo admin.
    let (st, body) = app
        .patch(
            &format!("/api/orgs/{}/extensions/{}", a.org(), ramal.id),
            Some(&a.token),
            json!({"active": false}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        &ramal.sip_username,
        &ramal.sip_domain,
        &pin,
    )
    .await;
    assert_eq!(st, 404, "ramal inactivo entrou: {body}");

    // Membro arquivado: o ramal continua na tabela, mas já não abre reuniões.
    app.archive_member(a.org(), &colega.user_id).await;
    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        &ramal_colega.sip_username,
        &ramal_colega.sip_domain,
        &pin,
    )
    .await;
    assert_eq!(st, 404, "ramal de membro arquivado entrou: {body}");
}

/// Rota de máquina: sem o segredo de voz não responde, exista ou não o ramal.
#[sqlx::test(migrations = "./migrations")]
async fn sem_o_segredo_de_voz_e_401(db: sqlx::PgPool) {
    let app = spawn(db, &[]).await;
    let a = app.new_org("alfa-ramal.ao").await;
    let (_code, pin) = sala_com_pin(&app, &a, "+244222300004").await;
    let ramal = novo_ramal(&app, &a, &a, "101").await;

    let (st, body) = ivr(&app, None, &ramal.sip_username, &ramal.sip_domain, &pin).await;
    assert_eq!(st, 401, "{body}");
    let (st, body) = ivr(
        &app,
        Some("outro-segredo-0123456789ab"),
        &ramal.sip_username,
        &ramal.sip_domain,
        &pin,
    )
    .await;
    assert_eq!(st, 401, "{body}");
    // Uma sessão de utilizador não é o segredo de voz.
    let r = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/voice/ivr/validate-extension",
            &[("authorization", &format!("Bearer {}", a.token))],
            Some(
                json!({"sip_username": ramal.sip_username, "domain": ramal.sip_domain, "pin": pin}),
            ),
        )
        .await;
    assert_eq!(r.status, 401);

    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        &ramal.sip_username,
        &ramal.sip_domain,
        &pin,
    )
    .await;
    assert_eq!(st, 200, "controlo positivo: {body}");
}

/// O PIN só é único por DID. Duas salas activas da mesma org com o mesmo PIN:
/// sem DID não há como escolher, e não se escolhe.
#[sqlx::test(migrations = "./migrations")]
async fn pin_ambiguo_dentro_da_org_e_recusado(db: sqlx::PgPool) {
    let app = spawn(db, &[]).await;
    let a = app.new_org("alfa-ramal.ao").await;
    let (_c1, pin1) = sala_com_pin(&app, &a, "+244222300005").await;
    let ramal = novo_ramal(&app, &a, &a, "101").await;

    // Segunda sala de voz, noutro DID, com o PIN da primeira.
    let did2: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO voice_did (org_id, e164) VALUES ($1::uuid, '+244222300006') RETURNING id",
    )
    .bind(a.org())
    .fetch_one(&app.db)
    .await
    .unwrap();
    let room2 = app.new_room(&a, "Outra sala").await;
    let (st, vr2) = app
        .post(
            &format!("/api/orgs/{}/voice/rooms", a.org()),
            Some(&a.token),
            json!({"room_code": room2["code"], "did_id": did2}),
        )
        .await;
    assert_eq!(st, 200, "{vr2}");
    let pin2 = vr2["pin"].as_str().unwrap().to_string();
    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        &ramal.sip_username,
        &ramal.sip_domain,
        &pin2,
    )
    .await;
    assert_eq!(st, 200, "controlo positivo: {body}");

    sqlx::query("UPDATE voice_room SET pin = $1 WHERE id = $2::uuid")
        .bind(&pin1)
        .bind(vr2["id"].as_str().unwrap())
        .execute(&app.db)
        .await
        .unwrap();
    let (st, body) = ivr(
        &app,
        Some(VOICE_SECRET),
        &ramal.sip_username,
        &ramal.sip_domain,
        &pin1,
    )
    .await;
    assert_eq!(st, 404, "{body}");
}

/// O número de acesso não é de ninguém: criar um ramal com ele é recusado com
/// código estável, as leituras dizem qual é, e o dialplan é mandado para o IVR.
#[sqlx::test(migrations = "./migrations")]
async fn o_numero_de_acesso_e_reservado_e_anunciado(db: sqlx::PgPool) {
    let app = spawn(db, &[]).await;
    let a = app.new_org("alfa-ramal.ao").await;
    let extensions = format!("/api/orgs/{}/extensions", a.org());

    let (st, body) = app
        .post(
            &extensions,
            Some(&a.token),
            json!({"member_id": a.user_id, "extension": "8000"}),
        )
        .await;
    assert_eq!(st, 409, "{body}");
    assert_eq!(body["code"], "ramais.extension_reserved", "{body}");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_extensions")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0, "o ramal reservado ficou na base");

    // O vizinho não é reservado, e a resposta já traz o número de acesso.
    let (st, created) = app
        .post(
            &extensions,
            Some(&a.token),
            json!({"member_id": a.user_id, "extension": "8001"}),
        )
        .await;
    assert_eq!(st, 200, "{created}");
    assert_eq!(created["meeting_access_number"], "8000", "{created}");
    let (st, list) = app.get(&extensions, Some(&a.token)).await;
    assert_eq!(st, 200, "{list}");
    assert_eq!(list[0]["extension"], "8001");
    assert_eq!(list[0]["meeting_access_number"], "8000", "{list}");
    let (st, patched) = app
        .patch(
            &format!("{extensions}/{}", created["id"].as_str().unwrap()),
            Some(&a.token),
            json!({"label": "secretária"}),
        )
        .await;
    assert_eq!(st, 200, "{patched}");
    assert_eq!(patched["meeting_access_number"], "8000", "{patched}");

    // O que o `ramais_dial.lua` pergunta: 8000 → IVR da sala; 8001 → o ramal.
    let resolve = |ext: &'static str| {
        let app = &app;
        let domain = created["sip_domain"].as_str().unwrap().to_string();
        async move {
            let r = app
                .raw(
                    reqwest::Method::POST,
                    "/internal/v1/voice/ivr/resolve-extension",
                    &[("x-voice-secret", VOICE_SECRET)],
                    Some(json!({"domain": domain, "extension": ext})),
                )
                .await;
            (r.status, r.json())
        }
    };
    let (st, body) = resolve("8000").await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body, json!({"meeting_access": true}));
    let (st, body) = resolve("8001").await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body, json!({"sip_username": created["sip_username"]}));
    let (st, _) = resolve("8002").await;
    assert_eq!(st, 404);
}

/// O número é configuração: muda-se `VOICE_MEETING_ACCESS_NUMBER` e muda o que
/// é reservado e o que é anunciado.
#[sqlx::test(migrations = "./migrations")]
async fn o_numero_de_acesso_vem_da_configuracao(db: sqlx::PgPool) {
    let app = spawn(db, &[("VOICE_MEETING_ACCESS_NUMBER", "777")]).await;
    let a = app.new_org("alfa-ramal.ao").await;
    let extensions = format!("/api/orgs/{}/extensions", a.org());

    let (st, body) = app
        .post(
            &extensions,
            Some(&a.token),
            json!({"member_id": a.user_id, "extension": "777"}),
        )
        .await;
    assert_eq!(st, 409, "{body}");
    assert_eq!(body["code"], "ramais.extension_reserved", "{body}");

    let (st, created) = app
        .post(
            &extensions,
            Some(&a.token),
            json!({"member_id": a.user_id, "extension": "8000"}),
        )
        .await;
    assert_eq!(
        st, 200,
        "com outro número reservado, o 8000 é livre: {created}"
    );
    assert_eq!(created["meeting_access_number"], "777", "{created}");
}

/// O endereço público do servidor SIP é configuração da instalação: sem
/// `VOICE_RAMAIS_PUBLIC_HOST` as leituras dizem `null` — nunca um valor
/// inventado a partir do domínio SIP ou do host do pedido.
#[sqlx::test(migrations = "./migrations")]
async fn sem_endereco_publico_configurado_o_servidor_sip_vem_nulo(db: sqlx::PgPool) {
    let app = spawn(db, &[]).await;
    let a = app.new_org("alfa-ramal.ao").await;
    let extensions = format!("/api/orgs/{}/extensions", a.org());

    let (st, created) = app
        .post(
            &extensions,
            Some(&a.token),
            json!({"member_id": a.user_id, "extension": "101"}),
        )
        .await;
    assert_eq!(st, 200, "{created}");
    assert!(
        created.as_object().unwrap().contains_key("sip_server"),
        "o campo existe mesmo sem valor: {created}"
    );
    assert_eq!(created["sip_server"], Value::Null, "{created}");
    let (st, list) = app.get(&extensions, Some(&a.token)).await;
    assert_eq!(st, 200, "{list}");
    assert_eq!(list[0]["sip_server"], Value::Null, "{list}");
}

/// Com o endereço configurado, todas as leituras de um ramal o trazem — lista,
/// criação, PATCH e regeneração da password — com o proxy pronto a colar.
#[sqlx::test(migrations = "./migrations")]
async fn o_endereco_publico_configurado_vem_em_todas_as_leituras(db: sqlx::PgPool) {
    let app = spawn(
        db,
        &[
            ("VOICE_RAMAIS_PUBLIC_HOST", "sip.exemplo.ao"),
            ("VOICE_RAMAIS_PUBLIC_PORT", "5080"),
            ("VOICE_RAMAIS_PUBLIC_TRANSPORT", "TLS"),
        ],
    )
    .await;
    let a = app.new_org("alfa-ramal.ao").await;
    let extensions = format!("/api/orgs/{}/extensions", a.org());
    let expected = json!({
        "host": "sip.exemplo.ao",
        "port": 5080,
        "transport": "tls",
        "uri": "sip:sip.exemplo.ao:5080;transport=tls",
    });

    let (st, created) = app
        .post(
            &extensions,
            Some(&a.token),
            json!({"member_id": a.user_id, "extension": "101"}),
        )
        .await;
    assert_eq!(st, 200, "{created}");
    assert_eq!(created["sip_server"], expected, "{created}");
    // O domínio SIP continua a ser o realm lógico: não foi trocado pelo host.
    assert_ne!(created["sip_domain"], "sip.exemplo.ao", "{created}");
    let id = created["id"].as_str().unwrap();

    let (st, list) = app.get(&extensions, Some(&a.token)).await;
    assert_eq!(st, 200, "{list}");
    assert_eq!(list[0]["sip_server"], expected, "{list}");

    let (st, patched) = app
        .patch(
            &format!("{extensions}/{id}"),
            Some(&a.token),
            json!({"label": "secretária"}),
        )
        .await;
    assert_eq!(st, 200, "{patched}");
    assert_eq!(patched["sip_server"], expected, "{patched}");

    let (st, regenerated) = app
        .post(
            &format!("{extensions}/{id}/regenerate-password"),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{regenerated}");
    assert_eq!(regenerated["sip_server"], expected, "{regenerated}");
}

/// Só o host é obrigatório: porta e transporte têm omissão (5070, udp). Um
/// host que não é um host (`sip:…`, com porta) não vira endereço.
#[sqlx::test(migrations = "./migrations")]
async fn porta_e_transporte_tem_omissao_e_um_host_mal_formado_nao_conta(db: sqlx::PgPool) {
    let app = spawn(db.clone(), &[("VOICE_RAMAIS_PUBLIC_HOST", "203.0.113.7")]).await;
    let a = app.new_org("alfa-ramal.ao").await;
    let extensions = format!("/api/orgs/{}/extensions", a.org());
    let (st, created) = app
        .post(
            &extensions,
            Some(&a.token),
            json!({"member_id": a.user_id, "extension": "101"}),
        )
        .await;
    assert_eq!(st, 200, "{created}");
    assert_eq!(
        created["sip_server"],
        json!({
            "host": "203.0.113.7",
            "port": 5070,
            "transport": "udp",
            "uri": "sip:203.0.113.7:5070;transport=udp",
        }),
        "{created}"
    );

    let bad = spawn(db, &[("VOICE_RAMAIS_PUBLIC_HOST", "sip:meet.ao:5070")]).await;
    let (st, list) = bad.get(&extensions, Some(&a.token)).await;
    assert_eq!(st, 200, "{list}");
    assert_eq!(list[0]["sip_server"], Value::Null, "{list}");
}

/// O `POST` de formulário do `mod_xml_curl` ao directório, com o segredo em
/// HTTP Basic como o FreeSWITCH o manda.
async fn directory(app: &TestApp, path: &str, user: &str, domain: &str) -> (u16, String) {
    use base64::Engine as _;
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("freeswitch:{VOICE_SECRET}"))
    );
    let res = app
        .http
        .post(app.url(path))
        .header("authorization", basic)
        .form(&[("user", user), ("domain", domain)])
        .send()
        .await
        .expect("pedido HTTP falhou");
    (res.status().as_u16(), res.text().await.unwrap_or_default())
}

fn md5_hex(s: &str) -> String {
    use md5::{Digest, Md5};
    Md5::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

const DIRECTORY: &str = "/internal/v1/voice/ivr/directory";

/// R286 — o HA1 do ramal está cifrado na base, e o directório continua a
/// entregar ao FreeSWITCH o valor que o digest SIP usa. O herdado em claro
/// lê-se, e um HA1 cifrado copiado para a linha de outro ramal não abre.
#[sqlx::test(migrations = "./migrations")]
async fn the_extension_ha1_is_sealed_at_rest_and_still_reaches_freeswitch(db: sqlx::PgPool) {
    let app = spawn(db, &[]).await;
    let a = app.new_org("alfa-ha1.ao").await;
    let colega = app.add_member(&a, "colega", "member").await;

    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/extensions", a.org()),
            Some(&a.token),
            json!({"member_id": a.user_id, "extension": "101"}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    let (id, user, domain, password) = (
        body["id"].as_str().unwrap().to_string(),
        body["sip_username"].as_str().unwrap().to_string(),
        body["sip_domain"].as_str().unwrap().to_string(),
        body["sip_password"].as_str().unwrap().to_string(),
    );
    let ha1 = md5_hex(&format!("{user}:{domain}:{password}"));

    // Na base: cifrado, e o valor que o SIP usa não está lá.
    let stored: String =
        sqlx::query_scalar("SELECT sip_ha1 FROM voice_extensions WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(
        stored.starts_with("enc:v1:"),
        "HA1 em claro na base: {stored}"
    );
    assert!(!stored.contains(&ha1));

    // Para o FreeSWITCH: o HA1 de sempre.
    let (st, xml) = directory(&app, DIRECTORY, &user, &domain).await;
    assert_eq!(st, 200, "{xml}");
    assert!(
        xml.contains(&format!(r#"<param name="a1-hash" value="{ha1}"/>"#)),
        "o directório não devolveu o HA1 do ramal: {xml}"
    );

    // Um ramal criado antes da R286 (HA1 em claro) continua a registar-se.
    sqlx::query("UPDATE voice_extensions SET sip_ha1 = $2 WHERE id = $1::uuid")
        .bind(&id)
        .bind(&ha1)
        .execute(&app.db)
        .await
        .unwrap();
    let (st, xml) = directory(&app, DIRECTORY, &user, &domain).await;
    assert_eq!(st, 200);
    assert!(
        xml.contains(&ha1),
        "o HA1 herdado em claro deixou de servir: {xml}"
    );

    // O cifrado de um ramal, copiado para a linha de OUTRO, não abre: o
    // directório não entrega credenciais trocadas.
    let outro = novo_ramal(&app, &a, &colega, "102").await;
    sqlx::query("UPDATE voice_extensions SET sip_ha1 = $2 WHERE id = $1::uuid")
        .bind(&outro.id)
        .bind(&stored)
        .execute(&app.db)
        .await
        .unwrap();
    let (st, xml) = directory(&app, DIRECTORY, &outro.sip_username, &outro.sip_domain).await;
    assert_ne!(st, 200, "abriu um HA1 de outra linha: {xml}");
    assert!(!xml.contains(&ha1));

    // Regenerar a password volta a gravar cifrado.
    let (st, body) = app
        .post(
            &format!("/api/orgs/{}/extensions/{id}/regenerate-password", a.org()),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    let stored: String =
        sqlx::query_scalar("SELECT sip_ha1 FROM voice_extensions WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(
        stored.starts_with("enc:v1:"),
        "regenerar gravou em claro: {stored}"
    );
}

/// R286 — as três rotas de máquina dos ramais deixaram o router público. Com
/// o listener interno configurado, o router público (o que o ingress publica)
/// não as tem, nem no caminho novo nem no antigo; sem ele, ficam no mesmo
/// porto, como o resto da API interna, e só no caminho novo.
#[sqlx::test(migrations = "./migrations")]
async fn the_extension_machine_routes_left_the_public_listener(db: sqlx::PgPool) {
    const ROUTES: [&str; 3] = ["directory", "resolve-extension", "dialplan-did"];

    // Com listener interno: o router público não responde a nenhuma.
    let split = spawn(db.clone(), &[("INTERNAL_BIND_ADDR", "127.0.0.1:0")]).await;
    for r in ROUTES {
        for prefix in ["/internal/v1/voice/ivr", "/api/voice/ivr"] {
            let (st, body) = directory(&split, &format!("{prefix}/{r}"), "x", "y").await;
            assert_eq!(st, 404, "{prefix}/{r} no listener público: {body}");
        }
    }

    // Sem listener interno (um só porto): o caminho novo responde, o antigo não.
    let single = spawn(db, &[]).await;
    for r in ROUTES {
        let (st, _) = directory(&single, &format!("/internal/v1/voice/ivr/{r}"), "x", "y").await;
        assert_ne!(st, 404, "/internal/v1/voice/ivr/{r} devia existir");
        let (st, _) = directory(&single, &format!("/api/voice/ivr/{r}"), "x", "y").await;
        assert_eq!(st, 404, "/api/voice/ivr/{r} ainda existe");
    }
}
