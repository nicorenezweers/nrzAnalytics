//! Diagramme als SVG direkt in rsx! (keine JS-Bibliotheken).

use dioxus::prelude::*;

/// Mini-Verlauf (z. B. Besucher je Stunde der letzten 24 h).
#[component]
pub fn Sparkline(values: Vec<u64>, #[props(into, default = "h-10 w-full".to_string())] class: String) -> Element {
    if values.len() < 2 {
        return rsx! { div { class: "{class}" } };
    }
    let max = values.iter().copied().max().unwrap_or(0).max(1) as f64;
    let n = values.len() as f64 - 1.0;
    let pts: Vec<(f64, f64)> =
        values.iter().enumerate().map(|(i, v)| (i as f64 / n * 100.0, 28.0 - (*v as f64 / max) * 26.0)).collect();
    let line = pts.iter().map(|(x, y)| format!("{x:.2},{y:.2}")).collect::<Vec<_>>().join(" ");
    let area = format!("M0,30 L{} L100,30 Z", pts.iter().map(|(x, y)| format!("{x:.2},{y:.2}")).collect::<Vec<_>>().join(" L"));
    rsx! {
        svg { class: "{class}", view_box: "0 0 100 30", preserve_aspect_ratio: "none", "aria-hidden": "true",
            path { d: "{area}", class: "fill-chart-1/15" }
            polyline {
                points: "{line}",
                fill: "none",
                class: "stroke-chart-1",
                stroke_width: "1.5",
                stroke_linejoin: "round",
                stroke_linecap: "round",
                "vector-effect": "non-scaling-stroke",
            }
        }
    }
}

/// Daten fuer das Hauptdiagramm.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Series {
    /// Zeitstempel (`YYYY-MM-DD` bzw. `YYYY-MM-DD HH:MM:SS`).
    pub labels: Vec<String>,
    pub values: Vec<f64>,
    /// Vergleichswerte (gleicher Index), falls ein Vergleich aktiv ist.
    pub comparison: Option<Vec<Option<f64>>>,
    pub comparison_labels: Vec<String>,
    /// Veraenderung je Punkt (Prozent bzw. Prozentpunkte).
    pub changes: Vec<Option<f64>>,
    pub metric: String,
}

/// Runde Achsenschritte: (Maximum, Schritte).
pub fn nice_ticks(max: f64) -> (f64, Vec<f64>) {
    let max = if max.is_finite() && max > 0.0 { max } else { 1.0 };
    let rough = max / 4.0;
    let mag = 10f64.powf(rough.log10().floor());
    let residual = rough / mag;
    let nice = if residual <= 1.0 {
        1.0
    } else if residual <= 2.0 {
        2.0
    } else if residual <= 2.5 {
        2.5
    } else if residual <= 5.0 {
        5.0
    } else {
        10.0
    };
    let mut step = nice * mag;
    if step < 1.0 && max >= 1.0 {
        step = 1.0;
    }
    let top = (max / step).ceil() * step;
    let n = (top / step).round() as usize;
    ((top).max(step), (0..=n).map(|i| i as f64 * step).collect())
}

/// Beschriftung der x-Achse.
fn x_label(label: &str, interval: &str, multi_day: bool) -> String {
    match interval {
        "minute" => crate::util::time_of(label),
        "hour" => {
            if multi_day {
                format!("{} {}", crate::util::date_short(label), crate::util::time_of(label))
            } else {
                crate::util::time_of(label)
            }
        }
        "month" => crate::util::month_year(label),
        _ => crate::util::date_short(label),
    }
}

/// Beschriftung im Tooltip.
pub fn tooltip_label(label: &str, interval: &str) -> String {
    match interval {
        "minute" | "hour" => format!("{}, {}", crate::util::date_long(label), crate::util::time_of(label)),
        "week" => format!("Woche ab {}", crate::util::date_medium(label)),
        "month" => crate::util::month_year_long(label),
        _ => crate::util::date_long(label),
    }
}

/// Liegt `now` (JJJJ-MM-TT HH:MM:SS) im Abschnitt, der bei `label` beginnt?
fn bucket_contains(label: &str, interval: &str, now: &str) -> bool {
    let today = now.get(..10).unwrap_or(now);
    let start = label.get(..10).unwrap_or(label);
    match interval {
        "minute" => label.get(..16).is_some_and(|l| now.get(..16) == Some(l)),
        "hour" => label.get(..13).is_some_and(|l| now.get(..13) == Some(l)),
        "day" => start == today,
        "week" => crate::util::add_days(start, 7).is_some_and(|end| start <= today && today < end.as_str()),
        "month" => start.get(..7).is_some_and(|m| today.starts_with(m)),
        _ => false,
    }
}

/// Beginnt der Abschnitt `label` erst nach `now`? (Beide lexikografisch vergleichbar.)
fn starts_after(label: &str, now: &str) -> bool {
    let k = label.len().min(now.len());
    label.get(..k) > now.get(..k)
}

fn fmt_value(metric: &str, v: f64) -> String {
    nrzanalytics_shared::labels::format_metric(metric, Some(v))
}

/// Linien-/Flaechendiagramm mit Vergleich, Fadenkreuz und Tooltip.
#[component]
pub fn LineChart(
    series: Series,
    metric: String,
    interval: String,
    /// Beschriftung der Kennzahl im Tooltip (sonst die Standard-Beschriftung).
    #[props(into, optional)]
    label: Option<String>,
    /// Jetzt (JJJJ-MM-TT HH:MM:SS, Zeitzone der Website): Abschnitte danach werden nicht
    /// gezeichnet, der laufende (unvollstaendige) Abschnitt gestrichelt.
    #[props(into, optional)]
    now: Option<String>,
    #[props(optional)] on_click: Option<EventHandler<String>>,
) -> Element {
    use dioxus::web::WebEventExt;
    use wasm_bindgen::JsCast;
    let mut hover = use_signal(|| None::<usize>);
    let n = series.values.len();
    if n == 0 {
        return rsx! { div { class: "flex h-[260px] items-center justify-center text-sm text-muted-foreground", "Keine Daten im Zeitraum" } };
    }
    let cmp_vals: Vec<f64> = series.comparison.as_ref().map(|c| c.iter().map(|v| v.unwrap_or(0.0)).collect()).unwrap_or_default();
    let max = series.values.iter().chain(cmp_vals.iter()).copied().fold(0.0, f64::max);
    let (top, ticks) = nice_ticks(max);
    let x = |i: usize| if n <= 1 { 50.0 } else { i as f64 / (n - 1) as f64 * 100.0 };
    let y = |v: f64| 100.0 - (v / top * 100.0).clamp(0.0, 100.0);
    let pts = |vals: &[f64]| -> String {
        vals.iter().enumerate().map(|(i, v)| format!("{:.3},{:.3}", x(i) * 10.0, y(*v))).collect::<Vec<_>>().join(" L")
    };
    // Abschnitte nach "jetzt" (heute, dieser Monat, dieses Jahr) gibt es noch nicht: die Linie
    // endet beim laufenden Abschnitt, die Achse zeigt trotzdem den ganzen Zeitraum. Der laufende
    // Abschnitt ist unvollstaendig: das letzte Stueck der Linie wird gestrichelt.
    let n_real = now
        .as_deref()
        .map_or(n, |nw| series.labels.iter().take(n).take_while(|l| !starts_after(l, nw)).count())
        .max(1);
    let open_end = n_real > 1
        && now.as_deref().is_some_and(|t| series.labels.get(n_real - 1).is_some_and(|l| bucket_contains(l, &interval, t)));
    let solid_until = if open_end { n_real - 1 } else { n_real };
    let line = format!("M{}", pts(&series.values[..solid_until]));
    let open_line = open_end.then(|| {
        let (a, b) = (n_real - 2, n_real - 1);
        format!("M{:.3},{:.3} L{:.3},{:.3}", x(a) * 10.0, y(series.values[a]), x(b) * 10.0, y(series.values[b]))
    });
    // Nur ein Punkt bisher (z. B. heute kurz nach Mitternacht): als Punkt zeigen.
    let lone_point = (n_real == 1 && n > 1).then(|| series.values[0]);
    let metric_label = label.clone().unwrap_or_else(|| nrzanalytics_shared::labels::metric(&metric).to_string());
    let area = if n_real > 1 {
        format!("M0,100 L{} L{:.3},100 Z", pts(&series.values[..n_real]), x(n_real - 1) * 10.0)
    } else {
        String::new()
    };
    let cmp_line = if series.comparison.is_some() && !cmp_vals.is_empty() { Some(format!("M{}", pts(&cmp_vals))) } else { None };
    let multi_day = {
        let first = series.labels.first().and_then(|l| l.get(..10)).unwrap_or("");
        let last = series.labels.last().and_then(|l| l.get(..10)).unwrap_or("");
        first != last
    };
    // x-Beschriftungen: hoechstens 7, gleichmaessig verteilt.
    let count = n.min(if interval == "hour" && multi_day { 5 } else { 7 });
    let mut xticks: Vec<usize> = if count <= 1 {
        vec![0]
    } else {
        (0..count).map(|k| ((k as f64) * (n - 1) as f64 / (count - 1) as f64).round() as usize).collect()
    };
    xticks.dedup();
    let clickable = on_click.is_some() && matches!(interval.as_str(), "day" | "week" | "month");
    let h = hover().filter(|i| *i < n_real);
    let tooltip = h.and_then(|i| {
        let label = series.labels.get(i)?;
        let value = *series.values.get(i)?;
        let cmp = series.comparison.as_ref().and_then(|c| c.get(i).copied().flatten());
        let cmp_label = series.comparison_labels.get(i).cloned();
        let change = series.changes.get(i).copied().flatten();
        Some((i, label.clone(), value, cmp, cmp_label, change))
    });
    let labels_for_click = series.labels.clone();
    let lower_better = metric == "bounce_rate" || metric == "exit_rate";
    let rate = nrzanalytics_shared::query::metrics::is_rate(&metric);

    rsx! {
        div { class: "relative h-[260px] w-full select-none",
            // y-Achse
            div { class: "pointer-events-none absolute bottom-7 left-0 top-2 w-12",
                for t in ticks.iter() {
                    div {
                        key: "{t}",
                        class: "absolute right-2 -translate-y-1/2 text-[11px] tabular-nums text-muted-foreground",
                        style: "top: {y(*t)}%",
                        {nrzanalytics_shared::labels::format_metric(&metric, Some(*t))}
                    }
                }
            }
            // Zeichenflaeche
            div { class: "absolute bottom-7 left-12 right-2 top-2",
                svg {
                    class: "absolute inset-0 h-full w-full overflow-visible",
                    view_box: "0 0 1000 100",
                    preserve_aspect_ratio: "none",
                    "aria-hidden": "true",
                    defs {
                        linearGradient { id: "nrz-area", x1: "0", y1: "0", x2: "0", y2: "1",
                            stop { offset: "0%", style: "stop-color: var(--chart-1); stop-opacity: 0.28" }
                            stop { offset: "100%", style: "stop-color: var(--chart-1); stop-opacity: 0" }
                        }
                    }
                    for t in ticks.iter() {
                        line {
                            key: "g{t}",
                            x1: "0",
                            x2: "1000",
                            y1: "{y(*t)}",
                            y2: "{y(*t)}",
                            class: "stroke-chart-grid",
                            stroke_width: "1",
                            "vector-effect": "non-scaling-stroke",
                        }
                    }
                    if let Some(c) = &cmp_line {
                        path {
                            d: "{c}",
                            fill: "none",
                            class: "stroke-chart-muted",
                            stroke_width: "1.75",
                            stroke_dasharray: "5 4",
                            stroke_linejoin: "round",
                            "vector-effect": "non-scaling-stroke",
                        }
                    }
                    if !area.is_empty() {
                        path { d: "{area}", fill: "url(#nrz-area)" }
                    }
                    path {
                        d: "{line}",
                        fill: "none",
                        class: "stroke-chart-1",
                        stroke_width: "2.25",
                        stroke_linejoin: "round",
                        stroke_linecap: "round",
                        "vector-effect": "non-scaling-stroke",
                    }
                    if let Some(o) = &open_line {
                        path {
                            d: "{o}",
                            fill: "none",
                            class: "stroke-chart-1",
                            stroke_width: "2.25",
                            stroke_dasharray: "4 5",
                            stroke_linecap: "round",
                            "vector-effect": "non-scaling-stroke",
                        }
                    }
                }
                if let Some(v) = lone_point {
                    div { class: "pointer-events-none absolute size-2 -translate-x-1/2 -translate-y-1/2 rounded-full bg-chart-1", style: "left: {x(0)}%; top: {y(v)}%" }
                }
                if let Some((i, _, value, cmp, _, _)) = &tooltip {
                    div { class: "pointer-events-none absolute inset-y-0 w-px bg-foreground/25", style: "left: {x(*i)}%" }
                    if let Some(c) = cmp {
                        div { class: "pointer-events-none absolute size-2 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-chart-muted bg-card", style: "left: {x(*i)}%; top: {y(*c)}%" }
                    }
                    div { class: "pointer-events-none absolute size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-chart-1 bg-card shadow", style: "left: {x(*i)}%; top: {y(*value)}%" }
                }
                // Ereignisflaeche (ohne Kinder, damit die Koordinaten stimmen)
                div {
                    class: if clickable { "absolute inset-0 cursor-pointer" } else { "absolute inset-0" },
                    onpointermove: move |e: PointerEvent| {
                        let px = e.element_coordinates().x;
                        let width = e
                            .data()
                            .try_as_web_event()
                            .and_then(|w| w.target())
                            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                            .map(|el| el.client_width() as f64)
                            .unwrap_or(0.0);
                        if width > 0.0 && n > 0 {
                            let i = if n <= 1 { 0 } else { ((px / width) * (n - 1) as f64).round().clamp(0.0, (n - 1) as f64) as usize };
                            if hover() != Some(i) {
                                hover.set(Some(i));
                            }
                        }
                    },
                    onpointerleave: move |_| hover.set(None),
                    onclick: move |_| {
                        if clickable
                            && let (Some(i), Some(h)) = (hover(), &on_click)
                                && let Some(l) = labels_for_click.get(i) {
                                    h.call(l.clone());
                                }
                    },
                }
                if let Some((i, point_label, value, cmp, cmp_label, change)) = &tooltip {
                    div {
                        class: "pointer-events-none absolute top-2 z-10 min-w-44 rounded-lg border bg-popover px-3 py-2 text-xs shadow-xl",
                        style: if x(*i) > 60.0 { format!("right: {}%; margin-right: 12px", 100.0 - x(*i)) } else { format!("left: {}%; margin-left: 12px", x(*i)) },
                        div { class: "mb-1.5 font-medium text-foreground", {tooltip_label(point_label, &interval)} }
                        div { class: "flex items-center justify-between gap-4",
                            span { class: "flex items-center gap-1.5 text-muted-foreground",
                                span { class: "inline-block size-2 rounded-full bg-chart-1" }
                                "{metric_label}"
                            }
                            span { class: "font-semibold tabular-nums", {fmt_value(&metric, *value)} }
                        }
                        if let Some(c) = cmp {
                            div { class: "mt-1 flex items-center justify-between gap-4",
                                span { class: "flex items-center gap-1.5 text-muted-foreground",
                                    span { class: "inline-block size-2 rounded-full bg-chart-muted" }
                                    {cmp_label.as_ref().map(|l| tooltip_label(l, &interval)).unwrap_or_else(|| "Vergleich".into())}
                                }
                                span { class: "tabular-nums", {fmt_value(&metric, *c)} }
                            }
                        }
                        if let Some(ch) = change {
                            {
                                let good = if lower_better { *ch < 0.0 } else { *ch > 0.0 };
                                let txt = if rate { format!("{}{} Pp.", if *ch > 0.0 { "+" } else { "" }, nrzanalytics_shared::labels::number_short(*ch)) } else { format!("{}{} %", if *ch > 0.0 { "+" } else { "" }, nrzanalytics_shared::labels::number_short(ch.round())) };
                                rsx! {
                                    div { class: if *ch == 0.0 { "mt-1 text-right text-muted-foreground" } else if good { "mt-1 text-right text-positive" } else { "mt-1 text-right text-negative" }, "{txt}" }
                                }
                            }
                        }
                    }
                }
            }
            // x-Achse
            div { class: "pointer-events-none absolute bottom-0 left-12 right-2 h-6",
                for (k, i) in xticks.iter().enumerate() {
                    div {
                        key: "x{i}",
                        // Schmale Bildschirme: nur jede zweite Beschriftung (erste und letzte bleiben).
                        class: {
                            let pos = if *i == 0 && n > 1 { "" } else if *i == n - 1 && n > 1 { "-translate-x-full" } else { "-translate-x-1/2" };
                            let narrow = if k % 2 == 1 && k + 1 != xticks.len() { "max-sm:hidden" } else { "" };
                            format!("absolute top-1 whitespace-nowrap text-[11px] text-muted-foreground {pos} {narrow}")
                        },
                        style: "left: {x(*i)}%",
                        {series.labels.get(*i).map(|l| x_label(l, &interval, multi_day)).unwrap_or_default()}
                    }
                }
            }
        }
    }
}
