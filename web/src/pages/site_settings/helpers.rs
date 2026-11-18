//! Kleine Helfer fuer die Website-Einstellungen (Pfade, Rechte, Hinweise, leere Zustaende).

use crate::api;
use dioxus::prelude::*;
use nrzanalytics_shared::api::Role;

/// API-Pfad unterhalb einer Website, z. B. `site_api("a.ch", "/goals")` → `/api/sites/a.ch/goals`.
pub fn site_api(domain: &str, rest: &str) -> String {
    format!("/api/sites/{}{}", api::enc(domain), rest)
}

/// Darf Ziele, Trichter, Eigenschaften, Schutzregeln und geteilte Links bearbeiten.
pub fn can_edit(role: Option<Role>) -> bool {
    role.is_some_and(Role::can_edit)
}

/// Darf Allgemeines, Personen und Sichtbarkeit aendern sowie Statistiken zuruecksetzen.
pub fn can_admin(role: Option<Role>) -> bool {
    role.is_some_and(Role::can_admin)
}

/// Besitzer der Website (darf sie loeschen).
pub fn is_owner(role: Option<Role>) -> bool {
    role == Some(Role::Owner)
}

/// Hinweis fuer Konten ohne Schreibrechte.
#[component]
pub fn ReadOnlyNote(#[props(into)] text: String) -> Element {
    rsx! {
        div { class: "mb-4 flex items-start gap-2 rounded-lg border bg-muted/40 px-3 py-2 text-sm text-muted-foreground", role: "note",
            icons::Lock { class: "mt-0.5 size-4 shrink-0" }
            span { "{text}" }
        }
    }
}

/// Leerer Zustand mit Symbol, Titel und Erklaerung.
#[component]
pub fn EmptyBox(icon: Element, #[props(into)] title: String, children: Element) -> Element {
    rsx! {
        div { class: "flex flex-col items-center gap-2 rounded-lg border border-dashed px-6 py-10 text-center",
            div { class: "text-muted-foreground [&>svg]:size-8", {icon} }
            p { class: "font-medium", "{title}" }
            div { class: "max-w-md text-sm text-muted-foreground", {children} }
        }
    }
}

/// Kleiner Ladeplatzhalter innerhalb eines Abschnitts.
#[component]
pub fn InlineLoading() -> Element {
    rsx! {
        div { class: "grid gap-2",
            crate::components::ui::skeleton::Skeleton { class: "h-9 w-full" }
            crate::components::ui::skeleton::Skeleton { class: "h-9 w-full" }
            crate::components::ui::skeleton::Skeleton { class: "h-9 w-2/3" }
        }
    }
}
