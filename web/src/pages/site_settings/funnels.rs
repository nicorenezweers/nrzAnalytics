//! Abschnitt „Trichter“: Abfolgen von Zielen (2 bis 8 Schritte).

use super::helpers::{EmptyBox, InlineLoading, ReadOnlyNote, can_edit, site_api};
use crate::api;
use crate::components::common::{ConfirmModal, ErrorState, Field, Section};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::Input;
use crate::components::ui::modal::{Modal, ModalSize};
use crate::components::ui::select::Select;
use crate::route::Route;
use crate::state;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{Funnel, Goal, NewFunnel, Role};

const MIN_STEPS: usize = 2;
const MAX_STEPS: usize = 8;

/// Geoeffneter Editor: `None` = neu, `Some(f)` = bestehenden Trichter bearbeiten.
#[derive(Clone, PartialEq)]
struct Editing(Option<Funnel>);

#[component]
pub fn FunnelsSection(domain: String, role: Option<Role>) -> Element {
    let editor = can_edit(role);
    let mut funnels = use_resource(use_reactive!(|domain| async move { api::get::<Vec<Funnel>>(&site_api(&domain, "/funnels")).await }));
    let goals = use_resource(use_reactive!(|domain| async move { api::get::<Vec<Goal>>(&site_api(&domain, "/goals")).await }));
    let mut editing = use_signal(|| None::<Editing>);
    let mut remove = use_signal(|| None::<Funnel>);
    let mut remove_busy = use_signal(|| false);

    let d_rm = domain.clone();
    let confirm_remove = move |_| {
        let Some(f) = remove() else { return };
        let path = site_api(&d_rm, &format!("/funnels/{}", f.id));
        remove_busy.set(true);
        spawn(async move {
            match api::delete(&path).await {
                Ok(()) => {
                    state::toast("Trichter gelöscht.");
                    funnels.restart();
                }
                Err(e) => state::toast_error(e.message),
            }
            remove_busy.set(false);
            remove.set(None);
        });
    };

    let goal_list: Vec<Goal> = match &*goals.read() {
        Some(Ok(g)) => g.clone(),
        _ => Vec::new(),
    };
    let goals_loaded = matches!(&*goals.read(), Some(Ok(_)));
    let no_goals = goals_loaded && goal_list.len() < MIN_STEPS;
    let data = funnels.read().clone();
    let remove_name = remove().map(|f| f.name).unwrap_or_default();

    rsx! {
        Section {
            title: "Trichter",
            description: "Trichter zeigen, wie viele Besucher eine Abfolge von Zielen durchlaufen und wo sie abspringen.",
            actions: rsx! {
                if editor {
                    Button { size: ButtonSize::Sm, disabled: no_goals || !goals_loaded, onclick: move |_| editing.set(Some(Editing(None))),
                        icons::Plus {}
                        "Trichter hinzufügen"
                    }
                }
            },
            if !editor {
                ReadOnlyNote { text: "Nur Bearbeiter und Admins können Trichter verwalten." }
            }
            if no_goals {
                div { class: "mb-4 flex flex-wrap items-center gap-2 rounded-lg border bg-muted/40 px-3 py-2 text-sm text-muted-foreground",
                    icons::Info { class: "size-4 shrink-0" }
                    span { "Für einen Trichter braucht es mindestens zwei Ziele." }
                    Link { to: Route::site_settings(&domain, "goals"), class: "font-medium text-foreground underline underline-offset-4", "Ziele anlegen" }
                }
            }
            match data {
                None => rsx! { InlineLoading {} },
                Some(Err(e)) => rsx! { ErrorState { message: e.message, on_retry: move |_| funnels.restart() } },
                Some(Ok(list)) if list.is_empty() => rsx! {
                    EmptyBox { icon: rsx! { icons::Funnel {} }, title: "Noch keine Trichter",
                        "Ein Trichter besteht aus 2 bis 8 Zielen in fester Reihenfolge, z. B. Produktseite → Warenkorb → Kauf."
                    }
                },
                Some(Ok(list)) => rsx! {
                    ul { class: "divide-y rounded-lg border",
                        for f in list {
                            li { key: "{f.id}", class: "flex flex-wrap items-center gap-3 px-3 py-2.5",
                                div { class: "min-w-0 flex-1",
                                    p { class: "truncate font-medium", "{f.name}" }
                                    p { class: "text-xs text-muted-foreground",
                                        {f.steps.iter().map(|g| g.display_name.as_str()).collect::<Vec<_>>().join(" → ")}
                                    }
                                }
                                span { class: "text-xs text-muted-foreground", "{f.steps.len()} Schritte" }
                                if editor {
                                    Button {
                                        variant: ButtonVariant::Ghost,
                                        size: ButtonSize::IconSm,
                                        aria_label: "Trichter „{f.name}“ bearbeiten",
                                        title: "Bearbeiten",
                                        disabled: !goals_loaded,
                                        onclick: {
                                            let f = f.clone();
                                            move |_| editing.set(Some(Editing(Some(f.clone()))))
                                        },
                                        icons::Pencil {}
                                    }
                                    Button {
                                        variant: ButtonVariant::Ghost,
                                        size: ButtonSize::IconSm,
                                        aria_label: "Trichter „{f.name}“ löschen",
                                        title: "Löschen",
                                        onclick: {
                                            let f = f.clone();
                                            move |_| remove.set(Some(f.clone()))
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
        if let Some(Editing(existing)) = editing() {
            FunnelModal {
                domain: domain.clone(),
                existing,
                goals: goal_list,
                on_close: move |_| editing.set(None),
                on_saved: move |_| {
                    editing.set(None);
                    funnels.restart();
                },
            }
        }
        ConfirmModal {
            open: remove().is_some(),
            on_close: move |_| remove.set(None),
            on_confirm: confirm_remove,
            title: "Trichter löschen?",
            message: "Der Trichter „{remove_name}“ wird gelöscht. Die Ziele bleiben erhalten.",
            confirm_label: "Löschen",
            busy: remove_busy(),
        }
    }
}

/// Prueft die Schritte; liefert die Ziel-IDs oder eine Fehlermeldung.
fn validate(name: &str, steps: &[String]) -> Result<Vec<i64>, String> {
    if name.trim().is_empty() {
        return Err("Bitte einen Namen eingeben.".into());
    }
    if steps.len() < MIN_STEPS {
        return Err(format!("Ein Trichter braucht mindestens {MIN_STEPS} Schritte."));
    }
    if steps.len() > MAX_STEPS {
        return Err(format!("Höchstens {MAX_STEPS} Schritte sind möglich."));
    }
    steps
        .iter()
        .enumerate()
        .map(|(i, s)| s.parse::<i64>().map_err(|_| format!("Bitte für Schritt {} ein Ziel wählen.", i + 1)))
        .collect()
}

#[component]
fn FunnelModal(domain: String, existing: Option<Funnel>, goals: Vec<Goal>, on_close: EventHandler<()>, on_saved: EventHandler<()>) -> Element {
    let edit_id = existing.as_ref().map(|f| f.id);
    let mut name = use_signal(|| existing.as_ref().map(|f| f.name.clone()).unwrap_or_default());
    let mut steps = use_signal(|| match &existing {
        Some(f) => f.steps.iter().map(|g| g.id.to_string()).collect::<Vec<_>>(),
        None => vec![String::new(); MIN_STEPS],
    });
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let submit = move |e: FormEvent| {
        e.prevent_default();
        if busy() {
            return;
        }
        let ids = match validate(&name(), &steps()) {
            Ok(ids) => ids,
            Err(msg) => {
                error.set(Some(msg));
                return;
            }
        };
        let body = NewFunnel { name: name().trim().to_string(), goal_ids: ids };
        let path = match edit_id {
            Some(id) => site_api(&domain, &format!("/funnels/{id}")),
            None => site_api(&domain, "/funnels"),
        };
        error.set(None);
        busy.set(true);
        spawn(async move {
            let res = if edit_id.is_some() { api::put::<_, Funnel>(&path, &body).await } else { api::post::<_, Funnel>(&path, &body).await };
            match res {
                Ok(f) => {
                    state::toast(format!("Trichter „{}“ gespeichert.", f.name));
                    on_saved.call(());
                }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };

    let mut options: Vec<(String, String)> = vec![(String::new(), "Ziel wählen …".into())];
    options.extend(goals.iter().map(|g| (g.id.to_string(), g.display_name.clone())));
    let current = steps();
    let count = current.len();

    rsx! {
        Modal {
            open: true,
            on_close: move |_| on_close.call(()),
            title: if edit_id.is_some() { "Trichter bearbeiten" } else { "Trichter hinzufügen" },
            size: ModalSize::Lg,
            form { class: "grid gap-4", onsubmit: submit,
                Field { label: "Name", html_for: "funnel-name",
                    Input { id: "funnel-name", value: name(), placeholder: "z. B. Kauf", autofocus: true, autocomplete: "off", oninput: move |e: FormEvent| name.set(e.value()) }
                }
                fieldset { class: "grid gap-2",
                    legend { class: "mb-1.5 text-sm font-medium", "Schritte ({count} von höchstens {MAX_STEPS})" }
                    for (i, step) in current.iter().enumerate() {
                        div { key: "{i}", class: "flex items-center gap-2",
                            span { class: "w-6 shrink-0 text-right text-sm tabular-nums text-muted-foreground", "{i + 1}." }
                            Select {
                                class: "min-w-0 flex-1",
                                id: "funnel-step-{i}",
                                value: step.clone(),
                                options: options.clone(),
                                aria_label: "Schritt {i + 1}",
                                on_change: move |v: String| {
                                    if let Some(s) = steps.write().get_mut(i) {
                                        *s = v;
                                    }
                                },
                            }
                            Button {
                                variant: ButtonVariant::Ghost,
                                size: ButtonSize::IconSm,
                                aria_label: "Schritt {i + 1} nach oben",
                                disabled: i == 0,
                                onclick: move |_| {
                                    if i > 0 {
                                        steps.write().swap(i, i - 1);
                                    }
                                },
                                icons::ArrowUp {}
                            }
                            Button {
                                variant: ButtonVariant::Ghost,
                                size: ButtonSize::IconSm,
                                aria_label: "Schritt {i + 1} nach unten",
                                disabled: i + 1 >= count,
                                onclick: move |_| {
                                    let mut s = steps.write();
                                    if i + 1 < s.len() {
                                        s.swap(i, i + 1);
                                    }
                                },
                                icons::ArrowDown {}
                            }
                            Button {
                                variant: ButtonVariant::Ghost,
                                size: ButtonSize::IconSm,
                                aria_label: "Schritt {i + 1} entfernen",
                                disabled: count <= MIN_STEPS,
                                onclick: move |_| {
                                    let mut s = steps.write();
                                    if s.len() > MIN_STEPS && i < s.len() {
                                        s.remove(i);
                                    }
                                },
                                icons::X {}
                            }
                        }
                    }
                    div {
                        Button {
                            variant: ButtonVariant::Outline,
                            size: ButtonSize::Sm,
                            disabled: count >= MAX_STEPS,
                            onclick: move |_| {
                                let mut s = steps.write();
                                if s.len() < MAX_STEPS {
                                    s.push(String::new());
                                }
                            },
                            icons::Plus {}
                            "Schritt hinzufügen"
                        }
                    }
                }
                if let Some(err) = error() {
                    p { class: "text-sm text-destructive", role: "alert", "{err}" }
                }
                div { class: "flex justify-end gap-2 pt-2",
                    Button { variant: ButtonVariant::Ghost, onclick: move |_| on_close.call(()), "Abbrechen" }
                    Button { button_type: "submit", disabled: busy(),
                        if busy() {
                            icons::LoaderCircle { class: "animate-spin" }
                        }
                        "Speichern"
                    }
                }
            }
        }
    }
}
