//! Sicherheit: Client-IP, CSRF-Schutz, Sicherheits-Header, Rate-Limits, oeffentlicher Port.

use crate::{AppState, error::AppError};
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, Request, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use nrzanalytics_shared::api::CSRF_HEADER;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

/// IP des Besuchers. Hinter einem Proxy (`trust_proxy`) aus den ueblichen Headern.
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>, trust_proxy: bool) -> Option<IpAddr> {
    if trust_proxy {
        let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
        for name in ["cf-connecting-ip", "x-real-ip"] {
            if let Some(ip) = get(name).and_then(|v| v.parse::<IpAddr>().ok()) {
                return Some(ip);
            }
        }
        if let Some(xff) = get("x-forwarded-for")
            && let Some(ip) = xff
                .split(',')
                .map(str::trim)
                .find_map(|p| p.parse::<IpAddr>().ok())
        {
            return Some(ip);
        }
        if let Some(fwd) = get("forwarded") {
            for part in fwd.split([';', ',']) {
                if let Some(v) = part.trim().strip_prefix("for=") {
                    let v = v.trim_matches('"').trim_start_matches('[');
                    let v = v.split(']').next().unwrap_or(v);
                    if let Ok(ip) = v.parse::<IpAddr>() {
                        return Some(ip);
                    }
                }
            }
        }
    }
    peer.map(|p| p.ip())
}

/// Pfade ohne CSRF-Pruefung (eigene Anmeldung per API-Schluessel bzw. oeffentliche Erfassung).
fn csrf_exempt(path: &str) -> bool {
    path == "/api/event"
        || path.starts_with("/api/v1/")
        || path.starts_with("/api/v2/")
        || path == "/mcp"
        || !path.starts_with("/api/")
}

/// Host aus einer URL (`https://a.b:8080/x` → `a.b:8080`).
fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    Some(rest.split('/').next().unwrap_or(rest))
}

/// Middleware: CSRF fuer schreibende Anfragen der Oberflaeche, Sicherheits-Header fuer alle.
pub async fn guard(State(state): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    if !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) && !csrf_exempt(&path) {
        let h = req.headers();
        if h.get(CSRF_HEADER).is_none() {
            return AppError::forbidden().into_response();
        }
        if let Some(site) = h.get("sec-fetch-site").and_then(|v| v.to_str().ok())
            && site != "same-origin"
            && site != "none"
        {
            return AppError::forbidden().into_response();
        }
        if let Some(origin) = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
            let host = h
                .get("x-forwarded-host")
                .filter(|_| state.config.trust_proxy)
                .or_else(|| h.get(header::HOST))
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if host_of(origin) != Some(host) {
                return AppError::forbidden().into_response();
            }
        }
    }
    let embeddable = path.starts_with("/share/") || path.starts_with("/api/") || path.starts_with("/assets/");
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    h.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    if !embeddable && !h.contains_key(header::X_FRAME_OPTIONS) {
        h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    }
    if state.config.secure_cookies {
        h.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    res
}

/// Middleware fuer den oeffentlichen Port: nur Skript, Erfassung, geteilte Dashboards und Stats-API.
pub async fn public_only(req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path();
    let allowed = path.starts_with("/js/")
        || path == "/api/event"
        || path == "/api/health"
        || path.starts_with("/api/v1/stats/")
        || path.starts_with("/api/v2/")
        || path.starts_with("/share/")
        || path.starts_with("/api/share/")
        || path.starts_with("/api/stats/")
        || path == "/api/status"
        || path.starts_with("/assets/")
        || path.starts_with("/wasm/")
        || path == "/favicon.svg"
        || path == "/favicon.ico"
        || path == "/robots.txt"
        || (path.starts_with("/api/sites/") && req.method() == Method::GET && path.matches('/').count() == 3);
    if allowed {
        next.run(req).await
    } else {
        AppError::not_found().into_response()
    }
}

/// Einfache Rate-Limits je Schluessel (Zaehler pro Zeitfenster).
#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    /// `true`, wenn die Anfrage erlaubt ist.
    pub fn check(&self, key: &str, max: u32, window: Duration) -> bool {
        let Ok(mut map) = self.buckets.lock() else {
            return true;
        };
        let now = Instant::now();
        let entry = map.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) > window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= max
    }

    pub fn prune(&self) {
        if let Ok(mut map) = self.buckets.lock() {
            let now = Instant::now();
            map.retain(|_, (t, _)| now.duration_since(*t) < Duration::from_secs(3600));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_ip() {
        let mut h = HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.7, 10.0.0.1"),
        );
        assert_eq!(client_ip(&h, None, true), "203.0.113.7".parse().ok());
        assert_eq!(client_ip(&h, None, false), None);
        let mut h = HeaderMap::new();
        h.insert(
            "forwarded",
            HeaderValue::from_static("for=\"[2001:db8::1]:4711\";proto=https"),
        );
        assert_eq!(client_ip(&h, None, true), "2001:db8::1".parse().ok());
    }

    #[test]
    fn limiter() {
        let l = RateLimiter::default();
        assert!(l.check("a", 2, Duration::from_secs(60)));
        assert!(l.check("a", 2, Duration::from_secs(60)));
        assert!(!l.check("a", 2, Duration::from_secs(60)));
        assert!(l.check("b", 2, Duration::from_secs(60)));
    }
}
