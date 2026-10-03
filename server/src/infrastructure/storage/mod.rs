mod local_fs;

pub use local_fs::LocalFsStorage;

// WebDavStorage fica para uma fase seguinte: hoje `storage.rs` só guarda a
// configuração NFS/WebDAV e testa conectividade — não existe nenhum caminho
// de código que leia/escreva bytes de gravação por WebDAV (confirmado por
// leitura de `recordings.rs`/`recorder.rs`). Implementar isso é capacidade
// nova, não um refactor; ver ADR-0004, secção "Fora de escopo".
