//! Vorschlaege fuer Filterwerte (Autovervollstaendigung im Filter-Dialog).

use super::{SiteCtx, sql, time};
use crate::error::{AppError, AppResult};
use nrzanalytics_shared::{
    api::{Suggestion, SuggestionQuery},
    labels,
    query::{DIRECT_SOURCE, DateRange, Filter, dimensions as d},
};
use rusqlite::Connection;

const LIMIT: i64 = 25;

pub fn suggest(c: &Connection, site: &SiteCtx, q: &SuggestionQuery, now: i64) -> AppResult<Vec<Suggestion>> {
    let needle = q.q.trim().to_lowercase();
    // Ziele: aus den Einstellungen.
    if q.dimension == d::EVENT_GOAL {
        return Ok(site
            .goals
            .iter()
            .filter(|g| needle.is_empty() || g.display_name.to_lowercase().contains(&needle))
            .map(|g| Suggestion {
                value: g.display_name.clone(),
                label: g.display_name.clone(),
            })
            .collect());
    }
    let first_day = match &q.date_range {
        DateRange::Named(n) if n == "all" => site.first_day(c)?,
        _ => None,
    };
    let range = time::resolve(&q.date_range, q.date.as_deref(), site.tz, now, first_day)?;
    let mut p = sql::Params::default();
    let site_ref = p.add(site.id);
    let start = p.add(range.start);
    let end = p.add(range.end);
    // Filter auf dieselbe Dimension weglassen (sonst schlaegt man nur schon Gewaehltes vor).
    let filters: Vec<Filter> = q
        .filters
        .iter()
        .filter(|f| !f.dimensions().contains(&q.dimension.as_str()))
        .cloned()
        .collect();
    let env = sql::FilterEnv {
        goals: &site.goals,
        site: site_ref.clone(),
        start: start.clone(),
        end: end.clone(),
    };

    // Namen der Eigenschaften
    if q.dimension == "event:props" {
        let mut wh = vec![
            format!("e.site_id = {site_ref}"),
            format!("e.ts >= {start}"),
            format!("e.ts < {end}"),
            "e.props IS NOT NULL".to_string(),
        ];
        for f in &filters {
            wh.push(sql::filter_expr(f, sql::Table::Events, &env, &mut p)?);
        }
        let sql_text = format!(
            "SELECT j.key, COUNT(DISTINCT e.user_id) AS c FROM events e, json_each(e.props) j WHERE {} \
             GROUP BY j.key ORDER BY c DESC, j.key LIMIT 100",
            wh.join(" AND ")
        );
        let mut stmt = c.prepare(&sql_text)?;
        let keys = stmt
            .query_map(rusqlite::params_from_iter(p.values.iter()), |r| {
                r.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(keys
            .into_iter()
            .filter(|k| needle.is_empty() || k.to_lowercase().contains(&needle))
            .map(|k| Suggestion {
                value: k.clone(),
                label: k,
            })
            .collect());
    }

    let session_col = match q.dimension.as_str() {
        d::VISIT_ENTRY_PAGE => Some("s.entry_page"),
        d::VISIT_EXIT_PAGE => Some("s.exit_page"),
        d::VISIT_ENTRY_PAGE_HOSTNAME => Some("s.hostname"),
        d::VISIT_EXIT_PAGE_HOSTNAME => Some("s.exit_hostname"),
        _ => None,
    };
    let (table, expr) = if let Some(col) = session_col {
        (sql::Table::Sessions, col.to_string())
    } else if let Some(col) = sql::visit_column(&q.dimension) {
        (sql::Table::Events, format!("e.{col}"))
    } else {
        match q.dimension.as_str() {
            d::EVENT_PAGE => (sql::Table::Events, "e.pathname".to_string()),
            d::EVENT_HOSTNAME => (sql::Table::Events, "e.hostname".to_string()),
            d::EVENT_NAME => (sql::Table::Events, "e.name".to_string()),
            other => match d::prop_key(other) {
                Some(key) => (
                    sql::Table::Events,
                    format!(
                        "COALESCE(json_extract(e.props, {}), '')",
                        p.add(sql::prop_path(key)?)
                    ),
                ),
                None => return Err(AppError::invalid(format!("Keine Vorschläge für '{other}'."))),
            },
        }
    };
    let mut wh = match table {
        sql::Table::Events => vec![
            format!("e.site_id = {site_ref}"),
            format!("e.ts >= {start}"),
            format!("e.ts < {end}"),
            "e.name != 'engagement'".to_string(),
        ],
        sql::Table::Sessions => {
            vec![
                format!("s.site_id = {site_ref}"),
                format!("s.start >= {start}"),
                format!("s.start < {end}"),
            ]
        }
    };
    for f in &filters {
        wh.push(sql::filter_expr(f, table, &env, &mut p)?);
    }
    // Laender: Suche auch im deutschen Namen.
    if q.dimension == d::VISIT_COUNTRY && !needle.is_empty() {
        let codes: Vec<String> = nrzanalytics_shared::geo::COUNTRIES
            .iter()
            .filter(|(code, name)| name.to_lowercase().contains(&needle) || code.to_lowercase() == needle)
            .map(|(code, _)| p.add((*code).to_string()))
            .collect();
        if codes.is_empty() {
            return Ok(Vec::new());
        }
        wh.push(format!("{expr} IN ({})", codes.join(", ")));
    } else if !needle.is_empty() {
        wh.push(format!("instr(lower({expr}), {}) > 0", p.add(needle.clone())));
    }
    let count = if table == sql::Table::Events {
        "COUNT(DISTINCT e.user_id)"
    } else {
        "COUNT(DISTINCT s.user_id)"
    };
    let from = if table == sql::Table::Events {
        "events e"
    } else {
        "sessions s"
    };
    let sql_text = format!(
        "SELECT {expr} AS v, {count} AS c FROM {from} WHERE {} GROUP BY v ORDER BY c DESC, v LIMIT {LIMIT}",
        wh.join(" AND ")
    );
    let mut stmt = c.prepare(&sql_text)?;
    let values = stmt
        .query_map(rusqlite::params_from_iter(p.values.iter()), |r| {
            r.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for v in values {
        if v.is_empty() {
            if q.dimension == d::VISIT_SOURCE {
                out.push(Suggestion {
                    value: DIRECT_SOURCE.into(),
                    label: labels::source(DIRECT_SOURCE),
                });
            }
            continue;
        }
        let label = labels::dimension_value(&q.dimension, &v);
        out.push(Suggestion { value: v, label });
    }
    Ok(out)
}
