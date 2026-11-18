//! Statistik fuer die Oberflaeche (auch oeffentliche Dashboards und geteilte Links).

use super::{access, load_funnels, site_ctx};
use crate::{
    AppState,
    auth::{self, MaybeUser, Need},
    db::now,
    error::{AppError, AppResult},
    stats,
};
use axum::{
    Json,
    extract::{Path, Query as QueryParams, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use nrzanalytics_shared::{
    api::{
        ExportQuery, FunnelQuery, FunnelResult, Realtime, ShareInfo, SharePassword, Suggestion,
        SuggestionQuery,
    },
    query::{Query, QueryResult},
};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use std::time::Duration;

pub async fn query(
    State(state): State<AppState>,
    headers: HeaderMap,
    MaybeUser(user): MaybeUser,
    Path(domain): Path<String>,
    Json(q): Json<Query>,
) -> AppResult<Json<QueryResult>> {
    let a = access(&state, &headers, user, &domain, None, Need::View).await?;
    let result = state
        .db
        .read(move |c| {
            let site = site_ctx(c, &a.site)?;
            stats::execute(c, &site, &q, now())
        })
        .await?;
    Ok(Json(result))
}

pub async fn realtime(
    State(state): State<AppState>,
    headers: HeaderMap,
    MaybeUser(user): MaybeUser,
    Path(domain): Path<String>,
) -> AppResult<Json<Realtime>> {
    let a = access(&state, &headers, user, &domain, None, Need::View).await?;
    let visitors = state
        .db
        .read(move |c| stats::current_visitors(c, a.site.id, now()))
        .await?;
    Ok(Json(Realtime { visitors }))
}

pub async fn suggestions(
    State(state): State<AppState>,
    headers: HeaderMap,
    MaybeUser(user): MaybeUser,
    Path(domain): Path<String>,
    Json(q): Json<SuggestionQuery>,
) -> AppResult<Json<Vec<Suggestion>>> {
    let a = access(&state, &headers, user, &domain, None, Need::View).await?;
    let list = state
        .db
        .read(move |c| {
            let site = site_ctx(c, &a.site)?;
            stats::suggest::suggest(c, &site, &q, now())
        })
        .await?;
    Ok(Json(list))
}

pub async fn funnel(
    State(state): State<AppState>,
    headers: HeaderMap,
    MaybeUser(user): MaybeUser,
    Path(domain): Path<String>,
    Json(q): Json<FunnelQuery>,
) -> AppResult<Json<FunnelResult>> {
    let a = access(&state, &headers, user, &domain, None, Need::View).await?;
    let result = state
        .db
        .read(move |c| {
            let site = site_ctx(c, &a.site)?;
            let funnels = load_funnels(c, a.site.id, &site.goals)?;
            let f = funnels
                .into_iter()
                .find(|f| f.id == q.funnel_id)
                .ok_or_else(AppError::not_found)?;
            stats::funnel::compute(c, &site, &f, &q, now())
        })
        .await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct ExportParams {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    share: Option<String>,
}

pub async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    MaybeUser(user): MaybeUser,
    Path(domain): Path<String>,
    QueryParams(p): QueryParams<ExportParams>,
) -> AppResult<Response> {
    let a = access(&state, &headers, user, &domain, p.share.clone(), Need::View).await?;
    let eq: ExportQuery = match p.q.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(s) => serde_json::from_str(s)
            .map_err(|e| AppError::invalid(format!("Ungültige Export-Abfrage: {e}")))?,
        None => ExportQuery::default(),
    };
    let domain_name = a.site.domain.clone();
    let bytes = state
        .db
        .read(move |c| {
            let site = site_ctx(c, &a.site)?;
            stats::export::export_zip(c, &site, &eq, now())
        })
        .await?;
    let file = format!(
        "nrzanalytics-{}-{}.zip",
        domain_name.replace(['/', ':'], "_"),
        chrono::Utc::now().format("%Y%m%d")
    );
    let mut res = bytes.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{file}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(res)
}

pub async fn share_info(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> AppResult<Json<ShareInfo>> {
    let secret = state.secret.clone();
    let info = state
        .db
        .read(move |c| {
            let row: Option<(i64, String, String, Option<String>)> = c
                .query_row(
                    "SELECT l.id, s.domain, l.name, l.password_hash FROM shared_links l JOIN sites s ON s.id = l.site_id WHERE l.slug = ?1",
                    [slug],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            let (id, domain, name, hash) = row.ok_or_else(AppError::not_found)?;
            // Nach dem Passwort traegt der Browser das Freigabe-Cookie: dann nicht erneut fragen.
            let needs_password = hash.is_some_and(|h| !auth::share_cookie_ok(&secret, &headers, id, &h));
            Ok(ShareInfo { domain, name, needs_password })
        })
        .await?;
    Ok(Json(info))
}

pub async fn share_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(req): Json<SharePassword>,
) -> AppResult<Response> {
    let ip = crate::security::client_ip(&headers, None, state.config.trust_proxy)
        .map(|i| i.to_string())
        .unwrap_or_default();
    if !state
        .limiter
        .check(&format!("share:{slug}:{ip}"), 20, Duration::from_secs(600))
    {
        return Err(AppError::rate_limited());
    }
    let s = slug.clone();
    let row: Option<(i64, Option<String>)> = state
        .db
        .read(move |c| {
            Ok(c.query_row(
                "SELECT id, password_hash FROM shared_links WHERE slug = ?1",
                params![s],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
        })
        .await?;
    let Some((id, hash)) = row else {
        return Err(AppError::not_found());
    };
    let Some(hash) = hash else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    if !auth::verify_password(&hash, &req.password) {
        return Err(AppError::invalid("Das Passwort stimmt nicht."));
    }
    let value = auth::sign(&state.secret, &format!("share:{id}:{hash}"));
    let cookie = format!(
        "{}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000{}",
        auth::share_cookie_name(id),
        if state.config.secure_cookies {
            "; Secure"
        } else {
            ""
        }
    );
    let mut res = StatusCode::NO_CONTENT.into_response();
    if let Ok(v) = HeaderValue::from_str(&cookie) {
        res.headers_mut().insert(header::SET_COOKIE, v);
    }
    Ok(res)
}
