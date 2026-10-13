//! A superfície MCP contra Postgres, servidor e Ollama (falso) reais
//! (ADR-0027).
//!
//! O que se prova:
//!
//! 1. **`tools/list` é o contrato, e é filtrado** — pelos escopos do token e
//!    pelas capacidades da pessoa. Uma tool anunciada é uma tool que funciona.
//! 2. **Cada uma das catorze tools responde** — é o `check-mcp-tools.sh` a
//!    exigi-lo, e é o que impede «no catálogo mas não ligada».
//! 3. **Isolamento entre organizações** — com um token da org B, as gravações
//!    e as reuniões da org A não existem. A resposta é a mesma de um id
//!    inventado.
//! 4. **Sem escopo, a tool não existe** — a mesma resposta de uma tool
//!    inventada (`-32601`), porque é o que a `tools/list` daquele token diz.
//! 5. **O transporte** — JSON-RPC: notificação sem resposta, método
//!    desconhecido, corpo que não é JSON-RPC, e `GET`/`DELETE` sem fluxo.
//! 6. **Sem token não se entra**, e o `401` leva o `WWW-Authenticate` que
//!    manda o cliente ao documento de metadata do recurso.
mod common;

use common::fake_ollama::{FakeOllama, Reply};
use common::{Account, TestApp};
use serde_json::{json, Value};
use sqlx::PgPool;

const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const PUBLIC_URL: &str = "https://meet.test";
const REDIRECT: &str = "http://127.0.0.1:33418/callback";
const MODEL: &str = "qwen2.5:1.5b";

/// Todos os escopos do catálogo — o que um agente «completo» pediria.
const TODOS: &str = "meet.read minutes.read minutes.write recordings.read captions.write chapters.write clips.write shorts.publish live.translate";

fn resource() -> String {
    format!("{PUBLIC_URL}/api/mcp")
}

fn segments() -> Value {
    json!([
        {"start_ms": 0, "end_ms": 20000, "text": "Bom dia, vamos falar da rede de Luanda."},
        {"start_ms": 20000, "end_ms": 60000, "text": "O troço do Kilamba está em manutenção."},
        {"start_ms": 60000, "end_ms": 120000, "text": "Pronto, então, decidimos adiar a migração."}
    ])
}

struct Cenario {
    app: TestApp,
    #[allow(dead_code)]
    fake: FakeOllama,
    /// Admin da org A: dono da reunião e da gravação.
    a: Account,
    /// Admin de outra organização.
    b: Account,
    meeting: String,
    rec: String,
}

/// Faz o percurso OAuth completo e devolve o access token do agente.
async fn token_de_agente(app: &TestApp, dono: &Account, escopos: &str) -> String {
    let (st, cli) = app
        .post(
            "/api/oauth/register",
            None,
            json!({"client_name": "Prova MCP", "redirect_uris": [REDIRECT]}),
        )
        .await;
    assert_eq!(st, 201, "registo: {cli}");
    let client_id = cli["client_id"].as_str().unwrap().to_string();

    let (st, r) = app
        .post(
            "/api/oauth/authorizations",
            Some(&dono.token),
            json!({
                "response_type": "code", "client_id": client_id,
                "redirect_uri": REDIRECT, "scope": escopos,
                "code_challenge": CHALLENGE, "code_challenge_method": "S256",
                "resource": resource(), "approve": true,
            }),
        )
        .await;
    assert_eq!(st, 200, "consentimento: {r}");
    let code = url::Url::parse(r["redirect_to"].as_str().unwrap())
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .expect("código");

    let (st, tok) = app
        .post(
            "/api/oauth/token",
            None,
            json!({"grant_type": "authorization_code", "client_id": client_id,
                   "code": code, "code_verifier": VERIFIER, "redirect_uri": REDIRECT}),
        )
        .await;
    assert_eq!(st, 200, "token: {tok}");
    tok["access_token"].as_str().unwrap().to_string()
}

/// Uma mensagem JSON-RPC para `/api/mcp`. Devolve `(status, corpo)`.
async fn rpc(app: &TestApp, token: &str, metodo: &str, params: Value) -> (u16, Value) {
    app.post(
        "/api/mcp",
        Some(token),
        json!({"jsonrpc": "2.0", "id": 1, "method": metodo, "params": params}),
    )
    .await
}

/// `tools/call` com o resultado já desembrulhado.
async fn chamar(app: &TestApp, token: &str, nome: &str, args: Value) -> Value {
    let (st, body) = rpc(
        app,
        token,
        "tools/call",
        json!({"name": nome, "arguments": args}),
    )
    .await;
    assert_eq!(st, 200, "{nome}: {body}");
    body
}

/// A tool correu e devolveu dados.
fn conteudo(body: &Value, nome: &str) -> Value {
    assert!(
        body.get("error").is_none(),
        "{nome} deu erro de JSON-RPC: {body}"
    );
    let r = &body["result"];
    assert_eq!(r["isError"], json!(false), "{nome} recusou: {r}");
    r["structuredContent"].clone()
}

/// A tool foi recusada pelo servidor (autorização, alcance, estado).
fn recusa(body: &Value, nome: &str) -> String {
    assert!(
        body.get("error").is_none(),
        "{nome}: era recusa de execução, não de protocolo: {body}"
    );
    let r = &body["result"];
    assert_eq!(r["isError"], json!(true), "{nome} não recusou: {r}");
    r["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

async fn cenario(db: PgPool, reply: Reply) -> Cenario {
    let fake = FakeOllama::start(&[MODEL], reply).await;
    let app = TestApp::spawn_with(
        db,
        &[
            ("PUBLIC_URL", PUBLIC_URL),
            ("OLLAMA_URL", fake.url.as_str()),
            ("OLLAMA_MODEL_SUMMARY", MODEL),
            ("OLLAMA_MODEL_TRANSLATE", MODEL),
            ("OLLAMA_MODEL_STUDIO", MODEL),
        ],
    )
    .await;
    let a = app.new_org("alfa-mcp.test").await;
    let b = app.new_org("beta-mcp.test").await;

    let meeting = app.new_meeting(&a, "Rede de Luanda", &[]).await;
    let meeting = meeting["id"].as_str().unwrap().to_string();
    // Uma acta, para a tool de leitura ter o que ler.
    let (st, _) = app
        .put(
            &format!("/api/meetings/{meeting}/minutes"),
            Some(&a.token),
            json!({"minutes": "Adiar a migração",
                   "transcript": "Bom dia, vamos falar da rede de Luanda. \
O troço do Kilamba está em manutenção desde segunda-feira e a equipa de campo \
só volta na próxima semana. Decidimos adiar a migração para Outubro, porque \
não vale a pena mexer na rede com um troço em obra. A Carla fica com o \
levantamento dos clientes afectados e o Tomás avisa o suporte."}),
        )
        .await;
    assert_eq!(st, 204);

    let room = app.new_room(&a, "gravada").await;
    let room_id = room["id"].as_str().unwrap().to_string();
    let rec = app.insert_recording(&room_id, &a.user_id).await;
    sqlx::query(
        "UPDATE recordings SET transcribed_at = now(), transcript = 'rede de Luanda',
                transcript_language = 'pt', duration_ms = 180000, transcript_segments = $2
          WHERE id = $1::uuid",
    )
    .bind(&rec)
    .bind(segments())
    .execute(&app.db)
    .await
    .unwrap();

    Cenario {
        app,
        fake,
        a,
        b,
        meeting,
        rec,
    }
}

// ---------------------------------------------------------------------------

#[sqlx::test]
async fn sem_token_nao_se_entra_e_o_401_diz_onde_autorizar(db: PgPool) {
    let app = TestApp::spawn_with(db, &[("PUBLIC_URL", PUBLIC_URL)]).await;
    let res = app
        .raw(
            reqwest::Method::POST,
            "/api/mcp",
            &[],
            Some(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})),
        )
        .await;
    assert_eq!(res.status, 401, "{}", res.text);
    let wa = res
        .header("www-authenticate")
        .expect("a especificação MCP exige WWW-Authenticate no 401");
    assert!(
        wa.contains("resource_metadata=") && wa.contains("/.well-known/oauth-protected-resource"),
        "o 401 tem de mandar o cliente ao documento de metadata: {wa}"
    );
}

#[sqlx::test]
async fn um_token_da_sessao_web_nao_entra_no_mcp(db: PgPool) {
    let c = cenario(db, Reply::Answer("x".into())).await;
    // O token do BFF tem `typ: "access"`; esta superfície exige `typ: "mcp"`.
    let (st, body) = rpc(&c.app, &c.a.token, "tools/list", json!({})).await;
    assert_eq!(st, 401, "{body}");
}

#[sqlx::test]
async fn o_transporte_e_json_rpc(db: PgPool) {
    let c = cenario(db, Reply::Answer("x".into())).await;
    let t = token_de_agente(&c.app, &c.a, TODOS).await;

    let (st, body) = rpc(&c.app, &t, "initialize", json!({})).await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(body["result"]["serverInfo"]["name"], "delonix-meet");
    // `sampling` e `elicitation` ficam declaradas ausentes de propósito.
    let caps = &body["result"]["capabilities"];
    assert!(caps["tools"].is_object(), "{caps}");
    assert!(caps.get("sampling").is_none(), "{caps}");
    assert!(caps.get("elicitation").is_none(), "{caps}");

    // Uma NOTIFICAÇÃO (sem `id`) não leva resposta.
    let res = c
        .app
        .raw(
            reqwest::Method::POST,
            "/api/mcp",
            &[("authorization", &format!("Bearer {t}"))],
            Some(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
        )
        .await;
    assert_eq!(res.status, 202, "{}", res.text);
    assert!(res.text.trim().is_empty(), "sem corpo: {}", res.text);

    // Método desconhecido.
    let (st, body) = rpc(&c.app, &t, "inventado/metodo", json!({})).await;
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["error"]["code"], -32601, "{body}");

    // Corpo que não é JSON-RPC.
    let res = c
        .app
        .raw(
            reqwest::Method::POST,
            "/api/mcp",
            &[("authorization", &format!("Bearer {t}"))],
            Some(json!({"ola": "mundo"})),
        )
        .await;
    assert_eq!(res.status, 200, "{}", res.text);
    assert_eq!(res.json()["error"]["code"], -32600);

    // Não há fluxo de eventos nem sessão para terminar.
    let res = c
        .app
        .raw(
            reqwest::Method::GET,
            "/api/mcp",
            &[("authorization", &format!("Bearer {t}"))],
            None,
        )
        .await;
    assert_eq!(res.status, 405, "{}", res.text);
}

#[sqlx::test]
async fn tools_list_e_filtrada_pelos_escopos_do_token(db: PgPool) {
    let c = cenario(db, Reply::Answer("x".into())).await;

    let completo = token_de_agente(&c.app, &c.a, TODOS).await;
    let (_, body) = rpc(&c.app, &completo, "tools/list", json!({})).await;
    let todas: Vec<String> = body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        todas.contains(&"recording_captions_translate".to_string()),
        "{todas:?}"
    );
    assert!(
        todas.contains(&"meeting_minutes_summarize".to_string()),
        "{todas:?}"
    );
    // Cada tool anunciada traz esquema e a dica de só-leitura.
    for t in body["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["annotations"]["readOnlyHint"].is_boolean(), "{t}");
        assert!(
            !t["description"].as_str().unwrap_or_default().is_empty(),
            "{t}"
        );
    }

    // Um token só de leitura não anuncia nada que escreva.
    let so_leitura = token_de_agente(&c.app, &c.a, "meet.read recordings.read").await;
    let (_, body) = rpc(&c.app, &so_leitura, "tools/list", json!({})).await;
    let nomes: Vec<String> = body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(nomes.contains(&"recordings_list".to_string()), "{nomes:?}");
    for escreve in [
        "meeting_minutes_summarize",
        "recording_captions_translate",
        "recording_chapters_generate",
    ] {
        assert!(
            !nomes.contains(&escreve.to_string()),
            "{escreve} não devia ser anunciada a um token de leitura: {nomes:?}"
        );
    }
    assert!(nomes.len() < todas.len());
}

#[sqlx::test]
async fn sem_o_escopo_a_tool_nao_existe(db: PgPool) {
    let c = cenario(db, Reply::Answer("x".into())).await;
    let so_leitura = token_de_agente(&c.app, &c.a, "meet.read").await;

    // A mesma resposta de uma tool inventada: é o que a `tools/list` deste
    // token diz, e dizer «não tens permissão» revelaria o catálogo inteiro.
    let body = chamar(
        &c.app,
        &so_leitura,
        "recording_chapters_generate",
        json!({"recording_id": c.rec}),
    )
    .await;
    assert_eq!(body["error"]["code"], -32601, "{body}");
    let body = chamar(&c.app, &so_leitura, "tool_inventada", json!({})).await;
    assert_eq!(body["error"]["code"], -32601, "{body}");
}

#[sqlx::test]
async fn as_tools_de_leitura_respondem(db: PgPool) {
    let c = cenario(db, Reply::Answer("x".into())).await;
    let t = token_de_agente(&c.app, &c.a, TODOS).await;

    let v = conteudo(
        &chamar(&c.app, &t, "meetings_list", json!({})).await,
        "meetings_list",
    );
    assert!(
        v["items"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
            || v.is_array(),
        "a lista de reuniões vem vazia: {v}"
    );

    let v = conteudo(
        &chamar(&c.app, &t, "meeting_get", json!({"meeting_id": c.meeting})).await,
        "meeting_get",
    );
    assert_eq!(v["title"], "Rede de Luanda", "{v}");

    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "meeting_minutes_get",
            json!({"meeting_id": c.meeting}),
        )
        .await,
        "meeting_minutes_get",
    );
    assert_eq!(v["minutes"], "Adiar a migração", "{v}");
    assert!(v["transcript"].as_str().unwrap().contains("Kilamba"), "{v}");

    let v = conteudo(
        &chamar(&c.app, &t, "recordings_list", json!({})).await,
        "recordings_list",
    );
    assert!(v.to_string().contains(&c.rec), "{v}");

    let v = conteudo(
        &chamar(&c.app, &t, "recording_get", json!({"recording_id": c.rec})).await,
        "recording_get",
    );
    assert_eq!(v["id"], c.rec, "{v}");

    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "recording_transcript_get",
            json!({"recording_id": c.rec}),
        )
        .await,
        "recording_transcript_get",
    );
    assert!(v.to_string().contains("Kilamba"), "{v}");

    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "recording_captions_list",
            json!({"recording_id": c.rec}),
        )
        .await,
        "recording_captions_list",
    );
    assert!(v.is_array(), "{v}");

    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "recording_chapters_list",
            json!({"recording_id": c.rec}),
        )
        .await,
        "recording_chapters_list",
    );
    assert!(v["items"].is_array(), "{v}");

    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "recording_chapters_status",
            json!({"recording_id": c.rec}),
        )
        .await,
        "recording_chapters_status",
    );
    assert_eq!(v["status"], "idle", "{v}");

    // Argumentos que não casam com o esquema são erro de PROTOCOLO.
    let body = chamar(&c.app, &t, "meeting_get", json!({})).await;
    assert_eq!(body["error"]["code"], -32602, "{body}");
}

#[sqlx::test]
async fn as_tools_que_escrevem_chamam_o_llm_local(db: PgPool) {
    // O resumo da acta: a tool devolve o ESTADO da fila, não espera pelo
    // modelo — é o que o ADR-0027 §6 manda.
    let c = cenario(db, Reply::Answer("Resumo da reunião.".into())).await;
    let t = token_de_agente(&c.app, &c.a, TODOS).await;

    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "meeting_minutes_summarize",
            json!({"meeting_id": c.meeting}),
        )
        .await,
        "meeting_minutes_summarize",
    );
    assert!(v.get("queued_at").is_some(), "o estado da fila: {v}");

    // Capítulos: o modelo responde e os capítulos ficam.
    c.fake.set_reply(Reply::Answer(
        r#"[{"start":"00:00:00","title":"Rede de Luanda"},{"start":"00:00:20","title":"Kilamba"}]"#
            .into(),
    ));
    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "recording_chapters_generate",
            json!({"recording_id": c.rec}),
        )
        .await,
        "recording_chapters_generate",
    );
    assert!(
        v.as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "capítulos: {v}"
    );

    // Legendas da transcrição, na língua dela.
    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "recording_captions_translate",
            json!({"recording_id": c.rec, "lang": "pt", "source": "transcript"}),
        )
        .await,
        "recording_captions_translate",
    );
    assert_eq!(v["lang"], "pt", "{v}");

    // E o WebVTT lê-se de volta.
    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "recording_caption_get",
            json!({"recording_id": c.rec, "lang": "pt"}),
        )
        .await,
        "recording_caption_get",
    );
    assert!(
        v["vtt"].as_str().unwrap_or_default().starts_with("WEBVTT"),
        "{v}"
    );

    // Sugestões de edição: a organização vem do TOKEN, não dos argumentos.
    // A forma que o `ai_studio` aceita para os bordões é `{"terms": [...]}`, e
    // cada termo tem de aparecer MESMO na transcrição — o serviço valida-o.
    c.fake
        .set_reply(Reply::Answer(r#"{"terms": ["pronto", "então"]}"#.into()));
    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "edit_suggestions",
            json!({"task": "fillers", "segments": segments(), "language": "pt"}),
        )
        .await,
        "edit_suggestions",
    );
    assert!(v.to_string().contains("pronto"), "{v}");
}

#[sqlx::test]
async fn as_tools_de_recorte_pedem_e_acompanham(db: PgPool) {
    let c = cenario(db, Reply::Answer("x".into())).await;
    let t = token_de_agente(&c.app, &c.a, TODOS).await;
    // A gravação precisa de ficheiro para se poder recortar: o portão é o da
    // gravação, e sem ficheiro recusa (`recording.processing`).
    sqlx::query("UPDATE recordings SET status = 'ready' WHERE id = $1::uuid")
        .bind(&c.rec)
        .execute(&c.app.db)
        .await
        .unwrap();

    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "recording_clip_create",
            json!({"recording_id": c.rec, "kind": "short", "label": "Kilamba",
                   "start_ms": 1_000, "end_ms": 9_000, "focus": "left"}),
        )
        .await,
        "recording_clip_create",
    );
    assert_eq!(v["status"], "queued", "não espera pelo ffmpeg: {v}");
    assert_eq!(v["kind"], "short", "{v}");
    let clip_id = v["id"].as_str().unwrap().to_string();

    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "recording_clips_list",
            json!({"recording_id": c.rec}),
        )
        .await,
        "recording_clips_list",
    );
    assert!(v["items"].as_array().map(|a| a.len()) == Some(1), "{v}");

    let v = conteudo(
        &chamar(
            &c.app,
            &t,
            "recording_clip_status",
            json!({"recording_id": c.rec, "clip_id": clip_id}),
        )
        .await,
        "recording_clip_status",
    );
    assert_eq!(v["id"], clip_id, "{v}");
    assert!(v["progress"].is_number(), "{v}");

    // Um pedido sem sentido é recusado pelo servidor, com o código estável.
    let texto = recusa(
        &chamar(
            &c.app,
            &t,
            "recording_clip_create",
            json!({"recording_id": c.rec, "kind": "short", "label": "longo",
                   "start_ms": 0, "end_ms": 500_000}),
        )
        .await,
        "recording_clip_create",
    );
    assert!(texto.contains("clip.too_long"), "{texto}");
}

#[sqlx::test]
async fn com_um_token_de_outra_organizacao_nada_disto_existe(db: PgPool) {
    let c = cenario(db, Reply::Answer("x".into())).await;
    // O MESMO cenário, visto por um agente autorizado pelo admin da org B.
    let t = token_de_agente(&c.app, &c.b, TODOS).await;
    let inventado = common::INVENTED_ID;

    for (nome, args) in [
        ("meeting_get", json!({"meeting_id": c.meeting})),
        ("meeting_minutes_get", json!({"meeting_id": c.meeting})),
        (
            "meeting_minutes_summarize",
            json!({"meeting_id": c.meeting}),
        ),
        ("recording_get", json!({"recording_id": c.rec})),
        ("recording_transcript_get", json!({"recording_id": c.rec})),
        ("recording_captions_list", json!({"recording_id": c.rec})),
        ("recording_chapters_list", json!({"recording_id": c.rec})),
        ("recording_chapters_status", json!({"recording_id": c.rec})),
        (
            "recording_chapters_generate",
            json!({"recording_id": c.rec}),
        ),
        (
            "recording_captions_translate",
            json!({"recording_id": c.rec, "lang": "pt", "source": "transcript"}),
        ),
        (
            "recording_caption_get",
            json!({"recording_id": c.rec, "lang": "pt"}),
        ),
    ] {
        let alheio = recusa(&chamar(&c.app, &t, nome, args.clone()).await, nome);
        // E a MESMA resposta para um id que não existe em lado nenhum: é isso
        // que impede a superfície de servir de oráculo de existência.
        let trocado = if args.get("meeting_id").is_some() {
            json!({"meeting_id": inventado})
        } else {
            let mut j = args.clone();
            j["recording_id"] = json!(inventado);
            j
        };
        let fantasma = recusa(&chamar(&c.app, &t, nome, trocado).await, nome);
        assert_eq!(
            alheio, fantasma,
            "{nome}: um recurso de outra org tem de dar a MESMA resposta que um inexistente"
        );
    }

    // As listas dele são as dele, e estão vazias.
    let v = conteudo(
        &chamar(&c.app, &t, "recordings_list", json!({})).await,
        "recordings_list",
    );
    assert!(!v.to_string().contains(&c.rec), "fuga na biblioteca: {v}");
    let v = conteudo(
        &chamar(&c.app, &t, "meetings_list", json!({})).await,
        "meetings_list",
    );
    assert!(
        !v.to_string().contains(&c.meeting),
        "fuga nas reuniões: {v}"
    );
}
