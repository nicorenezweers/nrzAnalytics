//! Abschnitt „Ziele“: eigene Ereignisse und Seitenaufrufe als Ziele (Conversions).

use super::helpers::{EmptyBox, InlineLoading, ReadOnlyNote, can_edit, site_api};
use crate::api;
use crate::components::common::{ConfirmModal, ErrorState, Field, Section};
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::checkbox::Checkbox;
use crate::components::ui::input::Input;
use crate::components::ui::modal::Modal;
use crate::components::ui::select::Select;
use crate::state;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{Goal, GoalKind, NewGoal, Role};

/// Waehrungen fuer Umsatz-Ziele (ISO 4217).
const CURRENCIES: [(&str, &str); 14] = [
    ("CHF", "CHF – Schweizer Franken"),
    ("EUR", "EUR – Euro"),
    ("USD", "USD – US-Dollar"),
    ("GBP", "GBP – Britisches Pfund"),
    ("JPY", "JPY – Japanischer Yen"),
    ("CAD", "CAD – Kanadischer Dollar"),
    ("AUD", "AUD – Australischer Dollar"),
    ("SEK", "SEK – Schwedische Krone"),
    ("NOK", "NOK – Norwegische Krone"),
    ("DKK", "DKK – Dänische Krone"),
    ("PLN", "PLN – Polnischer Złoty"),
    ("CZK", "CZK – Tschechische Krone"),
    ("CNY", "CNY – Renminbi"),
    ("INR", "INR – Indische Rupie"),
];

/// Ziel als Text (Ereignisname bzw. Pfad).
pub fn goal_target(g: &Goal) -> String {
    match g.kind {
        GoalKind::Event => g.event_name.clone().unwrap_or_default(),
        GoalKind::Page => g.page_path.clone().unwrap_or_default(),
    }
}

#[component]
pub fn GoalsSection(domain: String, role: Option<Role>) -> Element {
    let editor = can_edit(role);
    let mut goals = use_resource(use_reactive!(|domain| async move { api::get::<Vec<Goal>>(&site_api(&domain, "/goals")).await }));
    let mut modal_open = use_signal(|| false);
    let mut remove = use_signal(|| None::<Goal>);
    let mut remove_busy = use_signal(|| false);

    let d_rm = domain.clone();
    let confirm_remove = move |_| {
        let Some(g) = remove() else { return };
        let path = site_api(&d_rm, &format!("/goals/{}", g.id));
        remove_busy.set(true);
        spawn(async move {
            match api::delete(&path).await {
                Ok(()) => {
                    state::toast("Ziel gelöscht.");
                    goals.restart();
                }
                Err(e) => state::toast_error(e.message),
            }
            remove_busy.set(false);
            remove.set(None);
        });
    };

    let data = goals.read().clone();
    let remove_name = remove().map(|g| g.display_name).unwrap_or_default();

    rsx! {
        Section {
            title: "Ziele",
            description: "Ziele zählen Conversions: eigene Ereignisse oder Aufrufe bestimmter Seiten.",
            actions: rsx! {
                if editor {
                    Button { size: ButtonSize::Sm, onclick: move |_| modal_open.set(true),
                        icons::Plus {}
                        "Ziel hinzufügen"
                    }
                }
            },
            if !editor {
                ReadOnlyNote { text: "Nur Bearbeiter und Admins können Ziele verwalten." }
            }
            match data {
                None => rsx! { InlineLoading {} },
                Some(Err(e)) => rsx! { ErrorState { message: e.message, on_retry: move |_| goals.restart() } },
                Some(Ok(list)) if list.is_empty() => rsx! {
                    EmptyBox { icon: rsx! { icons::Target {} }, title: "Noch keine Ziele",
                        p { "Ein Ereignis-Ziel zählt eigene Ereignisse, die du auf deiner Website auslöst, z. B. "
                            code { class: "rounded bg-muted px-1 py-0.5 font-mono text-xs", "plausible('Signup')" }
                            ". Ein Seitenaufruf-Ziel zählt Besuche bestimmter Seiten, z. B. "
                            code { class: "rounded bg-muted px-1 py-0.5 font-mono text-xs", "/danke" }
                            "."
                        }
                    }
                },
                Some(Ok(list)) => rsx! {
                    ul { class: "divide-y rounded-lg border",
                        for g in list {
                            li { key: "{g.id}", class: "flex flex-wrap items-center gap-3 px-3 py-2.5",
                                Badge { variant: if g.kind == GoalKind::Event { BadgeVariant::Info } else { BadgeVariant::Secondary },
                                    if g.kind == GoalKind::Event { "Ereignis" } else { "Seite" }
                                }
                                div { class: "min-w-0 flex-1",
                                    p { class: "truncate font-medium", "{g.display_name}" }
                                    p { class: "truncate font-mono text-xs text-muted-foreground", "{goal_target(&g)}" }
                                }
                                if let Some(c) = &g.currency {
                                    Badge { variant: BadgeVariant::Success, "Umsatz · {c}" }
                                }
                                if editor {
                                    Button {
                                        variant: ButtonVariant::Ghost,
                                        size: ButtonSize::IconSm,
                                        aria_label: "Ziel „{g.display_name}“ löschen",
                                        title: "Löschen",
                                        onclick: {
                                            let g = g.clone();
                                            move |_| remove.set(Some(g.clone()))
                                        },
                                        icons::Trash2 {}
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
        if modal_open() {
            GoalModal {
                domain: domain.clone(),
                on_close: move |_| modal_open.set(false),
                on_created: move |_| {
                    modal_open.set(false);
                    goals.restart();
                },
            }
        }
        ConfirmModal {
            open: remove().is_some(),
            on_close: move |_| remove.set(None),
            on_confirm: confirm_remove,
            title: "Ziel löschen?",
            message: "Das Ziel „{remove_name}“ wird gelöscht. Trichter mit diesem Ziel funktionieren danach nicht mehr. Die erfassten Ereignisse bleiben erhalten.",
            confirm_label: "Löschen",
            busy: remove_busy(),
        }
    }
}

/// Dialog zum Anlegen eines Ziels (wird bei jedem Oeffnen neu erzeugt, Felder starten leer).
#[component]
fn GoalModal(domain: String, on_close: EventHandler<()>, on_created: EventHandler<()>) -> Element {
    let mut kind = use_signal(|| GoalKind::Event);
    let mut target = use_signal(String::new);
    let mut display = use_signal(String::new);
    let mut revenue = use_signal(|| false);
    let mut currency = use_signal(|| "CHF".to_string());
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let submit = move |e: FormEvent| {
        e.prevent_default();
        if busy() {
            return;
        }
        let mut t = target().trim().to_string();
        if t.is_empty() {
            error.set(Some(if kind() == GoalKind::Event { "Bitte einen Ereignisnamen eingeben." } else { "Bitte einen Pfad eingeben." }.into()));
            return;
        }
        if kind() == GoalKind::Page && !t.starts_with('/') {
            t.insert(0, '/');
        }
        let name = display().trim().to_string();
        let body = NewGoal {
            kind: kind(),
            event_name: (kind() == GoalKind::Event).then(|| t.clone()),
            page_path: (kind() == GoalKind::Page).then(|| t.clone()),
            display_name: if name.is_empty() { None } else { Some(name) },
            currency: (kind() == GoalKind::Event && revenue()).then(&*currency),
        };
        let path = site_api(&domain, "/goals");
        error.set(None);
        busy.set(true);
        spawn(async move {
            match api::post::<_, Goal>(&path, &body).await {
                Ok(g) => {
                    state::toast(format!("Ziel „{}“ angelegt.", g.display_name));
                    on_created.call(());
                }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };

    let is_event = kind() == GoalKind::Event;
    let seg = |active: bool| {
        if active {
            "flex-1 rounded-md bg-background px-3 py-1.5 text-sm font-medium shadow-sm"
        } else {
            "flex-1 rounded-md px-3 py-1.5 text-sm text-muted-foreground hover:text-foreground"
        }
    };
    let currency_options: Vec<(String, String)> = CURRENCIES.iter().map(|(c, l)| (c.to_string(), l.to_string())).collect();

    rsx! {
        Modal { open: true, on_close: move |_| on_close.call(()), title: "Ziel hinzufügen",
            form { id: "goal-form", class: "grid gap-4", onsubmit: submit,
                div { role: "radiogroup", "aria-label": "Art des Ziels", class: "flex gap-1 rounded-lg bg-muted p-1",
                    button {
                        r#type: "button",
                        role: "radio",
                        "aria-checked": "{is_event}",
                        class: seg(is_event),
                        onclick: move |_| kind.set(GoalKind::Event),
                        "Ereignis"
                    }
                    button {
                        r#type: "button",
                        role: "radio",
                        "aria-checked": "{!is_event}",
                        class: seg(!is_event),
                        onclick: move |_| kind.set(GoalKind::Page),
                        "Seitenaufruf"
                    }
                }
                if is_event {
                    Field { label: "Ereignisname", html_for: "goal-target", hint: "Genau so, wie er im Code steht, z. B. Signup für plausible('Signup').",
                        Input { id: "goal-target", value: target(), placeholder: "Signup", autofocus: true, autocomplete: "off", oninput: move |e: FormEvent| target.set(e.value()) }
                    }
                } else {
                    Field { label: "Pfad", html_for: "goal-target", hint: "Platzhalter: * steht für beliebige Zeichen außer /, ** für alles. Beispiele: /danke, /blog/*, /shop/**",
                        Input { id: "goal-target", value: target(), placeholder: "/danke", autofocus: true, autocomplete: "off", oninput: move |e: FormEvent| target.set(e.value()) }
                    }
                }
                Field { label: "Anzeigename (optional)", html_for: "goal-display", hint: "Wird in den Berichten angezeigt. Standard: der Ereignisname bzw. „Besuch /pfad“.",
                    Input { id: "goal-display", value: display(), autocomplete: "off", oninput: move |e: FormEvent| display.set(e.value()) }
                }
                if is_event {
                    div { class: "grid gap-3 rounded-lg border p-3",
                        label { class: "flex cursor-pointer items-center gap-2 text-sm",
                            Checkbox { checked: revenue(), aria_label: "Umsatz erfassen", on_checked_change: move |v: bool| revenue.set(v) }
                            "Umsatz erfassen (Umsatz-Ziel)"
                        }
                        if revenue() {
                            Field { label: "Währung", html_for: "goal-currency", hint: "Beträge werden mit dem Ereignis übergeben (revenue: {{ currency, amount }}).",
                                Select { id: "goal-currency", value: currency(), options: currency_options, on_change: move |v: String| currency.set(v) }
                            }
                        }
                    }
                }
                if let Some(err) = error() {
                    p { class: "text-sm text-destructive", role: "alert", "{err}" }
                }
                div { class: "flex justify-end gap-2 pt-2",
                    Button { variant: ButtonVariant::Ghost, onclick: move |_| on_close.call(()), "Abbrechen" }
                    Button { button_type: "submit", disabled: busy() || target().trim().is_empty(),
                        if busy() {
                            icons::LoaderCircle { class: "animate-spin" }
                        }
                        "Ziel anlegen"
                    }
                }
            }
        }
    }
}
