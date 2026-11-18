//! Abschnitt „Gefahrenzone“: Statistiken zuruecksetzen, Website loeschen.

use super::helpers::{can_admin, is_owner, site_api};
use crate::api;
use crate::components::common::ConfirmModal;
use crate::components::ui::button::{Button, ButtonVariant};
use crate::route::Route;
use crate::state;
use dioxus::prelude::*;
use nrzanalytics_shared::api::Role;

#[component]
pub fn DangerSection(domain: String, role: Option<Role>) -> Element {
    let admin = can_admin(role);
    let owner = is_owner(role);
    let nav = navigator();
    let mut reset_open = use_signal(|| false);
    let mut delete_open = use_signal(|| false);
    let mut busy = use_signal(|| false);

    let d_reset = domain.clone();
    let do_reset = move |_| {
        if busy() {
            return;
        }
        let path = site_api(&d_reset, "/reset");
        busy.set(true);
        spawn(async move {
            match api::post_empty::<()>(&path).await {
                Ok(()) => state::toast("Alle Statistiken wurden gelöscht."),
                Err(e) => state::toast_error(e.message),
            }
            busy.set(false);
            reset_open.set(false);
        });
    };

    let d_delete = domain.clone();
    let do_delete = move |_| {
        if busy() {
            return;
        }
        let path = site_api(&d_delete, "");
        busy.set(true);
        spawn(async move {
            match api::delete(&path).await {
                Ok(()) => {
                    state::toast("Website gelöscht.");
                    *state::SITES_REVISION.write() += 1;
                    nav.push(Route::Sites {});
                }
                Err(e) => {
                    state::toast_error(e.message);
                    busy.set(false);
                    delete_open.set(false);
                }
            }
        });
    };

    rsx! {
        section { class: "rounded-xl border border-destructive/50 bg-card text-card-foreground shadow-sm",
            div { class: "flex items-center gap-2 border-b border-destructive/30 px-5 py-4",
                icons::TriangleAlert { class: "size-5 text-destructive" }
                h2 { class: "font-semibold leading-tight text-destructive", "Gefahrenzone" }
            }
            div { class: "divide-y divide-destructive/20",
                div { class: "flex flex-wrap items-center justify-between gap-4 px-5 py-4",
                    div { class: "min-w-0 max-w-xl",
                        p { class: "font-medium", "Statistiken zurücksetzen" }
                        p { class: "text-sm text-muted-foreground", "Löscht alle erfassten Besuche und Ereignisse dieser Website. Einstellungen, Ziele und Personen bleiben erhalten." }
                    }
                    Button { variant: ButtonVariant::Destructive, disabled: !admin, onclick: move |_| reset_open.set(true),
                        icons::RotateCcw {}
                        "Statistiken zurücksetzen"
                    }
                }
                div { class: "flex flex-wrap items-center justify-between gap-4 px-5 py-4",
                    div { class: "min-w-0 max-w-xl",
                        p { class: "font-medium", "Website löschen" }
                        p { class: "text-sm text-muted-foreground", "Entfernt die Website mit allen Statistiken und Einstellungen endgültig." }
                        if !owner {
                            p { class: "mt-1 text-xs text-muted-foreground", "Nur der Besitzer kann die Website löschen." }
                        }
                    }
                    Button { variant: ButtonVariant::Destructive, disabled: !owner, onclick: move |_| delete_open.set(true),
                        icons::Trash2 {}
                        "Website löschen"
                    }
                }
            }
        }
        ConfirmModal {
            open: reset_open(),
            on_close: move |_| reset_open.set(false),
            on_confirm: do_reset,
            title: "Statistiken zurücksetzen?",
            message: "Alle Statistiken von {domain} werden unwiderruflich gelöscht.",
            confirm_label: "Zurücksetzen",
            confirm_text: Some(domain.clone()),
            busy: busy(),
        }
        ConfirmModal {
            open: delete_open(),
            on_close: move |_| delete_open.set(false),
            on_confirm: do_delete,
            title: "Website löschen?",
            message: "{domain} wird mit allen Statistiken, Zielen, Trichtern und geteilten Links unwiderruflich gelöscht.",
            confirm_label: "Endgültig löschen",
            confirm_text: Some(domain.clone()),
            busy: busy(),
        }
    }
}
