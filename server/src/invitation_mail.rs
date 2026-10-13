//! O convite de organização **por correio** (E2, D7/ADR-0025).
//!
//! **PORQUE EXISTE.** O `Purpose::Invitation` da espinha do correio (#296)
//! estava declarado e **sem consumidor nenhum**: o convite continuava a sair
//! sempre `manual`, com o `///` da resposta a afirmar «o servidor não envia
//! correio (medido: zero SMTP)» — verdade quando foi escrito, falsa desde que a
//! espinha entrou. Isto fecha o convite do E2 e põe o contrato a dizer a
//! verdade.
//!
//! **O ENVIO É BEST-EFFORT, DE PROPÓSITO.** Ao contrário da prova de endereço
//! (`email_verification`) e da recuperação de password (`password_recovery`),
//! que **existem** para mandar um email e por isso devolvem `mail.disabled`
//! quando não há relay, o convite **já funcionava sem correio**: o
//! administrador copia o link ou dita o código. Propagar o erro aqui partia
//! esse fluxo em todos os servidores sem relay — incluindo o laboratório.
//! Logo: sem relay, sem `PUBLIC_URL`, ou com a fila a recusar, o convite é
//! criado e o `delivery_channel` diz **`manual`**. O token continua a vir na
//! resposta nos dois casos, porque a consola oferece «copiar link» e quem
//! convida pode querer entregá-lo à mão mesmo com o email a sair.
//!
//! O texto vai na língua da conta de quem é convidado, se já tiver conta;
//! `pt` por omissão, como no `password_recovery`. Texto simples: não há HTML a
//! escapar nem imagens a seguir.

use uuid::Uuid;

use crate::AppState;

/// O que a resposta da API diz ter acontecido.
const POR_EMAIL: &str = "email";
const A_MAO: &str = "manual";

/// O link que vai no email. **Sai do `PUBLIC_URL`, nunca do `Host` do pedido**
/// — a mesma regra do `email_verification` (ver `Config::public_url`): um
/// `Host` forjado punha o nosso token num domínio alheio.
fn link(public_url: &str, token: &str) -> String {
    format!("{public_url}/#/invite/{token}")
}

/// O texto, por língua e por forma de entrega. Um convite por `code` não leva
/// link: leva o código, porque é para ser digitado (ou ditado ao telefone).
fn mensagem(
    locale: &str,
    org: &str,
    token: &str,
    delivery: &str,
    public_url: &str,
    horas: i64,
) -> (String, String) {
    let en = locale.starts_with("en");
    let por_codigo = delivery == "code";
    if en {
        let assunto = format!("You have been invited to {org} on Delonix Meet");
        let corpo = if por_codigo {
            format!(
                "You have been invited to join {org} on Delonix Meet.\n\nYour invitation \
                 code is:\n\n    {token}\n\nEnter it in the app within {horas} hours. If you \
                 were not expecting this invitation, ignore this message.\n"
            )
        } else {
            format!(
                "You have been invited to join {org} on Delonix Meet.\n\nOpen this link \
                 within {horas} hours to accept:\n\n{}\n\nIf you were not expecting this \
                 invitation, ignore this message.\n",
                link(public_url, token)
            )
        };
        (assunto, corpo)
    } else {
        let assunto = format!("Foi convidado para {org} no Delonix Meet");
        let corpo = if por_codigo {
            format!(
                "Foi convidado para entrar em {org} no Delonix Meet.\n\nO seu código de \
                 convite é:\n\n    {token}\n\nIntroduza-o na aplicação nas próximas {horas} \
                 horas. Se não estava à espera deste convite, ignore esta mensagem.\n"
            )
        } else {
            format!(
                "Foi convidado para entrar em {org} no Delonix Meet.\n\nAbra este link nas \
                 próximas {horas} horas para aceitar:\n\n{}\n\nSe não estava à espera deste \
                 convite, ignore esta mensagem.\n",
                link(public_url, token)
            )
        };
        (assunto, corpo)
    }
}

/// Manda o convite por correio se for possível, e diz por que canal foi.
///
/// **Nunca falha**: devolve `"manual"` em vez de erro. Ver a nota do topo do
/// módulo — o convite tem de continuar a funcionar num servidor sem relay.
pub(crate) async fn send_best_effort(
    state: &AppState,
    org_id: Uuid,
    email: &str,
    token: &str,
    delivery: &str,
    horas: i64,
) -> &'static str {
    if !crate::mail::enabled(state) {
        return A_MAO;
    }
    let Some(public_url) = state.config.public_url.clone() else {
        // Com relay mas sem `PUBLIC_URL` o link não se consegue montar. Não é
        // um erro do pedido: o convite vale, entrega-se à mão.
        tracing::warn!("convite: há relay mas falta PUBLIC_URL — fica entrega manual");
        return A_MAO;
    };

    // A língua de quem RECEBE, se já tiver conta. Sem conta é `pt`, como no
    // `password_recovery`. Não revela nada: o resultado é só o idioma do texto.
    let locale: String = sqlx::query_scalar(
        "SELECT COALESCE(locale, 'pt') FROM users WHERE lower(email) = lower($1)",
    )
    .bind(email)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| "pt".into());

    let org: String = sqlx::query_scalar("SELECT name FROM organizations WHERE id = $1")
        .bind(org_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "Delonix Meet".into());

    let (subject, body) = mensagem(&locale, &org, token, delivery, &public_url, horas);
    match crate::mail::enqueue(
        state,
        crate::mail::Outgoing {
            // A org fica na linha: serve a justiça entre inquilinos da fila e
            // a auditoria de quem convidou quem.
            org_id: Some(org_id),
            purpose: crate::mail::Purpose::Invitation,
            to: email,
            subject: &subject,
            body: &body,
            // Pedido à mão: quem pede outra vez QUER outra mensagem.
            dedupe_key: None,
        },
    )
    .await
    {
        Ok(_) => POR_EMAIL,
        Err(e) => {
            // A fila recusou (relay removido a meio, base em apuros). O convite
            // já existe e o token vai na resposta: não se perde nada.
            tracing::warn!(error = ?e, "convite: não entrou na fila do correio — fica manual");
            A_MAO
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convite_por_link_leva_o_link_e_nao_o_token_cru() {
        let (assunto, corpo) = mensagem("pt", "Kaeso", "dlxi_abc", "link", "https://m.ao", 48);
        assert!(assunto.contains("Kaeso"));
        assert!(corpo.contains("https://m.ao/#/invite/dlxi_abc"));
        assert!(corpo.contains("48"));
    }

    #[test]
    fn convite_por_codigo_leva_o_codigo_e_nenhum_link() {
        let (_, corpo) = mensagem("pt", "Kaeso", "ABCDEFGHIJ", "code", "https://m.ao", 24);
        assert!(corpo.contains("ABCDEFGHIJ"));
        assert!(
            !corpo.contains("#/invite/"),
            "um convite por código não leva link: {corpo}"
        );
    }

    #[test]
    fn a_lingua_da_conta_escolhe_o_texto() {
        let (pt, _) = mensagem("pt", "Kaeso", "t", "link", "https://m.ao", 48);
        let (en, _) = mensagem("en-GB", "Kaeso", "t", "link", "https://m.ao", 48);
        assert!(pt.starts_with("Foi convidado"), "{pt}");
        assert!(en.starts_with("You have been invited"), "{en}");
    }
}
