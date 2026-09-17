//! Política de saída: para que endereços o servidor pode abrir uma ligação.
//!
//! É a regra pura por trás da guarda anti-SSRF — sem DNS nem sockets (isso é do
//! `net_guard` do servidor). Dois níveis, porque há dois donos de URL:
//!
//! - [`EgressPolicy::Tenant`] — o URL foi escrito por um CLIENTE (webhook,
//!   `odoo_url` da organização, emissor OIDC). Só destinos públicos: nada de
//!   loopback, redes privadas, CGNAT, link-local, ULA, nem os prefixos que
//!   embutem um IPv4 (mapeado, NAT64, 6to4) e serviriam para os contornar.
//! - [`EgressPolicy::Operator`] — o URL foi configurado pelo OPERADOR (Odoo da
//!   plataforma, WebDAV, Ollama). Uma rede privada é legítima on-prem; o que
//!   continua recusado é o link-local, onde vivem os metadados da cloud
//!   (`169.254.169.254`, `fd00:ec2::254`) — nenhum serviço real mora lá.
//!
//! A isenção por nome (`OUTBOUND_ALLOW_HOSTS`) é por HOST exacto, nunca por
//! rede: um rebind de DNS para um IP interno continua recusado em todos os
//! nomes que o operador não declarou.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressPolicy {
    Tenant,
    Operator,
}

impl EgressPolicy {
    /// `true` se uma ligação a `ip` é permitida por esta política.
    pub fn allows(self, ip: IpAddr) -> bool {
        match self {
            EgressPolicy::Tenant => !is_internal(ip),
            EgressPolicy::Operator => !is_metadata_or_unroutable(ip),
        }
    }
}

/// `true` se `host` consta, por nome exacto (sem distinguir maiúsculas), da
/// lista de destinos que o operador declarou.
pub fn host_is_allowlisted(host: &str, allow_hosts: &[String]) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    allow_hosts.iter().any(|h| {
        h.trim_start_matches('[')
            .trim_end_matches(']')
            .eq_ignore_ascii_case(host)
    })
}

/// O IPv4 que um endereço IPv6 transporta, quando o transporta: mapeado
/// (`::ffff:a.b.c.d`), compatível (`::a.b.c.d`), NAT64 (`64:ff9b::/96`) e 6to4
/// (`2002:AABB:CCDD::/16`). Sem isto, `::ffff:127.0.0.1` passava por público.
fn embedded_v4(v6: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = v6.to_ipv4_mapped() {
        return Some(v4);
    }
    let s = v6.segments();
    if s[..6] == [0, 0, 0, 0, 0, 0] && !(s[6] == 0 && s[7] <= 1) {
        return Some(Ipv4Addr::new(
            (s[6] >> 8) as u8,
            s[6] as u8,
            (s[7] >> 8) as u8,
            s[7] as u8,
        ));
    }
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return Some(Ipv4Addr::new(
            (s[6] >> 8) as u8,
            s[6] as u8,
            (s[7] >> 8) as u8,
            s[7] as u8,
        ));
    }
    if s[0] == 0x2002 {
        return Some(Ipv4Addr::new(
            (s[1] >> 8) as u8,
            s[1] as u8,
            (s[2] >> 8) as u8,
            s[2] as u8,
        ));
    }
    None
}

/// Tudo o que não é a Internet pública.
pub fn is_internal(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10 CGNAT
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24 IETF
                || (o[0] == 198 && (o[1] & 0xfe) == 18) // 198.18.0.0/15 benchmarking
                || o[0] >= 240 // 240.0.0.0/4 reservado
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = embedded_v4(v6) {
                return is_internal(IpAddr::V4(v4));
            }
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg0 & 0xfe00) == 0xfc00 // fc00::/7 ULA
                || (seg0 & 0xffc0) == 0xfe80 // fe80::/10 link-local
                || (seg0 == 0x2001 && v6.segments()[1] == 0x0db8) // 2001:db8::/32 documentação
        }
    }
}

/// O mínimo que nem o operador alcança: link-local (metadados da cloud), o
/// endereço não especificado e multicast.
pub fn is_metadata_or_unroutable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast() || v4.is_multicast()
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = embedded_v4(v6) {
                return is_metadata_or_unroutable(IpAddr::V4(v4));
            }
            let seg0 = v6.segments()[0];
            v6.is_unspecified()
                || v6.is_multicast()
                || (seg0 & 0xffc0) == 0xfe80
                || v6 == Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254) // metadados AWS IPv6
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn tenant_recusa_tudo_o_que_e_interno() {
        for s in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "198.18.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fc00::1",
            "fd00:ec2::254",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "::10.0.0.1",
            "64:ff9b::a9fe:a9fe", // NAT64 de 169.254.169.254
            "2002:7f00:1::",      // 6to4 de 127.0.0.1
            "2001:db8::1",
        ] {
            assert!(
                !EgressPolicy::Tenant.allows(ip(s)),
                "{s} devia ser recusado"
            );
        }
    }

    #[test]
    fn tenant_aceita_enderecos_publicos() {
        for s in [
            "1.1.1.1",
            "8.8.8.8",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
            "2002:0808:0808::",
        ] {
            assert!(EgressPolicy::Tenant.allows(ip(s)), "{s} devia passar");
        }
    }

    #[test]
    fn operador_alcanca_a_rede_privada_mas_nao_os_metadados() {
        for s in [
            "10.0.0.5",
            "192.168.1.10",
            "127.0.0.1",
            "fc00::5",
            "1.1.1.1",
        ] {
            assert!(EgressPolicy::Operator.allows(ip(s)), "{s} devia passar");
        }
        for s in [
            "169.254.169.254",
            "fe80::1",
            "fd00:ec2::254",
            "::ffff:169.254.169.254",
            "0.0.0.0",
        ] {
            assert!(
                !EgressPolicy::Operator.allows(ip(s)),
                "{s} devia ser recusado"
            );
        }
    }

    #[test]
    fn allowlist_e_por_nome_exacto() {
        let allow = vec!["odoo.interno".to_string(), "::1".to_string()];
        assert!(host_is_allowlisted("ODOO.interno", &allow));
        assert!(host_is_allowlisted("[::1]", &allow));
        assert!(!host_is_allowlisted("x.odoo.interno", &allow));
        assert!(!host_is_allowlisted("odoo", &allow));
    }
}
