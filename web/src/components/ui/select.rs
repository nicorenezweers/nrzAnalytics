//! Auswahlfeld auf Basis des nativen `<select>` (barrierefrei, mobil gut bedienbar).

use dioxus::prelude::*;
use tw_merge::tw_merge;

/// Auswahl. Optionen als (Wert, Beschriftung).
#[component]
pub fn Select(
    #[props(into)] value: String,
    options: Vec<(String, String)>,
    on_change: EventHandler<String>,
    #[props(into, optional)] id: Option<String>,
    #[props(into, optional)] class: Option<String>,
    #[props(into, optional)] aria_label: Option<String>,
    #[props(default = false)] disabled: bool,
) -> Element {
    let merged = tw_merge!(
        "h-9 w-full appearance-none rounded-md border border-input bg-transparent pl-3 pr-8 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-input/30 [&>option]:bg-popover [&>option]:text-popover-foreground",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        div { class: "relative w-full",
            select {
                id: id.as_deref(),
                class: "{merged}",
                disabled,
                "aria-label": aria_label,
                onchange: move |e| on_change.call(e.value()),
                for (v, l) in options.iter() {
                    option { key: "{v}", value: "{v}", selected: *v == value, "{l}" }
                }
            }
            icons::ChevronDown { class: "pointer-events-none absolute right-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" }
        }
    }
}
