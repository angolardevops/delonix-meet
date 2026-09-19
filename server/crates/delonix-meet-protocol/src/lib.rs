//! # delonix-meet-protocol
//!
//! Tipos e stubs gerados a partir dos `.proto` em `server/proto/`
//! (pacotes versionados `delonix.meet.<serviço>.v1`). Um campo removido fica
//! `reserved`; `buf breaking` no CI impede a quebra do contrato.

pub mod telephony {
    pub mod v1 {
        tonic::include_proto!("delonix.meet.telephony.v1");
    }
}

pub mod transcription {
    pub mod v1 {
        tonic::include_proto!("delonix.meet.transcription.v1");
    }
}

/// Descritor de todos os serviços, para o gRPC reflection (`grpcurl`).
pub const FILE_DESCRIPTOR_SET: &[u8] =
    tonic::include_file_descriptor_set!("delonix_meet_descriptor");
