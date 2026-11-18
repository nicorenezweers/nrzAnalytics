//! MCP-Server (Model Context Protocol, Streamable HTTP, JSON-RPC 2.0) unter `POST /mcp`.
//! Anmeldung mit API-Schluessel: `Authorization: Bearer nrzan_…` (Bereich `stats` = lesen,
//! `sites` = zusaetzlich Websites und Ziele anlegen). Fuer nrzDashboard und Coding-Agenten.

use crate::{
    AppState,
    api::{base_url, load_goals, site_ctx, snippet},
    auth::{Need, User},
    db::now,
    error::{AppError, AppResult},
    stats,
};
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use nrzanalytics_shared::{
    api::{ApiScope, GoalKind, NewGoal},
    query::{Comparison, DateRange, Filter, Query, dimensions as d, metrics as m},
};
use serde_json::{Value, json};

const PROTOCOL: &str = "2025-06-18";

pub async fn info() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        "nrzAnalytics MCP: POST JSON-RPC an /mcp (Authorization: Bearer <API-Schlüssel>)",
    )
        .into_response()
}

pub async fn handle(
    axum::extract::State(state): axum::extract::State<AppState>,
    headers: HeaderMap,
    Json(req): Json<Value>,
) -> Response {
    let (user, scope) = match crate::api::public_api_user(&state, &headers).await {
        Ok(u) => u,
        Err(e) => return e.into_response(),
    };
    let base = base_url(&state, &headers);
    let ctx = Ctx {
        state: &state,
        user,
        scope,
        base,
    };
    if let Value::Array(items) = req {
        let mut out = Vec::new();
        for item in items {
            if let Some(r) = dispatch(&ctx, item).await {
                out.push(r);
            }
        }
        return Json(Value::Array(out)).into_response();
    }
    match dispatch(&ctx, req).await {
        Some(r) => Json(r).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

struct Ctx<'a> {
    state: &'a AppState,
    user: User,
    scope: ApiScope,
    base: String,
}

async fn dispatch(ctx: &Ctx<'_>, req: Value) -> Option<Value> {
    let id = req.get("id").cloned()?;
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let result: Result<Value, (i64, String)> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "nrzAnalytics", "version": nrzanalytics_shared::VERSION },
            "instructions": "Webanalyse (wie Plausible): Websites auflisten, Besucher in Echtzeit, Kennzahlen, Zeitreihen und Aufschlüsselungen (Quellen, Seiten, Länder, Geräte, Ziele), Ziele verwalten und Tracking-Snippets holen. Zeiträume: day, 7d, 28d, 30d, 91d, month, 6mo, 12mo, year, all, realtime oder [\"JJJJ-MM-TT\",\"JJJJ-MM-TT\"]."
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            Ok(match call(ctx, &name, &args).await {
                Ok(v) => json!({
                    "content": [{ "type": "text", "text": if let Value::String(s) = &v { s.clone() } else { serde_json::to_string_pretty(&v).unwrap_or_default() } }],
                    "structuredContent": if v.is_object() { v.clone() } else { json!({ "result": v }) },
                    "isError": false
                }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e.message }], "isError": true }),
            })
        }
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        _ => Err((-32601, format!("Unbekannte Methode: {method}"))),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, msg)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } }),
    })
}

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({ "name": name, "description": description, "inputSchema": { "type": "object", "properties": props, "required": required } })
}

fn tools() -> Vec<Value> {
    let site =
        json!({ "type": "string", "description": "Domain der Website (aus list_sites), z. B. example.com" });
    let range = json!({ "description": "Zeitraum: day, 7d, 28d, 30d, 91d, month, 6mo, 12mo, year, all, realtime oder [\"JJJJ-MM-TT\",\"JJJJ-MM-TT\"]", "default": "28d" });
    let date = json!({ "type": "string", "description": "Bezugsdatum JJJJ-MM-TT für day/month/year (Standard: heute)" });
    let filters = json!({ "description": "Filter wie Stats-API v2, z. B. [[\"is\",\"visit:country\",[\"CH\"]]] oder v1-Text \"visit:source==Google;event:page==/blog/**\"" });
    let metrics = json!({ "type": "array", "items": { "type": "string" }, "description": format!("Kennzahlen: {}", m::ALL.join(", ")) });
    vec![
        tool(
            "list_sites",
            "Alle Websites mit Rolle und Besuchern der letzten 24 Stunden.",
            json!({}),
            &[],
        ),
        tool(
            "get_site",
            "Details einer Website: Zeitzone, Ziele, Trichter, Eigenschaften, Tracking-Snippet.",
            json!({ "site_id": site }),
            &["site_id"],
        ),
        tool(
            "realtime_visitors",
            "Aktuelle Besucher (letzte 5 Minuten).",
            json!({ "site_id": site }),
            &["site_id"],
        ),
        tool(
            "aggregate",
            "Kennzahlen für einen Zeitraum (Standard: Besucher, Besuche, Seitenaufrufe, Absprungrate, Besuchsdauer), optional mit Vergleich zum Vorzeitraum.",
            json!({ "site_id": site, "date_range": range, "date": date, "metrics": metrics, "filters": filters, "compare": { "type": "boolean", "default": true } }),
            &["site_id"],
        ),
        tool(
            "timeseries",
            "Zeitreihe einer oder mehrerer Kennzahlen (Intervall automatisch oder minute/hour/day/week/month).",
            json!({ "site_id": site, "date_range": range, "date": date, "metrics": metrics, "filters": filters, "interval": { "type": "string", "enum": ["minute", "hour", "day", "week", "month"] } }),
            &["site_id"],
        ),
        tool(
            "breakdown",
            "Aufschlüsselung nach einer Dimension, z. B. Top-Quellen (visit:source), Kanäle (visit:channel), Seiten (event:page), Einstiegsseiten (visit:entry_page), Länder (visit:country), Browser (visit:browser), Geräte (visit:device), Ziele (event:goal) oder Eigenschaften (event:props:<name>).",
            json!({ "site_id": site, "dimension": { "type": "string" }, "date_range": range, "date": date, "metrics": metrics, "filters": filters, "limit": { "type": "integer", "default": 10 } }),
            &["site_id", "dimension"],
        ),
        tool(
            "query",
            "Beliebige Abfrage im Format der Stats-API v2 (metrics, date_range, dimensions, filters, order_by, include, pagination).",
            json!({ "site_id": site, "query": { "type": "object", "description": "Abfrage ohne site_id" } }),
            &["site_id", "query"],
        ),
        tool(
            "list_goals",
            "Ziele einer Website.",
            json!({ "site_id": site }),
            &["site_id"],
        ),
        tool(
            "create_goal",
            "Ziel anlegen (API-Schlüssel mit Bereich 'sites'): goal_type 'event' (event_name) oder 'page' (page_path, Platzhalter * und **).",
            json!({ "site_id": site, "goal_type": { "type": "string", "enum": ["event", "page"] }, "event_name": { "type": "string" }, "page_path": { "type": "string" }, "display_name": { "type": "string" }, "currency": { "type": "string", "description": "ISO-Code für Umsatz-Ziele" } }),
            &["site_id", "goal_type"],
        ),
        tool(
            "create_site",
            "Website anlegen (API-Schlüssel mit Bereich 'sites'). Gibt das Tracking-Snippet zurück.",
            json!({ "domain": { "type": "string" }, "timezone": { "type": "string", "default": "Europe/Zurich" } }),
            &["domain"],
        ),
        tool(
            "get_snippet",
            "Tracking-Snippet für <head> und Beispiele für eigene Ereignisse.",
            json!({ "site_id": site }),
            &["site_id"],
        ),
    ]
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
}

fn date_range(args: &Value) -> AppResult<DateRange> {
    match args.get("date_range") {
        None | Some(Value::Null) => Ok(DateRange::named("28d")),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|_| AppError::invalid("date_range: Text oder [von, bis]")),
    }
}

fn filters(args: &Value) -> AppResult<Vec<Filter>> {
    match args.get("filters") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => crate::api::parse_v1_filters(s),
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| AppError::invalid(format!("filters: {e}"))),
    }
}

fn metrics_arg(args: &Value, default: &[&str]) -> Vec<String> {
    match args.get("metrics").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => list
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => default.iter().map(|s| (*s).to_string()).collect(),
    }
}

async fn run_query(
    ctx: &Ctx<'_>,
    site_id: &str,
    q: Query,
) -> AppResult<nrzanalytics_shared::query::QueryResult> {
    let a = crate::api::public_api_site(ctx.state, ctx.user.clone(), site_id, Need::View).await?;
    ctx.state
        .db
        .read(move |c| {
            let site = site_ctx(c, &a.site)?;
            stats::execute(c, &site, &q, now())
        })
        .await
}

/// Ergebnis als Liste von Objekten (lesbarer fuer Agenten).
fn rows_as_objects(q: &Query, r: &nrzanalytics_shared::query::QueryResult) -> Value {
    let rows: Vec<Value> = r
        .results
        .iter()
        .map(|row| {
            let mut o = serde_json::Map::new();
            for (i, dim) in q.dimensions.iter().enumerate() {
                o.insert(dim.clone(), json!(row.dimensions.get(i)));
            }
            for (i, metric) in q.metrics.iter().enumerate() {
                o.insert(metric.clone(), row.metrics.get(i).cloned().unwrap_or(Value::Null));
                if let Some(cmp) = &row.comparison {
                    o.insert(
                        format!("{metric}_previous"),
                        cmp.metrics.get(i).cloned().unwrap_or(Value::Null),
                    );
                    o.insert(
                        format!("{metric}_change"),
                        cmp.change.get(i).cloned().unwrap_or(Value::Null),
                    );
                }
            }
            Value::Object(o)
        })
        .collect();
    json!({ "date_range": r.meta.date_range, "comparison_date_range": r.meta.comparison_date_range, "interval": r.meta.interval, "rows": rows })
}

async fn call(ctx: &Ctx<'_>, name: &str, args: &Value) -> AppResult<Value> {
    let site_id = arg_str(args, "site_id").unwrap_or_default();
    match name {
        "list_sites" => {
            let user = ctx.user.clone();
            let sites = ctx
                .state
                .db
                .read(move |c| {
                    let mut stmt = c.prepare(
                        "SELECT s.id, s.domain, s.timezone, m.role FROM sites s LEFT JOIN memberships m ON m.site_id = s.id AND m.user_id = ?1 \
                         WHERE m.user_id IS NOT NULL OR ?2 ORDER BY s.domain",
                    )?;
                    let rows: Vec<(i64, String, String, Option<String>)> = stmt
                        .query_map(rusqlite::params![user.id, user.is_admin], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                        .collect::<Result<_, _>>()?;
                    let mut out = Vec::new();
                    for (id, domain, tz, role) in rows {
                        let visitors: i64 = c.query_row(
                            "SELECT COUNT(DISTINCT user_id) FROM events WHERE site_id = ?1 AND ts >= ?2 AND name != 'engagement'",
                            rusqlite::params![id, now() - 86_400],
                            |r| r.get(0),
                        )?;
                        out.push(json!({ "domain": domain, "timezone": tz, "role": role.unwrap_or_else(|| "owner".into()), "visitors_24h": visitors }));
                    }
                    Ok(out)
                })
                .await?;
            Ok(json!({ "sites": sites }))
        }
        "get_site" => {
            let a = crate::api::public_api_site(ctx.state, ctx.user.clone(), &site_id, Need::View).await?;
            let base = ctx.base.clone();
            let site = ctx
                .state
                .db
                .read(move |c| crate::api::site_json(c, &a, &base))
                .await?;
            Ok(serde_json::to_value(site).unwrap_or_default())
        }
        "realtime_visitors" => {
            let a = crate::api::public_api_site(ctx.state, ctx.user.clone(), &site_id, Need::View).await?;
            let n = ctx
                .state
                .db
                .read(move |c| stats::current_visitors(c, a.site.id, now()))
                .await?;
            Ok(json!({ "site_id": site_id, "current_visitors": n }))
        }
        "aggregate" => {
            let mut q = Query::new(&[], date_range(args)?);
            q.metrics = metrics_arg(
                args,
                &[
                    m::VISITORS,
                    m::VISITS,
                    m::PAGEVIEWS,
                    m::VIEWS_PER_VISIT,
                    m::BOUNCE_RATE,
                    m::VISIT_DURATION,
                ],
            );
            q.date = arg_str(args, "date");
            q.filters = filters(args)?;
            if args.get("compare").and_then(Value::as_bool).unwrap_or(true) && !q.date_range.is_realtime() {
                q.include.comparisons = Some(Comparison::previous_period());
            }
            let r = run_query(ctx, &site_id, q.clone()).await?;
            Ok(rows_as_objects(&q, &r))
        }
        "timeseries" => {
            let mut q = Query::new(&[], date_range(args)?);
            q.metrics = metrics_arg(args, &[m::VISITORS, m::PAGEVIEWS]);
            q.date = arg_str(args, "date");
            q.filters = filters(args)?;
            let dim = match arg_str(args, "interval").as_deref() {
                Some("minute") => d::TIME_MINUTE,
                Some("hour") => d::TIME_HOUR,
                Some("day") => d::TIME_DAY,
                Some("week") => d::TIME_WEEK,
                Some("month") => d::TIME_MONTH,
                _ => d::TIME,
            };
            q.dimensions = vec![dim.into()];
            q.include.zero_fill = true;
            q.include.time_labels = true;
            let r = run_query(ctx, &site_id, q.clone()).await?;
            Ok(rows_as_objects(&q, &r))
        }
        "breakdown" => {
            let dimension = arg_str(args, "dimension").ok_or_else(|| AppError::invalid("dimension fehlt"))?;
            let mut q = Query::new(&[], date_range(args)?);
            let default: &[&str] = if dimension == d::EVENT_GOAL {
                &[m::VISITORS, m::EVENTS, m::CONVERSION_RATE]
            } else {
                &[m::VISITORS]
            };
            q.metrics = metrics_arg(args, default);
            q.date = arg_str(args, "date");
            q.filters = filters(args)?;
            q.dimensions = vec![dimension];
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(10)
                .clamp(1, 1000) as u32;
            q = q.limit(limit, 0);
            let r = run_query(ctx, &site_id, q.clone()).await?;
            Ok(rows_as_objects(&q, &r))
        }
        "query" => {
            let mut q: Query = serde_json::from_value(args.get("query").cloned().unwrap_or(json!({})))
                .map_err(|e| AppError::invalid(format!("query: {e}")))?;
            q.site_id = site_id.clone();
            let r = run_query(ctx, &site_id, q).await?;
            Ok(serde_json::to_value(r).unwrap_or_default())
        }
        "list_goals" => {
            let a = crate::api::public_api_site(ctx.state, ctx.user.clone(), &site_id, Need::View).await?;
            let goals = ctx.state.db.read(move |c| load_goals(c, a.site.id)).await?;
            Ok(json!({ "goals": goals }))
        }
        "create_goal" => {
            if ctx.scope != ApiScope::Sites {
                return Err(AppError::forbidden());
            }
            let a = crate::api::public_api_site(ctx.state, ctx.user.clone(), &site_id, Need::Edit).await?;
            let kind = match arg_str(args, "goal_type").as_deref() {
                Some("page") => GoalKind::Page,
                Some("event") => GoalKind::Event,
                _ => return Err(AppError::invalid("goal_type: event oder page")),
            };
            let req = NewGoal {
                kind,
                event_name: arg_str(args, "event_name"),
                page_path: arg_str(args, "page_path"),
                display_name: arg_str(args, "display_name"),
                currency: arg_str(args, "currency"),
            };
            let goal = ctx
                .state
                .db
                .write(move |c| crate::api::insert_goal(c, a.site.id, &req))
                .await?;
            Ok(serde_json::to_value(goal).unwrap_or_default())
        }
        "create_site" => {
            if ctx.scope != ApiScope::Sites {
                return Err(AppError::forbidden());
            }
            let domain = crate::api::check_domain(&arg_str(args, "domain").unwrap_or_default())?;
            let tz = arg_str(args, "timezone").unwrap_or_else(|| "Europe/Zurich".into());
            if tz.parse::<chrono_tz::Tz>().is_err() {
                return Err(AppError::invalid("Unbekannte Zeitzone."));
            }
            let user = ctx.user.clone();
            let (d2, t2) = (domain.clone(), tz.clone());
            ctx.state
                .db
                .write(move |c| {
                    c.execute("INSERT INTO sites (domain, timezone, created_at) VALUES (?1, ?2, ?3)", rusqlite::params![d2, t2, now()])
                        .map_err(|e| match AppError::from(e) {
                            err if err.code == "conflict" => AppError::conflict("Diese Domain gibt es schon."),
                            err => err,
                        })?;
                    let id = c.last_insert_rowid();
                    c.execute(
                        "INSERT INTO memberships (site_id, user_id, role, created_at) VALUES (?1, ?2, 'owner', ?3)",
                        rusqlite::params![id, user.id, now()],
                    )?;
                    Ok(())
                })
                .await?;
            ctx.state.ingest.reload_sites().await?;
            Ok(json!({ "domain": domain, "timezone": tz, "snippet": snippet(&ctx.base, &domain) }))
        }
        "get_snippet" => {
            let a = crate::api::public_api_site(ctx.state, ctx.user.clone(), &site_id, Need::View).await?;
            let s = snippet(&ctx.base, &a.site.domain);
            Ok(json!({
                "snippet": s,
                "custom_events": "window.plausible = window.plausible || function() { (window.plausible.q = window.plausible.q || []).push(arguments) }; plausible('Signup', { props: { plan: 'pro' } })",
                "server_side": format!("POST {}/api/event mit JSON {{\"name\":\"Signup\",\"url\":\"https://{}/\",\"domain\":\"{}\"}} und den Headern User-Agent und X-Forwarded-For des Besuchers", ctx.base, a.site.domain, a.site.domain),
                "script_variants": ["script.js", "script.hash.js", "script.outbound-links.js", "script.file-downloads.js", "script.tagged-events.js", "script.revenue.js", "script.pageview-props.js", "script.manual.js", "script.local.js"]
            }))
        }
        other => Err(AppError::invalid(format!("Unbekanntes Werkzeug: {other}"))),
    }
}
