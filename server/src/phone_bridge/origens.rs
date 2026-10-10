//! De onde a ponte aceita o FreeSWITCH: a lista de origens, que pode vir por
//! **nome**.
//!
//! A ponte só aceita SIP e RTP de um FreeSWITCH que está na lista
//! (`PHONE_BRIDGE_FREESWITCH_IPS`), por **IP exacto**: sem isso, qualquer host
//! da rede injectava áudio numa reunião fingindo ser o FreeSWITCH. Essa regra
//! não muda.
//!
//! O que muda é como a lista se escreve. Num orquestrador cujo motor dá um IP
//! novo a cada arranque (medido a 2026-10-04: quatro recriações seguidas, quatro
//! IPs diferentes, e o `delonix compose` recusa sub-redes fixas), um IP literal
//! deixa de valer ao primeiro reinício e o chart do Helm documentava o mesmo
//! defeito. Agora uma entrada pode ser um **nome** (`delonix-freeswitch`, o
//! Service `freeswitch`): resolve-se para IPs, e volta a resolver-se de tempos
//! a tempos, para seguir um FreeSWITCH que reiniciou.
//!
//! O que se mantém, e é o que torna isto aceitável:
//!
//! - **Só endereços privados.** Um nome que resolva para um IP público (ou para
//!   `169.254.0.0/16`, onde vivem os serviços de metadados das clouds) é
//!   ignorado e registado. Um registo DNS comprometido não abre a ponte à Internet.
//! - **Fail-closed.** Lista vazia recusa tudo; se a resolução falhar, mantém-se a
//!   lista anterior em vez de a esvaziar (um tropeção do DNS não derruba as
//!   chamadas em curso) e **nunca** se passa a aceitar «qualquer origem».
//! - **Sem CIDR.** Continua a ser uma lista de endereços, não de redes.
//! - Os literais continuam a valer, e valem sem DNS.

use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    sync::{Arc, RwLock},
    time::Duration,
};

/// A lista de origens aceites, partilhada entre o UA SIP e o tarefa que a refresca.
///
/// `Clone` é barato (um `Arc`). Lê-se muito (cada pacote) e escreve-se raramente
/// (a cada resolução), por isso `RwLock`; nunca se segura através de um `.await`.
#[derive(Debug, Clone, Default)]
pub struct SourceAllowlist(Arc<RwLock<Vec<IpAddr>>>);

impl SourceAllowlist {
    pub fn new(ips: Vec<IpAddr>) -> Self {
        Self(Arc::new(RwLock::new(ips)))
    }

    /// `true` só se o IP está na lista. Um lock envenenado dá `false`: fail-closed.
    pub fn contains(&self, ip: &IpAddr) -> bool {
        self.0.read().map(|v| v.contains(ip)).unwrap_or(false)
    }

    /// Cópia da lista neste instante (uma perna fixa as origens da sua chamada).
    pub fn snapshot(&self) -> Vec<IpAddr> {
        self.0.read().map(|v| v.clone()).unwrap_or_default()
    }

    pub fn replace(&self, ips: Vec<IpAddr>) {
        if let Ok(mut g) = self.0.write() {
            *g = ips;
        }
    }
}

/// Uma entrada de `PHONE_BRIDGE_FREESWITCH_IPS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origem {
    Ip(IpAddr),
    Nome(String),
}

/// Lê uma entrada: um IP, um nome de máquina, ou nada que se aceite.
///
/// Um nome tem de ser um nome de DNS **simples** (letras, dígitos, `-`, `.` e
/// `_`, sem `/`, `:`, `@`, espaços): nada que pareça um URL ou um endereço com
/// porta, e nada que o resolvedor tenha de interpretar.
pub fn parse_origem(s: &str) -> Option<Origem> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Some(Origem::Ip(ip));
    }
    let valid = s.len() <= 253
        && !s.starts_with(['.', '-'])
        // Um ponto final é permitido: o nome absoluto (`x.ns.svc.cluster.local.`) não passa
        // pelos domínios de pesquisa do resolvedor, que um nome curto atravessa.
        && !s.ends_with('-')
        && !s.ends_with("..")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
        // Um número com pontos que não é IP (`10.0.0`, `1.2.3.4.5`) é um erro, não um nome.
        && !s.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    valid.then(|| Origem::Nome(s.to_ascii_lowercase()))
}

/// O endereço é privado ou local? É o único tipo de origem que a ponte aceita
/// por nome. `169.254.0.0/16` (link-local) fica **de fora** de propósito: é onde
/// as clouds põem o serviço de metadados.
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
                || o[0] == 127
        }
        IpAddr::V6(v6) => {
            // IPv4 mapeado (`::ffff:a.b.c.d`) vale o IPv4 que carrega.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_ip(IpAddr::V4(v4));
            }
            v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 (ULA)
        }
    }
}

/// Resolve um nome para os seus endereços **privados**; descarta (e conta) os
/// outros. `Err` se o resolvedor falhou, para quem chama poder manter a lista
/// anterior — uma lista vazia por falha de DNS não é o mesmo que «sem origens».
pub async fn resolve_private(name: &str) -> Result<(Vec<IpAddr>, usize), std::io::Error> {
    let addrs = tokio::net::lookup_host((name, 0)).await?;
    let mut ok = BTreeSet::new();
    let mut recusados = 0usize;
    for a in addrs {
        // Loopback só como literal: por nome, um `localhost` ou um `127.0.1.1` do `/etc/hosts`
        // poria na lista processos do próprio netns do servidor.
        if is_private_ip(a.ip()) && !a.ip().is_loopback() {
            ok.insert(a.ip());
        } else {
            recusados += 1;
        }
    }
    Ok((ok.into_iter().collect(), recusados))
}

/// A lista completa: os IPs literais mais o que cada nome resolve agora.
///
/// `Ok(None)` = nenhum nome resolveu (falha geral do resolvedor): quem chama
/// mantém a lista de antes. Os literais entram sempre.
pub async fn resolve_all(literais: &[IpAddr], nomes: &[String]) -> Option<Vec<IpAddr>> {
    let mut out: BTreeSet<IpAddr> = literais.iter().copied().collect();
    let mut algum_nome_resolveu = nomes.is_empty();
    for n in nomes {
        match resolve_private(n).await {
            Ok((ips, recusados)) => {
                algum_nome_resolveu = true;
                if recusados > 0 {
                    tracing::warn!(
                        nome = %n,
                        recusados,
                        "ponte: o nome resolve também para endereços NÃO privados — ignorados"
                    );
                }
                if ips.is_empty() {
                    tracing::warn!(nome = %n, "ponte: o nome não tem nenhum endereço privado");
                }
                out.extend(ips);
            }
            Err(e) => tracing::warn!(nome = %n, erro = %e, "ponte: não consegui resolver o nome"),
        }
    }
    algum_nome_resolveu.then(|| out.into_iter().collect())
}

/// O IP local por onde se alcança `alvo`, sem enviar nada: um `connect` num
/// socket UDP só consulta a tabela de rotas. Serve para o SDP e para a morada
/// que se dá ao FreeSWITCH quando ninguém os configurou, porque `0.0.0.0` não é
/// um endereço onde alguém possa mandar RTP.
///
/// Sem alvo usa `192.0.2.1` (TEST-NET-1, RFC 5737): não é encaminhável, mas a
/// consulta de rotas responde com a interface da rota por omissão.
pub fn local_ip_towards(alvo: Option<IpAddr>) -> Option<IpAddr> {
    let alvo = alvo.unwrap_or(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)));
    let bind: SocketAddr = if alvo.is_ipv4() {
        "0.0.0.0:0".parse().ok()?
    } else {
        "[::]:0".parse().ok()?
    };
    let s = UdpSocket::bind(bind).ok()?;
    s.connect(SocketAddr::new(alvo, 9)).ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

/// Ciclos seguidos sem resolver nenhum nome antes de a lista cair para os literais.
const MAX_FALHAS_SEGUIDAS: u32 = 3;

/// Refresca a lista de tempos a tempos. Só se lança se houver nomes.
pub fn spawn_refresh(
    lista: SourceAllowlist,
    literais: Vec<IpAddr>,
    nomes: Vec<String>,
    cada: Duration,
) {
    if nomes.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(cada);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut falhas = 0u32;
        tick.tick().await; // o 1.º tick é imediato: a lista inicial já foi posta
        loop {
            tick.tick().await;
            // Falha geral do resolvedor: mantém-se a lista, mas só por `MAX_FALHAS_SEGUIDAS`
            // ticks. Sem prazo, um FreeSWITCH que caiu deixava o seu IP autorizado e um
            // contentor novo que o herdasse entrava nas salas.
            let Some(nova) = resolve_all(&literais, &nomes).await else {
                falhas += 1;
                if falhas == MAX_FALHAS_SEGUIDAS {
                    tracing::warn!("ponte: os nomes do FreeSWITCH não resolvem há {falhas} ciclos — só ficam os IPs literais");
                    lista.replace(literais.clone());
                }
                continue;
            };
            falhas = 0;
            let antiga = lista.snapshot();
            if antiga != nova {
                tracing::info!(
                    antes = ?antiga,
                    agora = ?nova,
                    "ponte: as origens do FreeSWITCH mudaram"
                );
                lista.replace(nova);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn so_os_enderecos_privados_e_locais_valem_por_nome() {
        for ok in [
            "10.0.0.1",
            "10.225.223.215",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.0.141",
            "127.0.0.1",
            "::1",
            "fd00::1",
            "fc00::1",
            "::ffff:10.0.0.5",
        ] {
            assert!(is_private_ip(ip(ok)), "{ok} devia ser privado");
        }
        for no in [
            "8.8.8.8",
            "1.1.1.1",
            "172.15.0.1",
            "172.32.0.1",
            "192.169.0.1",
            "11.0.0.1",
            // os serviços de metadados das clouds
            "169.254.169.254",
            "169.254.0.1",
            "2001:db8::1",
            "fe80::1",
            "::ffff:8.8.8.8",
            "0.0.0.0",
        ] {
            assert!(!is_private_ip(ip(no)), "{no} NÃO devia ser privado");
        }
    }

    #[test]
    fn entradas_sao_ip_nome_ou_nada() {
        assert_eq!(parse_origem("10.0.0.5"), Some(Origem::Ip(ip("10.0.0.5"))));
        assert_eq!(parse_origem(" ::1 "), Some(Origem::Ip(ip("::1"))));
        assert_eq!(
            parse_origem("Delonix-FreeSwitch"),
            Some(Origem::Nome("delonix-freeswitch".into()))
        );
        assert_eq!(
            parse_origem("freeswitch.ngolacloud-meet.svc"),
            Some(Origem::Nome("freeswitch.ngolacloud-meet.svc".into()))
        );
        assert_eq!(
            parse_origem("freeswitch.ns.svc.cluster.local."),
            Some(Origem::Nome("freeswitch.ns.svc.cluster.local.".into()))
        );
        for mau in [
            "",
            "  ",
            "10.0.0",
            "1.2.3.4.5",
            "host:5060",
            "http://host",
            "host/path",
            "user@host",
            "a b",
            "-host",
            "host-",
            ".host",
            "host..",
            "10.0.0.0/8",
            "ho$t",
        ] {
            assert_eq!(parse_origem(mau), None, "{mau:?} não é uma origem");
        }
    }

    #[test]
    fn a_lista_vazia_nao_aceita_nada_e_nunca_tudo() {
        let l = SourceAllowlist::default();
        assert!(l.snapshot().is_empty());
        assert!(!l.contains(&ip("10.0.0.1")));
        assert!(!l.contains(&ip("127.0.0.1")));
        l.replace(vec![ip("10.0.0.1")]);
        assert!(l.contains(&ip("10.0.0.1")));
        assert!(!l.contains(&ip("10.0.0.2")), "exacto, não por rede");
        // Uma cópia partilha a lista: o refresco vê-se no UA.
        let outra = l.clone();
        l.replace(vec![ip("10.0.0.2")]);
        assert!(outra.contains(&ip("10.0.0.2")) && !outra.contains(&ip("10.0.0.1")));
        assert_eq!(l.snapshot(), vec![ip("10.0.0.2")]);
    }

    #[tokio::test]
    async fn por_nome_o_loopback_e_recusado() {
        let (ips, recusados) = resolve_private("localhost")
            .await
            .expect("localhost resolve");
        assert!(ips.is_empty(), "loopback só entra como literal");
        assert!(recusados > 0);
    }

    #[tokio::test]
    async fn os_literais_entram_sempre_e_sem_dns() {
        let r = resolve_all(&[ip("10.0.0.9")], &[]).await;
        assert_eq!(r, Some(vec![ip("10.0.0.9")]));
        // Com um nome que não resolve, nada resolveu: quem chama mantém a lista de antes.
        let r = resolve_all(&[ip("10.0.0.9")], &["nao-existe.invalid".to_string()]).await;
        assert_eq!(r, None, "uma falha total do DNS não esvazia a lista");
        // Um nome que resolve só para loopback conta como resolvido, sem acrescentar nada.
        let r = resolve_all(&[ip("10.0.0.9")], &["localhost".to_string()])
            .await
            .unwrap();
        assert_eq!(r, vec![ip("10.0.0.9")]);
    }

    #[test]
    fn o_ip_local_para_um_destino_loopback_e_loopback() {
        assert_eq!(
            local_ip_towards(Some(ip("127.0.0.1"))),
            Some(ip("127.0.0.1"))
        );
        // Sem alvo: ou uma interface real, ou nada — nunca 0.0.0.0.
        if let Some(i) = local_ip_towards(None) {
            assert!(!i.is_unspecified());
        }
    }
}
