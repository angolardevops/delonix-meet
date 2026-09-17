//! Pedidos de saída: o único dono dos clientes HTTP do servidor.
//!
//! Há dois clientes, porque há dois donos de URL (a regra está em
//! `delonix_meet_core::egress`):
//!
//! - [`Outbound::tenant`] — URLs escritos por um cliente: webhooks, `odoo_url`
//!   da organização, emissor OIDC. Só destinos públicos, salvo os nomes de
//!   `OUTBOUND_ALLOW_HOSTS` (e o host do `PLATFORM_ODOO_URL`, que o operador já
//!   declarou ao configurá-lo).
//! - [`Outbound::operator`] — URLs do operador: WebDAV, Ollama. Rede privada
//!   permitida; metadados da cloud nunca.
//!
//! # Onde a guarda actua
//!
//! 1. **Na resolução de DNS de CADA ligação** ([`GuardedResolver`]). Validar o
//!    URL e depois deixar o `reqwest` resolver outra vez era a janela do DNS
//!    rebinding: o nome respondia um IP público ao teste e `169.254.169.254` à
//!    ligação. Aqui o IP que se valida é o IP a que se liga.
//! 2. **No URL**, com [`Outbound::check_tenant_url`] / [`Outbound::check_operator_url`]
//!    — obrigatório antes de usar o cliente, porque um host que já é um IP
//!    literal não passa pelo resolver, e porque um erro 400 com razão ao GRAVAR
//!    a configuração é melhor do que uma integração que falha calada depois.
//! 3. **Sem redirects**, nos dois clientes. Um `302` para um IP literal interno
//!    saltaria as duas guardas acima; nenhum dos destinos legítimos (webhook,
//!    JSON-RPC do Odoo, descoberta OIDC, PROPFIND) precisa de os seguir.
//!
//! Timeout de ligação e total em todos: um destino que não responde não pode
//! prender um pedido do utilizador (o login do Odoo e a descoberta OIDC correm
//! no caminho do login).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use delonix_meet_core::egress::{host_is_allowlisted, EgressPolicy};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{Client, Url};

use crate::error::ApiError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(15);

/// Resolve e descarta os endereços que a política não permite. Se não sobrar
/// nenhum, a ligação falha — nunca se liga a um IP recusado.
struct GuardedResolver {
    policy: EgressPolicy,
    allow_hosts: Arc<[String]>,
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let policy = self.policy;
        let allow_hosts = self.allow_hosts.clone();
        Box::pin(async move {
            let host = name.as_str().to_string();
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .collect();
            let allowed: Vec<SocketAddr> = if host_is_allowlisted(&host, &allow_hosts) {
                addrs
            } else {
                addrs.into_iter().filter(|a| policy.allows(a.ip())).collect()
            };
            if allowed.is_empty() {
                return Err(format!("{host}: destino recusado pela guarda de saída").into());
            }
            Ok(Box::new(allowed.into_iter()) as Addrs)
        })
    }
}

fn build_client(policy: EgressPolicy, allow_hosts: Arc<[String]>) -> Client {
    Client::builder()
        .dns_resolver(Arc::new(GuardedResolver {
            policy,
            allow_hosts,
        }))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TOTAL_TIMEOUT)
        .build()
        .expect("falha ao criar o cliente HTTP de saída")
}

/// Os clientes de saída partilhados (um pool de ligações por política).
#[derive(Clone)]
pub struct Outbound {
    tenant: Client,
    operator: Client,
    allow_hosts: Arc<[String]>,
}

impl Outbound {
    pub fn new(allow_hosts: Vec<String>) -> Self {
        let allow_hosts: Arc<[String]> = allow_hosts.into();
        Self {
            tenant: build_client(EgressPolicy::Tenant, allow_hosts.clone()),
            operator: build_client(EgressPolicy::Operator, allow_hosts.clone()),
            allow_hosts,
        }
    }

    /// Cliente para URLs escritos por um cliente. Antes de o usar:
    /// [`Self::check_tenant_url`].
    pub fn tenant(&self) -> &Client {
        &self.tenant
    }

    /// Cliente para URLs configurados pelo operador. Antes de o usar:
    /// [`Self::check_operator_url`].
    pub fn operator(&self) -> &Client {
        &self.operator
    }

    /// Antes de LIGAR a um URL escrito por um cliente: tem de resolver, e só
    /// para endereços públicos.
    pub async fn check_tenant_url(&self, raw: &str) -> Result<Url, ApiError> {
        check_url(raw, EgressPolicy::Tenant, &self.allow_hosts, Resolution::Required).await
    }

    /// Ao GRAVAR um URL escrito por um cliente (`odoo_url`, emissor OIDC). Um
    /// nome que ainda não resolve é aceite — o DNS pode vir depois da
    /// configuração, e a guarda da ligação é a autoridade —, mas um nome ou IP
    /// literal que resolve para um endereço interno é recusado já, com razão.
    pub async fn check_tenant_config_url(&self, raw: &str) -> Result<Url, ApiError> {
        check_url(raw, EgressPolicy::Tenant, &self.allow_hosts, Resolution::Optional).await
    }

    /// Antes de ligar a um URL do operador.
    pub async fn check_operator_url(&self, raw: &str) -> Result<Url, ApiError> {
        check_url(raw, EgressPolicy::Operator, &self.allow_hosts, Resolution::Required).await
    }

    /// Ao gravar um URL do operador (WebDAV). Ver [`Self::check_tenant_config_url`].
    pub async fn check_operator_config_url(&self, raw: &str) -> Result<Url, ApiError> {
        check_url(raw, EgressPolicy::Operator, &self.allow_hosts, Resolution::Optional).await
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Resolution {
    Required,
    Optional,
}

/// Valida um URL de saída: `http`/`https`, sem credenciais embutidas, e todos
/// os endereços para que resolve permitidos pela política. A isenção é pelo
/// HOST do URL, não pelo IP resolvido.
async fn check_url(
    raw: &str,
    policy: EgressPolicy,
    allow_hosts: &[String],
    resolution: Resolution,
) -> Result<Url, ApiError> {
    let url = Url::parse(raw.trim()).map_err(|_| ApiError::BadRequest("URL inválido".into()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApiError::BadRequest("esquema de URL inválido".into()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ApiError::BadRequest("URL não pode conter credenciais".into()));
    }
    let host = url
        .host_str()
        .ok_or_else(|| ApiError::BadRequest("URL sem host".into()))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if host_is_allowlisted(&host, allow_hosts) {
        return Ok(url);
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<SocketAddr> = match tokio::net::lookup_host((host.as_str(), port)).await {
        Ok(a) => a.collect(),
        Err(_) => Vec::new(),
    };
    if addrs.is_empty() {
        return match resolution {
            Resolution::Required => Err(ApiError::BadRequest("host do URL não resolve".into())),
            Resolution::Optional => Ok(url),
        };
    }
    if addrs.iter().any(|a| !policy.allows(a.ip())) {
        return Err(ApiError::BadRequest(
            "o URL aponta para um endereço interno/privado".into(),
        ));
    }
    Ok(url)
}

/// Erro do cliente HTTP do `openidconnect` guardado ([`Outbound::oidc_call`]).
#[derive(Debug, thiserror::Error)]
pub enum OidcHttpError {
    #[error("destino OIDC recusado: {0}")]
    Blocked(String),
    #[error(transparent)]
    Http(#[from] openidconnect::HttpClientError<reqwest::Error>),
}

/// O cliente HTTP do fluxo OIDC, com a guarda de URL em CADA pedido. A
/// descoberta não é um só pedido: o documento do IdP diz de onde vêm as chaves
/// (`jwks_uri`) e onde se troca o código (`token_endpoint`), e um IdP malicioso
/// pode apontá-los a um IP literal interno — que o resolver não vê.
pub struct OidcHttp<'a>(&'a Outbound);

impl Outbound {
    pub fn oidc(&self) -> OidcHttp<'_> {
        OidcHttp(self)
    }
}

impl<'c> openidconnect::AsyncHttpClient<'c> for OidcHttp<'_> {
    type Error = OidcHttpError;
    type Future = std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<openidconnect::HttpResponse, OidcHttpError>>
                + Send
                + 'c,
        >,
    >;

    fn call(&'c self, request: openidconnect::HttpRequest) -> Self::Future {
        Box::pin(async move {
            self.0
                .check_tenant_url(&request.uri().to_string())
                .await
                .map_err(|e| OidcHttpError::Blocked(e.to_string()))?;
            Ok(self.0.tenant.call(request).await?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allowlist_isenta_so_os_hosts_nomeados() {
        let out = Outbound::new(vec!["odoo.interno".to_string()]);
        assert!(out.check_tenant_url("http://odoo.interno:8069/hook").await.is_ok());
        assert!(out.check_tenant_url("http://ODOO.INTERNO/hook").await.is_ok());
        assert!(out
            .check_tenant_url("http://169.254.169.254/latest/meta-data")
            .await
            .is_err());
        assert!(out.check_tenant_url("http://127.0.0.1:8069/hook").await.is_err());
        assert!(out.check_tenant_url("http://[::ffff:127.0.0.1]/").await.is_err());
        let sem = Outbound::new(vec![]);
        assert!(sem.check_tenant_url("http://odoo.interno:8069/hook").await.is_err());
        // Ao gravar, um nome que (ainda) não resolve passa; um IP interno não.
        assert!(sem.check_tenant_config_url("https://idp.ainda-sem-dns.test").await.is_ok());
        assert!(sem.check_tenant_config_url("https://127.0.0.1/").await.is_err());
        assert!(sem.check_tenant_config_url("https://[fd00:ec2::254]/").await.is_err());
    }

    #[tokio::test]
    async fn recusa_esquemas_e_credenciais() {
        let out = Outbound::new(vec!["h.test".to_string()]);
        assert!(out.check_tenant_url("file:///etc/passwd").await.is_err());
        assert!(out.check_tenant_url("gopher://h.test/").await.is_err());
        assert!(out.check_tenant_url("http://u:p@h.test/").await.is_err());
    }

    #[tokio::test]
    async fn operador_alcanca_a_rede_privada_mas_nao_os_metadados() {
        let out = Outbound::new(vec![]);
        assert!(out.check_operator_url("http://127.0.0.1:8080/dav").await.is_ok());
        assert!(out.check_operator_url("http://10.0.0.7/dav").await.is_ok());
        assert!(out
            .check_operator_url("http://169.254.169.254/latest/meta-data")
            .await
            .is_err());
    }

    /// A guarda da LIGAÇÃO, não só a do URL: um nome que resolve para loopback
    /// e não está na allowlist não chega a abrir socket, mesmo que o chamador
    /// se tenha esquecido do `check_tenant_url`.
    #[tokio::test]
    async fn o_resolver_recusa_a_ligacao_a_um_nome_interno() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let out = Outbound::new(vec![]);
        let err = out
            .tenant()
            .get(format!("http://localhost:{port}/"))
            .send()
            .await
            .expect_err("localhost não pode ser alcançável pelo cliente de inquilino");
        assert!(err.is_connect(), "devia falhar na ligação: {err}");

        // O mesmo nome, declarado pelo operador, passa.
        let aberto = Outbound::new(vec!["localhost".to_string()]);
        let servidor = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let _ = s
                .write_all(b"HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n")
                .await;
        });
        let resp = aberto
            .tenant()
            .get(format!("http://localhost:{port}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 204);
        servidor.await.unwrap();
    }

    /// Sem redirects: um `302` para os metadados não é seguido.
    #[tokio::test]
    async fn redirects_nao_sao_seguidos() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let _ = s
                .write_all(b"HTTP/1.1 302 Found\r\nlocation: http://169.254.169.254/\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                .await;
        });
        let out = Outbound::new(vec!["localhost".to_string()]);
        let resp = out
            .tenant()
            .get(format!("http://localhost:{port}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 302);
    }
}
