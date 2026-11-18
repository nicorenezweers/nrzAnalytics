//! Websites und ihre Einstellungen: Ziele, Trichter, Eigenschaften, Mitglieder, geteilte Links,
//! Schutzregeln.

use super::{access, base_url, check_domain, load_funnels, load_goals, seen_props, site_json};
use crate::{
    AppState,
    auth::{self, CurrentUser, MaybeUser, Need},
    db::now,
    error::{AppError, AppResult},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use chrono::Offset;
use chrono_tz::Tz;
use nrzanalytics_shared::api::{
    Funnel, Goal, GoalKind, Member, NewFunnel, NewGoal, NewMember, NewSharedLink, NewShield, NewSite,
    PropsSettings, Role, SharedLink, Shield, ShieldKind, Site, SiteSummary, TimezoneOption, UpdateMember,
    UpdateProps, UpdateSite, Verification,
};
use rusqlite::{OptionalExtension, params};

pub async fn timezones() -> Json<Vec<TimezoneOption>> {
    let now = chrono::Utc::now();
    let mut list: Vec<(i32, TimezoneOption)> = chrono_tz::TZ_VARIANTS
        .iter()
        .filter(|tz| {
            let n = tz.name();
            n.contains('/') && !n.starts_with("Etc/") && !n.starts_with("SystemV/") && !n.starts_with("US/")
        })
        .map(|tz: &Tz| {
            let offset = now.with_timezone(tz).offset().fix().local_minus_utc();
            let sign = if offset < 0 { '-' } else { '+' };
            let (h, m) = (offset.abs() / 3600, (offset.abs() % 3600) / 60);
            (
                offset,
                TimezoneOption {
                    name: tz.name().to_string(),
                    label: format!("(GMT{sign}{h:02}:{m:02}) {}", tz.name()),
                },
            )
        })
        .collect();
    list.push((
        0,
        TimezoneOption {
            name: "UTC".into(),
            label: "(GMT+00:00) UTC".into(),
        },
    ));
    list.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));
    Json(list.into_iter().map(|(_, t)| t).collect())
}

fn check_timezone(tz: &str) -> AppResult<String> {
    let t = tz.trim();
    t.parse::<Tz>()
        .map(|_| t.to_string())
        .map_err(|_| AppError::invalid(format!("Unbekannte Zeitzone: {t}")))
}

pub async fn list(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> AppResult<Json<Vec<SiteSummary>>> {
    let sites = state
        .db
        .read(move |c| {
            let sql = if user.is_admin {
                "SELECT s.id, s.domain, s.timezone, s.public, COALESCE(m.role, 'owner') FROM sites s \
                 LEFT JOIN memberships m ON m.site_id = s.id AND m.user_id = ?1 ORDER BY s.domain"
            } else {
                "SELECT s.id, s.domain, s.timezone, s.public, m.role FROM sites s \
                 JOIN memberships m ON m.site_id = s.id AND m.user_id = ?1 ORDER BY s.domain"
            };
            let mut stmt = c.prepare(sql)?;
            let rows: Vec<(i64, String, String, bool, String)> = stmt
                .query_map([user.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0, r.get(4)?)))?
                .collect::<Result<_, _>>()?;
            let t = now();
            let hour_start = t - t.rem_euclid(3600) + 3600;
            let start = hour_start - 24 * 3600;
            let mut out = Vec::new();
            for (id, domain, timezone, public, role) in rows {
                let visitors: i64 = c.query_row(
                    "SELECT COUNT(DISTINCT user_id) FROM events WHERE site_id = ?1 AND ts >= ?2 AND name != 'engagement'",
                    params![id, t - 86_400],
                    |r| r.get(0),
                )?;
                let previous: i64 = c.query_row(
                    "SELECT COUNT(DISTINCT user_id) FROM events WHERE site_id = ?1 AND ts >= ?2 AND ts < ?3 AND name != 'engagement'",
                    params![id, t - 2 * 86_400, t - 86_400],
                    |r| r.get(0),
                )?;
                let mut spark = vec![0u64; 24];
                let mut st = c.prepare_cached(
                    "SELECT (ts - ?2) / 3600 AS h, COUNT(DISTINCT user_id) FROM events \
                     WHERE site_id = ?1 AND ts >= ?2 AND name != 'engagement' GROUP BY h",
                )?;
                let hours = st.query_map(params![id, start], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
                for h in hours {
                    let (h, n) = h?;
                    if let Some(slot) = usize::try_from(h).ok().and_then(|h| spark.get_mut(h)) {
                        *slot = n.max(0) as u64;
                    }
                }
                let change = if previous > 0 {
                    Some(((visitors - previous) as f64 / previous as f64 * 100.0).round())
                } else {
                    None
                };
                out.push(SiteSummary {
                    domain,
                    timezone,
                    public,
                    role: Role::parse(&role),
                    visitors_24h: visitors.max(0) as u64,
                    change_24h: change,
                    sparkline: spark,
                });
            }
            Ok(out)
        })
        .await?;
    Ok(Json(sites))
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Json(req): Json<NewSite>,
) -> AppResult<Json<Site>> {
    let domain = check_domain(&req.domain)?;
    let timezone = check_timezone(&req.timezone)?;
    let base = base_url(&state, &headers);
    let site = state
        .db
        .write(move |c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM memberships WHERE user_id = ?1 AND role = 'owner'",
                [user.id],
                |r| r.get(0),
            )?;
            if n >= 500 {
                return Err(AppError::invalid("Zu viele Websites."));
            }
            c.execute(
                "INSERT INTO sites (domain, timezone, created_at) VALUES (?1, ?2, ?3)",
                params![domain, timezone, now()],
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
            let site = auth::site_by_domain(c, &domain)?.ok_or_else(AppError::not_found)?;
            site_json(
                c,
                &auth::SiteAccess {
                    site,
                    role: Some(Role::Owner),
                    shared: false,
                },
                &base,
            )
        })
        .await?;
    state.ingest.reload_sites().await?;
    Ok(Json(site))
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    MaybeUser(user): MaybeUser,
    Path(domain): Path<String>,
) -> AppResult<Json<Site>> {
    let a = access(&state, &headers, user, &domain, None, Need::View).await?;
    let base = base_url(&state, &headers);
    Ok(Json(state.db.read(move |c| site_json(c, &a, &base)).await?))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
    Json(req): Json<UpdateSite>,
) -> AppResult<Json<Site>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Admin).await?;
    let new_domain = req.domain.as_deref().map(check_domain).transpose()?;
    let tz = req.timezone.as_deref().map(check_timezone).transpose()?;
    let base = base_url(&state, &headers);
    let site = state
        .db
        .write(move |c| {
            if let Some(d) = &new_domain {
                c.execute(
                    "UPDATE sites SET domain = ?1 WHERE id = ?2",
                    params![d, a.site.id],
                )
                .map_err(|e| match AppError::from(e) {
                    err if err.code == "conflict" => AppError::conflict("Diese Domain gibt es schon."),
                    err => err,
                })?;
            }
            if let Some(t) = &tz {
                c.execute(
                    "UPDATE sites SET timezone = ?1 WHERE id = ?2",
                    params![t, a.site.id],
                )?;
            }
            if let Some(p) = req.public {
                c.execute(
                    "UPDATE sites SET public = ?1 WHERE id = ?2",
                    params![i64::from(p), a.site.id],
                )?;
            }
            let site = auth::site_by_domain(c, new_domain.as_deref().unwrap_or(&a.site.domain))?
                .ok_or_else(AppError::not_found)?;
            site_json(
                c,
                &auth::SiteAccess {
                    site,
                    role: a.role,
                    shared: false,
                },
                &base,
            )
        })
        .await?;
    state.ingest.reload_sites().await?;
    Ok(Json(site))
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
) -> AppResult<StatusCode> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Owner).await?;
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
    Ok(StatusCode::NO_CONTENT)
}

pub async fn reset(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
) -> AppResult<StatusCode> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Admin).await?;
    state.ingest.flush().await;
    state
        .db
        .write(move |c| {
            let tx = c.transaction()?;
            tx.execute("DELETE FROM events WHERE site_id = ?1", [a.site.id])?;
            tx.execute("DELETE FROM sessions WHERE site_id = ?1", [a.site.id])?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
) -> AppResult<Json<Verification>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::View).await?;
    state.ingest.flush().await;
    let site_id = a.site.id;
    let last: Option<i64> = state
        .db
        .read(move |c| {
            Ok(
                c.query_row("SELECT MAX(ts) FROM events WHERE site_id = ?1", [site_id], |r| {
                    r.get(0)
                })?,
            )
        })
        .await?;
    // Startseite laden und nach dem Snippet suchen.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("Mozilla/5.0 (compatible; nrzAnalytics-Verification/1.0)")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(AppError::internal)?;
    let mut snippet_found = None;
    for url in [
        format!("https://{}", a.site.domain),
        format!("http://{}", a.site.domain),
    ] {
        if let Ok(resp) = client.get(&url).send().await
            && let Ok(body) = resp.text().await
        {
            let body = body.to_lowercase();
            let d = a.site.domain.to_lowercase();
            let found =
                (body.contains("/js/script") || body.contains("/js/plausible") || body.contains("/js/nrz"))
                    && (body.contains(&format!("data-domain=\"{d}"))
                        || body.contains(&format!("data-domain='{d}"))
                        || body.contains(&format!(",{d}\"")));
            snippet_found = Some(found);
            break;
        }
    }
    let received = last.is_some();
    let message = match (received, snippet_found) {
        (true, _) => "Es kommen Daten an – alles bereit.".to_string(),
        (false, Some(true)) => {
            "Das Snippet ist eingebaut, aber es sind noch keine Daten angekommen. Seite einmal im Browser öffnen (ohne Werbeblocker) und erneut prüfen.".to_string()
        }
        (false, Some(false)) => format!(
            "Auf https://{} wurde das Snippet nicht gefunden. Bitte im <head> einbauen und die Seite neu veröffentlichen.",
            a.site.domain
        ),
        (false, None) => "Die Website war nicht erreichbar und es sind noch keine Daten angekommen.".to_string(),
    };
    Ok(Json(Verification {
        received,
        last_event_at: last,
        snippet_found,
        message,
    }))
}

// ---------------------------------------------------------------------------------------------
// Ziele
// ---------------------------------------------------------------------------------------------

pub async fn goals(
    State(state): State<AppState>,
    headers: HeaderMap,
    MaybeUser(user): MaybeUser,
    Path(domain): Path<String>,
) -> AppResult<Json<Vec<Goal>>> {
    let a = access(&state, &headers, user, &domain, None, Need::View).await?;
    Ok(Json(state.db.read(move |c| load_goals(c, a.site.id)).await?))
}

/// Ziel pruefen und anlegen (auch fuer die Sites-API und MCP).
pub fn insert_goal(c: &rusqlite::Connection, site_id: i64, req: &NewGoal) -> AppResult<Goal> {
    let (event_name, page_path, default_name) = match req.kind {
        GoalKind::Event => {
            let n = req.event_name.as_deref().unwrap_or("").trim().to_string();
            if n.is_empty() || n.len() > 120 {
                return Err(AppError::invalid(
                    "Bitte einen Ereignisnamen (bis 120 Zeichen) angeben.",
                ));
            }
            if n == "pageview" || n == "engagement" {
                return Err(AppError::invalid(
                    "'pageview' und 'engagement' sind reserviert – für Seiten ein Seitenziel anlegen.",
                ));
            }
            (Some(n.clone()), None, n)
        }
        GoalKind::Page => {
            let p = req.page_path.as_deref().unwrap_or("").trim().to_string();
            if !p.starts_with('/') || p.len() > 2000 {
                return Err(AppError::invalid(
                    "Der Pfad muss mit / beginnen, z. B. /danke oder /blog/**.",
                ));
            }
            (None, Some(p.clone()), format!("Besuch {p}"))
        }
    };
    let display = req
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or(default_name);
    let currency = req
        .currency
        .as_deref()
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| !c.is_empty())
        .map(|c| {
            if c.len() == 3 && c.chars().all(|x| x.is_ascii_alphabetic()) {
                Ok(c)
            } else {
                Err(AppError::invalid("Währung als ISO-Code, z. B. CHF."))
            }
        })
        .transpose()?;
    let count: i64 = c.query_row("SELECT COUNT(*) FROM goals WHERE site_id = ?1", [site_id], |r| {
        r.get(0)
    })?;
    if count >= 200 {
        return Err(AppError::invalid("Höchstens 200 Ziele je Website."));
    }
    c.execute(
        "INSERT INTO goals (site_id, kind, event_name, page_path, display_name, currency, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![site_id, if req.kind == GoalKind::Page { "page" } else { "event" }, event_name, page_path, display, currency, now()],
    )
    .map_err(|e| match AppError::from(e) {
        err if err.code == "conflict" => AppError::conflict("Ein Ziel mit diesem Namen gibt es schon."),
        err => err,
    })?;
    let id = c.last_insert_rowid();
    load_goals(c, site_id)?
        .into_iter()
        .find(|g| g.id == id)
        .ok_or_else(AppError::not_found)
}

pub async fn create_goal(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
    Json(req): Json<NewGoal>,
) -> AppResult<Json<Goal>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    Ok(Json(
        state.db.write(move |c| insert_goal(c, a.site.id, &req)).await?,
    ))
}

pub async fn delete_goal(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path((domain, id)): Path<(String, i64)>,
) -> AppResult<StatusCode> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
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
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------------------------
// Trichter
// ---------------------------------------------------------------------------------------------

pub async fn funnels(
    State(state): State<AppState>,
    headers: HeaderMap,
    MaybeUser(user): MaybeUser,
    Path(domain): Path<String>,
) -> AppResult<Json<Vec<Funnel>>> {
    let a = access(&state, &headers, user, &domain, None, Need::View).await?;
    Ok(Json(
        state
            .db
            .read(move |c| {
                let goals = load_goals(c, a.site.id)?;
                load_funnels(c, a.site.id, &goals)
            })
            .await?,
    ))
}

fn save_funnel(
    c: &mut rusqlite::Connection,
    site_id: i64,
    id: Option<i64>,
    req: &NewFunnel,
) -> AppResult<Funnel> {
    let name = req.name.trim().chars().take(120).collect::<String>();
    if name.is_empty() {
        return Err(AppError::invalid("Bitte einen Namen angeben."));
    }
    if !(2..=8).contains(&req.goal_ids.len()) {
        return Err(AppError::invalid("Ein Trichter hat 2 bis 8 Schritte."));
    }
    let goals = load_goals(c, site_id)?;
    if req.goal_ids.iter().any(|gid| !goals.iter().any(|g| g.id == *gid)) {
        return Err(AppError::invalid("Unbekanntes Ziel im Trichter."));
    }
    let tx = c.transaction()?;
    let fid = match id {
        Some(fid) => {
            let n = tx.execute(
                "UPDATE funnels SET name = ?1 WHERE id = ?2 AND site_id = ?3",
                params![name, fid, site_id],
            )?;
            if n == 0 {
                return Err(AppError::not_found());
            }
            tx.execute("DELETE FROM funnel_steps WHERE funnel_id = ?1", [fid])?;
            fid
        }
        None => {
            tx.execute(
                "INSERT INTO funnels (site_id, name, created_at) VALUES (?1, ?2, ?3)",
                params![site_id, name, now()],
            )?;
            tx.last_insert_rowid()
        }
    };
    for (pos, gid) in req.goal_ids.iter().enumerate() {
        tx.execute(
            "INSERT INTO funnel_steps (funnel_id, position, goal_id) VALUES (?1, ?2, ?3)",
            params![fid, pos as i64, gid],
        )?;
    }
    tx.commit()?;
    load_funnels(c, site_id, &goals)?
        .into_iter()
        .find(|f| f.id == fid)
        .ok_or_else(AppError::not_found)
}

pub async fn create_funnel(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
    Json(req): Json<NewFunnel>,
) -> AppResult<Json<Funnel>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    Ok(Json(
        state
            .db
            .write(move |c| save_funnel(c, a.site.id, None, &req))
            .await?,
    ))
}

pub async fn update_funnel(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path((domain, id)): Path<(String, i64)>,
    Json(req): Json<NewFunnel>,
) -> AppResult<Json<Funnel>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    Ok(Json(
        state
            .db
            .write(move |c| save_funnel(c, a.site.id, Some(id), &req))
            .await?,
    ))
}

pub async fn delete_funnel(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path((domain, id)): Path<(String, i64)>,
) -> AppResult<StatusCode> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    let n = state
        .db
        .write(move |c| {
            Ok(c.execute(
                "DELETE FROM funnels WHERE id = ?1 AND site_id = ?2",
                params![id, a.site.id],
            )?)
        })
        .await?;
    if n == 0 {
        return Err(AppError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------------------------
// Eigenschaften
// ---------------------------------------------------------------------------------------------

pub async fn props(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
) -> AppResult<Json<PropsSettings>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    let allowed = a.site.allowed_props.clone();
    let seen = state.db.read(move |c| seen_props(c, a.site.id)).await?;
    Ok(Json(PropsSettings { allowed, seen }))
}

pub async fn update_props(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
    Json(req): Json<UpdateProps>,
) -> AppResult<Json<PropsSettings>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    let allowed = req.allowed.map(|list| {
        let mut v: Vec<String> = list
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s.len() <= 300)
            .collect();
        v.sort();
        v.dedup();
        v
    });
    let json = allowed
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "[]".into()));
    let site_id = a.site.id;
    let seen = state
        .db
        .write(move |c| {
            c.execute(
                "UPDATE sites SET allowed_props = ?1 WHERE id = ?2",
                params![json, site_id],
            )?;
            seen_props(c, site_id)
        })
        .await?;
    state.ingest.reload_sites().await?;
    Ok(Json(PropsSettings { allowed, seen }))
}

// ---------------------------------------------------------------------------------------------
// Mitglieder
// ---------------------------------------------------------------------------------------------

fn load_members(c: &rusqlite::Connection, site_id: i64) -> AppResult<Vec<Member>> {
    let mut stmt = c.prepare(
        "SELECT u.id, u.email, u.name, m.role, m.created_at FROM memberships m JOIN users u ON u.id = m.user_id \
         WHERE m.site_id = ?1 ORDER BY m.created_at",
    )?;
    let list = stmt
        .query_map([site_id], |r| {
            Ok(Member {
                user_id: r.get(0)?,
                email: r.get(1)?,
                name: r.get(2)?,
                role: Role::parse(&r.get::<_, String>(3)?).unwrap_or(Role::Viewer),
                created_at: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(list)
}

pub async fn members(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
) -> AppResult<Json<Vec<Member>>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Admin).await?;
    Ok(Json(state.db.read(move |c| load_members(c, a.site.id)).await?))
}

pub async fn add_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
    Json(req): Json<NewMember>,
) -> AppResult<Json<Member>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Admin).await?;
    if req.role == Role::Owner && a.role != Some(Role::Owner) {
        return Err(AppError::forbidden());
    }
    let member = state
        .db
        .write(move |c| {
            let target = auth::user_by_email(c, &req.email)?
                .ok_or_else(|| AppError::invalid("Kein Konto mit dieser E-Mail bzw. diesem Benutzernamen – ein Admin kann es unter Verwaltung anlegen."))?;
            c.execute(
                "INSERT INTO memberships (site_id, user_id, role, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![a.site.id, target.id, req.role.as_str(), now()],
            )
            .map_err(|e| match AppError::from(e) {
                err if err.code == "conflict" => AppError::conflict("Dieses Konto ist schon Mitglied."),
                err => err,
            })?;
            load_members(c, a.site.id)?.into_iter().find(|m| m.user_id == target.id).ok_or_else(AppError::not_found)
        })
        .await?;
    Ok(Json(member))
}

fn owners(c: &rusqlite::Connection, site_id: i64) -> AppResult<i64> {
    Ok(c.query_row(
        "SELECT COUNT(*) FROM memberships WHERE site_id = ?1 AND role = 'owner'",
        [site_id],
        |r| r.get(0),
    )?)
}

pub async fn update_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path((domain, user_id)): Path<(String, i64)>,
    Json(req): Json<UpdateMember>,
) -> AppResult<Json<Member>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Admin).await?;
    let is_owner = a.role == Some(Role::Owner);
    let member = state
        .db
        .write(move |c| {
            let current: Option<String> = c
                .query_row(
                    "SELECT role FROM memberships WHERE site_id = ?1 AND user_id = ?2",
                    params![a.site.id, user_id],
                    |r| r.get(0),
                )
                .optional()?;
            let current = current
                .and_then(|r| Role::parse(&r))
                .ok_or_else(AppError::not_found)?;
            if (current == Role::Owner || req.role == Role::Owner) && !is_owner {
                return Err(AppError::forbidden());
            }
            if current == Role::Owner && req.role != Role::Owner && owners(c, a.site.id)? <= 1 {
                return Err(AppError::invalid(
                    "Die Website braucht mindestens einen Besitzer.",
                ));
            }
            c.execute(
                "UPDATE memberships SET role = ?1 WHERE site_id = ?2 AND user_id = ?3",
                params![req.role.as_str(), a.site.id, user_id],
            )?;
            load_members(c, a.site.id)?
                .into_iter()
                .find(|m| m.user_id == user_id)
                .ok_or_else(AppError::not_found)
        })
        .await?;
    Ok(Json(member))
}

pub async fn remove_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path((domain, user_id)): Path<(String, i64)>,
) -> AppResult<StatusCode> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Admin).await?;
    let is_owner = a.role == Some(Role::Owner);
    state
        .db
        .write(move |c| {
            let current: Option<String> = c
                .query_row(
                    "SELECT role FROM memberships WHERE site_id = ?1 AND user_id = ?2",
                    params![a.site.id, user_id],
                    |r| r.get(0),
                )
                .optional()?;
            let current = current
                .and_then(|r| Role::parse(&r))
                .ok_or_else(AppError::not_found)?;
            if current == Role::Owner {
                if !is_owner {
                    return Err(AppError::forbidden());
                }
                if owners(c, a.site.id)? <= 1 {
                    return Err(AppError::invalid(
                        "Die Website braucht mindestens einen Besitzer.",
                    ));
                }
            }
            c.execute(
                "DELETE FROM memberships WHERE site_id = ?1 AND user_id = ?2",
                params![a.site.id, user_id],
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------------------------
// Geteilte Links
// ---------------------------------------------------------------------------------------------

pub fn load_links(
    c: &rusqlite::Connection,
    site_id: i64,
    domain: &str,
    base: &str,
) -> AppResult<Vec<SharedLink>> {
    let mut stmt =
        c.prepare("SELECT id, name, slug, password_hash IS NOT NULL, created_at FROM shared_links WHERE site_id = ?1 ORDER BY id")?;
    let list = stmt
        .query_map([site_id], |r| {
            let slug: String = r.get(2)?;
            Ok(SharedLink {
                id: r.get(0)?,
                name: r.get(1)?,
                url: format!("{base}/share/{domain}?auth={slug}"),
                slug,
                has_password: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(list)
}

pub async fn shared_links(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
) -> AppResult<Json<Vec<SharedLink>>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    let base = base_url(&state, &headers);
    Ok(Json(
        state
            .db
            .read(move |c| load_links(c, a.site.id, &a.site.domain, &base))
            .await?,
    ))
}

/// Geteilten Link anlegen (auch fuer die Sites-API).
pub fn insert_link(
    c: &rusqlite::Connection,
    site_id: i64,
    domain: &str,
    base: &str,
    req: &NewSharedLink,
) -> AppResult<SharedLink> {
    let name = req.name.trim().chars().take(120).collect::<String>();
    if name.is_empty() {
        return Err(AppError::invalid("Bitte einen Namen angeben."));
    }
    let hash = match req.password.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            if p.chars().count() < 6 {
                return Err(AppError::invalid("Das Passwort braucht mindestens 6 Zeichen."));
            }
            Some(auth::hash_password(p)?)
        }
        None => None,
    };
    let slug = auth::random_token(12);
    c.execute(
        "INSERT INTO shared_links (site_id, name, slug, password_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![site_id, name, slug, hash, now()],
    )?;
    let id = c.last_insert_rowid();
    load_links(c, site_id, domain, base)?
        .into_iter()
        .find(|l| l.id == id)
        .ok_or_else(AppError::not_found)
}

pub async fn create_shared_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
    Json(req): Json<NewSharedLink>,
) -> AppResult<Json<SharedLink>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    let base = base_url(&state, &headers);
    Ok(Json(
        state
            .db
            .write(move |c| insert_link(c, a.site.id, &a.site.domain, &base, &req))
            .await?,
    ))
}

pub async fn delete_shared_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path((domain, id)): Path<(String, i64)>,
) -> AppResult<StatusCode> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    let n = state
        .db
        .write(move |c| {
            Ok(c.execute(
                "DELETE FROM shared_links WHERE id = ?1 AND site_id = ?2",
                params![id, a.site.id],
            )?)
        })
        .await?;
    if n == 0 {
        return Err(AppError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------------------------
// Schutzregeln
// ---------------------------------------------------------------------------------------------

fn load_shields(c: &rusqlite::Connection, site_id: i64) -> AppResult<Vec<Shield>> {
    let mut stmt = c.prepare(
        "SELECT id, kind, value, description, created_at FROM shields WHERE site_id = ?1 ORDER BY kind, id",
    )?;
    let list = stmt
        .query_map([site_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?
        .filter_map(Result::ok)
        .filter_map(|(id, kind, value, description, created_at)| {
            Some(Shield {
                id,
                kind: ShieldKind::parse(&kind)?,
                value,
                description,
                created_at,
            })
        })
        .collect();
    Ok(list)
}

pub async fn shields(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
) -> AppResult<Json<Vec<Shield>>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    Ok(Json(state.db.read(move |c| load_shields(c, a.site.id)).await?))
}

pub async fn create_shield(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path(domain): Path<String>,
    Json(req): Json<NewShield>,
) -> AppResult<Json<Shield>> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    let value = req.value.trim().to_string();
    let value = match req.kind {
        ShieldKind::Ip => {
            let ok = value.parse::<std::net::IpAddr>().is_ok() || value.parse::<ipnet::IpNet>().is_ok();
            if !ok {
                return Err(AppError::invalid(
                    "Bitte eine IP-Adresse oder ein Netz (z. B. 192.168.1.0/24) angeben.",
                ));
            }
            value
        }
        ShieldKind::Country => {
            let code = nrzanalytics_shared::geo::country_code(&value).ok_or_else(|| {
                AppError::invalid("Unbekanntes Land – Code (CH) oder Name (Schweiz) angeben.")
            })?;
            code.to_string()
        }
        ShieldKind::Page => {
            if !value.starts_with('/') {
                return Err(AppError::invalid(
                    "Der Pfad muss mit / beginnen, Platzhalter * und ** sind erlaubt.",
                ));
            }
            value
        }
        ShieldKind::Hostname => {
            let v = value.to_ascii_lowercase();
            if v.is_empty() || v.contains('/') || v.contains(' ') {
                return Err(AppError::invalid(
                    "Bitte einen Hostnamen angeben, z. B. example.com oder *.example.com.",
                ));
            }
            v
        }
    };
    if value.len() > 500 {
        return Err(AppError::invalid("Zu lang."));
    }
    let description = req.description.trim().chars().take(200).collect::<String>();
    let shield = state
        .db
        .write(move |c| {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM shields WHERE site_id = ?1", [a.site.id], |r| r.get(0))?;
            if n >= 500 {
                return Err(AppError::invalid("Höchstens 500 Schutzregeln je Website."));
            }
            c.execute(
                "INSERT INTO shields (site_id, kind, value, description, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![a.site.id, req.kind.as_str(), value, description, now()],
            )
            .map_err(|e| match AppError::from(e) {
                err if err.code == "conflict" => AppError::conflict("Diese Regel gibt es schon."),
                err => err,
            })?;
            let id = c.last_insert_rowid();
            load_shields(c, a.site.id)?.into_iter().find(|s| s.id == id).ok_or_else(AppError::not_found)
        })
        .await?;
    state.ingest.reload_sites().await?;
    Ok(Json(shield))
}

pub async fn delete_shield(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Path((domain, id)): Path<(String, i64)>,
) -> AppResult<StatusCode> {
    let a = access(&state, &headers, Some(user), &domain, None, Need::Edit).await?;
    let n = state
        .db
        .write(move |c| {
            Ok(c.execute(
                "DELETE FROM shields WHERE id = ?1 AND site_id = ?2",
                params![id, a.site.id],
            )?)
        })
        .await?;
    if n == 0 {
        return Err(AppError::not_found());
    }
    state.ingest.reload_sites().await?;
    Ok(StatusCode::NO_CONTENT)
}
