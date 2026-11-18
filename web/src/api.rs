//! HTTP-Client fuer die JSON-API (`/api/...`) per `web-sys`-fetch.
//!
//! - gleiche Origin, Cookie-Sitzung (`credentials: same-origin`)
//! - schreibende Anfragen tragen den CSRF-Header (`x-nrz-analytics: 1`)
//! - im Kontext eines geteilten Links zusaetzlich `x-nrz-share: <slug>`
//! - Fehler kommen als [`ApiErr`]; Nicht-JSON-Antworten ergeben eine generische Meldung.

use nrzanalytics_shared::api::{ApiError, CSRF_HEADER, SHARE_HEADER};
use nrzanalytics_shared::query::{Query, QueryResult};
use serde::{Serialize, de::DeserializeOwned};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestCredentials, RequestInit, Response};

/// Fehler einer API-Anfrage.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiErr {
    /// HTTP-Status (0 = keine Verbindung).
    pub status: u16,
    /// Maschinenlesbarer Code, z. B. `totp_required`.
    pub code: String,
    /// Meldung fuer Menschen.
    pub message: String,
}

impl ApiErr {
    fn network() -> Self {
        Self { status: 0, code: "network".into(), message: "Keine Verbindung zum Server.".into() }
    }

    #[allow(dead_code)]
    pub fn is(&self, code: &str) -> bool {
        self.code == code
    }

    pub fn unauthorized(&self) -> bool {
        self.status == 401
    }
}

impl std::fmt::Display for ApiErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

pub type ApiResult<T> = Result<T, ApiErr>;

fn generic_message(status: u16) -> String {
    match status {
        401 => "Bitte melde dich an.".into(),
        403 => "Dafür fehlen die Rechte.".into(),
        404 => "Nicht gefunden.".into(),
        429 => "Zu viele Anfragen – bitte kurz warten.".into(),
        500..=599 => format!("Serverfehler ({status}). Bitte später erneut versuchen."),
        _ => format!("Unerwartete Antwort vom Server ({status})."),
    }
}

async fn raw(method: &str, path: &str, body: Option<String>) -> ApiResult<(u16, String)> {
    let window = web_sys::window().ok_or_else(ApiErr::network)?;
    let init = RequestInit::new();
    init.set_method(method);
    init.set_credentials(RequestCredentials::SameOrigin);
    if let Some(b) = &body {
        init.set_body(&JsValue::from_str(b));
    }
    let req = Request::new_with_str_and_init(path, &init).map_err(|_| ApiErr::network())?;
    let h = req.headers();
    let _ = h.set("accept", "application/json");
    if method != "GET" {
        let _ = h.set(CSRF_HEADER, "1");
    }
    if body.is_some() {
        let _ = h.set("content-type", "application/json");
    }
    if let Some(slug) = crate::state::share_slug() {
        let _ = h.set(SHARE_HEADER, &slug);
    }
    let resp = JsFuture::from(window.fetch_with_request(&req)).await.map_err(|_| ApiErr::network())?;
    let resp: Response = resp.dyn_into().map_err(|_| ApiErr::network())?;
    let status = resp.status();
    let text = match resp.text() {
        Ok(p) => JsFuture::from(p).await.ok().and_then(|v| v.as_string()).unwrap_or_default(),
        Err(_) => String::new(),
    };
    Ok((status, text))
}

fn parse<T: DeserializeOwned>(status: u16, text: &str) -> ApiResult<T> {
    if (200..300).contains(&status) {
        let src = if text.trim().is_empty() { "null" } else { text };
        return serde_json::from_str(src).map_err(|_| ApiErr {
            status,
            code: "invalid_response".into(),
            message: "Die Antwort des Servers konnte nicht gelesen werden.".into(),
        });
    }
    Err(match serde_json::from_str::<ApiError>(text) {
        Ok(e) => ApiErr {
            status,
            code: e.code.unwrap_or_default(),
            message: if e.error.trim().is_empty() { generic_message(status) } else { e.error },
        },
        Err(_) => ApiErr { status, code: String::new(), message: generic_message(status) },
    })
}

pub async fn get<T: DeserializeOwned>(path: &str) -> ApiResult<T> {
    let (status, text) = raw("GET", path, None).await?;
    parse(status, &text)
}

pub async fn send<B: Serialize, T: DeserializeOwned>(method: &str, path: &str, body: &B) -> ApiResult<T> {
    let json = serde_json::to_string(body).map_err(|_| ApiErr {
        status: 0,
        code: "invalid".into(),
        message: "Anfrage konnte nicht erstellt werden.".into(),
    })?;
    let (status, text) = raw(method, path, Some(json)).await?;
    parse(status, &text)
}

pub async fn post<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> ApiResult<T> {
    send("POST", path, body).await
}

pub async fn put<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> ApiResult<T> {
    send("PUT", path, body).await
}

/// POST ohne Inhalt (z. B. Abmelden, 2FA einrichten).
pub async fn post_empty<T: DeserializeOwned>(path: &str) -> ApiResult<T> {
    let (status, text) = raw("POST", path, None).await?;
    parse(status, &text)
}

pub async fn delete(path: &str) -> ApiResult<()> {
    let (status, text) = raw("DELETE", path, None).await?;
    if (200..300).contains(&status) {
        return Ok(());
    }
    parse::<()>(status, &text)
}

/// Pfadsegment URL-kodieren.
pub fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

/// Statistik-Abfrage (`POST /api/stats/{domain}/query`).
pub async fn query(domain: &str, q: &Query) -> ApiResult<QueryResult> {
    post(&format!("/api/stats/{}/query", enc(domain)), q).await
}
