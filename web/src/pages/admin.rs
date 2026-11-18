//! Verwaltung der Instanz (`/admin`, nur Instanz-Admins): Konten und Einstellungen.

use crate::api;
use crate::components::common::{ConfirmModal, ErrorState, Field, PageHeader, PageSkeleton, RequireAdmin, Section};
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::modal::{Modal, ModalSize};
use crate::components::ui::switch::Switch;
use crate::components::ui::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper};
use crate::route::Route;
use crate::state;
use crate::util;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{AdminUser, InstanceSettings, NewUser, UpdateInstanceSettings, UpdateUser};

#[component]
pub fn Admin(tab: String) -> Element {
    let tab = if tab == "instance" { "instance" } else { "users" };
    let nav = navigator();
    rsx! {
        RequireAdmin {
            div { class: "grid gap-6",
                PageHeader { title: "Verwaltung", description: "Konten und Einstellungen dieser nrzAnalytics-Instanz." }
                div { class: "flex gap-1 border-b", role: "tablist",
                    for (key, label) in [("users", "Konten"), ("instance", "Instanz")] {
                        button {
                            key: "{key}",
                            role: "tab",
                            "aria-selected": "{tab == key}",
                            class: if tab == key { "-mb-px border-b-2 border-foreground px-3 py-2 text-sm font-medium" } else { "-mb-px border-b-2 border-transparent px-3 py-2 text-sm text-muted-foreground hover:text-foreground" },
                            onclick: move |_| {
                                nav.replace(Route::Admin { tab: key.to_string() });
                            },
                            "{label}"
                        }
                    }
                }
                if tab == "instance" {
                    InstanceTab {}
                } else {
                    UsersTab {}
                }
            }
        }
    }
}

#[component]
fn UsersTab() -> Element {
    let mut users = use_resource(|| async move { api::get::<Vec<AdminUser>>("/api/admin/users").await });
    let me_id = state::me().map(|m| m.id);

    // Neues Konto
    let mut email = use_signal(String::new);
    let mut name = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut is_admin = use_signal(|| false);
    let mut busy = use_signal(|| false);
    // Passwort setzen / loeschen
    let mut pw_user = use_signal(|| None::<AdminUser>);
    let mut new_pw = use_signal(String::new);
    let mut del_user = use_signal(|| None::<AdminUser>);
    let mut row_busy = use_signal(|| None::<i64>);

    let create = move |e: FormEvent| {
        e.prevent_default();
        if busy() {
            return;
        }
        let body = NewUser { email: email().trim().to_string(), name: name().trim().to_string(), password: password(), is_admin: is_admin() };
        busy.set(true);
        spawn(async move {
            match api::post::<_, AdminUser>("/api/admin/users", &body).await {
                Ok(u) => {
                    state::toast(format!("Konto {} angelegt.", u.email));
                    email.set(String::new());
                    name.set(String::new());
                    password.set(String::new());
                    is_admin.set(false);
                    users.restart();
                }
                Err(e) => state::toast_error(e.message),
            }
            busy.set(false);
        });
    };

    let mut update = move |(id, body, ok): (i64, UpdateUser, &'static str)| {
        row_busy.set(Some(id));
        spawn(async move {
            match api::put::<_, AdminUser>(&format!("/api/admin/users/{id}"), &body).await {
                Ok(_) => {
                    state::toast(ok);
                    users.restart();
                }
                Err(e) => state::toast_error(e.message),
            }
            row_busy.set(None);
        });
    };

    let save_password = move |_| {
        let Some(u) = pw_user() else { return };
        let body = UpdateUser { password: Some(new_pw()), ..Default::default() };
        pw_user.set(None);
        new_pw.set(String::new());
        update((u.id, body, "Passwort gesetzt – das Konto wurde auf allen Geräten abgemeldet."));
    };

    let confirm_delete = move |_| {
        let Some(u) = del_user() else { return };
        del_user.set(None);
        spawn(async move {
            match api::delete(&format!("/api/admin/users/{}", u.id)).await {
                Ok(()) => {
                    state::toast(format!("Konto {} gelöscht.", u.email));
                    users.restart();
                }
                Err(e) => state::toast_error(e.message),
            }
        });
    };

    let del_name = del_user().map(|u| u.email).unwrap_or_default();
    let pw_name = pw_user().map(|u| u.email).unwrap_or_default();

    rsx! {
        Section { title: "Konten", description: "Alle Konten dieser Instanz. Instanz-Admins sehen und verwalten alle Websites.",
            match &*users.read_unchecked() {
                None => rsx! { PageSkeleton {} },
                Some(Err(e)) => rsx! { ErrorState { message: e.message.clone(), on_retry: move |_| users.restart() } },
                Some(Ok(list)) => rsx! {
                    TableWrapper {
                        Table {
                            TableHeader {
                                TableRow {
                                    TableHead { "Konto" }
                                    TableHead { class: "hidden md:table-cell", "Websites" }
                                    TableHead { class: "hidden md:table-cell", "Zuletzt angemeldet" }
                                    TableHead { "Admin" }
                                    TableHead { span { class: "sr-only", "Aktionen" } }
                                }
                            }
                            TableBody {
                                for u in list.iter().cloned() {
                                    TableRow { key: "{u.id}",
                                        TableCell {
                                            div { class: "flex flex-col",
                                                span { class: "font-medium",
                                                    "{u.email}"
                                                    if Some(u.id) == me_id {
                                                        span { class: "ml-1 text-xs text-muted-foreground", "(du)" }
                                                    }
                                                }
                                                span { class: "flex items-center gap-1.5 text-xs text-muted-foreground",
                                                    if !u.name.is_empty() { "{u.name}" }
                                                    if u.totp_enabled {
                                                        Badge { variant: BadgeVariant::Secondary, class: "px-1.5 py-0 text-[10px]", "2FA" }
                                                    }
                                                }
                                            }
                                        }
                                        TableCell { class: "hidden text-muted-foreground md:table-cell", "{u.sites}" }
                                        TableCell { class: "hidden text-muted-foreground md:table-cell",
                                            match u.last_login_at {
                                                Some(ts) => rsx! { span { title: util::datetime(ts), "{util::relative(ts)}" } },
                                                None => rsx! { "nie" },
                                            }
                                        }
                                        TableCell {
                                            Switch {
                                                checked: u.is_admin,
                                                disabled: Some(u.id) == me_id || row_busy() == Some(u.id),
                                                aria_label: "Admin-Recht für {u.email}",
                                                on_change: {
                                                    let id = u.id;
                                                    move |v: bool| update((id, UpdateUser { is_admin: Some(v), ..Default::default() }, if v { "Admin-Recht erteilt." } else { "Admin-Recht entfernt." }))
                                                },
                                            }
                                        }
                                        TableCell { class: "text-right",
                                            div { class: "flex justify-end gap-1",
                                                Button {
                                                    variant: ButtonVariant::Ghost,
                                                    size: ButtonSize::IconSm,
                                                    title: "Passwort setzen",
                                                    aria_label: "Passwort für {u.email} setzen",
                                                    onclick: {
                                                        let u = u.clone();
                                                        move |_| pw_user.set(Some(u.clone()))
                                                    },
                                                    icons::KeyRound {}
                                                }
                                                if u.totp_enabled {
                                                    Button {
                                                        variant: ButtonVariant::Ghost,
                                                        size: ButtonSize::IconSm,
                                                        title: "Zwei-Faktor zurücksetzen",
                                                        aria_label: "Zwei-Faktor für {u.email} zurücksetzen",
                                                        onclick: {
                                                            let id = u.id;
                                                            move |_| update((id, UpdateUser { disable_totp: true, ..Default::default() }, "Zwei-Faktor zurückgesetzt."))
                                                        },
                                                        icons::ShieldOff {}
                                                    }
                                                }
                                                if Some(u.id) != me_id {
                                                    Button {
                                                        variant: ButtonVariant::Ghost,
                                                        size: ButtonSize::IconSm,
                                                        title: "Konto löschen",
                                                        aria_label: "Konto {u.email} löschen",
                                                        onclick: {
                                                            let u = u.clone();
                                                            move |_| del_user.set(Some(u.clone()))
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
                },
            }
        }
        Section { title: "Konto anlegen", description: "Für Personen, die Websites sehen oder verwalten sollen. Danach unter Website → Einstellungen → Personen hinzufügen.",
            form { class: "grid gap-4 md:grid-cols-2", onsubmit: create,
                Field { label: "E-Mail oder Benutzername", html_for: "new-user-email",
                    Input { id: "new-user-email", value: email(), autocomplete: "off", oninput: move |e: FormEvent| email.set(e.value()) }
                }
                Field { label: "Name", html_for: "new-user-name",
                    Input { id: "new-user-name", value: name(), oninput: move |e: FormEvent| name.set(e.value()) }
                }
                Field { label: "Passwort", html_for: "new-user-password", hint: "Mindestens 10 Zeichen.",
                    Input { id: "new-user-password", r#type: InputType::Password, value: password(), autocomplete: "new-password", oninput: move |e: FormEvent| password.set(e.value()) }
                }
                div { class: "flex items-center gap-3 md:pt-6",
                    Switch { checked: is_admin(), aria_label: "Instanz-Admin", on_change: move |v: bool| is_admin.set(v) }
                    span { class: "text-sm", "Instanz-Admin (sieht alle Websites)" }
                }
                div { class: "md:col-span-2",
                    Button { button_type: "submit", disabled: busy() || email().trim().len() < 3 || password().chars().count() < 10,
                        if busy() {
                            icons::LoaderCircle { class: "animate-spin" }
                        } else {
                            icons::UserPlus {}
                        }
                        "Konto anlegen"
                    }
                }
            }
        }
        Modal {
            open: pw_user().is_some(),
            on_close: move |_| {
                pw_user.set(None);
                new_pw.set(String::new());
            },
            title: "Passwort setzen",
            description: "Neues Passwort für {pw_name}. Alle Geräte des Kontos werden abgemeldet.",
            size: ModalSize::Sm,
            footer: rsx! {
                Button { variant: ButtonVariant::Ghost, onclick: move |_| pw_user.set(None), "Abbrechen" }
                Button { disabled: new_pw().chars().count() < 10, onclick: save_password, "Speichern" }
            },
            Field { label: "Neues Passwort", html_for: "admin-new-pw", hint: "Mindestens 10 Zeichen.",
                Input { id: "admin-new-pw", r#type: InputType::Password, autofocus: true, value: new_pw(), autocomplete: "new-password", oninput: move |e: FormEvent| new_pw.set(e.value()) }
            }
        }
        ConfirmModal {
            open: del_user().is_some(),
            on_close: move |_| del_user.set(None),
            on_confirm: confirm_delete,
            title: "Konto löschen?",
            message: "Das Konto {del_name} wird gelöscht. Websites, deren einziger Besitzer es ist, gehen an dich über.",
            confirm_label: "Löschen",
        }
    }
}

#[component]
fn InstanceTab() -> Element {
    let mut settings = use_resource(|| async move { api::get::<InstanceSettings>("/api/admin/settings").await });
    let mut url = use_signal(String::new);
    let mut url_loaded = use_signal(|| false);
    let mut busy = use_signal(|| false);

    if let Some(Ok(s)) = &*settings.read_unchecked()
        && !url_loaded() {
            url.set(s.public_url.clone());
            url_loaded.set(true);
        }

    let mut save = move |body: UpdateInstanceSettings| {
        busy.set(true);
        spawn(async move {
            match api::put::<_, InstanceSettings>("/api/admin/settings", &body).await {
                Ok(_) => {
                    state::toast("Gespeichert.");
                    url_loaded.set(false);
                    settings.restart();
                }
                Err(e) => state::toast_error(e.message),
            }
            busy.set(false);
        });
    };

    rsx! {
        match &*settings.read_unchecked() {
            None => rsx! { PageSkeleton {} },
            Some(Err(e)) => rsx! { ErrorState { message: e.message.clone(), on_retry: move |_| settings.restart() } },
            Some(Ok(s)) => {
                let s = s.clone();
                rsx! {
                    Section { title: "Öffentliche Adresse", description: "Unter dieser Adresse erreichen Websites das Tracking-Skript. Sie steht in Snippets und geteilten Links (z. B. https://analytics.example.com – über NetBird/Cloudflare auf Port 8581).",
                        form {
                            class: "flex flex-col gap-3 sm:flex-row sm:items-end",
                            onsubmit: move |e: FormEvent| {
                                e.prevent_default();
                                save(UpdateInstanceSettings { public_url: Some(url().trim().to_string()), ..Default::default() });
                            },
                            div { class: "flex-1",
                                Field { label: "Öffentliche URL", html_for: "public-url", hint: "Leer lassen, um die Adresse der Anfrage zu nehmen.",
                                    Input { id: "public-url", value: url(), placeholder: "https://analytics.example.com", oninput: move |e: FormEvent| url.set(e.value()) }
                                }
                            }
                            Button { button_type: "submit", disabled: busy(), class: "sm:mb-5", "Speichern" }
                        }
                    }
                    Section { title: "Registrierung", description: "Ist sie offen, kann sich jeder ein Konto anlegen. Sonst legen Admins Konten an.",
                        div { class: "flex items-center gap-3",
                            Switch {
                                checked: s.registration_open,
                                disabled: busy(),
                                aria_label: "Registrierung offen",
                                on_change: move |v: bool| save(UpdateInstanceSettings { registration_open: Some(v), ..Default::default() }),
                            }
                            span { class: "text-sm", if s.registration_open { "Offen" } else { "Geschlossen" } }
                        }
                    }
                    Section { title: "Länderzuordnung (GeoIP)", description: "Länder kommen aus der IP-Adresse; die Adresse selbst wird nie gespeichert.",
                        div { class: "grid gap-2 text-sm",
                            div { class: "flex items-center gap-2",
                                if s.geoip.enabled {
                                    Badge { variant: BadgeVariant::Secondary, icons::Check { class: "size-3" } "Aktiv" }
                                } else {
                                    Badge { variant: BadgeVariant::Outline, "Inaktiv" }
                                }
                                span { class: "text-muted-foreground", "{s.geoip.message}" }
                            }
                            if let Some(db) = &s.geoip.database {
                                p { class: "text-muted-foreground", "Datenbank: {db}"
                                    if let Some(d) = &s.geoip.build_date { " · Stand {d}" }
                                    if s.geoip.city_level { " · mit Regionen und Städten" } else { " · nur Länder" }
                                }
                            }
                            p { class: "text-xs text-muted-foreground", "IP-Geolocation von DB-IP (db-ip.com), Lizenz CC BY 4.0. Für Regionen und Städte NRZANALYTICS_GEOIP=dbip-city setzen." }
                        }
                    }
                    Section { title: "Datenbank",
                        div { class: "grid gap-4 sm:grid-cols-2",
                            div {
                                p { class: "text-xs text-muted-foreground", "Gespeicherte Ereignisse" }
                                p { class: "text-2xl font-semibold tabular-nums", "{nrzanalytics_shared::labels::number_full(s.total_events as f64)}" }
                            }
                            div {
                                p { class: "text-xs text-muted-foreground", "Grösse (SQLite)" }
                                p { class: "text-2xl font-semibold tabular-nums", "{util::bytes(s.database_bytes)}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
