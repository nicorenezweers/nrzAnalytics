//! Detail-Modal eines Berichts: volle Tabelle mit Suche, sortierbaren Spalten und „Mehr laden“.

use super::Dash;
use super::reports::{Report, row_label, row_link};
use crate::api;
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::modal::{Modal, ModalSize};
use crate::components::ui::skeleton::Skeleton;
use dioxus::prelude::*;
use nrzanalytics_shared::labels;
use nrzanalytics_shared::query::{Filter, FilterOp, ResultRow, SortDirection};

const PAGE: u32 = 100;

#[component]
pub fn DetailsModal(dash: Dash, report: Report, open: bool, on_close: EventHandler<()>) -> Element {
    rsx! {
        Modal {
            open,
            on_close: move |_| on_close.call(()),
            title: report.title.clone(),
            size: ModalSize::Xl,
            class: "sm:h-[85dvh]",
            if open {
                DetailsBody { dash: dash.clone(), report: report.clone(), on_close }
            }
        }
    }
}

#[component]
fn DetailsBody(dash: Dash, report: Report, on_close: EventHandler<()>) -> Element {
    let nav = navigator();
    let metrics = report.modal_metrics(&dash);
    let mut input = use_signal(String::new);
    let mut search = use_signal(String::new);
    let mut sort = use_signal(|| (metrics.first().copied().unwrap_or("visitors").to_string(), SortDirection::Desc));
    let mut rows = use_signal(Vec::<ResultRow>::new);
    let mut offset = use_signal(|| 0u32);
    let mut has_more = use_signal(|| false);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);
    let mut generation = use_signal(|| 0u32);
    let mut debounce = use_signal(|| 0u32);

    // Suche entprellen (300 ms).
    let on_input = move |e: FormEvent| {
        input.set(e.value());
        let g = debounce.peek().wrapping_add(1);
        debounce.set(g);
        spawn(async move {
            gloo_timers::future::TimeoutFuture::new(300).await;
            if *debounce.peek() == g {
                search.set(input.peek().trim().to_string());
            }
        });
    };

    let build = {
        let dash = dash.clone();
        let report = report.clone();
        let metrics = metrics.clone();
        move |off: u32| {
            let mut q = dash.query(&metrics).limit(PAGE, off);
            q.dimensions = report.dimensions.clone();
            let s = search.peek().clone();
            if !s.is_empty() {
                q.filters.push(Filter::Condition { op: FilterOp::Contains, dimension: report.filter_dim.clone(), clauses: vec![s], case_sensitive: false });
            }
            let (by, dir) = sort.peek().clone();
            q.order(&by, dir)
        }
    };

    let load = {
        let build = build.clone();
        let domain = dash.domain.clone();
        move |off: u32| {
            let q = build(off);
            let domain = domain.clone();
            let g = generation.peek().wrapping_add(1);
            generation.set(g);
            loading.set(true);
            spawn(async move {
                let r = api::query(&domain, &q).await;
                if *generation.peek() != g {
                    return;
                }
                match r {
                    Ok(res) => {
                        let n = res.results.len() as u32;
                        if off == 0 {
                            rows.set(res.results);
                        } else {
                            rows.write().extend(res.results);
                        }
                        offset.set(off);
                        has_more.set(n >= PAGE);
                        error.set(None);
                    }
                    Err(e) => error.set(Some(e.message)),
                }
                loading.set(false);
            });
        }
    };

    {
        let mut load = load.clone();
        use_effect(move || {
            let _ = search();
            let _ = sort();
            load(0);
        });
    }

    let (sort_by, sort_dir) = sort();
    let list = rows.read().clone();
    let max = list.iter().map(|r| r.metric(0)).fold(0.0, f64::max).max(1.0);
    let bar = if report.goal_bar || dash.params.has_goal_filter() { "bg-bar-goal" } else { "bg-bar" };

    rsx! {
        div { class: "flex h-full flex-col gap-3",
            div { class: "relative",
                icons::Search { class: "pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" }
                Input {
                    r#type: InputType::Search,
                    class: "pl-9",
                    placeholder: "Suchen in {report.column} …",
                    value: input(),
                    autofocus: true,
                    aria_label: "Suchen",
                    oninput: on_input,
                }
            }
            if let Some(e) = error() {
                div { class: "rounded-md border border-destructive/40 px-3 py-2 text-sm text-destructive", "{e}" }
            }
            div { class: "min-h-0 flex-1 overflow-auto rounded-lg border",
                table { class: "w-full text-sm",
                    thead { class: "sticky top-0 z-10 bg-popover",
                        tr { class: "border-b",
                            th { class: "w-full min-w-40 px-3 py-2 text-left text-xs font-semibold uppercase tracking-wide text-muted-foreground", "{report.column}" }
                            for metric in metrics.iter() {
                                th { key: "{metric}", class: "px-3 py-2 text-right",
                                    button {
                                        r#type: "button",
                                        class: if sort_by == *metric { "inline-flex items-center gap-1 whitespace-nowrap text-xs font-semibold uppercase tracking-wide text-foreground outline-none focus-visible:underline" } else { "inline-flex items-center gap-1 whitespace-nowrap text-xs font-semibold uppercase tracking-wide text-muted-foreground hover:text-foreground outline-none focus-visible:underline" },
                                        "aria-label": "Sortieren nach {labels::metric(metric)}",
                                        onclick: {
                                            let metric = metric.to_string();
                                            move |_| {
                                                let (by, dir) = sort.peek().clone();
                                                let next = if by == metric {
                                                    if dir == SortDirection::Desc { SortDirection::Asc } else { SortDirection::Desc }
                                                } else {
                                                    SortDirection::Desc
                                                };
                                                sort.set((metric.clone(), next));
                                            }
                                        },
                                        {column_label(&report, metric)}
                                        if sort_by == *metric {
                                            if sort_dir == SortDirection::Desc {
                                                icons::ArrowDown { class: "size-3" }
                                            } else {
                                                icons::ArrowUp { class: "size-3" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    tbody {
                        if list.is_empty() && loading() {
                            for i in 0..8 {
                                tr { key: "s{i}", class: "border-b last:border-0",
                                    td { class: "px-3 py-2", colspan: "{metrics.len() + 1}", Skeleton { class: "h-5 w-full" } }
                                }
                            }
                        } else if list.is_empty() {
                            tr {
                                td { class: "px-3 py-10 text-center text-muted-foreground", colspan: "{metrics.len() + 1}", "Keine Einträge gefunden." }
                            }
                        }
                        for (i, row) in list.iter().enumerate() {
                            {
                                let (label, flag) = row_label(&report, row);
                                let value = row.dimension(0).to_string();
                                let link = row_link(&report, &dash, &value);
                                let width = (row.metric(0) / max * 100.0).clamp(0.0, 100.0);
                                let target = dash.route(dash.params.with_filter_is(&report.filter_dim, &value));
                                rsx! {
                                    tr { key: "{i}-{value}", class: "group border-b last:border-0 hover:bg-accent/30",
                                        td { class: "relative w-full min-w-40 max-w-0 px-3 py-1.5",
                                            div { class: "absolute inset-y-1 left-1 rounded {bar} opacity-70", style: "width: calc({width}% - 0.5rem)" }
                                            div { class: "relative flex min-w-0 items-center gap-2",
                                                button {
                                                    r#type: "button",
                                                    class: "flex min-w-0 items-center gap-2 text-left outline-none hover:underline focus-visible:underline",
                                                    onclick: move |_| {
                                                        nav.push(target.clone());
                                                        on_close.call(());
                                                    },
                                                    if let Some(f) = &flag {
                                                        span { class: "shrink-0", "{f}" }
                                                    }
                                                    span { class: "truncate", title: "{label}", "{label}" }
                                                }
                                                if let Some(href) = &link {
                                                    a {
                                                        href: "{href}",
                                                        target: "_blank",
                                                        rel: "noreferrer noopener",
                                                        class: "shrink-0 text-muted-foreground opacity-0 hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100",
                                                        "aria-label": "Link öffnen",
                                                        icons::ExternalLink { class: "size-3.5" }
                                                    }
                                                }
                                            }
                                        }
                                        for (mi, metric) in metrics.iter().enumerate() {
                                            td { key: "{metric}", class: "whitespace-nowrap px-3 py-1.5 text-right tabular-nums",
                                                {labels::format_metric(metric, row.metric_opt(mi))}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if has_more() {
                div { class: "flex justify-center",
                    Button {
                        variant: ButtonVariant::Outline,
                        size: ButtonSize::Sm,
                        disabled: loading(),
                        onclick: {
                            let mut load = load.clone();
                            move |_| load(offset() + PAGE)
                        },
                        if loading() { icons::LoaderCircle { class: "animate-spin" } }
                        "Mehr laden"
                    }
                }
            }
        }
    }
}

/// Spaltenkopf (Einstiege/Ausstiege statt Besuche bei Einstiegs-/Ausstiegsseiten).
fn column_label(report: &Report, metric: &str) -> String {
    if metric == report.metric && metric != "visitors" {
        return report.metric_label.to_string();
    }
    match metric {
        "group_conversion_rate" | "conversion_rate" => "KR".into(),
        "events" if report.filter_dim != "event:goal" && !report.filter_dim.starts_with("event:props:") => "Konversionen".into(),
        other => labels::metric(other).into(),
    }
}
