//! Configuração de armazenamento remoto (TrueNAS NFS / Nextcloud WebDAV).
//!
//! A tabela `platform_storage` tem um único registo (id=1). Lê/escreve o admin
//! global pelo painel de definições da plataforma.

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

#[derive(Deserialize)]
pub struct StorageConfigReq {
    pub storage_type: String,
    pub nfs_server: Option<String>,
    pub nfs_path: Option<String>,
    pub webdav_url: Option<String>,
    pub webdav_user: Option<String>,
    /// Novo valor; se omitido ou vazio mantém o valor actual (nunca apaga por engano).
    pub webdav_password: Option<String>,
    pub webdav_path: Option<String>,
}

/// `GET /api/v1/platform/storage` — lê a config actual (admin plataforma).
/// Máscara a password WebDAV: devolve `"••••"` se existir.
pub async fn get_storage(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_platform_admin(&state, auth.user_id).await?;

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

    Ok(Json(serde_json::json!({
        "storage_type": cfg.storage_type,
        "nfs_server": cfg.nfs_server,
        "nfs_path": cfg.nfs_path,
        "webdav_url": cfg.webdav_url,
        "webdav_user": cfg.webdav_user,
        "webdav_password_set": has_pwd,
        "webdav_path": cfg.webdav_path.unwrap_or_else(|| "/remote.php/dav/files/{user}/Delonix".into()),
    })))
}

/// `PUT /api/v1/platform/storage` — actualiza a config (admin plataforma).
pub async fn save_storage(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<StorageConfigReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_platform_admin(&state, auth.user_id).await?;

    let valid = ["local", "nfs", "webdav"];
    if !valid.contains(&req.storage_type.as_str()) {
        return Err(ApiError::BadRequest("storage_type inválido".into()));
    }

    // Se password vazia/omitida → manter a existente (COALESCE).
    let new_pwd = req
        .webdav_password
        .as_deref()
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string());

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

    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /api/v1/platform/storage/test` — testa a ligação ao storage configurado.
pub async fn test_storage(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_platform_admin(&state, auth.user_id).await?;

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
        return Ok(Json(serde_json::json!({ "ok": true, "type": "local", "message": "Armazenamento local activo (sem configuração remota)." })));
    };

    match stype.as_str() {
        "local" => Ok(Json(
            serde_json::json!({ "ok": true, "type": "local", "message": "Armazenamento local activo." }),
        )),
        "nfs" => {
            let srv = nfs_srv.unwrap_or_default();
            if srv.is_empty() {
                return Err(ApiError::BadRequest("nfs_server não configurado".into()));
            }
            Ok(Json(
                serde_json::json!({ "ok": true, "type": "nfs", "message": format!("NFS configurado para {srv}. O volume é montado pelo K8s — verificar o PVC.") }),
            ))
        }
        "webdav" => {
            let url = wurl.unwrap_or_default();
            let user = wuser.unwrap_or_default();
            let pwd = wpwd.unwrap_or_default();
            if url.is_empty() || user.is_empty() {
                return Err(ApiError::BadRequest(
                    "webdav_url e webdav_user são obrigatórios".into(),
                ));
            }
            // Teste real: PROPFIND na raiz do WebDAV.
            let client = &state.webhook_client;
            let resp = client
                .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
                .basic_auth(&user, Some(&pwd))
                .header("Depth", "0")
                .send()
                .await
                .map_err(|e| ApiError::BadRequest(format!("Falha na ligação WebDAV: {e}")))?;
            if resp.status().is_success() || resp.status().as_u16() == 207 {
                Ok(Json(
                    serde_json::json!({ "ok": true, "type": "webdav", "message": "Ligação WebDAV bem-sucedida." }),
                ))
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
/// `GET /api/v1/platform/storage/pvc-manifest` — devolve YAML para kubectl apply.
pub async fn pvc_manifest(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<axum::response::Response, ApiError> {
    require_platform_admin(&state, auth.user_id).await?;

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

/// Verifica se o utilizador é admin de pelo menos uma org (proxy para admin da plataforma
/// neste contexto — sem superadmin separado na v1).
async fn require_platform_admin(
    state: &Arc<AppState>,
    user_id: uuid::Uuid,
) -> Result<(), ApiError> {
    let is_admin: Option<(bool,)> = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM org_members WHERE user_id = $1 AND role = 'admin')",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    if is_admin.map(|r| r.0).unwrap_or(false) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}
