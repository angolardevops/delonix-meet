//! gRPC INTERNO (ADR-0004 §4, ADR-0006 §3): máquina-a-máquina, numa porta sem
//! ingress (`GRPC_BIND_ADDR`), com mTLS.
//!
//! Os serviços são adaptadores finos: convertem mensagens e chamam as MESMAS
//! funções que o HTTP (`voice::validate_pin`, `transcription::claim`, …). Uma
//! regra nunca tem uma segunda implementação aqui.
//!
//! Não há gRPC para o browser (ADR-0004 §4).

use std::sync::Arc;

use delonix_meet_core::ErrorKind;
use delonix_meet_protocol::{
    telephony::v1::{
        ivr_service_server::{IvrService, IvrServiceServer},
        RecordCallDetailRequest, RecordCallDetailResponse, ValidatePinRequest, ValidatePinResponse,
    },
    transcription::v1::{
        transcription_service_server::{TranscriptionService, TranscriptionServiceServer},
        ClaimJobRequest, ClaimJobResponse, CompleteJobRequest, CompleteJobResponse, FailJobRequest,
        FailJobResponse, TranscriptionJob,
    },
};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{error::ApiError, AppState};

/// Tradução canónica erro → estado gRPC, a par de `error::status_of` para HTTP.
pub fn status_from(e: ApiError) -> Status {
    let (code, msg) = match &e {
        ApiError::BadRequest(m) => (tonic::Code::InvalidArgument, m.clone()),
        ApiError::Unauthorized => (tonic::Code::Unauthenticated, "unauthenticated".into()),
        ApiError::Forbidden => (tonic::Code::PermissionDenied, "permission denied".into()),
        ApiError::Conflict(m) => (tonic::Code::AlreadyExists, m.clone()),
        ApiError::Unprocessable(m) => (tonic::Code::FailedPrecondition, m.clone()),
        ApiError::NotFound => (tonic::Code::NotFound, "not found".into()),
        ApiError::TooManyRequests => (tonic::Code::ResourceExhausted, "too many requests".into()),
        ApiError::ServiceUnavailable(m) => (tonic::Code::Unavailable, m.clone()),
        ApiError::Internal(detail) => {
            tracing::error!(error = %detail, "internal error (grpc)");
            (tonic::Code::Internal, "internal error".into())
        }
        ApiError::Domain(d) => {
            let code = match d.kind {
                ErrorKind::InvalidArgument => tonic::Code::InvalidArgument,
                ErrorKind::FailedPrecondition => tonic::Code::FailedPrecondition,
                ErrorKind::Unauthenticated => tonic::Code::Unauthenticated,
                ErrorKind::PermissionDenied => tonic::Code::PermissionDenied,
                ErrorKind::NotFound => tonic::Code::NotFound,
                ErrorKind::Conflict => tonic::Code::AlreadyExists,
                ErrorKind::ResourceExhausted => tonic::Code::ResourceExhausted,
                ErrorKind::Unavailable => tonic::Code::Unavailable,
                ErrorKind::Internal => tonic::Code::Internal,
            };
            let msg = if d.kind == ErrorKind::Internal {
                tracing::error!(error = %d.message, "internal error (grpc)");
                "internal error".to_string()
            } else {
                format!("{}: {}", d.code, d.message)
            };
            (code, msg)
        }
    };
    Status::new(code, msg)
}

fn parse_uuid(field: &str, v: &str) -> Result<Uuid, Status> {
    v.parse()
        .map_err(|_| Status::invalid_argument(format!("{field} não é um UUID")))
}

pub struct Ivr {
    pub state: Arc<AppState>,
}

#[tonic::async_trait]
impl IvrService for Ivr {
    async fn validate_pin(
        &self,
        req: Request<ValidatePinRequest>,
    ) -> Result<Response<ValidatePinResponse>, Status> {
        if self.state.config.voice_internal_secret.is_empty() {
            return Err(Status::unimplemented(
                "telefonia desligada nesta instalação",
            ));
        }
        let r = req.into_inner();
        let out = crate::voice::validate_pin(&self.state, &r.did_e164, &r.pin)
            .await
            .map_err(status_from)?;
        Ok(Response::new(ValidatePinResponse {
            voice_room_id: out.voice_room_id.to_string(),
            room_code: out.room_code,
            media_backend: out.media_backend,
        }))
    }

    async fn record_call_detail(
        &self,
        req: Request<RecordCallDetailRequest>,
    ) -> Result<Response<RecordCallDetailResponse>, Status> {
        if self.state.config.voice_internal_secret.is_empty() {
            return Err(Status::unimplemented(
                "telefonia desligada nesta instalação",
            ));
        }
        let r = req.into_inner();
        let cdr = crate::voice::CdrReq {
            voice_room_id: parse_uuid("voice_room_id", &r.voice_room_id)?,
            direction: if r.direction.is_empty() {
                crate::voice::inbound()
            } else {
                r.direction
            },
            caller_number: r.caller_number,
            did_e164: r.did_e164,
            duration_secs: r.duration_secs,
        };
        let (id, cost) = crate::voice::record_cdr(&self.state, &cdr)
            .await
            .map_err(status_from)?;
        Ok(Response::new(RecordCallDetailResponse {
            id: id.to_string(),
            cost_estimate: cost,
        }))
    }
}

pub struct Transcription {
    pub state: Arc<AppState>,
}

#[tonic::async_trait]
impl TranscriptionService for Transcription {
    async fn claim_job(
        &self,
        req: Request<ClaimJobRequest>,
    ) -> Result<Response<ClaimJobResponse>, Status> {
        let r = req.into_inner();
        let job = crate::transcription::claim(&self.state, &r.worker_id, r.lease_seconds)
            .await
            .map_err(status_from)?;
        Ok(Response::new(ClaimJobResponse {
            job: job.map(|j| TranscriptionJob {
                recording_id: j.recording_id.to_string(),
                lease_token: j.lease_token,
                media_file: j.media_file,
                room_code: j.room_code,
                lease_expires_unix: j.lease_expires_unix,
                attempt: j.attempt,
            }),
        }))
    }

    async fn complete_job(
        &self,
        req: Request<CompleteJobRequest>,
    ) -> Result<Response<CompleteJobResponse>, Status> {
        let r = req.into_inner();
        let id = parse_uuid("recording_id", &r.recording_id)?;
        let segments = r
            .segments
            .into_iter()
            .map(|s| delonix_meet_domain::content::transcription::Segment {
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                text: s.text,
                confidence: s.confidence,
            })
            .collect();
        let delivery = crate::transcription::Delivery {
            transcript: &r.transcript,
            minutes: &r.minutes,
            segments,
            language: &r.language,
        };
        crate::transcription::complete(&self.state, id, &r.lease_token, delivery)
            .await
            .map_err(status_from)?;
        Ok(Response::new(CompleteJobResponse {}))
    }

    async fn fail_job(
        &self,
        req: Request<FailJobRequest>,
    ) -> Result<Response<FailJobResponse>, Status> {
        let r = req.into_inner();
        let id = parse_uuid("recording_id", &r.recording_id)?;
        crate::transcription::fail(&self.state, id, &r.lease_token, &r.reason, r.retryable)
            .await
            .map_err(status_from)?;
        Ok(Response::new(FailJobResponse {}))
    }
}

/// Configuração TLS do listener. mTLS é obrigatório; sem certificados só se
/// arranca em texto claro com `DELONIX_ALLOW_INSECURE=1` (desenvolvimento).
fn tls_config(
    config: &crate::config::Config,
) -> Result<Option<tonic::transport::ServerTlsConfig>, String> {
    use tonic::transport::{Certificate, Identity, ServerTlsConfig};
    match (&config.grpc_tls_cert, &config.grpc_tls_key, &config.grpc_client_ca) {
        (Some(cert), Some(key), Some(ca)) => {
            let read = |p: &str| std::fs::read(p).map_err(|e| format!("{p}: {e}"));
            Ok(Some(
                ServerTlsConfig::new()
                    .identity(Identity::from_pem(read(cert)?, read(key)?))
                    .client_ca_root(Certificate::from_pem(read(ca)?)),
            ))
        }
        (None, None, None) if config.allow_insecure => Ok(None),
        (None, None, None) => Err(
            "GRPC_BIND_ADDR sem GRPC_TLS_CERT/GRPC_TLS_KEY/GRPC_CLIENT_CA — o gRPC interno exige mTLS \
             (texto claro só com DELONIX_ALLOW_INSECURE=1)"
                .into(),
        ),
        _ => Err("mTLS do gRPC incompleto: são precisos os três GRPC_TLS_CERT, GRPC_TLS_KEY e GRPC_CLIENT_CA".into()),
    }
}

/// Monta o router gRPC: serviços, health (`grpc.health.v1`) e reflection.
pub async fn serve(
    state: Arc<AppState>,
    listener: tokio::net::TcpListener,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), String> {
    let (health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter.set_serving::<IvrServiceServer<Ivr>>().await;
    health_reporter
        .set_serving::<TranscriptionServiceServer<Transcription>>()
        .await;
    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(delonix_meet_protocol::FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
        .build_v1()
        .map_err(|e| e.to_string())?;

    let mut builder = tonic::transport::Server::builder();
    if let Some(tls) = tls_config(&state.config)? {
        builder = builder.tls_config(tls).map_err(|e| e.to_string())?;
    } else {
        tracing::warn!(
            "gRPC interno em TEXTO CLARO (DELONIX_ALLOW_INSECURE=1) — nunca em produção"
        );
    }
    builder
        .add_service(health_service)
        .add_service(reflection)
        .add_service(IvrServiceServer::new(Ivr {
            state: state.clone(),
        }))
        .add_service(TranscriptionServiceServer::new(Transcription { state }))
        .serve_with_incoming_shutdown(
            tokio_stream::wrappers::TcpListenerStream::new(listener),
            shutdown,
        )
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use delonix_meet_core::DomainError;

    #[test]
    fn domain_errors_keep_their_class_over_grpc() {
        let s = status_from(ApiError::Domain(DomainError::precondition(
            "transcription.lease_lost",
            "expirou",
        )));
        assert_eq!(s.code(), tonic::Code::FailedPrecondition);
        assert!(s.message().starts_with("transcription.lease_lost"));
        let s = status_from(ApiError::internal("segredo"));
        assert_eq!(s.code(), tonic::Code::Internal);
        assert!(!s.message().contains("segredo"));
    }
}
