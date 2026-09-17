//! Sessões da própria conta: o que se mostra de cada uma e quando uma
//! alteração de factor exige reautenticação.
//!
//! Uma SESSÃO nasce num login (password, MFA, SSO, Odoo) e vive enquanto a
//! família de refresh tokens rodar. O access token leva o id dela (`sid`), e é
//! por isso que terminar uma sessão corta o acesso JÁ — não só no próximo
//! refresh.
//!
//! Regras sem IO:
//! - [`describe_user_agent`]: dispositivo e browser a partir do `User-Agent`,
//!   por regras simples e conhecidas (sem base externa). Um UA desconhecido é
//!   «Desconhecido», nunca um palpite.
//! - [`mask_ip`]: o endereço mostrado é MASCARADO (último octeto em IPv4,
//!   últimos 80 bits em IPv6). Sem base GeoIP local não se mostra cidade — e
//!   nenhum serviço externo é consultado.
//! - [`reauth_is_recent`]: registar/remover factores e gerar códigos novos
//!   exige ter provado a identidade há menos de [`REAUTH_WINDOW_SECS`].

use chrono::{DateTime, Utc};
use delonix_meet_core::DomainError;
use serde::Serialize;

/// Janela de reautenticação para alterar factores.
pub const REAUTH_WINDOW_SECS: i64 = 5 * 60;

/// Tecto do `User-Agent` guardado (há clientes que mandam kilobytes).
pub const USER_AGENT_MAX: usize = 512;

pub const REAUTH_REQUIRED: &str = "auth.reauthentication_required";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceInfo {
    /// `desktop` | `mobile` | `tablet` | `app` | `unknown`.
    pub device_type: &'static str,
    /// `Windows`, `macOS`, `iOS`, `Android`, `Linux`, `ChromeOS` ou `Desconhecido`.
    pub os: &'static str,
    /// `Chrome`, `Edge`, `Firefox`, `Safari`, `Opera`, `Delonix app` ou `Desconhecido`.
    pub browser: &'static str,
}

pub fn describe_user_agent(ua: &str) -> DeviceInfo {
    let u = ua.to_ascii_lowercase();
    let app = u.contains("delonixmeet") || u.contains("delonix-meet-app");
    let os = if u.contains("iphone") || u.contains("ipad") || u.contains("ipod") {
        "iOS"
    } else if u.contains("android") {
        "Android"
    } else if u.contains("cros") {
        "ChromeOS"
    } else if u.contains("windows") {
        "Windows"
    } else if u.contains("mac os x") || u.contains("macintosh") {
        "macOS"
    } else if u.contains("linux") {
        "Linux"
    } else {
        "Desconhecido"
    };
    // A ordem importa: o Edge e o Opera dizem «Chrome», e o Chrome diz «Safari».
    let browser = if app {
        "Delonix app"
    } else if u.contains("edg/") || u.contains("edga/") || u.contains("edgios/") {
        "Edge"
    } else if u.contains("opr/") || u.contains("opera") {
        "Opera"
    } else if u.contains("firefox/") || u.contains("fxios/") {
        "Firefox"
    } else if u.contains("chrome/") || u.contains("crios/") || u.contains("chromium/") {
        "Chrome"
    } else if u.contains("safari/") && u.contains("version/") {
        "Safari"
    } else {
        "Desconhecido"
    };
    let device_type = if app {
        "app"
    } else if u.contains("ipad") || (u.contains("android") && !u.contains("mobile")) {
        "tablet"
    } else if u.contains("mobile") || u.contains("iphone") {
        "mobile"
    } else if os == "Desconhecido" {
        "unknown"
    } else {
        "desktop"
    };
    DeviceInfo {
        device_type,
        os,
        browser,
    }
}

/// `203.0.113.77` → `203.0.113.x`; `2001:db8:1:2:3:4:5:6` → `2001:db8:1:…`.
/// Um texto que não é IP não se mostra.
pub fn mask_ip(raw: &str) -> Option<String> {
    match raw.trim().parse::<std::net::IpAddr>().ok()? {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            Some(format!("{}.{}.{}.x", o[0], o[1], o[2]))
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return mask_ip(&v4.to_string());
            }
            let s = v6.segments();
            Some(format!("{:x}:{:x}:{:x}:…", s[0], s[1], s[2]))
        }
    }
}

/// Corta o `User-Agent` a um tamanho guardável, em fronteira de carácter.
pub fn clip_user_agent(ua: &str) -> String {
    ua.chars()
        .filter(|c| !c.is_control())
        .take(USER_AGENT_MAX)
        .collect()
}

pub fn reauth_is_recent(reauthenticated_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    matches!(reauthenticated_at, Some(t) if t <= now && (now - t).num_seconds() < REAUTH_WINDOW_SECS)
}

pub fn require_recent_reauth(
    reauthenticated_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), DomainError> {
    if reauth_is_recent(reauthenticated_at, now) {
        Ok(())
    } else {
        Err(
            DomainError::forbidden(REAUTH_REQUIRED).with_message(format!(
                "confirme a sua identidade (POST /api/users/me/reauthentication) — vale {} min",
                REAUTH_WINDOW_SECS / 60
            )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agents_of_the_template() {
        let mac_chrome = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
        assert_eq!(
            describe_user_agent(mac_chrome),
            DeviceInfo {
                device_type: "desktop",
                os: "macOS",
                browser: "Chrome"
            }
        );
        let win_edge = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36 Edg/128.0.2739.42";
        assert_eq!(describe_user_agent(win_edge).browser, "Edge");
        assert_eq!(describe_user_agent(win_edge).os, "Windows");
        let iphone_safari = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";
        assert_eq!(
            describe_user_agent(iphone_safari),
            DeviceInfo {
                device_type: "mobile",
                os: "iOS",
                browser: "Safari"
            }
        );
        let firefox = "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0";
        assert_eq!(describe_user_agent(firefox).browser, "Firefox");
        assert_eq!(describe_user_agent(firefox).os, "Linux");
        assert_eq!(
            describe_user_agent("DelonixMeet/2.4 (iPhone)").device_type,
            "app"
        );
        assert_eq!(
            describe_user_agent("curl/8.5"),
            DeviceInfo {
                device_type: "unknown",
                os: "Desconhecido",
                browser: "Desconhecido"
            }
        );
        assert_eq!(describe_user_agent("").device_type, "unknown");
    }

    #[test]
    fn ips_are_masked_never_shown_whole() {
        assert_eq!(mask_ip("10.20.4.11").as_deref(), Some("10.20.4.x"));
        assert_eq!(
            mask_ip("2001:db8:85a3::8a2e:370:7334").as_deref(),
            Some("2001:db8:85a3:…")
        );
        assert_eq!(mask_ip("::ffff:192.0.2.9").as_deref(), Some("192.0.2.x"));
        assert_eq!(mask_ip("unknown"), None);
        assert_eq!(mask_ip(""), None);
    }

    #[test]
    fn user_agent_is_clipped() {
        assert_eq!(
            clip_user_agent(&"é".repeat(1000)).chars().count(),
            USER_AGENT_MAX
        );
        assert_eq!(clip_user_agent("a\nb"), "ab");
    }

    #[test]
    fn reauth_window() {
        let now = Utc::now();
        assert!(!reauth_is_recent(None, now));
        assert!(reauth_is_recent(
            Some(now - chrono::Duration::seconds(10)),
            now
        ));
        assert!(!reauth_is_recent(
            Some(now - chrono::Duration::seconds(REAUTH_WINDOW_SECS)),
            now
        ));
        assert!(
            !reauth_is_recent(Some(now + chrono::Duration::seconds(60)), now),
            "futuro não conta"
        );
        let e = require_recent_reauth(None, now).unwrap_err();
        assert_eq!(e.code, REAUTH_REQUIRED);
        assert_eq!(e.kind, delonix_meet_core::ErrorKind::PermissionDenied);
    }
}
