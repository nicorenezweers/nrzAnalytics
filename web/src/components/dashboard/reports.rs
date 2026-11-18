//! Berichtskarten: Quellen, Seiten, Standorte, Geraete. Jede Karte hat Tabs und eine Liste mit
//! Balken im Hintergrund; Klick auf eine Zeile setzt einen Filter, „Details“ oeffnet ein Modal.

use super::details::DetailsModal;
use super::{Dash, use_loader};
use crate::api;
use crate::components::ui::skeleton::Skeleton;
use dioxus::prelude::*;
use nrzanalytics_shared::geo;
use nrzanalytics_shared::labels;
use nrzanalytics_shared::query::{NONE_VALUE, ResultRow, dimensions as d, metrics as m};

/// Art der Beschriftung einer Zeile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Display {
    /// Wert unveraendert (bzw. ueber `labels::dimension_value`).
    Plain,
    /// Land: Flagge + Name.
    Country,
    /// Region: Dimensionen `[region, region_name]`, Flagge aus dem Code.
    Region,
    /// Stadt: Dimensionen `[city, country]`, Flagge des Landes.
    City,
}

/// Externer Link einer Zeile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkKind {
    None,
    /// `https://{domain}{pfad}`.
    Page,
    /// Wert ist eine URL bzw. ein Host.
    Url,
}

/// Beschreibung eines Berichts (ein Tab einer Karte).
#[derive(Clone, PartialEq, Debug)]
pub struct Report {
    pub key: &'static str,
    pub tab: &'static str,
    /// Titel im Detail-Modal.
    pub title: String,
    pub dimensions: Vec<String>,
    /// Dimension fuer Filter beim Klick.
    pub filter_dim: String,
    /// Spaltenkopf der ersten Spalte.
    pub column: String,
    /// Kennzahl fuer Balken und Wert.
    pub metric: &'static str,
    /// Beschriftung der Wertspalte.
    pub metric_label: &'static str,
    pub display: Display,
    pub link: LinkKind,
    /// Kennzahlen im Detail-Modal.
    pub detail_metrics: Vec<&'static str>,
    /// Balkenfarbe fuer Ziele.
    pub goal_bar: bool,
}

impl Report {
    pub fn new(key: &'static str, tab: &'static str, dimension: &str) -> Self {
        Self {
            key,
            tab,
            title: labels::dimension(dimension),
            dimensions: vec![dimension.to_string()],
            filter_dim: dimension.to_string(),
            column: labels::dimension(dimension),
            metric: m::VISITORS,
            metric_label: "Besucher",
            display: Display::Plain,
            link: LinkKind::None,
            detail_metrics: vec![m::VISITORS],
            goal_bar: false,
        }
    }

    fn title(mut self, t: &str) -> Self {
        self.title = t.to_string();
        self
    }

    fn column(mut self, c: &str) -> Self {
        self.column = c.to_string();
        self
    }

    fn dims(mut self, dims: &[&str]) -> Self {
        self.dimensions = dims.iter().map(|s| s.to_string()).collect();
        self
    }

    fn metric(mut self, metric: &'static str, label: &'static str) -> Self {
        self.metric = metric;
        self.metric_label = label;
        self
    }

    fn display(mut self, d: Display) -> Self {
        self.display = d;
        self
    }

    fn link(mut self, l: LinkKind) -> Self {
        self.link = l;
        self
    }

    fn details(mut self, metrics: &[&'static str]) -> Self {
        self.detail_metrics = metrics.to_vec();
        self
    }

    /// Kennzahlen der Liste (bei Ziel-Filter zusaetzlich Konversionsrate).
    pub fn list_metrics(&self, dash: &Dash) -> Vec<&'static str> {
        if dash.params.has_goal_filter() && self.filter_dim != d::EVENT_GOAL {
            vec![self.metric, m::GROUP_CONVERSION_RATE]
        } else {
            vec![self.metric]
        }
    }

    /// Kennzahlen im Detail-Modal (bei Ziel-Filter: Konversionen).
    pub fn modal_metrics(&self, dash: &Dash) -> Vec<&'static str> {
        if dash.params.has_goal_filter() && self.filter_dim != d::EVENT_GOAL {
            vec![m::VISITORS, m::EVENTS, m::GROUP_CONVERSION_RATE]
        } else {
            self.detail_metrics.clone()
        }
    }
}

/// Anzeige einer Zeile: (Beschriftung, Flagge).
pub fn row_label(report: &Report, row: &ResultRow) -> (String, Option<String>) {
    let v = row.dimension(0);
    match report.display {
        Display::Country => (labels::dimension_value(d::VISIT_COUNTRY, v), (v != NONE_VALUE).then(|| geo::flag(v))),
        Display::Region => {
            let name = row.dimension(1);
            let label = if name.is_empty() || name == NONE_VALUE { v.to_string() } else { name.to_string() };
            let cc = v.split('-').next().unwrap_or("");
            (label, (cc.len() == 2).then(|| geo::flag(cc)))
        }
        Display::City => {
            let cc = row.dimension(1);
            (v.to_string(), (cc.len() == 2).then(|| geo::flag(cc)))
        }
        Display::Plain => (labels::dimension_value(&report.filter_dim, v), None),
    }
}

/// Externer Link einer Zeile (oder `None`).
pub fn row_link(report: &Report, dash: &Dash, value: &str) -> Option<String> {
    if value.is_empty() || value == NONE_VALUE {
        return None;
    }
    match report.link {
        LinkKind::None => None,
        LinkKind::Page => value.starts_with('/').then(|| format!("https://{}{}", dash.domain, value)),
        LinkKind::Url => {
            if value.starts_with("http://") || value.starts_with("https://") {
                Some(value.to_string())
            } else if value.contains('.') && !value.contains(' ') {
                Some(format!("https://{value}"))
            } else {
                None
            }
        }
    }
}

/// Karte mit Tabs (Auswahl wird in localStorage gemerkt).
#[component]
pub fn ReportCard(dash: Dash, #[props(into)] title: String, card: &'static str, reports: Vec<Report>, #[props(optional)] header_extra: Option<Element>) -> Element {
    let storage_key = format!("nrz_tab_{card}");
    let mut chosen = use_signal(|| crate::util::storage_get(&storage_key).unwrap_or_default());
    let mut details_open = use_signal(|| false);
    let active = reports.iter().find(|r| r.key == chosen()).or_else(|| reports.first()).cloned();
    let Some(active) = active else { return rsx! {} };
    rsx! {
        section { class: "flex min-h-[27rem] flex-col rounded-xl border bg-card text-card-foreground shadow-sm",
            div { class: "flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-5 pt-4 pb-3",
                h2 { class: "font-semibold", "{title}" }
                div { class: "no-scrollbar -mx-1 flex max-w-full items-center gap-0.5 overflow-x-auto px-1", role: "tablist",
                    {header_extra}
                    if reports.len() > 1 {
                        for r in reports.iter() {
                            button {
                                key: "{r.key}",
                                r#type: "button",
                                role: "tab",
                                "aria-selected": "{r.key == active.key}",
                                class: if r.key == active.key { "whitespace-nowrap rounded-md px-2 py-1 text-xs font-semibold text-foreground underline decoration-chart-1 decoration-2 underline-offset-[6px] outline-none focus-visible:ring-2 focus-visible:ring-ring" } else { "whitespace-nowrap rounded-md px-2 py-1 text-xs font-medium text-muted-foreground hover:text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring" },
                                onclick: {
                                    let k = r.key.to_string();
                                    let sk = storage_key.clone();
                                    move |_| {
                                        crate::util::storage_set(&sk, &k);
                                        chosen.set(k.clone());
                                    }
                                },
                                "{r.tab}"
                            }
                        }
                    }
                }
            }
            ReportList { key: "{active.key}", dash: dash.clone(), report: active.clone(), on_details: move |_| details_open.set(true) }
        }
        DetailsModal { dash: dash.clone(), report: active.clone(), open: details_open(), on_close: move |_| details_open.set(false) }
    }
}

/// Liste der Top-9 mit Balken.
#[component]
pub fn ReportList(dash: Dash, report: Report, on_details: EventHandler<()>) -> Element {
    let nav = navigator();
    let metrics = report.list_metrics(&dash);
    let mut q = dash.query(&metrics).limit(10, 0);
    q.dimensions = report.dimensions.clone();
    let res = use_loader((dash.domain.clone(), q, dash.tick), |(domain, q, _)| async move { api::query(&domain, &q).await });
    let loading = *res.loading.read();
    let data = res.data.read();
    let content = match &*data {
        None => rsx! {
            div { class: "grid gap-2 px-5",
                for i in 0..6 {
                    Skeleton { key: "{i}", class: "h-7 w-full" }
                }
            }
        },
        Some(Err(e)) => rsx! { div { class: "flex flex-1 items-center justify-center px-5 text-sm text-destructive", "{e.message}" } },
        Some(Ok(r)) if r.results.is_empty() => rsx! {
            div { class: "flex flex-1 flex-col items-center justify-center gap-2 px-5 text-sm text-muted-foreground",
                icons::ChartNoAxesColumn { class: "size-6 opacity-50" }
                "Keine Daten im Zeitraum"
            }
        },
        Some(Ok(r)) => {
            let rows: Vec<ResultRow> = r.results.iter().take(9).cloned().collect();
            let more = r.results.len() > 9;
            let max = rows.iter().map(|row| row.metric(0)).fold(0.0, f64::max).max(1.0);
            let show_cr = metrics.len() > 1;
            rsx! {
                div { class: if loading { "flex flex-1 flex-col px-5 opacity-60 transition-opacity" } else { "flex flex-1 flex-col px-5 transition-opacity" },
                    div { class: "mb-1 flex items-center justify-between text-[11px] font-semibold uppercase tracking-wide text-muted-foreground",
                        span { "{report.column}" }
                        span { class: "flex gap-4",
                            span { class: "w-16 text-right", "{report.metric_label}" }
                            if show_cr {
                                span { class: "w-12 text-right", "KR" }
                            }
                        }
                    }
                    ul { class: "grid gap-1",
                        for (i, row) in rows.iter().enumerate() {
                            {
                                let (label, flag) = row_label(&report, row);
                                let value = row.dimension(0).to_string();
                                let link = row_link(&report, &dash, &value);
                                let width = (row.metric(0) / max * 100.0).clamp(0.0, 100.0);
                                let target = dash.route(dash.params.with_filter_is(&report.filter_dim, &value));
                                let bar = if report.goal_bar || dash.params.has_goal_filter() { "bg-bar-goal" } else { "bg-bar" };
                                rsx! {
                                    li { key: "{i}-{value}", class: "group flex items-center gap-4 text-sm",
                                        div { class: "relative min-w-0 flex-1",
                                            div { class: "absolute inset-y-0 left-0 rounded-md {bar} transition-[width] duration-300", style: "width: {width}%" }
                                            div { class: "relative flex h-7 items-center gap-2 px-2",
                                                button {
                                                    r#type: "button",
                                                    class: "flex min-w-0 items-center gap-2 text-left outline-none hover:underline focus-visible:underline",
                                                    title: "Filtern nach {label}",
                                                    onclick: move |_| { nav.push(target.clone()); },
                                                    if let Some(f) = &flag {
                                                        span { class: "shrink-0", "{f}" }
                                                    }
                                                    span { class: "truncate", "{label}" }
                                                }
                                                if let Some(href) = &link {
                                                    a {
                                                        href: "{href}",
                                                        target: "_blank",
                                                        rel: "noreferrer noopener",
                                                        class: "shrink-0 text-muted-foreground opacity-0 transition-opacity hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100",
                                                        "aria-label": "Link öffnen",
                                                        icons::ExternalLink { class: "size-3.5" }
                                                    }
                                                }
                                            }
                                        }
                                        span { class: "w-16 text-right font-medium tabular-nums", {labels::format_metric(report.metric, row.metric_opt(0))} }
                                        if show_cr {
                                            span { class: "w-12 text-right tabular-nums text-muted-foreground", {labels::format_metric(m::CONVERSION_RATE, row.metric_opt(1))} }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "mt-auto flex justify-center pt-3",
                        button {
                            r#type: "button",
                            class: "flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium text-muted-foreground hover:bg-accent hover:text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring",
                            onclick: move |_| on_details.call(()),
                            icons::Maximize2 { class: "size-3.5" }
                            if more { "Details und mehr" } else { "Details" }
                        }
                    }
                }
            }
        }
    };
    drop(data);
    rsx! {
        div { class: "flex flex-1 flex-col pb-4", {content} }
    }
}

// ---------------------------------------------------------------------------------------------
// Die vier Karten
// ---------------------------------------------------------------------------------------------

const SOURCE_DETAILS: [&str; 4] = [m::VISITORS, m::VISITS, m::BOUNCE_RATE, m::VISIT_DURATION];

#[component]
pub fn SourcesCard(dash: Dash) -> Element {
    let source_filtered = dash.params.has_filter(d::VISIT_SOURCE);
    let mut reports = vec![
        Report::new("channels", "Kanäle", d::VISIT_CHANNEL).title("Top-Kanäle").column("Kanal").details(&SOURCE_DETAILS),
        Report::new("sources", "Quellen", d::VISIT_SOURCE).title("Top-Quellen").column("Quelle").details(&SOURCE_DETAILS),
    ];
    if source_filtered {
        reports.push(
            Report::new("referrers", "Verweise", d::VISIT_REFERRER)
                .title("Verweis-URLs")
                .column("Verweis-URL")
                .link(LinkKind::Url)
                .details(&SOURCE_DETAILS),
        );
    }
    for (key, tab, dim) in [
        ("utm_medium", "Medium", d::VISIT_UTM_MEDIUM),
        ("utm_source", "UTM-Quelle", d::VISIT_UTM_SOURCE),
        ("utm_campaign", "Kampagne", d::VISIT_UTM_CAMPAIGN),
        ("utm_content", "Inhalt", d::VISIT_UTM_CONTENT),
        ("utm_term", "Begriff", d::VISIT_UTM_TERM),
    ] {
        reports.push(Report::new(key, tab, dim).details(&SOURCE_DETAILS));
    }
    rsx! { ReportCard { dash, title: "Quellen", card: "sources", reports } }
}

#[component]
pub fn PagesCard(dash: Dash) -> Element {
    let reports = vec![
        Report::new("pages", "Top-Seiten", d::EVENT_PAGE)
            .title("Top-Seiten")
            .column("Seite")
            .link(LinkKind::Page)
            .details(&[m::VISITORS, m::PAGEVIEWS, m::BOUNCE_RATE, m::TIME_ON_PAGE, m::SCROLL_DEPTH]),
        Report::new("entry", "Einstiegsseiten", d::VISIT_ENTRY_PAGE)
            .title("Einstiegsseiten")
            .column("Einstiegsseite")
            .metric(m::VISITS, "Einstiege")
            .link(LinkKind::Page)
            .details(&[m::VISITORS, m::VISITS, m::BOUNCE_RATE, m::VISIT_DURATION]),
        Report::new("exit", "Ausstiegsseiten", d::VISIT_EXIT_PAGE)
            .title("Ausstiegsseiten")
            .column("Ausstiegsseite")
            .metric(m::VISITS, "Ausstiege")
            .link(LinkKind::Page)
            .details(&[m::VISITORS, m::VISITS, m::EXIT_RATE]),
    ];
    rsx! { ReportCard { dash, title: "Seiten", card: "pages", reports } }
}

#[component]
pub fn LocationsCard(dash: Dash) -> Element {
    let reports = vec![
        Report::new("countries", "Länder", d::VISIT_COUNTRY).title("Länder").column("Land").display(Display::Country).details(&[m::VISITORS, m::PERCENTAGE]),
        Report::new("regions", "Regionen", d::VISIT_REGION)
            .title("Regionen")
            .column("Region")
            .dims(&[d::VISIT_REGION, d::VISIT_REGION_NAME])
            .display(Display::Region)
            .details(&[m::VISITORS, m::PERCENTAGE]),
        Report::new("cities", "Städte", d::VISIT_CITY)
            .title("Städte")
            .column("Stadt")
            .dims(&[d::VISIT_CITY, d::VISIT_COUNTRY])
            .display(Display::City)
            .details(&[m::VISITORS, m::PERCENTAGE]),
    ];
    rsx! { ReportCard { dash, title: "Standorte", card: "locations", reports } }
}

#[component]
pub fn DevicesCard(dash: Dash) -> Element {
    let browser = if dash.params.has_filter(d::VISIT_BROWSER) {
        Report::new("browser", "Browser", d::VISIT_BROWSER_VERSION).title("Browser-Versionen").column("Version")
    } else {
        Report::new("browser", "Browser", d::VISIT_BROWSER).title("Browser").column("Browser")
    };
    let os = if dash.params.has_filter(d::VISIT_OS) {
        Report::new("os", "Betriebssysteme", d::VISIT_OS_VERSION).title("Betriebssystem-Versionen").column("Version")
    } else {
        Report::new("os", "Betriebssysteme", d::VISIT_OS).title("Betriebssysteme").column("Betriebssystem")
    };
    let reports = vec![
        browser.details(&[m::VISITORS, m::PERCENTAGE, m::BOUNCE_RATE, m::VISIT_DURATION]),
        os.details(&[m::VISITORS, m::PERCENTAGE, m::BOUNCE_RATE, m::VISIT_DURATION]),
        Report::new("size", "Bildschirmgröße", d::VISIT_DEVICE)
            .title("Bildschirmgrößen")
            .column("Gerät")
            .details(&[m::VISITORS, m::PERCENTAGE, m::BOUNCE_RATE, m::VISIT_DURATION]),
    ];
    rsx! { ReportCard { dash, title: "Geräte", card: "devices", reports } }
}
