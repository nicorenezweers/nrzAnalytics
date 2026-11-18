//! Auswertung: fuehrt eine [`Query`] (kompatibel mit der Stats-API v2 von Plausible) aus.
//!
//! Drei Arten von Abfragen:
//! - **Standard**: Ereignis-Kennzahlen aus `events`, Besuchs-Kennzahlen aus `sessions`, zusammengefuehrt
//!   ueber die Dimensionswerte.
//! - **Sitzungen**: bei Einstiegs-/Ausstiegsseite alles aus `sessions`.
//! - **Ziele**: bei `event:goal` je Ziel eine Teilabfrage (`UNION ALL`).
//!
//! Sortierung und Seitenaufteilung passieren in SQL, wenn die Sortier-Kennzahl direkt aus der
//! Hauptabfrage kommt (schnell auch bei vielen Seiten), sonst in Rust.

pub mod export;
pub mod funnel;
pub mod sql;
pub mod suggest;
pub mod time;

use crate::db::bounds_blob;
use crate::error::{AppError, AppResult};
use chrono::NaiveDate;
use chrono_tz::Tz;
use nrzanalytics_shared::{
    api::Goal,
    query::{
        DIRECT_SOURCE, DateRange, Filter, NONE_VALUE, Query, QueryMeta, QueryResult, ResultRow,
        RowComparison, SortDirection, dimensions as d, metrics as m,
    },
};
use rusqlite::{Connection, types::Value};
use serde_json::json;
use sql::{Dim, FilterEnv, Params, Table};
use std::collections::HashMap;
use time::{Interval, Range};

/// Website-Kontext fuer Auswertungen.
#[derive(Clone, Debug)]
pub struct SiteCtx {
    pub id: i64,
    pub domain: String,
    pub tz: Tz,
    pub goals: Vec<Goal>,
}

impl SiteCtx {
    /// Erster Tag mit Daten.
    pub fn first_day(&self, c: &Connection) -> AppResult<Option<NaiveDate>> {
        let ts: Option<i64> =
            c.query_row("SELECT MIN(ts) FROM events WHERE site_id = ?1", [self.id], |r| {
                r.get(0)
            })?;
        Ok(ts.map(|t| time::local_date(self.tz, t)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Standard,
    Sessions,
    Goals,
}

/// Dimensionswerte einer Zeile (Zeitscheiben als Index).
type Key = Vec<String>;
type Metrics = HashMap<String, f64>;

#[derive(Default, Debug)]
struct RowSet {
    rows: Vec<(Key, Metrics)>,
    total: Option<u64>,
    /// Sortierung und Seitenaufteilung schon in SQL erledigt.
    paged: bool,
}

impl RowSet {
    fn map(&self) -> HashMap<Key, Metrics> {
        self.rows.iter().cloned().collect()
    }
}

struct Order {
    metric: String,
    desc: bool,
    limit: u32,
    offset: u32,
}

struct Ctx<'a> {
    site: &'a SiteCtx,
    range: &'a Range,
    bounds: Option<&'a [i64]>,
}

const EVENT_METRICS: [&str; 5] = [
    m::VISITORS,
    m::PAGEVIEWS,
    m::EVENTS,
    m::TOTAL_REVENUE,
    m::AVERAGE_REVENUE,
];
const SESSION_METRICS: [&str; 4] = [m::VISITS, m::BOUNCE_RATE, m::VISIT_DURATION, m::VIEWS_PER_VISIT];
const ENGAGEMENT_METRICS: [&str; 2] = [m::TIME_ON_PAGE, m::SCROLL_DEPTH];

fn event_expr(metric: &str) -> Option<&'static str> {
    Some(match metric {
        m::VISITORS => "COUNT(DISTINCT e.user_id)",
        m::PAGEVIEWS => "SUM(e.name = 'pageview')",
        m::EVENTS => "COUNT(*)",
        m::VISITS => "COUNT(DISTINCT e.session_id)",
        m::TOTAL_REVENUE => "SUM(e.revenue)",
        m::AVERAGE_REVENUE => "AVG(e.revenue)",
        _ => return None,
    })
}

fn session_expr(metric: &str) -> Option<&'static str> {
    Some(match metric {
        m::VISITORS => "COUNT(DISTINCT s.user_id)",
        m::VISITS => "COUNT(*)",
        m::PAGEVIEWS => "SUM(s.pageviews)",
        m::EVENTS => "SUM(s.events)",
        m::BOUNCE_RATE => "100.0 * SUM(s.is_bounce) / COUNT(*)",
        m::VISIT_DURATION => "AVG(s.end - s.start)",
        m::VIEWS_PER_VISIT => "1.0 * SUM(s.pageviews) / COUNT(*)",
        _ => return None,
    })
}

fn value_to_key(v: Value) -> String {
    match v {
        Value::Integer(i) => i.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Text(s) => s,
        Value::Blob(_) | Value::Null => String::new(),
    }
}

fn key_param(dim: &Dim, value: &str) -> Value {
    if dim.is_time() {
        Value::Integer(value.parse().unwrap_or(-1))
    } else {
        Value::Text(value.to_string())
    }
}

/// Eine gruppierte Teilabfrage auf `events` oder `sessions`.
#[allow(clippy::too_many_arguments)]
fn run_sub(
    c: &Connection,
    ctx: &Ctx<'_>,
    table: Table,
    dims: &[Dim],
    metrics: &[(&str, String)],
    extra_where: &[&str],
    filters: &[Filter],
    restrict: Option<&[Key]>,
    order: Option<&Order>,
    want_total: bool,
) -> AppResult<RowSet> {
    let mut p = Params::default();
    let site = p.add(ctx.site.id);
    let start = p.add(ctx.range.start);
    let end = p.add(ctx.range.end);
    let bounds = ctx.bounds.map(|b| p.add(bounds_blob(b)));
    let (from, mut wh) = match table {
        Table::Events => (
            "events e",
            vec![
                format!("e.site_id = {site}"),
                format!("e.ts >= {start}"),
                format!("e.ts < {end}"),
            ],
        ),
        Table::Sessions => (
            "sessions s",
            vec![
                format!("s.site_id = {site}"),
                format!("s.start >= {start}"),
                format!("s.start < {end}"),
            ],
        ),
    };
    wh.extend(extra_where.iter().map(|s| (*s).to_string()));
    let mut dim_exprs = Vec::with_capacity(dims.len());
    for dim in dims {
        let expr = dim.expr(table, &mut p, bounds.as_deref())?;
        if dim.is_time() {
            wh.push(format!("{expr} >= 0"));
        }
        if let Some(col) = dim.skip_empty_column() {
            let a = if table == Table::Events { "e" } else { "s" };
            wh.push(format!("{a}.{col} != ''"));
        }
        dim_exprs.push(expr);
    }
    let env = FilterEnv {
        goals: &ctx.site.goals,
        site: site.clone(),
        start: start.clone(),
        end: end.clone(),
    };
    for f in filters {
        wh.push(sql::filter_expr(f, table, &env, &mut p)?);
    }
    if let Some(keys) = restrict {
        if dims.is_empty() {
            // nichts einzuschraenken
        } else if keys.is_empty() {
            return Ok(RowSet::default());
        } else if dims.len() == 1 {
            let list: Vec<String> = keys.iter().map(|k| p.add(key_param(&dims[0], &k[0]))).collect();
            wh.push(format!("{} IN ({})", dim_exprs[0], list.join(", ")));
        } else {
            let rows: Vec<String> = keys
                .iter()
                .map(|k| {
                    let vals: Vec<String> = dims
                        .iter()
                        .zip(k.iter())
                        .map(|(dim, v)| p.add(key_param(dim, v)))
                        .collect();
                    format!("({})", vals.join(", "))
                })
                .collect();
            wh.push(format!(
                "({}) IN (VALUES {})",
                dim_exprs.join(", "),
                rows.join(", ")
            ));
        }
    }
    let where_sql = wh.join(" AND ");
    let dim_aliases: Vec<String> = (0..dims.len()).map(|i| format!("d{i}")).collect();
    let mut select: Vec<String> = dim_exprs
        .iter()
        .zip(&dim_aliases)
        .map(|(e, a)| format!("{e} AS {a}"))
        .collect();
    for (name, expr) in metrics {
        select.push(format!("{expr} AS m_{name}"));
    }
    let group = if dims.is_empty() {
        String::new()
    } else {
        format!(" GROUP BY {}", dim_aliases.join(", "))
    };
    let mut sql = format!(
        "SELECT {} FROM {from} WHERE {where_sql}{group}",
        select.join(", ")
    );
    if let Some(o) = order {
        let dir = if o.desc { "DESC" } else { "ASC" };
        let tiebreak: Vec<String> = dim_aliases.iter().map(|a| format!("{a} ASC")).collect();
        let mut ord = vec![format!("m_{} {dir}", o.metric)];
        ord.extend(tiebreak);
        let limit = p.add(i64::from(o.limit));
        let offset = p.add(i64::from(o.offset));
        sql.push_str(&format!(
            " ORDER BY {} LIMIT {limit} OFFSET {offset}",
            ord.join(", ")
        ));
    }
    let total = if want_total && !dims.is_empty() {
        // Zaehlung ohne LIMIT/OFFSET: dieselben Parameter bis auf die letzten zwei.
        let count_sql = format!(
            "SELECT COUNT(*) FROM (SELECT {} FROM {from} WHERE {where_sql}{group})",
            dim_exprs
                .iter()
                .zip(&dim_aliases)
                .map(|(e, a)| format!("{e} AS {a}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let n = if order.is_some() {
            p.values.len() - 2
        } else {
            p.values.len()
        };
        let params = p.values.get(..n).unwrap_or(&[]);
        let total: i64 = c.query_row(&count_sql, rusqlite::params_from_iter(params.iter()), |r| {
            r.get(0)
        })?;
        Some(total.max(0) as u64)
    } else {
        None
    };
    let mut stmt = c.prepare(&sql)?;
    let n_dims = dims.len();
    let names: Vec<String> = metrics.iter().map(|(n, _)| (*n).to_string()).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(p.values.iter()), |r| {
            let mut key = Vec::with_capacity(n_dims);
            for i in 0..n_dims {
                key.push(value_to_key(r.get::<_, Value>(i)?));
            }
            let mut vals = Metrics::new();
            for (j, name) in names.iter().enumerate() {
                let v: Option<f64> = r.get(n_dims + j)?;
                if let Some(v) = v {
                    vals.insert(name.clone(), v);
                }
            }
            Ok((key, vals))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RowSet {
        rows,
        total,
        paged: order.is_some(),
    })
}

/// Zeit auf der Seite und Scrolltiefe aus Engagement-Ereignissen (je Besuch und Seite).
fn run_engagement(
    c: &Connection,
    ctx: &Ctx<'_>,
    dims: &[Dim],
    filters: &[Filter],
    restrict: Option<&[Key]>,
) -> AppResult<RowSet> {
    let mut p = Params::default();
    let site = p.add(ctx.site.id);
    let start = p.add(ctx.range.start);
    let end = p.add(ctx.range.end);
    let bounds = ctx.bounds.map(|b| p.add(bounds_blob(b)));
    let mut wh = vec![
        format!("e.site_id = {site}"),
        format!("e.ts >= {start}"),
        format!("e.ts < {end}"),
        "e.name = 'engagement'".to_string(),
    ];
    let mut dim_exprs = Vec::new();
    for dim in dims {
        let expr = dim.expr(Table::Events, &mut p, bounds.as_deref())?;
        if dim.is_time() {
            wh.push(format!("{expr} >= 0"));
        }
        dim_exprs.push(expr);
    }
    let env = FilterEnv {
        goals: &ctx.site.goals,
        site,
        start,
        end,
    };
    for f in filters {
        wh.push(sql::filter_expr(f, Table::Events, &env, &mut p)?);
    }
    if let (Some(keys), false) = (restrict, dims.is_empty()) {
        if keys.is_empty() {
            return Ok(RowSet::default());
        }
        if dims.len() == 1 {
            let list: Vec<String> = keys.iter().map(|k| p.add(key_param(&dims[0], &k[0]))).collect();
            wh.push(format!("{} IN ({})", dim_exprs[0], list.join(", ")));
        }
    }
    let aliases: Vec<String> = (0..dims.len()).map(|i| format!("d{i}")).collect();
    let inner_select: Vec<String> = dim_exprs
        .iter()
        .zip(&aliases)
        .map(|(e, a)| format!("{e} AS {a}"))
        .collect();
    let mut inner_group = aliases.clone();
    inner_group.push("e.session_id".into());
    inner_group.push("e.pathname".into());
    let mut inner_cols = inner_select;
    inner_cols.push("SUM(e.engagement_ms) AS t".into());
    inner_cols.push("MAX(e.scroll_depth) AS sd".into());
    let outer_group = if dims.is_empty() {
        String::new()
    } else {
        format!(" GROUP BY {}", aliases.join(", "))
    };
    let mut outer_cols = aliases.clone();
    outer_cols.push("AVG(t) / 1000.0".into());
    outer_cols.push("AVG(sd)".into());
    let sql = format!(
        "SELECT {} FROM (SELECT {} FROM events e WHERE {} GROUP BY {}){outer_group}",
        outer_cols.join(", "),
        inner_cols.join(", "),
        wh.join(" AND "),
        inner_group.join(", ")
    );
    let mut stmt = c.prepare(&sql)?;
    let n = dims.len();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(p.values.iter()), |r| {
            let mut key = Vec::with_capacity(n);
            for i in 0..n {
                key.push(value_to_key(r.get::<_, Value>(i)?));
            }
            let mut vals = Metrics::new();
            if let Some(t) = r.get::<_, Option<f64>>(n)? {
                vals.insert(m::TIME_ON_PAGE.into(), t);
            }
            if let Some(sd) = r.get::<_, Option<f64>>(n + 1)? {
                vals.insert(m::SCROLL_DEPTH.into(), sd);
            }
            Ok((key, vals))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RowSet {
        rows,
        total: None,
        paged: false,
    })
}

/// Ziele: je Ziel eine Teilabfrage, zusammengefasst nach Ziel (und weiteren Dimensionen).
fn run_goals(c: &Connection, ctx: &Ctx<'_>, dims: &[Dim], filters: &[Filter]) -> AppResult<RowSet> {
    if ctx.site.goals.is_empty() {
        return Ok(RowSet::default());
    }
    let mut p = Params::default();
    let site = p.add(ctx.site.id);
    let start = p.add(ctx.range.start);
    let end = p.add(ctx.range.end);
    let bounds = ctx.bounds.map(|b| p.add(bounds_blob(b)));
    let mut wh = vec![
        format!("e.site_id = {site}"),
        format!("e.ts >= {start}"),
        format!("e.ts < {end}"),
        "e.name != 'engagement'".to_string(),
    ];
    let mut other: Vec<(usize, String)> = Vec::new();
    for (i, dim) in dims.iter().enumerate() {
        if *dim == Dim::Goal {
            continue;
        }
        let expr = dim.expr(Table::Events, &mut p, bounds.as_deref())?;
        if dim.is_time() {
            wh.push(format!("{expr} >= 0"));
        }
        if let Some(col) = dim.skip_empty_column() {
            wh.push(format!("e.{col} != ''"));
        }
        other.push((i, expr));
    }
    let env = FilterEnv {
        goals: &ctx.site.goals,
        site,
        start,
        end,
    };
    for f in filters {
        wh.push(sql::filter_expr(f, Table::Events, &env, &mut p)?);
    }
    let base = wh.join(" AND ");
    let aliases: Vec<String> = other.iter().map(|(i, _)| format!("d{i}")).collect();
    let cols: Vec<String> = other
        .iter()
        .zip(&aliases)
        .map(|((_, e), a)| format!("{e} AS {a}"))
        .collect();
    let mut branches = Vec::new();
    for (gi, goal) in ctx.site.goals.iter().enumerate() {
        let pred = sql::goal_predicate(goal, &mut p);
        let mut sel = vec![format!("{gi} AS g")];
        sel.extend(cols.iter().cloned());
        sel.push("e.user_id AS user_id, e.session_id AS session_id, e.revenue AS revenue".into());
        branches.push(format!(
            "SELECT {} FROM events e WHERE {base} AND {pred}",
            sel.join(", ")
        ));
    }
    let mut group = vec!["g".to_string()];
    group.extend(aliases.iter().cloned());
    let sql = format!(
        "SELECT {}, COUNT(DISTINCT user_id), COUNT(*), COUNT(DISTINCT session_id), SUM(revenue), AVG(revenue) \
         FROM ({}) GROUP BY {}",
        group.join(", "),
        branches.join(" UNION ALL "),
        group.join(", ")
    );
    let mut stmt = c.prepare(&sql)?;
    let n_other = aliases.len();
    let raw = stmt
        .query_map(rusqlite::params_from_iter(p.values.iter()), |r| {
            let g: i64 = r.get(0)?;
            let mut others = Vec::with_capacity(n_other);
            for i in 0..n_other {
                others.push(value_to_key(r.get::<_, Value>(1 + i)?));
            }
            let base = 1 + n_other;
            let mut vals = Metrics::new();
            vals.insert(m::VISITORS.into(), r.get::<_, f64>(base)?);
            vals.insert(m::EVENTS.into(), r.get::<_, f64>(base + 1)?);
            vals.insert(m::VISITS.into(), r.get::<_, f64>(base + 2)?);
            if let Some(v) = r.get::<_, Option<f64>>(base + 3)? {
                vals.insert(m::TOTAL_REVENUE.into(), v);
            }
            if let Some(v) = r.get::<_, Option<f64>>(base + 4)? {
                vals.insert(m::AVERAGE_REVENUE.into(), v);
            }
            Ok((g, others, vals))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut rows = Vec::new();
    for (g, others, vals) in raw {
        let Some(goal) = usize::try_from(g).ok().and_then(|g| ctx.site.goals.get(g)) else {
            continue;
        };
        let mut key = Vec::with_capacity(dims.len());
        let mut it = others.into_iter();
        for dim in dims {
            if *dim == Dim::Goal {
                key.push(goal.display_name.clone());
            } else {
                key.push(it.next().unwrap_or_default());
            }
        }
        rows.push((key, vals));
    }
    Ok(RowSet {
        rows,
        total: None,
        paged: false,
    })
}

/// Vorbereitete Abfrage.
struct Plan<'q> {
    q: &'q Query,
    dims: Vec<Dim>,
    mode: Mode,
    interval: Option<Interval>,
}

fn validate<'q>(q: &'q Query, default_interval: Interval) -> AppResult<Plan<'q>> {
    if q.metrics.is_empty() {
        return Err(AppError::invalid("Mindestens eine Kennzahl angeben (metrics)."));
    }
    for metric in &q.metrics {
        if !m::ALL.contains(&metric.as_str()) {
            return Err(AppError::invalid(format!("Unbekannte Kennzahl: {metric}")));
        }
    }
    let dims = q
        .dimensions
        .iter()
        .map(|s| Dim::parse(s, default_interval))
        .collect::<AppResult<Vec<_>>>()?;
    let times: Vec<Interval> = dims
        .iter()
        .filter_map(|dim| match dim {
            Dim::Time(i) => Some(*i),
            _ => None,
        })
        .collect();
    if times.len() > 1 {
        return Err(AppError::invalid("Höchstens eine Zeit-Dimension."));
    }
    if dims.len() > 3 {
        return Err(AppError::invalid("Höchstens drei Dimensionen."));
    }
    let has_session_only = dims.iter().any(Dim::is_session_only);
    let has_event_level = dims.iter().any(Dim::is_event_level);
    if has_session_only && has_event_level {
        return Err(AppError::invalid(
            "Einstiegs-/Ausstiegsseiten lassen sich nicht mit Ereignis-Dimensionen kombinieren.",
        ));
    }
    let has_goal_dim = dims.contains(&Dim::Goal);
    if has_goal_dim && dims.iter().any(|x| x.is_event_level() && *x != Dim::Goal) {
        return Err(AppError::invalid(
            "event:goal lässt sich nicht mit anderen Ereignis-Dimensionen kombinieren.",
        ));
    }
    for f in &q.filters {
        for dim in f.dimensions() {
            if !d::is_known(dim) || d::is_time(dim) || dim.ends_with("_name") {
                return Err(AppError::invalid(format!(
                    "Nach '{dim}' kann nicht gefiltert werden."
                )));
            }
        }
    }
    let goal_filter = sql::has_goal_filter(&q.filters);
    for metric in &q.metrics {
        match metric.as_str() {
            m::CONVERSION_RATE | m::GROUP_CONVERSION_RATE if !goal_filter && !has_goal_dim => {
                return Err(AppError::invalid(format!(
                    "{metric} braucht einen Ziel-Filter (event:goal) oder die Dimension event:goal."
                )));
            }
            m::EXIT_RATE if !dims.contains(&Dim::ExitPage) => {
                return Err(AppError::invalid(
                    "exit_rate gibt es nur mit der Dimension visit:exit_page.",
                ));
            }
            _ => {}
        }
    }
    let mode = if has_goal_dim {
        Mode::Goals
    } else if has_session_only {
        Mode::Sessions
    } else {
        Mode::Standard
    };
    Ok(Plan {
        q,
        dims,
        mode,
        interval: times.first().copied(),
    })
}

/// Schluessel ohne die Ziel-Dimension (fuer die Basis der Konversionsrate).
fn strip_goal(dims: &[Dim], key: &Key) -> Key {
    dims.iter()
        .zip(key.iter())
        .filter(|(dim, _)| **dim != Dim::Goal)
        .map(|(_, v)| v.clone())
        .collect()
}

/// Sortier-Kennzahl und Richtung.
fn sort_spec(plan: &Plan<'_>) -> Vec<(String, SortDirection)> {
    if let Some(o) = &plan.q.order_by
        && !o.is_empty()
    {
        return o.clone();
    }
    if plan.interval.is_some() {
        let time_name = plan
            .q
            .dimensions
            .iter()
            .find(|x| d::is_time(x))
            .cloned()
            .unwrap_or_else(|| d::TIME.into());
        let mut v = vec![(time_name, SortDirection::Asc)];
        v.push((plan.q.metrics[0].clone(), SortDirection::Desc));
        return v;
    }
    vec![(plan.q.metrics[0].clone(), SortDirection::Desc)]
}

/// Alle Kennzahlen einer Abfrage fuer einen Zeitraum.
fn compute(
    c: &Connection,
    plan: &Plan<'_>,
    ctx: &Ctx<'_>,
    page: Option<(u32, u32)>,
    restrict: Option<&[Key]>,
    want_total: bool,
) -> AppResult<RowSet> {
    let q = plan.q;
    let dims = &plan.dims;
    let want = |metric: &str| q.metrics.iter().any(|x| x == metric);
    let has_event_dims = dims.iter().any(Dim::is_event_level);
    let filters = &q.filters;
    let sort = sort_spec(plan);
    let (sort_key, sort_dir) = sort
        .first()
        .cloned()
        .unwrap_or((q.metrics[0].clone(), SortDirection::Desc));

    let mut set = match plan.mode {
        Mode::Goals => {
            let mut set = run_goals(c, ctx, dims, filters)?;
            if let Some(keys) = restrict {
                set.rows.retain(|(k, _)| keys.contains(k));
            }
            set
        }
        Mode::Sessions => {
            let mut metrics: Vec<(&str, String)> = Vec::new();
            for metric in [
                m::VISITORS,
                m::VISITS,
                m::PAGEVIEWS,
                m::EVENTS,
                m::BOUNCE_RATE,
                m::VISIT_DURATION,
                m::VIEWS_PER_VISIT,
            ] {
                if (want(metric) || metric == m::VISITORS || (metric == m::VISITS && want(m::EXIT_RATE)))
                    && let Some(e) = session_expr(metric)
                {
                    metrics.push((metric, e.to_string()));
                }
            }
            let sql_order =
                page.filter(|_| metrics.iter().any(|(n, _)| *n == sort_key))
                    .map(|(limit, offset)| Order {
                        metric: sort_key.clone(),
                        desc: sort_dir == SortDirection::Desc,
                        limit,
                        offset,
                    });
            run_sub(
                c,
                ctx,
                Table::Sessions,
                dims,
                &metrics,
                &[],
                filters,
                restrict,
                sql_order.as_ref(),
                want_total,
            )?
        }
        Mode::Standard => {
            // Ereignis-Kennzahlen
            let mut e_metrics: Vec<(&str, String)> =
                vec![(m::VISITORS, event_expr(m::VISITORS).unwrap_or_default().into())];
            for metric in EVENT_METRICS {
                if metric != m::VISITORS && want(metric) {
                    e_metrics.push((metric, event_expr(metric).unwrap_or_default().into()));
                }
            }
            if has_event_dims && want(m::VISITS) {
                e_metrics.push((m::VISITS, event_expr(m::VISITS).unwrap_or_default().into()));
            }
            // Besuchs-Kennzahlen (ohne Ereignis-Dimensionen)
            let s_metrics: Vec<(&str, String)> = if has_event_dims {
                Vec::new()
            } else {
                SESSION_METRICS
                    .iter()
                    .filter(|x| want(x))
                    .map(|x| (*x, session_expr(x).unwrap_or_default().to_string()))
                    .collect()
            };
            let sort_in_e = e_metrics.iter().any(|(n, _)| *n == sort_key);
            let sort_in_s = s_metrics.iter().any(|(n, _)| *n == sort_key);
            let primary_order = |use_it: bool| {
                page.filter(|_| use_it).map(|(limit, offset)| Order {
                    metric: sort_key.clone(),
                    desc: sort_dir == SortDirection::Desc,
                    limit,
                    offset,
                })
            };
            let mut set;
            if sort_in_s && !sort_in_e && !dims.is_empty() {
                // Hauptabfrage auf Sitzungen, Ereignis-Kennzahlen danach fuer dieselben Zeilen.
                let order = primary_order(true);
                set = run_sub(
                    c,
                    ctx,
                    Table::Sessions,
                    dims,
                    &s_metrics,
                    &[],
                    filters,
                    restrict,
                    order.as_ref(),
                    want_total,
                )?;
                let keys: Vec<Key> = set.rows.iter().map(|(k, _)| k.clone()).collect();
                let e = run_sub(
                    c,
                    ctx,
                    Table::Events,
                    dims,
                    &e_metrics,
                    &["e.name != 'engagement'"],
                    filters,
                    Some(&keys),
                    None,
                    false,
                )?
                .map();
                for (k, vals) in &mut set.rows {
                    if let Some(ev) = e.get(k) {
                        vals.extend(ev.clone());
                    }
                }
            } else {
                let order = primary_order(sort_in_e);
                set = run_sub(
                    c,
                    ctx,
                    Table::Events,
                    dims,
                    &e_metrics,
                    &["e.name != 'engagement'"],
                    filters,
                    restrict,
                    order.as_ref(),
                    want_total,
                )?;
                if !s_metrics.is_empty() {
                    let keys: Vec<Key> = set.rows.iter().map(|(k, _)| k.clone()).collect();
                    let restrict_s = if dims.is_empty() {
                        None
                    } else {
                        Some(keys.as_slice())
                    };
                    let s = run_sub(
                        c,
                        ctx,
                        Table::Sessions,
                        dims,
                        &s_metrics,
                        &[],
                        filters,
                        restrict_s,
                        None,
                        false,
                    )?;
                    if dims.is_empty() {
                        // Aggregat: immer genau eine Zeile.
                        let sv = s.rows.into_iter().next().map(|(_, v)| v).unwrap_or_default();
                        if set.rows.is_empty() {
                            set.rows.push((Vec::new(), Metrics::new()));
                        }
                        if let Some((_, vals)) = set.rows.first_mut() {
                            vals.extend(sv);
                        }
                    } else {
                        let s = s.map();
                        for (k, vals) in &mut set.rows {
                            if let Some(sv) = s.get(k) {
                                vals.extend(sv.clone());
                            }
                        }
                    }
                }
            }
            // Seiten: Absprungrate und Besuchsdauer der Besuche, die auf dieser Seite begannen.
            if dims == &[Dim::Page] && (want(m::BOUNCE_RATE) || want(m::VISIT_DURATION)) {
                let keys: Vec<Key> = set.rows.iter().map(|(k, _)| k.clone()).collect();
                let metrics: Vec<(&str, String)> = [m::BOUNCE_RATE, m::VISIT_DURATION]
                    .iter()
                    .map(|x| (*x, session_expr(x).unwrap_or_default().to_string()))
                    .collect();
                let entry = run_sub(
                    c,
                    ctx,
                    Table::Sessions,
                    &[Dim::EntryPage],
                    &metrics,
                    &[],
                    filters,
                    Some(&keys),
                    None,
                    false,
                )?
                .map();
                for (k, vals) in &mut set.rows {
                    if let Some(ev) = entry.get(k) {
                        vals.extend(ev.clone());
                    }
                }
            }
            set
        }
    };

    // Engagement (Zeit auf der Seite, Scrolltiefe)
    if plan.mode != Mode::Sessions && ENGAGEMENT_METRICS.iter().any(|x| want(x)) {
        let keys: Vec<Key> = set.rows.iter().map(|(k, _)| k.clone()).collect();
        let eng_dims: Vec<Dim> = dims.iter().filter(|x| **x != Dim::Goal).cloned().collect();
        if eng_dims.len() == dims.len() {
            let eng = run_engagement(
                c,
                ctx,
                &eng_dims,
                filters,
                if dims.is_empty() { None } else { Some(&keys) },
            )?;
            if dims.is_empty() {
                if let (Some((_, vals)), Some((_, ev))) = (set.rows.first_mut(), eng.rows.first()) {
                    vals.extend(ev.clone());
                }
            } else {
                let eng = eng.map();
                for (k, vals) in &mut set.rows {
                    if let Some(ev) = eng.get(k) {
                        vals.extend(ev.clone());
                    }
                }
            }
        }
    }

    // Konversionsrate: Besucher mit Ziel / Besucher ohne Ziel-Filter.
    if want(m::CONVERSION_RATE) || want(m::GROUP_CONVERSION_RATE) {
        let base_filters = sql::without_goal_filters(filters);
        let base_dims: Vec<Dim> = dims.iter().filter(|x| **x != Dim::Goal).cloned().collect();
        let table = if plan.mode == Mode::Sessions {
            Table::Sessions
        } else {
            Table::Events
        };
        let visitors_expr = if table == Table::Sessions {
            session_expr(m::VISITORS)
        } else {
            event_expr(m::VISITORS)
        };
        let metrics = vec![(m::VISITORS, visitors_expr.unwrap_or_default().to_string())];
        let extra: &[&str] = if table == Table::Events {
            &["e.name != 'engagement'"]
        } else {
            &[]
        };
        let base_keys: Vec<Key> = set.rows.iter().map(|(k, _)| strip_goal(dims, k)).collect();
        let restrict_b = if base_dims.is_empty() {
            None
        } else {
            Some(base_keys.as_slice())
        };
        let base = run_sub(
            c,
            ctx,
            table,
            &base_dims,
            &metrics,
            extra,
            &base_filters,
            restrict_b,
            None,
            false,
        )?;
        let base_map = base.map();
        for (k, vals) in &mut set.rows {
            let bk = strip_goal(dims, k);
            let total = base_map
                .get(&bk)
                .and_then(|v| v.get(m::VISITORS))
                .copied()
                .unwrap_or(0.0);
            let conv = vals.get(m::VISITORS).copied().unwrap_or(0.0);
            let rate = if total > 0.0 { conv / total * 100.0 } else { 0.0 };
            vals.insert(m::CONVERSION_RATE.into(), rate);
            vals.insert(m::GROUP_CONVERSION_RATE.into(), rate);
        }
    }

    // Anteil an allen Besuchern
    if want(m::PERCENTAGE) {
        let table = if plan.mode == Mode::Sessions {
            Table::Sessions
        } else {
            Table::Events
        };
        let visitors_expr = if table == Table::Sessions {
            session_expr(m::VISITORS)
        } else {
            event_expr(m::VISITORS)
        };
        let extra: &[&str] = if table == Table::Events {
            &["e.name != 'engagement'"]
        } else {
            &[]
        };
        let total = run_sub(
            c,
            ctx,
            table,
            &[],
            &[(m::VISITORS, visitors_expr.unwrap_or_default().to_string())],
            extra,
            filters,
            None,
            None,
            false,
        )?;
        let t = total
            .rows
            .first()
            .and_then(|(_, v)| v.get(m::VISITORS))
            .copied()
            .unwrap_or(0.0);
        for (_, vals) in &mut set.rows {
            let v = vals.get(m::VISITORS).copied().unwrap_or(0.0);
            vals.insert(m::PERCENTAGE.into(), if t > 0.0 { v / t * 100.0 } else { 0.0 });
        }
    }

    // Ausstiegsrate: Ausstiege / Seitenaufrufe der Seite.
    if want(m::EXIT_RATE) && dims == &[Dim::ExitPage] {
        let keys: Vec<Key> = set.rows.iter().map(|(k, _)| k.clone()).collect();
        let pv = run_sub(
            c,
            ctx,
            Table::Events,
            &[Dim::Page],
            &[(m::PAGEVIEWS, "COUNT(*)".into())],
            &["e.name = 'pageview'"],
            &q.filters
                .iter()
                .filter(|f| !f.dimensions().iter().any(|x| x.starts_with("visit:e")))
                .cloned()
                .collect::<Vec<_>>(),
            Some(&keys),
            None,
            false,
        )?
        .map();
        for (k, vals) in &mut set.rows {
            let exits = vals.get(m::VISITS).copied().unwrap_or(0.0);
            let views = pv
                .get(k)
                .and_then(|v| v.get(m::PAGEVIEWS))
                .copied()
                .unwrap_or(0.0);
            if views > 0.0 {
                vals.insert(m::EXIT_RATE.into(), (exits / views * 100.0).min(100.0));
            }
        }
    }

    Ok(set)
}

fn compare_values(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    a.unwrap_or(f64::NEG_INFINITY)
        .partial_cmp(&b.unwrap_or(f64::NEG_INFINITY))
        .unwrap_or(std::cmp::Ordering::Equal)
}

fn sort_rows(plan: &Plan<'_>, rows: &mut [(Key, Metrics)]) {
    let spec = sort_spec(plan);
    let dims = &plan.q.dimensions;
    rows.sort_by(|(ka, va), (kb, vb)| {
        for (key, dir) in &spec {
            let ord = if let Some(i) = dims.iter().position(|x| x == key) {
                if plan.dims.get(i).is_some_and(Dim::is_time) {
                    let a: i64 = ka.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
                    let b: i64 = kb.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
                    a.cmp(&b)
                } else {
                    ka.get(i).cmp(&kb.get(i))
                }
            } else if d::is_time(key) {
                std::cmp::Ordering::Equal
            } else {
                compare_values(va.get(key).copied(), vb.get(key).copied())
            };
            let ord = if *dir == SortDirection::Desc {
                ord.reverse()
            } else {
                ord
            };
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        ka.cmp(kb)
    });
}

/// Gerundeter Wert fuer die Ausgabe.
fn output_value(metric: &str, v: f64) -> serde_json::Value {
    match metric {
        m::VISITORS
        | m::VISITS
        | m::PAGEVIEWS
        | m::EVENTS
        | m::BOUNCE_RATE
        | m::VISIT_DURATION
        | m::TIME_ON_PAGE
        | m::SCROLL_DEPTH => json!(v.round() as i64),
        m::PERCENTAGE | m::EXIT_RATE => json!((v * 10.0).round() / 10.0),
        _ => json!((v * 100.0).round() / 100.0),
    }
}

fn metric_value(metric: &str, vals: Option<&Metrics>, future: bool) -> serde_json::Value {
    if future {
        return serde_json::Value::Null;
    }
    match vals.and_then(|v| v.get(metric)) {
        Some(v) => output_value(metric, *v),
        None => match metric {
            m::TIME_ON_PAGE | m::SCROLL_DEPTH | m::EXIT_RATE | m::TOTAL_REVENUE | m::AVERAGE_REVENUE => {
                serde_json::Value::Null
            }
            _ => json!(0),
        },
    }
}

fn change(metric: &str, cur: &serde_json::Value, prev: &serde_json::Value) -> serde_json::Value {
    let (Some(c), Some(p)) = (cur.as_f64(), prev.as_f64()) else {
        return serde_json::Value::Null;
    };
    if m::is_rate(metric) {
        return json!(((c - p) * 10.0).round() / 10.0);
    }
    if p == 0.0 {
        return if c == 0.0 {
            json!(0)
        } else {
            serde_json::Value::Null
        };
    }
    json!(((c - p) / p * 100.0).round() as i64)
}

/// Fuehrt eine Abfrage aus.
pub fn execute(c: &Connection, site: &SiteCtx, q: &Query, now: i64) -> AppResult<QueryResult> {
    let first_day = match &q.date_range {
        DateRange::Named(n) if n == "all" => site.first_day(c)?,
        _ => None,
    };
    let range = time::resolve(&q.date_range, q.date.as_deref(), site.tz, now, first_day)?;
    let default_iv = time::default_interval(&range, &q.date_range);
    let mut plan = validate(q, default_iv)?;
    let mut warnings = Vec::new();
    if let Some(iv) = plan.interval
        && !time::interval_allowed(&range, iv)
    {
        warnings.push(format!(
            "Intervall {} passt nicht zum Zeitraum, nehme {}.",
            iv.as_str(),
            default_iv.as_str()
        ));
        plan.interval = Some(default_iv);
        for dim in &mut plan.dims {
            if dim.is_time() {
                *dim = Dim::Time(default_iv);
            }
        }
    }
    let (bounds, labels) = match plan.interval {
        Some(iv) => {
            let (b, l) = time::buckets(&range, iv, site.tz);
            (Some(b), l)
        }
        None => (None, Vec::new()),
    };
    let ctx = Ctx {
        site,
        range: &range,
        bounds: bounds.as_deref(),
    };
    let pagination = q.pagination.map(|p| (p.limit.clamp(1, 10_000), p.offset));
    // SQL-Seitenaufteilung nur ohne Zeit-Dimension.
    let sql_page = if plan.interval.is_none() && !plan.dims.is_empty() {
        Some(pagination.unwrap_or((10_000, 0)))
    } else {
        None
    };
    let mut set = compute(c, &plan, &ctx, sql_page, None, q.include.total_rows)?;
    sort_rows(&plan, &mut set.rows);
    let mut total_rows = set.total;
    if !plan.dims.is_empty() && !set.paged {
        // In Rust aufteilen (Zeit-Dimension oder abgeleitete Sortierung).
        total_rows = Some(set.rows.len() as u64);
        if let Some((limit, offset)) = pagination.or(sql_page) {
            set.rows = set
                .rows
                .into_iter()
                .skip(offset as usize)
                .take(limit as usize)
                .collect();
        }
    } else if total_rows.is_none() && q.include.total_rows {
        total_rows = Some(set.rows.len() as u64);
    }

    // Lueckenlose Zeitreihe
    let time_only = plan.dims.len() == 1 && plan.interval.is_some();
    let now_ts = now;
    let mut future_index: Option<usize> = None;
    if time_only && q.include.zero_fill {
        let map = set.map();
        set.rows = (0..labels.len())
            .map(|i| {
                let key = vec![i.to_string()];
                let vals = map.get(&key).cloned().unwrap_or_default();
                (key, vals)
            })
            .collect();
        if let Some(b) = &bounds {
            future_index = b.iter().position(|start| *start > now_ts);
        }
    }

    // Vergleich
    let mut cmp_labels: Option<Vec<String>> = None;
    let mut cmp_range_out = None;
    let mut cmp_set: Option<HashMap<Key, Metrics>> = None;
    if let Some(cmp) = &q.include.comparisons {
        let cmp_range = time::comparison(&range, &q.date_range, cmp, site.tz, now, plan.interval.is_none())?;
        let cmp_bounds = plan.interval.map(|iv| time::buckets(&cmp_range, iv, site.tz));
        let cctx = Ctx {
            site,
            range: &cmp_range,
            bounds: cmp_bounds.as_ref().map(|(b, _)| b.as_slice()),
        };
        let keys: Vec<Key> = set.rows.iter().map(|(k, _)| k.clone()).collect();
        let restrict = if plan.dims.is_empty() || plan.interval.is_some() {
            None
        } else {
            Some(keys.as_slice())
        };
        let cs = compute(c, &plan, &cctx, None, restrict, false)?;
        cmp_set = Some(if plan.dims.is_empty() {
            // Aggregat: die eine Zeile unter dem leeren Schluessel.
            cs.rows
                .into_iter()
                .take(1)
                .map(|(_, v)| (Vec::new(), v))
                .collect()
        } else {
            cs.map()
        });
        cmp_labels = cmp_bounds.map(|(_, l)| l);
        cmp_range_out = Some(cmp_range.labels());
    }

    // Regionsnamen nachschlagen
    let region_names: HashMap<String, String> = if plan.dims.contains(&Dim::RegionName) {
        let mut stmt = c.prepare_cached("SELECT code, name FROM geo_names")?;
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<_, _>>()?
    } else {
        HashMap::new()
    };
    let label_dim = |dim: &Dim, raw: &str, labels: &[String]| -> String {
        match dim {
            Dim::Time(_) => raw
                .parse::<usize>()
                .ok()
                .and_then(|i| labels.get(i))
                .cloned()
                .unwrap_or_default(),
            Dim::Visit("source") if raw.is_empty() => DIRECT_SOURCE.to_string(),
            Dim::CountryName => nrzanalytics_shared::geo::country_name(raw)
                .map(str::to_string)
                .unwrap_or_else(|| raw.to_string()),
            Dim::RegionName => region_names.get(raw).cloned().unwrap_or_else(|| raw.to_string()),
            _ if raw.is_empty() => NONE_VALUE.to_string(),
            _ => raw.to_string(),
        }
    };

    let mut results = Vec::with_capacity(set.rows.len());
    for (i, (key, vals)) in set.rows.iter().enumerate() {
        let future = future_index.is_some_and(|f| i >= f);
        let metrics: Vec<serde_json::Value> = q
            .metrics
            .iter()
            .map(|x| metric_value(x, Some(vals), future))
            .collect();
        let dimensions: Vec<String> = plan
            .dims
            .iter()
            .zip(key.iter())
            .map(|(dim, raw)| label_dim(dim, raw, &labels))
            .collect();
        let comparison = cmp_set.as_ref().map(|cs| {
            let cvals = cs.get(key);
            let in_range = cmp_labels.as_ref().is_none_or(|l| {
                key.first()
                    .and_then(|k| k.parse::<usize>().ok())
                    .is_none_or(|i| !time_only || i < l.len())
            });
            let cmetrics: Vec<serde_json::Value> = q
                .metrics
                .iter()
                .map(|x| {
                    if in_range {
                        metric_value(x, cvals, false)
                    } else {
                        serde_json::Value::Null
                    }
                })
                .collect();
            let change: Vec<serde_json::Value> = q
                .metrics
                .iter()
                .zip(metrics.iter().zip(cmetrics.iter()))
                .map(|(x, (cur, prev))| {
                    if future {
                        serde_json::Value::Null
                    } else {
                        change(x, cur, prev)
                    }
                })
                .collect();
            let cdims = plan
                .dims
                .iter()
                .zip(key.iter())
                .map(|(dim, raw)| label_dim(dim, raw, cmp_labels.as_deref().unwrap_or(&labels)))
                .collect();
            RowComparison {
                dimensions: cdims,
                metrics: cmetrics,
                change,
            }
        });
        results.push(ResultRow {
            metrics,
            dimensions,
            comparison,
        });
    }
    // Aggregat ohne Daten: trotzdem eine Zeile mit Nullen.
    if plan.dims.is_empty() && results.is_empty() {
        let metrics: Vec<serde_json::Value> =
            q.metrics.iter().map(|x| metric_value(x, None, false)).collect();
        results.push(ResultRow {
            metrics,
            dimensions: Vec::new(),
            comparison: None,
        });
    }

    let mut echo = serde_json::to_value(q).unwrap_or_default();
    if let Some(obj) = echo.as_object_mut() {
        let (from, to) = range.labels();
        obj.insert("site_id".into(), json!(site.domain));
        obj.insert("date_range".into(), json!([from, to]));
    }
    let meta = QueryMeta {
        time_labels: (q.include.time_labels && plan.interval.is_some()).then(|| labels.clone()),
        comparison_time_labels: if q.include.time_labels {
            cmp_labels.clone()
        } else {
            None
        },
        total_rows: if q.include.total_rows { total_rows } else { None },
        date_range: Some(range.labels()),
        comparison_date_range: cmp_range_out,
        interval: plan.interval.map(|i| i.as_str().to_string()),
        warnings,
    };
    Ok(QueryResult {
        results,
        meta,
        query: echo,
    })
}

/// Aktuelle Besucher (letzte 5 Minuten).
pub fn current_visitors(c: &Connection, site_id: i64, now: i64) -> AppResult<u64> {
    let n: i64 = c.query_row(
        "SELECT COUNT(DISTINCT user_id) FROM events WHERE site_id = ?1 AND ts >= ?2",
        rusqlite::params![site_id, now - 300],
        |r| r.get(0),
    )?;
    Ok(n.max(0) as u64)
}

#[cfg(test)]
mod tests;
