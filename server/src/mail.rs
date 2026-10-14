//! Correio: a espinha do envio, e só ela.
//!
//! **PORQUE EXISTE.** Até aqui o servidor não enviava correio nenhum — a única
//! ocorrência de «smtp» em todo o `server/` era um comentário a dizer que não
//! enviava. Isso travava três itens do plano de lacunas: os convites (E2), a
//! reposição de password pela própria pessoa (E3) e o calendário (E7).
//!
//! **O D7 está decidido** (2026-10-09, ADR-0025): **relay do operador
//! primeiro**, SMTP por organização depois. Por isso aqui não há fornecedor por
//! inquilino, e isso é uma decisão de segurança, não de pressa: o `net_guard`
//! guarda **URLs HTTP** e não cobre uma ligação SMTP, que é `host:porta` em TCP
//! puro. Um host de SMTP escolhido pelo inquilino seria uma ligação de saída
//! arbitrária — varrer a rede interna, chegar ao `169.254.169.254`, servir de
//! sonda. O trabalho caro do «SMTP do cliente» é esse guarda, e ele ainda não
//! existe.
//!
//! **Este módulo não sabe nada sobre convites nem sobre passwords.** Quem quer
//! mandar uma mensagem chama `enqueue`; os casos de uso vivem nos seus módulos,
//! como o `sms_notify` vive ao lado do `sms`.
//!
//! **A fila é a peça comum** (`delonix_meet_core::jobs` +
//! `delonix_meet_store::jobs::claim_in`), como manda o
//! `check-filas-reivindicacao.sh`: não se escreve outro `FOR UPDATE SKIP
//! LOCKED` à mão. O livro de entregas É a fila — não há broker, e um reinício
//! não perde o que está agendado.

use std::sync::Arc;
use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::message::MessageBuilder;
use lettre::message::{MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use uuid::Uuid;

use delonix_meet_core::DomainError;
/// O `METHOD` de um calendário é definido no domínio (`scheduling::calendar`);
/// aqui só se re-exporta, para quem enfileira não precisar de dois caminhos.
pub use delonix_meet_domain::scheduling::calendar::Method as CalendarMethod;

use crate::{error::ApiError, AppState};

/// Quantas mensagens um passo do worker reclama de uma vez. Um relay que volta
/// depois de uma queda recebe-as em lotes, não numa rajada só.
const BATCH: i64 = 50;

/// A política de repetição dos webhooks, que é a completa da casa: cinco
/// tentativas, 30 s a 1 h, ±20 % de espalhamento. Um relay que esteve em baixo
/// uma hora não recebe tudo o que falhou no mesmo segundo.
const RETRY: delonix_meet_core::jobs::Retry = delonix_meet_core::jobs::RETRY_WEBHOOK;

/// Falha que repetir não cura: endereço inválido, remetente mal escrito,
/// configuração errada.
const PERMANENTE: delonix_meet_core::jobs::Failure = delonix_meet_core::jobs::Failure::Permanent;
/// Falha que pode passar sozinha: o relay em baixo, DNS, tempo-limite.
const TRANSITORIA: delonix_meet_core::jobs::Failure = delonix_meet_core::jobs::Failure::Transient;

/// Para que serve uma mensagem. Fica em texto na base (uma mensagem nova não
/// exige migração), mas quem enfileira passa por aqui — assim a lista das
/// mensagens que existem está num só sítio e não espalhada por literais.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Reposição de password pedida pela própria pessoa (E3, por fazer).
    PasswordReset,
    /// Convite para uma organização (E2, por fazer).
    Invitation,
    /// Confirmação de endereço de email (pré-requisito do E3).
    EmailVerification,
    /// Uma notificação do centro de notificações (G8) entregue por correio:
    /// convite para reunião, hora mudada, «a começar», gravação pronta. O que
    /// sai depende da preferência da pessoa por tipo e canal.
    Notification,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Purpose::PasswordReset => "password_reset",
            Purpose::Invitation => "invitation",
            Purpose::EmailVerification => "email_verification",
            Purpose::Notification => "notification",
        }
    }

    /// O catálogo FECHADO, para quem valida um filtro. Fica ao lado do
    /// `as_str` de propósito: acrescentar um propósito sem o pôr aqui faz o
    /// `match` abaixo não compilar.
    pub const TODOS: [&'static str; 4] = [
        "password_reset",
        "invitation",
        "email_verification",
        "notification",
    ];

    pub fn parse(s: &str) -> Option<Purpose> {
        Some(match s {
            "password_reset" => Purpose::PasswordReset,
            "invitation" => Purpose::Invitation,
            "email_verification" => Purpose::EmailVerification,
            "notification" => Purpose::Notification,
            _ => return None,
        })
    }
}

/// Um calendário a viajar DENTRO da mensagem.
///
/// Sem isto, um convite nosso é um link que alguém tem de copiar para a agenda à
/// mão — e é a razão número 2 por que se perde a comparação com o Teams
/// (levantamento de 2026-10-13). Com isto, o Outlook e o Google mostram
/// «aceitar/recusar» e, no remarcar, SUBSTITUEM o evento em vez de criar outro.
pub struct Calendar<'a> {
    /// O texto iCalendar já construído (`scheduling::calendar::ics`).
    pub ics: &'a str,
    /// O `METHOD` de dentro do texto. Vai TAMBÉM no `Content-Type` da parte,
    /// porque é de lá que os clientes de correio o lêem — um `REQUEST` sem ele
    /// é tratado como um anexo qualquer.
    pub method: CalendarMethod,
}

/// Uma mensagem a enfileirar.
pub struct Outgoing<'a> {
    /// A organização em cujo nome se envia, quando há uma. `None` para correio
    /// da plataforma ou de uma conta sem org.
    pub org_id: Option<Uuid>,
    pub purpose: Purpose,
    pub to: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
    /// Chave de dedupe, para quem enfileira a partir de um produtor
    /// **repetível** — o lembrete de reunião é um passo de varrimento e uma
    /// notificação pode nascer outra vez depois de um reinício. Com chave, a
    /// segunda tentativa para o mesmo endereço **não** cria mensagem nova.
    ///
    /// `None` para tudo o que é pedido à mão (reposição de password, convite,
    /// prova de endereço): aí quem pede outra vez QUER outra mensagem.
    pub dedupe_key: Option<&'a str>,
    /// Um convite de calendário a anexar. `None` na esmagadora maioria das
    /// mensagens — só as de reunião o levam.
    pub calendar: Option<Calendar<'a>>,
}

/// O relay DO OPERADOR está configurado?
///
/// É a resposta para o correio que não pertence a organização nenhuma. Para uma
/// mensagem de uma organização, a pergunta certa é [`enabled_for`]: desde o
/// SMTP por organização (ADR-0025), uma org com o seu próprio relay manda
/// correio mesmo que o operador não tenha nenhum.
pub fn enabled(state: &AppState) -> bool {
    state.config.smtp_host.is_some() && state.config.smtp_from.is_some()
}

/// O correio está configurado **para esta organização**: o relay dela, ou o do
/// operador.
pub async fn enabled_for(state: &AppState, org_id: Option<Uuid>) -> bool {
    if enabled(state) {
        return true;
    }
    match org_id {
        Some(org) => org_relay(state, org).await.is_some(),
        None => false,
    }
}

/// Enfileira uma mensagem. **Não envia**: o worker envia.
///
/// Separar as duas é o que torna a rota que chama isto rápida e honesta — uma
/// rota que esperasse pelo SMTP ficaria presa num relay lento, e um relay em
/// baixo passaria a ser um erro 500 na cara de quem pede a reposição.
///
/// Com o correio desligado devolve `mail.disabled` em vez de enfileirar
/// mensagens que ninguém vai enviar.
/// Devolve o id da mensagem criada, ou `None` quando a `dedupe_key` já tinha
/// mandado esta mensagem a este endereço — que não é um erro: é a defesa a
/// funcionar.
pub async fn enqueue(state: &AppState, msg: Outgoing<'_>) -> Result<Option<Uuid>, ApiError> {
    if !enabled_for(state, msg.org_id).await {
        return Err(DomainError::precondition(
            "mail.disabled",
            "o correio não está configurado neste servidor",
        )
        .into());
    }
    // Mesmo mecanismo da tabela `notifications`: com `dedupe_key`, a segunda
    // tentativa para o mesmo endereço não cria linha. O `fetch_optional` é o
    // que distingue «não criei porque já existia» de um erro.
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO mail_messages (org_id, purpose, to_address, subject, body_text, dedupe_key,
                                    calendar_ics, calendar_method)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (to_address, dedupe_key) WHERE dedupe_key IS NOT NULL DO NOTHING
         RETURNING id",
    )
    .bind(msg.org_id)
    .bind(msg.purpose.as_str())
    .bind(msg.to)
    .bind(msg.subject)
    .bind(msg.body)
    .bind(msg.dedupe_key)
    .bind(msg.calendar.as_ref().map(|c| c.ics))
    .bind(msg.calendar.as_ref().map(|c| c.method.as_str()))
    .fetch_optional(&state.db)
    .await?;
    Ok(id)
}

/// Por onde uma mensagem sai: o relay do operador, ou o da organização.
#[derive(Clone)]
pub(crate) struct Relay {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub starttls: bool,
    pub from: String,
    /// `true` quando o host foi escrito por um CLIENTE. Decide se a ligação
    /// passa pela guarda de saída: o relay do operador não precisa dela (ele
    /// declarou-o), o de uma organização precisa sempre.
    pub do_inquilino: bool,
}

/// O relay do operador, se estiver configurado.
pub(crate) fn operator_relay(state: &AppState) -> Option<Relay> {
    Some(Relay {
        host: state.config.smtp_host.clone()?,
        port: state.config.smtp_port,
        username: state.config.smtp_username.clone(),
        password: state.config.smtp_password.clone(),
        starttls: state.config.smtp_starttls,
        from: state.config.smtp_from.clone()?,
        do_inquilino: false,
    })
}

/// O relay desta organização, se tiver um LIGADO. A password abre-se aqui e
/// não sai deste módulo.
pub(crate) async fn org_relay(state: &AppState, org_id: Uuid) -> Option<Relay> {
    relay_da_org(state, org_id, true).await
}

/// O mesmo, **mesmo que esteja desligado**: serve a rota de verificação, que
/// existe para se provar uma configuração ANTES de a ligar.
pub(crate) async fn org_relay_any(state: &AppState, org_id: Uuid) -> Option<Relay> {
    relay_da_org(state, org_id, false).await
}

async fn relay_da_org(state: &AppState, org_id: Uuid, so_ligado: bool) -> Option<Relay> {
    let row: Option<(String, i32, String, String, bool, String)> = sqlx::query_as(
        "SELECT host, port, username, password_sealed, starttls, from_address
           FROM org_mail_settings WHERE org_id = $1 AND (enabled OR NOT $2)",
    )
    .bind(org_id)
    .bind(so_ligado)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let (host, port, username, sealed, starttls, from) = row?;
    let password = if sealed.is_empty() {
        None
    } else {
        // Se a abertura falhar (chave rodada sem resselar), o relay fica
        // INUTILIZÁVEL em vez de tentar sem password: tentar sem ela seria
        // entregar a mensagem a um relay aberto, se houver.
        match crate::secrets_at_rest::open(&state.config, &sealed, &aad(org_id)) {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(%org_id, error = ?e,
                    "SMTP da organização: a password não abriu — o relay dela não é usado");
                return None;
            }
        }
    };
    Some(Relay {
        host,
        port: port.clamp(1, u16::MAX as i32) as u16,
        username: Some(username).filter(|u| !u.is_empty()),
        password,
        starttls,
        from,
        do_inquilino: true,
    })
}

/// O AAD da password selada: preso ao `org_id`, para uma linha copiada para
/// outra organização não se abrir. Pelo helper da casa, não por um `format!`
/// próprio — o formato do AAD é uma regra e tem um só dono.
pub(crate) fn aad(org_id: Uuid) -> String {
    crate::secrets_at_rest::aad("org_mail_settings", "password_sealed", org_id)
}

/// Constrói o transporte para um relay.
///
/// **A diferença que importa está no `do_inquilino`.** Para o relay de uma
/// organização, o host foi escrito por um cliente, e por isso:
///
/// 1. passa pela guarda de saída (`resolve_tenant_target`), que resolve o nome
///    e aprova UM endereço;
/// 2. a ligação é feita a esse **endereço**, não ao nome — é isto que fecha a
///    janela do DNS rebinding, pela mesma razão que o `GuardedResolver` existe
///    para o HTTP;
/// 3. o certificado continua a ser validado contra o **NOME**, por
///    `TlsParameters::builder(nome)`. Ligar a um IP e validar o nome é o que dá
///    as duas garantias ao mesmo tempo.
///
/// O `builder_dangerous` tem esse nome por não pôr TLS sozinho; aqui o TLS é
/// posto explicitamente a seguir, excepto quando o relay o dispensa.
async fn transport(
    state: &AppState,
    r: &Relay,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    let mut b = if r.do_inquilino {
        let alvo = state
            .outbound
            .resolve_tenant_target(&r.host, r.port)
            .await
            .map_err(|e| format!("{e}"))?;
        let b = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(alvo.ip().to_string())
            .port(alvo.port());
        if r.starttls {
            let params = TlsParameters::builder(r.host.clone())
                .build()
                .map_err(|e| e.to_string())?;
            b.tls(Tls::Required(params))
        } else {
            b
        }
    } else if r.starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&r.host)
            .map_err(|e| e.to_string())?
            .port(r.port)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&r.host).port(r.port)
    }
    .timeout(Some(Duration::from_secs(20)));
    if let (Some(u), Some(p)) = (r.username.as_deref(), r.password.as_deref()) {
        b = b.credentials(Credentials::new(u.to_string(), p.to_string()));
    }
    Ok(b.build())
}

/// Liga-se ao relay e faz o diálogo de SMTP (incluindo a autenticação) **sem
/// mandar mensagem nenhuma**. É o que a rota de verificação usa: quem configura
/// um relay tem de poder saber se está certo sem esperar por um email perdido.
pub(crate) async fn test_relay(state: &AppState, r: &Relay) -> Result<(), String> {
    let tp = transport(state, r).await?;
    match tp.test_connection().await {
        Ok(true) => Ok(()),
        Ok(false) => Err("o relay não respondeu ao diálogo de SMTP".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// A fila, declarada uma vez.
///
/// `ready_when` apanha os dois casos no mesmo passo: as que nunca foram
/// enviadas (`pending`) e as que falharam e têm repetição vencida.
///
/// O `claim_set` põe `status = 'sending'`, que é a **marca de posse**. Tem de
/// ser um estado DIFERENTE do `pending`: com a marca a deixar a linha em
/// `pending`, ela continuava a bater no `ready_when` e era reclamada outra vez
/// a cada volta do worker — a mensagem sairia tantas vezes quantas as voltas.
/// Por isso existe também o varredor (`sweep`): se o processo morrer entre a
/// reivindicação e o resultado, a linha fica `sending` para sempre sem ele.
const FILA: delonix_meet_core::jobs::Queue = delonix_meet_core::jobs::Queue {
    name: "mail_send",
    table: "mail_messages",
    id_column: "id",
    ready_when: "status = 'pending' \
                 OR (status = 'failed' AND retry_at IS NOT NULL AND retry_at <= now())",
    claim_set: "status = 'sending', claimed_at = now(), retry_at = NULL",
    returning: "id, org_id, to_address, subject, body_text, calendar_ics, calendar_method, attempt",
    order_by: "created_at",
    tenant_column: Some("org_id"),
    batch: BATCH,
};

/// Põe o corpo na mensagem: texto simples, ou `multipart/alternative` com o
/// texto E o `text/calendar` quando a mensagem leva um convite.
///
/// **O `method` vai no `Content-Type` da parte**, e não só dentro do `.ics`: é
/// de lá que os clientes de correio o lêem. Um `REQUEST` sem ele é tratado como
/// um anexo qualquer — o convite aparece como ficheiro para descarregar em vez
/// de «aceitar/recusar», que é precisamente o que se está a corrigir.
///
/// A ordem das partes não é indiferente: num `multipart/alternative` o cliente
/// mostra a ÚLTIMA que entende, por isso o calendário vem depois do texto.
fn com_corpo(
    builder: MessageBuilder,
    body: &str,
    ics: Option<&str>,
    method: Option<&str>,
) -> Result<Message, lettre::error::Error> {
    let Some(ics) = ics else {
        return builder.body(body.to_owned());
    };
    // Vem da nossa própria base, mas escolhe-se de uma lista fechada: um valor
    // inesperado não vai montar um cabeçalho estranho.
    let method = match method {
        Some("PUBLISH") => "PUBLISH",
        Some("CANCEL") => "CANCEL",
        _ => "REQUEST",
    };
    let calendario = ContentType::parse(&format!("text/calendar; charset=utf-8; method={method}"))
        .expect("o Content-Type do calendário é construído de uma lista fechada");
    builder.multipart(
        MultiPart::alternative()
            .singlepart(
                SinglePart::builder()
                    .header(ContentType::TEXT_PLAIN)
                    .body(body.to_owned()),
            )
            .singlepart(
                SinglePart::builder()
                    .header(calendario)
                    .body(ics.to_owned()),
            ),
    )
}

/// Uma mensagem reivindicada para enviar.
#[derive(sqlx::FromRow)]
struct PorEnviar {
    id: Uuid,
    /// Decide por que relay a mensagem sai: o da organização, se tiver um
    /// ligado; senão o do operador.
    org_id: Option<Uuid>,
    to_address: String,
    subject: String,
    body_text: String,
    /// O convite de calendário, quando a mensagem leva um.
    calendar_ics: Option<String>,
    /// O `METHOD` de dentro do `.ics`, para o pôr no `Content-Type` da parte.
    calendar_method: Option<String>,
    attempt: i32,
}

/// Envia as mensagens por enviar. Devolve quantas saíram.
///
/// O envio vem **depois** do commit da reivindicação, para não segurar locks
/// durante uma conversa SMTP que pode levar 20 s. A entrega é «pelo menos uma
/// vez»: se o processo morrer entre o envio e a escrita do resultado, a
/// mensagem pode sair duas vezes — preferível a não sair.
pub async fn send_due(state: &Arc<AppState>) -> Result<usize, sqlx::Error> {
    // NÃO se sai cedo por o operador não ter relay: desde o SMTP por
    // organização, uma org com o seu próprio relay manda correio num servidor
    // em que o operador não configurou nenhum. Quem não tem por onde sair
    // descobre-o por mensagem, abaixo.

    let mut tx = state.db.begin().await?;
    let due: Vec<PorEnviar> =
        delonix_meet_store::jobs::claim_in(&mut tx, &FILA, &RETRY, None).await?;
    tx.commit().await?;
    if due.is_empty() {
        return Ok(0);
    }

    // UM transporte por organização, não por mensagem: construir o de um
    // inquilino custa uma consulta de DNS, e um lote traz muitas mensagens da
    // mesma organização. `None` no valor = esta organização não tem por onde
    // sair, e já ficou registado porquê.
    type Pronto = (String, AsyncSmtpTransport<Tokio1Executor>);
    let mut relays: std::collections::HashMap<Option<Uuid>, Option<Pronto>> =
        std::collections::HashMap::new();

    let mut enviadas = 0usize;
    for m in due {
        // O relay desta organização, ou o do operador. A ordem é esta de
        // propósito: quem configurou o seu próprio SMTP quer que o correio dele
        // saia por lá, não pelo nosso.
        if let std::collections::hash_map::Entry::Vacant(vaga) = relays.entry(m.org_id) {
            let escolhido = match m.org_id {
                Some(org) => match org_relay(state, org).await {
                    Some(r) => Some(r),
                    None => operator_relay(state),
                },
                None => operator_relay(state),
            };
            let pronto = match escolhido {
                Some(r) => match transport(state, &r).await {
                    Ok(tp) => Some((r.from.clone(), tp)),
                    Err(e) => {
                        tracing::warn!(org = ?m.org_id, error = %e,
                            "correio: o relay desta organização não serve");
                        None
                    }
                },
                None => None,
            };
            vaga.insert(pronto);
        }
        let Some(Some((from, tp))) = relays.get(&m.org_id) else {
            // Sem relay utilizável: a configuração está errada, não o destino.
            // Falha PERMANENTE — insistir não a corrige, e gastaria as
            // tentativas que servem para um relay em baixo.
            falhou(
                state,
                m.id,
                m.attempt,
                "sem relay utilizável para esta organização",
                PERMANENTE,
            )
            .await;
            continue;
        };

        let built = Message::builder()
            .from(match from.parse() {
                Ok(f) => f,
                Err(e) => {
                    falhou(
                        state,
                        m.id,
                        m.attempt,
                        &format!("remetente: {e}"),
                        PERMANENTE,
                    )
                    .await;
                    continue;
                }
            })
            .to(match m.to_address.parse() {
                Ok(t) => t,
                Err(e) => {
                    // Endereço inválido não melhora com repetição.
                    falhou(
                        state,
                        m.id,
                        m.attempt,
                        &format!("destinatário: {e}"),
                        PERMANENTE,
                    )
                    .await;
                    continue;
                }
            })
            .subject(&m.subject);
        let built = com_corpo(
            built,
            &m.body_text,
            m.calendar_ics.as_deref(),
            m.calendar_method.as_deref(),
        );
        let built = match built {
            Ok(b) => b,
            Err(e) => {
                falhou(
                    state,
                    m.id,
                    m.attempt,
                    &format!("mensagem: {e}"),
                    PERMANENTE,
                )
                .await;
                continue;
            }
        };
        match tp.send(built).await {
            Ok(_) => {
                let _ = sqlx::query(
                    "UPDATE mail_messages SET status = 'sent', sent_at = now(), error = NULL
                      WHERE id = $1",
                )
                .bind(m.id)
                .execute(&state.db)
                .await;
                enviadas += 1;
            }
            Err(e) => {
                // O relay pode estar em baixo: esta é a falha que a repetição
                // existe para cobrir.
                falhou(state, m.id, m.attempt, &e.to_string(), TRANSITORIA).await;
            }
        }
    }
    Ok(enviadas)
}

/// Marca uma falha.
///
/// `failure` decide se há outra tentativa, pelo vocabulário da casa
/// (`jobs::Failure`): um endereço inválido ou uma configuração errada são
/// `Permanent` — dão o mesmo à terceira vez e gastariam as tentativas que
/// servem para o caso que importa, que é o relay em baixo.
///
/// O espalhamento (±20 %) aplica-se no SQL, como nos webhooks: é o que impede
/// um relay que volta de receber, no mesmo segundo, tudo o que falhou junto.
///
/// `AND status = 'sending'` na condição: um resultado que chega tarde não
/// reescreve uma linha que o varredor já deu por abandonada.
async fn falhou(
    state: &Arc<AppState>,
    id: Uuid,
    attempt: i32,
    erro: &str,
    failure: delonix_meet_core::jobs::Failure,
) {
    let atraso: Option<f64> = RETRY
        .should_retry(failure, attempt)
        .then(|| RETRY.delay_after(attempt))
        .flatten()
        .map(|d| d.as_secs() as f64);
    let _ = sqlx::query(
        "UPDATE mail_messages
            SET status = 'failed', error = $2, attempt = attempt + 1, claimed_at = NULL,
                retry_at = CASE WHEN $3::float8 IS NULL THEN NULL
                                ELSE now() + make_interval(
                                       secs => $3::float8 * (0.8 + random() * 0.4)) END
          WHERE id = $1 AND status = 'sending'",
    )
    .bind(id)
    .bind(erro)
    .bind(atraso)
    .execute(&state.db)
    .await;
}

/// Retenção: quanto tempo o livro de entregas guarda uma mensagem.
const RETENTION_DAYS: i32 = 30;
/// Uma reivindicada que fica `pending` tempo demais foi abandonada por um
/// processo que morreu a meio do envio.
const STALE_SECS: i64 = 300;

/// Varre as abandonadas e apaga as velhas. Devolve `(reagendadas, apagadas)`.
///
/// Uma abandonada com tentativas por gastar é reagendada para já: o processo
/// que a enviava morreu sem o relay ter dito nada, e é exactamente o caso que
/// a repetição existe para cobrir. Pode, raramente, duplicar uma mensagem que
/// saiu mas cujo resultado não se chegou a escrever.
pub async fn sweep(db: &sqlx::PgPool) -> Result<(u64, u64), sqlx::Error> {
    let reagendadas = sqlx::query(
        "UPDATE mail_messages
            SET status = 'failed', error = 'abandonada por um processo que terminou',
                attempt = attempt + 1, claimed_at = NULL,
                retry_at = CASE WHEN attempt < $2 THEN now() END
          WHERE status = 'sending' AND claimed_at < now() - make_interval(secs => $1)",
    )
    .bind(STALE_SECS as f64)
    .bind(RETRY.max_attempts)
    .execute(db)
    .await?
    .rows_affected();
    let apagadas = sqlx::query(
        "DELETE FROM mail_messages WHERE created_at < now() - make_interval(days => $1)",
    )
    .bind(RETENTION_DAYS)
    .execute(db)
    .await?
    .rows_affected();
    Ok((reagendadas, apagadas))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder() -> MessageBuilder {
        Message::builder()
            .from("Delonix <nao-responder@exemplo.ao>".parse().unwrap())
            .to("maria@outra.ao".parse().unwrap())
            .subject("Convite: Revisão trimestral")
    }

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nEND:VCALENDAR\r\n";

    fn texto(m: &Message) -> String {
        String::from_utf8_lossy(&m.formatted()).to_string()
    }

    #[test]
    fn sem_calendario_a_mensagem_sai_como_saia() {
        let m = com_corpo(builder(), "olá", None, None).unwrap();
        let s = texto(&m);
        assert!(!s.contains("multipart"), "{s}");
        assert!(!s.contains("text/calendar"), "{s}");
    }

    /// O `method` no `Content-Type` é o que distingue um convite de um anexo:
    /// é de lá que os clientes de correio o lêem.
    #[test]
    fn com_calendario_sai_multipart_com_o_method_no_content_type() {
        let m = com_corpo(builder(), "olá", Some(ICS), Some("REQUEST")).unwrap();
        let s = texto(&m);
        assert!(s.contains("multipart/alternative"), "{s}");
        assert!(s.contains("text/plain"), "{s}");
        assert!(
            s.contains("text/calendar; charset=utf-8; method=REQUEST"),
            "{s}"
        );
        assert!(s.contains("BEGIN:VCALENDAR"), "{s}");
    }

    #[test]
    fn o_cancelamento_leva_o_seu_method() {
        let s = texto(&com_corpo(builder(), "olá", Some(ICS), Some("CANCEL")).unwrap());
        assert!(s.contains("method=CANCEL"), "{s}");
    }

    /// A coluna é texto, por isso um valor inesperado é possível — e não pode
    /// montar um cabeçalho estranho nem fazer o envio entrar em pânico.
    #[test]
    fn um_method_desconhecido_cai_em_request() {
        for valor in [None, Some(""), Some("lixo; charset=x"), Some("REPLY")] {
            let s = texto(&com_corpo(builder(), "olá", Some(ICS), valor).unwrap());
            assert!(s.contains("method=REQUEST"), "{valor:?}: {s}");
            assert!(!s.contains("lixo"), "{valor:?}: {s}");
        }
    }

    /// Num `multipart/alternative` o cliente mostra a ÚLTIMA parte que entende.
    /// Com a ordem trocada, o convite perdia para o texto e voltava a ser um
    /// link que alguém copia à mão.
    #[test]
    fn o_calendario_vem_depois_do_texto() {
        let s = texto(&com_corpo(builder(), "olá", Some(ICS), Some("REQUEST")).unwrap());
        assert!(
            s.find("text/plain").unwrap() < s.find("text/calendar").unwrap(),
            "{s}"
        );
    }
}
