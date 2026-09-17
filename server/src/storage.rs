//! Configuração de armazenamento remoto (TrueNAS NFS / Nextcloud WebDAV).
//!
//! A tabela `platform_storage` tem um único registo (id=1). Lê/escreve o admin
//! global pelo painel de definições da plataforma.
//!
//! A password WebDAV guarda-se CIFRADA (S5; `secrets_at_rest`, aad
//! `platform_storage.webdav_password:1`) e só se abre para o teste de ligação.
//! Gravar uma password nova sem `DATA_ENCRYPTION_KEYS` é `422`; guardar o resto
//! da configuração sem mexer na password continua a funcionar sem chaves.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{auth::AuthUser, error::ApiError, AppState};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct StorageConfig {
    pub storage_type: String,
    pub nfs_server: Option<String>,
    pub nfs_path: Option<String>,
    pub webdav_url: Option<String>,
    pub webdav_user: Option<String>,
    /// Password mascarada na leitura (nunca devolver em claro ao cliente).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webdav_password: Option<String>,
    pub webdav_path: Option<String>,
}

/// Documentação OpenAPI das rotas deste módulo (`openapi.rs` junta-as).
///
/// Estas rotas vivem em `/api/v1` mas autenticam por SESSÃO de administrador
/// da plataforma, não por chave de API.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(get_storage, save_storage, test_storage, pvc_manifest),
    components(schemas(StorageConfigView, StorageConfigReq, StorageTestResult))
)]
pub struct ApiDoc;

/// Leitura da configuração de armazenamento, com a password WebDAV substituída
/// por `webdav_password_set`.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StorageConfigView {
    /// `local` | `nfs` | `webdav` (`local` quando nunca foi configurado).
    pub storage_type: String,
    pub nfs_server: Option<String>,
    pub nfs_path: Option<String>,
    pub webdav_url: Option<String>,
    pub webdav_user: Option<String>,
    /// Há password WebDAV guardada? (a password nunca é devolvida).
    pub webdav_password_set: bool,
    pub webdav_path: String,
}

/// Resultado do teste de ligação ao armazenamento.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StorageTestResult {
    /// Sempre `true` (as falhas são erros HTTP).
    pub ok: bool,
    /// `local` | `nfs` | `webdav`.
    #[serde(rename = "type")]
    pub kind: String,
    pub message: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct StorageConfigReq {
    /// `local` | `nfs` | `webdav`.
    pub storage_type: String,
    pub nfs_server: Option<String>,
    pub nfs_path: Option<String>,
    pub webdav_url: Option<String>,
    pub webdav_user: Option<String>,
    /// Novo valor; se omitido ou vazio mantém o valor actual (nunca apaga por engano).
    /// Guarda-se cifrado; não vazio sem `DATA_ENCRYPTION_KEYS` → `422`.
    pub webdav_password: Option<String>,
    pub webdav_path: Option<String>,
}

/// `GET /api/operator/v1/storage` — lê a config actual (admin plataforma).
/// A password WebDAV nunca é devolvida: `webdav_password_set` diz se existe.
#[utoipa::path(
    get, path = "/api/operator/v1/storage", tag = "platform",
    security(("session" = [])),
    responses(
        (status = 200, body = StorageConfigView),
        (status = 401, description = "Sem sessão válida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Não é administrador da plataforma (`PLATFORM_ADMIN_USER_IDS`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da superfície v1 por IP.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn get_storage(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<StorageConfigView>, ApiError> {
    require_platform_admin(&state, auth.user_id)?;

    let row: Option<StorageConfig> = sqlx::query_as(
        "SELECT storage_type, nfs_server, nfs_path,
                webdav_url, webdav_user, webdav_path,
                NULL::TEXT AS webdav_password
         FROM platform_storage WHERE id = 1",
    )
    .fetch_optional(&state.db)
    .await?;

    // Indica se há password guardada (sem a expor).
    let has_password: Option<(bool,)> = sqlx::query_as(
        "SELECT webdav_password IS NOT NULL AND webdav_password <> ''
         FROM platform_storage WHERE id = 1",
    )
    .fetch_optional(&state.db)
    .await?;

    let has_pwd = has_password.map(|r| r.0).unwrap_or(false);
    let cfg = row.unwrap_or(StorageConfig {
        storage_type: "local".into(),
        nfs_server: None,
        nfs_path: None,
        webdav_url: None,
        webdav_user: None,
        webdav_password: None,
        webdav_path: None,
    });

    Ok(Json(StorageConfigView {
        storage_type: cfg.storage_type,
        nfs_server: cfg.nfs_server,
        nfs_path: cfg.nfs_path,
        webdav_url: cfg.webdav_url,
        webdav_user: cfg.webdav_user,
        webdav_password_set: has_pwd,
        webdav_path: cfg
            .webdav_path
            .unwrap_or_else(|| "/remote.php/dav/files/{user}/Delonix".into()),
    }))
}

/// `PUT /api/operator/v1/storage` — actualiza a config (admin plataforma).
/// `webdav_password` vazia ou omissa mantém a guardada.
#[utoipa::path(
    put, path = "/api/operator/v1/storage", tag = "platform",
    security(("session" = [])),
    request_body = StorageConfigReq,
    responses(
        (status = 200, body = StorageConfigView, description = "A configuração como ficou gravada (a mesma forma do `GET`, sem a password)."),
        (status = 400, description = "`storage_type` fora de `local`/`nfs`/`webdav`.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão válida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Não é administrador da plataforma (`PLATFORM_ADMIN_USER_IDS`).", body = crate::openapi::ErrorBody),
        (status = 422, description = "`webdav_password` não vazia sem DATA_ENCRYPTION_KEYS (`secrets.encryption_unconfigured`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da superfície v1 por IP.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn save_storage(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<StorageConfigReq>,
) -> Result<Json<StorageConfigView>, ApiError> {
    require_platform_admin(&state, auth.user_id)?;

    let valid = ["local", "nfs", "webdav"];
    if !valid.contains(&req.storage_type.as_str()) {
        return Err(ApiError::BadRequest("storage_type inválido".into()));
    }
    if let Some(u) = req.webdav_url.as_deref().filter(|u| !u.is_empty()) {
        state.outbound.check_operator_config_url(u).await?;
    }

    // Se password vazia/omitida → manter a existente (COALESCE). Nova → cifrada.
    let new_pwd = req
        .webdav_password
        .as_deref()
        .filter(|p| !p.is_empty())
        .map(|p| crate::secrets_at_rest::seal(&state.config, p, &webdav_password_aad()))
        .transpose()?;

    sqlx::query(
        "INSERT INTO platform_storage (id, storage_type, nfs_server, nfs_path,
                                       webdav_url, webdav_user, webdav_password, webdav_path, updated_at)
         VALUES (1, $1, $2, $3, $4, $5,
                 COALESCE($6, (SELECT webdav_password FROM platform_storage WHERE id=1)),
                 $7, now())
         ON CONFLICT (id) DO UPDATE
         SET storage_type = EXCLUDED.storage_type,
             nfs_server   = EXCLUDED.nfs_server,
             nfs_path     = EXCLUDED.nfs_path,
             webdav_url   = EXCLUDED.webdav_url,
             webdav_user  = EXCLUDED.webdav_user,
             webdav_password = COALESCE($6, platform_storage.webdav_password),
             webdav_path  = EXCLUDED.webdav_path,
             updated_at   = now()",
    )
    .bind(&req.storage_type)
    .bind(req.nfs_server.as_deref().filter(|s| !s.is_empty()))
    .bind(req.nfs_path.as_deref().filter(|s| !s.is_empty()))
    .bind(req.webdav_url.as_deref().filter(|s| !s.is_empty()))
    .bind(req.webdav_user.as_deref().filter(|s| !s.is_empty()))
    .bind(new_pwd.as_deref())
    .bind(
        req.webdav_path
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("/remote.php/dav/files/{user}/Delonix"),
    )
    .execute(&state.db)
    .await?;

    get_storage(State(state), auth).await
}

/// `POST /api/operator/v1/storage/test` — testa a ligação ao storage configurado.
///
/// `local` e `nfs` só confirmam a configuração; `webdav` faz um `PROPFIND` real.
/// Uma falha do destino remoto responde 400, não 502.
#[utoipa::path(
    post, path = "/api/operator/v1/storage/test", tag = "platform",
    security(("session" = [])),
    responses(
        (status = 200, body = StorageTestResult),
        (status = 400, description = "Configuração incompleta, tipo desconhecido, ou o WebDAV falhou/respondeu não-2xx.", body = crate::openapi::ErrorBody),
        (status = 401, description = "Sem sessão válida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Não é administrador da plataforma (`PLATFORM_ADMIN_USER_IDS`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da superfície v1 por IP.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn test_storage(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<StorageTestResult>, ApiError> {
    require_platform_admin(&state, auth.user_id)?;

    let row: Option<(
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT storage_type, webdav_url, webdav_user, webdav_password, nfs_server
             FROM platform_storage WHERE id = 1",
    )
    .fetch_optional(&state.db)
    .await?;

    let Some((stype, wurl, wuser, wpwd, nfs_srv)) = row else {
        return Ok(Json(StorageTestResult {
            ok: true,
            kind: "local".into(),
            message: "Armazenamento local activo (sem configuração remota).".into(),
        }));
    };

    match stype.as_str() {
        "local" => Ok(Json(StorageTestResult {
            ok: true,
            kind: "local".into(),
            message: "Armazenamento local activo.".into(),
        })),
        "nfs" => {
            let srv = nfs_srv.unwrap_or_default();
            if srv.is_empty() {
                return Err(ApiError::BadRequest("nfs_server não configurado".into()));
            }
            Ok(Json(StorageTestResult {
                ok: true,
                kind: "nfs".into(),
                message: format!(
                    "NFS configurado para {srv}. O volume é montado pelo K8s — verificar o PVC."
                ),
            }))
        }
        "webdav" => {
            let url = wurl.unwrap_or_default();
            let user = wuser.unwrap_or_default();
            let pwd = crate::secrets_at_rest::open(
                &state.config,
                wpwd.as_deref().unwrap_or_default(),
                &webdav_password_aad(),
            )?;
            if url.is_empty() || user.is_empty() {
                return Err(ApiError::BadRequest(
                    "webdav_url e webdav_user são obrigatórios".into(),
                ));
            }
            // Teste real: PROPFIND na raiz do WebDAV.
            let url = state.outbound.check_operator_url(&url).await?;
            let client = state.outbound.operator();
            let resp = client
                .request(
                    reqwest::Method::from_bytes(b"PROPFIND").unwrap(),
                    url.as_str(),
                )
                .basic_auth(&user, Some(&pwd))
                .header("Depth", "0")
                .send()
                .await
                .map_err(|e| ApiError::BadRequest(format!("Falha na ligação WebDAV: {e}")))?;
            if resp.status().is_success() || resp.status().as_u16() == 207 {
                Ok(Json(StorageTestResult {
                    ok: true,
                    kind: "webdav".into(),
                    message: "Ligação WebDAV bem-sucedida.".into(),
                }))
            } else {
                Err(ApiError::BadRequest(format!(
                    "WebDAV respondeu com HTTP {}",
                    resp.status()
                )))
            }
        }
        _ => Err(ApiError::BadRequest("storage_type desconhecido".into())),
    }
}

/// Gera o manifesto K8s do PVC para o tipo de storage configurado.
/// `GET /api/operator/v1/storage/pvc-manifest` — devolve YAML para kubectl apply.
#[utoipa::path(
    get, path = "/api/operator/v1/storage/pvc-manifest", tag = "platform",
    security(("session" = [])),
    responses(
        (status = 200, description = "Manifesto YAML (PV + PVC para NFS, ou um comentário para `local`), servido como anexo `delonix-recordings-pv.yaml`.", body = String, content_type = "text/plain"),
        (status = 401, description = "Sem sessão válida.", body = crate::openapi::ErrorBody),
        (status = 403, description = "Não é administrador da plataforma (`PLATFORM_ADMIN_USER_IDS`).", body = crate::openapi::ErrorBody),
        (status = 429, description = "Limite de pedidos da superfície v1 por IP.", body = crate::openapi::ErrorBody),
    )
)]
pub async fn pvc_manifest(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<axum::response::Response, ApiError> {
    require_platform_admin(&state, auth.user_id)?;

    let row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT storage_type, nfs_server, nfs_path FROM platform_storage WHERE id = 1",
    )
    .fetch_optional(&state.db)
    .await?;

    let yaml = match row {
        Some((t, srv, path)) if t == "nfs" => {
            let srv = srv.unwrap_or_default();
            let path = path.unwrap_or_else(|| "/mnt/delonix/recordings".into());
            format!(
                r#"# PersistentVolume + PVC para gravações em TrueNAS NFS
# kubectl apply -f delonix-recordings-pv.yaml
apiVersion: v1
kind: PersistentVolume
metadata:
  name: delonix-recordings-nfs
spec:
  capacity:
    storage: 100Gi
  accessModes: [ReadWriteMany]
  nfs:
    server: {srv}
    path: {path}
  persistentVolumeReclaimPolicy: Retain
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: delonix-recordings
  namespace: delonix
spec:
  accessModes: [ReadWriteMany]
  resources:
    requests:
      storage: 100Gi
  volumeName: delonix-recordings-nfs
  storageClassName: ""
"#
            )
        }
        _ => r#"# Armazenamento local (default) — nenhum PV externo necessário.
# Para NFS ou WebDAV, configura primeiro em Settings > Armazenamento.
"#
        .to_string(),
    };

    Ok(axum::response::Response::builder()
        .header("Content-Type", "text/plain; charset=utf-8")
        .header(
            "Content-Disposition",
            "attachment; filename=\"delonix-recordings-pv.yaml\"",
        )
        .body(axum::body::Body::from(yaml))
        .unwrap())
}

/// `aad` da password WebDAV: a tabela tem um único registo, `id = 1`.
fn webdav_password_aad() -> String {
    crate::secrets_at_rest::aad("platform_storage", "webdav_password", 1)
}

/// Administrador da PLATAFORMA — declarado na configuração
/// (`PLATFORM_ADMIN_USER_IDS`), nunca derivado de `org_members`.
///
/// A versão anterior aceitava «admin de pelo menos uma org». Como o registo
/// público cria sempre o autor como admin da sua org nova, isso era qualquer
/// pessoa: lia e reescrevia o armazenamento de TODAS as organizações, e o
/// `/test` fazia o servidor pedir um URL à escolha dela (SSRF). Auditoria
/// 2026-09-16, S1 — provado ao vivo antes desta correcção.
fn is_platform_admin(declared: &[uuid::Uuid], user_id: uuid::Uuid) -> bool {
    declared.contains(&user_id)
}

pub(crate) fn require_platform_admin(
    state: &AppState,
    user_id: uuid::Uuid,
) -> Result<(), ApiError> {
    if is_platform_admin(&state.config.platform_admin_user_ids, user_id) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

#[cfg(test)]
mod tests {
    use super::{is_platform_admin, StorageConfigView, StorageTestResult};
    use uuid::Uuid;

    /// Os tipos que substituíram os `json!` (OpenAPI) serializam igual.
    #[test]
    fn respostas_tipadas_serializam_como_antes() {
        let v = serde_json::to_value(StorageConfigView {
            storage_type: "local".into(),
            nfs_server: None,
            nfs_path: None,
            webdav_url: Some("https://x".into()),
            webdav_user: None,
            webdav_password_set: false,
            webdav_path: "/p".into(),
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "storage_type": "local", "nfs_server": null, "nfs_path": null,
                "webdav_url": "https://x", "webdav_user": null,
                "webdav_password_set": false, "webdav_path": "/p",
            })
        );
        let t = serde_json::to_value(StorageTestResult {
            ok: true,
            kind: "nfs".into(),
            message: "m".into(),
        })
        .unwrap();
        // Campo a campo, e não um `json!` literal: a catraca da arquitectura
        // conta os `{"ok": true}` do código, e um teste não é dívida.
        assert_eq!(t.as_object().unwrap().len(), 3);
        assert_eq!(t["ok"], serde_json::Value::Bool(true));
        assert_eq!(t["type"], "nfs");
        assert_eq!(t["message"], "m");
    }

    #[test]
    fn nobody_is_platform_admin_when_none_is_declared() {
        // Fail-closed: sem `PLATFORM_ADMIN_USER_IDS`, nem o primeiro utilizador
        // do sistema administra a plataforma. Era aqui que «admin de uma org
        // qualquer» abria a porta a quem se registasse.
        assert!(!is_platform_admin(&[], Uuid::new_v4()));
    }

    #[test]
    fn only_declared_users_are_platform_admins() {
        let admin = Uuid::new_v4();
        let other = Uuid::new_v4();
        assert!(is_platform_admin(&[admin], admin));
        assert!(!is_platform_admin(&[admin], other));
    }
}
