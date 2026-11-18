//! Erfassung (`POST /api/event`) und Tracking-Skript (`/js/*.js`) – kompatibel mit Plausible.

use crate::{
    AppState,
    ingest::{IncomingEvent, RequestInfo},
    security::client_ip,
    tracker,
};
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;

fn cors(res: &mut Response) {
    let h = res.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    h.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("POST, OPTIONS"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type"),
    );
    h.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("86400"));
}

pub async fn preflight() -> Response {
    let mut res = StatusCode::NO_CONTENT.into_response();
    cors(&mut res);
    res
}

pub async fn event(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut res = handle(&state, peer, &headers, &body);
    cors(&mut res);
    res
}

fn handle(state: &AppState, peer: SocketAddr, headers: &HeaderMap, body: &[u8]) -> Response {
    if body.len() > 64 * 1024 {
        return (StatusCode::PAYLOAD_TOO_LARGE, "too large").into_response();
    }
    let ev: IncomingEvent = match serde_json::from_slice(body) {
        Ok(e) => e,
        Err(e) => {
            let msg = serde_json::json!({ "errors": { "request": format!("Ungültige Anfrage: {e}") } });
            return (StatusCode::BAD_REQUEST, axum::Json(msg)).into_response();
        }
    };
    let req = RequestInfo {
        ip: client_ip(headers, Some(peer), state.config.trust_proxy),
        user_agent: headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string(),
        country_header: headers
            .get("cf-ipcountry")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string),
    };
    match state.ingest.process(&ev, &req) {
        Ok(0) => {
            let mut res = (StatusCode::ACCEPTED, "ok").into_response();
            res.headers_mut()
                .insert("x-plausible-dropped", HeaderValue::from_static("1"));
            res
        }
        Ok(_) => (StatusCode::ACCEPTED, "ok").into_response(),
        Err(e) => {
            let msg = serde_json::json!({ "errors": { "request": e.message } });
            (e.status, axum::Json(msg)).into_response()
        }
    }
}

pub async fn script(Path(file): Path<String>) -> Response {
    let Some(name) = file.strip_suffix(".js") else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    match tracker::script_for(name) {
        Some(js) => {
            let mut res = js.into_response();
            let h = res.headers_mut();
            h.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/javascript; charset=utf-8"),
            );
            h.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=86400, must-revalidate"),
            );
            h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
            h.insert(
                "cross-origin-resource-policy",
                HeaderValue::from_static("cross-origin"),
            );
            res
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
