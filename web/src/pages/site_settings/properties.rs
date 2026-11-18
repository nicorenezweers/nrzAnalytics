//! Abschnitt „Eigenschaften“: welche eigenen Eigenschaften (Props) gespeichert werden.

use super::helpers::{InlineLoading, ReadOnlyNote, can_edit, site_api};
use crate::api;
use crate::components::common::{ErrorState, Section};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::Input;
use crate::components::ui::switch::Switch;
use crate::state;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{PropsSettings, Role, UpdateProps};

#[component]
pub fn PropertiesSection(domain: String, role: Option<Role>) -> Element {
    let editor = can_edit(role);
    let mut props = use_resource(use_reactive!(|domain| async move { api::get::<PropsSettings>(&site_api(&domain, "/props")).await }));
    let data = props.read().clone();
    rsx! {
        Section { title: "Eigenschaften", description: "Eigene Eigenschaften (Props) an Ereignissen, z. B. Autor oder Tarif.",
            if !editor {
                ReadOnlyNote { text: "Nur Bearbeiter und Admins können Eigenschaften verwalten." }
            }
            match data {
                None => rsx! { InlineLoading {} },
                Some(Err(e)) => rsx! { ErrorState { message: e.message, on_retry: move |_| props.restart() } },
                Some(Ok(settings)) => rsx! {
                    PropsEditor { domain: domain.clone(), settings, editor, on_saved: move |_| props.restart() }
                },
            }
        }
    }
}

/// Bearbeitet eine lokale Kopie der Einstellungen und speichert sie auf Knopfdruck.
#[component]
fn PropsEditor(domain: String, settings: PropsSettings, editor: bool, on_saved: EventHandler<()>) -> Element {
    let mut all = use_signal(|| settings.allowed.is_none());
    let mut allowed = use_signal(|| settings.allowed.clone().unwrap_or_default());
    let mut input = use_signal(String::new);
    let mut busy = use_signal(|| false);

    let original_all = settings.allowed.is_none();
    let original_list = settings.allowed.clone().unwrap_or_default();
    let dirty = all() != original_all || (!all() && allowed() != original_list);

    let mut add_prop = move |name: String| {
        let n = name.trim().to_string();
        if n.is_empty() {
            return;
        }
        if !allowed.peek().contains(&n) {
            allowed.write().push(n);
        }
    };

    let save = move |_| {
        if busy() {
            return;
        }
        let body = UpdateProps { allowed: if all() { None } else { Some(allowed()) } };
        let path = site_api(&domain, "/props");
        busy.set(true);
        spawn(async move {
            match api::put::<_, PropsSettings>(&path, &body).await {
                Ok(_) => {
                    state::toast("Eigenschaften gespeichert.");
                    on_saved.call(());
                }
                Err(e) => state::toast_error(e.message),
            }
            busy.set(false);
        });
    };

    let suggestions: Vec<String> = settings.seen.iter().filter(|s| !allowed().contains(s)).cloned().collect();

    rsx! {
        div { class: "grid gap-5",
            div { class: "flex items-start justify-between gap-4",
                div {
                    label { r#for: "props-all", class: "text-sm font-medium", "Alle Eigenschaften erfassen" }
                    p { class: "text-xs text-muted-foreground",
                        if all() {
                            "Jede gesendete Eigenschaft wird gespeichert."
                        } else {
                            "Nur die unten erlaubten Eigenschaften werden gespeichert, alle anderen verworfen."
                        }
                    }
                }
                Switch { id: "props-all", checked: all(), disabled: !editor, aria_label: "Alle Eigenschaften erfassen", on_change: move |v: bool| all.set(v) }
            }
            if !all() {
                div { class: "grid gap-2",
                    p { class: "text-sm font-medium", "Erlaubte Eigenschaften" }
                    if allowed().is_empty() {
                        p { class: "text-sm text-muted-foreground", "Noch keine – es werden keine Eigenschaften gespeichert." }
                    } else {
                        ul { class: "flex flex-wrap gap-2",
                            for p in allowed() {
                                li { key: "{p}", class: "inline-flex items-center gap-1 rounded-md border bg-secondary py-0.5 pl-2 pr-0.5 font-mono text-xs text-secondary-foreground",
                                    "{p}"
                                    if editor {
                                        button {
                                            r#type: "button",
                                            class: "rounded p-0.5 text-muted-foreground hover:bg-accent hover:text-foreground [&>svg]:size-3.5",
                                            "aria-label": "{p} entfernen",
                                            onclick: {
                                                let p = p.clone();
                                                move |_| allowed.write().retain(|x| *x != p)
                                            },
                                            icons::X {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if editor {
                        form {
                            class: "flex gap-2",
                            onsubmit: move |e: FormEvent| {
                                e.prevent_default();
                                add_prop(input());
                                input.set(String::new());
                            },
                            Input {
                                class: "max-w-xs",
                                value: input(),
                                placeholder: "z. B. author",
                                aria_label: "Neue Eigenschaft",
                                autocomplete: "off",
                                oninput: move |e: FormEvent| input.set(e.value()),
                            }
                            Button { button_type: "submit", variant: ButtonVariant::Outline, disabled: input().trim().is_empty(),
                                icons::Plus {}
                                "Hinzufügen"
                            }
                        }
                    }
                }
            }
            div { class: "grid gap-2",
                p { class: "text-sm font-medium", "In den letzten 30 Tagen gesehen" }
                if settings.seen.is_empty() {
                    p { class: "text-sm text-muted-foreground", "Bisher wurden keine eigenen Eigenschaften gesendet." }
                } else if all() || !editor {
                    ul { class: "flex flex-wrap gap-2",
                        for s in settings.seen.iter() {
                            li { key: "{s}", class: "rounded-md border px-2 py-0.5 font-mono text-xs text-muted-foreground", "{s}" }
                        }
                    }
                } else if suggestions.is_empty() {
                    p { class: "text-sm text-muted-foreground", "Alle gesehenen Eigenschaften sind bereits erlaubt." }
                } else {
                    div { class: "flex flex-wrap gap-2",
                        for s in suggestions {
                            button {
                                key: "{s}",
                                r#type: "button",
                                class: "inline-flex items-center gap-1 rounded-md border border-dashed px-2 py-0.5 font-mono text-xs text-muted-foreground hover:border-solid hover:bg-accent hover:text-foreground [&>svg]:size-3",
                                "aria-label": "{s} erlauben",
                                onclick: {
                                    let s = s.clone();
                                    move |_| add_prop(s.clone())
                                },
                                icons::Plus {}
                                "{s}"
                            }
                        }
                    }
                }
            }
            if editor {
                div { class: "flex items-center gap-3 border-t pt-4",
                    Button { size: ButtonSize::Default, disabled: busy() || !dirty, onclick: save,
                        if busy() {
                            icons::LoaderCircle { class: "animate-spin" }
                        }
                        "Speichern"
                    }
                    if dirty {
                        span { class: "text-xs text-muted-foreground", "Ungespeicherte Änderungen" }
                    }
                }
            }
        }
    }
}
