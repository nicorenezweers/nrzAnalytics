//! Kleine Seiten: nicht gefunden, Weiterleitung der Einstellungen.

use crate::components::ui::button::{Button, ButtonVariant};
use crate::route::Route;
use dioxus::prelude::*;

#[component]
pub fn NotFound(segments: Vec<String>) -> Element {
    let path = format!("/{}", segments.join("/"));
    rsx! {
        div { class: "flex flex-col items-center gap-4 py-24 text-center",
            div { class: "text-6xl font-semibold text-muted-foreground/40", "404" }
            h1 { class: "text-xl font-semibold", "Seite nicht gefunden" }
            p { class: "text-sm text-muted-foreground", "Die Adresse {path} gibt es nicht." }
            Button { href: Route::Sites {}.to_string(), variant: ButtonVariant::Outline, icons::House {} "Zur Startseite" }
        }
    }
}

/// `/{domain}/settings` → `/{domain}/settings/general`.
#[component]
pub fn SiteSettingsIndex(domain: String) -> Element {
    let nav = navigator();
    let d = domain.clone();
    use_effect(move || {
        nav.replace(Route::site_settings(&d, "general"));
    });
    rsx! {}
}
