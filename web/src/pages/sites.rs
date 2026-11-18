//! Website-Liste („Meine Websites“) und Formular für eine neue Website.

use crate::api;
use crate::components::charts::Sparkline;
use crate::components::common::{ErrorState, Field, LetterAvatar, PageHeader, RequireLogin, Section};
use crate::components::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::empty::{Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::select::Select;
use crate::components::ui::skeleton::Skeleton;
use crate::route::Route;
use crate::state::{self, SITES_REVISION};
use dioxus::prelude::*;
use nrzanalytics_shared::api::{NewSite as NewSiteBody, Site, SiteSummary, TimezoneOption};
use nrzanalytics_shared::labels;

/// Ab dieser Anzahl Websites wird das Suchfeld angezeigt.
const SEARCH_THRESHOLD: usize = 5;
/// Standard-Zeitzone, falls die des Browsers nicht bekannt ist.
const FALLBACK_TZ: &str = "Europe/Zurich";

/// `/` – zeigt die Website-Liste.
#[component]
pub fn Home() -> Element {
    rsx! { Sites {} }
}

/// `/sites` – alle Websites des Kontos als Karten.
#[component]
pub fn Sites() -> Element {
    rsx! {
        RequireLogin {
            PageHeader {
                title: "Websites",
                description: "Alle Websites, auf die du Zugriff hast.",
                actions: rsx! {
                    Button { href: Route::NewSite {}.to_string(),
                        icons::Plus {}
                        "Website hinzufügen"
                    }
                },
            }
            SiteList {}
        }
    }
}

/// Liste mit Laden, Fehler, Leerzustand, Suche und Raster.
#[component]
fn SiteList() -> Element {
    let mut sites = use_resource(move || async move {
        // Neu laden, sobald sich die Websites ändern.
        let _ = SITES_REVISION();
        api::get::<Vec<SiteSummary>>("/api/sites").await
    });
    let mut query = use_signal(String::new);

    let result = sites.read().clone();
    match result {
        None => rsx! {
            div { class: "grid gap-4 sm:grid-cols-2 lg:grid-cols-3", "aria-busy": "true",
                for i in 0..6 {
                    SiteCardSkeleton { key: "{i}" }
                }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState { message: e.message, on_retry: move |_| sites.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! { NoSites {} },
        Some(Ok(list)) => {
            let q = query().trim().to_lowercase();
            let total = list.len();
            let shown: Vec<SiteSummary> = list.into_iter().filter(|s| q.is_empty() || s.domain.to_lowercase().contains(&q)).collect();
            rsx! {
                if total > SEARCH_THRESHOLD {
                    div { class: "relative mb-4 max-w-sm",
                        icons::Search { class: "pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" }
                        Input {
                            class: "pl-9",
                            r#type: InputType::Search,
                            placeholder: "Domain suchen …",
                            aria_label: "Websites nach Domain durchsuchen",
                            value: query(),
                            oninput: move |e: FormEvent| query.set(e.value()),
                        }
                    }
                }
                if shown.is_empty() {
                    p { class: "rounded-xl border border-dashed px-6 py-10 text-center text-sm text-muted-foreground",
                        "Keine Website passt zu „{query}“."
                    }
                } else {
                    div { class: "grid gap-4 sm:grid-cols-2 lg:grid-cols-3",
                        for s in shown {
                            SiteCard { key: "{s.domain}", site: s }
                        }
                    }
                }
            }
        }
    }
}

/// Karte einer Website. Die ganze Karte ist ein Link zum Dashboard; das Zahnrad liegt darüber.
#[component]
fn SiteCard(site: SiteSummary) -> Element {
    let domain = site.domain.clone();
    let can_configure = site.role.is_some_and(|r| r.can_admin() || r.can_edit());
    let visitors = labels::number_short(site.visitors_24h as f64);
    rsx! {
        div { class: "group relative flex flex-col gap-4 rounded-xl border bg-card p-5 text-card-foreground shadow-sm transition-colors hover:border-ring/60 hover:bg-accent/30",
            Link {
                to: Route::dashboard(&domain),
                class: "absolute inset-0 z-0 rounded-xl outline-none focus-visible:ring-2 focus-visible:ring-ring",
                "aria-label": "Dashboard von {domain} öffnen",
            }
            div { class: "pointer-events-none relative flex items-start gap-3",
                LetterAvatar { domain: domain.clone(), class: "size-9 text-sm" }
                div { class: "min-w-0 flex-1",
                    div { class: "flex items-center gap-2",
                        span { class: "truncate font-semibold", "{domain}" }
                        if site.public {
                            Badge { variant: BadgeVariant::Secondary, size: BadgeSize::Sm, "öffentlich" }
                        }
                    }
                    if let Some(role) = site.role {
                        p { class: "mt-0.5 text-xs text-muted-foreground", "{role.label()}" }
                    }
                }
                if can_configure {
                    Link {
                        to: Route::site_settings(&domain, "general"),
                        class: "pointer-events-auto relative z-10 -mr-1 -mt-1 rounded-md p-1.5 text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring",
                        "aria-label": "Einstellungen von {domain}",
                        title: "Einstellungen",
                        icons::Settings { class: "size-4" }
                    }
                }
            }
            div { class: "pointer-events-none relative flex items-end justify-between gap-3",
                div {
                    p { class: "text-2xl font-semibold tabular-nums tracking-tight", "{visitors}" }
                    p { class: "text-xs text-muted-foreground", "Besucher in 24 h" }
                }
                ChangeIndicator { change: site.change_24h }
            }
            div { class: "pointer-events-none relative",
                Sparkline { values: site.sparkline.clone(), class: "h-10 w-full" }
            }
        }
    }
}

/// Veränderung gegenüber den 24 Stunden davor, z. B. „+12 %“ mit Pfeil.
#[component]
fn ChangeIndicator(change: Option<f64>) -> Element {
    let Some(c) = change.filter(|c| c.is_finite()) else {
        return rsx! {};
    };
    let rounded = c.round() as i64;
    if rounded == 0 {
        return rsx! {
            span { class: "text-sm font-medium tabular-nums text-muted-foreground", title: "Veränderung zu den 24 Stunden davor", "0 %" }
        };
    }
    let up = rounded > 0;
    let text = if up { format!("+{rounded} %") } else { format!("−{} %", rounded.unsigned_abs()) };
    let color = if up { "text-positive" } else { "text-negative" };
    rsx! {
        span { class: "inline-flex items-center gap-0.5 text-sm font-medium tabular-nums {color}", title: "Veränderung zu den 24 Stunden davor",
            if up {
                icons::ArrowUpRight { class: "size-4" }
            } else {
                icons::ArrowDownRight { class: "size-4" }
            }
            "{text}"
        }
    }
}

#[component]
fn SiteCardSkeleton() -> Element {
    rsx! {
        div { class: "flex flex-col gap-4 rounded-xl border bg-card p-5",
            div { class: "flex items-center gap-3",
                Skeleton { class: "size-9" }
                div { class: "grid flex-1 gap-1.5",
                    Skeleton { class: "h-4 w-32" }
                    Skeleton { class: "h-3 w-16" }
                }
            }
            div { class: "grid gap-1.5",
                Skeleton { class: "h-7 w-20" }
                Skeleton { class: "h-3 w-24" }
            }
            Skeleton { class: "h-10 w-full" }
        }
    }
}

/// Leerer Zustand: noch keine Website.
#[component]
fn NoSites() -> Element {
    rsx! {
        Empty { class: "py-16",
            EmptyHeader {
                EmptyMedia { variant: EmptyMediaVariant::Icon, icons::Globe {} }
                EmptyTitle { "Noch keine Website" }
                EmptyDescription { class: "max-w-md",
                    "Füge deine erste Website hinzu. Danach erhältst du ein kleines Skript für den "
                    code { class: "rounded bg-muted px-1 font-mono text-xs", "<head>" }
                    " deiner Seiten – ohne Cookies und ohne personenbezogene Daten."
                }
            }
            EmptyContent {
                Button { href: Route::NewSite {}.to_string(),
                    icons::Plus {}
                    "Website hinzufügen"
                }
            }
        }
    }
}

/// Eingabe bereinigen: Schema, Pfad, Abfrage, `www.` entfernen und klein schreiben.
fn clean_domain(input: &str) -> String {
    let mut s = input.trim().to_lowercase();
    for prefix in ["https://", "http://", "//"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
            break;
        }
    }
    if let Some(i) = s.find(['/', '?', '#']) {
        s.truncate(i);
    }
    if let Some(rest) = s.strip_prefix("www.") {
        s = rest.to_string();
    }
    s.trim_end_matches('.').trim().to_string()
}

/// `/sites/new` – neue Website anlegen.
#[component]
pub fn NewSite() -> Element {
    rsx! {
        RequireLogin {
            div { class: "mx-auto max-w-xl",
                PageHeader { title: "Website hinzufügen", description: "Danach erhältst du das Snippet für deine Website." }
                NewSiteForm {}
            }
        }
    }
}

#[component]
fn NewSiteForm() -> Element {
    let nav = navigator();
    let mut domain = use_signal(String::new);
    let mut tz = use_signal(crate::util::browser_timezone);
    let mut busy = use_signal(|| false);
    let timezones = use_resource(|| async { api::get::<Vec<TimezoneOption>>("/api/timezones").await });

    // Zeitzone des Browsers nur übernehmen, wenn der Server sie kennt.
    use_effect(move || {
        if let Some(Ok(list)) = &*timezones.read() {
            let current = tz.peek().clone();
            if !list.is_empty() && !list.iter().any(|t| t.name == current) {
                tz.set(FALLBACK_TZ.to_string());
            }
        }
    });

    let options: Vec<(String, String)> = match &*timezones.read() {
        Some(Ok(list)) if !list.is_empty() => list.iter().map(|t| (t.name.clone(), t.label.clone())).collect(),
        _ => vec![(tz(), tz())],
    };
    let tz_failed = matches!(&*timezones.read(), Some(Err(_)));
    let cleaned = clean_domain(&domain());
    let show_cleaned = !cleaned.is_empty() && cleaned != domain().trim();

    rsx! {
        Section { title: "Website",
            form {
                class: "grid gap-5",
                onsubmit: move |e: FormEvent| {
                    e.prevent_default();
                    if busy() {
                        return;
                    }
                    let d = clean_domain(&domain());
                    if d.is_empty() || !d.contains('.') || d.contains(char::is_whitespace) {
                        state::toast_error("Bitte eine gültige Domain eingeben, z. B. example.com.");
                        return;
                    }
                    let body = NewSiteBody { domain: d, timezone: tz() };
                    busy.set(true);
                    spawn(async move {
                        match api::post::<_, Site>("/api/sites", &body).await {
                            Ok(site) => {
                                *SITES_REVISION.write() += 1;
                                state::toast(format!("{} wurde angelegt.", site.domain));
                                nav.replace(Route::Snippet { domain: site.domain });
                            }
                            Err(e) => state::toast_error(e.message),
                        }
                        busy.set(false);
                    });
                },
                Field {
                    label: "Domain",
                    html_for: "site-domain",
                    hint: "Ohne „https://“ und „www.“, z. B. example.com oder blog.example.com.",
                    Input {
                        id: "site-domain",
                        placeholder: "example.com",
                        value: domain(),
                        autofocus: true,
                        required: true,
                        autocomplete: "off",
                        inputmode: "url",
                        oninput: move |e: FormEvent| domain.set(e.value()),
                    }
                    if show_cleaned {
                        p { class: "text-xs text-muted-foreground",
                            "Wird gespeichert als "
                            code { class: "rounded bg-muted px-1 font-mono", "{cleaned}" }
                        }
                    }
                }
                Field {
                    label: "Zeitzone",
                    html_for: "site-timezone",
                    hint: if tz_failed { "Zeitzonen konnten nicht geladen werden – es wird die des Browsers verwendet." } else { "Bestimmt, wann für die Auswertung ein Tag beginnt." },
                    Select {
                        id: "site-timezone",
                        value: tz(),
                        options,
                        aria_label: "Zeitzone",
                        on_change: move |v: String| tz.set(v),
                    }
                }
                div { class: "flex flex-wrap items-center justify-end gap-2 pt-1",
                    Button { variant: ButtonVariant::Ghost, href: Route::Sites {}.to_string(), "Abbrechen" }
                    Button { button_type: "submit", disabled: busy(), size: ButtonSize::Default,
                        if busy() {
                            icons::LoaderCircle { class: "animate-spin" }
                        }
                        "Weiter zum Snippet"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::clean_domain;

    #[test]
    fn bereinigt_domain() {
        assert_eq!(clean_domain(" https://www.Example.com/pfad?x=1 "), "example.com");
        assert_eq!(clean_domain("blog.example.com"), "blog.example.com");
        assert_eq!(clean_domain("http://www.example.ch#top"), "example.ch");
    }
}
