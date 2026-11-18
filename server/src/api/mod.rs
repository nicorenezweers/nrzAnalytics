//! HTTP-API: Router und gemeinsame Hilfen.

mod account;
mod admin;
pub mod event;
mod public;
mod sites;
mod stats;

pub use public::{api_site as public_api_site, api_user as public_api_user, parse_v1_filters};
pub use sites::insert_goal;

use crate::{
    AppState,
    auth::{self, Need, SiteAccess, SiteRow, User},
    error::{AppError, AppResult},
    stats::{SiteCtx, time},
};
use axum::{
    Json, Router,
    http::{HeaderMap, header},
    routing::{delete, get, post, put},
};
use nrzanalytics_shared::api::{Funnel, Goal, GoalKind, Site};
use rusqlite::Connection;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/status", get(account::status))
        .route("/api/timezones", get(sites::timezones))
        .route("/api/auth/login", post(account::login))
        .route("/api/auth/register", post(account::register))
        .route("/api/auth/logout", post(account::logout))
        .route("/api/account", put(account::update))
        .route("/api/account/password", put(account::password))
        .route("/api/account/2fa/setup", post(account::totp_setup))
        .route("/api/account/2fa/enable", post(account::totp_enable))
        .route("/api/account/2fa/disable", post(account::totp_disable))
        .route(
            "/api/account/api-keys",
            get(account::list_keys).post(account::create_key),
        )
        .route("/api/account/api-keys/{id}", delete(account::delete_key))
        .route("/api/sites", get(sites::list).post(sites::create))
        .route(
            "/api/sites/{domain}",
            get(sites::get).put(sites::update).delete(sites::delete),
        )
        .route("/api/sites/{domain}/reset", post(sites::reset))
        .route("/api/sites/{domain}/verify", get(sites::verify))
        .route(
            "/api/sites/{domain}/goals",
            get(sites::goals).post(sites::create_goal),
        )
        .route("/api/sites/{domain}/goals/{id}", delete(sites::delete_goal))
        .route(
            "/api/sites/{domain}/funnels",
            get(sites::funnels).post(sites::create_funnel),
        )
        .route(
            "/api/sites/{domain}/funnels/{id}",
            put(sites::update_funnel).delete(sites::delete_funnel),
        )
        .route(
            "/api/sites/{domain}/props",
            get(sites::props).put(sites::update_props),
        )
        .route(
            "/api/sites/{domain}/members",
            get(sites::members).post(sites::add_member),
        )
        .route(
            "/api/sites/{domain}/members/{user_id}",
            put(sites::update_member).delete(sites::remove_member),
        )
        .route(
            "/api/sites/{domain}/shared-links",
            get(sites::shared_links).post(sites::create_shared_link),
        )
        .route(
            "/api/sites/{domain}/shared-links/{id}",
            delete(sites::delete_shared_link),
        )
        .route(
            "/api/sites/{domain}/shields",
            get(sites::shields).post(sites::create_shield),
        )
        .route("/api/sites/{domain}/shields/{id}", delete(sites::delete_shield))
        .route("/api/stats/{domain}/query", post(stats::query))
        .route("/api/stats/{domain}/realtime", get(stats::realtime))
        .route("/api/stats/{domain}/suggestions", post(stats::suggestions))
        .route("/api/stats/{domain}/funnel", post(stats::funnel))
        .route("/api/stats/{domain}/export", get(stats::export))
        .route("/api/share/{slug}", get(stats::share_info))
        .route("/api/share/{slug}/auth", post(stats::share_auth))
        .route("/api/admin/users", get(admin::users).post(admin::create_user))
        .route(
            "/api/admin/users/{id}",
            put(admin::update_user).delete(admin::delete_user),
        )
        .route(
            "/api/admin/settings",
            get(admin::settings).put(admin::update_settings),
        )
        .route("/api/event", post(event::event).options(event::preflight))
        .route("/js/{file}", get(event::script))
        .route("/api/v1/stats/realtime/visitors", get(public::realtime))
        .route("/api/v1/stats/aggregate", get(public::aggregate))
        .route("/api/v1/stats/timeseries", get(public::timeseries))
        .route("/api/v1/stats/breakdown", get(public::breakdown))
        .route(
            "/api/v1/sites",
            get(public::sites_list).post(public::sites_create),
        )
        .route(
            "/api/v1/sites/goals",
            get(public::goals_list).put(public::goals_put),
        )
        .route("/api/v1/sites/goals/{id}", delete(public::goals_delete))
        .route("/api/v1/sites/shared-links", put(public::shared_link_put))
        .route(
            "/api/v1/sites/{site_id}",
            get(public::site_get)
                .put(public::site_put)
                .delete(public::site_delete),
        )
        .route("/api/v2/query", post(public::query_v2))
        .route("/mcp", post(crate::mcp::handle).get(crate::mcp::info))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "version": nrzanalytics_shared::VERSION }))
}

/// Verwaltungs-Host (`admin.…`) aus einer oeffentlichen Adresse entfernen.
fn without_admin(url: &str) -> String {
    for scheme in ["https://", "http://"] {
        if let Some(rest) = url.strip_prefix(scheme)
            && let Some(rest) = rest.strip_prefix("admin.")
        {
            return format!("{scheme}{rest}");
        }
    }
    url.to_string()
}

/// Oeffentliche Adresse: Umgebung, Einstellung oder aus der Anfrage.
pub fn base_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(u) = &state.config.public_url {
        return without_admin(u);
    }
    if let Ok(i) = state.instance.read()
        && !i.public_url.is_empty()
    {
        return without_admin(&i.public_url);
    }
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let host = state
        .config
        .trust_proxy
        .then(|| get("x-forwarded-host"))
        .flatten()
        .or_else(|| get(header::HOST.as_str()))
        .unwrap_or_else(|| state.config.bind.to_string());
    let proto = state
        .config
        .trust_proxy
        .then(|| get("x-forwarded-proto"))
        .flatten()
        .unwrap_or_else(|| {
            if state.config.secure_cookies {
                "https".into()
            } else {
                "http".into()
            }
        });
    let host = host.strip_prefix("admin.").unwrap_or(&host);
    format!("{proto}://{host}")
}

/// Zugriff auf eine Website pruefen.
pub async fn access(
    state: &AppState,
    headers: &HeaderMap,
    user: Option<User>,
    domain: &str,
    share: Option<String>,
    need: Need,
) -> AppResult<SiteAccess> {
    let headers = headers.clone();
    let domain = domain.to_string();
    let secret = state.secret.clone();
    state
        .db
        .read(move |c| {
            auth::site_access(
                c,
                &secret,
                &headers,
                user.as_ref(),
                &domain,
                share.as_deref(),
                need,
            )
        })
        .await
}

pub fn load_goals(c: &Connection, site_id: i64) -> AppResult<Vec<Goal>> {
    let mut stmt = c.prepare_cached(
        "SELECT id, kind, event_name, page_path, display_name, currency FROM goals WHERE site_id = ?1 ORDER BY id",
    )?;
    let goals = stmt
        .query_map([site_id], |r| {
            Ok(Goal {
                id: r.get(0)?,
                kind: if r.get::<_, String>(1)? == "page" {
                    GoalKind::Page
                } else {
                    GoalKind::Event
                },
                event_name: r.get(2)?,
                page_path: r.get(3)?,
                display_name: r.get(4)?,
                currency: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(goals)
}

pub fn load_funnels(c: &Connection, site_id: i64, goals: &[Goal]) -> AppResult<Vec<Funnel>> {
    let mut stmt = c.prepare_cached("SELECT id, name FROM funnels WHERE site_id = ?1 ORDER BY id")?;
    let funnels: Vec<(i64, String)> = stmt
        .query_map([site_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, name) in funnels {
        let mut st =
            c.prepare_cached("SELECT goal_id FROM funnel_steps WHERE funnel_id = ?1 ORDER BY position")?;
        let ids: Vec<i64> = st.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let steps = ids
            .iter()
            .filter_map(|gid| goals.iter().find(|g| g.id == *gid).cloned())
            .collect();
        out.push(Funnel { id, name, steps });
    }
    Ok(out)
}

/// In den letzten 30 Tagen gesehene Eigenschaften.
pub fn seen_props(c: &Connection, site_id: i64) -> AppResult<Vec<String>> {
    let since = crate::db::now() - 30 * 86_400;
    let mut stmt = c.prepare_cached(
        "SELECT j.key, COUNT(*) AS n FROM events e, json_each(e.props) j \
         WHERE e.site_id = ?1 AND e.ts >= ?2 AND e.props IS NOT NULL GROUP BY j.key ORDER BY n DESC LIMIT 200",
    )?;
    let keys = stmt
        .query_map(rusqlite::params![site_id, since], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    Ok(keys)
}

pub fn site_ctx(c: &Connection, site: &SiteRow) -> AppResult<SiteCtx> {
    Ok(SiteCtx {
        id: site.id,
        domain: site.domain.clone(),
        tz: time::tz(&site.timezone),
        goals: load_goals(c, site.id)?,
    })
}

pub fn snippet(base: &str, domain: &str) -> String {
    format!("<script defer data-domain=\"{domain}\" src=\"{base}/js/script.js\"></script>")
}

/// Website fuer die Oberflaeche.
pub fn site_json(c: &Connection, access: &SiteAccess, base: &str) -> AppResult<Site> {
    let s = &access.site;
    let goals = load_goals(c, s.id)?;
    let funnels = load_funnels(c, s.id, &goals)?;
    let props = match &s.allowed_props {
        Some(list) => list.clone(),
        None => seen_props(c, s.id)?,
    };
    let first: Option<i64> = c.query_row("SELECT MIN(ts) FROM events WHERE site_id = ?1", [s.id], |r| {
        r.get(0)
    })?;
    let tz = time::tz(&s.timezone);
    Ok(Site {
        id: s.id,
        domain: s.domain.clone(),
        timezone: s.timezone.clone(),
        public: s.public,
        role: access.role,
        created_at: s.created_at,
        stats_start: first.map(|t| time::local_date(tz, t).format("%Y-%m-%d").to_string()),
        goals,
        funnels,
        props,
        script_url: format!("{base}/js/script.js"),
        snippet: snippet(base, &s.domain),
        shared: access.shared,
    })
}

/// Domain pruefen und vereinheitlichen (ohne Protokoll, `www.`, Pfad und Port).
pub fn check_domain(domain: &str) -> AppResult<String> {
    let raw = domain.trim().to_ascii_lowercase();
    let raw = raw
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let d = crate::ingest::normalize_domain(raw);
    let valid = (3..=253).contains(&d.len())
        && d.contains('.')
        && !d.starts_with('.')
        && !d.starts_with('-')
        && d.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !valid {
        return Err(AppError::invalid(
            "Bitte eine gültige Domain angeben, z. B. example.com.",
        ));
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::without_admin;

    #[test]
    fn entfernt_admin_host() {
        assert_eq!(
            without_admin("https://admin.analytics.zweers.ch"),
            "https://analytics.zweers.ch"
        );
        assert_eq!(
            without_admin("http://admin.example.com:8581"),
            "http://example.com:8581"
        );
        assert_eq!(
            without_admin("https://analytics.zweers.ch"),
            "https://analytics.zweers.ch"
        );
    }
}
