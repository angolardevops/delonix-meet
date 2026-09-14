//! Backend de storage em disco local — omissão segura para on-premise. Um
//! mount NFS entra na mesma implementação: do ponto de vista do processo é
//! só um directório, montado pelo SO/K8s antes de o servidor arrancar.

use axum::body::Bytes;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::domain::ports::{RecordingStorage, StorageError};

pub struct LocalFsStorage {
    base_dir: PathBuf,
}

impl LocalFsStorage {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn path_for(&self, id: Uuid) -> PathBuf {
        self.base_dir.join(format!("{id}.webm"))
    }
}

#[async_trait::async_trait]
impl RecordingStorage for LocalFsStorage {
    async fn put(&self, id: Uuid, bytes: Bytes) -> Result<(), StorageError> {
        tokio::fs::create_dir_all(&self.base_dir).await?;
        tokio::fs::write(self.path_for(id), &bytes).await?;
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Bytes, StorageError> {
        tokio::fs::read(self.path_for(id))
            .await
            .map(Bytes::from)
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => StorageError::NotFound,
                _ => StorageError::Io(e),
            })
    }

    async fn delete(&self, id: Uuid) -> Result<(), StorageError> {
        match tokio::fs::remove_file(self.path_for(id)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn put_from_path(&self, id: Uuid, path: &Path) -> Result<(), StorageError> {
        tokio::fs::create_dir_all(&self.base_dir).await?;
        let dest = self.path_for(id);
        // rename falha com EXDEV (errno 18) se `path` e `dest` estiverem em
        // filesystems diferentes (ex.: tmp num volume separado). Fallback:
        // copy+delete — a mesma lógica que estava em recorder.rs antes desta
        // porta existir.
        match tokio::fs::rename(path, &dest).await {
            Ok(()) => Ok(()),
            Err(e) if e.raw_os_error() == Some(18) => {
                tokio::fs::copy(path, &dest).await?;
                let _ = tokio::fs::remove_file(path).await;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("delonix-test-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn put_then_get_roundtrip() {
        let storage = LocalFsStorage::new(tmp_dir());
        let id = Uuid::new_v4();
        storage
            .put(id, Bytes::from_static(b"conteudo"))
            .await
            .unwrap();
        let got = storage.get(id).await.unwrap();
        assert_eq!(&got[..], b"conteudo");
    }

    #[tokio::test]
    async fn get_missing_is_not_found() {
        let storage = LocalFsStorage::new(tmp_dir());
        let err = storage.get(Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(err, StorageError::NotFound));
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let storage = LocalFsStorage::new(tmp_dir());
        let id = Uuid::new_v4();
        storage.delete(id).await.unwrap();
        storage.put(id, Bytes::from_static(b"x")).await.unwrap();
        storage.delete(id).await.unwrap();
        storage.delete(id).await.unwrap();
        assert!(matches!(
            storage.get(id).await.unwrap_err(),
            StorageError::NotFound
        ));
    }

    #[tokio::test]
    async fn put_from_path_moves_local_file() {
        let base = tmp_dir();
        tokio::fs::create_dir_all(&base).await.unwrap();
        let src = base.join("out.webm");
        tokio::fs::write(&src, b"ffmpeg-output").await.unwrap();

        let storage = LocalFsStorage::new(base.join("final"));
        let id = Uuid::new_v4();
        storage.put_from_path(id, &src).await.unwrap();

        assert!(!src.exists());
        let got = storage.get(id).await.unwrap();
        assert_eq!(&got[..], b"ffmpeg-output");
    }
}
