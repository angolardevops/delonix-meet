//! Inventário de captura de TV (RFC-0002, Fase 1; ADR-0031) contra Postgres
//! real: o contrato das rotas, o token que sai uma vez, o emparelhamento do
//! áudio pela PORTA, o que acontece a uma entrada que desaparece do relatório,
//! a ligação a um canal, e o isolamento entre organizações.
mod common;

use common::TestApp;
use serde_json::json;

fn agentes(org: &str) -> String {
    format!("/api/orgs/{org}/tv/capture-agents")
}

/// Caminhos físicos de duas portas do mesmo controlador, com o áudio de cada
/// uma na interface `:1.2` — a forma real do `ID_PATH` do udev.
const VIDEO_P3: &str = "pci-0000:00:14.0-usb-0:3:1.0";
const AUDIO_P3: &str = "pci-0000:00:14.0-usb-0:3:1.2";
const VIDEO_P4: &str = "pci-0000:00:14.0-usb-0:4:1.0";
const AUDIO_P4: &str = "pci-0000:00:14.0-usb-0:4:1.2";

#[sqlx::test(migrations = "./migrations")]
async fn contrato_da_caixa_e_token_que_sai_uma_vez(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;

    let res = app
        .http
        .post(app.url(&agentes(a.org())))
        .bearer_auth(&a.token)
        .json(&json!({"name": "  Caixa da sala  ", "location": "sala técnica"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let location = res.headers()["location"].to_str().unwrap().to_string();
    let c: serde_json::Value = res.json().await.unwrap();
    let id = c["id"].as_str().unwrap();
    assert_eq!(location, format!("{}/{id}", agentes(a.org())));
    assert_eq!(c["name"], "Caixa da sala", "o nome vem aparado");
    let token = c["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("dlxtv_"), "token do agente: {token}");
    assert!(c["token_prefix"].as_str().unwrap().starts_with("dlxtv_"));
    assert!(c["last_seen_at"].is_null(), "ainda não reportou nada");
    assert!(c["agent_version"].is_null());
    assert!(c["revoked_at"].is_null());

    // O token sai UMA vez: nem o detalhe nem a lista o repetem.
    let (st, got) = app.get(&location, Some(&a.token)).await;
    assert_eq!(st, 200, "{got}");
    assert!(got.get("token").is_none(), "o token não volta: {got}");
    let (st, lista) = app.get(&agentes(a.org()), Some(&a.token)).await;
    assert_eq!(st, 200);
    assert_eq!(lista["items"].as_array().unwrap().len(), 1);
    assert!(!serde_json::to_string(&lista).unwrap().contains(&token));

    // Nem a base guarda o token em claro.
    let guardado: Option<String> =
        sqlx::query_scalar("SELECT token_hash FROM tv_capture_agents WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    let guardado = guardado.unwrap();
    assert_ne!(guardado, token);
    assert_eq!(guardado.len(), 64, "SHA-256 em hexadecimal");

    let (st, patched) = app
        .patch(&location, Some(&a.token), json!({"location": "garagem"}))
        .await;
    assert_eq!(st, 200, "{patched}");
    assert_eq!(patched["location"], "garagem");
    assert_eq!(patched["name"], "Caixa da sala", "o que não vem não muda");

    let (st, _) = app
        .post(&format!("{location}/revoke"), Some(&a.token), json!({}))
        .await;
    assert_eq!(st, 200);
    let (st, revogada) = app
        .post(&format!("{location}/revoke"), Some(&a.token), json!({}))
        .await;
    assert_eq!(st, 200, "revogar duas vezes é inofensivo");
    assert!(!revogada["revoked_at"].is_null());

    let (st, _) = app.delete(&location, Some(&a.token)).await;
    assert_eq!(st, 204);
    let (st, _) = app.delete(&location, Some(&a.token)).await;
    assert_eq!(st, 404, "apagar duas vezes não finge sucesso");
}

#[sqlx::test(migrations = "./migrations")]
async fn nome_vazio_e_caminho_fisico_invalido_sao_recusados(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let (st, _) = app
        .post(&agentes(a.org()), Some(&a.token), json!({"name": "   "}))
        .await;
    assert_eq!(st, 400, "uma caixa sem nome não se escolhe na consola");

    let (_, caixa) = app
        .post(&agentes(a.org()), Some(&a.token), json!({"name": "Caixa"}))
        .await;
    let token = caixa["token"].as_str().unwrap().to_string();

    for mau in ["", "   ", "pci-0000 usb-0:3"] {
        let (st, body) = app
            .put(
                "/api/integrations/tv-agent/v1/inputs",
                Some(&token),
                json!({"inputs": [{"device_path": mau, "signal": "live"}]}),
            )
            .await;
        assert_eq!(st, 400, "«{mau}» devia ser recusado: {body}");
    }
    // Um sinal que não está no catálogo não entra como se fosse.
    let (st, body) = app
        .put(
            "/api/integrations/tv-agent/v1/inputs",
            Some(&token),
            json!({"inputs": [{"device_path": VIDEO_P3, "signal": "preto"}]}),
        )
        .await;
    assert_eq!(st, 400, "{body}");
    assert_eq!(body["code"], "tv.capture.invalid_signal");
}

#[sqlx::test(migrations = "./migrations")]
async fn o_servidor_emparelha_o_audio_pela_porta(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let (_, caixa) = app
        .post(&agentes(a.org()), Some(&a.token), json!({"name": "Caixa"}))
        .await;
    let token = caixa["token"].as_str().unwrap().to_string();

    // O agente reporta os nós de áudio FORA DE ORDEM, de propósito: quem
    // emparelha é o servidor, pela porta, e não a ordem da lista.
    let (st, resp) = app
        .put(
            "/api/integrations/tv-agent/v1/inputs",
            Some(&token),
            json!({
                "agent_version": "0.1.0",
                "audio": [
                    {"device_path": AUDIO_P4, "node": "hw:2"},
                    {"device_path": AUDIO_P3, "node": "hw:1"},
                ],
                "inputs": [
                    {"device_path": VIDEO_P3, "video_node": "/dev/video0",
                     "vendor_id": "534d", "product_id": "2130", "serial": "SN0001",
                     "signal": "live", "capabilities": {"formats": ["MJPG"]}},
                    {"device_path": VIDEO_P4, "video_node": "/dev/video2",
                     "vendor_id": "534d", "product_id": "2130", "serial": "SN0001",
                     "signal": "protected"},
                ]
            }),
        )
        .await;
    assert_eq!(st, 200, "{resp}");
    assert_eq!(resp["accepted"], 2);
    let vistas = resp["inputs"].as_array().unwrap();
    assert_eq!(vistas.len(), 2);
    let p3 = vistas
        .iter()
        .find(|i| i["device_path"] == VIDEO_P3)
        .unwrap();
    let p4 = vistas
        .iter()
        .find(|i| i["device_path"] == VIDEO_P4)
        .unwrap();
    assert_eq!(p3["audio_node"], "hw:1", "porta 3 leva o áudio da porta 3");
    assert_eq!(p4["audio_node"], "hw:2", "porta 4 leva o áudio da porta 4");
    assert!(p3["channel_id"].is_null(), "inventariada e sem uso");

    // A caixa passa a dizer a versão, e foi vista.
    let (_, detalhe) = app
        .get(
            &format!("{}/{}", agentes(a.org()), caixa["id"].as_str().unwrap()),
            Some(&a.token),
        )
        .await;
    assert_eq!(detalhe["agent_version"], "0.1.0");
    assert!(!detalhe["last_seen_at"].is_null());

    // Dois dongles do mesmo modelo, com a MESMA «série» do fabricante,
    // continuam a ser duas entradas: a chave é o caminho físico (ADR-0031 §2).
    let (st, inputs) = app
        .get(
            &format!(
                "{}/{}/inputs",
                agentes(a.org()),
                caixa["id"].as_str().unwrap()
            ),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 200);
    let items = inputs["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["serial"], items[1]["serial"]);
    assert_ne!(items[0]["device_path"], items[1]["device_path"]);
    // O HDCP tem nome próprio, e não se disfarça de avaria.
    let protegida = items.iter().find(|i| i["device_path"] == VIDEO_P4).unwrap();
    assert_eq!(protegida["signal"], "protected");
    assert!(
        protegida["last_signal_at"].is_null(),
        "nunca houve imagem nesta entrada"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn uma_entrada_que_desaparece_perde_o_sinal_e_nao_a_configuracao(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let (_, caixa) = app
        .post(&agentes(a.org()), Some(&a.token), json!({"name": "Caixa"}))
        .await;
    let token = caixa["token"].as_str().unwrap().to_string();
    let relatar = |inputs: serde_json::Value| {
        let token = token.clone();
        async move {
            app.put(
                "/api/integrations/tv-agent/v1/inputs",
                Some(&token),
                json!({"audio": [{"device_path": AUDIO_P3, "node": "hw:1"}], "inputs": inputs}),
            )
            .await
        }
    };
    let (st, resp) = relatar(json!([
        {"device_path": VIDEO_P3, "signal": "live"},
        {"device_path": VIDEO_P4, "signal": "live"}
    ]))
    .await;
    assert_eq!(st, 200, "{resp}");
    let id_p3 = resp["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["device_path"] == VIDEO_P3)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // A pessoa etiqueta a entrada e liga-a a um canal.
    let (st, canal) = app
        .post(
            &format!("/api/orgs/{}/tv/channels", a.org()),
            Some(&a.token),
            json!({"slug": "zap-sala", "name": "ZAP sala"}),
        )
        .await;
    assert_eq!(st, 201, "{canal}");
    let entrada = format!("/api/orgs/{}/tv/capture-inputs/{id_p3}", a.org());
    let (st, ligada) = app
        .patch(
            &entrada,
            Some(&a.token),
            json!({"version": 1, "label": "ZAP sala", "tuner_kind": "ir",
                   "tuner_config": {"remote": "zap-hd"}, "channel_id": canal["id"]}),
        )
        .await;
    assert_eq!(st, 200, "{ligada}");
    assert_eq!(ligada["version"], 2, "a versão avança com a alteração");
    assert_eq!(ligada["tuner_kind"], "ir");

    // Cabo solto: a entrada deixa de vir no relatório.
    let (st, _) = relatar(json!([{"device_path": VIDEO_P4, "signal": "live"}])).await;
    assert_eq!(st, 200);
    let (_, depois) = app.get(&entrada, Some(&a.token)).await;
    assert_eq!(depois["signal"], "no_signal", "perdeu o sinal");
    assert_eq!(depois["label"], "ZAP sala", "e NÃO a etiqueta");
    assert_eq!(depois["channel_id"], canal["id"], "nem o canal");
    assert_eq!(depois["tuner_kind"], "ir", "nem a sintonia");
    assert!(
        !depois["last_signal_at"].is_null(),
        "a última vez que houve imagem fica registada"
    );

    // Volta a aparecer: o mesmo id, porque a chave é a porta.
    let (_, outra_vez) = relatar(json!([
        {"device_path": VIDEO_P3, "signal": "live"},
        {"device_path": VIDEO_P4, "signal": "live"}
    ]))
    .await;
    let voltou = outra_vez["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["device_path"] == VIDEO_P3)
        .unwrap();
    assert_eq!(voltou["id"].as_str().unwrap(), id_p3, "a mesma entrada");
    assert_eq!(voltou["channel_id"], canal["id"]);
}

#[sqlx::test(migrations = "./migrations")]
async fn versao_velha_canal_de_outra_org_e_canal_ja_alimentado(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;
    let (_, caixa) = app
        .post(&agentes(a.org()), Some(&a.token), json!({"name": "Caixa"}))
        .await;
    let token = caixa["token"].as_str().unwrap().to_string();
    let (_, resp) = app
        .put(
            "/api/integrations/tv-agent/v1/inputs",
            Some(&token),
            json!({"inputs": [
                {"device_path": VIDEO_P3, "signal": "live"},
                {"device_path": VIDEO_P4, "signal": "live"}
            ]}),
        )
        .await;
    let ids: Vec<String> = resp["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect();
    let (e1, e2) = (
        format!("/api/orgs/{}/tv/capture-inputs/{}", a.org(), ids[0]),
        format!("/api/orgs/{}/tv/capture-inputs/{}", a.org(), ids[1]),
    );

    // Versão velha: `409` e nada se escreve.
    let (st, conflito) = app
        .patch(&e1, Some(&a.token), json!({"version": 99, "label": "x"}))
        .await;
    assert_eq!(st, 409, "{conflito}");
    assert_eq!(conflito["code"], "tv.capture.version_conflict");
    let (_, intacta) = app.get(&e1, Some(&a.token)).await;
    assert_eq!(intacta["label"], "");

    // Um canal da org B é indistinguível de um que não existe.
    let (_, canal_b) = app
        .post(
            &format!("/api/orgs/{}/tv/channels", b.org()),
            Some(&b.token),
            json!({"slug": "tv-beta", "name": "TV Beta"}),
        )
        .await;
    let (st, _) = app
        .patch(
            &e1,
            Some(&a.token),
            json!({"version": 1, "channel_id": canal_b["id"]}),
        )
        .await;
    assert_eq!(st, 404, "o canal de outra org não se confirma nem se liga");

    // Dois sinais no mesmo canal era sinal a lutar com sinal: `409`.
    let (_, canal_a) = app
        .post(
            &format!("/api/orgs/{}/tv/channels", a.org()),
            Some(&a.token),
            json!({"slug": "tv-alfa", "name": "TV Alfa"}),
        )
        .await;
    let (st, _) = app
        .patch(
            &e1,
            Some(&a.token),
            json!({"version": 1, "channel_id": canal_a["id"]}),
        )
        .await;
    assert_eq!(st, 200);
    let (st, dupla) = app
        .patch(
            &e2,
            Some(&a.token),
            json!({"version": 1, "channel_id": canal_a["id"]}),
        )
        .await;
    assert_eq!(st, 409, "{dupla}");
    assert_eq!(dupla["code"], "tv.capture.channel_taken");
}

#[sqlx::test(migrations = "./migrations")]
async fn outra_org_membro_simples_e_token_revogado_sao_recusados(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let b = app.new_org("beta.ao").await;
    let (_, caixa_b) = app
        .post(
            &agentes(b.org()),
            Some(&b.token),
            json!({"name": "Caixa da B"}),
        )
        .await;
    let id_b = caixa_b["id"].as_str().unwrap();
    let token_b = caixa_b["token"].as_str().unwrap().to_string();

    // A organização A não chega a nada da B — e a resposta não confirma nada.
    for (metodo, caminho) in [
        ("GET", agentes(b.org())),
        ("GET", format!("{}/{id_b}", agentes(b.org()))),
        ("GET", format!("{}/{id_b}/inputs", agentes(b.org()))),
    ] {
        let (st, body) = match metodo {
            "GET" => app.get(&caminho, Some(&a.token)).await,
            _ => unreachable!(),
        };
        common::assert_denied(&caminho, st, &body, id_b);
    }
    let (st, _) = app
        .post(
            &agentes(b.org()),
            Some(&a.token),
            json!({"name": "intrusa"}),
        )
        .await;
    assert!(st == 403 || st == 404, "criar na org B: {st}");
    let (st, _) = app
        .delete(&format!("{}/{id_b}", agentes(b.org())), Some(&a.token))
        .await;
    assert!(st == 403 || st == 404, "apagar da org B: {st}");
    // Pelo caminho da própria org A, com o id da B.
    let (st, _) = app
        .get(&format!("{}/{id_b}", agentes(a.org())), Some(&a.token))
        .await;
    assert_eq!(st, 404);

    // Um membro simples não gere caixas de captura (é preparar um canal).
    let membro = app.add_member(&b, "zeca", "member").await;
    let (st, _) = app.get(&agentes(b.org()), Some(&membro.token)).await;
    assert!(st == 403 || st == 404, "membro simples: {st}");

    // O token do agente não é sessão: não abre a consola de ninguém.
    let (st, _) = app.get(&agentes(b.org()), Some(&token_b)).await;
    assert_eq!(st, 401);

    // Revogado, o token do agente deixa de reportar.
    let (st, _) = app
        .post(
            &format!("{}/{id_b}/revoke", agentes(b.org())),
            Some(&b.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200);
    let (st, _) = app
        .put(
            "/api/integrations/tv-agent/v1/inputs",
            Some(&token_b),
            json!({"inputs": [{"device_path": VIDEO_P3, "signal": "live"}]}),
        )
        .await;
    assert_eq!(st, 401, "um token revogado não reporta mais nada");
}
