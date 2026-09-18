//! gRPC interno (ADR-0006 §3) contra Postgres real: IVR, fila de transcrição
//! com reserva, DLP na entrega, e mTLS que recusa um cliente sem certificado.
mod common;

use std::{future::pending, net::SocketAddr, sync::Arc};

use common::TestApp;
use delonix_meet_protocol::{
    telephony::v1::{ivr_service_client::IvrServiceClient, ValidatePinRequest},
    transcription::v1::{
        transcription_service_client::TranscriptionServiceClient, ClaimJobRequest,
        CompleteJobRequest, FailJobRequest, TranscriptSegment,
    },
};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};
use uuid::Uuid;

async fn spawn_grpc(state: Arc<delonix_server::AppState>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        delonix_server::grpc::serve(state, listener, pending())
            .await
            .unwrap()
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    addr
}

async fn plaintext(addr: SocketAddr) -> Channel {
    Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap()
}

/// Uma gravação pronta, numa sala de uma org nova, com reunião ligada.
async fn seed_recording(app: &TestApp) -> (Uuid, String) {
    let admin = app.new_org("grpc.test").await;
    let (st, room) = app
        .post(
            "/api/rooms",
            Some(&admin.token),
            serde_json::json!({"name": "Sala"}),
        )
        .await;
    assert!(st < 300, "{room}");
    let code = room["code"].as_str().unwrap().to_string();
    let rec: Uuid = sqlx::query_scalar(
        "INSERT INTO recordings (room_id, uploader_id, filename, size_bytes)
         SELECT id, $1::uuid, 'x.webm', 10 FROM rooms WHERE code = $2 RETURNING id",
    )
    .bind(&admin.user_id)
    .bind(&code)
    .fetch_one(&app.db)
    .await
    .unwrap();
    (rec, code)
}

#[sqlx::test(migrations = "./migrations")]
async fn transcription_queue_lease_complete_and_dlp(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let (rec, _code) = seed_recording(&app).await;
    let addr = spawn_grpc(app.state.clone()).await;
    let mut c = TranscriptionServiceClient::new(plaintext(addr).await);

    let job = c
        .claim_job(ClaimJobRequest {
            worker_id: "gpu-1".into(),
            lease_seconds: 600,
        })
        .await
        .unwrap()
        .into_inner()
        .job
        .expect("havia uma gravação na fila");
    assert_eq!(job.recording_id, rec.to_string());
    assert_eq!(job.media_file, format!("{rec}.webm"));
    assert_eq!(job.attempt, 1);

    // Um segundo worker não leva a mesma.
    let second = c
        .claim_job(ClaimJobRequest {
            worker_id: "gpu-2".into(),
            lease_seconds: 600,
        })
        .await
        .unwrap()
        .into_inner();
    assert!(second.job.is_none());

    // Token errado: FAILED_PRECONDITION, e nada é gravado.
    let err = c
        .complete_job(CompleteJobRequest {
            recording_id: job.recording_id.clone(),
            lease_token: "nao-e-meu".into(),
            transcript: "x".into(),
            minutes: String::new(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::FailedPrecondition);

    let secret = format!("sk-{}", "a".repeat(32));
    c.complete_job(CompleteJobRequest {
        recording_id: job.recording_id.clone(),
        lease_token: job.lease_token.clone(),
        transcript: format!("a chave é {secret} e acabou"),
        minutes: "# Acta".into(),
        // Segmentos (R183): o DLP corre em cada um; os incoerentes saem; a
        // confiança da transcrição é a média dos que a têm.
        segments: vec![
            TranscriptSegment {
                start_ms: 2500,
                end_ms: 4000,
                text: format!("é {secret}"),
                confidence: Some(0.6),
            },
            TranscriptSegment {
                start_ms: 0,
                end_ms: 2400,
                text: "a chave".into(),
                confidence: Some(0.8),
            },
            TranscriptSegment {
                start_ms: 5000,
                end_ms: 4000,
                text: "fim antes do início".into(),
                confidence: None,
            },
        ],
        language: "pt".into(),
    })
    .await
    .unwrap();
    let (segments, language, confidence): (serde_json::Value, Option<String>, Option<f32>) =
        sqlx::query_as(
            "SELECT transcript_segments, transcript_language, transcript_confidence
               FROM recordings WHERE id = $1",
        )
        .bind(rec)
        .fetch_one(&app.db)
        .await
        .unwrap();
    let segs = segments.as_array().unwrap();
    assert_eq!(segs.len(), 2, "o segmento incoerente sai: {segments}");
    assert_eq!(segs[0]["text"], "a chave", "por ordem de início");
    assert_eq!(segs[0]["start_ms"], 0);
    assert!(
        !segments.to_string().contains(&secret),
        "o DLP corre nos segmentos: {segments}"
    );
    assert_eq!(language.as_deref(), Some("pt"));
    assert!((confidence.unwrap() - 0.7).abs() < 1e-4, "{confidence:?}");
    let (transcript, done): (String, bool) = sqlx::query_as(
        "SELECT transcript, transcribed_at IS NOT NULL FROM recordings WHERE id = $1",
    )
    .bind(rec)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(done);
    assert!(
        !transcript.contains(&secret),
        "o DLP tem de correr antes da base: {transcript}"
    );
    assert!(transcript.contains("CENSURADA"));
}

/// R230: o DLP tem de correr ANTES do corte a `MAX_SEGMENT_CHARS` (2000) —
/// se corresse depois, uma chave a atravessar essa fronteira ficava partida
/// ao meio e a expressão regular deixava de a reconhecer.
#[sqlx::test(migrations = "./migrations")]
async fn dlp_runs_before_truncating_a_segment_that_straddles_the_limit(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let (rec, _code) = seed_recording(&app).await;
    let addr = spawn_grpc(app.state.clone()).await;
    let mut c = TranscriptionServiceClient::new(plaintext(addr).await);

    let job = c
        .claim_job(ClaimJobRequest {
            worker_id: "gpu-1".into(),
            lease_seconds: 600,
        })
        .await
        .unwrap()
        .into_inner()
        .job
        .expect("havia uma gravação na fila");

    // 1970 caracteres de enchimento + uma chave de 35 — a chave começa no
    // 1970 e acaba no 2004, atravessando o corte de 2000 (por isso um corte
    // ANTES da censura apanhava só metade da chave, e a expressão regular
    // deixava de bater certo com o fragmento).
    let secret = format!("sk-{}", "a".repeat(32));
    let text = format!("{}{secret}", "x".repeat(1970));
    assert!(
        text.len() > 2000,
        "o texto tem de exceder MAX_SEGMENT_CHARS"
    );

    c.complete_job(CompleteJobRequest {
        recording_id: job.recording_id.clone(),
        lease_token: job.lease_token.clone(),
        transcript: String::new(),
        minutes: String::new(),
        segments: vec![TranscriptSegment {
            start_ms: 0,
            end_ms: 1000,
            text,
            confidence: Some(0.9),
        }],
        language: "pt".into(),
    })
    .await
    .unwrap();

    let (segments,): (serde_json::Value,) =
        sqlx::query_as("SELECT transcript_segments FROM recordings WHERE id = $1")
            .bind(rec)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(
        !segments.to_string().contains(&secret),
        "a chave sobreviveu ao corte sem ser censurada: {segments}"
    );
    assert!(
        segments.to_string().contains("CENSURADA"),
        "a censura tinha de deixar o marcador: {segments}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn failed_job_returns_to_queue_until_not_retryable(db: sqlx::PgPool) {
    let app = TestApp::spawn(db).await;
    let (rec, _) = seed_recording(&app).await;
    let addr = spawn_grpc(app.state.clone()).await;
    let mut c = TranscriptionServiceClient::new(plaintext(addr).await);
    let claim = |c: &mut TranscriptionServiceClient<Channel>| {
        let mut c = c.clone();
        async move {
            c.claim_job(ClaimJobRequest {
                worker_id: "w".into(),
                lease_seconds: 60,
            })
            .await
            .unwrap()
            .into_inner()
            .job
        }
    };
    let j = claim(&mut c).await.unwrap();
    c.fail_job(FailJobRequest {
        recording_id: j.recording_id.clone(),
        lease_token: j.lease_token,
        reason: "CUDA out of memory".into(),
        retryable: true,
    })
    .await
    .unwrap();
    let j = claim(&mut c).await.expect("retryable volta à fila");
    assert_eq!(j.attempt, 2);
    c.fail_job(FailJobRequest {
        recording_id: j.recording_id.clone(),
        lease_token: j.lease_token,
        reason: "ficheiro corrompido".into(),
        retryable: false,
    })
    .await
    .unwrap();
    assert!(claim(&mut c).await.is_none(), "não-retryable sai da fila");
    let err: Option<String> =
        sqlx::query_scalar("SELECT transcription_error FROM recordings WHERE id = $1")
            .bind(rec)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(err.as_deref(), Some("ficheiro corrompido"));
}

#[sqlx::test(migrations = "./migrations")]
async fn ivr_validate_pin_shares_the_http_rule(db: sqlx::PgPool) {
    // Telefonia desligada: UNIMPLEMENTED.
    let app = TestApp::spawn(db.clone()).await;
    let addr = spawn_grpc(app.state.clone()).await;
    let mut c = IvrServiceClient::new(plaintext(addr).await);
    let err = c
        .validate_pin(ValidatePinRequest {
            did_e164: "+244222000000".into(),
            pin: "1234".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::Unimplemented);

    let app = TestApp::spawn_with(
        db,
        &[("VOICE_INTERNAL_SECRET", "segredo-da-media-0123456789")],
    )
    .await;
    let admin = app.new_org("voz.test").await;
    let did: Uuid = sqlx::query_scalar(
        "INSERT INTO voice_did (org_id, e164) VALUES ($1::uuid, '+244222000001') RETURNING id",
    )
    .bind(admin.org())
    .fetch_one(&app.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO voice_room (org_id, room_code, pin, did_id, created_by)
         VALUES ($1::uuid, 'abc-def-ghi', '4321', $2, $3::uuid)",
    )
    .bind(admin.org())
    .bind(did)
    .bind(&admin.user_id)
    .execute(&app.db)
    .await
    .unwrap();
    let addr = spawn_grpc(app.state.clone()).await;
    let mut c = IvrServiceClient::new(plaintext(addr).await);
    let ok = c
        .validate_pin(ValidatePinRequest {
            did_e164: "+244222000001".into(),
            pin: "4321".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(ok.room_code, "abc-def-ghi");
    let err = c
        .validate_pin(ValidatePinRequest {
            did_e164: "+244222000001".into(),
            pin: "0000".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

struct Pki {
    dir: std::path::PathBuf,
    ca_pem: String,
    client_cert: String,
    client_key: String,
}

fn make_pki() -> Pki {
    use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let srv_key = KeyPair::generate().unwrap();
    let srv = CertificateParams::new(vec!["localhost".to_string()])
        .unwrap()
        .signed_by(&srv_key, &ca, &ca_key)
        .unwrap();
    let cli_key = KeyPair::generate().unwrap();
    let cli = CertificateParams::new(vec!["ai-worker".to_string()])
        .unwrap()
        .signed_by(&cli_key, &ca, &ca_key)
        .unwrap();
    let dir = std::env::temp_dir().join(format!("delonix-pki-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ca.pem"), ca.pem()).unwrap();
    std::fs::write(dir.join("server.pem"), srv.pem()).unwrap();
    std::fs::write(dir.join("server.key"), srv_key.serialize_pem()).unwrap();
    Pki {
        dir,
        ca_pem: ca.pem(),
        client_cert: cli.pem(),
        client_key: cli_key.serialize_pem(),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn mtls_accepts_client_certificate_and_refuses_anonymous(db: sqlx::PgPool) {
    let pki = make_pki();
    let p = |f: &str| pki.dir.join(f).to_str().unwrap().to_string();
    let (cert, key, ca) = (p("server.pem"), p("server.key"), p("ca.pem"));
    let app = TestApp::spawn_with(
        db,
        &[
            ("GRPC_TLS_CERT", &cert),
            ("GRPC_TLS_KEY", &key),
            ("GRPC_CLIENT_CA", &ca),
        ],
    )
    .await;
    let addr = spawn_grpc(app.state.clone()).await;
    let url = format!("https://localhost:{}", addr.port());

    let with_cert = Channel::from_shared(url.clone())
        .unwrap()
        .tls_config(
            ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(&pki.ca_pem))
                .identity(Identity::from_pem(&pki.client_cert, &pki.client_key))
                .domain_name("localhost"),
        )
        .unwrap()
        .connect()
        .await
        .expect("cliente com certificado da CA liga");
    let mut c = TranscriptionServiceClient::new(with_cert);
    let r = c
        .claim_job(ClaimJobRequest {
            worker_id: "tls".into(),
            lease_seconds: 60,
        })
        .await
        .unwrap()
        .into_inner();
    assert!(r.job.is_none());

    // Sem certificado de cliente: o handshake ou a primeira chamada falham.
    let anon = Channel::from_shared(url)
        .unwrap()
        .tls_config(
            ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(&pki.ca_pem))
                .domain_name("localhost"),
        )
        .unwrap()
        .connect()
        .await;
    let refused = match anon {
        Err(_) => true,
        Ok(ch) => TranscriptionServiceClient::new(ch)
            .claim_job(ClaimJobRequest {
                worker_id: "anon".into(),
                lease_seconds: 60,
            })
            .await
            .is_err(),
    };
    assert!(
        refused,
        "um cliente sem certificado não pode chamar o gRPC interno"
    );
    std::fs::remove_dir_all(&pki.dir).ok();
}
