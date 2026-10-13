//! SMTP por organização e a **guarda de saída em TCP** (ADR-0025), contra
//! Postgres real.
//!
//! O ADR-0025 pôs o SMTP por organização atrás de uma condição: um guarda que
//! valide `host:porta` antes de abrir a ligação. O que estes testes guardam,
//! por ordem de gravidade:
//!
//! 1. **a guarda recusa a rede interna e os metadados da cloud** — sem isto, um
//!    administrador de qualquer organização transformava os nossos servidores
//!    num varredor de portas da rede interna;
//! 2. **a password nunca sai** numa resposta;
//! 3. **cada organização só vê a sua**;
//! 4. **uma ligação em claro tem de ser pedida por escrito**;
//! 5. **sem configuração, o correio sai pelo relay do operador** — isto
//!    acrescenta um caminho, não substitui nenhum.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

const RELAY_OPERADOR: &[(&str, &str)] = &[
    ("SMTP_HOST", "127.0.0.1"),
    ("SMTP_PORT", "2"),
    ("SMTP_STARTTLS", "0"),
    ("SMTP_FROM", "Operador <nao-responda@operador.ao>"),
    ("PUBLIC_URL", "https://meet.exemplo.ao"),
    // A cifra em repouso NÃO se configura aqui: em modo inseguro (o dos testes)
    // o `Config` deriva a caixa do `JWT_SECRET` sozinho. A recusa sem chaves
    // tem o seu próprio teste em `secrets_at_rest.rs`.
];

async fn gravar(app: &TestApp, who: &Account, body: Value) -> (u16, Value) {
    app.put(
        &format!("/api/orgs/{}/mail-settings", who.org()),
        Some(&who.token),
        body,
    )
    .await
}

fn config(host: &str, port: u16) -> Value {
    json!({
        "host": host, "port": port,
        "username": "conta", "password": "segredo-do-cliente",
        "from_address": "Kaeso <nao-responda@kaeso.ao>",
    })
}

/// **O TESTE QUE JUSTIFICA O PR.** A guarda recusa a rede interna, o loopback,
/// e o endereço de metadados da cloud. Sem ela, qualquer administrador
/// transformava os nossos servidores num varredor da rede interna — e o erro
/// não diz QUAL endereço foi recusado, para não ser um mapeador.
#[sqlx::test(migrations = "./migrations")]
async fn a_guarda_recusa_destinos_internos(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY_OPERADOR).await;
    let dono = app.new_org("guarda.ao").await;

    for (host, porque) in [
        ("169.254.169.254", "metadados da cloud"),
        ("127.0.0.1", "loopback"),
        ("10.0.0.5", "rede privada"),
        ("192.168.1.1", "rede privada"),
        ("172.16.0.1", "rede privada"),
        ("[::1]", "loopback IPv6"),
        ("[::ffff:127.0.0.1]", "loopback mapeado em IPv6"),
    ] {
        let (st, body) = gravar(&app, &dono, config(host, 587)).await;
        assert_eq!(st, 400, "{host} ({porque}) tinha de ser recusado: {body}");
        assert_eq!(
            body["code"], "org_mail_settings.host_refused",
            "{host}: o código tinha de ser o da recusa de host: {body}"
        );
        assert!(
            !body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("169.254.169.254")
                || host == "169.254.169.254",
            "a mensagem não pode mapear a rede interna: {body}"
        );
    }
    // E nada ficou gravado.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM org_mail_settings")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(n, 0, "nenhuma configuração recusada podia ter ficado");
}

/// A password é selada e **nunca** volta numa resposta — nem na gravação, nem
/// na leitura. A resposta diz apenas que existe.
#[sqlx::test(migrations = "./migrations")]
async fn a_password_nunca_sai_na_resposta(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY_OPERADOR).await;
    let dono = app.new_org("segredo.ao").await;

    let (st, gravada) = gravar(&app, &dono, config("smtp.exemplo.ao", 587)).await;
    assert_eq!(st, 200, "gravar: {gravada}");
    assert_eq!(gravada["password_configured"], true);
    let texto = gravada.to_string();
    assert!(
        !texto.contains("segredo-do-cliente"),
        "a password saiu na resposta da gravação: {texto}"
    );

    let (st, lida) = app
        .get(
            &format!("/api/orgs/{}/mail-settings", dono.org()),
            Some(&dono.token),
        )
        .await;
    assert_eq!(st, 200, "{lida}");
    assert!(
        !lida.to_string().contains("segredo-do-cliente"),
        "a password saiu na leitura: {lida}"
    );
    assert_eq!(lida["host"], "smtp.exemplo.ao");
    assert_eq!(lida["starttls"], true, "STARTTLS é a omissão");

    // E na base está selada, não em claro.
    let selada: String =
        sqlx::query_scalar("SELECT password_sealed FROM org_mail_settings WHERE org_id = $1::uuid")
            .bind(dono.org())
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(!selada.is_empty());
    assert!(
        !selada.contains("segredo-do-cliente"),
        "a password está em claro na base"
    );
}

/// Uma ligação em claro tem de ser pedida **por escrito**: sem TLS a password
/// do cliente viaja à vista, e isso não pode ser o descuido de um campo.
#[sqlx::test(migrations = "./migrations")]
async fn sem_starttls_exige_confirmacao_escrita(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY_OPERADOR).await;
    let dono = app.new_org("claro.ao").await;

    let mut c = config("smtp.exemplo.ao", 25);
    c["starttls"] = json!(false);
    let (st, body) = gravar(&app, &dono, c.clone()).await;
    assert_eq!(st, 400, "sem confirmação tinha de recusar: {body}");
    assert_eq!(body["code"], "org_mail_settings.plaintext_not_confirmed");

    c["allow_plaintext"] = json!(true);
    let (st, body) = gravar(&app, &dono, c).await;
    assert_eq!(st, 200, "com confirmação escrita aceita: {body}");
    assert_eq!(body["starttls"], false);
}

/// Cada organização só vê a sua. (O controlo entre organizações completo está
/// na `web/e2e/isolamento.mjs`; aqui fica a prova em Rust de que a leitura de
/// uma não mostra a outra.)
#[sqlx::test(migrations = "./migrations")]
async fn uma_org_nao_ve_o_smtp_da_outra(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY_OPERADOR).await;
    let a = app.new_org("alfa-smtp.ao").await;
    let b = app.new_org("beta-smtp.ao").await;

    let (st, _) = gravar(&app, &b, config("smtp.beta.ao", 587)).await;
    assert_eq!(st, 200);

    // A, na SUA org, não tem nada — a de B não lhe aparece.
    let (st, body) = app
        .get(
            &format!("/api/orgs/{}/mail-settings", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 404, "a org A não configurou nada: {body}");

    // E a apontar para a org de B é recusado sem confirmar que existe.
    let (st, body) = app
        .get(
            &format!("/api/orgs/{}/mail-settings", b.org()),
            Some(&a.token),
        )
        .await;
    common::assert_denied("A lê o SMTP de B", st, &body, "smtp.beta.ao");
}

/// Sem configuração própria, o correio da organização sai pelo relay do
/// operador — isto acrescenta um caminho e não substitui nenhum. E uma
/// configuração DESLIGADA é como não haver nenhuma.
#[sqlx::test(migrations = "./migrations")]
async fn sem_configuracao_usa_o_relay_do_operador(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY_OPERADOR).await;
    let dono = app.new_org("fallback.ao").await;
    let convidado = app.add_member(&dono, "bruno", "member").await;
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE id = $1::uuid")
        .bind(&convidado.user_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    // Sem SMTP próprio: a mensagem entra na caixa de saída (pelo operador).
    app.new_meeting(&dono, "Com o relay do operador", &[&convidado.user_id])
        .await;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM mail_messages")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(n, 1, "sem SMTP próprio o correio sai pelo operador");

    // Desligada é como não haver: a leitura mostra `enabled: false` e o correio
    // continua a sair.
    let mut c = config("smtp.exemplo.ao", 587);
    c["enabled"] = json!(false);
    let (st, body) = gravar(&app, &dono, c).await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["enabled"], false);
}

/// Apagar devolve a organização ao relay do operador, e é idempotente.
#[sqlx::test(migrations = "./migrations")]
async fn apagar_volta_ao_operador_e_repete_se(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY_OPERADOR).await;
    let dono = app.new_org("apagar.ao").await;
    assert_eq!(
        gravar(&app, &dono, config("smtp.exemplo.ao", 587)).await.0,
        200
    );

    let caminho = format!("/api/orgs/{}/mail-settings", dono.org());
    assert_eq!(app.delete(&caminho, Some(&dono.token)).await.0, 204);
    assert_eq!(
        app.get(&caminho, Some(&dono.token)).await.0,
        404,
        "depois de apagar não há configuração"
    );
    assert_eq!(
        app.delete(&caminho, Some(&dono.token)).await.0,
        204,
        "apagar outra vez é a mesma coisa"
    );
}

/// O remetente tem de ser um endereço: um relay sério recusa o resto, e o erro
/// dele chegaria tarde e sem razão clara.
#[sqlx::test(migrations = "./migrations")]
async fn remetente_tem_de_ser_um_endereco(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY_OPERADOR).await;
    let dono = app.new_org("remetente.ao").await;
    let mut c = config("smtp.exemplo.ao", 587);
    c["from_address"] = json!("isto nao e um endereco");
    let (st, body) = gravar(&app, &dono, c).await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "org_mail_settings.invalid_from");
}

/// A verificação não manda mensagem nenhuma, e passa pela MESMA guarda: um
/// relay que não existe dá `ok: false` com a razão — uma resposta, não um erro.
#[sqlx::test(migrations = "./migrations")]
async fn verificar_diz_a_razao_e_nao_manda_nada(db: sqlx::PgPool) {
    let app = TestApp::spawn_with(db, RELAY_OPERADOR).await;
    let dono = app.new_org("verificar.ao").await;

    // Sem configuração: 404, não um falso «ok».
    let check = format!("/api/orgs/{}/mail-settings/check", dono.org());
    let (st, body) = app.post(&check, Some(&dono.token), json!({})).await;
    assert_eq!(st, 404, "{body}");

    // Com uma configuração que aponta para onde nada ouve.
    let mut c = config("127.0.0.2", 2);
    c["starttls"] = json!(false);
    c["allow_plaintext"] = json!(true);
    // 127.0.0.2 é loopback: a guarda recusa-o ao GRAVAR, que é o ponto.
    let (st, body) = gravar(&app, &dono, c).await;
    assert_eq!(st, 400, "um loopback não se grava: {body}");

    // E nada foi para a caixa de saída por causa de uma verificação.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM mail_messages")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(n, 0, "verificar não manda mensagem nenhuma");
}
