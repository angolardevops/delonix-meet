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
//!   activos (o perfil sofia usa `<domain name="delonix-trunks" parse="true"/>`; `all` só lê o directório estático), com a
//!   password decifrada — por isso só no listener interno e com o segredo.
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

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
                            "[delonix_trunk_id={}]sofia/gateway/{gw}/{wire_number}",
                            leg.trunk_id
                        ),
                    );
                } else {
                    act(
                        "limit_execute",
                        format!(
                            "hash delonix_trunk {} {} bridge [delonix_trunk_id={}]sofia/gateway/{gw}/{wire_number}",
                            leg.trunk_id, leg.max_channels, leg.trunk_id
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

async fn dialplan(state: &AppState, form: &HashMap<String, String>) -> Result<Response, ApiError> {
    if form.get("Caller-Context").map(String::as_str) != Some(OUTBOUND_CONTEXT) {
        return Ok(xml(not_found()));
    }
    // A org: a variável que nós pusemos, ou o domínio SIP de quem liga.
    let org_id = match form
        .get("variable_delonix_org_id")
        .and_then(|v| Uuid::parse_str(v).ok())
    {
        Some(o) => Some(o),
        None => match form
            .get("variable_sip_req_host")
            .or_else(|| form.get("variable_domain_name"))
        {
            Some(host) => {
                sqlx::query_scalar(
                    "SELECT org_id FROM telephony_sip_settings WHERE lower(domain) = lower($1)",
                )
                .bind(host)
                .fetch_optional(&state.db)
                .await?
            }
            None => None,
        },
    };
    let Some(org_id) = org_id else {
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
        "SELECT id, org_id, host, port, transport, srtp, register, username, password_sealed
           FROM telephony_trunks WHERE enabled ORDER BY org_id, position, id",
    )
    .fetch_all(&state.db)
    .await?;
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
                "hash delonix_trunk {a} 60 bridge [delonix_trunk_id={a}]sofia/gateway/dlx-{a}/244923447108"
            ))
            .unwrap();
        let ib = x
            .find(&format!(
                "hash delonix_trunk {b} 30 bridge [delonix_trunk_id={b}]sofia/gateway/dlx-{b}/244923447108"
            ))
            .unwrap();
        assert!(ia < ib, "a ordem de failover é a da resolução");
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
            r#"application="bridge" data="[delonix_trunk_id={a}]sofia/gateway/dlx-{a}/112""#
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
