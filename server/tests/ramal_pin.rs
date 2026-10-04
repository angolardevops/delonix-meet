//! R276 — o PIN de um ramal, os ramais da empresa e a atribuição em massa.
//! Contra Postgres real.
//!
//! O que se mede aqui é a REGRA do servidor. Nenhum FreeSWITCH corre nestes
//! testes, e o Lua do IVR ainda não chama a verificação: o que faz de IVR é o
//! próprio teste, com o segredo de voz.
mod common;

use common::{Account, TestApp};
use serde_json::{json, Value};

const VOICE_SECRET: &str = "segredo-da-media-0123456789";

async fn spawn(db: sqlx::PgPool) -> TestApp {
    TestApp::spawn_with(db, &[("VOICE_INTERNAL_SECRET", VOICE_SECRET)]).await
}

fn ext_path(org: &str, tail: &str) -> String {
    format!("/api/orgs/{org}/extensions{tail}")
}

fn mine(org: &str, tail: &str) -> String {
    format!("/api/orgs/{org}/my-extension{tail}")
}

/// Cria um ramal; `member` ausente = ramal da empresa. Devolve o corpo.
async fn novo_ramal(
    app: &TestApp,
    admin: &Account,
    member: Option<&Account>,
    number: &str,
    label: &str,
) -> Value {
    let mut body = json!({"extension": number, "label": label});
    if let Some(m) = member {
        body["member_id"] = json!(m.user_id);
    }
    let (st, resp) = app
        .post(&ext_path(admin.org(), ""), Some(&admin.token), body)
        .await;
    assert_eq!(st, 200, "criar ramal {number}: {resp}");
    resp
}

async fn listar(app: &TestApp, admin: &Account) -> Vec<Value> {
    let (st, body) = app
        .get(&ext_path(admin.org(), ""), Some(&admin.token))
        .await;
    assert_eq!(st, 200, "{body}");
    body.as_array().unwrap().clone()
}

async fn estado_do_pin(app: &TestApp, admin: &Account, id: &str) -> String {
    listar(app, admin)
        .await
        .into_iter()
        .find(|e| e["id"] == id)
        .expect("ramal na lista")["pin_state"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn dominio(app: &TestApp, org: &str) -> String {
    let slug: String = sqlx::query_scalar("SELECT slug FROM organizations WHERE id = $1::uuid")
        .bind(org)
        .fetch_one(&app.db)
        .await
        .unwrap();
    format!("{slug}.{}", app.state.config.voice_ramais_domain_suffix)
}

/// O que o IVR enviará (lote seguinte).
async fn verificar(app: &TestApp, domain: &str, extension: &str, pin: &str) -> Value {
    let r = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/voice/ivr/verify-extension-pin",
            &[("x-voice-secret", VOICE_SECRET)],
            Some(json!({"domain": domain, "extension": extension, "pin": pin})),
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

async fn gerar_o_meu(app: &TestApp, quem: &Account) -> String {
    let (st, body) = app
        .post(
            &mine(quem.org(), "/regenerate-pin"),
            Some(&quem.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    body["pin"].as_str().unwrap().to_string()
}

async fn accoes_de_auditoria(app: &TestApp, org: &str, action: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT target FROM audit_logs WHERE org_id = $1::uuid AND action = $2 ORDER BY seq",
    )
    .bind(org)
    .bind(action)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn o_pin_sai_uma_vez_e_so_o_hash_fica(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-pin.ao").await;
    let colega = app.add_member(&a, "colega", "member").await;
    let ramal = novo_ramal(&app, &a, Some(&colega), "1004", "").await;
    let id = ramal["id"].as_str().unwrap();

    // Nasce «por definir», e a criação não traz PIN nenhum.
    assert_eq!(ramal["pin_state"], "unset", "{ramal}");
    assert!(ramal.get("pin").is_none(), "{ramal}");
    let (st, meu) = app.get(&mine(a.org(), ""), Some(&colega.token)).await;
    assert_eq!(st, 200, "{meu}");
    assert_eq!(meu["extension"], "1004");
    assert_eq!(meu["pin_state"], "unset");
    let dom = dominio(&app, a.org()).await;
    assert_eq!(
        verificar(&app, &dom, "1004", "584930").await["reason"],
        "not_set"
    );

    let pin = gerar_o_meu(&app, &colega).await;
    assert_eq!(pin.len(), 6, "{pin}");
    assert!(pin.bytes().all(|b| b.is_ascii_digit()), "{pin}");
    assert!(!pin.contains("1004"));

    // Na base só o hash; nenhuma leitura devolve o valor.
    let hash: String =
        sqlx::query_scalar("SELECT pin_hash FROM voice_extensions WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(hash.starts_with("$argon2"), "{hash}");
    assert!(!hash.contains(&pin));
    let lista = serde_json::to_string(&listar(&app, &a).await).unwrap();
    assert!(
        !lista.contains(&pin) && !lista.contains("pin_hash"),
        "{lista}"
    );
    assert_eq!(estado_do_pin(&app, &a, id).await, "set");
    let (_, meu) = app.get(&mine(a.org(), ""), Some(&colega.token)).await;
    assert_eq!(meu["pin_state"], "set");
    assert!(!meu.to_string().contains(&pin), "{meu}");

    // O PIN identifica a pessoa.
    let ok = verificar(&app, &dom, "1004", &pin).await;
    assert_eq!(ok["valid"], true, "{ok}");
    assert_eq!(ok["member_id"], json!(colega.user_id));
    assert_eq!(ok["extension_id"], json!(id));
    assert!(ok["display_name"].as_str().unwrap().starts_with("colega"));

    // Regenerar: o anterior deixa de servir.
    let novo = gerar_o_meu(&app, &colega).await;
    if novo != pin {
        assert_eq!(verificar(&app, &dom, "1004", &pin).await["valid"], false);
    }
    assert_eq!(verificar(&app, &dom, "1004", &novo).await["valid"], true);

    // Quem não tem ramal: 404 com código.
    let (st, body) = app.get(&mine(a.org(), ""), Some(&a.token)).await;
    assert_eq!(st, 404, "{body}");
    assert_eq!(body["code"], "ramais.no_extension");
}

#[sqlx::test(migrations = "./migrations")]
async fn um_pin_escolhido_passa_pelas_recusas(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-escolha.ao").await;
    novo_ramal(&app, &a, Some(&a), "1234", "").await;
    for (pin, code) in [
        ("111111", "ramais.pin_repeated"),
        ("123456", "ramais.pin_sequence"),
        ("987654", "ramais.pin_sequence"),
        ("001234", "ramais.pin_contains_extension"),
        ("12345", "ramais.pin_format"),
        ("12345a", "ramais.pin_format"),
    ] {
        let (st, body) = app
            .put(&mine(a.org(), "/pin"), Some(&a.token), json!({"pin": pin}))
            .await;
        assert_eq!(st, 400, "{pin}: {body}");
        assert_eq!(body["code"], code, "{pin}: {body}");
    }
    let (_, meu) = app.get(&mine(a.org(), ""), Some(&a.token)).await;
    assert_eq!(meu["pin_state"], "unset", "uma recusa não grava nada");

    let (st, body) = app
        .put(
            &mine(a.org(), "/pin"),
            Some(&a.token),
            json!({"pin": "584930"}),
        )
        .await;
    assert_eq!(st, 204, "{body}");
    let dom = dominio(&app, a.org()).await;
    assert_eq!(verificar(&app, &dom, "1234", "584930").await["valid"], true);
    assert_eq!(
        accoes_de_auditoria(&app, a.org(), "ramal.pin_alterado").await,
        ["1234"]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn o_admin_forca_a_regeneracao_sem_ver_o_pin(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-admin.ao").await;
    let colega = app.add_member(&a, "colega", "member").await;
    let ramal = novo_ramal(&app, &a, Some(&colega), "1004", "").await;
    let id = ramal["id"].as_str().unwrap();
    let pin = gerar_o_meu(&app, &colega).await;
    let dom = dominio(&app, a.org()).await;

    // O PIN de uma pessoa não se gera nem se escolhe por terceiros.
    let (st, body) = app
        .post(
            &ext_path(a.org(), &format!("/{id}/regenerate-pin")),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 409, "{body}");
    assert_eq!(body["code"], "ramais.pin_belongs_to_member");
    assert!(body.get("pin").is_none());
    let (st, body) = app
        .put(
            &ext_path(a.org(), &format!("/{id}/pin")),
            Some(&a.token),
            json!({"pin": "584930"}),
        )
        .await;
    assert_eq!(st, 409, "{body}");
    assert_eq!(verificar(&app, &dom, "1004", &pin).await["valid"], true);

    // Forçar a regeneração = limpar. A resposta não traz PIN.
    let r = app
        .raw(
            reqwest::Method::DELETE,
            &ext_path(a.org(), &format!("/{id}/pin")),
            &[("authorization", &format!("Bearer {}", a.token))],
            None,
        )
        .await;
    assert_eq!(r.status, 204, "{}", r.text);
    assert!(r.text.is_empty(), "{}", r.text);
    assert_eq!(estado_do_pin(&app, &a, id).await, "unset");
    assert_eq!(
        verificar(&app, &dom, "1004", &pin).await["reason"],
        "not_set"
    );

    // A pessoa gera o novo na sua área.
    let novo = gerar_o_meu(&app, &colega).await;
    assert_eq!(verificar(&app, &dom, "1004", &novo).await["valid"], true);

    // Um membro que não é admin não mexe no PIN de ninguém pela rota de admin.
    let (st, _) = app
        .delete(
            &ext_path(a.org(), &format!("/{id}/pin")),
            Some(&colega.token),
        )
        .await;
    assert_eq!(st, 403);
    assert_eq!(estado_do_pin(&app, &a, id).await, "set");
}

#[sqlx::test(migrations = "./migrations")]
async fn cinco_falhas_bloqueiam_e_ficam_na_auditoria(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-bloqueio.ao").await;
    let colega = app.add_member(&a, "colega", "member").await;
    let ramal = novo_ramal(&app, &a, Some(&colega), "1004", "").await;
    let id = ramal["id"].as_str().unwrap();
    let pin = gerar_o_meu(&app, &colega).await;
    let mau = errado(&pin);
    let dom = dominio(&app, a.org()).await;

    for n in 1..=4 {
        let r = verificar(&app, &dom, "1004", mau).await;
        assert_eq!(r["valid"], false, "tentativa {n}: {r}");
        assert_eq!(r["reason"], "invalid", "tentativa {n}: {r}");
    }
    assert_eq!(estado_do_pin(&app, &a, id).await, "set");
    let quinta = verificar(&app, &dom, "1004", mau).await;
    assert_eq!(quinta["reason"], "locked", "{quinta}");
    assert_eq!(quinta["retry_after_secs"], 900, "{quinta}");
    assert_eq!(estado_do_pin(&app, &a, id).await, "locked");

    // Bloqueado: nem o PIN certo passa, e a tentativa não conta.
    let certo = verificar(&app, &dom, "1004", &pin).await;
    assert_eq!(certo["valid"], false, "{certo}");
    assert_eq!(certo["reason"], "locked");
    assert!(certo["retry_after_secs"].as_i64().unwrap() > 0);

    // A trilha: cinco falhas, um bloqueio, e o PIN tentado em lado nenhum.
    let falhas = accoes_de_auditoria(&app, a.org(), "ramal.pin_falhado").await;
    assert_eq!(falhas.len(), 5, "{falhas:?}");
    assert!(falhas[4].contains("tentativa 5 de 5"), "{falhas:?}");
    let bloqueios = accoes_de_auditoria(&app, a.org(), "ramal.pin_bloqueado").await;
    assert_eq!(bloqueios.len(), 1, "{bloqueios:?}");
    assert!(falhas
        .iter()
        .chain(&bloqueios)
        .all(|t| !t.contains(mau) && !t.contains(&pin)));
    // Quem falhou foi quem ligou, não o dono do ramal: nenhuma destas linhas
    // tem a pessoa do ramal como actor — nem pelo id, nem pelo nome.
    let actores: Vec<(String, String)> = sqlx::query_as(
        "SELECT actor_id::text, actor_name FROM audit_logs
          WHERE org_id = $1::uuid AND action IN ('ramal.pin_falhado', 'ramal.pin_bloqueado')",
    )
    .bind(a.org())
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(actores.len(), 6, "{actores:?}");
    for (id, nome) in &actores {
        assert_ne!(id, &colega.user_id, "a falha ficou em nome da vítima");
        assert_eq!(id, "00000000-0000-0000-0000-000000000000");
        assert!(
            !nome.contains("colega"),
            "a falha ficou em nome da vítima: {nome}"
        );
    }
    // E o ramal está no alvo, para a trilha dizer QUAL foi atacado.
    assert!(falhas
        .iter()
        .chain(&bloqueios)
        .all(|t| t.contains("ramal 1004")));
    // O administrador vê-a pela rota de auditoria, e a cadeia continua inteira.
    let (st, eventos) = app
        .get(
            &format!("/api/orgs/{}/audit-events", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 200);
    assert!(
        eventos
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "ramal.pin_bloqueado"),
        "{eventos}"
    );
    // O que o administrador LÊ também não aponta para a vítima.
    for e in eventos.as_array().unwrap() {
        if e["action"]
            .as_str()
            .unwrap()
            .starts_with("ramal.pin_falhado")
            || e["action"] == "ramal.pin_bloqueado"
        {
            assert!(!e["actor"].as_str().unwrap().contains("colega"), "{e}");
        }
    }
    let (st, cadeia) = app
        .get(
            &format!("/api/orgs/{}/audit-events/verification", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 200, "{cadeia}");
    assert_eq!(cadeia["intact"], true, "{cadeia}");

    // O bloqueio é TEMPORÁRIO: passado o prazo, o PIN certo volta a servir e
    // há outra vez cinco tentativas.
    sqlx::query(
        "UPDATE voice_extensions SET pin_locked_until = now() - interval '1 second'
          WHERE id = $1::uuid",
    )
    .bind(id)
    .execute(&app.db)
    .await
    .unwrap();
    assert_eq!(estado_do_pin(&app, &a, id).await, "set");
    for _ in 0..4 {
        assert_eq!(
            verificar(&app, &dom, "1004", mau).await["reason"],
            "invalid"
        );
    }
    assert_eq!(verificar(&app, &dom, "1004", &pin).await["valid"], true);
    // Um acerto zera o contador.
    for _ in 0..4 {
        assert_eq!(
            verificar(&app, &dom, "1004", mau).await["reason"],
            "invalid"
        );
    }
    assert_eq!(verificar(&app, &dom, "1004", mau).await["reason"], "locked");

    // Um PIN novo, gerado pela pessoa com a sessão, levanta o bloqueio.
    let novo = gerar_o_meu(&app, &colega).await;
    assert_eq!(verificar(&app, &dom, "1004", &novo).await["valid"], true);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_verificacao_exige_o_segredo_de_voz_e_nao_sai_da_org(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-segredo.ao").await;
    let b = app.new_org("beta-segredo.ao").await;
    let ramal = novo_ramal(&app, &a, Some(&a), "1004", "").await;
    let id = ramal["id"].as_str().unwrap();
    let pin = gerar_o_meu(&app, &a).await;
    let dom_a = dominio(&app, a.org()).await;
    let dom_b = dominio(&app, b.org()).await;
    let corpo = json!({"domain": dom_a, "extension": "1004", "pin": pin});

    let bearer = format!("Bearer {}", a.token);
    for headers in [
        vec![],
        vec![("x-voice-secret", "outro-segredo-0123456789ab")],
        vec![("authorization", bearer.as_str())],
    ] {
        let r = app
            .raw(
                reqwest::Method::POST,
                "/internal/v1/voice/ivr/verify-extension-pin",
                &headers,
                Some(corpo.clone()),
            )
            .await;
        assert_eq!(r.status, 401, "{headers:?}: {}", r.text);
        assert!(!r.text.contains("valid"), "{}", r.text);
    }

    // O mesmo ramal e o mesmo PIN, pelo domínio de OUTRA organização: nada.
    assert_eq!(
        verificar(&app, &dom_b, "1004", &pin).await["reason"],
        "invalid"
    );
    assert_eq!(
        verificar(&app, "ninguem.ramais.exemplo", "1004", &pin).await["reason"],
        "invalid"
    );
    // Ramal que não existe responde como um PIN errado.
    assert_eq!(
        verificar(&app, &dom_a, "1999", &pin).await["reason"],
        "invalid"
    );

    // O admin da B não alcança o ramal da A, e a pessoa da A não tem «o meu
    // ramal» na B.
    for (method, tail, body) in [
        (reqwest::Method::DELETE, format!("/{id}/pin"), None),
        (
            reqwest::Method::POST,
            format!("/{id}/regenerate-pin"),
            Some(json!({})),
        ),
        (
            reqwest::Method::PUT,
            format!("/{id}/pin"),
            Some(json!({"pin": "584930"})),
        ),
    ] {
        let (st, resp) = app
            .call(
                method.clone(),
                &ext_path(b.org(), &tail),
                Some(&b.token),
                body,
            )
            .await;
        assert_eq!(st, 404, "{method} {tail}: {resp}");
    }
    let (st, _) = app.get(&mine(b.org(), ""), Some(&a.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app
        .post(&mine(b.org(), "/regenerate-pin"), Some(&a.token), json!({}))
        .await;
    assert_eq!(st, 404);
    assert_eq!(verificar(&app, &dom_a, "1004", &pin).await["valid"], true);

    // Pessoa arquivada: o ramal deixa de identificar alguém.
    let colega = app.add_member(&a, "colega", "member").await;
    novo_ramal(&app, &a, Some(&colega), "1005", "").await;
    let pin_colega = gerar_o_meu(&app, &colega).await;
    assert_eq!(
        verificar(&app, &dom_a, "1005", &pin_colega).await["valid"],
        true
    );
    app.archive_member(a.org(), &colega.user_id).await;
    assert_eq!(
        verificar(&app, &dom_a, "1005", &pin_colega).await["reason"],
        "invalid"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn ramal_da_empresa_tem_etiqueta_e_o_pin_e_do_admin(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-empresa.ao").await;

    // Sem pessoa E sem etiqueta: recusado.
    for label in ["", "   "] {
        let (st, body) = app
            .post(
                &ext_path(a.org(), ""),
                Some(&a.token),
                json!({"extension": "2000", "label": label}),
            )
            .await;
        assert_eq!(st, 400, "{body}");
        assert_eq!(body["code"], "ramais.label_required");
    }

    let recepcao = novo_ramal(&app, &a, None, "2000", "Recepção").await;
    let id = recepcao["id"].as_str().unwrap();
    assert!(recepcao["member_id"].is_null(), "{recepcao}");
    assert!(recepcao["member_username"].is_null(), "{recepcao}");
    assert_eq!(recepcao["label"], "Recepção");
    assert_eq!(recepcao["pin_state"], "unset", "sem PIN por omissão");
    assert_eq!(recepcao["sip_password"].as_str().unwrap().len(), 30);
    // Não há «um ramal da empresa por organização».
    let portaria = novo_ramal(&app, &a, None, "2001", "Portaria").await;
    assert!(portaria["member_id"].is_null());
    assert_eq!(listar(&app, &a).await.len(), 2);

    // A etiqueta não se apaga.
    let (st, body) = app
        .patch(
            &ext_path(a.org(), &format!("/{id}")),
            Some(&a.token),
            json!({"label": " "}),
        )
        .await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "ramais.label_required");
    let (st, body) = app
        .patch(
            &ext_path(a.org(), &format!("/{id}")),
            Some(&a.token),
            json!({"label": "Recepção — piso 0"}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["label"], "Recepção — piso 0");

    // O PIN de um ramal da empresa: o admin gera-o e vê-o uma vez…
    let dom = dominio(&app, a.org()).await;
    let (st, body) = app
        .post(
            &ext_path(a.org(), &format!("/{id}/regenerate-pin")),
            Some(&a.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    let pin = body["pin"].as_str().unwrap().to_string();
    assert_eq!(body["extension"], "2000");
    let ok = verificar(&app, &dom, "2000", &pin).await;
    assert_eq!(ok["valid"], true, "{ok}");
    assert!(ok.get("member_id").is_none(), "{ok}");
    assert_eq!(ok["display_name"], "Recepção — piso 0");
    assert!(!serde_json::to_string(&listar(&app, &a).await)
        .unwrap()
        .contains(&pin));

    // …ou escolhe-o, com as mesmas recusas…
    let (st, body) = app
        .put(
            &ext_path(a.org(), &format!("/{id}/pin")),
            Some(&a.token),
            json!({"pin": "200011"}),
        )
        .await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "ramais.pin_contains_extension");
    let (st, _) = app
        .put(
            &ext_path(a.org(), &format!("/{id}/pin")),
            Some(&a.token),
            json!({"pin": "584930"}),
        )
        .await;
    assert_eq!(st, 204);
    assert_eq!(verificar(&app, &dom, "2000", "584930").await["valid"], true);

    // …e limpa-o.
    let (st, _) = app
        .delete(&ext_path(a.org(), &format!("/{id}/pin")), Some(&a.token))
        .await;
    assert_eq!(st, 204);
    assert_eq!(estado_do_pin(&app, &a, id).await, "unset");

    // Um ramal da empresa marca o número de acesso sem rebentar o IVR da sala
    // (o caminho da R273 lia `member_id` como obrigatório): PIN de sala errado
    // é a recusa de sempre, não um 500.
    let r = app
        .raw(
            reqwest::Method::POST,
            "/internal/v1/voice/ivr/validate-extension",
            &[("x-voice-secret", VOICE_SECRET)],
            Some(json!({"sip_username": recepcao["sip_username"], "domain": dom, "pin": "000000"})),
        )
        .await;
    assert_eq!(r.status, 404, "{}", r.text);
}

#[sqlx::test(migrations = "./migrations")]
async fn atribuir_ramais_a_todos_e_idempotente_e_salta_o_reservado(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-massa.ao").await;
    let ana = app.add_member(&a, "ana", "member").await;
    let rui = app.add_member(&a, "rui", "member").await;
    let eva = app.add_member(&a, "eva", "member").await;
    let saiu = app.add_member(&a, "saiu", "member").await;
    app.archive_member(a.org(), &saiu.user_id).await;
    let range = format!("/api/orgs/{}/extension-range", a.org());
    let assign = ext_path(a.org(), "/assign-missing");

    // Por omissão 1000–1999.
    let (st, body) = app.get(&range, Some(&a.token)).await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body, json!({"range_start": 1000, "range_end": 1999}));

    for bad in [
        json!({"range_start": 99, "range_end": 200}),
        json!({"range_start": 2000, "range_end": 1000}),
        json!({"range_start": 1000, "range_end": 100000}),
    ] {
        let (st, body) = app.put(&range, Some(&a.token), bad).await;
        assert_eq!(st, 400, "{body}");
        assert_eq!(body["code"], "ramais.range_invalid");
    }

    // Um intervalo que atravessa o número de acesso (8000) e um número já
    // ocupado por um ramal da empresa (7999): 7998, 8001 e 8002 livres.
    let (st, body) = app
        .put(
            &range,
            Some(&a.token),
            json!({"range_start": 7998, "range_end": 8002}),
        )
        .await;
    assert_eq!(st, 200, "{body}");
    novo_ramal(&app, &a, None, "7999", "Portaria").await;
    // A Ana já tem ramal, fora do intervalo: não é tocada.
    novo_ramal(&app, &a, Some(&ana), "101", "").await;

    // Quatro pessoas activas (admin, ana, rui, eva); três sem ramal.
    let (st, body) = app.post(&assign, Some(&a.token), json!({})).await;
    assert_eq!(st, 200, "{body}");
    let numeros: Vec<&str> = body["assigned"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["extension"].as_str().unwrap())
        .collect();
    assert_eq!(numeros, ["7998", "8001", "8002"], "{body}");
    assert_eq!(body["already_assigned"], 1);
    assert_eq!(body["remaining"], 0);
    assert_eq!(body["range_exhausted"], false);
    // A resposta em massa não traz segredos.
    let texto = body.to_string();
    assert!(
        !texto.contains("sip_password") && !texto.contains("pin"),
        "{texto}"
    );
    let quem: Vec<&str> = body["assigned"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["member_id"].as_str().unwrap())
        .collect();
    for p in [&a, &rui, &eva] {
        assert!(quem.contains(&p.user_id.as_str()), "{quem:?}");
    }
    assert!(
        !quem.contains(&saiu.user_id.as_str()),
        "arquivado não recebe ramal"
    );
    assert!(!quem.contains(&ana.user_id.as_str()));

    // Os ramais criados em massa servem: PIN por definir, e a pessoa gera-o.
    let (st, meu) = app.get(&mine(a.org(), ""), Some(&rui.token)).await;
    assert_eq!(st, 200, "{meu}");
    assert_eq!(meu["pin_state"], "unset");
    let pin = gerar_o_meu(&app, &rui).await;
    let dom = dominio(&app, a.org()).await;
    let numero_rui = meu["extension"].as_str().unwrap();
    assert_eq!(verificar(&app, &dom, numero_rui, &pin).await["valid"], true);

    // Idempotente.
    let (st, body) = app.post(&assign, Some(&a.token), json!({})).await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["assigned"], json!([]));
    assert_eq!(body["already_assigned"], 4);
    assert_eq!(body["remaining"], 0);
    assert_eq!(listar(&app, &a).await.len(), 5);

    // Intervalo esgotado: diz quantos ficam de fora, não inventa números.
    let novo = app.add_member(&a, "novo", "member").await;
    let (st, body) = app.post(&assign, Some(&a.token), json!({})).await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["assigned"], json!([]));
    assert_eq!(body["remaining"], 1);
    assert_eq!(body["range_exhausted"], true);

    // Alargado o intervalo, a pessoa nova recebe o primeiro livre.
    let (st, _) = app
        .put(
            &range,
            Some(&a.token),
            json!({"range_start": 7998, "range_end": 8010}),
        )
        .await;
    assert_eq!(st, 200);
    let (st, body) = app.post(&assign, Some(&a.token), json!({})).await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["assigned"][0]["extension"], "8003");
    assert_eq!(body["assigned"][0]["member_id"], json!(novo.user_id));
    assert_eq!(body["remaining"], 0);

    // Só o admin; e uma só linha de auditoria por chamada que criou algo.
    let (st, _) = app.post(&assign, Some(&rui.token), json!({})).await;
    assert_eq!(st, 403);
    let (st, _) = app.get(&range, Some(&rui.token)).await;
    assert_eq!(st, 403);
    assert_eq!(
        accoes_de_auditoria(&app, a.org(), "ramal.atribuicao_em_massa")
            .await
            .len(),
        2
    );
}

/// O `FOR UPDATE` da verificação: palpites em paralelo não rendem mais do que
/// em série. Dez errados ao mesmo tempo → cinco contam, e o ramal bloqueia.
#[sqlx::test(migrations = "./migrations")]
async fn dez_palpites_em_paralelo_contam_cinco_e_bloqueiam(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-paralelo.ao").await;
    let colega = app.add_member(&a, "colega", "member").await;
    let ramal = novo_ramal(&app, &a, Some(&colega), "1004", "").await;
    let id = ramal["id"].as_str().unwrap();
    let pin = gerar_o_meu(&app, &colega).await;
    let mau = errado(&pin);
    let dom = dominio(&app, a.org()).await;

    let respostas =
        futures_util::future::join_all((0..10).map(|_| verificar(&app, &dom, "1004", mau))).await;
    assert!(
        respostas.iter().all(|r| r["valid"] == false),
        "{respostas:?}"
    );
    let por_razao = |razao: &str| respostas.iter().filter(|r| r["reason"] == razao).count();
    assert_eq!(por_razao("invalid"), 4, "{respostas:?}");
    assert_eq!(por_razao("locked"), 6, "{respostas:?}");

    assert_eq!(
        accoes_de_auditoria(&app, a.org(), "ramal.pin_falhado")
            .await
            .len(),
        5
    );
    assert_eq!(
        accoes_de_auditoria(&app, a.org(), "ramal.pin_bloqueado")
            .await
            .len(),
        1
    );
    assert_eq!(estado_do_pin(&app, &a, id).await, "locked");
    // Bloqueado: o PIN certo não passa.
    assert_eq!(
        verificar(&app, &dom, "1004", &pin).await["reason"],
        "locked"
    );
}

/// Dois administradores carregam em «atribuir ramais a todos» ao mesmo tempo:
/// nenhum número repetido, e cada pessoa com exactamente um ramal.
#[sqlx::test(migrations = "./migrations")]
async fn duas_atribuicoes_em_paralelo_nao_duplicam_nada(db: sqlx::PgPool) {
    let app = spawn(db).await;
    let a = app.new_org("alfa-corrida.ao").await;
    for nome in ["ana", "rui", "eva", "ivo", "lia", "gil"] {
        app.add_member(&a, nome, "member").await;
    }
    let assign = ext_path(a.org(), "/assign-missing");
    let (um, dois) = tokio::join!(
        app.post(&assign, Some(&a.token), json!({})),
        app.post(&assign, Some(&a.token), json!({})),
    );
    assert_eq!(um.0, 200, "{}", um.1);
    assert_eq!(dois.0, 200, "{}", dois.1);
    let criados =
        um.1["assigned"].as_array().unwrap().len() + dois.1["assigned"].as_array().unwrap().len();
    assert_eq!(
        criados, 7,
        "sete pessoas, sete ramais: {} / {}",
        um.1, dois.1
    );

    let ramais = listar(&app, &a).await;
    assert_eq!(ramais.len(), 7, "{ramais:?}");
    let numeros: std::collections::HashSet<&str> = ramais
        .iter()
        .map(|r| r["extension"].as_str().unwrap())
        .collect();
    let pessoas: std::collections::HashSet<&str> = ramais
        .iter()
        .map(|r| r["member_id"].as_str().unwrap())
        .collect();
    assert_eq!(numeros.len(), 7, "número repetido: {ramais:?}");
    assert_eq!(pessoas.len(), 7, "pessoa com dois ramais: {ramais:?}");
    assert!(numeros
        .iter()
        .all(|n| (1000..=1999).contains(&n.parse::<u32>().unwrap())));

    // E depois da corrida continua idempotente.
    let (st, body) = app.post(&assign, Some(&a.token), json!({})).await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["assigned"], json!([]));
    assert_eq!(body["remaining"], 0);
}
