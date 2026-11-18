//! Abschnitt „Import/Export“: Statistiken als ZIP mit CSV-Dateien herunterladen.

use crate::components::common::{Field, Section};
use crate::components::ui::button::Button;
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::select::Select;
use crate::util;
use dioxus::prelude::*;
use nrzanalytics_shared::api::ExportQuery;
use nrzanalytics_shared::query::DateRange;

/// Waehlbare Zeitraeume (Schluessel laut `DateRange`, `custom` = benutzerdefiniert).
const PERIODS: [(&str, &str); 9] = [
    ("7d", "Letzte 7 Tage"),
    ("28d", "Letzte 28 Tage"),
    ("30d", "Letzte 30 Tage"),
    ("91d", "Letzte 91 Tage"),
    ("month", "Dieser Monat"),
    ("12mo", "Letzte 12 Monate"),
    ("year", "Dieses Jahr"),
    ("all", "Gesamter Zeitraum"),
    ("custom", "Benutzerdefiniert"),
];

/// Download-Adresse oder Fehlermeldung.
fn export_href(domain: &str, period: &str, from: &str, to: &str) -> Result<String, &'static str> {
    let date_range = if period == "custom" {
        if util::parse_date(from).is_none() || util::parse_date(to).is_none() {
            return Err("Bitte beide Tage angeben.");
        }
        // ISO-Daten lassen sich als Text vergleichen.
        if from > to {
            return Err("Der Starttag liegt nach dem Endtag.");
        }
        DateRange::custom(from, to)
    } else {
        DateRange::named(period)
    };
    let q = ExportQuery { date_range, date: None, filters: Vec::new() };
    let json = serde_json::to_string(&q).map_err(|_| "Export konnte nicht vorbereitet werden.")?;
    Ok(format!("/api/stats/{}/export?q={}", urlencoding::encode(domain), urlencoding::encode(&json)))
}

#[component]
pub fn ImportsExportsSection(domain: String) -> Element {
    let mut period = use_signal(|| "28d".to_string());
    let today = util::today();
    let start = util::add_days(&today, -29).unwrap_or_else(|| today.clone());
    let mut from = use_signal(|| start);
    let mut to = use_signal(move || today);

    let href = export_href(&domain, &period(), &from(), &to());
    let options: Vec<(String, String)> = PERIODS.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect();

    rsx! {
        Section { title: "Export", description: "Statistiken als ZIP-Datei mit CSV-Dateien herunterladen (Besucher, Quellen, Seiten, Länder, Geräte, Ziele …).",
            div { class: "grid gap-4",
                Field { label: "Zeitraum", html_for: "export-period",
                    Select { id: "export-period", class: "sm:max-w-xs", value: period(), options, on_change: move |v: String| period.set(v) }
                }
                if period() == "custom" {
                    div { class: "grid gap-4 sm:grid-cols-2 sm:max-w-md",
                        Field { label: "Von", html_for: "export-from",
                            Input { id: "export-from", r#type: InputType::Date, value: from(), max: to(), oninput: move |e: FormEvent| from.set(e.value()) }
                        }
                        Field { label: "Bis", html_for: "export-to",
                            Input { id: "export-to", r#type: InputType::Date, value: to(), min: from(), oninput: move |e: FormEvent| to.set(e.value()) }
                        }
                    }
                }
                match href {
                    Ok(h) => rsx! {
                        div {
                            a {
                                href: "{h}",
                                download: "",
                                class: "inline-flex h-9 items-center justify-center gap-2 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground shadow-xs hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&>svg]:size-4",
                                icons::Download {}
                                "Export herunterladen"
                            }
                        }
                    },
                    Err(msg) => rsx! {
                        div { class: "grid gap-2",
                            p { class: "text-sm text-destructive", role: "alert", "{msg}" }
                            div {
                                Button { disabled: true,
                                    icons::Download {}
                                    "Export herunterladen"
                                }
                            }
                        }
                    },
                }
            }
        }
        Section { title: "Import", description: "Daten aus einer anderen Plausible-Instanz übernehmen.",
            div { class: "flex items-start gap-2 text-sm text-muted-foreground",
                icons::Info { class: "mt-0.5 size-4 shrink-0" }
                p { "Der Import aus Plausible-CSV-Exporten ist noch nicht verfügbar." }
            }
        }
    }
}
