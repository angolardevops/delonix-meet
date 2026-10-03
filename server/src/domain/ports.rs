//! Portas: traits que a camada de aplicação usa e a infraestrutura implementa
//! (Dependency Inversion — ver ADR-0004). Trocar de backend passa a ser uma
//! escolha de configuração no arranque (`main.rs`), nunca uma edição ao
//! módulo que usa a porta.

use axum::body::Bytes;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("recording not found in storage")]
    NotFound,
    #[error("storage io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Onde os bytes de uma gravação vivem. `LocalFsStorage`
/// (`infrastructure::storage`) é a omissão segura para on-premise — disco
/// local ou um mount de rede (NFS) são o mesmo do ponto de vista do
/// processo. Um backend WebDAV/S3 implementa a mesma porta sem o resto do
/// sistema saber a diferença.
#[async_trait::async_trait]
pub trait RecordingStorage: Send + Sync {
    async fn put(&self, id: Uuid, bytes: Bytes) -> Result<(), StorageError>;
    async fn get(&self, id: Uuid) -> Result<Bytes, StorageError>;
    /// Idempotente: apagar o que já não existe não é erro.
    async fn delete(&self, id: Uuid) -> Result<(), StorageError>;

    /// Publica um ficheiro que o chamador já tem em disco local (ex.: a
    /// saída do `ffmpeg` em `recorder.rs`) sem o materializar em memória
    /// duas vezes. A omissão lê e escreve pelos métodos acima; um backend
    /// local sobrepõe isto com um `rename` atómico (ver `LocalFsStorage`).
    async fn put_from_path(&self, id: Uuid, path: &Path) -> Result<(), StorageError> {
        let bytes = tokio::fs::read(path).await.map(Bytes::from)?;
        self.put(id, bytes).await?;
        let _ = tokio::fs::remove_file(path).await;
        Ok(())
    }
}
