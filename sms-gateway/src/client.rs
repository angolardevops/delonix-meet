//! Cliente HTTP da superfície do agente (`/api/sms/agent/*`, ADR-0005).
//!
//! O token (`dlxg_…`) nunca aparece em log: vive num `Secret` com `Debug`
//! redigido e só é lido para montar o cabeçalho `Authorization`.

use std::fmt;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::at::Pdu;
use crate::inventory::Device;

/// Segredo com `Debug`/`Display` redigidos.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// Aceita só tokens de gateway (`dlxg_…`) — um token de sessão ou de outro
    /// tipo colado por engano falha aqui, e não no servidor com um 401 opaco.
    pub fn gateway_token(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if !raw.starts_with("dlxg_") || raw.len() <= "dlxg_".len() {
            bail!("o token do gateway tem de começar por dlxg_ (criado na consola, ao registar o gateway)");
        }
        Ok(Secret(raw.to_string()))
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(«redigido»)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("«redigido»")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaimedMessage {
    pub id: String,
    pub device_key: String,
    pub to: String,
    pub body: String,
    #[serde(default)]
    pub pdus: Vec<Pdu>,
}

#[derive(Deserialize)]
struct ClaimResponse {
    #[serde(default)]
    messages: Vec<ClaimedMessage>,
}

#[derive(Deserialize)]
struct DevicesResponse {
    poll_interval_secs: Option<u64>,
}

#[derive(Serialize)]
struct DevicesBody<'a> {
    devices: &'a [Device],
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SendResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    token: Secret,
}

impl Client {
    pub fn new(server: &str, token: Secret) -> Result<Self> {
        let base = server.trim().trim_end_matches('/').to_string();
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            bail!("--server tem de ser um URL http(s)://");
        }
        if base.starts_with("http://") {
            tracing::warn!("servidor em http:// — o token vai em claro na rede");
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("delonix-sms-gateway/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("não foi possível criar o cliente HTTP")?;
        Ok(Client { http, base, token })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn check(resp: reqwest::Response, what: &str) -> Result<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let mut body = resp.text().await.unwrap_or_default();
        body.truncate(300);
        if status == reqwest::StatusCode::UNAUTHORIZED {
            bail!("{what}: 401 — token do gateway inválido ou revogado");
        }
        bail!("{what}: HTTP {status}: {body}");
    }

    /// `PUT /api/sms/agent/devices` → `poll_interval_secs` sugerido.
    pub async fn put_devices(&self, devices: &[Device]) -> Result<Option<u64>> {
        let resp = self
            .http
            .put(self.url("/api/sms/agent/devices"))
            .bearer_auth(self.token.expose())
            .json(&DevicesBody { devices })
            .send()
            .await
            .context("reportar inventário")?;
        let resp = Self::check(resp, "reportar inventário").await?;
        let parsed: DevicesResponse = resp.json().await.context("resposta do inventário")?;
        Ok(parsed.poll_interval_secs)
    }

    /// `POST /api/sms/agent/claim`.
    pub async fn claim(&self) -> Result<Vec<ClaimedMessage>> {
        let resp = self
            .http
            .post(self.url("/api/sms/agent/claim"))
            .bearer_auth(self.token.expose())
            .send()
            .await
            .context("pedir mensagens")?;
        let resp = Self::check(resp, "pedir mensagens").await?;
        let parsed: ClaimResponse = resp.json().await.context("resposta do claim")?;
        Ok(parsed.messages)
    }

    /// `POST /api/sms/agent/messages/{id}/result`.
    pub async fn report_result(&self, message_id: &str, result: &SendResult) -> Result<()> {
        if message_id.is_empty()
            || !message_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            bail!("id de mensagem inválido vindo do servidor");
        }
        let resp = self
            .http
            .post(self.url(&format!("/api/sms/agent/messages/{message_id}/result")))
            .bearer_auth(self.token.expose())
            .json(result)
            .send()
            .await
            .context("reportar resultado")?;
        Self::check(resp, "reportar resultado").await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_must_be_a_gateway_token_and_is_redacted() {
        assert!(Secret::gateway_token("dlxs_abc").is_err());
        assert!(Secret::gateway_token("dlxg_").is_err());
        let s = Secret::gateway_token(" dlxg_supersecreto \n").unwrap();
        assert_eq!(s.expose(), "dlxg_supersecreto");
        assert!(!format!("{s:?}").contains("supersecreto"));
        assert!(!format!("{s}").contains("supersecreto"));
        let c = Client::new("https://meet.example/", s).unwrap();
        assert!(!format!("{c:?}").contains("supersecreto"));
        assert_eq!(c.url("/api/x"), "https://meet.example/api/x");
    }

    #[test]
    fn claim_response_shape() {
        let json = r#"{"messages":[{"id":"m1","device_key":"12d1:1506@1-2","to":"+244923000000","body":"olá","pdus":[{"hex":"0011","tpdu_len":1}]}]}"#;
        let r: ClaimResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.messages[0].pdus[0].tpdu_len, 1);
        assert_eq!(r.messages[0].device_key, "12d1:1506@1-2");
    }

    #[test]
    fn result_body_omits_absent_fields() {
        let ok = SendResult {
            ok: true,
            error: None,
            provider_ref: Some("42".into()),
        };
        assert_eq!(
            serde_json::to_string(&ok).unwrap(),
            r#"{"ok":true,"provider_ref":"42"}"#
        );
    }
}
