//! Liefert die Weboberflaeche (Dioxus/WASM) aus – eingebettet oder aus einem Ordner.
//!
//! Existierende Dateien kommen direkt, alle anderen GET-Pfade bekommen `index.html`
//! (Single-Page-App). `index.html` traegt eine strenge Content-Security-Policy; die Inline-Skripte
//! von Dioxus werden per SHA-256-Hash freigegeben.

use axum::{
    body::Body,
    http::{HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_web.rs"));
}

enum Source {
    Embedded(HashMap<&'static str, &'static [u8]>),
    Dir(PathBuf),
}

pub struct WebUi {
    source: Source,
    index: String,
    csp: String,
}

impl WebUi {
    /// Ordner (falls angegeben und vorhanden) oder eingebettete Dateien.
    pub fn load(dir: Option<&Path>) -> Option<Arc<Self>> {
        if let Some(dir) = dir {
            if let Ok(root) = dir.canonicalize()
                && let Ok(index) = std::fs::read_to_string(root.join("index.html"))
            {
                tracing::info!("Weboberflaeche aus {}", root.display());
                return Some(Arc::new(Self::with_index(Source::Dir(root), index)));
            }
            tracing::warn!("NRZANALYTICS_WEB_DIR={} hat keine index.html", dir.display());
        }
        let files: HashMap<&'static str, &'static [u8]> = embedded::FILES.iter().copied().collect();
        let index = files
            .get("index.html")
            .map(|b| String::from_utf8_lossy(b).to_string())?;
        tracing::info!("Weboberflaeche eingebettet ({} Dateien)", files.len());
        Some(Arc::new(Self::with_index(Source::Embedded(files), index)))
    }

    fn with_index(source: Source, mut index: String) -> Self {
        if !index.contains("rel=\"icon\"") {
            index = index.replacen(
                "</head>",
                "<link rel=\"icon\" type=\"image/svg+xml\" href=\"/favicon.svg\"></head>",
                1,
            );
        }
        if !index.contains("class=\"dark\"") {
            // Dunkles Thema schon vor dem Start der App (kein heller Blitz).
            index = index.replacen("<html", "<html class=\"dark\"", 1);
        }
        let csp = build_csp(&index);
        Self { source, index, csp }
    }

    fn file(&self, url_path: &str) -> Option<Vec<u8>> {
        let rel = url_path.trim_start_matches('/');
        if rel.is_empty() {
            return None;
        }
        match &self.source {
            Source::Embedded(files) => files.get(rel).map(|b| b.to_vec()),
            Source::Dir(root) => {
                let mut out = root.clone();
                for c in Path::new(rel).components() {
                    match c {
                        Component::Normal(part) if !part.to_string_lossy().starts_with('.') => out.push(part),
                        Component::CurDir => {}
                        _ => return None,
                    }
                }
                let real = out.canonicalize().ok()?;
                if !real.starts_with(root) || !real.is_file() {
                    return None;
                }
                std::fs::read(real).ok()
            }
        }
    }

    pub async fn serve(self: Arc<Self>, method: Method, uri: Uri) -> Response {
        let path = uri.path();
        if path.starts_with("/api/") || !matches!(method, Method::GET | Method::HEAD) {
            return crate::error::AppError::not_found().into_response();
        }
        if let Some(bytes) = self.file(path) {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let immutable = path.starts_with("/assets/") || path.starts_with("/wasm/");
            let cache = if immutable {
                "public, max-age=31536000, immutable"
            } else {
                "public, max-age=3600"
            };
            let mut res = Response::new(Body::from(bytes));
            let h = res.headers_mut();
            if let Ok(v) = HeaderValue::from_str(mime.as_ref()) {
                h.insert(header::CONTENT_TYPE, v);
            }
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
            return res;
        }
        if path == "/favicon.svg" || path == "/favicon.ico" {
            let mut res = Response::new(Body::from(FAVICON));
            res.headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/svg+xml"));
            res.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=86400"),
            );
            return res;
        }
        if path == "/robots.txt" {
            return (
                [(header::CONTENT_TYPE, "text/plain")],
                "User-agent: *\nDisallow: /\n",
            )
                .into_response();
        }
        // Fehlende Datei (Build-Ordner oder bekannte Endung): 404 statt index.html. Pfade mit Punkt
        // wie `/beispiel.ch` oder `/share/beispiel.ch` sind Routen der App (Domains) und bekommen
        // index.html.
        if is_asset_path(path) {
            return (StatusCode::NOT_FOUND, "not found").into_response();
        }
        let mut res = Response::new(Body::from(self.index.clone()));
        let h = res.headers_mut();
        h.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        let csp = if path.starts_with("/share/") {
            // Geteilte Dashboards duerfen eingebettet werden.
            self.csp.replace("frame-ancestors 'none'", "frame-ancestors *")
        } else {
            self.csp.clone()
        };
        if let Ok(v) = HeaderValue::from_str(&csp) {
            h.insert(header::CONTENT_SECURITY_POLICY, v);
        }
        res
    }
}

/// Logo als SVG (auch Favicon).
pub const FAVICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="#2e2e2e"/><path d="M14 46 L26 32 L34 38 L50 18" fill="none" stroke="#9ec5ff" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/><circle cx="50" cy="18" r="4.5" fill="#ffffff"/></svg>"##;

/// Endungen statischer Dateien. Domains (Routen der App) enden nie darauf.
const ASSET_EXTENSIONS: &[&str] = &[
    "js",
    "mjs",
    "wasm",
    "css",
    "map",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "webp",
    "avif",
    "svg",
    "ico",
    "woff",
    "woff2",
    "ttf",
    "otf",
    "txt",
    "xml",
    "json",
    "webmanifest",
];

/// Pfad zeigt auf eine (fehlende) statische Datei statt auf eine Route der App.
fn is_asset_path(path: &str) -> bool {
    if path.starts_with("/assets/") || path.starts_with("/wasm/") {
        return true;
    }
    let last = path.rsplit('/').next().unwrap_or_default();
    last.rsplit_once('.')
        .is_some_and(|(_, ext)| ASSET_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Inline-`<script>`-Bloecke (ohne `src`) hashen.
fn inline_script_hashes(html: &str) -> Vec<String> {
    let mut hashes = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<script") {
        let after = &rest[start..];
        let Some(tag_end) = after.find('>') else { break };
        let tag = &after[..tag_end];
        let body_start = tag_end + 1;
        let Some(close) = after[body_start..].find("</script>") else {
            break;
        };
        let body = &after[body_start..body_start + close];
        if !tag.contains(" src=") && !body.trim().is_empty() {
            hashes.push(format!(
                "'sha256-{}'",
                STANDARD.encode(Sha256::digest(body.as_bytes()))
            ));
        }
        rest = &after[body_start + close + "</script>".len()..];
    }
    hashes
}

fn build_csp(index: &str) -> String {
    let hashes = inline_script_hashes(index).join(" ");
    format!(
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval' {hashes}; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; frame-src 'self'; worker-src 'self' blob:; \
         manifest-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_routes_are_not_assets() {
        assert!(!is_asset_path("/demo.zweers.ch"));
        assert!(!is_asset_path("/share/demo.zweers.ch"));
        assert!(!is_asset_path("/demo.zweers.ch/settings/goals"));
        assert!(!is_asset_path("/shop.example.co.uk/snippet"));
        assert!(is_asset_path("/assets/missing-dxh123.wasm"));
        assert!(is_asset_path("/wasm/x"));
        assert!(is_asset_path("/apple-touch-icon.png"));
        assert!(is_asset_path("/manifest.webmanifest"));
    }

    #[test]
    fn hashes_only_inline_scripts() {
        let h = inline_script_hashes(r#"<script src="/a.js"></script><script type="module">init()</script>"#);
        assert_eq!(h.len(), 1);
        assert!(h[0].starts_with("'sha256-"));
    }
}
