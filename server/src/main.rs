//! Binário do Delonix Meet. Toda a lógica está na biblioteca (`lib.rs`).

#[tokio::main]
async fn main() {
    delonix_server::run().await;
}
