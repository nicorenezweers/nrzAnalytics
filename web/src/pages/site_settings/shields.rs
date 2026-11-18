//! Abschnitt „Schutzregeln“: IP-Adressen, Länder, Seiten blockieren bzw. Hostnamen erlauben.

use super::helpers::{EmptyBox, InlineLoading, ReadOnlyNote, can_edit, site_api};
use crate::api;
use crate::components::common::{ConfirmModal, ErrorState, Field, Section};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::Input;
use crate::components::ui::select::Select;
use crate::components::ui::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper};
use crate::state;
use crate::util;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{NewShield, Role, Shield, ShieldKind};
use nrzanalytics_shared::geo;

/// Erklaerung je Art.
fn kind_help(kind: ShieldKind) -> &'static str {
    match kind {
        ShieldKind::Ip => "Besuche von diesen IP-Adressen oder Netzen werden nicht erfasst, z. B. 203.0.113.7 oder 2001:db8::/32.",
        ShieldKind::Country => "Besuche aus diesen Ländern werden nicht erfasst.",
        ShieldKind::Page => "Aufrufe dieser Seiten werden nicht erfasst. Platzhalter: * (beliebige Zeichen außer /) und ** (alles), z. B. /admin/**.",
        ShieldKind::Hostname => "Nur Besuche auf diesen Hostnamen werden erfasst. Ohne Eintrag werden alle Hostnamen erfasst. Platzhalter: *.example.com.",
    }
}

/// Reiter-Beschriftung (wie `ShieldKind::label`, aber mit echten Umlauten).
fn tab_label(kind: ShieldKind) -> &'static str {
    match kind {
        ShieldKind::Country => "Länder",
        k => k.label(),
    }
}

fn value_label(kind: ShieldKind) -> &'static str {
    match kind {
        ShieldKind::Ip => "IP-Adresse oder CIDR",
        ShieldKind::Country => "Land",
        ShieldKind::Page => "Pfad",
        ShieldKind::Hostname => "Hostname",
    }
}

fn placeholder(kind: ShieldKind) -> &'static str {
    match kind {
        ShieldKind::Ip => "203.0.113.0/24",
        ShieldKind::Country => "",
        ShieldKind::Page => "/admin/**",
        ShieldKind::Hostname => "*.example.com",
    }
}

/// Anzeigename eines Laendercodes mit Flagge.
fn country_display(code: &str) -> String {
    match geo::country_name(code) {
        Some(n) => format!("{} {}", geo::flag(code), n),
        None => code.to_string(),
    }
}

#[component]
pub fn ShieldsSection(domain: String, role: Option<Role>) -> Element {
    let editor = can_edit(role);
    let mut shields = use_resource(use_reactive!(|domain| async move { api::get::<Vec<Shield>>(&site_api(&domain, "/shields")).await }));
    let mut active = use_signal(|| ShieldKind::Ip);
    let mut remove = use_signal(|| None::<Shield>);
    let mut remove_busy = use_signal(|| false);

    let d_rm = domain.clone();
    let confirm_remove = move |_| {
        let Some(s) = remove() else { return };
        let path = site_api(&d_rm, &format!("/shields/{}", s.id));
        remove_busy.set(true);
        spawn(async move {
            match api::delete(&path).await {
                Ok(()) => {
                    state::toast("Schutzregel gelöscht.");
                    shields.restart();
                }
                Err(e) => state::toast_error(e.message),
            }
            remove_busy.set(false);
            remove.set(None);
        });
    };

    let kind = active();
    let data = shields.read().clone();
    let count = |k: ShieldKind| match &data {
        Some(Ok(l)) => l.iter().filter(|s| s.kind == k).count(),
        _ => 0,
    };
    let remove_value = remove().map(|s| if s.kind == ShieldKind::Country { country_display(&s.value) } else { s.value }).unwrap_or_default();

    rsx! {
        Section { title: "Schutzregeln", description: "Bestimmte Besuche von der Erfassung ausschließen.",
            if !editor {
                ReadOnlyNote { text: "Nur Bearbeiter und Admins können Schutzregeln verwalten." }
            }
            div { role: "tablist", "aria-label": "Art der Schutzregel", class: "mb-4 flex gap-1 overflow-x-auto rounded-lg bg-muted p-1",
                for k in ShieldKind::ALL {
                    button {
                        key: "{k.as_str()}",
                        r#type: "button",
                        role: "tab",
                        id: "shield-tab-{k.as_str()}",
                        "aria-selected": "{k == kind}",
                        "aria-controls": "shield-panel",
                        class: if k == kind {
                            "flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md bg-background px-3 py-1.5 text-sm font-medium shadow-sm"
                        } else {
                            "flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md px-3 py-1.5 text-sm text-muted-foreground hover:text-foreground"
                        },
                        onclick: move |_| active.set(k),
                        "{tab_label(k)}"
                        if count(k) > 0 {
                            span { class: "rounded bg-secondary px-1.5 text-xs tabular-nums text-secondary-foreground", "{count(k)}" }
                        }
                    }
                }
            }
            div { id: "shield-panel", role: "tabpanel", "aria-labelledby": "shield-tab-{kind.as_str()}", class: "grid gap-4",
                p { class: "text-sm text-muted-foreground", "{kind_help(kind)}" }
                match data {
                    None => rsx! { InlineLoading {} },
                    Some(Err(e)) => rsx! { ErrorState { message: e.message, on_retry: move |_| shields.restart() } },
                    Some(Ok(list)) => {
                        let rows: Vec<Shield> = list.into_iter().filter(|s| s.kind == kind).collect();
                        if rows.is_empty() {
                            rsx! {
                                EmptyBox { icon: rsx! { icons::Shield {} }, title: "Keine Regeln",
                                    if kind == ShieldKind::Hostname { "Alle Hostnamen werden erfasst." } else { "Für diese Art gibt es noch keine Regeln." }
                                }
                            }
                        } else {
                            rsx! {
                                TableWrapper {
                                    Table {
                                        TableHeader {
                                            TableRow {
                                                TableHead { "{value_label(kind)}" }
                                                TableHead { "Beschreibung" }
                                                TableHead { class: "hidden sm:table-cell", "Angelegt" }
                                                TableHead { span { class: "sr-only", "Aktionen" } }
                                            }
                                        }
                                        TableBody {
                                            for s in rows {
                                                TableRow { key: "{s.id}",
                                                    TableCell { class: "font-mono text-xs",
                                                        if s.kind == ShieldKind::Country {
                                                            span { class: "font-sans text-sm", "{country_display(&s.value)}" }
                                                        } else {
                                                            "{s.value}"
                                                        }
                                                    }
                                                    TableCell { class: "text-muted-foreground",
                                                        if s.description.is_empty() { "–" } else { "{s.description}" }
                                                    }
                                                    TableCell { class: "hidden text-muted-foreground sm:table-cell",
                                                        span { title: util::datetime(s.created_at), "{util::date_of_ts(s.created_at)}" }
                                                    }
                                                    TableCell { class: "text-right",
                                                        if editor {
                                                            Button {
                                                                variant: ButtonVariant::Ghost,
                                                                size: ButtonSize::IconSm,
                                                                aria_label: "Regel {s.value} löschen",
                                                                title: "Löschen",
                                                                onclick: {
                                                                    let s = s.clone();
                                                                    move |_| remove.set(Some(s.clone()))
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
                            }
                        }
                    }
                }
                if editor {
                    ShieldForm { key: "{kind.as_str()}", domain: domain.clone(), kind, on_added: move |_| shields.restart() }
                }
            }
        }
        ConfirmModal {
            open: remove().is_some(),
            on_close: move |_| remove.set(None),
            on_confirm: confirm_remove,
            title: "Schutzregel löschen?",
            message: "Die Regel „{remove_value}“ wird gelöscht und gilt ab sofort nicht mehr.",
            confirm_label: "Löschen",
            busy: remove_busy(),
        }
    }
}

/// Formular zum Hinzufuegen einer Regel der gewaehlten Art.
#[component]
fn ShieldForm(domain: String, kind: ShieldKind, on_added: EventHandler<()>) -> Element {
    let mut value = use_signal(String::new);
    let mut description = use_signal(String::new);
    let mut busy = use_signal(|| false);

    let submit = move |e: FormEvent| {
        e.prevent_default();
        let v = value().trim().to_string();
        if v.is_empty() || busy() {
            return;
        }
        let body = NewShield { kind, value: v, description: description().trim().to_string() };
        let path = site_api(&domain, "/shields");
        busy.set(true);
        spawn(async move {
            match api::post::<_, Shield>(&path, &body).await {
                Ok(_) => {
                    state::toast("Schutzregel hinzugefügt.");
                    value.set(String::new());
                    description.set(String::new());
                    on_added.call(());
                }
                Err(e) => state::toast_error(e.message),
            }
            busy.set(false);
        });
    };

    let country_options = move || {
        let mut o: Vec<(String, String)> = vec![(String::new(), "Land wählen …".into())];
        o.extend(geo::COUNTRIES.iter().map(|(c, n)| (c.to_string(), format!("{} {}", geo::flag(c), n))));
        o
    };

    rsx! {
        form { class: "grid gap-4 border-t pt-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] sm:items-end", onsubmit: submit,
            Field { label: value_label(kind), html_for: "shield-value",
                if kind == ShieldKind::Country {
                    Select { id: "shield-value", value: value(), options: country_options(), on_change: move |v: String| value.set(v) }
                } else {
                    Input {
                        id: "shield-value",
                        value: value(),
                        placeholder: placeholder(kind),
                        autocomplete: "off",
                        oninput: move |e: FormEvent| value.set(e.value()),
                    }
                }
            }
            Field { label: "Beschreibung (optional)", html_for: "shield-description",
                Input {
                    id: "shield-description",
                    value: description(),
                    placeholder: "z. B. Büro",
                    autocomplete: "off",
                    oninput: move |e: FormEvent| description.set(e.value()),
                }
            }
            Button { button_type: "submit", disabled: busy() || value().trim().is_empty(),
                if busy() {
                    icons::LoaderCircle { class: "animate-spin" }
                } else {
                    icons::Plus {}
                }
                "Hinzufügen"
            }
        }
    }
}
