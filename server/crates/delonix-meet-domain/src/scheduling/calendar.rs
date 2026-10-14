//! iCalendar (RFC 5545) — o texto de um `VEVENT`, e só ele.
//!
//! **Porque é aqui.** O `.ics` nasceu dentro do handler da descarga
//! (`meetings::ics`): servia para quem clicasse «exportar». Para o convite por
//! correio aterrar no Outlook ou no Google é preciso o MESMO evento com
//! `METHOD:REQUEST` dentro do email, e copiar o `format!` para o módulo do
//! correio era exactamente a cópia de regra que a catraca da arquitectura
//! persegue. Fica num sítio, puro, sem base de dados e sem rede.
//!
//! **O que o `METHOD` muda, e porque importa:**
//!
//! - `PUBLISH` — «aqui tens um evento». É o que a descarga do `.ics` serve, e
//!   não pede resposta a ninguém.
//! - `REQUEST` — é um CONVITE: o cliente de calendário mostra «aceitar /
//!   recusar» e, com o mesmo `UID` e um `SEQUENCE` maior, SUBSTITUI o evento
//!   que já tinha em vez de criar outro. É isto que falta a um convite nosso
//!   para ele deixar de ser um link que alguém copia à mão.
//! - `CANCEL` — tira o evento da agenda (com `STATUS:CANCELLED`).
//!
//! **Limite conhecido:** não dobra linhas aos 75 octetos (RFC 5545 §3.1). O
//! código que isto substitui também não dobrava, e os clientes que usamos
//! aceitam linhas longas; um título muito comprido é o caso a medir antes de
//! prometer o contrário.

use chrono::{DateTime, Utc};

/// Para que serve este calendário. Ver o cabeçalho do módulo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Publish,
    Request,
    Cancel,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Publish => "PUBLISH",
            Method::Request => "REQUEST",
            Method::Cancel => "CANCEL",
        }
    }
}

/// Um evento de calendário, já com os valores resolvidos por quem sabe lê-los
/// da base — este módulo não lê nada.
#[derive(Debug, Clone)]
pub struct Event<'a> {
    /// Identidade do evento para o cliente de calendário, estável ao longo da
    /// vida da reunião: é o que permite a uma versão nova substituir a antiga.
    pub uid: &'a str,
    pub dtstamp: DateTime<Utc>,
    /// Revisão (RFC 5545 §3.8.7.4). Um cliente só aceita uma versão nova de um
    /// evento que já conhece se este número SUBIR — sem ele, o `.ics` de uma
    /// reunião remarcada era ignorado em silêncio.
    pub sequence: i32,
    pub starts_at: DateTime<Utc>,
    pub duration_min: i32,
    pub title: &'a str,
    pub description: &'a str,
    /// O link da sala. **Absoluto ou nada**: um `.ics` é lido noutro programa,
    /// dias depois, e um caminho relativo lá dentro não aponta para lado nenhum.
    pub url: Option<&'a str>,
    /// Quem organiza (endereço). Num `REQUEST` sem organizador, o Outlook e o
    /// Gmail mostram o evento mas não mostram «aceitar».
    pub organizer: Option<&'a str>,
    /// Quem participa (endereços).
    ///
    /// **Quantos entram aqui é decisão de QUEM CHAMA, não deste módulo.** Um
    /// convite por correio leva UM — cada pessoa recebe o seu, e assim um
    /// destinatário não fica a saber a lista dos outros. A descarga do `.ics`
    /// leva TODOS, porque quem a pede já pode ler a lista de convidados pela
    /// API (o mesmo guarda serve as duas rotas) e um evento de calendário com
    /// os participantes é metade da utilidade dele.
    ///
    /// Vazio = nenhum `ATTENDEE`, que era o comportamento antes disto existir.
    pub attendees: &'a [&'a str],
}

/// Escapa um valor de texto (RFC 5545 §3.3.11).
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

/// Carimbo em UTC, na forma que a norma pede.
fn stamp(t: &DateTime<Utc>) -> String {
    t.format("%Y%m%dT%H%M%SZ").to_string()
}

/// O texto completo de um `VCALENDAR` com um `VEVENT`.
///
/// Com `Method::Publish` e sem organizador nem participante, a saída é a mesma,
/// byte por byte, que a descarga do `.ics` dava antes de este módulo existir —
/// está preso por teste, porque é um formato que outros programas lêem.
pub fn ics(event: &Event<'_>, method: Method) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Delonix Meet//PT\r\nMETHOD:");
    out.push_str(method.as_str());
    out.push_str("\r\nBEGIN:VEVENT\r\nUID:");
    out.push_str(event.uid);
    out.push_str("\r\nDTSTAMP:");
    out.push_str(&stamp(&event.dtstamp));
    out.push_str("\r\nSEQUENCE:");
    out.push_str(&event.sequence.to_string());
    out.push_str("\r\nDTSTART:");
    out.push_str(&stamp(&event.starts_at));
    out.push_str("\r\nDURATION:PT");
    out.push_str(&event.duration_min.to_string());
    out.push_str("M\r\nSUMMARY:");
    out.push_str(&escape(event.title));
    out.push_str("\r\nDESCRIPTION:");
    out.push_str(&escape(event.description));
    out.push_str("\r\n");
    if let Some(url) = event.url {
        out.push_str("URL:");
        out.push_str(url);
        out.push_str("\r\n");
    }
    if let Some(organizer) = event.organizer {
        out.push_str("ORGANIZER:mailto:");
        out.push_str(organizer);
        out.push_str("\r\n");
    }
    for attendee in event.attendees {
        // `RSVP=TRUE` só num convite: num `PUBLISH` ninguém responde, e num
        // `CANCEL` pedir resposta não faz sentido.
        out.push_str(match method {
            Method::Request => "ATTENDEE;RSVP=TRUE;PARTSTAT=NEEDS-ACTION:mailto:",
            _ => "ATTENDEE:mailto:",
        });
        out.push_str(attendee);
        out.push_str("\r\n");
    }
    if method == Method::Cancel {
        out.push_str("STATUS:CANCELLED\r\n");
    }
    out.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn em(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn evento<'a>() -> Event<'a> {
        Event {
            uid: "11111111-1111-1111-1111-111111111111@delonix-meet",
            dtstamp: em("2026-10-14T09:00:00Z"),
            sequence: 0,
            starts_at: em("2026-10-20T14:30:00Z"),
            duration_min: 45,
            title: "Revisão trimestral",
            description: "Delonix Meet — entrar: https://meet.exemplo.ao/#/r/abc-defg-hij",
            url: Some("https://meet.exemplo.ao/#/r/abc-defg-hij"),
            organizer: None,
            attendees: &[],
        }
    }

    /// O formato está PRESO: outros programas lêem isto, e uma mudança de
    /// ordem ou de linha é uma mudança de contrato. Este é o texto que a
    /// descarga do `.ics` dava antes de o módulo existir.
    #[test]
    fn o_publish_e_o_mesmo_byte_a_byte_que_a_descarga_dava() {
        assert_eq!(
            ics(&evento(), Method::Publish),
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Delonix Meet//PT\r\nMETHOD:PUBLISH\r\n\
             BEGIN:VEVENT\r\nUID:11111111-1111-1111-1111-111111111111@delonix-meet\r\n\
             DTSTAMP:20261014T090000Z\r\nSEQUENCE:0\r\nDTSTART:20261020T143000Z\r\n\
             DURATION:PT45M\r\nSUMMARY:Revisão trimestral\r\n\
             DESCRIPTION:Delonix Meet — entrar: https://meet.exemplo.ao/#/r/abc-defg-hij\r\n\
             URL:https://meet.exemplo.ao/#/r/abc-defg-hij\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        );
    }

    #[test]
    fn o_convite_pede_resposta_e_diz_quem_organiza() {
        let mut e = evento();
        e.organizer = Some("ana@exemplo.ao");
        e.attendees = &["maria@outra.ao"];
        let s = ics(&e, Method::Request);
        assert!(s.contains("METHOD:REQUEST\r\n"), "{s}");
        assert!(s.contains("ORGANIZER:mailto:ana@exemplo.ao\r\n"), "{s}");
        assert!(
            s.contains("ATTENDEE;RSVP=TRUE;PARTSTAT=NEEDS-ACTION:mailto:maria@outra.ao\r\n"),
            "{s}"
        );
        assert!(!s.contains("STATUS:CANCELLED"), "um convite não é um corte");
    }

    #[test]
    fn o_cancelamento_tira_o_evento_da_agenda_e_nao_pede_resposta() {
        let mut e = evento();
        e.attendees = &["maria@outra.ao"];
        let s = ics(&e, Method::Cancel);
        assert!(s.contains("METHOD:CANCEL\r\n"), "{s}");
        assert!(s.contains("STATUS:CANCELLED\r\n"), "{s}");
        assert!(s.contains("ATTENDEE:mailto:maria@outra.ao\r\n"), "{s}");
        assert!(!s.contains("RSVP"), "num cancelamento não se pede resposta");
    }

    /// A revisão é o que faz um cliente aceitar a versão nova: se isto não
    /// subir, o Outlook e o Google ignoram o evento remarcado em silêncio.
    #[test]
    fn a_revisao_entra_no_texto() {
        let mut e = evento();
        e.sequence = 7;
        assert!(ics(&e, Method::Request).contains("SEQUENCE:7\r\n"));
    }

    #[test]
    fn escapa_o_que_a_norma_manda_escapar() {
        let mut e = evento();
        e.title = "Plano; custos, 2026\\2027\ne depois";
        e.description = "a, b; c";
        let s = ics(&e, Method::Publish);
        assert!(
            s.contains("SUMMARY:Plano\\; custos\\, 2026\\\\2027\\ne depois\r\n"),
            "{s}"
        );
        assert!(s.contains("DESCRIPTION:a\\, b\\; c\r\n"), "{s}");
        // Uma linha a mais no meio do VEVENT quebraria o ficheiro para quem o lê.
        assert_eq!(s.matches("\r\n").count(), s.lines().count());
    }

    #[test]
    fn sem_link_nao_ha_linha_de_url() {
        let mut e = evento();
        e.url = None;
        assert!(!ics(&e, Method::Publish).contains("URL:"));
    }
}
