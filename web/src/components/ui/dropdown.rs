//! Aufklappmenue (nrzUI-Stil, ohne JavaScript). Ein unsichtbarer Hintergrund faengt Klicks
//! ausserhalb ab, Esc schliesst. Der Zustand liegt in einem Signal des Aufrufers oder intern.

use dioxus::prelude::*;
use tw_merge::tw_merge;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum DropdownAlign {
    #[default]
    Start,
    End,
}

#[derive(Clone, Copy)]
struct DropdownCtx {
    open: Signal<bool>,
}

/// Menue mit Ausloeser. `trigger` ist der Inhalt des Knopfs.
#[component]
pub fn Dropdown(
    trigger: Element,
    #[props(into, optional)] trigger_class: Option<String>,
    #[props(into, optional)] menu_class: Option<String>,
    #[props(into, optional)] class: Option<String>,
    #[props(into, optional)] aria_label: Option<String>,
    #[props(default)] align: DropdownAlign,
    #[props(optional)] open: Option<Signal<bool>>,
    children: Element,
) -> Element {
    let inner = use_signal(|| false);
    let mut open = open.unwrap_or(inner);
    use_context_provider(|| DropdownCtx { open });
    let is_open = open();
    let trig = tw_merge!(
        "inline-flex items-center gap-2 rounded-md text-sm font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring/60 [&_svg]:shrink-0",
        trigger_class.as_deref().unwrap_or("")
    );
    let menu = tw_merge!(
        "absolute z-50 mt-1.5 min-w-48 max-h-[70dvh] overflow-y-auto rounded-lg border bg-popover p-1 text-popover-foreground shadow-xl outline-none",
        if align == DropdownAlign::End { "right-0" } else { "left-0" },
        menu_class.as_deref().unwrap_or("")
    );
    let wrap = tw_merge!("relative inline-block", class.as_deref().unwrap_or(""));
    rsx! {
        div {
            class: "{wrap}",
            onkeydown: move |e: KeyboardEvent| {
                if e.key() == Key::Escape && open() {
                    e.stop_propagation();
                    open.set(false);
                }
            },
            button {
                r#type: "button",
                class: "{trig}",
                "aria-haspopup": "menu",
                "aria-expanded": "{is_open}",
                "aria-label": aria_label,
                onclick: move |_| open.toggle(),
                {trigger}
            }
            if is_open {
                div { class: "fixed inset-0 z-40", onclick: move |_| open.set(false) }
                div { class: "{menu}", role: "menu", {children} }
            }
        }
    }
}

/// Eintrag im Menue; schliesst das Menue nach dem Klick.
#[component]
pub fn DropdownItem(
    #[props(into, optional)] class: Option<String>,
    #[props(default = false)] active: bool,
    #[props(default = false)] disabled: bool,
    #[props(optional)] onclick: Option<EventHandler<MouseEvent>>,
    #[props(into, optional)] shortcut: Option<String>,
    children: Element,
) -> Element {
    let ctx = try_use_context::<DropdownCtx>();
    let merged = tw_merge!(
        "relative flex w-full cursor-pointer select-none items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm outline-none hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent disabled:pointer-events-none disabled:opacity-50 [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-muted-foreground",
        if active { "bg-accent/70 font-medium text-foreground" } else { "" },
        class.as_deref().unwrap_or("")
    );
    rsx! {
        button {
            r#type: "button",
            role: "menuitem",
            class: "{merged}",
            disabled,
            onclick: move |e| {
                if let Some(mut c) = ctx {
                    c.open.set(false);
                }
                if let Some(h) = &onclick {
                    h.call(e);
                }
            },
            {children}
            if let Some(s) = &shortcut {
                if !s.is_empty() {
                    kbd { class: "ml-auto pl-4 font-mono text-[11px] text-muted-foreground", "{s}" }
                }
            }
        }
    }
}

/// Link-Eintrag im Menue (Router-Link).
#[component]
pub fn DropdownLink(#[props(into)] to: NavigationTarget, #[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let ctx = try_use_context::<DropdownCtx>();
    let merged = tw_merge!(
        "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm outline-none hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-muted-foreground",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        Link {
            to,
            class: "{merged}",
            onclick: move |_| {
                if let Some(mut c) = ctx {
                    c.open.set(false);
                }
            },
            {children}
        }
    }
}

#[component]
pub fn DropdownLabel(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let merged = tw_merge!("px-2 py-1.5 text-xs font-medium text-muted-foreground", class.as_deref().unwrap_or(""));
    rsx! { div { class: "{merged}", {children} } }
}

#[component]
pub fn DropdownSeparator() -> Element {
    rsx! { div { class: "-mx-1 my-1 h-px bg-border", role: "separator" } }
}
