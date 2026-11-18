use dioxus::prelude::*;
use tw_merge::tw_merge;

#[component]
pub fn SwitchLabel(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let merged = tw_merge!("text-sm font-medium", class.as_deref().unwrap_or(""));
    rsx! { span { class: "{merged}", {children} } }
}

/// Schalter (gesteuert): `checked` kommt von aussen, Aenderungen ueber `on_change`.
#[component]
pub fn Switch(
    #[props(into, optional)] id: Option<String>,
    #[props(default = false)] checked: bool,
    #[props(default = false)] disabled: bool,
    #[props(into, optional)] aria_label: Option<String>,
    #[props(into, optional)] class: Option<String>,
    #[props(optional)] on_change: Option<EventHandler<bool>>,
) -> Element {
    let state = if checked { "checked" } else { "unchecked" };

    let track_class = tw_merge!(
        "inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:bg-primary data-[state=unchecked]:bg-input",
        class.as_deref().unwrap_or("")
    );

    rsx! {
        button {
            "data-name": "Switch",
            id: id.as_deref(),
            r#type: "button",
            role: "switch",
            disabled,
            aria_checked: "{checked}",
            aria_label: aria_label.as_deref().unwrap_or("Schalter"),
            "data-state": "{state}",
            class: "{track_class}",
            onclick: move |_| {
                if let Some(h) = &on_change {
                    h.call(!checked);
                }
            },
            span {
                "data-state": "{state}",
                class: "block rounded-full ring-0 shadow-lg transition-transform pointer-events-none size-5 bg-background data-[state=checked]:translate-x-5 data-[state=unchecked]:translate-x-0",
            }
        }
    }
}
