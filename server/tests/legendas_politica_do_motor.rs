//! A organização PROÍBE o motor de voz do browser — contra Postgres real e
//! sobre o `/ws` a sério.
//!
//! PORQUE EXISTE: até 2026-10-18 a escolha do motor de transcrição vivia
//! inteiramente no `localStorage` do browser (`dx_asr_server`,
//! `web/src/legendasPref.ts:18`), e o grep de ausência do lado do servidor dava
//! zero: `grep -rniE 'webspeech|web_speech|asr_policy' server/src` = 0. Uma
//! instalação soberana não tinha como dizer «aqui, não» — qualquer utilizador,
//! ou qualquer extensão instalada no browser dele, mudava a preferência.
//!
//! A OBSERVAÇÃO É A ORDEM DAS MENSAGENS, e é de propósito: a política tem de
//! chegar ANTES do `joined`, porque é no `joined` que o cliente entra na sala
//! e arranca o microfone (`useRoomCore`: `roomState === 'in'` →
//! `useTranscription` constrói o `Transcriber`). Uma política que chegasse
//! depois já tinha deixado a primeira frase ir à Google, e um teste que só
//! verificasse «a mensagem chegou» não distinguia os dois casos.
//!
//! A RECUSA da frase está onde ela acontece, em `signaling.rs`
//! (`legenda_da_web_speech_recusada_quando_a_organizacao_a_proibe`, com
//! controlo positivo): não precisa de base de dados para ser observada —
//! precisa dos outros participantes da sala.

mod common;

use std::time::Duration;

use common::{Account, TestApp};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

/// Uma sala desta conta. Devolve o código.
async fn sala(app: &TestApp, dono: &Account) -> String {
    let (st, room) = app
        .post(
            "/api/rooms",
            Some(&dono.token),
            json!({"name": "Reunião", "topology": "sfu"}),
        )
        .await;
    assert_eq!(st, 200, "criar sala: {room}");
    room["code"].as_str().unwrap().to_string()
}

/// Escreve a política pela MESMA rota que a consola usa.
async fn politica(app: &TestApp, dono: &Account, permitir: bool) {
    let (st, body) = app
        .patch(
            &format!("/api/orgs/{}", dono.org()),
            Some(&dono.token),
            json!({"domain": "", "retention_days": 0, "allow_web_speech": permitir}),
        )
        .await;
    assert_eq!(st, 200, "PATCH /api/orgs: {body}");
}

/// Entra na sala pelo `/ws` e devolve os tipos das mensagens recebidas ATÉ ao
/// `joined`, inclusive e por ordem. É a ordem que prova a garantia.
async fn tipos_ate_ao_joined(app: &TestApp, code: &str, quem: &Account) -> Vec<String> {
    let (st, join) = app
        .post(
            &format!("/api/rooms/{code}/join"),
            Some(&quem.token),
            json!({}),
        )
        .await;
    assert_eq!(st, 200, "{join}");
    let token = join["room_token"].as_str().unwrap();
    let url = format!(
        "{}/ws?token={token}",
        app.base.replacen("http://", "ws://", 1)
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("o /ws recusou a ligação");
    let mut tipos = Vec::new();
    for _ in 0..50 {
        let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("sem resposta do /ws")
            .expect("o socket fechou antes do joined")
            .expect("erro no socket");
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            let tipo = v["type"].as_str().unwrap_or("?").to_string();
            let fim = tipo == "joined";
            if tipo == "caption-policy" {
                assert_eq!(
                    v["web_speech"], false,
                    "a política só se envia para PROIBIR: {v}"
                );
            }
            tipos.push(tipo);
            if fim {
                return tipos;
            }
        }
    }
    panic!("o joined não chegou; vistas: {tipos:?}");
}

/// O caso que esta sessão existe para fechar.
#[sqlx::test(migrations = "./migrations")]
async fn a_politica_da_organizacao_chega_antes_de_o_microfone_arrancar(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("proibe-google.ao").await;
    let code = sala(&app, &a).await;

    politica(&app, &a, false).await;

    let tipos = tipos_ate_ao_joined(&app, &code, &a).await;
    let pol = tipos.iter().position(|t| t == "caption-policy");
    let entrou = tipos.iter().position(|t| t == "joined");
    assert!(
        pol.is_some(),
        "a organização proibiu e quem entra não foi avisado: {tipos:?}"
    );
    assert!(
        pol < entrou,
        "a política chegou DEPOIS do joined — a primeira frase já teria ido à Google: {tipos:?}"
    );

    // E lê-se de volta pela API, que é o que a consola mostra ao admin.
    let (st, orgs) = app.get("/api/orgs", Some(&a.token)).await;
    assert_eq!(st, 200, "{orgs}");
    assert_eq!(
        orgs[0]["allow_web_speech"], false,
        "o admin tem de VER a política que acabou de escrever: {orgs}"
    );

    // A auditoria registou-o com nome próprio, e não dentro de um
    // `org.settings_updated` genérico: é uma decisão de soberania do dado.
    let acoes: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_logs WHERE org_id = $1::uuid AND action LIKE 'org.web_speech%'",
    )
    .bind(a.org())
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        acoes,
        vec!["org.web_speech_forbidden".to_string()],
        "a proibição tem de ficar na cadeia de auditoria"
    );
}

/// CONTROLO POSITIVO, e é o que dá valor ao teste de cima: a MESMA montagem,
/// o mesmo caminho, sem proibição — o `caption-policy` NÃO é enviado.
///
/// Sem isto, uma implementação que proibisse sempre (um `true` fixo, ou o
/// `fail-closed` a disparar por engano) passava o teste acima e deixava todas
/// as organizações do mundo sem o motor do browser — sem ninguém notar até
/// alguém abrir uma reunião em Chrome.
#[sqlx::test(migrations = "./migrations")]
async fn sem_proibicao_nao_se_envia_politica_nenhuma(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let a = app.new_org("permite-google.ao").await;
    let code = sala(&app, &a).await;

    // Por omissão (migração 0125) a organização permite: nada a dizer.
    let tipos = tipos_ate_ao_joined(&app, &code, &a).await;
    assert!(
        !tipos.iter().any(|t| t == "caption-policy"),
        "sem proibição o silêncio é o caso normal: {tipos:?}"
    );

    // Proibir e voltar a permitir: a política não é de sentido único.
    politica(&app, &a, false).await;
    let tipos = tipos_ate_ao_joined(&app, &code, &a).await;
    assert!(tipos.iter().any(|t| t == "caption-policy"), "{tipos:?}");
    politica(&app, &a, true).await;
    let tipos = tipos_ate_ao_joined(&app, &code, &a).await;
    assert!(
        !tipos.iter().any(|t| t == "caption-policy"),
        "voltar a permitir tem de funcionar: {tipos:?}"
    );

    let acoes: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_logs WHERE org_id = $1::uuid AND action LIKE 'org.web_speech%' ORDER BY created_at",
    )
    .bind(a.org())
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(
        acoes,
        vec![
            "org.web_speech_forbidden".to_string(),
            "org.web_speech_allowed".to_string()
        ],
        "as duas decisões ficam na auditoria, por ordem"
    );
}
