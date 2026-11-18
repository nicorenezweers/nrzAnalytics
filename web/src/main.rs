//! nrzAnalytics – Weboberflaeche (Dioxus 0.7, WASM, nrzUI).
//!
//! Nachbau des Plausible-Dashboards in Rust: Website-Liste, Dashboard mit Kennzahlen, Diagramm
//! und Berichtskarten, Einstellungen je Website, Konto, Verwaltung und geteilte Links.

#![allow(non_snake_case)]

mod api;
mod components;
mod pages;
mod route;
mod state;
mod util;

use dioxus::prelude::*;
use route::Route;

const TAILWIND: Asset = asset!("/assets/tailwind.css");
const FAVICON: Asset = asset!("/assets/favicon.svg");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    use_hook(state::init_theme);
    use_future(|| async { state::load_status().await });
    let ready = state::STATUS.read().is_some();
    let error = state::STATUS_ERROR.read().clone();

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: TAILWIND }
        document::Meta { name: "viewport", content: "width=device-width, initial-scale=1, viewport-fit=cover" }
        document::Meta { name: "referrer", content: "same-origin" }
        document::Meta { name: "color-scheme", content: "dark light" }
        components::ui::toast::Toaster {}
        if ready {
            Router::<Route> {}
        } else if let Some(e) = error {
            div { class: "grid min-h-dvh place-items-center bg-background p-6",
                div { class: "flex max-w-sm flex-col items-center gap-3 text-center",
                    components::common::Logo { class: "size-12" }
                    p { class: "text-sm text-muted-foreground", "{e}" }
                    components::ui::button::Button {
                        variant: components::ui::button::ButtonVariant::Outline,
                        onclick: move |_| {
                            spawn(async move { state::load_status().await });
                        },
                        icons::RefreshCw {}
                        "Erneut versuchen"
                    }
                }
            }
        } else {
            div { class: "grid min-h-dvh place-items-center bg-background",
                div { class: "flex flex-col items-center gap-3 text-muted-foreground",
                    components::common::Logo { class: "size-12" }
                    icons::LoaderCircle { class: "size-5 animate-spin" }
                }
            }
        }
    }
}
