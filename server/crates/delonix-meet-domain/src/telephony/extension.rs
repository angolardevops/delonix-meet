//! Números curtos dos ramais internos — a forma, e o número RESERVADO pelo
//! qual um ramal entra numa reunião (R273).
//!
//! E o ENDEREÇO PÚBLICO do servidor SIP dos ramais: o que um softphone põe em
//! «servidor/proxy». Não é o domínio SIP — esse é o realm do digest, um nome
//! lógico (`<slug>.<sufixo>`) que não tem de resolver em DNS.
//!
//! Sem IO: quem lê a configuração e a base é `server/src/ramais.rs`.

use std::net::Ipv6Addr;

/// Número de acesso às reuniões quando `VOICE_MEETING_ACCESS_NUMBER` não está
/// definido.
pub const DEFAULT_MEETING_ACCESS_NUMBER: usize = 8000;

/// Limites do número de acesso lido do ambiente: três a cinco dígitos, sem
/// zero à esquerda (é lido como inteiro).
pub const MEETING_ACCESS_NUMBER_MIN: usize = 100;
pub const MEETING_ACCESS_NUMBER_MAX: usize = 99_999;

/// Um número curto: 3 a 5 dígitos ASCII, nada mais. É a mesma forma que o
/// dialplan `delonix_ramais` aceita (`^\d{3,5}$`).
pub fn is_short_number(s: &str) -> bool {
    (3..=5).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

/// O número marcado é o de acesso às reuniões — não pode ser de um ramal, e
/// quem o marca vai para o IVR da sala em vez de tocar em alguém.
pub fn is_meeting_access_number(dialed: &str, meeting_access_number: &str) -> bool {
    !meeting_access_number.is_empty() && dialed == meeting_access_number
}

/// Porta pública do servidor SIP dos ramais quando `VOICE_RAMAIS_PUBLIC_PORT`
/// não está definido — a do perfil `internal` do FreeSWITCH.
pub const DEFAULT_SIP_PUBLIC_PORT: usize = 5070;

/// Transporte SIP que o softphone usa para chegar ao servidor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SipTransport {
    Udp,
    Tcp,
    Tls,
}

impl SipTransport {
    /// `udp`, `tcp` ou `tls`, sem distinguir maiúsculas. Qualquer outra coisa
    /// é `None` — quem lê a configuração decide o que fazer com isso.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "udp" => Some(Self::Udp),
            "tcp" => Some(Self::Tcp),
            "tls" => Some(Self::Tls),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Udp => "udp",
            Self::Tcp => "tcp",
            Self::Tls => "tls",
        }
    }
}

/// O host é um nome DNS ou um IP literal — sem esquema, porta, caminho ou
/// espaços. É o que vai parar ao campo «servidor» de um softphone: um valor
/// com `sip:` ou `:5070` lá dentro daria um proxy que não se consegue marcar.
pub fn is_sip_host(s: &str) -> bool {
    if s.parse::<Ipv6Addr>().is_ok() {
        return true;
    }
    !s.is_empty()
        && s.len() <= 253
        && s.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// Onde o softphone se liga: host, porta e transporte públicos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SipServer {
    host: String,
    port: u16,
    transport: SipTransport,
}

impl SipServer {
    /// `None` se o host não tiver forma de host ou a porta for 0.
    pub fn new(host: &str, port: u16, transport: SipTransport) -> Option<Self> {
        let host = host.trim();
        (is_sip_host(host) && port != 0).then(|| Self {
            host: host.to_string(),
            port,
            transport,
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn transport(&self) -> SipTransport {
        self.transport
    }

    /// O proxy pronto a colar num softphone: `sip:host:porta;transport=x`.
    /// Um IPv6 literal vai entre parênteses rectos (RFC 3261 §19.1.1).
    pub fn proxy_uri(&self) -> String {
        let t = self.transport.as_str();
        if self.host.contains(':') {
            format!("sip:[{}]:{};transport={t}", self.host, self.port)
        } else {
            format!("sip:{}:{};transport={t}", self.host, self.port)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numero_curto_tem_tres_a_cinco_digitos() {
        for ok in ["101", "8000", "12345", "007"] {
            assert!(is_short_number(ok), "{ok}");
        }
        for bad in ["", "12", "123456", "10a", "+101", " 101", "１０１"] {
            assert!(!is_short_number(bad), "{bad}");
        }
    }

    #[test]
    fn so_o_numero_exacto_e_reservado() {
        assert!(is_meeting_access_number("8000", "8000"));
        assert!(!is_meeting_access_number("800", "8000"));
        assert!(!is_meeting_access_number("80000", "8000"));
        assert!(!is_meeting_access_number("8001", "8000"));
        // Sem número configurado nada é reservado — e nada entra por engano.
        assert!(!is_meeting_access_number("", ""));
    }

    #[test]
    fn a_omissao_e_um_numero_curto_dentro_dos_limites() {
        let n = DEFAULT_MEETING_ACCESS_NUMBER;
        assert!((MEETING_ACCESS_NUMBER_MIN..=MEETING_ACCESS_NUMBER_MAX).contains(&n));
        assert!(is_short_number(&n.to_string()));
        assert!(is_short_number(&MEETING_ACCESS_NUMBER_MIN.to_string()));
        assert!(is_short_number(&MEETING_ACCESS_NUMBER_MAX.to_string()));
    }

    #[test]
    fn transporte_so_aceita_udp_tcp_tls() {
        assert_eq!(SipTransport::parse("udp"), Some(SipTransport::Udp));
        assert_eq!(SipTransport::parse(" TCP "), Some(SipTransport::Tcp));
        assert_eq!(SipTransport::parse("Tls"), Some(SipTransport::Tls));
        for bad in ["", "ws", "wss", "sctp", "udp,tcp", "tls1.3"] {
            assert_eq!(SipTransport::parse(bad), None, "{bad}");
        }
        for t in [SipTransport::Udp, SipTransport::Tcp, SipTransport::Tls] {
            assert_eq!(SipTransport::parse(t.as_str()), Some(t));
        }
    }

    #[test]
    fn host_e_um_nome_ou_um_ip_sem_mais_nada() {
        for ok in [
            "meet.ngolacloud.ao",
            "sip-01.exemplo.co.ao",
            "localhost",
            "203.0.113.7",
            "2001:db8::7",
        ] {
            assert!(is_sip_host(ok), "{ok}");
        }
        for bad in [
            "",
            "sip:meet.ao",
            "meet.ao:5070",
            "meet.ao/",
            "meet ao",
            "-meet.ao",
            "meet-.ao",
            "meet..ao",
            "meet.ao;transport=udp",
            "[2001:db8::7]",
            "utilizador@meet.ao",
        ] {
            assert!(!is_sip_host(bad), "{bad}");
        }
    }

    #[test]
    fn o_proxy_sai_pronto_a_colar() {
        let s = SipServer::new(" meet.ngolacloud.ao ", 5070, SipTransport::Udp).unwrap();
        assert_eq!(s.host(), "meet.ngolacloud.ao");
        assert_eq!(s.port(), 5070);
        assert_eq!(s.transport(), SipTransport::Udp);
        assert_eq!(s.proxy_uri(), "sip:meet.ngolacloud.ao:5070;transport=udp");
        let tls = SipServer::new("203.0.113.7", 5061, SipTransport::Tls).unwrap();
        assert_eq!(tls.proxy_uri(), "sip:203.0.113.7:5061;transport=tls");
        let v6 = SipServer::new("2001:db8::7", 5070, SipTransport::Tcp).unwrap();
        assert_eq!(v6.proxy_uri(), "sip:[2001:db8::7]:5070;transport=tcp");
    }

    #[test]
    fn sem_host_valido_nao_ha_servidor() {
        assert!(SipServer::new("", 5070, SipTransport::Udp).is_none());
        assert!(SipServer::new("sip:meet.ao", 5070, SipTransport::Udp).is_none());
        assert!(SipServer::new("meet.ao", 0, SipTransport::Udp).is_none());
    }
}
