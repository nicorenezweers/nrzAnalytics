//! Trichter: wie viele Besucher erreichen die Ziele in dieser Reihenfolge (innerhalb eines Besuchs)?

use super::{SiteCtx, sql, time};
use crate::error::{AppError, AppResult};
use nrzanalytics_shared::{
    api::{Funnel, FunnelQuery, FunnelResult, FunnelStepResult},
    query::DateRange,
};
use rusqlite::{Connection, types::Value};
use std::collections::{HashMap, HashSet};

pub fn compute(
    c: &Connection,
    site: &SiteCtx,
    funnel: &Funnel,
    q: &FunnelQuery,
    now: i64,
) -> AppResult<FunnelResult> {
    if funnel.steps.len() < 2 {
        return Err(AppError::invalid(
            "Ein Trichter braucht mindestens zwei Schritte.",
        ));
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
    let env = sql::FilterEnv {
        goals: &site.goals,
        site: site_ref.clone(),
        start: start.clone(),
        end: end.clone(),
    };
    let mut wh = vec![
        format!("e.site_id = {site_ref}"),
        format!("e.ts >= {start}"),
        format!("e.ts < {end}"),
        "e.name != 'engagement'".to_string(),
    ];
    for f in &q.filters {
        wh.push(sql::filter_expr(f, sql::Table::Events, &env, &mut p)?);
    }
    let base = wh.join(" AND ");

    // Alle Besucher im Zeitraum (mit Filtern)
    let all_sql = format!("SELECT COUNT(DISTINCT e.user_id) FROM events e WHERE {base}");
    let all: i64 = c.query_row(&all_sql, rusqlite::params_from_iter(p.values.iter()), |r| {
        r.get(0)
    })?;

    // Ereignisse, die irgendeinen Schritt erfuellen: eine Zeile je (Ereignis, Schritt).
    let mut branches = Vec::new();
    for (i, goal) in funnel.steps.iter().enumerate() {
        let pred = sql::goal_predicate(goal, &mut p);
        branches.push(format!(
            "SELECT e.session_id AS sid, e.user_id AS uid, e.ts AS ts, e.id AS eid, {i} AS step FROM events e WHERE {base} AND {pred}"
        ));
    }
    let sql_text = format!(
        "SELECT sid, uid, step FROM ({}) ORDER BY sid, ts, eid, step",
        branches.join(" UNION ALL ")
    );
    let mut stmt = c.prepare(&sql_text)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(p.values.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Value>(2)?))
    })?;

    // Je Besuch: wie weit kam er der Reihe nach?
    let n = funnel.steps.len();
    let mut reached: Vec<HashSet<i64>> = vec![HashSet::new(); n];
    let mut progress: HashMap<i64, (i64, usize)> = HashMap::new();
    for row in rows {
        let (sid, uid, step) = row?;
        let step = match step {
            Value::Integer(s) => s as usize,
            _ => continue,
        };
        let entry = progress.entry(sid).or_insert((uid, 0));
        if entry.1 < n && step == entry.1 {
            reached[step].insert(uid);
            entry.1 += 1;
        }
    }
    let counts: Vec<u64> = reached.iter().map(|s| s.len() as u64).collect();
    let entering = counts.first().copied().unwrap_or(0);
    let all = all.max(0) as u64;
    let pct = |a: u64, b: u64| {
        if b > 0 {
            (a as f64 / b as f64 * 1000.0).round() / 10.0
        } else {
            0.0
        }
    };
    let mut steps = Vec::with_capacity(n);
    for (i, goal) in funnel.steps.iter().enumerate() {
        let visitors = counts[i];
        let prev = if i == 0 { entering } else { counts[i - 1] };
        let dropoff = prev.saturating_sub(visitors);
        steps.push(FunnelStepResult {
            label: goal.display_name.clone(),
            visitors,
            conversion_rate: pct(visitors, entering),
            conversion_rate_step: if i == 0 { 100.0 } else { pct(visitors, prev) },
            dropoff: if i == 0 { 0 } else { dropoff },
            dropoff_percentage: if i == 0 { 0.0 } else { pct(dropoff, prev) },
        });
    }
    Ok(FunnelResult {
        name: funnel.name.clone(),
        all_visitors: all,
        entering_visitors: entering,
        entering_visitors_percentage: pct(entering, all),
        never_entering_visitors: all.saturating_sub(entering),
        conversion_rate: pct(counts.last().copied().unwrap_or(0), entering),
        steps,
    })
}
