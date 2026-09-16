//! A UI servida pelo próprio binário (`UI_DIR`, ADR-0005 §4).
//!
//! Para a edição pessoal e o enterprise pequeno: um processo, sem nginx à
//! frente. Em SaaS a UI continua servida à parte (nginx/CDN) e este módulo não
//! é montado.
//!
//! Replica o que o `deploy/k8s/nginx.conf` e o `deploy/nginx-delonix.conf`
//! fazem pela SPA, porque sem isso a app partia de formas pouco óbvias:
//! - **COOP/COEP/CORP**: sem isolamento cross-origin não há `SharedArrayBuffer`,
//!   e o WASM multi-thread do ONNX (fundo virtual, Whisper) cai para uma thread;
//! - **CSP** igual à do nginx de produção;
//! - **cache**: `/assets/*` (nome com hash) imutável; `index.html` e `sw.js`
//!   sempre revalidados — sem isto o PWA serve JS antigo depois de actualizar;
//! - **fallback SPA**: um caminho desconhecido devolve o `index.html`, EXCEPTO
//!   debaixo de `/api/`, `/ws` e `/rtc`, onde um 404 tem de continuar a ser
//!   um 404 em JSON (senão um cliente da API recebia HTML com 200).

use std::path::PathBuf;

use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self' blob:; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' blob:; font-src 'self'; connect-src 'self' ws: wss:; frame-ancestors 'none'; base-uri 'self'; object-src 'none'; form-action 'self'";

/// Caminhos que nunca são da SPA.
fn is_backend_path(path: &str) -> bool {
    path.starts_with("/api/")
        || path == "/api"
        || path == "/ws"
        || path == "/rtc"
        || path == "/health"
        || path == "/ready"
        || path == "/metrics"
}

/// Handler de fallback do router quando há `UI_DIR`.
pub async fn serve(dir: PathBuf, req: Request) -> Response {
    let path = req.uri().path().to_owned();
    if is_backend_path(&path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let index = dir.join("index.html");
    let svc = ServeDir::new(&dir)
        .append_index_html_on_directories(true)
        .fallback(ServeFile::new(&index));
    let res = match svc.oneshot(req).await {
        Ok(r) => r.map(Body::new),
        Err(e) => match e {},
    };
    with_headers(&path, res)
}

fn with_headers(path: &str, mut res: Response) -> Response {
    let ok = res.status().is_success();
    let h = res.headers_mut();
    h.insert("Content-Security-Policy", HeaderValue::from_static(CSP));
    h.insert(
        "Cross-Origin-Opener-Policy",
        HeaderValue::from_static("same-origin"),
    );
    h.insert(
        "Cross-Origin-Embedder-Policy",
        HeaderValue::from_static("require-corp"),
    );
    h.insert(
        "Cross-Origin-Resource-Policy",
        HeaderValue::from_static("same-origin"),
    );
    h.insert(
        "Permissions-Policy",
        HeaderValue::from_static(
            "camera=(self), microphone=(self), display-capture=(self), geolocation=()",
        ),
    );
    let immutable = path.starts_with("/assets/")
        || ["/ort/", "/ort-rvm/", "/models/", "/mediapipe-wasm/"]
            .iter()
            .any(|p| path.starts_with(p));
    let cache = if immutable && ok {
        "public, max-age=31536000, immutable"
    } else {
        // index.html, sw.js e o fallback SPA: revalidar sempre.
        "no-cache"
    };
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    if path.ends_with(".webmanifest") {
        h.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/manifest+json"),
        );
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_paths_never_fall_back_to_the_spa() {
        assert!(is_backend_path("/api/nao-existe"));
        assert!(is_backend_path("/ws"));
        assert!(!is_backend_path("/sala/abc-def"));
        assert!(!is_backend_path("/assets/index-1234.js"));
    }
}
