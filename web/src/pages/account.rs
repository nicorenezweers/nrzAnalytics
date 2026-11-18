//! Kontoeinstellungen (`/settings`): Konto, Passwort, Zwei-Faktor, API-Schlüssel.

use crate::api;
use crate::components::common::{ConfirmModal, CopyField, ErrorState, Field, PageHeader, RequireLogin, Section};
use crate::components::ui::alert::{Alert, AlertDescription, AlertTitle};
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::modal::Modal;
use crate::components::ui::select::Select;
use crate::components::ui::skeleton::Skeleton;
use crate::components::ui::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};
use crate::route::Route;
use crate::state;
use crate::util;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{
    ApiKey, ApiScope, ChangePassword, CreatedApiKey, Me, NewApiKey, RecoveryCodes, TotpDisable, TotpEnable, TotpSetup, UpdateAccount,
};

/// Mindestlänge für Passwörter.
const MIN_PASSWORD: usize = 8;

/// Reiter: (Schlüssel, Beschriftung).
const TABS: [(&str, &str); 4] = [("account", "Konto"), ("password", "Passwort"), ("2fa", "Zwei-Faktor"), ("api", "API-Schlüssel")];

#[component]
pub fn Account(tab: String) -> Element {
    let active = if TABS.iter().any(|(k, _)| *k == tab) { tab } else { "account".to_string() };
    rsx! {
        RequireLogin {
            PageHeader { title: "Einstellungen", description: "Dein Konto, Sicherheit und Zugänge für die API." }
            div { class: "grid gap-6 md:grid-cols-[13rem_1fr]",
                nav { class: "-mx-1 flex gap-1 overflow-x-auto px-1 md:flex-col md:overflow-visible", "aria-label": "Einstellungen",
                    for (key, label) in TABS {
                        TabLink { key: "{key}", tab: key, label, active: active == key }
                    }
                }
                div { class: "min-w-0",
                    match active.as_str() {
                        "password" => rsx! { PasswordTab {} },
                        "2fa" => rsx! { TwoFactorTab {} },
                        "api" => rsx! { ApiKeysTab {} },
                        _ => rsx! { AccountTab {} },
                    }
                }
            }
        }
    }
}

#[component]
fn TabLink(tab: &'static str, label: &'static str, active: bool) -> Element {
    let class = if active {
        "flex shrink-0 items-center gap-2 rounded-md bg-accent px-3 py-2 text-sm font-medium text-accent-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
    } else {
        "flex shrink-0 items-center gap-2 rounded-md px-3 py-2 text-sm text-muted-foreground outline-none hover:bg-accent/50 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
    };
    rsx! {
        Link { to: Route::Account { tab: tab.to_string() }, class, "aria-current": if active { "page" } else { "false" },
            match tab {
                "password" => rsx! { icons::Lock { class: "size-4" } },
                "2fa" => rsx! { icons::ShieldCheck { class: "size-4" } },
                "api" => rsx! { icons::KeyRound { class: "size-4" } },
                _ => rsx! { icons::User { class: "size-4" } },
            }
            "{label}"
        }
    }
}

/// Angemeldetes Konto ändern und im globalen Zustand speichern.
fn update_me(f: impl FnOnce(&mut Me)) {
    let current = state::STATUS.peek().as_ref().and_then(|s| s.user.clone());
    if let Some(mut m) = current {
        f(&mut m);
        state::set_me(Some(m));
    }
}

/// Lade-Symbol für Knöpfe.
#[component]
fn Spinner(busy: bool) -> Element {
    rsx! {
        if busy {
            icons::LoaderCircle { class: "animate-spin" }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Konto
// ---------------------------------------------------------------------------------------------

#[component]
fn AccountTab() -> Element {
    let initial = state::me();
    let mut name = use_signal(|| initial.as_ref().map(|m| m.name.clone()).unwrap_or_default());
    let mut email = use_signal(|| initial.as_ref().map(|m| m.email.clone()).unwrap_or_default());
    let mut busy = use_signal(|| false);
    rsx! {
        Section { title: "Konto", description: "Name und E-Mail-Adresse für die Anmeldung.",
            form {
                class: "grid max-w-md gap-4",
                onsubmit: move |e: FormEvent| {
                    e.prevent_default();
                    if busy() {
                        return;
                    }
                    let body = UpdateAccount { email: email().trim().to_string(), name: name().trim().to_string() };
                    if body.email.is_empty() {
                        state::toast_error("Bitte eine E-Mail-Adresse angeben.");
                        return;
                    }
                    busy.set(true);
                    spawn(async move {
                        match api::put::<_, Me>("/api/account", &body).await {
                            Ok(me) => {
                                state::set_me(Some(me));
                                state::toast("Konto gespeichert.");
                            }
                            Err(e) => state::toast_error(e.message),
                        }
                        busy.set(false);
                    });
                },
                Field { label: "Name", html_for: "acc-name",
                    Input { id: "acc-name", value: name(), autocomplete: "name", oninput: move |e: FormEvent| name.set(e.value()) }
                }
                Field { label: "E-Mail", html_for: "acc-email",
                    Input { id: "acc-email", r#type: InputType::Email, value: email(), required: true, autocomplete: "email", oninput: move |e: FormEvent| email.set(e.value()) }
                }
                div {
                    Button { button_type: "submit", disabled: busy(),
                        Spinner { busy: busy() }
                        "Speichern"
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Passwort
// ---------------------------------------------------------------------------------------------

#[component]
fn PasswordTab() -> Element {
    let mut current = use_signal(String::new);
    let mut new = use_signal(String::new);
    let mut repeat = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mismatch = !repeat().is_empty() && new() != repeat();
    rsx! {
        Section { title: "Passwort ändern", description: "Nach der Änderung bleiben andere Geräte nicht angemeldet.",
            form {
                class: "grid max-w-md gap-4",
                onsubmit: move |e: FormEvent| {
                    e.prevent_default();
                    if busy() {
                        return;
                    }
                    if new().chars().count() < MIN_PASSWORD {
                        state::toast_error(format!("Das neue Passwort braucht mindestens {MIN_PASSWORD} Zeichen."));
                        return;
                    }
                    if new() != repeat() {
                        state::toast_error("Die Passwörter stimmen nicht überein.");
                        return;
                    }
                    let body = ChangePassword { current: current(), new: new() };
                    busy.set(true);
                    spawn(async move {
                        match api::put::<_, ()>("/api/account/password", &body).await {
                            Ok(()) => {
                                current.set(String::new());
                                new.set(String::new());
                                repeat.set(String::new());
                                state::toast("Passwort geändert.");
                            }
                            Err(e) => state::toast_error(e.message),
                        }
                        busy.set(false);
                    });
                },
                Field { label: "Aktuelles Passwort", html_for: "pw-current",
                    Input { id: "pw-current", r#type: InputType::Password, value: current(), required: true, autocomplete: "current-password", oninput: move |e: FormEvent| current.set(e.value()) }
                }
                Field { label: "Neues Passwort", html_for: "pw-new", hint: "Mindestens {MIN_PASSWORD} Zeichen.",
                    Input { id: "pw-new", r#type: InputType::Password, value: new(), required: true, autocomplete: "new-password", oninput: move |e: FormEvent| new.set(e.value()) }
                }
                Field { label: "Neues Passwort wiederholen", html_for: "pw-repeat",
                    Input { id: "pw-repeat", r#type: InputType::Password, value: repeat(), required: true, autocomplete: "new-password", oninput: move |e: FormEvent| repeat.set(e.value()) }
                    if mismatch {
                        p { class: "text-xs text-destructive", "Die Passwörter stimmen nicht überein." }
                    }
                }
                div {
                    Button { button_type: "submit", disabled: busy(),
                        Spinner { busy: busy() }
                        "Passwort ändern"
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Zwei-Faktor
// ---------------------------------------------------------------------------------------------

/// Geheimnis in 4er-Gruppen zum Abtippen.
fn group4(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().filter(|c| !c.is_whitespace()).collect();
    chars.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

#[component]
fn TwoFactorTab() -> Element {
    let enabled = state::me().is_some_and(|m| m.totp_enabled);
    let mut setup = use_signal(|| None::<TotpSetup>);
    let mut codes = use_signal(|| None::<Vec<String>>);
    let mut busy = use_signal(|| false);

    let start_setup = move |_| {
        if busy() {
            return;
        }
        busy.set(true);
        spawn(async move {
            match api::post_empty::<TotpSetup>("/api/account/2fa/setup").await {
                Ok(s) => setup.set(Some(s)),
                Err(e) => state::toast_error(e.message),
            }
            busy.set(false);
        });
    };

    let status = rsx! {
        div { class: "flex items-center gap-2 text-sm",
            span { class: "text-muted-foreground", "Status:" }
            if enabled {
                Badge { variant: BadgeVariant::Success, "Aktiv" }
            } else {
                Badge { variant: BadgeVariant::Muted, "Nicht aktiv" }
            }
        }
    };

    rsx! {
        Section {
            title: "Zwei-Faktor-Anmeldung",
            description: "Zusätzlich zum Passwort ein Code aus einer Authenticator-App (z. B. Aegis, 2FAS, 1Password).",
            actions: status,
            if let Some(list) = codes() {
                RecoveryCodesView { codes: list, on_done: move |_| codes.set(None) }
            } else if enabled {
                DisableTotp {}
            } else if let Some(s) = setup() {
                EnableTotp {
                    setup: s,
                    on_cancel: move |_| setup.set(None),
                    on_enabled: move |list: Vec<String>| {
                        setup.set(None);
                        codes.set(Some(list));
                        update_me(|m| m.totp_enabled = true);
                    },
                }
            } else {
                div { class: "grid gap-4",
                    p { class: "max-w-prose text-sm text-muted-foreground",
                        "Schützt dein Konto, falls jemand dein Passwort kennt. Du brauchst eine App, die TOTP-Codes erzeugt."
                    }
                    div {
                        Button { onclick: start_setup, disabled: busy(),
                            Spinner { busy: busy() }
                            icons::ShieldCheck {}
                            "Zwei-Faktor einrichten"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn EnableTotp(setup: TotpSetup, on_cancel: EventHandler<()>, on_enabled: EventHandler<Vec<String>>) -> Element {
    let mut code = use_signal(String::new);
    let mut busy = use_signal(|| false);
    rsx! {
        div { class: "grid gap-6",
            div { class: "grid gap-6 sm:grid-cols-[auto_1fr] sm:items-start",
                div { class: "grid justify-items-center gap-2",
                    QrCodeSvg { data: setup.otpauth_url.clone() }
                    p { class: "text-xs text-muted-foreground", "Mit der App scannen" }
                }
                div { class: "grid min-w-0 gap-3 text-sm",
                    p { "1. QR-Code mit der Authenticator-App scannen." }
                    div { class: "grid gap-1.5",
                        p { class: "text-muted-foreground", "Kein Scanner? Dieses Geheimnis von Hand eingeben:" }
                        CopyField { value: group4(&setup.secret), aria_label: "Geheimnis" }
                    }
                    p { "2. Den angezeigten 6-stelligen Code unten eingeben." }
                }
            }
            form {
                class: "grid max-w-xs gap-3",
                onsubmit: move |e: FormEvent| {
                    e.prevent_default();
                    if busy() {
                        return;
                    }
                    let c: String = code().chars().filter(|c| !c.is_whitespace()).collect();
                    if c.is_empty() {
                        state::toast_error("Bitte den Code aus der App eingeben.");
                        return;
                    }
                    busy.set(true);
                    spawn(async move {
                        match api::post::<_, RecoveryCodes>("/api/account/2fa/enable", &TotpEnable { code: c }).await {
                            Ok(r) => {
                                state::toast("Zwei-Faktor-Anmeldung ist aktiv.");
                                on_enabled.call(r.codes);
                            }
                            Err(e) => state::toast_error(e.message),
                        }
                        busy.set(false);
                    });
                },
                Field { label: "Code aus der App", html_for: "totp-code",
                    Input {
                        id: "totp-code",
                        class: "font-mono tracking-widest",
                        value: code(),
                        inputmode: "numeric",
                        autocomplete: "one-time-code",
                        placeholder: "123456",
                        maxlength: 10,
                        required: true,
                        oninput: move |e: FormEvent| code.set(e.value()),
                    }
                }
                div { class: "flex flex-wrap gap-2",
                    Button { button_type: "submit", disabled: busy(),
                        Spinner { busy: busy() }
                        "Aktivieren"
                    }
                    Button { variant: ButtonVariant::Ghost, onclick: move |_| on_cancel.call(()), "Abbrechen" }
                }
            }
        }
    }
}

/// QR-Code als SVG direkt in rsx!: schwarze Module auf weissem Grund (für Scanner lesbar).
#[component]
fn QrCodeSvg(data: String) -> Element {
    let Ok(code) = qrcode::QrCode::new(data.as_bytes()) else {
        return rsx! {
            p { class: "text-sm text-destructive", "QR-Code konnte nicht erzeugt werden – bitte das Geheimnis abtippen." }
        };
    };
    let width = code.width();
    let colors = code.to_colors();
    const QUIET: usize = 4;
    let size = width + 2 * QUIET;
    // Waagrechte Folgen dunkler Module zu einem Rechteck zusammenfassen (weniger Elemente).
    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    for y in 0..width {
        let mut x = 0;
        while x < width {
            if colors.get(y * width + x) == Some(&qrcode::Color::Dark) {
                let start = x;
                while x < width && colors.get(y * width + x) == Some(&qrcode::Color::Dark) {
                    x += 1;
                }
                runs.push((start + QUIET, y + QUIET, x - start));
            } else {
                x += 1;
            }
        }
    }
    rsx! {
        svg {
            class: "size-48 rounded-lg bg-white",
            view_box: "0 0 {size} {size}",
            shape_rendering: "crispEdges",
            role: "img",
            "aria-label": "QR-Code für die Authenticator-App",
            for (i, (x, y, w)) in runs.into_iter().enumerate() {
                rect { key: "{i}", x: "{x}", y: "{y}", width: "{w}", height: "1", class: "fill-black" }
            }
        }
    }
}

#[component]
fn RecoveryCodesView(codes: Vec<String>, on_done: EventHandler<()>) -> Element {
    let all = codes.join("\n");
    rsx! {
        div { class: "grid gap-4",
            Alert { class: "border-warning/40 bg-warning/10",
                icons::TriangleAlert { class: "text-warning" }
                AlertTitle { "Wiederherstellungscodes sichern" }
                AlertDescription {
                    "Diese Codes sind nur jetzt sichtbar. Jeder Code funktioniert einmal, falls du keinen Zugriff auf die App hast. Bewahre sie sicher auf, z. B. im Passwort-Manager."
                }
            }
            ul { class: "grid grid-cols-2 gap-2 sm:grid-cols-3", "aria-label": "Wiederherstellungscodes",
                for c in codes.iter() {
                    li { key: "{c}", class: "rounded-md border bg-muted/40 px-3 py-2 text-center font-mono text-sm select-all", "{c}" }
                }
            }
            div { class: "flex flex-wrap gap-2",
                Button { variant: ButtonVariant::Outline, onclick: move |_| util::copy(&all),
                    icons::Copy {}
                    "Alle kopieren"
                }
                Button { onclick: move |_| on_done.call(()), "Ich habe die Codes gesichert" }
            }
        }
    }
}

#[component]
fn DisableTotp() -> Element {
    let mut password = use_signal(String::new);
    let mut busy = use_signal(|| false);
    rsx! {
        form {
            class: "grid max-w-md gap-4",
            onsubmit: move |e: FormEvent| {
                e.prevent_default();
                if busy() {
                    return;
                }
                let body = TotpDisable { password: password() };
                busy.set(true);
                spawn(async move {
                    match api::post::<_, ()>("/api/account/2fa/disable", &body).await {
                        Ok(()) => {
                            password.set(String::new());
                            update_me(|m| m.totp_enabled = false);
                            state::toast("Zwei-Faktor-Anmeldung deaktiviert.");
                        }
                        Err(e) => state::toast_error(e.message),
                    }
                    busy.set(false);
                });
            },
            p { class: "text-sm text-muted-foreground", "Bei der Anmeldung wird ein Code aus deiner App verlangt. Zum Deaktivieren dein Passwort bestätigen." }
            Field { label: "Passwort", html_for: "totp-disable-pw",
                Input { id: "totp-disable-pw", r#type: InputType::Password, value: password(), required: true, autocomplete: "current-password", oninput: move |e: FormEvent| password.set(e.value()) }
            }
            div {
                Button { button_type: "submit", variant: ButtonVariant::Destructive, disabled: busy() || password().is_empty(),
                    Spinner { busy: busy() }
                    icons::ShieldOff {}
                    "Zwei-Faktor deaktivieren"
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// API-Schlüssel
// ---------------------------------------------------------------------------------------------

fn scope_label(scope: ApiScope) -> &'static str {
    match scope {
        ApiScope::Stats => "Statistiken",
        ApiScope::Sites => "Websites",
    }
}

#[component]
fn ApiKeysTab() -> Element {
    let mut keys = use_resource(|| async { api::get::<Vec<ApiKey>>("/api/account/api-keys").await });
    let mut create_open = use_signal(|| false);
    let mut to_delete = use_signal(|| None::<ApiKey>);
    let mut deleting = use_signal(|| false);

    let result = keys.read().clone();
    let body = match result {
        None => rsx! {
            div { class: "grid gap-2",
                Skeleton { class: "h-10 w-full" }
                Skeleton { class: "h-10 w-full" }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState { message: e.message, on_retry: move |_| keys.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            p { class: "rounded-lg border border-dashed px-4 py-8 text-center text-sm text-muted-foreground",
                "Noch kein API-Schlüssel. Lege einen an, um die Stats-API oder den MCP-Server zu nutzen."
            }
        },
        Some(Ok(list)) => rsx! {
            div { class: "overflow-x-auto rounded-lg border",
                Table { class: "max-w-none",
                    TableHeader {
                        TableRow {
                            TableHead { "Name" }
                            TableHead { "Schlüssel" }
                            TableHead { "Berechtigung" }
                            TableHead { class: "hidden sm:table-cell", "Angelegt" }
                            TableHead { class: "hidden sm:table-cell", "Zuletzt benutzt" }
                            TableHead { class: "w-12", span { class: "sr-only", "Aktionen" } }
                        }
                    }
                    TableBody {
                        for k in list {
                            TableRow { key: "{k.id}",
                                TableCell { class: "px-2 py-3 font-medium", "{k.name}" }
                                TableCell { class: "px-2 py-3 font-mono text-xs text-muted-foreground", "{k.prefix}…" }
                                TableCell { class: "px-2 py-3",
                                    Badge { variant: if k.scope == ApiScope::Sites { BadgeVariant::Info } else { BadgeVariant::Secondary }, "{scope_label(k.scope)}" }
                                }
                                TableCell { class: "hidden px-2 py-3 text-muted-foreground sm:table-cell", "{util::date_of_ts(k.created_at)}" }
                                TableCell { class: "hidden px-2 py-3 text-muted-foreground sm:table-cell",
                                    match k.last_used_at {
                                        Some(ts) => rsx! { span { title: "{util::datetime(ts)}", "{util::relative(ts)}" } },
                                        None => rsx! { "nie" },
                                    }
                                }
                                TableCell { class: "px-2 py-3 text-right",
                                    Button {
                                        variant: ButtonVariant::Ghost,
                                        size: ButtonSize::IconSm,
                                        aria_label: "Schlüssel {k.name} löschen",
                                        title: "Löschen",
                                        onclick: {
                                            let k = k.clone();
                                            move |_| to_delete.set(Some(k.clone()))
                                        },
                                        icons::Trash2 {}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    };

    let pending = to_delete();
    rsx! {
        div { class: "grid gap-6",
            Section {
                title: "API-Schlüssel",
                description: "Für die Stats-API, die Sites-API und den MCP-Server. Schlüssel gelten für alle deine Websites.",
                actions: rsx! {
                    Button { size: ButtonSize::Sm, onclick: move |_| create_open.set(true),
                        icons::Plus {}
                        "Schlüssel anlegen"
                    }
                },
                {body}
            }
            ApiDocs {}
        }
        CreateKeyModal {
            open: create_open(),
            on_close: move |_| create_open.set(false),
            on_created: move |_| keys.restart(),
        }
        ConfirmModal {
            open: pending.is_some(),
            on_close: move |_| to_delete.set(None),
            title: "API-Schlüssel löschen?",
            message: format!(
                "„{}“ funktioniert danach nicht mehr. Programme, die ihn nutzen, verlieren den Zugriff.",
                pending.as_ref().map(|k| k.name.as_str()).unwrap_or_default()
            ),
            confirm_label: "Löschen",
            busy: deleting(),
            on_confirm: move |_| {
                let Some(k) = to_delete() else { return };
                deleting.set(true);
                spawn(async move {
                    match api::delete(&format!("/api/account/api-keys/{}", k.id)).await {
                        Ok(()) => {
                            state::toast("Schlüssel gelöscht.");
                            keys.restart();
                        }
                        Err(e) => state::toast_error(e.message),
                    }
                    deleting.set(false);
                    to_delete.set(None);
                });
            },
        }
    }
}

/// Modal: neuen Schlüssel anlegen, danach den Schlüssel einmalig anzeigen.
#[component]
fn CreateKeyModal(open: bool, on_close: EventHandler<()>, on_created: EventHandler<()>) -> Element {
    let mut name = use_signal(String::new);
    let mut scope = use_signal(|| "stats".to_string());
    let mut busy = use_signal(|| false);
    let mut created = use_signal(|| None::<CreatedApiKey>);

    let mut close = move || {
        name.set(String::new());
        scope.set("stats".into());
        created.set(None);
        on_close.call(());
    };

    let scope_options = vec![
        ("stats".to_string(), "Statistiken – Stats-API und MCP lesen".to_string()),
        ("sites".to_string(), "Websites – zusätzlich Websites, Ziele, Links verwalten".to_string()),
    ];

    if let Some(c) = created() {
        return rsx! {
            Modal {
                open,
                on_close: move |_| close(),
                title: "Schlüssel angelegt",
                footer: rsx! {
                    Button { onclick: move |_| close(), "Fertig" }
                },
                div { class: "grid gap-4",
                    Alert { class: "border-warning/40 bg-warning/10",
                        icons::TriangleAlert { class: "text-warning" }
                        AlertTitle { "Nur jetzt sichtbar" }
                        AlertDescription { "Kopiere den Schlüssel jetzt. Er wird nicht noch einmal angezeigt." }
                    }
                    Field { label: "{c.api_key.name} ({scope_label(c.api_key.scope)})",
                        CopyField { value: c.key.clone(), aria_label: "API-Schlüssel" }
                    }
                }
            }
        };
    }

    rsx! {
        Modal {
            open,
            on_close: move |_| close(),
            title: "API-Schlüssel anlegen",
            form {
                id: "create-key-form",
                class: "grid gap-4",
                onsubmit: move |e: FormEvent| {
                    e.prevent_default();
                    if busy() {
                        return;
                    }
                    let n = name().trim().to_string();
                    if n.is_empty() {
                        state::toast_error("Bitte einen Namen angeben.");
                        return;
                    }
                    let body = NewApiKey { name: n, scope: if scope() == "sites" { ApiScope::Sites } else { ApiScope::Stats } };
                    busy.set(true);
                    spawn(async move {
                        match api::post::<_, CreatedApiKey>("/api/account/api-keys", &body).await {
                            Ok(c) => {
                                created.set(Some(c));
                                on_created.call(());
                            }
                            Err(e) => state::toast_error(e.message),
                        }
                        busy.set(false);
                    });
                },
                Field { label: "Name", html_for: "key-name", hint: "Wofür der Schlüssel benutzt wird, z. B. „Claude“ oder „Reporting“.",
                    Input { id: "key-name", value: name(), autofocus: true, required: true, autocomplete: "off", oninput: move |e: FormEvent| name.set(e.value()) }
                }
                Field { label: "Berechtigung", html_for: "key-scope",
                    Select { id: "key-scope", value: scope(), options: scope_options, on_change: move |v: String| scope.set(v) }
                }
                div { class: "flex justify-end gap-2 pt-1",
                    Button { variant: ButtonVariant::Ghost, onclick: move |_| close(), "Abbrechen" }
                    Button { button_type: "submit", disabled: busy(),
                        Spinner { busy: busy() }
                        "Anlegen"
                    }
                }
            }
        }
    }
}

/// Hinweise zu Stats-API und MCP-Server mit kopierbaren Beispielen.
#[component]
fn ApiDocs() -> Element {
    let url = state::public_url();
    let aggregate = format!(
        "curl \"{url}/api/v1/stats/aggregate?site_id=example.com&period=30d&metrics=visitors,pageviews,bounce_rate\" \\\n  -H \"Authorization: Bearer <schlüssel>\""
    );
    let v1_routes = format!(
        "GET {url}/api/v1/stats/aggregate\nGET {url}/api/v1/stats/timeseries\nGET {url}/api/v1/stats/breakdown\nGET {url}/api/v1/stats/realtime/visitors"
    );
    let v2 = format!(
        "curl -X POST \"{url}/api/v2/query\" \\\n  -H \"Authorization: Bearer <schlüssel>\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{{\n    \"site_id\": \"example.com\",\n    \"metrics\": [\"visitors\", \"pageviews\", \"bounce_rate\"],\n    \"date_range\": \"30d\",\n    \"dimensions\": [\"visit:source\"],\n    \"filters\": [[\"is\", \"visit:country\", [\"CH\"]]],\n    \"order_by\": [[\"visitors\", \"desc\"]]\n  }}'"
    );
    let mcp_json = format!(
        "{{\n  \"mcpServers\": {{\n    \"nrzanalytics\": {{\n      \"type\": \"http\",\n      \"url\": \"{url}/mcp\",\n      \"headers\": {{\n        \"Authorization\": \"Bearer <schlüssel>\"\n      }}\n    }}\n  }}\n}}"
    );
    let mcp_cli = format!("claude mcp add --transport http nrzanalytics {url}/mcp \\\n  --header \"Authorization: Bearer <schlüssel>\"");
    rsx! {
        Section { title: "Stats-API", description: "Kompatibel mit der Plausible Stats-API (v1 und v2). Jede Anfrage braucht den Header Authorization: Bearer <schlüssel>.",
            div { class: "grid gap-4 text-sm",
                div { class: "grid gap-2",
                    p { class: "text-muted-foreground", "Endpunkte (v1):" }
                    CopyField { value: v1_routes, multiline: true, aria_label: "Endpunkte v1" }
                }
                div { class: "grid gap-2",
                    p { class: "text-muted-foreground", "Beispiel: Besucher der letzten 30 Tage" }
                    CopyField { value: aggregate, multiline: true, aria_label: "Beispiel aggregate" }
                }
                div { class: "grid gap-2",
                    p { class: "text-muted-foreground", "Beispiel: Abfrage über v2 (POST /api/v2/query)" }
                    CopyField { value: v2, multiline: true, aria_label: "Beispiel v2" }
                }
            }
        }
        Section { title: "MCP-Server", description: "Statistiken direkt in KI-Assistenten abfragen – per POST an {url}/mcp mit demselben Bearer-Schlüssel.",
            div { class: "grid gap-4 text-sm",
                div { class: "grid gap-2",
                    p { class: "text-muted-foreground", "Konfiguration für Claude Desktop/Code (JSON):" }
                    CopyField { value: mcp_json, multiline: true, aria_label: "MCP-Konfiguration" }
                }
                div { class: "grid gap-2",
                    p { class: "text-muted-foreground", "Oder mit Claude Code auf der Kommandozeile:" }
                    CopyField { value: mcp_cli, multiline: true, aria_label: "MCP-Befehl" }
                }
                p { class: "text-xs text-muted-foreground",
                    "Mit der Berechtigung „Statistiken“ kann der Assistent nur lesen; mit „Websites“ auch Websites, Ziele und geteilte Links anlegen."
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::group4;

    #[test]
    fn gruppiert_geheimnis() {
        assert_eq!(group4("ABCDEFGHIJ"), "ABCD EFGH IJ");
    }
}
