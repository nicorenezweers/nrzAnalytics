//! Abschnitt „Personen“: Mitglieder mit Rollen verwalten.

use super::helpers::{InlineLoading, ReadOnlyNote, can_admin, site_api};
use crate::api;
use crate::components::common::{ConfirmModal, ErrorState, Field, Section};
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::Input;
use crate::components::ui::select::Select;
use crate::components::ui::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper};
use crate::state;
use crate::util;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{Member, NewMember, Role, UpdateMember};

/// Rollen, die vergeben werden koennen (Besitzer wird nicht uebertragen).
fn role_options() -> Vec<(String, String)> {
    [Role::Admin, Role::Editor, Role::Viewer].into_iter().map(|r| (r.as_str().to_string(), r.label().to_string())).collect()
}

#[component]
pub fn PeopleSection(domain: String, role: Option<Role>) -> Element {
    let admin = can_admin(role);
    let mut members = use_resource(use_reactive!(|domain| async move { api::get::<Vec<Member>>(&site_api(&domain, "/members")).await }));
    let me_id = state::me().map(|m| m.id);

    // Hinzufuegen
    let mut email = use_signal(String::new);
    let mut new_role = use_signal(|| Role::Viewer);
    let mut add_busy = use_signal(|| false);
    // Rolle aendern / entfernen
    let mut row_busy = use_signal(|| None::<i64>);
    let mut remove = use_signal(|| None::<Member>);
    let mut remove_busy = use_signal(|| false);

    let d_add = domain.clone();
    let add = move |e: FormEvent| {
        e.prevent_default();
        let who = email().trim().to_string();
        if who.is_empty() || add_busy() {
            return;
        }
        let path = site_api(&d_add, "/members");
        let body = NewMember { email: who, role: new_role() };
        add_busy.set(true);
        spawn(async move {
            match api::post::<_, Member>(&path, &body).await {
                Ok(m) => {
                    state::toast(format!("{} wurde hinzugefügt.", if m.name.is_empty() { &m.email } else { &m.name }));
                    email.set(String::new());
                    members.restart();
                }
                Err(e) => state::toast_error(e.message),
            }
            add_busy.set(false);
        });
    };

    let d_role = domain.clone();
    let change_role = move |(user_id, r): (i64, Role)| {
        let path = site_api(&d_role, &format!("/members/{user_id}"));
        row_busy.set(Some(user_id));
        spawn(async move {
            match api::put::<_, Member>(&path, &UpdateMember { role: r }).await {
                Ok(_) => state::toast("Rolle geändert."),
                Err(e) => state::toast_error(e.message),
            }
            row_busy.set(None);
            members.restart();
        });
    };

    let d_rm = domain.clone();
    let confirm_remove = move |_| {
        let Some(m) = remove() else { return };
        let path = site_api(&d_rm, &format!("/members/{}", m.user_id));
        remove_busy.set(true);
        spawn(async move {
            match api::delete(&path).await {
                Ok(()) => {
                    state::toast("Mitglied entfernt.");
                    members.restart();
                }
                Err(e) => state::toast_error(e.message),
            }
            remove_busy.set(false);
            remove.set(None);
        });
    };

    let data = members.read().clone();
    let remove_name = remove().map(|m| if m.name.is_empty() { m.email } else { m.name }).unwrap_or_default();

    rsx! {
        if !admin {
            ReadOnlyNote { text: "Nur Admins können Personen verwalten." }
        }
        Section { title: "Personen", description: "Wer diese Website sehen oder verwalten darf.",
            match data {
                None => rsx! { InlineLoading {} },
                Some(Err(e)) => rsx! { ErrorState { message: e.message, on_retry: move |_| members.restart() } },
                Some(Ok(list)) => rsx! {
                    TableWrapper {
                        Table {
                            TableHeader {
                                TableRow {
                                    TableHead { "Name" }
                                    TableHead { "E-Mail" }
                                    TableHead { "Rolle" }
                                    TableHead { class: "hidden sm:table-cell", "Seit" }
                                    TableHead { span { class: "sr-only", "Aktionen" } }
                                }
                            }
                            TableBody {
                                for m in list {
                                    TableRow { key: "{m.user_id}",
                                        TableCell { class: "font-medium",
                                            if m.name.is_empty() { "–" } else { "{m.name}" }
                                            if Some(m.user_id) == me_id {
                                                span { class: "ml-1 text-xs text-muted-foreground", "(du)" }
                                            }
                                        }
                                        TableCell { class: "text-muted-foreground", "{m.email}" }
                                        TableCell {
                                            if m.role == Role::Owner || !admin {
                                                Badge { variant: if m.role == Role::Owner { BadgeVariant::Accent } else { BadgeVariant::Secondary }, "{m.role.label()}" }
                                            } else {
                                                Select {
                                                    class: "w-36",
                                                    value: m.role.as_str(),
                                                    options: role_options(),
                                                    aria_label: "Rolle von {m.email}",
                                                    disabled: row_busy() == Some(m.user_id),
                                                    on_change: {
                                                        let uid = m.user_id;
                                                        let mut change_role = change_role.clone();
                                                        move |v: String| {
                                                            if let Some(r) = Role::parse(&v) {
                                                                change_role((uid, r));
                                                            }
                                                        }
                                                    },
                                                }
                                            }
                                        }
                                        TableCell { class: "hidden text-muted-foreground sm:table-cell",
                                            span { title: util::datetime(m.created_at), "{util::date_of_ts(m.created_at)}" }
                                        }
                                        TableCell { class: "text-right",
                                            if admin && m.role != Role::Owner {
                                                Button {
                                                    variant: ButtonVariant::Ghost,
                                                    size: ButtonSize::IconSm,
                                                    aria_label: "{m.email} entfernen",
                                                    title: "Entfernen",
                                                    onclick: {
                                                        let m = m.clone();
                                                        move |_| remove.set(Some(m.clone()))
                                                    },
                                                    icons::Trash2 {}
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
        if admin {
            Section { title: "Person hinzufügen", description: "Das Konto muss auf dieser Instanz bereits existieren.",
                form { class: "grid gap-4 sm:grid-cols-[minmax(0,1fr)_10rem_auto] sm:items-end", onsubmit: add,
                    Field { label: "E-Mail oder Benutzername", html_for: "member-email",
                        Input {
                            id: "member-email",
                            value: email(),
                            placeholder: "name@example.com",
                            autocomplete: "off",
                            oninput: move |e: FormEvent| email.set(e.value()),
                        }
                    }
                    Field { label: "Rolle", html_for: "member-role",
                        Select {
                            id: "member-role",
                            value: new_role().as_str(),
                            options: role_options(),
                            on_change: move |v: String| {
                                if let Some(r) = Role::parse(&v) {
                                    new_role.set(r);
                                }
                            },
                        }
                    }
                    Button { button_type: "submit", disabled: add_busy() || email().trim().is_empty(),
                        if add_busy() {
                            icons::LoaderCircle { class: "animate-spin" }
                        } else {
                            icons::Plus {}
                        }
                        "Hinzufügen"
                    }
                }
                p { class: "mt-3 text-xs text-muted-foreground",
                    "Betrachter sehen nur Statistiken. Bearbeiter verwalten zusätzlich Ziele, Trichter, Eigenschaften, Schutzregeln und geteilte Links. Admins dürfen außerdem Personen, Sichtbarkeit und allgemeine Einstellungen ändern."
                }
            }
        }
        ConfirmModal {
            open: remove().is_some(),
            on_close: move |_| remove.set(None),
            on_confirm: confirm_remove,
            title: "Person entfernen?",
            message: "{remove_name} verliert den Zugriff auf diese Website.",
            confirm_label: "Entfernen",
            busy: remove_busy(),
        }
    }
}
