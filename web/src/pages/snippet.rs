//! Snippet-Seite einer Website: Einbau-Anleitung, Skript-Varianten, eigene Ereignisse, Prüfung.

use crate::api;
use crate::components::common::{CopyField, ErrorState, LetterAvatar, PageHeader, PageSkeleton, RequireLogin, Section};
use crate::components::ui::alert::{Alert, AlertDescription, AlertTitle};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::route::Route;
use crate::util;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{Site, Verification};

/// Skript-Varianten wie bei Plausible: (Erweiterung, Beschreibung).
const VARIANTS: [(&str, &str); 8] = [
    ("hash", "Hash-Routing (#/pfad) als eigene Seiten erfassen"),
    ("outbound-links", "Klicks auf externe Links"),
    ("file-downloads", "Datei-Downloads (PDF, ZIP, …)"),
    ("tagged-events", "Ereignisse per CSS-Klasse plausible-event-name=…"),
    ("revenue", "Umsatz zu Ereignissen erfassen"),
    ("pageview-props", "Eigenschaften per event-*-Attribut am Skript"),
    ("manual", "Seitenaufrufe manuell auslösen"),
    ("local", "Auch localhost erfassen (zum Testen)"),
];

/// URL einer Variante aus der Skript-URL ableiten: `…/script.js` → `…/script.hash.js`.
fn variant_url(script_url: &str, ext: &str) -> String {
    match script_url.strip_suffix(".js") {
        Some(base) => format!("{base}.{ext}.js"),
        None => format!("{script_url}.{ext}.js"),
    }
}

#[component]
pub fn Snippet(domain: String) -> Element {
    rsx! {
        RequireLogin {
            SnippetPage { domain }
        }
    }
}

#[component]
fn SnippetPage(domain: String) -> Element {
    let mut site = use_resource(use_reactive!(|domain| async move { api::get::<Site>(&format!("/api/sites/{}", api::enc(&domain))).await }));
    let result = site.read().clone();
    match result {
        None => rsx! { PageSkeleton {} },
        Some(Err(e)) => rsx! {
            ErrorState { message: e.message, on_retry: move |_| site.restart() }
        },
        Some(Ok(s)) => rsx! {
            SnippetContent { key: "{s.domain}", site: s }
        },
    }
}

#[component]
fn SnippetContent(site: Site) -> Element {
    let mut verification = use_signal(|| None::<Verification>);
    let mut busy = use_signal(|| false);
    let domain = site.domain.clone();
    let received = verification().is_some_and(|v| v.received);

    let verify = {
        let domain = domain.clone();
        move |_| {
            if busy() {
                return;
            }
            busy.set(true);
            let domain = domain.clone();
            spawn(async move {
                match api::get::<Verification>(&format!("/api/sites/{}/verify", api::enc(&domain))).await {
                    Ok(v) => verification.set(Some(v)),
                    Err(e) => crate::state::toast_error(e.message),
                }
                busy.set(false);
            });
        }
    };

    let combined = variant_url(&site.script_url, "hash.outbound-links");
    let event_example = "plausible('Signup', {props: {plan: 'Pro'}})";
    let revenue_example = "plausible('Kauf', {revenue: {currency: 'CHF', amount: 49.90}})";

    rsx! {
        div { class: "mx-auto grid max-w-3xl gap-6",
            div { class: "flex items-center gap-3",
                LetterAvatar { domain: domain.clone(), class: "size-10 text-base" }
                div { class: "min-w-0 flex-1",
                    PageHeader { title: "Snippet einbauen", description: "Für {domain}" }
                }
            }
            Steps { done_verify: received }

            Section {
                title: "Snippet",
                description: "Füge diese Zeile in den <head>-Bereich jeder Seite ein – am besten im gemeinsamen Layout oder Template.",
                div { class: "grid gap-3",
                    CopyField { value: site.snippet.clone(), multiline: true, aria_label: "Snippet" }
                    p { class: "text-xs text-muted-foreground",
                        "Das Skript ist klein, setzt keine Cookies und speichert keine personenbezogenen Daten. Bei WordPress, Ghost & Co. gibt es meist ein Feld für „Code im Head“."
                    }
                }
            }

            Section {
                title: "Installation prüfen",
                description: "Rufe deine Website einmal auf und prüfe dann, ob Ereignisse ankommen.",
                div { class: "grid gap-4",
                    div { class: "flex flex-wrap gap-2",
                        Button { onclick: verify, disabled: busy(),
                            if busy() {
                                icons::LoaderCircle { class: "animate-spin" }
                            } else {
                                icons::CircleCheck {}
                            }
                            "Installation prüfen"
                        }
                        Button { variant: ButtonVariant::Outline, href: Route::dashboard(&domain).to_string(),
                            "Zum Dashboard"
                            icons::ArrowRight {}
                        }
                    }
                    if let Some(v) = verification() {
                        VerificationResult { v }
                    }
                }
            }

            Section {
                title: "Skript-Varianten",
                description: "Für zusätzliche Messungen ersetzt du script.js durch eine Variante.",
                div { class: "grid gap-4",
                    ul { class: "divide-y rounded-lg border",
                        for (ext, desc) in VARIANTS {
                            VariantRow { key: "{ext}", ext, desc, url: variant_url(&site.script_url, ext) }
                        }
                    }
                    div { class: "grid gap-2",
                        p { class: "text-sm text-muted-foreground",
                            "Varianten sind kombinierbar – Erweiterungen einfach aneinanderhängen, z. B. Hash-Routing und externe Links:"
                        }
                        CopyField { value: combined, aria_label: "Kombinierte Skript-URL" }
                    }
                }
            }

            Section {
                title: "Eigene Ereignisse",
                description: "Anmeldungen, Käufe oder Klicks per JavaScript an nrzAnalytics melden.",
                div { class: "grid gap-4 text-sm",
                    div { class: "grid gap-2",
                        p { class: "text-muted-foreground", "Ereignis mit Eigenschaften:" }
                        CopyField { value: event_example, multiline: true, aria_label: "Beispiel Ereignis" }
                    }
                    div { class: "grid gap-2",
                        p { class: "text-muted-foreground", "Umsatz (mit der Variante script.revenue.js):" }
                        CopyField { value: revenue_example, multiline: true, aria_label: "Beispiel Umsatz" }
                    }
                    Alert {
                        icons::Info {}
                        AlertTitle { "Ziele anlegen" }
                        AlertDescription {
                            "Ereignisse erscheinen im Dashboard als Conversions, sobald du dafür in den Einstellungen ein Ziel anlegst. "
                            if site.role.is_some_and(|r| r.can_edit()) {
                                Link { to: Route::site_settings(&domain, "goals"), class: "font-medium underline underline-offset-4", "Zu den Zielen" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Schritt-Anzeige „1 Website angelegt · 2 Snippet einbauen · 3 Prüfen“.
#[component]
fn Steps(done_verify: bool) -> Element {
    let steps = [("Website angelegt", true, false), ("Snippet einbauen", done_verify, !done_verify), ("Prüfen", done_verify, false)];
    rsx! {
        ol { class: "flex flex-wrap items-center gap-x-3 gap-y-2 text-sm", "aria-label": "Fortschritt",
            for (i, (label, done, current)) in steps.into_iter().enumerate() {
                li { key: "{i}", class: "flex items-center gap-2",
                    if i > 0 {
                        span { class: "text-muted-foreground/50", "aria-hidden": "true", "·" }
                    }
                    span {
                        class: if done { "inline-flex size-6 items-center justify-center rounded-full bg-success/20 text-success" } else if current { "inline-flex size-6 items-center justify-center rounded-full bg-primary text-xs font-semibold text-primary-foreground" } else { "inline-flex size-6 items-center justify-center rounded-full bg-muted text-xs font-semibold text-muted-foreground" },
                        "aria-hidden": "true",
                        if done {
                            icons::Check { class: "size-3.5" }
                        } else {
                            "{i + 1}"
                        }
                    }
                    span {
                        class: if done || current { "font-medium" } else { "text-muted-foreground" },
                        "aria-current": if current { "step" } else { "false" },
                        "{label}"
                    }
                }
            }
        }
    }
}

#[component]
fn VariantRow(ext: &'static str, desc: &'static str, url: String) -> Element {
    let u = url.clone();
    rsx! {
        li { class: "flex items-center gap-3 px-3 py-2.5",
            div { class: "min-w-0 flex-1",
                p { class: "truncate font-mono text-xs", title: "{url}", "script.{ext}.js" }
                p { class: "text-xs text-muted-foreground", "{desc}" }
            }
            Button {
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
                aria_label: "URL von script.{ext}.js kopieren",
                title: "URL kopieren",
                onclick: move |_| util::copy(&u),
                icons::Copy {}
            }
        }
    }
}

/// Ergebnis der Prüfung als Hinweis.
#[component]
fn VerificationResult(v: Verification) -> Element {
    if v.received {
        let last = v.last_event_at.map(util::relative);
        return rsx! {
            Alert { class: "border-success/40 bg-success/10",
                icons::CircleCheck { class: "text-success" }
                AlertTitle { "Es kommen Daten an" }
                AlertDescription {
                    if let Some(l) = last {
                        "Letztes Ereignis {l}. "
                    }
                    "{v.message}"
                }
            }
        };
    }
    let hint = match v.snippet_found {
        Some(true) => "Das Snippet ist auf der Startseite eingebaut, aber es ist noch kein Ereignis angekommen. Rufe die Seite in einem Browser ohne Werbeblocker auf und prüfe erneut.",
        Some(false) => "Auf der Startseite wurde kein Snippet gefunden. Prüfe, ob es im <head> steht, und leere gegebenenfalls den Cache (CDN, Caching-Plugin).",
        None => "Die Startseite war nicht erreichbar. Prüfe die Domain und ob die Seite öffentlich zugänglich ist.",
    };
    rsx! {
        Alert { class: "border-warning/40 bg-warning/10",
            icons::TriangleAlert { class: "text-warning" }
            AlertTitle { "Noch keine Daten" }
            AlertDescription {
                "{hint}"
                if !v.message.trim().is_empty() {
                    span { class: "mt-1 block text-muted-foreground", "{v.message}" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::variant_url;

    #[test]
    fn leitet_varianten_ab() {
        assert_eq!(variant_url("https://a.ch/js/script.js", "hash"), "https://a.ch/js/script.hash.js");
        assert_eq!(variant_url("https://a.ch/js/script.js", "hash.outbound-links"), "https://a.ch/js/script.hash.outbound-links.js");
    }
}
