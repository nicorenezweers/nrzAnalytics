//! Kurzmeldungen unten rechts (aus `state::TOASTS`).

use crate::state::{TOASTS, dismiss_toast};
use dioxus::prelude::*;

#[component]
pub fn Toaster() -> Element {
    let toasts = TOASTS.read().clone();
    rsx! {
        div {
            class: "pointer-events-none fixed inset-x-0 bottom-0 z-[100] flex flex-col items-center gap-2 p-4 sm:items-end",
            "aria-live": "polite",
            for t in toasts {
                div {
                    key: "{t.id}",
                    class: if t.error { "pointer-events-auto flex w-full max-w-sm items-start gap-3 rounded-lg border border-destructive/40 bg-popover px-4 py-3 text-sm shadow-xl" } else { "pointer-events-auto flex w-full max-w-sm items-start gap-3 rounded-lg border bg-popover px-4 py-3 text-sm shadow-xl" },
                    role: if t.error { "alert" } else { "status" },
                    if t.error {
                        icons::CircleAlert { class: "mt-0.5 size-4 shrink-0 text-destructive" }
                    } else {
                        icons::CircleCheck { class: "mt-0.5 size-4 shrink-0 text-success" }
                    }
                    span { class: "flex-1 break-words", "{t.text}" }
                    button {
                        r#type: "button",
                        class: "rounded p-0.5 text-muted-foreground hover:text-foreground",
                        "aria-label": "Meldung schliessen",
                        onclick: move |_| dismiss_toast(t.id),
                        icons::X { class: "size-3.5" }
                    }
                }
            }
        }
    }
}
