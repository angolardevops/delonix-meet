//! Login com conta Odoo — a organização e os utilizadores nascem do login.
//!
//! # O problema que isto resolve
//!
//! Antes, o login híbrido do `auth::login` só servia quem **já existia** no
//! Delonix e cuja org **já tinha** a integração ligada: o handler procura o
//! utilizador na tabela `users` e, se não o encontra, devolve 401 sem chegar
//! sequer a falar com o Odoo. Ou seja: alguém tinha de provisionar primeiro.
//!
//! Aqui o primeiro login faz tudo:
//!
//! 1. autentica as credenciais contra o Odoo (`/web/session/authenticate`);
//! 2. lê a **empresa** do utilizador da própria resposta e garante a
//!    organização Delonix correspondente;
//! 3. cria o utilizador, membro da org (admin se for admin no Odoo);
//! 4. dispara em segundo plano o sync de **todos os utilizadores internos
//!    activos** dessa empresa, para o directório aparecer completo.
//!
//! # Porque é que a identidade da org é `(odoo_db, company_id)`
//!
//! Não o nome (muda), não o slug (deriva do nome), não o domínio de email
//! (nem sempre existe). Sem essa chave, dois logins simultâneos da mesma
//! empresa criariam duas orgs. Ver a migração 0032.
//!
//! # Porque é que o sync corre em segundo plano
//!
//! A base real desta integração tem 117 utilizadores internos. Lê-los no
//! caminho do login acrescentaria uma chamada JSON-RPC pesada a cada
//! autenticação. O utilizador que entra é criado de forma síncrona (precisa
//! dele para o token); os restantes chegam segundos depois.

use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::{error::ApiError, AppState};

/// Uma sessão Odoo autenticada — o suficiente para ler dados em nome do
/// utilizador que acabou de entrar.
#[derive(Debug, Clone)]
pub struct OdooSession {
    pub uid: i32,
    pub name: String,
    pub company_id: i32,
    pub company_name: String,
    pub is_admin: bool,
    /// Cookie `session_id` — credencial das chamadas `call_kw` seguintes.
    pub session_id: String,
}

/// Um utilizador interno activo do Odoo.
#[derive(Debug, Clone, Deserialize)]
pub struct OdooUser {
    pub id: i32,
    pub login: String,
    #[serde(default)]
    pub name: String,
    /// O Odoo devolve `false` (não uma string) quando o campo está vazio, daí
    /// o `Option` — desserializar isto como String falha em contas sem email.
    #[serde(default)]
    pub email: Option<serde_json::Value>,
}

impl OdooUser {
    /// Endereço utilizável: o `email` do utilizador, caindo para o `login`
    /// quando este é ele próprio um endereço (o caso comum no Odoo).
    pub fn address(&self) -> Option<String> {
        let from_email = match &self.email {
            Some(serde_json::Value::String(s)) if s.contains('@') => Some(s.clone()),
            _ => None,
        };
        from_email
            .or_else(|| self.login.contains('@').then(|| self.login.clone()))
            .map(|e| e.trim().to_lowercase())
            .filter(|e| crate::meetings_v1::is_usable_email(e))
    }
}

const RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
/// A leitura do directório completo é mais pesada que um login.
const SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Só se relê a lista de utilizadores quando a última é mais velha que isto.
const SYNC_MAX_AGE_SECS: i64 = 3600;

/// Autentica no Odoo e devolve a sessão (uid + empresa + cookie).
///
/// `Ok(None)` = credenciais inválidas. `Err` = Odoo inacessível/ilegível, que
/// o chamador tem de distinguir para não trancar utilizadores fora quando é o
/// Odoo que está em baixo.
pub async fn login(
    client: &Client,
    odoo_url: &str,
    odoo_db: &str,
    login: &str,
    password: &str,
) -> anyhow::Result<Option<OdooSession>> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "method": "call", "id": 1,
        "params": { "db": odoo_db, "login": login, "password": password }
    });
    let resp = tokio::time::timeout(
        RPC_TIMEOUT,
        client
            .post(format!("{}/web/session/authenticate", odoo_url.trim_end_matches('/')))
            .json(&body)
            .send(),
    )
    .await??;

    // O cookie tem de ser lido ANTES do corpo: `resp.json()` consome a
    // resposta e leva os headers com ela.
    let session_id = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|c| {
            c.split(';')
                .next()?
                .trim()
                .strip_prefix("session_id=")
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    let json: serde_json::Value = resp.json().await?;
    // Credenciais erradas chegam como um `error` JSON-RPC (AccessDenied), não
    // como uid=false — tratar os dois.
    let Some(result) = json.get("result").filter(|r| !r.is_null()) else {
        return Ok(None);
    };
    let Some(uid) = result.get("uid").and_then(|u| u.as_i64()).filter(|u| *u > 0) else {
        return Ok(None);
    };
    let company_id = result
        .get("company_id")
        .and_then(|c| c.as_i64())
        .unwrap_or(0) as i32;
    // O nome da empresa vem aninhado em user_companies.allowed_companies.<id>.
    let company_name = result
        .get("user_companies")
        .and_then(|c| c.get("allowed_companies"))
        .and_then(|a| a.get(company_id.to_string()))
        .and_then(|c| c.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();

    Ok(Some(OdooSession {
        uid: uid as i32,
        name: result
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or(login)
            .to_string(),
        company_id,
        company_name,
        is_admin: result
            .get("is_admin")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        session_id,
    }))
}

/// Lê os utilizadores INTERNOS ACTIVOS da empresa (`share = false` exclui as
/// contas de portal/público, que não são pessoal da empresa).
pub async fn active_users(
    client: &Client,
    odoo_url: &str,
    session: &OdooSession,
) -> anyhow::Result<Vec<OdooUser>> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "method": "call", "id": 1,
        "params": {
            "model": "res.users",
            "method": "search_read",
            "args": [
                [["active", "=", true], ["share", "=", false],
                 ["company_id", "=", session.company_id]],
                ["login", "name", "email"]
            ],
            "kwargs": { "context": {} }
        }
    });
    let json: serde_json::Value = tokio::time::timeout(
        SYNC_TIMEOUT,
        client
            .post(format!("{}/web/dataset/call_kw", odoo_url.trim_end_matches('/')))
            .header(reqwest::header::COOKIE, format!("session_id={}", session.session_id))
            .json(&body)
            .send(),
    )
    .await??
    .json()
    .await?;

    if let Some(err) = json.get("error") {
        anyhow::bail!("Odoo call_kw falhou: {}", err);
    }
    let users: Vec<OdooUser> = serde_json::from_value(
        json.get("result").cloned().unwrap_or(serde_json::Value::Null),
    )
    .unwrap_or_default();
    Ok(users)
}

/// Garante a organização Delonix que projeta esta empresa Odoo.
///
/// Idempotente pela chave `(odoo_db, odoo_company_id)`: chamadas concorrentes
/// do mesmo primeiro login convergem para a mesma org em vez de criarem duas.
pub async fn ensure_org(
    state: &AppState,
    odoo_url: &str,
    odoo_db: &str,
    session: &OdooSession,
) -> Result<Uuid, ApiError> {
    if let Some((id,)) = sqlx::query_as::<_, (Uuid,)>(
        "SELECT id FROM organizations WHERE odoo_db = $1 AND odoo_company_id = $2",
    )
    .bind(odoo_db)
    .bind(session.company_id)
    .fetch_optional(&state.db)
    .await?
    {
        return Ok(id);
    }

    let name = if session.company_name.trim().is_empty() {
        format!("Odoo {odoo_db}")
    } else {
        session.company_name.trim().to_string()
    };
    // O mesmo utilizador de serviço que possui as orgs provisionadas por API:
    // a org tem de ter um dono técnico antes de existir gente lá dentro.
    let service_user = crate::apikeys::ensure_provisioning_user_pub(state).await?;
    let base = crate::org::slugify_pub(&name);

    for i in 0..8 {
        let slug = if i == 0 {
            base.clone()
        } else {
            format!("{base}-{i}")
        };
        let res: Result<(Uuid,), sqlx::Error> = sqlx::query_as(
            "INSERT INTO organizations
                (name, slug, created_by, odoo_enabled, odoo_url, odoo_db, odoo_company_id)
             VALUES ($1, $2, $3, TRUE, $4, $5, $6) RETURNING id",
        )
        .bind(&name)
        .bind(&slug)
        .bind(service_user)
        .bind(odoo_url)
        .bind(odoo_db)
        .bind(session.company_id)
        .fetch_one(&state.db)
        .await;
        match res {
            Ok((id,)) => {
                sqlx::query(
                    "INSERT INTO org_members (org_id, user_id, role, title)
                     VALUES ($1, $2, 'admin', 'Provisioning')
                     ON CONFLICT (org_id, user_id) DO NOTHING",
                )
                .bind(id)
                .bind(service_user)
                .execute(&state.db)
                .await?;
                crate::audit::log(&state.db, Some(id), service_user, "org.odoo_created", &name)
                    .await;
                return Ok(id);
            }
            // Corrida: outro login criou a org entretanto (ou o slug colidiu).
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
                if let Some((id,)) = sqlx::query_as::<_, (Uuid,)>(
                    "SELECT id FROM organizations WHERE odoo_db = $1 AND odoo_company_id = $2",
                )
                .bind(odoo_db)
                .bind(session.company_id)
                .fetch_optional(&state.db)
                .await?
                {
                    return Ok(id); // colisão na chave da empresa: o outro ganhou
                }
                continue; // colisão só no slug: tenta o sufixo seguinte
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(ApiError::internal("não foi possível alocar slug para a org Odoo"))
}

/// Cria (ou actualiza) um utilizador local e garante que é membro da org.
/// Devolve o id local.
pub async fn upsert_member(
    state: &AppState,
    org_id: Uuid,
    email: &str,
    name: &str,
    odoo_uid: i32,
    admin: bool,
) -> Result<Uuid, ApiError> {
    let email = email.trim().to_lowercase();
    let user_id = match sqlx::query_as::<_, (Uuid, Option<Uuid>)>(
        "SELECT id, odoo_org_id FROM users WHERE email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?
    {
        // Já é uma conta gerida por ESTA org: só refresca o uid.
        Some((id, Some(owner))) if owner == org_id => {
            sqlx::query("UPDATE users SET odoo_uid = $1, odoo_managed = TRUE WHERE id = $2")
                .bind(odoo_uid)
                .bind(id)
                .execute(&state.db)
                .await?;
            id
        }
        // A conta é gerida por OUTRA org. É a mesma regra de isolamento que o
        // `meetings_v1::resolve_org_user` já aplica (`ForeignOrg`): saber o
        // endereço de alguém não pode capturá-lo para o nosso tenant.
        Some((_, Some(_))) => {
            return Err(ApiError::Conflict(format!(
                "a conta {email} é gerida por outra organização"
            )));
        }
        // Conta LOCAL (registo normal, sem Odoo). Recusa deliberada, e é aqui
        // que estava o buraco: antes, esta sincronização escrevia-lhe
        // `odoo_uid`/`odoo_managed = TRUE` sem qualquer verificação. Como
        // `org_odoo_config` escolhia depois a autoridade de autenticação por
        // email, quem controlasse um Odoo qualquer podia listar o endereço de
        // outra pessoa, reclamar-lhe a conta e passar a validar-lhe a password
        // no seu próprio Odoo — tomada de conta completa, self-service.
        //
        // Ligar uma conta local a um Odoo tem de ser um acto do DONO da conta,
        // não um efeito lateral de alguém a escrever o endereço dela algures.
        Some((_, None)) => {
            return Err(ApiError::Conflict(format!(
                "já existe uma conta local com o endereço {email}; \
                 a ligação ao Odoo tem de ser feita pelo dono da conta"
            )));
        }
        None => {
            let username = crate::meetings_v1::unique_username_pub(&state.db, name).await;
            // Sem password local: o hash só é gravado quando o utilizador
            // entra de facto (auth::login), para servir de cache offline.
            match sqlx::query_as::<_, (Uuid,)>(
                "INSERT INTO users (email, username, password_hash, odoo_uid, odoo_managed, odoo_org_id)
                 VALUES ($1, $2, '', $3, TRUE, $4) RETURNING id",
            )
            .bind(&email)
            .bind(&username)
            .bind(odoo_uid)
            .bind(org_id)
            .fetch_one(&state.db)
            .await
            {
                Ok((id,)) => id,
                Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
                    // Corrida no email (ou colisão de username): relê — e volta a
                    // aplicar a MESMA regra de autoridade. Reler só o `id`, como
                    // era antes, reabria por esta porta exactamente a reclamação
                    // que os ramos acima recusam: bastava a conta nascer entre o
                    // SELECT inicial e este INSERT.
                    match sqlx::query_as::<_, (Uuid, Option<Uuid>)>(
                        "SELECT id, odoo_org_id FROM users WHERE email = $1",
                    )
                    .bind(&email)
                    .fetch_optional(&state.db)
                    .await?
                    {
                        Some((id, Some(owner))) if owner == org_id => id,
                        Some(_) => {
                            return Err(ApiError::Conflict(format!(
                                "a conta {email} não é gerida por esta organização"
                            )));
                        }
                        None => return Err(ApiError::Conflict("email já em uso".into())),
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
    };

    // Nunca DESPROMOVE: um admin nomeado no Delonix não perde o papel só
    // porque não é administrador no Odoo.
    let role = if admin { "admin" } else { "member" };
    sqlx::query(
        "INSERT INTO org_members (org_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (org_id, user_id) DO UPDATE
         SET role = CASE WHEN org_members.role = 'admin' THEN 'admin'
                         ELSE EXCLUDED.role END",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(role)
    .execute(&state.db)
    .await?;

    Ok(user_id)
}

/// True quando o directório desta org está velho o suficiente para valer uma
/// releitura ao Odoo.
async fn sync_is_stale(state: &AppState, org_id: Uuid) -> bool {
    let last: Option<(Option<chrono::DateTime<chrono::Utc>>,)> =
        sqlx::query_as("SELECT odoo_synced_at FROM organizations WHERE id = $1")
            .bind(org_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    match last.and_then(|(t,)| t) {
        None => true,
        Some(t) => (chrono::Utc::now() - t).num_seconds() > SYNC_MAX_AGE_SECS,
    }
}

/// Sincroniza TODOS os utilizadores internos activos da empresa para a org.
/// Corre em segundo plano: o login não espera por ela.
pub fn spawn_directory_sync(
    state: Arc<AppState>,
    org_id: Uuid,
    odoo_url: String,
    session: OdooSession,
    force: bool,
) {
    tokio::spawn(async move {
        if !force && !sync_is_stale(&state, org_id).await {
            return;
        }
        let users = match active_users(&state.webhook_client, &odoo_url, &session).await {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!(error = %e, %org_id, "sync do directório Odoo falhou");
                return;
            }
        };
        let (mut ok, mut skipped) = (0usize, 0usize);
        for u in &users {
            let Some(email) = u.address() else {
                skipped += 1; // sem endereço utilizável não há como entrar
                continue;
            };
            let display = if u.name.trim().is_empty() {
                email.clone()
            } else {
                u.name.clone()
            };
            match upsert_member(&state, org_id, &email, &display, u.id, false).await {
                Ok(_) => ok += 1,
                Err(e) => {
                    tracing::warn!(error = %e, user = %u.login, "utilizador Odoo não sincronizado");
                    skipped += 1;
                }
            }
        }
        let _ = sqlx::query("UPDATE organizations SET odoo_synced_at = now() WHERE id = $1")
            .bind(org_id)
            .execute(&state.db)
            .await;
        tracing::info!(%org_id, sincronizados = ok, ignorados = skipped, "directório Odoo sincronizado");
    });
}

/// Primeiro login de alguém que ainda não tem conta aqui.
///
/// Chamado por `auth::login` no ramo "utilizador não existe", ANTES do 401.
/// Devolve `None` — e o 401 mantém-se — quando o login por Odoo está
/// desligado, quando as credenciais não servem, ou quando o Odoo está
/// inacessível. Nunca propaga erro: o login não pode rebentar por causa de um
/// sistema externo.
///
/// Em caso de sucesso, quando esta função retorna já existem: a organização,
/// o utilizador, e o vínculo entre os dois. O directório completo vem a
/// seguir, em segundo plano.
pub async fn try_first_login(
    state: &Arc<AppState>,
    email: &str,
    password: &str,
) -> Option<crate::users::UserPublic> {
    let (url, db) = match (
        state.config.platform_odoo_url.as_ref(),
        state.config.platform_odoo_db.as_ref(),
    ) {
        (Some(u), Some(d)) => (u.clone(), d.clone()),
        _ => return None, // fail-closed: sem config, nada muda
    };

    let session = match login(&state.webhook_client, &url, &db, email, password).await {
        Ok(Some(s)) => s,
        Ok(None) => return None, // credenciais inválidas
        Err(e) => {
            // Odoo em baixo: não é o mesmo que credenciais erradas, mas para
            // quem não tem conta local o resultado é o mesmo — não há hash em
            // cache que valide. Regista-se para o operador ver.
            tracing::warn!(error = %e, "login por conta Odoo indisponível");
            return None;
        }
    };

    let org_id = match ensure_org(state, &url, &db, &session).await {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(error = %e, "não foi possível criar a org a partir do Odoo");
            return None;
        }
    };

    // O email de entrada é a identidade: é o que o utilizador escreveu e o
    // que a sessão dele vai carregar. O `login` do Odoo pode ser outra coisa.
    let user_id = match upsert_member(
        state,
        org_id,
        email,
        &session.name,
        session.uid,
        session.is_admin,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(error = %e, "não foi possível criar o utilizador vindo do Odoo");
            return None;
        }
    };

    // Guarda o hash para o modo offline: se o Odoo estiver em baixo no
    // próximo login, `auth::login` valida por aqui.
    if let Ok(h) = crate::auth::hash_password(password) {
        let _ = sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
            .bind(&h)
            .bind(user_id)
            .execute(&state.db)
            .await;
    }

    // Directório completo: em segundo plano, para o login não esperar por uma
    // leitura de N utilizadores. `force` na primeira vez (a org acabou de
    // nascer e está vazia); depois só quando estiver velho.
    spawn_directory_sync(state.clone(), org_id, url, session, true);

    crate::users::fetch_public(&state.db, user_id).await.ok()
}

#[cfg(test)]
mod tests {
    use super::OdooUser;
    use serde_json::json;

    fn user(login: &str, email: serde_json::Value) -> OdooUser {
        OdooUser {
            id: 1,
            login: login.into(),
            name: "X".into(),
            email: Some(email),
        }
    }

    #[test]
    fn address_prefers_email_then_login() {
        assert_eq!(
            user("admin", json!("admin@kaeso.co")).address().as_deref(),
            Some("admin@kaeso.co")
        );
        // O Odoo devolve `false` — não uma string — quando o email é vazio;
        // aí vale o login, mas só se ele próprio for um endereço.
        assert_eq!(
            user("ana@kaeso.co", json!(false)).address().as_deref(),
            Some("ana@kaeso.co")
        );
        assert_eq!(user("admin", json!(false)).address(), None);
    }

    #[test]
    fn address_rejects_unusable_values() {
        // Um `login` do Odoo pode ser qualquer coisa; não pode virar conta.
        assert_eq!(user("admin", json!("Ana <a@k.co>")).address(), None);
        assert_eq!(user("admin", json!("a@localhost")).address(), None);
        assert_eq!(user("admin", json!("")).address(), None);
    }
}
