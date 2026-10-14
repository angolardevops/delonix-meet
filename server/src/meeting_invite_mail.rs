//! O convite de uma REUNIÃO para um endereço **externo** — alguém que não tem
//! conta aqui.
//!
//! **PORQUE EXISTE.** O `.ics` no convite entrou no #348, mas só chegava a quem
//! já era utilizador: a `meeting_invitees` tem chave `(meeting_id, user_id)`, e
//! o caminho de entrega é o centro de notificações, que exige conta,
//! preferência por canal **e endereço provado**. Um convidado de fora não tem
//! nenhuma das três — e é precisamente o caso que se perde para o Teams
//! («convida a maria@outra-empresa.ao»). Era a outra metade da lacuna nº2 do
//! levantamento de 2026-10-13.
//!
//! **O ENVIO É BEST-EFFORT**, pelo mesmo critério do `invitation_mail`: a
//! reunião já existia sem correio, e quem convida pode entregar o link à mão.
//! Sem relay, sem `PUBLIC_URL` ou com a fila a recusar, o convidado fica
//! registado e a resposta diz `manual` — não se parte o agendamento num
//! servidor sem SMTP, incluindo o laboratório.
//!
//! **O que vai no email:** o título, a hora em UTC, o link da sala e o `.ics`
//! com `METHOD:REQUEST` (ver `scheduling::calendar`). A hora vai em UTC **de
//! propósito**: não sabemos o fuso de quem está do lado de fora, e o `.ics`
//! leva o instante exacto, que é o calendário dele a localizar. Texto simples:
//! não há HTML a escapar nem imagens a seguir.
//!
//! **E o que NÃO vai:** nada sobre a organização além do que o título disser, e
//! nenhuma lista de outros convidados — cada endereço recebe o seu convite, com
//! o seu `ATTENDEE`, para um externo não ficar a saber quem mais foi convidado.

use delonix_meet_domain::scheduling::calendar::Method as IcsMethod;

use crate::{meetings::Meeting, AppState};

/// O que a resposta da API diz ter acontecido com este convite.
pub(crate) const BY_EMAIL: &str = "email";
pub(crate) const BY_HAND: &str = "manual";

/// O texto da mensagem. Separado do envio para ser testável sem fila nem relay.
fn body(title: &str, starts_at: &str, duration_min: i32, link: Option<&str>) -> String {
    let mut t = format!(
        "Foi convidado para a reunião «{title}».\n\nQuando: {starts_at} (UTC)\nDuração: {duration_min} minutos\n"
    );
    if let Some(link) = link {
        t.push_str(&format!(
            "\nPara entrar, abra este link à hora da reunião:\n{link}\n\n\
             Não precisa de criar conta: escreva o seu nome e o anfitrião deixa-o entrar.\n"
        ));
    } else {
        // Sem link absoluto não se inventa um relativo: quem recebe isto está
        // fora e um `/#/r/...` não aponta para nada.
        t.push_str("\nO anfitrião vai enviar-lhe o link da sala.\n");
    }
    t.push_str("\nO convite em anexo entra na sua agenda.\n");
    t
}

/// Enfileira o convite para um endereço externo. Devolve o canal que a API
/// reporta: `email` se entrou na fila, `manual` se não há por onde sair.
///
/// **Nunca falha para quem chama.** Um convite que não consegue sair por email
/// não pode derrubar a criação da reunião.
pub(crate) async fn send_best_effort(
    state: &AppState,
    meeting: &Meeting,
    email: &str,
) -> &'static str {
    let org_id = crate::org::orgs_of_user(state, meeting.owner_id)
        .await
        .first()
        .copied();
    if !crate::mail::enabled_for(state, org_id).await {
        return BY_HAND;
    }
    let link = crate::meetings::room_link_absolute(state, meeting).await;
    let corpo = body(
        &meeting.title,
        &meeting.starts_at.format("%d/%m/%Y %H:%M").to_string(),
        meeting.duration_min,
        link.as_deref(),
    );
    // O `.ics` com o ATTENDEE DESTE endereço: é o que faz o cliente de
    // calendário mostrar «aceitar» em nome de quem recebeu.
    let ics = crate::meetings::ics_for(state, meeting, IcsMethod::Request, Some(email)).await;
    let assunto = format!("Convite: {}", meeting.title);
    // Com chave de dedupe: criar a reunião e logo remarcá-la não manda dois
    // convites iguais ao mesmo endereço. A hora entra na chave, por isso uma
    // hora NOVA manda convite novo — que é o que actualiza a agenda.
    let chave = format!(
        "meeting-ext:{}:{}",
        meeting.id,
        meeting.starts_at.timestamp()
    );
    match crate::mail::enqueue(
        state,
        crate::mail::Outgoing {
            org_id,
            purpose: crate::mail::Purpose::MeetingInvitation,
            to: email,
            subject: &assunto,
            body: &corpo,
            dedupe_key: Some(&chave),
            calendar: Some(crate::mail::Calendar {
                ics: &ics,
                method: IcsMethod::Request,
            }),
        },
    )
    .await
    {
        Ok(_) => BY_EMAIL,
        Err(e) => {
            tracing::warn!(meeting = %meeting.id, error = ?e,
                "convite externo não entrou na fila do correio (a reunião segue)");
            BY_HAND
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_corpo_diz_a_hora_a_duracao_e_que_nao_precisa_de_conta() {
        let t = body(
            "Revisão trimestral",
            "20/10/2026 14:30",
            45,
            Some("https://meet.exemplo.ao/#/r/abc"),
        );
        assert!(t.contains("«Revisão trimestral»"), "{t}");
        assert!(t.contains("20/10/2026 14:30 (UTC)"), "{t}");
        assert!(t.contains("45 minutos"), "{t}");
        assert!(t.contains("https://meet.exemplo.ao/#/r/abc"), "{t}");
        // A frase que faz a diferença para quem está de fora.
        assert!(t.contains("Não precisa de criar conta"), "{t}");
    }

    /// Sem link absoluto não se inventa um relativo: quem recebe está fora.
    #[test]
    fn sem_link_nao_mete_caminho_relativo_nenhum() {
        let t = body("Conselho", "20/10/2026 14:30", 30, None);
        assert!(!t.contains("/#/r/"), "{t}");
        assert!(t.contains("O anfitrião vai enviar-lhe o link"), "{t}");
    }
}
