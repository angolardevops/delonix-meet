//! Provisionamento de um softphone por QR de uso único (R278): a forma do
//! bilhete, o endereço público que pode ir num QR e a configuração que o
//! Linphone descarrega.
//!
//! Sem IO: quem emite e resgata o bilhete é `server/src/extension_provisioning.rs`.
//!
//! **O formato da configuração não foi verificado contra um Linphone real.** As
//! secções e as chaves (`sip`, `auth_info_0`, `proxy_0`, e o envelope
//! `lpconfig.xsd`) são as do ficheiro `linphonerc` e do «remote provisioning»
//! tal como documentados; nenhum aparelho leu ainda um XML gerado aqui.

use super::extension::SipServer;

/// Quanto tempo vale um bilhete depois de emitido.
pub const TICKET_TTL_SECS: i64 = 600;

/// Bytes de aleatoriedade de um bilhete (256 bits).
pub const TICKET_BYTES: usize = 32;

/// O bilhete tem a forma certa: `TICKET_BYTES` em hex minúsculo. Tudo o resto
/// nem chega à base.
pub fn is_ticket_shape(s: &str) -> bool {
    s.len() == TICKET_BYTES * 2
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A origem pública do servidor, pronta a levar um caminho — ou `None` se não
/// serve para um QR. Tem de ser `https://host[:porta]`, sem caminho: a
/// configuração descarregada leva a password SIP, e um telefone não alcança um
/// nome interno do cluster (`*.svc`), `localhost`, nem um nome `*.local`.
///
/// `.local` é o domínio reservado do mDNS (RFC 6762): resolve-se na rede local
/// por multicast, não por DNS, e um telemóvel (o Android em particular) em geral
/// não o resolve — o Linphone falha a descarregar sem nunca chegar ao
/// certificado. Cobre também `*.cluster.local`. Fica de fora quem precisar de um
/// laboratório: usa um nome que o telefone resolva (um domínio, ou `sslip.io`).
pub fn public_base_url(origin: &str) -> Option<&str> {
    let origin = origin.trim().trim_end_matches('/');
    let authority = origin.strip_prefix("https://")?;
    if authority.is_empty() || authority.contains(['/', '?', '#', '@', ' ']) {
        return None;
    }
    let host = match authority.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => authority,
    }
    .to_ascii_lowercase();
    let internal = host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".svc")
        || host == "local"
        || host.ends_with(".local");
    (!host.is_empty() && !internal).then_some(origin)
}

/// A conta SIP de um ramal, com a password em claro — só existe no instante em
/// que a configuração é gerada.
pub struct LinphoneAccount<'a> {
    /// O nome que aparece a quem recebe a chamada: a pessoa, ou a etiqueta de
    /// um ramal da empresa.
    pub display_name: &'a str,
    pub sip_username: &'a str,
    /// O realm do digest (`<slug>.<sufixo>`): um nome lógico.
    pub sip_domain: &'a str,
    pub sip_password: &'a str,
    /// Onde o aparelho se liga de facto.
    pub server: &'a SipServer,
}

fn xml_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// O nome de exibição dentro de `"…"` num cabeçalho SIP: sem aspas, barras
/// invertidas, parênteses angulares nem caracteres de controlo.
fn display_name(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\\' | '<' | '>'))
        .take(80)
        .collect::<String>()
        .trim()
        .to_string()
}

/// A configuração que o Linphone descarrega («remote provisioning», formato
/// `lpconfig`): uma conta, por omissão, com registo no endereço público do
/// servidor e SRTP (SDES) obrigatório — o perfil dos ramais não aceita áudio
/// em claro.
///
/// O domínio da identidade é o realm do digest, que não resolve em DNS: por
/// isso o registo (`reg_proxy`) e as chamadas (`reg_route`) vão explicitamente
/// para o endereço público.
pub fn linphone_config_xml(acc: &LinphoneAccount<'_>) -> String {
    let proxy = acc.server.proxy_uri();
    let name = display_name(acc.display_name);
    let identity = if name.is_empty() {
        format!("<sip:{}@{}>", acc.sip_username, acc.sip_domain)
    } else {
        format!("\"{name}\" <sip:{}@{}>", acc.sip_username, acc.sip_domain)
    };
    let entry = |name: &str, value: &str| {
        format!(
            "    <entry name=\"{name}\" overwrite=\"true\">{}</entry>\n",
            xml_text(value)
        )
    };
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <config xmlns=\"http://www.linphone.org/xsds/lpconfig.xsd\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
         xsi:schemaLocation=\"http://www.linphone.org/xsds/lpconfig.xsd lpconfig.xsd\">\n",
    );
    xml.push_str("  <section name=\"sip\">\n");
    xml.push_str(&entry("default_proxy", "0"));
    xml.push_str(&entry("media_encryption", "srtp"));
    xml.push_str(&entry("media_encryption_mandatory", "1"));
    xml.push_str("  </section>\n  <section name=\"auth_info_0\">\n");
    xml.push_str(&entry("username", acc.sip_username));
    xml.push_str(&entry("passwd", acc.sip_password));
    xml.push_str(&entry("realm", acc.sip_domain));
    xml.push_str(&entry("domain", acc.sip_domain));
    xml.push_str("  </section>\n  <section name=\"proxy_0\">\n");
    xml.push_str(&entry("reg_proxy", &format!("<{proxy}>")));
    xml.push_str(&entry("reg_route", &format!("<{proxy};lr>")));
    xml.push_str(&entry("reg_identity", &identity));
    xml.push_str(&entry("realm", acc.sip_domain));
    xml.push_str(&entry("reg_expires", "3600"));
    xml.push_str(&entry("reg_sendregister", "1"));
    xml.push_str(&entry("publish", "0"));
    xml.push_str("  </section>\n</config>\n");
    xml
}

#[cfg(test)]
mod tests {
    use super::super::extension::SipTransport;
    use super::*;

    #[test]
    fn o_bilhete_sao_64_hex_minusculos() {
        assert!(is_ticket_shape(&"a1".repeat(32)));
        for bad in [
            String::new(),
            "a1".repeat(31),
            "a1".repeat(33),
            "A1".repeat(32),
            "g1".repeat(32),
            format!("{} ", "a1".repeat(31)),
        ] {
            assert!(!is_ticket_shape(&bad), "{bad}");
        }
    }

    #[test]
    fn a_origem_publica_e_https_e_nao_e_interna() {
        for (origin, want) in [
            ("https://meet.exemplo.ao", Some("https://meet.exemplo.ao")),
            ("https://meet.exemplo.ao/", Some("https://meet.exemplo.ao")),
            // `.local` é mDNS: o telefone não o resolve (ver `public_base_url`).
            ("https://meet.ngolacloud.local:8443", None),
            ("https://meet.local", None),
            ("https://MEET.Local", None),
            ("https://local", None),
            // Um nome que só TERMINA em «local» (sem o ponto) é um domínio público.
            (
                "https://meet.notlocal.example",
                Some("https://meet.notlocal.example"),
            ),
            (
                "https://meet.local.exemplo.ao",
                Some("https://meet.local.exemplo.ao"),
            ),
            // Um laboratório que o telefone resolve: sslip.io aponta ao IP da máquina.
            (
                "https://meet.192-168-1-10.sslip.io",
                Some("https://meet.192-168-1-10.sslip.io"),
            ),
            ("http://meet.exemplo.ao", None),
            ("https://", None),
            ("https://meet.exemplo.ao/app", None),
            ("https://user@meet.exemplo.ao", None),
            ("https://localhost:5173", None),
            ("https://delonix-server.meet.svc", None),
            ("https://delonix-server.meet.svc.cluster.local:8080", None),
            ("meet.exemplo.ao", None),
        ] {
            assert_eq!(public_base_url(origin), want, "{origin}");
        }
    }

    fn conta<'a>(server: &'a SipServer, name: &'a str, password: &'a str) -> LinphoneAccount<'a> {
        LinphoneAccount {
            display_name: name,
            sip_username: "ramal_0123456789abcdef",
            sip_domain: "acme.ramais.delonix.meet",
            sip_password: password,
            server,
        }
    }

    #[test]
    fn a_configuracao_traz_a_conta_o_proxy_publico_e_srtp_obrigatorio() {
        let server = SipServer::new("meet.exemplo.ao", 5070, SipTransport::Udp).unwrap();
        let xml = linphone_config_xml(&conta(&server, "Ana", "s3gredo"));
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<config xmlns=\"http://www.linphone.org/xsds/lpconfig.xsd\""));
        assert!(xml.trim_end().ends_with("</config>"));
        for want in [
            "<section name=\"sip\">",
            "<entry name=\"default_proxy\" overwrite=\"true\">0</entry>",
            "<entry name=\"media_encryption\" overwrite=\"true\">srtp</entry>",
            "<entry name=\"media_encryption_mandatory\" overwrite=\"true\">1</entry>",
            "<section name=\"auth_info_0\">",
            "<entry name=\"username\" overwrite=\"true\">ramal_0123456789abcdef</entry>",
            "<entry name=\"passwd\" overwrite=\"true\">s3gredo</entry>",
            "<entry name=\"realm\" overwrite=\"true\">acme.ramais.delonix.meet</entry>",
            "<entry name=\"domain\" overwrite=\"true\">acme.ramais.delonix.meet</entry>",
            "<section name=\"proxy_0\">",
            "<entry name=\"reg_proxy\" overwrite=\"true\">&lt;sip:meet.exemplo.ao:5070;transport=udp&gt;</entry>",
            "<entry name=\"reg_route\" overwrite=\"true\">&lt;sip:meet.exemplo.ao:5070;transport=udp;lr&gt;</entry>",
            "<entry name=\"reg_identity\" overwrite=\"true\">\"Ana\" &lt;sip:ramal_0123456789abcdef@acme.ramais.delonix.meet&gt;</entry>",
            "<entry name=\"reg_sendregister\" overwrite=\"true\">1</entry>",
        ] {
            assert!(xml.contains(want), "falta {want} em\n{xml}");
        }
        // O domínio lógico nunca é o sítio onde o aparelho se liga.
        assert!(!xml.contains("&lt;sip:acme.ramais.delonix.meet"));
    }

    #[test]
    fn texto_livre_nao_parte_o_xml_nem_o_cabecalho_sip() {
        let server = SipServer::new("2001:db8::1", 5061, SipTransport::Tls).unwrap();
        let xml = linphone_config_xml(&conta(&server, "Recep\"ção <1> & \\C\n", "a&b"));
        assert!(xml.contains("\"Recepção 1 &amp; C\" &lt;sip:"), "{xml}");
        assert!(xml.contains("<entry name=\"passwd\" overwrite=\"true\">a&amp;b</entry>"));
        assert!(xml.contains("&lt;sip:[2001:db8::1]:5061;transport=tls&gt;"));
        // Sem nome, a identidade é só o URI.
        let anon = linphone_config_xml(&conta(&server, " \" ", "x"));
        assert!(anon.contains(
            "<entry name=\"reg_identity\" overwrite=\"true\">&lt;sip:ramal_0123456789abcdef@"
        ));
    }
}
