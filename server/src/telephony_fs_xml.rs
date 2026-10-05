//! Configuração do FreeSWITCH servida por `mod_xml_curl` (ADR-0009 §3):
//! `POST /internal/v1/telephony/freeswitch-config`.
//!
//! **Decisão: `mod_xml_curl`, não ficheiros XML gerados.** O plano de marcação
//! muda por organização e a qualquer hora; ficheiros obrigavam a escrever num
//! volume partilhado e a `reloadxml` a cada gravação, e um reload falhado
//! deixava o FreeSWITCH com o plano velho sem ninguém saber. Com `xml_curl`, o
//! FreeSWITCH pergunta ao servidor em CADA chamada — e a resposta sai da MESMA
//! função que o `dial-plan/test` usa (`telephony_service::resolve_number`):
//! o teste e a chamada real não podem divergir.
//!
//! Duas secções:
//!
//! - `section=dialplan` (contexto `delonix-outbound`): uma extensão para ESTE
//!   número, já resolvida (emergência primeiro, troncos por ordem com
//!   `limit_execute` para os canais máximos — excepto na emergência, que nunca
//!   é travada por limite de canais —, gravação só quando a regra o diz).
//! - `section=directory`, `purpose=gateways`: os gateways de todos os troncos
//!   activos, com a password decifrada — por isso só no listener interno e
//!   com o segredo. O perfil sofia pede-os com
//!   `<domain name="delonix-trunks" parse="true"/>`, e SÓ com esse: o
//!   `<domain name="all">` da vanilla também chega aqui, e com os dois a lista
//!   é lida duas vezes a cada `rescan` (R291). O arranque distribuído troca um
//!   pelo outro (`voice/cluster/freeswitch-entrypoint.sh`, passo 7b).
//!
//! Autenticação: `VOICE_INTERNAL_SECRET` por HTTP Basic (`gateway-credentials`).

use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::State,
    http::{header::CONTENT_TYPE, HeaderMap},
    response::{IntoResponse, Response},
};
use delonix_meet_domain::telephony::{
    dial_plan::{ResolutionOutcome, RuleAction},
    ports::gateway_name,
    trunk::password_aad,
};
use uuid::Uuid;

use crate::{error::ApiError, AppState};

pub const OUTBOUND_CONTEXT: &str = "delonix-outbound";

/// Escapa um valor para dentro de um atributo XML que o FreeSWITCH vai ler.
///
/// O `$` também, e não é por causa do XML: o FreeSWITCH passa a resposta do
/// `mod_xml_curl` pelo PRÉ-PROCESSADOR antes de a ler, e esse troca
/// `$${nome}` pelo valor da variável global `nome`. O segredo de voz é uma
/// (`delonix_voice_secret`). O utilizador e a password de um tronco são texto
/// que o administrador de QUALQUER organização escreve: cru, um utilizador
/// `$${delonix_voice_secret}` punha o FreeSWITCH a registar-se no servidor
/// SIP dele com o segredo da plataforma no `From` (R291, medido). Como
/// referência numérica o pré-processador não o vê, e o leitor de XML
/// devolve-o como o `$` que era.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('$', "&#36;")
}

pub fn not_found() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<document type="freeswitch/xml">
  <section name="result">
    <result status="not found"/>
  </section>
</document>
"#
    .to_string()
}

/// Um tronco já resolvido para a extensão.
pub struct LegSpec {
    pub trunk_id: Uuid,
    pub max_channels: i32,
}

/// A extensão do plano para UM número já resolvido. Pura (testada em unidade).
#[allow(clippy::too_many_arguments)]
pub fn dialplan_extension(
    org_id: Uuid,
    destination: &str,
    wire_number: &str,
    outcome: ResolutionOutcome,
    action: Option<RuleAction>,
    rule_position: Option<usize>,
    legs: &[LegSpec],
    record: bool,
    emergency: bool,
) -> String {
    let mut a: Vec<String> = Vec::new();
    let mut act = |app: &str, data: String| {
        a.push(format!(
            r#"        <action application="{app}" data="{}"/>"#,
            esc(&data)
        ))
    };
    // O CDR que conta é o de cada perna B (uma por tentativa de tronco: é o
    // que dá o ASR por operadora). A perna A — o PBX que marcou — marca-se
    // para a ingestão a ignorar; o resto EXPORTA-se para as pernas B.
    // O `mod_json_cdr` distribuído NÃO regista pernas B (um registo leva as
    // chaves SRTP da perna): a do tronco pede-o na dial string, com
    // `force_process_cdr=true` (R291). E volta a ligar o registo NA PRÓPRIA
    // perna (`execute_on_originate`): a perna de quem marca — um ramal —
    // tem `process_cdr=false`, e o FreeSWITCH copia esse valor para a perna
    // que ela origina DEPOIS de aplicar as variáveis da dial string
    // (switch_core_session.c). Sem isto a chamada de um ramal saía pelo
    // tronco e não deixava registo: não se cobrava (R292, medido).
    //
    // Mais três coisas na perna do tronco, todas medidas ou lidas no fonte:
    // - `unset switch_m_sdp`: ao originar, o FreeSWITCH copia para ela o SDP
    //   que quem marcou ofereceu — com a chave SRTP dele —, e o registo leva
    //   todas as variáveis. Só o modo proxy lê essa cópia.
    // - `execute_on_post_bridge=unset switch_m_sdp`: a cópia VOLTA a ser escrita
    //   sempre que quem marcou manda um SDP novo a meio da chamada — pôr em
    //   espera e retomar chega (sofia_glue_pass_sdp, sem condição). Tira-se
    //   outra vez quando a ponte acaba, na própria perna (`trunk_leg_vars`).
    // - `outbound_redirect_fatal`: um 3xx da operadora não é seguido. Segui-lo
    //   era ligar a um destino que a operadora escolhe (e que não passou pela
    //   guarda de saída, R213), ou a outro número por conta da organização.
    // - na perna de quem marca, `sip_copy_custom_headers=false`: os
    //   cabeçalhos `X-…`/`P-…` do INVITE do ramal iam para a operadora — um
    //   `P-Asserted-Identity` escrito pelo telefone, por exemplo.
    act("set", "sip_copy_custom_headers=false".into());
    // E as partes de um INVITE multipart (a de SDP incluída) também não:
    // ficavam no registo da perna do tronco e seguiam no corpo para a operadora.
    act("set", "sip_copy_multipart=false".into());
    act("set", "delonix_cdr_skip=true".into());
    act("export", format!("delonix_org_id={org_id}"));
    act("export", "delonix_direction=outbound".into());
    act("export", format!("delonix_dialed={wire_number}"));
    act("export", format!("delonix_emergency={emergency}"));
    // Emergência nunca gravada: nem que a regra o dissesse.
    let record = record && !emergency;
    act("export", format!("delonix_record={record}"));
    if let Some(p) = rule_position {
        act("export", format!("delonix_rule_position={p}"));
    }
    match (outcome, action) {
        (ResolutionOutcome::Route, _) => {
            act("set", "continue_on_fail=true".into());
            act("set", "hangup_after_bridge=true".into());
            if record {
                act(
                    "set",
                    "execute_on_answer=record_session ${recordings_dir}/delonix-${uuid}.wav".into(),
                );
            }
            for leg in legs {
                let gw = gateway_name(leg.trunk_id);
                if emergency {
                    // Nunca bloqueada: sem limite de canais.
                    act(
                        "bridge",
                        format!(
                            "[{}]sofia/gateway/{gw}/{wire_number}",
                            trunk_leg_vars(leg.trunk_id)
                        ),
                    );
                } else {
                    act(
                        "limit_execute",
                        format!(
                            "hash delonix_trunk {} {} bridge [{}]sofia/gateway/{gw}/{wire_number}",
                            leg.trunk_id,
                            leg.max_channels,
                            trunk_leg_vars(leg.trunk_id)
                        ),
                    );
                }
            }
            act("respond", "503 Service Unavailable".into());
        }
        (ResolutionOutcome::Internal, Some(RuleAction::RoomPin)) => {
            act("transfer", "delonix_dialin XML public".into());
        }
        (ResolutionOutcome::Internal, _) => {
            act("bridge", format!("user/{destination}@${{domain_name}}"));
        }
        (ResolutionOutcome::Blocked, _) => act("respond", "403 Forbidden".into()),
        (ResolutionOutcome::NoMatch, _) => act("respond", "404 Not Found".into()),
        (ResolutionOutcome::NoAvailableTrunk, _) => {
            act("respond", "503 Service Unavailable".into())
        }
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<document type="freeswitch/xml">
  <section name="dialplan" description="Delonix Meet">
    <context name="{OUTBOUND_CONTEXT}">
      <extension name="delonix-{org_id}">
        <condition field="destination_number" expression="^{}$">
{}
        </condition>
      </extension>
    </context>
  </section>
</document>
"#,
        esc(&regex_literal(destination)),
        a.join("\n")
    )
}

/// O número como literal de regex (só dígitos e `+` podem chegar aqui).
fn regex_literal(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '+' {
                "\\+".to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

pub struct GatewaySpec {
    pub trunk_id: Uuid,
    pub org_id: Uuid,
    pub host: String,
    pub port: i32,
    pub transport: String,
    pub srtp: String,
    pub register: bool,
    pub username: String,
    pub password: String,
}

/// Os gateways de todos os troncos activos, num domínio de directório.
/// Forma exigida pelo `mod_sofia` (`parse_domain_tag`): `<user>` directo no
/// domínio, ou `<groups><group><users><user>`. `<users>` directo no domínio é
/// ignorado EM SILÊNCIO — medido contra o FreeSWITCH 1.11.3.
pub fn gateways_directory(gws: &[GatewaySpec]) -> String {
    let mut users = String::new();
    for g in gws {
        let host = if g.host.contains(':') && !g.host.starts_with('[') {
            format!("[{}]", g.host)
        } else {
            g.host.clone()
        };
        let proxy = format!("{host}:{};transport={}", g.port, g.transport);
        let mut params = vec![
            ("realm", g.host.clone()),
            ("proxy", proxy),
            ("register", g.register.to_string()),
            ("register-transport", g.transport.clone()),
            ("caller-id-in-from", "true".into()),
            ("ping", "30".into()),
        ];
        if !g.username.is_empty() {
            params.push(("username", g.username.clone()));
        } else {
            params.push(("username", "delonix".into()));
        }
        params.push(("password", g.password.clone()));
        let p: String = params
            .iter()
            .map(|(k, v)| format!(r#"              <param name="{k}" value="{}"/>"#, esc(v)))
            .collect::<Vec<_>>()
            .join("\n");
        users.push_str(&format!(
            r#"        <user id="{gw}">
          <gateways>
            <gateway name="{gw}">
{p}
              <variables>
                <variable name="delonix_org_id" value="{org}" direction="inbound"/>
                <variable name="delonix_trunk_id" value="{trunk}" direction="inbound"/>
                <variable name="rtp_secure_media" value="{srtp}" direction="outbound"/>
              </variables>
            </gateway>
          </gateways>
        </user>
"#,
            gw = gateway_name(g.trunk_id),
            org = g.org_id,
            trunk = g.trunk_id,
            srtp = match g.srtp.as_str() {
                "mandatory" => "mandatory",
                "optional" => "optional",
                _ => "forbidden",
            },
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<document type="freeswitch/xml">
  <section name="directory" description="Delonix Meet — troncos">
    <domain name="delonix-trunks">
      <groups>
        <group name="trunks">
          <users>
{users}          </users>
        </group>
      </groups>
    </domain>
  </section>
</document>
"#
    )
}

fn parse_form(body: &[u8]) -> HashMap<String, String> {
    url::form_urlencoded::parse(body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn xml(body: String) -> Response {
    ([(CONTENT_TYPE, "text/xml; charset=utf-8")], body).into_response()
}

/// `POST /internal/v1/telephony/freeswitch-config` (`mod_xml_curl`).
pub async fn handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    crate::voice::check_media_secret(&state, &headers)?;
    let form = parse_form(&body);
    match form.get("section").map(String::as_str) {
        Some("dialplan") => dialplan(&state, &form).await,
        Some("directory") if form.get("purpose").map(String::as_str) == Some("gateways") => {
            gateways(&state).await
        }
        _ => Ok(xml(not_found())),
    }
}

/// As variáveis da perna de um TRONCO (o que vai dentro de `[...]`).
///
/// A cópia do SDP de quem marcou (`switch_m_sdp`, com a chave SRTP dele) é
/// tirada DUAS vezes: quando a perna nasce (`execute_on_originate_2`) e quando
/// a ponte acaba (`execute_on_post_bridge`) — porque o FreeSWITCH volta a
/// escrevê-la sempre que quem marcou manda um SDP novo a meio da chamada.
///
/// O que NÃO serve, medido (R292): um `api_hangup_hook=uuid_setvar …`. O texto
/// do plano é expandido na perna de quem marca (duas vezes, com o
/// `limit_execute`), e `${uuid}` saía com o identificador dela; e mesmo com o
/// identificador certo (`origination_uuid`) o `uuid_setvar` não encontra uma
/// sessão que já desligou (`switch_core_session_perform_read_lock`).
fn trunk_leg_vars(trunk_id: Uuid) -> String {
    format!(
        "delonix_trunk_id={trunk_id},force_process_cdr=true,execute_on_originate_1=set process_cdr=true,execute_on_originate_2=unset switch_m_sdp,execute_on_post_bridge=unset switch_m_sdp,outbound_redirect_fatal=true"
    )
}

async fn dialplan(state: &AppState, form: &HashMap<String, String>) -> Result<Response, ApiError> {
    if form.get("Caller-Context").map(String::as_str) != Some(OUTBOUND_CONTEXT) {
        return Ok(xml(not_found()));
    }
    // A organização que paga é a variável que NÓS pusemos no canal — o Lua
    // dos ramais (com a identidade que o digest autenticou), a chamada de
    // teste pelo ESL — e mais nada. Havia um recuo para o host do Request-URI
    // (`sip_req_host`): um dado que quem liga escreve. Sem a variável, não há
    // rota (R292).
    let Some(org_id) = form
        .get("variable_delonix_org_id")
        .and_then(|v| Uuid::parse_str(v).ok())
    else {
        return Ok(xml(not_found()));
    };
    let destination = form
        .get("Hunt-Destination-Number")
        .or_else(|| form.get("Caller-Destination-Number"))
        .cloned()
        .unwrap_or_default();
    let r = match crate::telephony_service::resolve_number(state, org_id, &destination).await {
        Ok(r) => r,
        // Número mal formado: recusa explícita (nunca cai no plano por omissão).
        Err(_) => {
            return Ok(xml(dialplan_extension(
                org_id,
                &destination
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '+')
                    .collect::<String>(),
                "",
                ResolutionOutcome::NoMatch,
                None,
                None,
                &[],
                false,
                false,
            )))
        }
    };
    let trunks = crate::telephony_service::load_trunks(state, org_id).await?;
    let legs: Vec<LegSpec> = r
        .resolution
        .legs
        .iter()
        .filter_map(|id| trunks.iter().find(|t| t.id == *id))
        .map(|t| LegSpec {
            trunk_id: t.id,
            max_channels: t.max_channels,
        })
        .collect();
    let wire = r
        .dialed
        .e164
        .as_deref()
        .map(|e| e.trim_start_matches('+').to_string())
        .unwrap_or_else(|| r.dialed.digits.clone());
    let dest: String = destination
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '+')
        .collect();
    Ok(xml(dialplan_extension(
        org_id,
        &dest,
        &wire,
        r.resolution.outcome,
        r.resolution.action,
        r.resolution.rule_position,
        &legs,
        r.resolution.record,
        r.resolution.emergency,
    )))
}

async fn gateways(state: &AppState) -> Result<Response, ApiError> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        org_id: Uuid,
        host: String,
        port: i32,
        transport: String,
        srtp: String,
        register: bool,
        username: String,
        password_sealed: String,
    }
    let rows: Vec<Row> = sqlx::query_as(
        // Os troncos de TODAS as organizações vão num só documento, e o
        // mod_xml_curl deita fora a resposta inteira acima do seu limite: o
        // tecto por organização vale também aqui, para as linhas que já lá
        // estavam antes de ele existir.
        "SELECT id, org_id, host, port, transport, srtp, register, username, password_sealed
           FROM (SELECT t.*, row_number() OVER (PARTITION BY org_id ORDER BY position, id) AS n
                   FROM telephony_trunks t WHERE enabled) t
          WHERE n <= $1 ORDER BY org_id, position, id",
    )
    .bind(delonix_meet_domain::telephony::trunk::MAX_TRUNKS_PER_ORG)
    .fetch_all(&state.db)
    .await?;
    // O host só é verificado quando o tronco se GRAVA (`telephony_trunks`):
    // um nome que então não resolvia foi aceite, e o FreeSWITCH resolve-o por
    // conta própria a cada registo. Voltar a perguntar ao DNS aqui punha o DNS
    // de uma organização a atrasar os troncos de todas, e não fechava nada —
    // um gateway já carregado nunca volta a ser verificado. Ver a R291.
    let mut specs = Vec::with_capacity(rows.len());
    for r in rows {
        // Um segredo que não abre não derruba os gateways das outras orgs.
        let password = match crate::secrets_at_rest::open(
            &state.config,
            &r.password_sealed,
            &password_aad(&r.id),
        ) {
            Ok(p) => p,
            Err(_) => continue,
        };
        specs.push(GatewaySpec {
            trunk_id: r.id,
            org_id: r.org_id,
            host: r.host,
            port: r.port,
            transport: r.transport,
            srtp: r.srtp,
            register: r.register,
            username: r.username,
            password,
        });
    }
    Ok(xml(gateways_directory(&specs)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_text_cannot_reference_a_freeswitch_global() {
        let g = gateways_directory(&[GatewaySpec {
            trunk_id: Uuid::from_u128(7),
            org_id: Uuid::nil(),
            host: "sip.exemplo.ao".into(),
            port: 5060,
            transport: "udp".into(),
            srtp: "off".into(),
            register: true,
            username: "$${delonix_voice_secret}".into(),
            password: "pa$$${delonix_voice_secret}".into(),
        }]);
        // O pré-processador do FreeSWITCH procura `$${`: não pode lá estar.
        assert!(!g.contains("$${"), "{g}");
        assert!(!g.contains('$'), "{g}");
        // E o valor continua a ser o que o inquilino escreveu, depois de lido.
        assert!(
            g.contains(r#"name="username" value="&#36;&#36;{delonix_voice_secret}""#),
            "{g}"
        );
    }

    #[test]
    fn route_extension_has_limits_failover_and_recording() {
        let (org, a, b) = (Uuid::nil(), Uuid::from_u128(1), Uuid::from_u128(2));
        let x = dialplan_extension(
            org,
            "923447108",
            "244923447108",
            ResolutionOutcome::Route,
            Some(RuleAction::External),
            Some(0),
            &[
                LegSpec {
                    trunk_id: a,
                    max_channels: 60,
                },
                LegSpec {
                    trunk_id: b,
                    max_channels: 30,
                },
            ],
            true,
            false,
        );
        assert!(x.contains(r#"<context name="delonix-outbound">"#));
        assert!(x.contains(r#"expression="^923447108$""#));
        let ia = x
            .find(&format!(
                "hash delonix_trunk {a} 60 bridge [delonix_trunk_id={a},force_process_cdr=true,execute_on_originate_1=set process_cdr=true,execute_on_originate_2=unset switch_m_sdp,execute_on_post_bridge=unset switch_m_sdp,outbound_redirect_fatal=true]sofia/gateway/dlx-{a}/244923447108"
            ))
            .unwrap();
        let ib = x
            .find(&format!(
                "hash delonix_trunk {b} 30 bridge [delonix_trunk_id={b},force_process_cdr=true,execute_on_originate_1=set process_cdr=true,execute_on_originate_2=unset switch_m_sdp,execute_on_post_bridge=unset switch_m_sdp,outbound_redirect_fatal=true]sofia/gateway/dlx-{b}/244923447108"
            ))
            .unwrap();
        assert!(ia < ib, "a ordem de failover é a da resolução");
        // Nada na dial string depende de uma expansão: o texto é expandido
        // na perna de quem marca, e um `${uuid}` saía com o identificador dela.
        assert!(!x.contains("uuid_setvar") && !x.contains("origination_uuid"));
        assert!(x.contains("record_session"));
        assert!(x.contains("delonix_record=true"));
    }

    #[test]
    fn emergency_extension_is_never_recorded_nor_limited() {
        let a = Uuid::from_u128(1);
        let x = dialplan_extension(
            Uuid::nil(),
            "112",
            "112",
            ResolutionOutcome::Route,
            Some(RuleAction::External),
            None,
            &[LegSpec {
                trunk_id: a,
                max_channels: 1,
            }],
            true, // mesmo que chegasse true
            true,
        );
        assert!(!x.contains("record_session"));
        assert!(x.contains("delonix_record=false"));
        assert!(!x.contains("limit_execute"));
        assert!(x.contains(&format!(
            r#"application="bridge" data="[delonix_trunk_id={a},force_process_cdr=true,execute_on_originate_1=set process_cdr=true,execute_on_originate_2=unset switch_m_sdp,execute_on_post_bridge=unset switch_m_sdp,outbound_redirect_fatal=true]sofia/gateway/dlx-{a}/112""#
        )));
    }

    #[test]
    fn refusals_and_escaping() {
        let x = dialplan_extension(
            Uuid::nil(),
            "0800",
            "0800",
            ResolutionOutcome::Blocked,
            Some(RuleAction::Block),
            Some(2),
            &[],
            false,
            false,
        );
        assert!(x.contains(r#"application="respond" data="403 Forbidden""#));
        let x = dialplan_extension(
            Uuid::nil(),
            "+27",
            "27",
            ResolutionOutcome::NoMatch,
            None,
            None,
            &[],
            false,
            false,
        );
        assert!(x.contains(r#"expression="^\+27$""#));
        assert!(x.contains("404 Not Found"));
        let g = gateways_directory(&[GatewaySpec {
            trunk_id: Uuid::nil(),
            org_id: Uuid::nil(),
            host: "sip.unitel.ao".into(),
            port: 5061,
            transport: "tls".into(),
            srtp: "mandatory".into(),
            register: true,
            username: "delonix".into(),
            password: "p\"<&>'".into(),
        }]);
        assert!(g.contains(r#"<param name="password" value="p&quot;&lt;&amp;&gt;&apos;"/>"#));
        assert!(g.contains(r#"value="sip.unitel.ao:5061;transport=tls""#));
        assert!(g.contains(r#"<variable name="rtp_secure_media" value="mandatory""#));
    }
}
