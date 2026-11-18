//! Oeffentliche API mit API-Schluessel – kompatibel mit Plausible:
//! Stats-API v1 (`/api/v1/stats/*`), v2 (`POST /api/v2/query`) und Sites-API (`/api/v1/sites`).

use super::{base_url, load_goals, site_ctx};
use crate::{
    AppState,
    auth::{self, Need, SiteAccess, User},
    db::now,
    error::{AppError, AppResult},
    stats,
};
use axum::{
    Json,
    extract::{Path, Query as QueryParams, State},
    http::{HeaderMap, StatusCode},
};
use nrzanalytics_shared::{
    api::{ApiScope, GoalKind, NewGoal, NewSharedLink},
    query::{Comparison, DateRange, Filter, FilterOp, Query, QueryResult, dimensions as d},
};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// Konto und Berechtigung zum API-Schluessel (Header `Authorization: Bearer …`).
pub async fn api_user(state: &AppState, headers: &HeaderMap) -> AppResult<(User, ApiScope)> {
    let key = auth::bearer(headers).ok_or_else(|| {
        AppError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "API-Schlüssel fehlt (Header Authorization: Bearer <schlüssel>).",
        )
    })?;
    let ip = crate::security::client_ip(headers, None, state.config.trust_proxy)
        .map(|i| i.to_string())
        .unwrap_or_default();
    if !state
        .limiter
        .check(&format!("api:{ip}"), 600, std::time::Duration::from_secs(60))
    {
        return Err(AppError::rate_limited());
    }
    state
        .db
        .write(move |c| auth::api_key_user(c, &key))
        .await?
        .ok_or_else(|| {
            AppError::new(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Ungültiger API-Schlüssel.",
            )
        })
}

pub async fn api_site(state: &AppState, user: User, site_id: &str, need: Need) -> AppResult<SiteAccess> {
    if site_id.trim().is_empty() {
        return Err(AppError::invalid("Parameter site_id (Domain) fehlt."));
    }
    let site_id = site_id.to_string();
    let secret = state.secret.clone();
    state
        .db
        .read(move |c| {
            let a = auth::site_access(c, &secret, &HeaderMap::new(), Some(&user), &site_id, None, need)?;
            // Oeffentliche Websites ohne Mitgliedschaft nicht ueber die API.
            if a.role.is_none() {
                return Err(AppError::not_found());
            }
            Ok(a)
        })
        .await
}

fn require_sites_scope(scope: ApiScope) -> AppResult<()> {
    if scope == ApiScope::Sites {
        Ok(())
    } else {
        Err(AppError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Dieser API-Schlüssel darf nur lesen (Bereich 'stats'). Für die Sites-API einen Schlüssel mit Bereich 'sites' anlegen.",
        ))
    }
}

async fn run(state: &AppState, a: SiteAccess, q: Query) -> AppResult<QueryResult> {
    state
        .db
        .read(move |c| {
            let site = site_ctx(c, &a.site)?;
            stats::execute(c, &site, &q, now())
        })
        .await
}

// ---------------------------------------------------------------------------------------------
// v2
// ---------------------------------------------------------------------------------------------

pub async fn query_v2(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(q): Json<Query>,
) -> AppResult<Json<QueryResult>> {
    let (user, _) = api_user(&state, &headers).await?;
    let a = api_site(&state, user, &q.site_id, Need::View).await?;
    Ok(Json(run(&state, a, q).await?))
}

// ---------------------------------------------------------------------------------------------
// v1
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct V1Params {
    #[serde(default)]
    site_id: String,
    #[serde(default)]
    period: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    metrics: Option<String>,
    #[serde(default)]
    filters: Option<String>,
    #[serde(default)]
    compare: Option<String>,
    #[serde(default)]
    interval: Option<String>,
    #[serde(default)]
    property: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    page: Option<u32>,
}

/// Filter im Format von v1: `visit:browser==Firefox;visit:country==FR|DE;event:page==/blog/**`.
pub fn parse_v1_filters(s: &str) -> AppResult<Vec<Filter>> {
    let mut out = Vec::new();
    for part in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (dim, op, value) = if let Some((dim, v)) = part.split_once("!=") {
            (dim, FilterOp::IsNot, v)
        } else if let Some((dim, v)) = part.split_once("!~") {
            (dim, FilterOp::ContainsNot, v)
        } else if let Some((dim, v)) = part.split_once("==") {
            (dim, FilterOp::Is, v)
        } else if let Some((dim, v)) = part.split_once('~') {
            (dim, FilterOp::Contains, v)
        } else {
            return Err(AppError::invalid(format!(
                "Ungültiger Filter '{part}' (erwartet z. B. visit:country==DE)"
            )));
        };
        let clauses: Vec<String> = value
            .split('|')
            .map(|v| v.trim().replace("\\|", "|"))
            .filter(|v| !v.is_empty())
            .collect();
        let wildcard = clauses.iter().any(|c| c.contains('*'));
        let op = match (op, wildcard) {
            (FilterOp::Is, true) => FilterOp::MatchesWildcard,
            (FilterOp::IsNot, true) => FilterOp::MatchesWildcardNot,
            (op, _) => op,
        };
        let dim = dim.trim();
        out.push(Filter::Condition {
            op,
            dimension: dim.to_string(),
            clauses,
            case_sensitive: true,
        });
    }
    Ok(out)
}

fn v1_date_range(p: &V1Params) -> AppResult<(DateRange, Option<String>)> {
    let period = p.period.clone().unwrap_or_else(|| "30d".into());
    if period == "custom" {
        let date = p
            .date
            .clone()
            .ok_or_else(|| AppError::invalid("period=custom braucht date=JJJJ-MM-TT,JJJJ-MM-TT"))?;
        let (a, b) = date
            .split_once(',')
            .ok_or_else(|| AppError::invalid("date=JJJJ-MM-TT,JJJJ-MM-TT"))?;
        return Ok((DateRange::custom(a.trim(), b.trim()), None));
    }
    Ok((DateRange::Named(period), p.date.clone()))
}

fn v1_metrics(p: &V1Params) -> Vec<String> {
    p.metrics
        .as_deref()
        .unwrap_or("visitors")
        .split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .collect()
}

fn v1_query(p: &V1Params) -> AppResult<Query> {
    let (date_range, date) = v1_date_range(p)?;
    Ok(Query {
        site_id: p.site_id.clone(),
        metrics: v1_metrics(p),
        date_range,
        date,
        filters: parse_v1_filters(p.filters.as_deref().unwrap_or(""))?,
        ..Query::default()
    })
}

pub async fn realtime(
    State(state): State<AppState>,
    headers: HeaderMap,
    QueryParams(p): QueryParams<V1Params>,
) -> AppResult<Json<u64>> {
    let (user, _) = api_user(&state, &headers).await?;
    let a = api_site(&state, user, &p.site_id, Need::View).await?;
    Ok(Json(
        state
            .db
            .read(move |c| stats::current_visitors(c, a.site.id, now()))
            .await?,
    ))
}

pub async fn aggregate(
    State(state): State<AppState>,
    headers: HeaderMap,
    QueryParams(p): QueryParams<V1Params>,
) -> AppResult<Json<Value>> {
    let (user, _) = api_user(&state, &headers).await?;
    let a = api_site(&state, user, &p.site_id, Need::View).await?;
    let mut q = v1_query(&p)?;
    if p.compare.as_deref() == Some("previous_period") {
        q.include.comparisons = Some(Comparison::previous_period());
    }
    let metrics = q.metrics.clone();
    let r = run(&state, a, q).await?;
    let mut out = Map::new();
    if let Some(row) = r.results.first() {
        for (i, m) in metrics.iter().enumerate() {
            let mut entry = Map::new();
            entry.insert("value".into(), row.metrics.get(i).cloned().unwrap_or(Value::Null));
            if let Some(cmp) = &row.comparison {
                entry.insert(
                    "comparison_value".into(),
                    cmp.metrics.get(i).cloned().unwrap_or(Value::Null),
                );
                entry.insert("change".into(), cmp.change.get(i).cloned().unwrap_or(Value::Null));
            }
            out.insert(m.clone(), Value::Object(entry));
        }
    }
    Ok(Json(json!({ "results": out })))
}

pub async fn timeseries(
    State(state): State<AppState>,
    headers: HeaderMap,
    QueryParams(p): QueryParams<V1Params>,
) -> AppResult<Json<Value>> {
    let (user, _) = api_user(&state, &headers).await?;
    let a = api_site(&state, user, &p.site_id, Need::View).await?;
    let mut q = v1_query(&p)?;
    let dim = match p.interval.as_deref() {
        Some("month") => d::TIME_MONTH,
        Some("week") => d::TIME_WEEK,
        Some("hour") => d::TIME_HOUR,
        Some("minute") => d::TIME_MINUTE,
        Some("date") | Some("day") => d::TIME_DAY,
        _ => d::TIME,
    };
    q.dimensions = vec![dim.to_string()];
    q.include.zero_fill = true;
    q.include.time_labels = true;
    let metrics = q.metrics.clone();
    let r = run(&state, a, q).await?;
    let results: Vec<Value> = r
        .results
        .iter()
        .map(|row| {
            let mut o = Map::new();
            o.insert(
                "date".into(),
                json!(row.dimensions.first().cloned().unwrap_or_default()),
            );
            for (i, m) in metrics.iter().enumerate() {
                o.insert(m.clone(), row.metrics.get(i).cloned().unwrap_or(Value::Null));
            }
            Value::Object(o)
        })
        .collect();
    Ok(Json(json!({ "results": results })))
}

pub async fn breakdown(
    State(state): State<AppState>,
    headers: HeaderMap,
    QueryParams(p): QueryParams<V1Params>,
) -> AppResult<Json<Value>> {
    let (user, _) = api_user(&state, &headers).await?;
    let a = api_site(&state, user, &p.site_id, Need::View).await?;
    let property = p
        .property
        .clone()
        .ok_or_else(|| AppError::invalid("Parameter property fehlt, z. B. visit:source"))?;
    let mut q = v1_query(&p)?;
    q.dimensions = vec![property.clone()];
    let limit = p.limit.unwrap_or(100).clamp(1, 1000);
    let page = p.page.unwrap_or(1).max(1);
    q.pagination = Some(nrzanalytics_shared::query::Pagination {
        limit,
        offset: (page - 1) * limit,
    });
    let key = d::prop_key(&property)
        .map(str::to_string)
        .unwrap_or_else(|| property.split(':').nth(1).unwrap_or(&property).to_string());
    let metrics = q.metrics.clone();
    let r = run(&state, a, q).await?;
    let results: Vec<Value> = r
        .results
        .iter()
        .map(|row| {
            let mut o = Map::new();
            o.insert(
                key.clone(),
                json!(row.dimensions.first().cloned().unwrap_or_default()),
            );
            for (i, m) in metrics.iter().enumerate() {
                o.insert(m.clone(), row.metrics.get(i).cloned().unwrap_or(Value::Null));
            }
            Value::Object(o)
        })
        .collect();
    Ok(Json(json!({ "results": results })))
}

// ---------------------------------------------------------------------------------------------
// Sites-API
// ---------------------------------------------------------------------------------------------

pub async fn sites_list(State(state): State<AppState>, headers: HeaderMap) -> AppResult<Json<Value>> {
    let (user, _) = api_user(&state, &headers).await?;
    let sites = state
        .db
        .read(move |c| {
            let sql = if user.is_admin {
                "SELECT domain, timezone FROM sites ORDER BY domain"
            } else {
                "SELECT s.domain, s.timezone FROM sites s JOIN memberships m ON m.site_id = s.id AND m.user_id = ?1 ORDER BY s.domain"
            };
            let mut stmt = c.prepare(sql)?;
            let rows: Vec<Value> = if user.is_admin {
                stmt.query_map([], |r| Ok(json!({"domain": r.get::<_, String>(0)?, "timezone": r.get::<_, String>(1)?})))?
                    .collect::<Result<_, _>>()?
            } else {
                stmt.query_map([user.id], |r| Ok(json!({"domain": r.get::<_, String>(0)?, "timezone": r.get::<_, String>(1)?})))?
                    .collect::<Result<_, _>>()?
            };
            Ok(rows)
        })
        .await?;
    let n = sites.len();
    Ok(Json(
        json!({ "sites": sites, "meta": { "after": null, "before": null, "limit": n } }),
    ))
}

#[derive(Deserialize)]
pub struct SiteBody {
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
}

pub async fn sites_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<SiteBody>,
) -> AppResult<Json<Value>> {
    let (user, scope) = api_user(&state, &headers).await?;
    require_sites_scope(scope)?;
    let domain = super::check_domain(b.domain.as_deref().unwrap_or(""))?;
    let tz = b.timezone.unwrap_or_else(|| "Etc/UTC".into());
    let tz = if tz.parse::<chrono_tz::Tz>().is_ok() {
        tz
    } else {
        return Err(AppError::invalid("Unbekannte Zeitzone."));
    };
    let (d2, t2) = (domain.clone(), tz.clone());
    state
        .db
        .write(move |c| {
            c.execute(
                "INSERT INTO sites (domain, timezone, created_at) VALUES (?1, ?2, ?3)",
                params![d2, t2, now()],
            )
            .map_err(|e| match AppError::from(e) {
                err if err.code == "conflict" => AppError::conflict("Diese Domain gibt es schon."),
                err => err,
            })?;
            let id = c.last_insert_rowid();
            c.execute(
                "INSERT INTO memberships (site_id, user_id, role, created_at) VALUES (?1, ?2, 'owner', ?3)",
                params![id, user.id, now()],
            )?;
            Ok(())
        })
        .await?;
    state.ingest.reload_sites().await?;
    Ok(Json(json!({ "domain": domain, "timezone": tz })))
}

pub async fn site_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(site_id): Path<String>,
) -> AppResult<Json<Value>> {
    let (user, _) = api_user(&state, &headers).await?;
    let a = api_site(&state, user, &site_id, Need::View).await?;
    let props = state.db.read(move |c| super::seen_props(c, a.site.id)).await?;
    Ok(Json(
        json!({ "domain": a.site.domain, "timezone": a.site.timezone, "custom_properties": props }),
    ))
}

pub async fn site_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(site_id): Path<String>,
    Json(b): Json<SiteBody>,
) -> AppResult<Json<Value>> {
    let (user, scope) = api_user(&state, &headers).await?;
    require_sites_scope(scope)?;
    let a = api_site(&state, user, &site_id, Need::Admin).await?;
    let domain = b
        .domain
        .as_deref()
        .map(super::check_domain)
        .transpose()?
        .unwrap_or(a.site.domain.clone());
    let tz = b.timezone.unwrap_or(a.site.timezone.clone());
    if tz.parse::<chrono_tz::Tz>().is_err() {
        return Err(AppError::invalid("Unbekannte Zeitzone."));
    }
    let (d2, t2) = (domain.clone(), tz.clone());
    state
        .db
        .write(move |c| {
            c.execute(
                "UPDATE sites SET domain = ?1, timezone = ?2 WHERE id = ?3",
                params![d2, t2, a.site.id],
            )?;
            Ok(())
        })
        .await?;
    state.ingest.reload_sites().await?;
    Ok(Json(json!({ "domain": domain, "timezone": tz })))
}

pub async fn site_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(site_id): Path<String>,
) -> AppResult<Json<Value>> {
    let (user, scope) = api_user(&state, &headers).await?;
    require_sites_scope(scope)?;
    let a = api_site(&state, user, &site_id, Need::Owner).await?;
    state.ingest.flush().await;
    state
        .db
        .write(move |c| {
            let tx = c.transaction()?;
            tx.execute("DELETE FROM events WHERE site_id = ?1", [a.site.id])?;
            tx.execute("DELETE FROM sessions WHERE site_id = ?1", [a.site.id])?;
            tx.execute("DELETE FROM sites WHERE id = ?1", [a.site.id])?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    state.ingest.reload_sites().await?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
pub struct SharedLinkBody {
    site_id: String,
    name: String,
}

pub async fn shared_link_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<SharedLinkBody>,
) -> AppResult<Json<Value>> {
    let (user, scope) = api_user(&state, &headers).await?;
    require_sites_scope(scope)?;
    let a = api_site(&state, user, &b.site_id, Need::Edit).await?;
    let base = base_url(&state, &headers);
    let link = state
        .db
        .write(move |c| {
            let existing = super::sites::load_links(c, a.site.id, &a.site.domain, &base)?
                .into_iter()
                .find(|l| l.name == b.name.trim());
            match existing {
                Some(l) => Ok(l),
                None => super::sites::insert_link(
                    c,
                    a.site.id,
                    &a.site.domain,
                    &base,
                    &NewSharedLink {
                        name: b.name.clone(),
                        password: None,
                    },
                ),
            }
        })
        .await?;
    Ok(Json(json!({ "name": link.name, "url": link.url })))
}

#[derive(Deserialize)]
pub struct GoalsParams {
    site_id: String,
}

fn goal_json(g: &nrzanalytics_shared::api::Goal) -> Value {
    json!({
        "id": g.id,
        "goal_type": if g.kind == GoalKind::Page { "page" } else { "event" },
        "event_name": g.event_name,
        "page_path": g.page_path,
        "display_name": g.display_name,
    })
}

pub async fn goals_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    QueryParams(p): QueryParams<GoalsParams>,
) -> AppResult<Json<Value>> {
    let (user, _) = api_user(&state, &headers).await?;
    let a = api_site(&state, user, &p.site_id, Need::View).await?;
    let goals = state.db.read(move |c| load_goals(c, a.site.id)).await?;
    Ok(Json(
        json!({ "goals": goals.iter().map(goal_json).collect::<Vec<_>>(), "meta": { "after": null, "before": null, "limit": goals.len() } }),
    ))
}

#[derive(Deserialize)]
pub struct GoalBody {
    site_id: String,
    goal_type: String,
    #[serde(default)]
    event_name: Option<String>,
    #[serde(default)]
    page_path: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    currency: Option<String>,
}

pub async fn goals_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<GoalBody>,
) -> AppResult<Json<Value>> {
    let (user, scope) = api_user(&state, &headers).await?;
    require_sites_scope(scope)?;
    let a = api_site(&state, user, &b.site_id, Need::Edit).await?;
    let kind = match b.goal_type.as_str() {
        "event" => GoalKind::Event,
        "page" => GoalKind::Page,
        _ => return Err(AppError::invalid("goal_type muss 'event' oder 'page' sein.")),
    };
    let goal = state
        .db
        .write(move |c| {
            let existing = load_goals(c, a.site.id)?.into_iter().find(|g| {
                g.kind == kind
                    && match kind {
                        GoalKind::Event => g.event_name.as_deref() == b.event_name.as_deref().map(str::trim),
                        GoalKind::Page => g.page_path.as_deref() == b.page_path.as_deref().map(str::trim),
                    }
            });
            match existing {
                Some(g) => Ok(g),
                None => super::sites::insert_goal(
                    c,
                    a.site.id,
                    &NewGoal {
                        kind,
                        event_name: b.event_name,
                        page_path: b.page_path,
                        display_name: b.display_name,
                        currency: b.currency,
                    },
                ),
            }
        })
        .await?;
    Ok(Json(goal_json(&goal)))
}

pub async fn goals_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    QueryParams(q): QueryParams<std::collections::HashMap<String, String>>,
    body: Option<Json<GoalsParams>>,
) -> AppResult<Json<Value>> {
    let (user, scope) = api_user(&state, &headers).await?;
    require_sites_scope(scope)?;
    let site_id = body
        .map(|b| b.0.site_id)
        .or_else(|| q.get("site_id").cloned())
        .unwrap_or_default();
    let a = api_site(&state, user, &site_id, Need::Edit).await?;
    let n = state
        .db
        .write(move |c| {
            Ok(c.execute(
                "DELETE FROM goals WHERE id = ?1 AND site_id = ?2",
                params![id, a.site.id],
            )?)
        })
        .await?;
    if n == 0 {
        return Err(AppError::not_found());
    }
    Ok(Json(json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_filters() {
        let f = parse_v1_filters(
            "visit:browser==Firefox;visit:country==FR|DE;event:page==/blog/**;visit:source!=Google",
        )
        .unwrap();
        assert_eq!(f.len(), 4);
        assert!(matches!(&f[1], Filter::Condition { clauses, .. } if clauses.len() == 2));
        assert!(matches!(
            &f[2],
            Filter::Condition {
                op: FilterOp::MatchesWildcard,
                ..
            }
        ));
        assert!(matches!(
            &f[3],
            Filter::Condition {
                op: FilterOp::IsNot,
                ..
            }
        ));
        assert!(parse_v1_filters("kaputt").is_err());
    }
}
