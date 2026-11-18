//! Leerer Zustand: noch keine Daten – Snippet und Pruef-Knopf statt leerer Diagramme.

use super::Dash;
use crate::api;
use crate::components::common::CopyField;
use crate::components::ui::button::{Button, ButtonVariant};
use crate::route::Route;
use dioxus::prelude::*;
use nrzanalytics_shared::api::Verification;

#[component]
pub fn EmptyState(dash: Dash) -> Element {
    let mut checking = use_signal(|| false);
    let mut result = use_signal(|| None::<Result<Verification, String>>);
    let domain = dash.domain.clone();
    let verify = move |_| {
        if checking() {
            return;
        }
        checking.set(true);
        let domain = domain.clone();
        spawn(async move {
            let r = api::get::<Verification>(&format!("/api/sites/{}/verify", api::enc(&domain))).await;
            if matches!(&r, Ok(v) if v.received) {
                crate::util::reload();
            }
            result.set(Some(r.map_err(|e| e.message)));
            checking.set(false);
        });
    };
    rsx! {
        section { class: "flex flex-col items-center gap-5 rounded-xl border border-dashed bg-card px-6 py-12 text-center",
            div { class: "relative flex size-12 items-center justify-center rounded-full bg-secondary",
                span { class: "absolute size-3 rounded-full bg-live animate-live" }
            }
            div {
                h2 { class: "text-lg font-semibold", "Warten auf die ersten Besucher …" }
                p { class: "mx-auto mt-1 max-w-lg text-sm text-muted-foreground",
                    "Für {dash.domain} sind noch keine Daten angekommen. Füge dieses Snippet in den "
                    code { class: "rounded bg-muted px-1 font-mono text-xs", "<head>" }
                    " deiner Website ein – danach erscheinen die Besuche hier innerhalb weniger Sekunden."
                }
            }
            if !dash.shared {
                div { class: "w-full max-w-2xl text-left", CopyField { value: dash.site.snippet.clone(), multiline: true } }
                div { class: "flex flex-wrap justify-center gap-2",
                    Button { onclick: verify, disabled: checking(),
                        if checking() { icons::LoaderCircle { class: "animate-spin" } } else { icons::SearchCheck {} }
                        "Installation prüfen"
                    }
                    if dash.site.role.is_some() {
                        Button { variant: ButtonVariant::Outline, href: Route::Snippet { domain: dash.domain.clone() }.to_string(),
                            icons::Code {}
                            "Anleitung und Skript-Varianten"
                        }
                    }
                }
                match result() {
                    Some(Ok(v)) => rsx! {
                        p { class: if v.received { "text-sm text-success" } else { "max-w-lg text-sm text-muted-foreground" }, "{v.message}" }
                    },
                    Some(Err(e)) => rsx! { p { class: "text-sm text-destructive", "{e}" } },
                    None => rsx! {},
                }
            }
        }
    }
}
