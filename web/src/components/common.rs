//! Gemeinsame Bausteine: Logo, Buchstaben-Avatar, Kopierfeld, Seitenkopf, Formularfelder,
//! Bestaetigungsdialog, Lade- und Fehlerzustaende, Anmelde-Waechter.

use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::Input;
use crate::components::ui::modal::{Modal, ModalSize};
use crate::components::ui::skeleton::Skeleton;
use crate::route::Route;
use crate::state;
use dioxus::prelude::*;

/// Standard-Titel des Tabs (wie in `index.html`).
pub const APP_TITLE: &str = "nrzAnalytics";

fn set_document_title(title: &str) {
    if let Some(d) = web_sys::window().and_then(|w| w.document()) {
        d.set_title(title);
    }
}

/// Titel des Tabs, solange die Komponente angezeigt wird. Ersetzt `document::Title`: das nutzt
/// intern `eval`, was die Content-Security-Policy (ohne `unsafe-eval`) blockiert.
#[component]
pub fn PageTitle(#[props(into)] title: String) -> Element {
    use_effect(use_reactive((&title,), |(title,)| set_document_title(&title)));
    use_drop(|| set_document_title(APP_TITLE));
    rsx! {}
}

/// Kleines Logo (wie das Favicon).
#[component]
pub fn Logo(#[props(into, default = "size-7".to_string())] class: String) -> Element {
    rsx! {
        svg {
            class: "{class} shrink-0",
            view_box: "0 0 64 64",
            "aria-hidden": "true",
            rect { width: "64", height: "64", rx: "14", class: "fill-secondary" }
            path {
                d: "M14 46 L26 32 L34 38 L50 18",
                fill: "none",
                class: "stroke-chart-1",
                stroke_width: "6",
                stroke_linecap: "round",
                stroke_linejoin: "round",
            }
            circle { cx: "50", cy: "18", r: "4.5", class: "fill-foreground" }
        }
    }
}

/// Logo mit Schriftzug.
#[component]
pub fn Brand() -> Element {
    rsx! {
        span { class: "flex items-center gap-2",
            Logo {}
            span { class: "text-base font-semibold tracking-tight",
                "nrz"
                span { class: "text-chart-1", "Analytics" }
            }
        }
    }
}

/// Buchstaben-Avatar statt Favicon (keine externen Anfragen).
#[component]
pub fn LetterAvatar(#[props(into)] domain: String, #[props(into, default = "size-6 text-xs".to_string())] class: String) -> Element {
    let color = match crate::util::hue_index(&domain) {
        1 => "bg-chart-1/20 text-chart-1",
        2 => "bg-chart-4/20 text-chart-4",
        3 => "bg-chart-5/20 text-chart-5",
        4 => "bg-chart-2/20 text-chart-2",
        _ => "bg-secondary text-secondary-foreground",
    };
    let letter = crate::util::initial(&domain);
    rsx! {
        span { class: "inline-flex shrink-0 items-center justify-center rounded-md font-semibold {color} {class}", "aria-hidden": "true", "{letter}" }
    }
}

/// Nur-Lese-Feld mit Kopieren-Knopf.
#[component]
pub fn CopyField(#[props(into)] value: String, #[props(default = false)] multiline: bool, #[props(into, optional)] aria_label: Option<String>) -> Element {
    let v = value.clone();
    rsx! {
        div { class: "flex w-full items-start gap-2",
            if multiline {
                pre { class: "min-w-0 flex-1 overflow-x-auto rounded-md border bg-muted/40 px-3 py-2 font-mono text-xs leading-relaxed whitespace-pre-wrap break-all select-all", "{value}" }
            } else {
                Input { class: "font-mono text-xs", value: value.clone(), readonly: true, aria_label: aria_label.clone().unwrap_or_else(|| "Wert".into()) }
            }
            Button {
                variant: ButtonVariant::Outline,
                size: ButtonSize::Icon,
                aria_label: "Kopieren",
                title: "Kopieren",
                onclick: move |_| crate::util::copy(&v),
                icons::Copy {}
            }
        }
    }
}

/// Seitenkopf mit Titel, Beschreibung und optionalen Aktionen rechts.
#[component]
pub fn PageHeader(#[props(into)] title: String, #[props(into, optional)] description: Option<String>, #[props(optional)] actions: Option<Element>) -> Element {
    rsx! {
        div { class: "mb-6 flex flex-wrap items-end justify-between gap-4",
            div { class: "min-w-0",
                h1 { class: "text-2xl font-semibold tracking-tight", "{title}" }
                if let Some(d) = description {
                    p { class: "mt-1 text-sm text-muted-foreground", "{d}" }
                }
            }
            if let Some(a) = actions {
                div { class: "flex flex-wrap items-center gap-2", {a} }
            }
        }
    }
}

/// Formularfeld mit Beschriftung und Hinweis.
#[component]
pub fn Field(#[props(into)] label: String, #[props(into, optional)] html_for: Option<String>, #[props(into, optional)] hint: Option<String>, children: Element) -> Element {
    rsx! {
        div { class: "grid gap-1.5",
            label { r#for: html_for.as_deref(), class: "text-sm font-medium", "{label}" }
            {children}
            if let Some(h) = hint {
                p { class: "text-xs text-muted-foreground", "{h}" }
            }
        }
    }
}

/// Abschnitt in Einstellungsseiten (Karte mit Titel, Beschreibung, Inhalt).
#[component]
pub fn Section(#[props(into)] title: String, #[props(into, optional)] description: Option<String>, #[props(optional)] actions: Option<Element>, #[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let extra = class.unwrap_or_default();
    rsx! {
        section { class: "rounded-xl border bg-card text-card-foreground shadow-sm {extra}",
            div { class: "flex flex-wrap items-start justify-between gap-3 border-b px-5 py-4",
                div { class: "min-w-0",
                    h2 { class: "font-semibold leading-tight", "{title}" }
                    if let Some(d) = description {
                        p { class: "mt-1 text-sm text-muted-foreground", "{d}" }
                    }
                }
                if let Some(a) = actions {
                    div { class: "flex items-center gap-2", {a} }
                }
            }
            div { class: "px-5 py-4", {children} }
        }
    }
}

/// Bestaetigungsdialog. Mit `confirm_text` muss dieser Text eingetippt werden.
#[component]
pub fn ConfirmModal(
    open: bool,
    on_close: EventHandler<()>,
    on_confirm: EventHandler<()>,
    #[props(into)] title: String,
    #[props(into)] message: String,
    #[props(into, default = "Bestätigen".to_string())] confirm_label: String,
    #[props(default = true)] destructive: bool,
    #[props(into, optional)] confirm_text: Option<String>,
    #[props(default = false)] busy: bool,
) -> Element {
    let mut typed = use_signal(String::new);
    let ok = confirm_text.as_ref().is_none_or(|t| typed().trim() == t.as_str());
    rsx! {
        Modal {
            open,
            on_close: move |_| {
                typed.set(String::new());
                on_close.call(());
            },
            title,
            size: ModalSize::Sm,
            footer: rsx! {
                Button { variant: ButtonVariant::Ghost, onclick: move |_| on_close.call(()), "Abbrechen" }
                Button {
                    variant: if destructive { ButtonVariant::Destructive } else { ButtonVariant::Default },
                    disabled: !ok || busy,
                    onclick: move |_| {
                        typed.set(String::new());
                        on_confirm.call(());
                    },
                    if busy {
                        icons::LoaderCircle { class: "animate-spin" }
                    }
                    "{confirm_label}"
                }
            },
            p { class: "text-sm text-muted-foreground", "{message}" }
            if let Some(t) = &confirm_text {
                div { class: "mt-4 grid gap-1.5",
                    label { class: "text-sm", "Zum Bestätigen "
                        code { class: "rounded bg-muted px-1 py-0.5 font-mono text-xs", "{t}" }
                        " eintippen:"
                    }
                    Input { value: typed(), autofocus: true, oninput: move |e: FormEvent| typed.set(e.value()), aria_label: "Bestätigung" }
                }
            }
        }
    }
}

/// Platzhalter beim Laden einer Seite.
#[component]
pub fn PageSkeleton() -> Element {
    rsx! {
        div { class: "grid gap-4",
            Skeleton { class: "h-8 w-56" }
            Skeleton { class: "h-4 w-80" }
            Skeleton { class: "h-40 w-full" }
            Skeleton { class: "h-40 w-full" }
        }
    }
}

/// Fehleranzeige mit optionalem Erneut-Knopf.
#[component]
pub fn ErrorState(#[props(into)] message: String, #[props(optional)] on_retry: Option<EventHandler<()>>) -> Element {
    rsx! {
        div { class: "flex flex-col items-center gap-3 rounded-xl border border-dashed px-6 py-12 text-center",
            icons::CircleAlert { class: "size-8 text-destructive" }
            p { class: "max-w-md text-sm text-muted-foreground", "{message}" }
            if let Some(r) = on_retry {
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, onclick: move |_| r.call(()),
                    icons::RefreshCw {}
                    "Erneut versuchen"
                }
            }
        }
    }
}

/// Leitet nicht angemeldete Besucher zur Anmeldung um; zeigt sonst den Inhalt.
#[component]
pub fn RequireLogin(children: Element) -> Element {
    let logged_in = state::me().is_some();
    let nav = navigator();
    use_effect(move || {
        if state::me().is_none() {
            nav.replace(Route::login_here());
        }
    });
    if logged_in { rsx! { {children} } } else { rsx! { PageSkeleton {} } }
}

/// Nur fuer Instanz-Admins.
#[component]
pub fn RequireAdmin(children: Element) -> Element {
    let me = state::me();
    match me {
        Some(m) if m.is_admin => rsx! { {children} },
        Some(_) => rsx! { ErrorState { message: "Dieser Bereich ist nur für Administratoren." } },
        None => rsx! { RequireLogin { {children} } },
    }
}
