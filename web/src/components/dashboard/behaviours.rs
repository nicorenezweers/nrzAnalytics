//! Karte „Verhalten“: Ziel-Konversionen, Eigenschaften und Trichter.

use super::reports::{Report, ReportCard};
use super::{Dash, use_loader};
use crate::api;
use crate::components::ui::select::Select;
use crate::components::ui::skeleton::Skeleton;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{FunnelQuery, FunnelResult};
use nrzanalytics_shared::labels;
use nrzanalytics_shared::query::{dimensions as d, metrics as m};

#[component]
pub fn BehavioursCard(dash: Dash) -> Element {
    let site = &dash.site;
    let storage_key = "nrz_tab_behaviours";
    let mut tab = use_signal(|| crate::util::storage_get(storage_key).unwrap_or_default());
    let mut tabs: Vec<(&str, &str)> = Vec::new();
    if !site.goals.is_empty() {
        tabs.push(("goals", "Ziele"));
    }
    if !site.props.is_empty() {
        tabs.push(("props", "Eigenschaften"));
    }
    if !site.funnels.is_empty() {
        tabs.push(("funnels", "Trichter"));
    }
    let active = tabs.iter().find(|(k, _)| *k == tab()).or(tabs.first()).map(|(k, _)| k.to_string()).unwrap_or_default();
    let switcher = rsx! {
        if tabs.len() > 1 {
            div { class: "mr-2 flex items-center gap-0.5 rounded-lg bg-muted p-0.5",
                for (k, l) in tabs.iter() {
                    button {
                        key: "{k}",
                        r#type: "button",
                        class: if *k == active { "rounded-md bg-background px-2 py-0.5 text-xs font-semibold shadow-sm dark:bg-input/40" } else { "rounded-md px-2 py-0.5 text-xs font-medium text-muted-foreground hover:text-foreground" },
                        onclick: {
                            let k = k.to_string();
                            move |_| {
                                crate::util::storage_set(storage_key, &k);
                                tab.set(k.clone());
                            }
                        },
                        "{l}"
                    }
                }
            }
        }
    };
    match active.as_str() {
        "goals" => {
            let mut r = Report::new("goals", "Ziele", d::EVENT_GOAL);
            r.title = "Ziel-Konversionen".into();
            r.column = "Ziel".into();
            r.metric_label = "Besucher";
            r.goal_bar = true;
            r.detail_metrics = if dash.has_revenue() {
                vec![m::VISITORS, m::EVENTS, m::CONVERSION_RATE, m::TOTAL_REVENUE, m::AVERAGE_REVENUE]
            } else {
                vec![m::VISITORS, m::EVENTS, m::CONVERSION_RATE]
            };
            rsx! { GoalsCard { dash, report: r, switcher } }
        }
        "props" => rsx! { PropsCard { dash, switcher } },
        _ => rsx! { FunnelCard { dash, switcher } },
    }
}

#[component]
fn GoalsCard(dash: Dash, report: Report, switcher: Element) -> Element {
    rsx! {
        ReportCard { dash, title: "Ziel-Konversionen", card: "goals", reports: vec![report], header_extra: switcher }
    }
}

#[component]
fn PropsCard(dash: Dash, switcher: Element) -> Element {
    let nav = navigator();
    let props = dash.site.props.clone();
    let chosen = if props.contains(&dash.params.prop) { dash.params.prop.clone() } else { props.first().cloned().unwrap_or_default() };
    let dim = format!("{}{chosen}", d::EVENT_PROPS_PREFIX);
    let mut r = Report::new("props", "Eigenschaften", &dim);
    r.title = format!("Eigenschaft {chosen}");
    r.column = chosen.clone();
    r.goal_bar = true;
    r.detail_metrics = vec![m::VISITORS, m::EVENTS, m::PERCENTAGE];
    let options: Vec<(String, String)> = props.iter().map(|p| (p.clone(), p.clone())).collect();
    let select = rsx! {
        {switcher}
        div { class: "w-40",
            Select {
                value: chosen.clone(),
                options,
                aria_label: "Eigenschaft wählen",
                class: "h-7 text-xs",
                on_change: {
                    let dash = dash.clone();
                    move |v: String| {
                        let mut p = dash.params.clone();
                        p.prop = v;
                        nav.push(dash.route(p));
                    }
                },
            }
        }
    };
    rsx! {
        ReportCard { key: "{chosen}", dash, title: "Eigenschaften", card: "props", reports: vec![r], header_extra: select }
    }
}

#[component]
fn FunnelCard(dash: Dash, switcher: Element) -> Element {
    let nav = navigator();
    let funnels = dash.site.funnels.clone();
    let chosen = funnels
        .iter()
        .find(|f| f.id.to_string() == dash.params.funnel)
        .or(funnels.first())
        .map(|f| f.id)
        .unwrap_or_default();
    let fq = FunnelQuery {
        funnel_id: chosen,
        date_range: dash.params.date_range(),
        date: dash.params.date_opt(),
        filters: dash.params.filters.clone(),
    };
    let res = use_loader((dash.domain.clone(), fq, dash.tick), |(domain, fq, _): (String, FunnelQuery, u32)| async move {
        api::post::<_, FunnelResult>(&format!("/api/stats/{}/funnel", api::enc(&domain)), &fq).await
    });
    let options: Vec<(String, String)> = funnels.iter().map(|f| (f.id.to_string(), f.name.clone())).collect();
    let loading = *res.loading.read();
    let data = res.data.read();
    let body = match &*data {
        None => rsx! {
            div { class: "grid gap-3 px-5",
                for i in 0..4 {
                    Skeleton { key: "{i}", class: "h-10 w-full" }
                }
            }
        },
        Some(Err(e)) => rsx! { div { class: "px-5 py-10 text-center text-sm text-destructive", "{e.message}" } },
        Some(Ok(f)) => {
            let first = f.steps.first().map(|s| s.visitors).unwrap_or(0).max(1) as f64;
            rsx! {
                div { class: if loading { "px-5 opacity-60 transition-opacity" } else { "px-5 transition-opacity" },
                    div { class: "mb-4 flex flex-wrap gap-x-6 gap-y-1 text-sm text-muted-foreground",
                        span {
                            span { class: "font-semibold text-foreground", {labels::format_metric(m::CONVERSION_RATE, Some(f.conversion_rate))} }
                            " Konversion insgesamt"
                        }
                        span {
                            span { class: "font-semibold text-foreground", {labels::number_full(f.entering_visitors as f64)} }
                            " von "
                            {labels::number_full(f.all_visitors as f64)}
                            " Besuchern betraten den Trichter ("
                            {labels::format_metric(m::PERCENTAGE, Some(f.entering_visitors_percentage))}
                            ")"
                        }
                    }
                    ol { class: "grid gap-3",
                        for (i, s) in f.steps.iter().enumerate() {
                            li { key: "{i}", class: "grid gap-1",
                                div { class: "flex items-baseline justify-between gap-3 text-sm",
                                    span { class: "flex min-w-0 items-center gap-2",
                                        span { class: "flex size-5 shrink-0 items-center justify-center rounded-full bg-secondary text-[11px] font-semibold", "{i + 1}" }
                                        span { class: "truncate font-medium", "{s.label}" }
                                    }
                                    span { class: "shrink-0 tabular-nums",
                                        span { class: "font-semibold", {labels::number_full(s.visitors as f64)} }
                                        span { class: "ml-2 text-muted-foreground", {labels::format_metric(m::CONVERSION_RATE, Some(s.conversion_rate))} }
                                    }
                                }
                                div { class: "h-6 w-full overflow-hidden rounded-md bg-muted/60",
                                    div { class: "h-full rounded-md bg-chart-1/70 transition-[width] duration-300", style: "width: {(s.visitors as f64 / first * 100.0).clamp(0.0, 100.0)}%" }
                                }
                                if i > 0 {
                                    div { class: "flex items-center gap-1 text-xs text-muted-foreground",
                                        icons::CornerDownRight { class: "size-3" }
                                        {format!("{} vom vorherigen Schritt", labels::format_metric(m::CONVERSION_RATE, Some(s.conversion_rate_step)))}
                                        if s.dropoff > 0 {
                                            span { class: "text-negative",
                                                {format!(" · {} abgesprungen ({})", labels::number_full(s.dropoff as f64), labels::format_metric(m::PERCENTAGE, Some(s.dropoff_percentage)))}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    drop(data);
    rsx! {
        section { class: "flex min-h-[20rem] flex-col rounded-xl border bg-card pb-5 text-card-foreground shadow-sm",
            div { class: "flex flex-wrap items-center justify-between gap-3 px-5 pt-4 pb-4",
                h2 { class: "font-semibold", "Trichter" }
                div { class: "flex items-center",
                    {switcher}
                    div { class: "w-48",
                        Select {
                            value: chosen.to_string(),
                            options,
                            aria_label: "Trichter wählen",
                            class: "h-7 text-xs",
                            on_change: {
                                let dash = dash.clone();
                                move |v: String| {
                                    let mut p = dash.params.clone();
                                    p.funnel = v;
                                    nav.push(dash.route(p));
                                }
                            },
                        }
                    }
                }
            }
            {body}
        }
    }
}
