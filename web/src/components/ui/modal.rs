//! Modal-Dialog (nrzUI-Stil, ohne JavaScript): per Signal gesteuert, Esc und Klick auf den
//! Hintergrund schliessen, Fokus wandert beim Oeffnen in den Dialog. Mobil vollbreit.

use dioxus::prelude::*;
use tw_merge::tw_merge;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ModalSize {
    Sm,
    #[default]
    Default,
    Lg,
    Xl,
}

impl ModalSize {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Sm => "sm:max-w-sm",
            Self::Default => "sm:max-w-lg",
            Self::Lg => "sm:max-w-2xl",
            Self::Xl => "sm:max-w-4xl",
        }
    }
}

/// Dialog. Wird nur gerendert, wenn `open` wahr ist; `on_close` bei Esc, Hintergrund und X.
#[component]
pub fn Modal(
    open: bool,
    on_close: EventHandler<()>,
    #[props(into, optional)] title: Option<String>,
    #[props(into, optional)] description: Option<String>,
    #[props(default)] size: ModalSize,
    #[props(into, optional)] class: Option<String>,
    #[props(optional)] footer: Option<Element>,
    children: Element,
) -> Element {
    if !open {
        return rsx! {};
    }
    let panel = tw_merge!(
        "relative flex max-h-[100dvh] w-full flex-col overflow-hidden border bg-popover text-popover-foreground shadow-2xl outline-none sm:max-h-[90dvh] sm:rounded-xl rounded-t-xl",
        size.as_str(),
        class.as_deref().unwrap_or("")
    );
    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-end justify-center bg-black/60 backdrop-blur-[2px] sm:items-center sm:p-4",
            onclick: move |_| on_close.call(()),
            onkeydown: move |e: KeyboardEvent| {
                if e.key() == Key::Escape {
                    e.stop_propagation();
                    on_close.call(());
                }
            },
            div {
                class: "{panel}",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": title.clone(),
                tabindex: "-1",
                onclick: move |e| e.stop_propagation(),
                onmounted: move |e| {
                    spawn(async move {
                        let _ = e.data().set_focus(true).await;
                    });
                },
                if title.is_some() || description.is_some() {
                    div { class: "flex items-start gap-4 border-b px-5 py-4",
                        div { class: "min-w-0 flex-1",
                            if let Some(t) = &title {
                                h2 { class: "text-base font-semibold leading-tight", "{t}" }
                            }
                            if let Some(d) = &description {
                                p { class: "mt-1 text-sm text-muted-foreground", "{d}" }
                            }
                        }
                        button {
                            r#type: "button",
                            class: "-mr-1 rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring outline-none",
                            "aria-label": "Schliessen",
                            onclick: move |_| on_close.call(()),
                            icons::X { class: "size-4" }
                        }
                    }
                }
                div { class: "min-h-0 flex-1 overflow-y-auto px-5 py-4", {children} }
                if let Some(f) = footer {
                    div { class: "flex flex-wrap items-center justify-end gap-2 border-t px-5 py-3", {f} }
                }
            }
        }
    }
}
