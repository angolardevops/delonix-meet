//! SMS de reunião: convite ao agendar e lembrete N minutos antes (ADR-0005,
//! extensão «contactos»).
//!
//! As mesmas regras do envio a contacto, chamadas — não copiadas:
//! - destinatário é membro ACTIVO da org do anfitrião (`org::sms_recipients`);
//! - o consentimento é o de reuniões (`sms_meeting_opt_out`) — `sms::recipient_phone`;
//! - quem agenda tem de poder enviar SMS a contactos (`sms::may_send`);
//! - a fila, a rota e a idempotência são as do `sms.rs` (`plan` + `insert`).
//!
//! O lembrete não tem daemon próprio: é um passo do worker de SMS que já existe
//! (`sms::spawn_worker`), e a reivindicação é atómica na base — dois pods não
//! lembram a mesma reunião duas vezes.

use chrono::{DateTime, FixedOffset, Utc};
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    error::ApiError,
    sms::{self, Purpose, Role, SendPolicy, Target},
    AppState,
};

/// Convidados por reunião a quem se manda SMS. Um SMS custa dinheiro e estes
/// não passam pelo limite por utilizador: o tecto é por reunião.
pub const MAX_RECIPIENTS_PER_MEETING: usize = 50;
/// Lembrete permitido, em minutos antes do início.
pub const REMINDER_MIN_RANGE: std::ops::RangeInclusive<i32> = 5..=1440;
/// Reivindicações por passo do varrimento.
const REMINDER_BATCH: i64 = 20;

/// Angola não muda de hora: WAT é sempre UTC+1.
fn wat() -> FixedOffset {
    FixedOffset::east_opt(3600).expect("UTC+1 é um desvio válido")
}

/// Troca as letras portuguesas que o GSM 03.38 não tem pela letra simples. Um
/// só «ç» passava a mensagem inteira a UCS-2 (70 caracteres por segmento em vez
/// de 160); num SMS de agenda, «Reuniao» lê-se igual e custa metade.
fn fold_to_gsm(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'â' | 'ã' => 'a',
            'Á' | 'Â' | 'Ã' | 'À' => 'A',
            'ê' | 'ë' => 'e',
            'Ê' | 'Ë' | 'È' => 'E',
            'í' | 'î' | 'ï' => 'i',
            'Í' | 'Î' | 'Ï' | 'Ì' => 'I',
            'ó' | 'ô' | 'õ' => 'o',
            'Ó' | 'Ô' | 'Õ' | 'Ò' => 'O',
            'ú' | 'û' => 'u',
            'Ú' | 'Û' | 'Ù' => 'U',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Invite,
    Reminder,
}

/// Texto do SMS em PT. Curto, com a hora em WAT e o link quando a org tem
/// domínio. Encolhe o TÍTULO (nunca a hora nem o link) até caber num segmento;
/// um título que nem assim cabe vai em dois — é o «quando possível».
pub fn meeting_text(
    template: Template,
    title: &str,
    starts_at: DateTime<Utc>,
    link: Option<&str>,
) -> String {
    let local = starts_at.with_timezone(&wat());
    let when = match template {
        Template::Invite => local.format("%d/%m as %H:%M WAT").to_string(),
        Template::Reminder => local.format("as %H:%M WAT").to_string(),
    };
    let link = link.map(|l| format!(" {l}")).unwrap_or_default();
    let title: Vec<char> = fold_to_gsm(title.trim()).chars().collect();
    let render = |t: &str| match template {
        Template::Invite => format!("Delonix Meet: convite para \"{t}\", {when}.{link}"),
        Template::Reminder => format!("Delonix Meet: \"{t}\" comeca {when}.{link}"),
    };
    let mut keep = title.len();
    loop {
        let t: String = if keep == title.len() {
            title.iter().collect()
        } else {
            format!("{}...", title[..keep].iter().collect::<String>().trim_end())
        };
        let text = render(&t);
        let one_segment = crate::sms_codec::encode(&text).is_ok_and(|e| e.parts.len() == 1);
        if one_segment || keep <= 12 {
            return text;
        }
        keep -= 1;
    }
}

#[derive(Debug, Serialize)]
pub struct SkippedRecipient {
    pub user_id: Uuid,
    /// Código estável: `sms.recipient_opted_out`, `sms.recipient_no_phone`,
    /// `sms.recipient_not_member`, `sms.too_many_recipients`, `sms.no_route`,
    /// `sms.invalid_body`.
    pub reason: &'static str,
}

#[derive(Debug, Default, Serialize)]
pub struct Delivery {
    pub queued: usize,
    pub skipped: Vec<SkippedRecipient>,
}

/// O que o agendamento devolve sobre SMS.
#[derive(Debug, Serialize)]
pub struct MeetingSmsReport {
    pub invite: Option<Delivery>,
    pub reminder_min: Option<i32>,
}

/// A org em nome da qual o anfitrião manda SMS, se ele puder. Corre ANTES de a
/// reunião existir: pedir SMS sem permissão recusa o agendamento inteiro, em
/// vez de criar a reunião e ignorar em silêncio a opção que a pessoa marcou.
pub(crate) async fn authorize(state: &AppState, owner: Uuid) -> Result<Uuid, ApiError> {
    let org_id = crate::org::orgs_of_user(state, owner)
        .await
        .first()
        .copied()
        .ok_or_else(|| {
            ApiError::Unprocessable(
                "sms.no_org: SMS de reunião só para quem pertence a uma organização".into(),
            )
        })?;
    let role = match crate::org::role_in_org(state, org_id, owner).await? {
        Some(r) if r == "admin" => Role::Admin,
        Some(_) => Role::Member,
        None => return Err(ApiError::Forbidden),
    };
    let policy: String =
        sqlx::query_scalar("SELECT sms_send_policy FROM organizations WHERE id = $1")
            .bind(org_id)
            .fetch_one(&state.db)
            .await?;
    if !sms::may_send(&Target::Contact(owner), role, SendPolicy::parse(&policy)) {
        return Err(ApiError::Forbidden);
    }
    Ok(org_id)
}

async fn meeting_link(state: &AppState, org_id: Uuid) -> Option<String> {
    let domain: String = sqlx::query_scalar("SELECT domain FROM organizations WHERE id = $1")
        .bind(org_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    (!domain.is_empty()).then(|| format!("https://{domain}/#/calendar"))
}

pub(crate) struct MeetingRef<'a> {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub title: &'a str,
    pub starts_at: DateTime<Utc>,
}

/// Enfileira um SMS por convidado elegível. Nunca falha por um convidado: cada
/// recusa sai em `skipped` com o código. A chave de idempotência torna a
/// repetição inofensiva (o lembrete leva a hora, para uma reunião remarcada
/// voltar a lembrar).
pub(crate) async fn queue_for_meeting(
    state: &AppState,
    org_id: Uuid,
    meeting: &MeetingRef<'_>,
    invitee_ids: &[Uuid],
    purpose: Purpose,
) -> Result<Delivery, ApiError> {
    let mut ids: Vec<Uuid> = Vec::new();
    for id in invitee_ids {
        if *id != meeting.owner_id && !ids.contains(id) {
            ids.push(*id);
        }
    }
    let mut delivery = Delivery::default();
    if ids.len() > MAX_RECIPIENTS_PER_MEETING {
        for id in ids.drain(MAX_RECIPIENTS_PER_MEETING..) {
            delivery.skipped.push(SkippedRecipient {
                user_id: id,
                reason: "sms.too_many_recipients",
            });
        }
    }
    let recipients: HashMap<Uuid, crate::org::SmsRecipient> =
        crate::org::sms_recipients(state, org_id, &ids)
            .await?
            .into_iter()
            .map(|r| (r.user_id, r))
            .collect();
    let (template, key_kind) = match purpose {
        Purpose::MeetingReminder => (Template::Reminder, "meeting-reminder"),
        _ => (Template::Invite, "meeting-invite"),
    };
    let link = meeting_link(state, org_id).await;
    let text = meeting_text(template, meeting.title, meeting.starts_at, link.as_deref());

    for user_id in ids {
        let skip = |reason| SkippedRecipient { user_id, reason };
        let Some(recipient) = recipients.get(&user_id) else {
            delivery.skipped.push(skip("sms.recipient_not_member"));
            continue;
        };
        let phone = match sms::recipient_phone(recipient, purpose) {
            Ok(p) => p,
            Err(refusal) => {
                delivery.skipped.push(skip(refusal.code()));
                continue;
            }
        };
        let planned = match sms::plan(state, org_id, phone, &text, "auto").await {
            Ok(p) => p,
            Err(ApiError::Unprocessable(reason)) if reason.starts_with("sem rota") => {
                delivery.skipped.push(skip("sms.no_route"));
                continue;
            }
            Err(ApiError::Unprocessable(_)) | Err(ApiError::BadRequest(_)) => {
                delivery.skipped.push(skip("sms.invalid_body"));
                continue;
            }
            Err(e) => return Err(e),
        };
        let origin = sms::Origin {
            org_id,
            created_by: Some(meeting.owner_id),
            purpose,
            recipient_user_id: Some(user_id),
            meeting_id: Some(meeting.id),
            idempotency_key: Some(format!(
                "{key_kind}:{}:{user_id}:{}",
                meeting.id,
                meeting.starts_at.timestamp()
            )),
        };
        // `None` = já estava na fila (repetição): conta como enfileirada.
        sms::insert(state, &origin, &planned).await?;
        delivery.queued += 1;
    }

    // Sem números na trilha: o alvo é a reunião, e os totais.
    crate::audit::log(
        &state.db,
        Some(org_id),
        meeting.owner_id,
        match purpose {
            Purpose::MeetingReminder => "sms.meeting_reminder_queued",
            _ => "sms.meeting_invite_queued",
        },
        &format!(
            "{} queued={} skipped={}",
            meeting.id,
            delivery.queued,
            delivery.skipped.len()
        ),
    )
    .await;
    Ok(delivery)
}

/// Um passo do varrimento de lembretes. Reivindica as reuniões vencidas
/// (marcando-as ANTES de enviar — no máximo uma vez) e enfileira o lembrete a
/// quem não recusou o convite. Uma reunião que já começou quando o varrimento a
/// apanha (servidor em baixo) fica marcada e não se lembra: um lembrete depois
/// da hora é ruído pago.
pub async fn remind_due(state: &AppState) -> Result<usize, ApiError> {
    let due: Vec<(Uuid, Uuid, String, DateTime<Utc>)> = sqlx::query_as(
        "UPDATE meetings SET sms_reminder_done_at = now()
         WHERE id IN (
             SELECT id FROM meetings
             WHERE sms_reminder_min IS NOT NULL AND sms_reminder_done_at IS NULL
               AND starts_at - make_interval(mins => sms_reminder_min) <= now()
             ORDER BY starts_at LIMIT $1
             FOR UPDATE SKIP LOCKED)
         RETURNING id, owner_id, title, starts_at",
    )
    .bind(REMINDER_BATCH)
    .fetch_all(&state.db)
    .await?;
    let mut queued = 0;
    for (id, owner_id, title, starts_at) in due {
        if starts_at <= Utc::now() {
            tracing::info!(meeting = %id, "SMS: lembrete fora de horas — não enviado");
            continue;
        }
        // A permissão volta a ver-se: entre agendar e lembrar, o anfitrião pode
        // ter saído da org ou a política pode ter fechado.
        let org_id = match authorize(state, owner_id).await {
            Ok(o) => o,
            Err(e) => {
                tracing::info!(meeting = %id, error = %e, "SMS: lembrete recusado — o anfitrião já não pode enviar");
                continue;
            }
        };
        let invitees: Vec<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM meeting_invitees WHERE meeting_id = $1 AND status <> 'declined'",
        )
        .bind(id)
        .fetch_all(&state.db)
        .await?;
        let meeting = MeetingRef {
            id,
            owner_id,
            title: &title,
            starts_at,
        };
        let d =
            queue_for_meeting(state, org_id, &meeting, &invitees, Purpose::MeetingReminder).await?;
        tracing::info!(meeting = %id, queued = d.queued, skipped = d.skipped.len(), "SMS: lembrete de reunião");
        queued += d.queued;
    }
    Ok(queued)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 17, h, m, 0).unwrap()
    }

    #[test]
    fn hour_is_written_in_wat() {
        // 09:30 UTC são 10:30 em Luanda.
        let t = meeting_text(Template::Reminder, "Coordenacao", at(9, 30), None);
        assert!(t.contains("as 10:30 WAT"), "{t}");
        let t = meeting_text(Template::Invite, "Coordenacao", at(23, 30), None);
        assert!(t.contains("18/09 as 00:30 WAT"), "virar o dia em WAT: {t}");
    }

    #[test]
    fn portuguese_title_stays_in_one_gsm_segment() {
        let link = "https://meet.kaeso.co.ao/#/calendar";
        let t = meeting_text(
            Template::Invite,
            "Reunião de coordenação — acções",
            at(8, 0),
            Some(link),
        );
        let enc = crate::sms_codec::encode(&t).unwrap();
        assert_eq!(enc.encoding.as_str(), "gsm7", "{t}");
        assert_eq!(enc.parts.len(), 1, "{t}");
        assert!(t.contains("Reuniao de coordenacao"), "{t}");
        assert!(t.ends_with(link), "o link nunca é cortado: {t}");
    }

    #[test]
    fn long_title_is_shortened_never_the_time_or_link() {
        let link = "https://meet.kaeso.co.ao/#/calendar";
        let title = "Revisao trimestral ".repeat(12);
        let t = meeting_text(Template::Reminder, &title, at(14, 0), Some(link));
        assert_eq!(crate::sms_codec::encode(&t).unwrap().parts.len(), 1, "{t}");
        assert!(
            t.contains("...") && t.contains("15:00 WAT") && t.ends_with(link),
            "{t}"
        );
    }

    #[test]
    fn without_domain_there_is_no_link() {
        let t = meeting_text(Template::Invite, "Diario", at(7, 0), None);
        assert!(!t.contains("http"), "{t}");
        assert!(t.ends_with("WAT."), "{t}");
    }
}
