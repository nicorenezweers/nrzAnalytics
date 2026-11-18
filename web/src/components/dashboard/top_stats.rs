//! Kennzahlen-Leiste und Hauptdiagramm in einer Karte.

use super::{Dash, use_loader};
use crate::api;
use crate::components::charts::{LineChart, Series};
use crate::components::ui::dropdown::{Dropdown, DropdownAlign, DropdownItem};
use crate::components::ui::skeleton::Skeleton;
use crate::util;
use dioxus::prelude::*;
use nrzanalytics_shared::api::Realtime;
use nrzanalytics_shared::labels;
use nrzanalytics_shared::query::{QueryResult, dimensions as d, metrics as m};

/// Eine Kennzahl der Leiste.
#[derive(Clone, PartialEq, Debug)]
pub struct Stat {
    pub metric: &'static str,
    pub label: String,
    pub value: Option<f64>,
    pub comparison: Option<f64>,
    pub change: Option<f64>,
    /// Im Diagramm darstellbar.
    pub graphable: bool,
}

/// Kennzahlen je nach Modus.
fn stat_metrics(dash: &Dash) -> Vec<&'static str> {
    if dash.params.is_realtime() {
        vec![m::VISITORS, m::PAGEVIEWS]
    } else if dash.params.has_goal_filter() {
        let mut v = vec![m::VISITORS, m::EVENTS, m::CONVERSION_RATE];
        if goal_filter_has_revenue(dash) {
            v.extend([m::TOTAL_REVENUE, m::AVERAGE_REVENUE]);
        }
        v
    } else {
        vec![m::VISITORS, m::VISITS, m::PAGEVIEWS, m::VIEWS_PER_VISIT, m::BOUNCE_RATE, m::VISIT_DURATION]
    }
}

/// Alle gefilterten Ziele haben eine Waehrung (Umsatz-Kennzahlen moeglich).
fn goal_filter_has_revenue(dash: &Dash) -> bool {
    use nrzanalytics_shared::query::{Filter, FilterOp};
    let Some(Filter::Condition { op: FilterOp::Is, clauses, .. }) = dash.params.filter_for(d::EVENT_GOAL) else {
        return false;
    };
    !clauses.is_empty()
        && clauses.iter().all(|c| dash.site.goals.iter().any(|g| &g.display_name == c && g.currency.is_some()))
}

fn stat_label(dash: &Dash, metric: &str) -> String {
    if dash.params.is_realtime() {
        return match metric {
            m::VISITORS => "Eindeutige Besucher (30 Min.)".into(),
            m::PAGEVIEWS => "Seitenaufrufe (30 Min.)".into(),
            other => labels::metric_long(other).into(),
        };
    }
    if dash.params.has_goal_filter() {
        return match metric {
            m::VISITORS => "Eindeutige Konversionen".into(),
            m::EVENTS => "Konversionen insgesamt".into(),
            m::TOTAL_REVENUE => "Umsatz insgesamt".into(),
            m::AVERAGE_REVENUE => "Ø Umsatz".into(),
            other => labels::metric_long(other).into(),
        };
    }
    // Kurz, damit sechs Kacheln nebeneinander passen; die lange Form steht im Tooltip.
    labels::metric(metric).into()
}

/// Bei Absprungrate ist weniger besser.
fn lower_is_better(metric: &str) -> bool {
    metric == m::BOUNCE_RATE || metric == m::EXIT_RATE
}

#[component]
pub fn GraphCard(dash: Dash) -> Element {
    let metrics = stat_metrics(&dash);
    let q = dash.query(&metrics).compare(dash.params.comparison());
    let stats = use_loader((dash.domain.clone(), q, dash.tick), |(domain, q, _)| async move { api::query(&domain, &q).await });
    let realtime = dash.params.is_realtime();
    let rt = use_loader((dash.domain.clone(), dash.tick, realtime), |(domain, _, realtime): (String, u32, bool)| async move {
        if !realtime {
            return Ok(Realtime { visitors: 0 });
        }
        api::get::<Realtime>(&format!("/api/stats/{}/realtime", api::enc(&domain))).await
    });

    let mut list: Vec<Stat> = Vec::new();
    let loading = *stats.loading.read();
    let data = stats.data.read();
    let error = match &*data {
        Some(Err(e)) => Some(e.message.clone()),
        _ => None,
    };
    if let Some(Ok(res)) = &*data {
        let row = res.results.first().cloned().unwrap_or_default();
        if realtime {
            let current = match &*rt.data.read() {
                Some(Ok(r)) => Some(r.visitors as f64),
                _ => None,
            };
            list.push(Stat { metric: "current", label: "Aktuelle Besucher".into(), value: current, comparison: None, change: None, graphable: false });
        }
        for (i, metric) in metrics.iter().enumerate() {
            list.push(Stat {
                metric,
                label: stat_label(&dash, metric),
                value: if res.results.is_empty() { Some(0.0) } else { row.metric_opt(i) },
                comparison: row.comparison.as_ref().map(|c| c.metric(i)),
                change: row.comparison.as_ref().and_then(|c| c.change(i)),
                graphable: true,
            });
        }
    }
    drop(data);

    let selected = {
        let wanted = dash.params.metric.as_str();
        if metrics.contains(&wanted) { wanted.to_string() } else { metrics.first().map(|s| s.to_string()).unwrap_or_else(|| m::VISITORS.into()) }
    };
    let has_cmp = dash.params.comparison().is_some();
    let nav = navigator();

    rsx! {
        section { class: "rounded-xl border bg-card text-card-foreground shadow-sm",
            // Kennzahlen-Leiste: mobil als Raster (2 bzw. 3 Spalten), ab lg in einer Reihe.
            div { class: "grid grid-cols-2 border-b sm:grid-cols-3 lg:flex",
                if list.is_empty() && error.is_none() {
                    for i in 0..metrics.len() {
                        div { key: "{i}", class: "min-w-0 flex-1 px-4 py-3 sm:px-5 sm:py-4 lg:min-w-[9.5rem]",
                            Skeleton { class: "h-3 w-24" }
                            Skeleton { class: "mt-3 h-7 w-16" }
                        }
                    }
                }
                if let Some(e) = &error {
                    div { class: "px-5 py-4 text-sm text-destructive", "{e}" }
                }
                for s in list.iter() {
                    StatTile {
                        key: "{s.metric}",
                        stat: s.clone(),
                        active: s.metric == selected,
                        show_cmp: has_cmp,
                        dimmed: loading,
                        onselect: {
                            let dash = dash.clone();
                            let metric = s.metric;
                            move |_| {
                                let mut p = dash.params.clone();
                                p.metric = if metric == m::VISITORS { String::new() } else { metric.to_string() };
                                nav.push(dash.route(p));
                            }
                        },
                    }
                }
            }
            MainGraph { dash: dash.clone(), metric: selected }
        }
    }
}

#[component]
fn StatTile(stat: Stat, active: bool, show_cmp: bool, dimmed: bool, onselect: EventHandler<()>) -> Element {
    let value = if stat.metric == "current" {
        stat.value.map(labels::number_full).unwrap_or_else(|| "–".into())
    } else {
        labels::format_metric(stat.metric, stat.value)
    };
    let change = stat.change.map(|c| {
        let good = if lower_is_better(stat.metric) { c < 0.0 } else { c > 0.0 };
        let rate = nrzanalytics_shared::query::metrics::is_rate(stat.metric);
        let text = if rate {
            format!("{} Pp.", labels::number_short(c.abs()))
        } else {
            format!("{} %", labels::number_short(c.abs().round()))
        };
        (c, good, text)
    });
    let cls = if active {
        "relative min-w-0 flex-1 cursor-pointer px-4 py-3 text-left outline-none transition-colors focus-visible:bg-accent/60 after:absolute after:inset-x-4 after:bottom-0 after:h-0.5 after:rounded-full after:bg-chart-1 sm:px-5 sm:py-4 sm:after:inset-x-5 lg:min-w-[9.5rem]"
    } else {
        "relative min-w-0 flex-1 cursor-pointer px-4 py-3 text-left outline-none transition-colors hover:bg-accent/40 focus-visible:bg-accent/60 sm:px-5 sm:py-4 lg:min-w-[9.5rem]"
    };
    let graphable = stat.graphable;
    rsx! {
        button {
            r#type: "button",
            class: "{cls}",
            disabled: !graphable,
            "aria-pressed": "{active}",
            title: if stat.label == labels::metric(stat.metric) { labels::metric_long(stat.metric).to_string() } else { stat.label.clone() },
            onclick: move |_| if graphable { onselect.call(()) },
            div { class: if active { "truncate text-xs font-semibold uppercase tracking-wide text-chart-1" } else { "truncate text-xs font-semibold uppercase tracking-wide text-muted-foreground" },
                if stat.metric == "current" {
                    span { class: "mr-1.5 inline-block size-2 rounded-full bg-live animate-live" }
                }
                "{stat.label}"
            }
            div { class: if dimmed { "mt-1.5 flex items-baseline gap-2 opacity-60 transition-opacity" } else { "mt-1.5 flex items-baseline gap-2 transition-opacity" },
                span { class: "text-2xl font-semibold tabular-nums tracking-tight", "{value}" }
                if let Some((c, good, text)) = &change {
                    span { class: if *c == 0.0 { "flex items-center text-xs font-medium text-muted-foreground" } else if *good { "flex items-center text-xs font-medium text-positive" } else { "flex items-center text-xs font-medium text-negative" },
                        if *c > 0.0 {
                            icons::ArrowUp { class: "size-3" }
                        } else if *c < 0.0 {
                            icons::ArrowDown { class: "size-3" }
                        }
                        "{text}"
                    }
                }
            }
            if show_cmp && stat.metric != "current" {
                div { class: "mt-0.5 text-xs text-muted-foreground tabular-nums",
                    "vorher "
                    {labels::format_metric(stat.metric, stat.comparison)}
                }
            }
        }
    }
}

/// Sinnvolle Intervalle fuer den Zeitraum (Server-Regeln: Minute ≤ 6 h, Stunde ≤ 31 Tage).
fn intervals_for(dash: &Dash, res: Option<&QueryResult>) -> Vec<&'static str> {
    let p = &dash.params;
    match p.period_key() {
        "realtime" => vec!["minute"],
        "day" => vec!["hour"],
        "6mo" | "12mo" | "year" => vec!["day", "week", "month"],
        "all" => vec!["day", "week", "month"],
        _ => {
            let days = res
                .and_then(|r| r.meta.date_range.as_ref())
                .and_then(|(a, b)| util::days_between(a, b))
                .map(|n| n + 1)
                .unwrap_or(30);
            if days <= 31 { vec!["hour", "day", "week"] } else { vec!["day", "week", "month"] }
        }
    }
}

#[component]
fn MainGraph(dash: Dash, metric: String) -> Element {
    let nav = navigator();
    let metric_label = stat_label(&dash, &metric);
    let now = util::now_in(&dash.site.timezone);
    let realtime = dash.params.is_realtime();
    let interval = dash.params.interval.clone();
    let time_dim = if realtime {
        d::TIME_MINUTE.to_string()
    } else if matches!(interval.as_str(), "minute" | "hour" | "day" | "week" | "month") {
        format!("time:{interval}")
    } else {
        d::TIME.to_string()
    };
    let q = dash.query(&[metric.as_str()]).dimension(time_dim).compare(dash.params.comparison()).zero_fill();
    let res = use_loader((dash.domain.clone(), q, dash.tick), |(domain, q, _)| async move { api::query(&domain, &q).await });
    let loading = *res.loading.read();
    let data = res.data.read();
    let body = match &*data {
        None => rsx! { Skeleton { class: "h-[260px] w-full" } },
        Some(Err(e)) => rsx! { div { class: "flex h-[260px] items-center justify-center text-sm text-destructive", "{e.message}" } },
        Some(Ok(r)) => {
            let series = build_series(r, &metric);
            let used_interval = r.meta.interval.clone().unwrap_or_else(|| if realtime { "minute".into() } else { "day".into() });
            let dash2 = dash.clone();
            let iv = used_interval.clone();
            rsx! {
                div { class: if loading { "opacity-60 transition-opacity" } else { "transition-opacity" },
                    LineChart {
                        series,
                        metric: metric.clone(),
                        label: metric_label.clone(),
                        now: now.clone(),
                        interval: used_interval.clone(),
                        on_click: move |label: String| {
                            // Klick auf einen Tag/Monat zoomt hinein.
                            let date = label.get(..10).unwrap_or("").to_string();
                            if util::parse_date(&date).is_none() {
                                return;
                            }
                            let p = match iv.as_str() {
                                "day" | "week" => dash2.params.with_period("day", Some(date)),
                                "month" => dash2.params.with_period("month", Some(date)),
                                _ => return,
                            };
                            nav.push(dash2.route(p));
                        },
                    }
                }
            }
        }
    };
    let options = intervals_for(&dash, match &*data { Some(Ok(r)) => Some(r), _ => None });
    let current_interval = match &*data {
        Some(Ok(r)) => r.meta.interval.clone().unwrap_or_default(),
        _ => interval.clone(),
    };
    drop(data);
    rsx! {
        div { class: "px-3 pb-4 pt-3 sm:px-5",
            div { class: "mb-2 flex items-center justify-between gap-2",
                div { class: "flex items-center gap-3 text-xs text-muted-foreground",
                    span { class: "flex items-center gap-1.5",
                        span { class: "inline-block h-0.5 w-4 rounded bg-chart-1" }
                        "{metric_label}"
                    }
                    if dash.params.comparison().is_some() {
                        span { class: "flex items-center gap-1.5",
                            span { class: "inline-block w-4 border-t-2 border-dashed border-chart-muted" }
                            "Vergleich"
                        }
                    }
                }
                if options.len() > 1 {
                    Dropdown {
                        align: DropdownAlign::End,
                        aria_label: "Intervall wählen",
                        trigger_class: "h-7 px-2 text-xs text-muted-foreground hover:bg-accent hover:text-foreground",
                        menu_class: "min-w-36",
                        trigger: rsx! {
                            {labels::interval(&current_interval)}
                            icons::ChevronDown { class: "size-3.5" }
                        },
                        for o in options.iter() {
                            DropdownItem {
                                key: "{o}",
                                active: *o == current_interval,
                                onclick: {
                                    let dash = dash.clone();
                                    let o = o.to_string();
                                    move |_| {
                                        let mut p = dash.params.clone();
                                        p.interval = o.clone();
                                        nav.push(dash.route(p));
                                    }
                                },
                                {labels::interval(o)}
                            }
                        }
                    }
                }
            }
            {body}
        }
    }
}

fn build_series(r: &QueryResult, metric: &str) -> Series {
    let labels_main = r.meta.time_labels.clone().unwrap_or_else(|| r.results.iter().map(|row| row.dimension(0).to_string()).collect());
    let cmp_labels = r.meta.comparison_time_labels.clone().unwrap_or_default();
    // Werte nach Zeitstempel zuordnen (zero_fill liefert ohnehin alle Zeilen in Reihenfolge).
    let mut values = vec![0.0; labels_main.len()];
    let mut comparison: Vec<Option<f64>> = vec![None; labels_main.len()];
    let has_cmp = r.results.iter().any(|row| row.comparison.is_some());
    for (i, row) in r.results.iter().enumerate() {
        let idx = labels_main.iter().position(|l| l == row.dimension(0)).unwrap_or(i);
        if let Some(v) = values.get_mut(idx) {
            *v = row.metric(0);
        }
        if let (Some(c), Some(slot)) = (&row.comparison, comparison.get_mut(idx)) {
            *slot = Some(c.metric(0));
        }
    }
    let changes = r
        .results
        .iter()
        .map(|row| row.comparison.as_ref().and_then(|c| c.change(0)))
        .collect::<Vec<_>>();
    Series {
        labels: labels_main,
        values,
        comparison: if has_cmp { Some(comparison) } else { None },
        comparison_labels: cmp_labels,
        changes,
        metric: metric.to_string(),
    }
}
