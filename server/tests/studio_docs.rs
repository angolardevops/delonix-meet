//! Documentos do estúdio de TV (ADR-0014 §5) contra Postgres e servidor reais:
//! os seis tipos, o histórico de versões, a concorrência optimista, a tecla
//! única, as referências dentro do estúdio, e o isolamento entre organizações.
mod common;

use common::{TestApp, INVENTED_ID};
use serde_json::{json, Value};

async fn studio_of(app: &TestApp, a: &common::Account) -> String {
    let (st, s) = app
        .post(
            &format!("/api/orgs/{}/studios", a.org()),
            Some(&a.token),
            json!({"name": "Régie principal"}),
        )
        .await;
    assert_eq!(st, 201, "{s}");
    s["id"].as_str().unwrap().to_string()
}

/// Uma cena de som mínima mas VÁLIDA (4 bandas exactas, barramento referido).
fn mixer_body() -> Value {
    json!({
        "sample_rate": 48000, "bit_depth": 24,
        "loudness": {"target_lufs": -16, "true_peak_max_dbtp": -1},
        "master": {"level_db": -3},
        "buses": [{"id": "aux1", "name": "retorno do palco", "kind": "aux", "level_db": -8}],
        "channels": [{
            "number": 1, "name": "Microfone principal", "input": "XLR 1",
            "gain_db": 0, "fader_db": -2.1, "mute": false, "solo": false, "pan": 0,
            "buses": ["aux1"],
            "eq": {"enabled": true, "bands": [
                {"type": "low_shelf", "freq_hz": 80, "gain_db": -2, "q": 0.7},
                {"type": "peak", "freq_hz": 420, "gain_db": 1.5, "q": 1},
                {"type": "peak", "freq_hz": 2400, "gain_db": 2.5, "q": 1},
                {"type": "high_shelf", "freq_hz": 8000, "gain_db": 1, "q": 0.7}]},
            "dynamics": {
                "gate": {"enabled": true, "threshold_db": -42},
                "compressor": {"enabled": true, "threshold_db": -18, "ratio": 3.2,
                               "attack_ms": 12, "release_ms": 180},
                "limiter": {"enabled": true, "ceiling_db": -2}},
            "cleanup": {"echo_cancellation": true, "noise_reduction_db": -14,
                        "silence_removal": false, "voice_leveling": true}
        }]
    })
}

#[sqlx::test(migrations = "./migrations")]
async fn document_crud_versions_and_who_can(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let colega = app.add_member(&a, "rui", "member").await;
    let b = app.new_org("beta.ao").await;
    let studio = studio_of(&app, &a).await;
    let base = format!("/api/orgs/{}/studios/{studio}/macros", a.org());

    // Cria: versão 1, tecla normalizada, autor gravado.
    let (st, doc) = app
        .post(
            &base,
            Some(&a.token),
            json!({"name": "Abertura", "body": {"key": "F2", "steps": [
                {"action": "set-preview", "source_number": 2},
                {"action": "transition", "kind": "mix", "duration_ms": 600}]}}),
        )
        .await;
    assert_eq!(st, 201, "{doc}");
    let id = doc["id"].as_str().unwrap().to_string();
    assert_eq!(doc["version"], 1);
    assert_eq!(doc["kind"], "macro");
    assert_eq!(doc["key"], "F2");
    assert_eq!(doc["created_by"], doc["updated_by"]);
    assert!(doc["summary"].is_null(), "só o alinhamento traz summary");

    // Membro lê; não escreve (não criou o estúdio).
    let (st, page) = app.get(&base, Some(&colega.token)).await;
    assert_eq!(st, 200, "{page}");
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    let (st, e) = app
        .patch(
            &format!("{base}/{id}"),
            Some(&colega.token),
            json!({"version": 1, "name": "roubada"}),
        )
        .await;
    assert_eq!((st, e["code"].as_str()), (403, Some("studio.not_operator")));
    let (st, _) = app
        .delete(&format!("{base}/{id}"), Some(&colega.token))
        .await;
    assert_eq!(st, 403);

    // Grava: versão sobe, histórico fica com as duas.
    let (st, upd) = app
        .patch(
            &format!("{base}/{id}"),
            Some(&a.token),
            json!({"version": 1, "name": "Abertura v2"}),
        )
        .await;
    assert_eq!(st, 200, "{upd}");
    assert_eq!(upd["version"], 2);
    assert_eq!(upd["name"], "Abertura v2");
    // O corpo omisso fica como estava — não se apaga por não ser enviado.
    assert_eq!(upd["body"]["steps"].as_array().unwrap().len(), 2);

    let (st, vs) = app
        .get(&format!("{base}/{id}/versions"), Some(&colega.token))
        .await;
    assert_eq!(st, 200, "{vs}");
    let items = vs["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "mais recente primeiro: {vs}");
    assert_eq!(items[0]["version"], 2);
    assert_eq!(items[0]["name"], "Abertura v2");
    assert_eq!(items[1]["version"], 1);
    assert_eq!(items[1]["name"], "Abertura");

    // Concorrência optimista: a versão velha não grava por cima.
    let (st, e) = app
        .patch(
            &format!("{base}/{id}"),
            Some(&a.token),
            json!({"version": 1, "name": "cega"}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("studio.version_conflict")),
        "{e}"
    );

    // Outra organização: 404 em tudo, e nem pelo caminho da sua própria org.
    let (st, _) = app.get(&format!("{base}/{id}"), Some(&b.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app
        .get(
            &format!("/api/orgs/{}/studios/{studio}/macros/{id}", b.org()),
            Some(&b.token),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app.get(&base, Some(&b.token)).await;
    assert_eq!(st, 404, "a lista de outra org não é uma lista vazia, é 404");
    let (st, _) = app
        .patch(
            &format!("{base}/{id}"),
            Some(&b.token),
            json!({"version": 2, "name": "x"}),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app.delete(&format!("{base}/{id}"), Some(&b.token)).await;
    assert_eq!(st, 404);

    // Um estúdio que não existe, e um documento que não existe.
    let (st, _) = app
        .get(
            &format!("/api/orgs/{}/studios/{INVENTED_ID}/macros", a.org()),
            Some(&a.token),
        )
        .await;
    assert_eq!(st, 404);
    let (st, _) = app
        .get(&format!("{base}/{INVENTED_ID}"), Some(&a.token))
        .await;
    assert_eq!(st, 404);

    // Apaga: leva o histórico, e a segunda vez é 404.
    let (st, _) = app.delete(&format!("{base}/{id}"), Some(&a.token)).await;
    assert_eq!(st, 204);
    let (st, _) = app.delete(&format!("{base}/{id}"), Some(&a.token)).await;
    assert_eq!(st, 404);
    let (st, _) = app
        .get(&format!("{base}/{id}/versions"), Some(&a.token))
        .await;
    assert_eq!(st, 404);
}

/// R250 — o `{kind}` dos documentos é irmão de `sources`, `pairing-codes` e
/// `recording-target` no router. Se a precedência do segmento estático se
/// perder, `…/sources` passa a cair no handler de documentos e a lista de
/// fontes responde `404` (ou, pior, uma lista de documentos vazia). Nenhum
/// teste dos dois lados dava por isso sozinho: é a VIZINHANÇA que se fixa aqui.
#[sqlx::test(migrations = "./migrations")]
async fn r250_segmentos_concretos_ganham_ao_tipo_de_documento(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let studio = studio_of(&app, &a).await;
    let base = format!("/api/orgs/{}/studios/{studio}", a.org());

    // As vizinhas concretas continuam a chegar ao seu próprio handler.
    let (st, srcs) = app.get(&format!("{base}/sources"), Some(&a.token)).await;
    assert_eq!(st, 200, "…/sources tem de ser a lista de FONTES: {srcs}");
    assert!(srcs["items"].is_array(), "{srcs}");
    let (st, codes) = app
        .get(&format!("{base}/pairing-codes"), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{codes}");
    let (st, target) = app
        .get(&format!("{base}/recording-target"), Some(&a.token))
        .await;
    assert_eq!(st, 200, "{target}");
    assert_eq!(target["kind"], "local");

    // E os seis tipos de documento respondem como documentos.
    for kind in [
        "mixer-scenes",
        "macros",
        "overlays",
        "light-scenes",
        "camera-profiles",
        "rundowns",
    ] {
        let (st, page) = app.get(&format!("{base}/{kind}"), Some(&a.token)).await;
        assert_eq!(st, 200, "{kind}: {page}");
        assert_eq!(page["items"].as_array().unwrap().len(), 0);
    }

    // Um segmento que não é nem vizinho concreto nem tipo conhecido é 404 —
    // não um tipo novo criado por um caminho inventado. O valor da COLUNA
    // também não abre caminho (`mixer_scene` ≠ `mixer-scenes`).
    for bad in ["cenas", "mixer_scene", "MACROS", "sources-x"] {
        let (st, _) = app.get(&format!("{base}/{bad}"), Some(&a.token)).await;
        assert_eq!(st, 404, "{bad} tinha de ser 404");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn corpos_dos_seis_tipos_sao_validados_e_normalizados(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let studio = studio_of(&app, &a).await;
    let base = format!("/api/orgs/{}/studios/{studio}", a.org());
    let post = |kind: &'static str, name: &'static str, body: Value| {
        let url = format!("{base}/{kind}");
        let token = a.token.clone();
        let app = &app;
        async move {
            app.post(&url, Some(&token), json!({"name": name, "body": body}))
                .await
        }
    };

    // Cada tipo aceita o seu corpo do contrato.
    let (st, m) = post("mixer-scenes", "Mesa base", mixer_body()).await;
    assert_eq!(st, 201, "{m}");
    assert_eq!(m["body"]["channels"][0]["number"], 1);

    let (st, o) = post(
        "overlays",
        "Ana",
        json!({"key": "mod+1", "type": "lower-third",
               "lower_third": {"title": "Ana Mbala", "subtitle": "directora"}}),
    )
    .await;
    assert_eq!(st, 201, "{o}");
    assert_eq!(o["key"], "mod+1");

    let (st, l) = post(
        "light-scenes",
        "Cheio",
        json!({"key": "F3", "transition_ms": 1800,
               "fixtures": [{"fixture_key": "dmx:1:1", "level": 86, "cct_k": 5200}]}),
    )
    .await;
    assert_eq!(st, 201, "{l}");

    let (st, p) = post(
        "camera-profiles",
        "CAM 2",
        json!({"source_number": 2, "exposure_ev": 0.4, "temperature_k": 5200, "tint": 0,
               "contrast": 1.0, "face_enhance": "low", "noise_reduction": "off",
               "background_blur": false, "white_balance_match": true}),
    )
    .await;
    assert_eq!(st, 201, "{p}");

    // Alinhamento: o `summary` é do SERVIDOR, não do cliente.
    let (st, r) = post(
        "rundowns",
        "Noticiário",
        json!({"items": [
            {"title": "Abertura", "duration_ms": 60000, "note": "genérico", "macro_key": "F1"},
            {"title": "Entrevista", "duration_ms": 900000}]}),
    )
    .await;
    assert_eq!(st, 201, "{r}");
    assert_eq!(r["summary"]["total_duration_ms"], 960_000);
    assert_eq!(r["summary"]["item_count"], 2);

    // Recusas de forma, com campo apontado.
    let (st, e) = post("rundowns", "Vazio", json!({"items": []})).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("studio.invalid_document")),
        "{e}"
    );
    let (st, e) = post(
        "light-scenes",
        "Fora de escala",
        json!({"transition_ms": 0, "fixtures": [{"fixture_key": "dmx:1:1", "level": 140}]}),
    )
    .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("studio.invalid_document"))
    );

    // Uma cena de som com 3 bandas (o contrato exige 4 exactas).
    let mut tres = mixer_body();
    tres["channels"][0]["eq"]["bands"]
        .as_array_mut()
        .unwrap()
        .pop();
    let (st, e) = post("mixer-scenes", "Três bandas", tres).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("studio.invalid_document"))
    );

    // Um campo desconhecido no corpo NÃO é engolido.
    let mut extra = mixer_body();
    extra["reverb"] = json!({"mix": 0.3});
    let (st, e) = post("mixer-scenes", "Com reverb", extra).await;
    assert_eq!(
        (st, e["code"].as_str()),
        (400, Some("studio.invalid_document")),
        "um campo que o servidor ignora é pior do que um campo que não existe: {e}"
    );

    // Um campo desconhecido no ENVELOPE também não.
    let (st, _) = app
        .post(
            &format!("{base}/macros"),
            Some(&a.token),
            json!({"name": "x", "body": {"key": "F1", "steps": [{"action": "end-broadcast"}]},
                   "version": 7}),
        )
        .await;
    assert_eq!(st, 422);
}

#[sqlx::test(migrations = "./migrations")]
async fn tecla_unica_por_tipo_e_referencias_do_proprio_estudio(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("alfa.ao").await;
    let studio = studio_of(&app, &a).await;
    let outro = studio_of(&app, &a).await;
    let base = format!("/api/orgs/{}/studios/{studio}", a.org());

    let macro_f1 = json!({"key": "F1", "steps": [{"action": "end-broadcast"}]});
    let (st, first) = app
        .post(
            &format!("{base}/macros"),
            Some(&a.token),
            json!({"name": "Primeira", "body": macro_f1}),
        )
        .await;
    assert_eq!(st, 201, "{first}");

    // A mesma tecla no MESMO tipo e estúdio: 409.
    let (st, e) = app
        .post(
            &format!("{base}/macros"),
            Some(&a.token),
            json!({"name": "Segunda", "body": {"key": "F1",
                   "steps": [{"action": "recording", "on": true}]}}),
        )
        .await;
    assert_eq!(
        (st, e["code"].as_str()),
        (409, Some("studio.key_taken")),
        "{e}"
    );

    // A mesma tecla noutro TIPO é outra tecla — F1 de luz não choca com F1 de macro.
    let (st, luz) = app
        .post(
            &format!("{base}/light-scenes"),
            Some(&a.token),
            json!({"name": "Luz F1", "body": {"key": "F1", "transition_ms": 0,
                   "fixtures": [{"fixture_key": "hue:bridge-1:3", "level": 40}]}}),
        )
        .await;
    assert_eq!(st, 201, "{luz}");
    let luz_id = luz["id"].as_str().unwrap().to_string();

    // E no MESMO tipo noutro estúdio também não choca.
    let (st, _) = app
        .post(
            &format!("/api/orgs/{}/studios/{outro}/macros", a.org()),
            Some(&a.token),
            json!({"name": "F1 do outro", "body": macro_f1}),
        )
        .await;
    assert_eq!(st, 201);

    // Uma macro pode referir a cena de luz DESTE estúdio.
    let (st, ok) = app
        .post(
            &format!("{base}/macros"),
            Some(&a.token),
            json!({"name": "Com luz", "body": {"key": "F4",
                   "steps": [{"action": "light-scene", "document_id": luz_id}]}}),
        )
        .await;
    assert_eq!(st, 201, "{ok}");

    // Mas não a de OUTRO estúdio, nem uma inventada.
    let (st, fora) = app
        .post(
            &format!("/api/orgs/{}/studios/{outro}/macros", a.org()),
            Some(&a.token),
            json!({"name": "Rouba luz", "body": {"key": "F5",
                   "steps": [{"action": "light-scene", "document_id": luz_id}]}}),
        )
        .await;
    assert_eq!(
        (st, fora["code"].as_str()),
        (400, Some("studio.invalid_document")),
        "uma macro não refere documentos de outro estúdio: {fora}"
    );
    let (st, _) = app
        .post(
            &format!("{base}/macros"),
            Some(&a.token),
            json!({"name": "Inventada", "body": {"key": "F6",
                   "steps": [{"action": "mixer-scene", "document_id": INVENTED_ID}]}}),
        )
        .await;
    assert_eq!(st, 400);

    // Libertar a tecla liberta-a de verdade.
    let id = first["id"].as_str().unwrap();
    let (st, _) = app
        .delete(&format!("{base}/macros/{id}"), Some(&a.token))
        .await;
    assert_eq!(st, 204);
    let (st, _) = app
        .post(
            &format!("{base}/macros"),
            Some(&a.token),
            json!({"name": "Agora cabe", "body": macro_f1}),
        )
        .await;
    assert_eq!(st, 201);
}
