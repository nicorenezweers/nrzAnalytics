//! Anmeldung, Registrierung und Ersteinrichtung (erstes Konto = Admin).

use crate::api;
use crate::components::common::{Brand, Field};
use crate::components::layout::ThemeButton;
use crate::components::ui::alert::{Alert, AlertDescription, AlertVariant};
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::input::{Input, InputType};
use crate::route::Route;
use crate::state;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{LoginRequest, Me, RegisterRequest};
use std::str::FromStr;

/// Ziel nach der Anmeldung (nur interne Pfade).
fn target(next: &str) -> Route {
    if next.starts_with('/') && !next.starts_with("//")
        && let Ok(r) = Route::from_str(next)
            && !matches!(r, Route::Login { .. } | Route::Register {} | Route::Setup {}) {
                return r;
            }
    Route::Sites {}
}

/// Rahmen der Anmeldeseiten: zentrierte Karte.
#[component]
fn AuthFrame(#[props(into)] title: String, #[props(into)] subtitle: String, children: Element, #[props(optional)] footer: Option<Element>) -> Element {
    rsx! {
        div { class: "relative flex min-h-dvh flex-col items-center justify-center bg-background px-4 py-10",
            div { class: "absolute right-3 top-3", ThemeButton {} }
            div { class: "mb-8", Brand {} }
            div { class: "w-full max-w-sm rounded-xl border bg-card p-6 shadow-sm",
                h1 { class: "text-xl font-semibold tracking-tight", "{title}" }
                p { class: "mt-1 mb-6 text-sm text-muted-foreground", "{subtitle}" }
                {children}
            }
            if let Some(f) = footer {
                div { class: "mt-6 text-sm text-muted-foreground", {f} }
            }
        }
    }
}

#[component]
pub fn Login(next: String) -> Element {
    let nav = navigator();
    let mut email = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut totp = use_signal(String::new);
    let mut need_totp = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);
    let status = state::STATUS.read().clone();
    let registration_open = status.as_ref().is_some_and(|s| s.registration_open);

    let next_for_effect = next.clone();
    use_effect(move || {
        let s = state::STATUS.read();
        if let Some(s) = s.as_ref() {
            if s.needs_setup {
                nav.replace(Route::Setup {});
            } else if s.user.is_some() {
                nav.replace(target(&next_for_effect));
            }
        }
    });

    let submit = move |e: FormEvent| {
        e.prevent_default();
        if busy() {
            return;
        }
        let req = LoginRequest {
            email: email().trim().to_string(),
            password: password(),
            totp: need_totp().then(|| totp().trim().replace(' ', "")).filter(|t| !t.is_empty()),
        };
        if req.email.is_empty() || req.password.is_empty() {
            error.set(Some("Bitte E-Mail bzw. Benutzername und Passwort eingeben.".into()));
            return;
        }
        busy.set(true);
        error.set(None);
        spawn(async move {
            match api::post::<_, Me>("/api/auth/login", &req).await {
                Ok(me) => state::set_me(Some(me)),
                Err(e) if e.code == "totp_required" => {
                    if need_totp() {
                        error.set(Some(e.message));
                    }
                    need_totp.set(true);
                    crate::util::focus_id("login-totp");
                }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };

    rsx! {
        crate::components::common::PageTitle { title: "Anmelden – nrzAnalytics" }
        AuthFrame {
            title: "Anmelden",
            subtitle: "Willkommen zurück bei nrzAnalytics.",
            footer: rsx! {
                if registration_open {
                    span { "Noch kein Konto? "
                        Link { to: Route::Register {}, class: "font-medium text-foreground underline-offset-4 hover:underline", "Registrieren" }
                    }
                }
            },
            form { class: "grid gap-4", onsubmit: submit,
                if let Some(e) = error() {
                    Alert { variant: AlertVariant::Destructive, AlertDescription { "{e}" } }
                }
                Field { label: "E-Mail oder Benutzername", html_for: "login-email",
                    Input { id: "login-email", value: email(), autocomplete: "username", autofocus: true, oninput: move |e: FormEvent| email.set(e.value()) }
                }
                Field { label: "Passwort", html_for: "login-password",
                    Input { id: "login-password", r#type: InputType::Password, value: password(), autocomplete: "current-password", oninput: move |e: FormEvent| password.set(e.value()) }
                }
                if need_totp() {
                    Field { label: "Code aus der Authenticator-App", html_for: "login-totp", hint: "Oder einen Wiederherstellungscode.",
                        Input { id: "login-totp", value: totp(), autocomplete: "one-time-code", inputmode: "numeric", autofocus: true, oninput: move |e: FormEvent| totp.set(e.value()) }
                    }
                }
                Button { button_type: "submit", class: "w-full", disabled: busy(),
                    if busy() { icons::LoaderCircle { class: "animate-spin" } } else { icons::LogIn {} }
                    "Anmelden"
                }
            }
        }
    }
}

/// Gemeinsames Formular fuer Registrierung und Einrichtung.
#[component]
fn RegisterForm(setup: bool) -> Element {
    let nav = navigator();
    let mut email = use_signal(String::new);
    let mut name = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut password2 = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    let submit = move |e: FormEvent| {
        e.prevent_default();
        if busy() {
            return;
        }
        let req = RegisterRequest { email: email().trim().to_string(), name: name().trim().to_string(), password: password() };
        if req.email.is_empty() {
            error.set(Some("Bitte eine E-Mail-Adresse eingeben.".into()));
            return;
        }
        if req.password.chars().count() < 8 {
            error.set(Some("Das Passwort muss mindestens 8 Zeichen lang sein.".into()));
            return;
        }
        if req.password != password2() {
            error.set(Some("Die Passwörter stimmen nicht überein.".into()));
            return;
        }
        busy.set(true);
        error.set(None);
        spawn(async move {
            match api::post::<_, Me>("/api/auth/register", &req).await {
                Ok(me) => {
                    state::set_me(Some(me));
                    nav.replace(Route::NewSite {});
                }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };

    rsx! {
        form { class: "grid gap-4", onsubmit: submit,
            if let Some(e) = error() {
                Alert { variant: AlertVariant::Destructive, AlertDescription { "{e}" } }
            }
            Field { label: "Name", html_for: "reg-name",
                Input { id: "reg-name", value: name(), autocomplete: "name", autofocus: true, oninput: move |e: FormEvent| name.set(e.value()) }
            }
            Field { label: "E-Mail", html_for: "reg-email",
                Input { id: "reg-email", r#type: InputType::Email, value: email(), autocomplete: "email", oninput: move |e: FormEvent| email.set(e.value()) }
            }
            Field { label: "Passwort", html_for: "reg-password", hint: "Mindestens 8 Zeichen.",
                Input { id: "reg-password", r#type: InputType::Password, value: password(), autocomplete: "new-password", oninput: move |e: FormEvent| password.set(e.value()) }
            }
            Field { label: "Passwort wiederholen", html_for: "reg-password2",
                Input { id: "reg-password2", r#type: InputType::Password, value: password2(), autocomplete: "new-password", oninput: move |e: FormEvent| password2.set(e.value()) }
            }
            Button { button_type: "submit", class: "w-full", disabled: busy(),
                if busy() { icons::LoaderCircle { class: "animate-spin" } } else { icons::UserPlus {} }
                if setup { "Admin-Konto anlegen" } else { "Konto anlegen" }
            }
        }
    }
}

#[component]
pub fn Register() -> Element {
    let nav = navigator();
    let status = state::STATUS.read().clone();
    use_effect(move || {
        let s = state::STATUS.read();
        if let Some(s) = s.as_ref() {
            if s.needs_setup {
                nav.replace(Route::Setup {});
            } else if s.user.is_some() {
                nav.replace(Route::Sites {});
            }
        }
    });
    let open = status.as_ref().is_some_and(|s| s.registration_open);
    rsx! {
        crate::components::common::PageTitle { title: "Registrieren – nrzAnalytics" }
        AuthFrame {
            title: "Konto anlegen",
            subtitle: "Datenschutzfreundliche Statistiken für deine Websites.",
            footer: rsx! {
                span { "Schon ein Konto? "
                    Link { to: Route::Login { next: String::new() }, class: "font-medium text-foreground underline-offset-4 hover:underline", "Anmelden" }
                }
            },
            if open {
                RegisterForm { setup: false }
            } else {
                Alert { AlertDescription { "Die Registrierung ist auf dieser Instanz geschlossen. Bitte wende dich an die Administration." } }
                Button { href: Route::Login { next: String::new() }.to_string(), variant: ButtonVariant::Outline, class: "mt-4 w-full", "Zur Anmeldung" }
            }
        }
    }
}

#[component]
pub fn Setup() -> Element {
    let nav = navigator();
    let needs_setup = state::STATUS.read().as_ref().is_some_and(|s| s.needs_setup);
    use_effect(move || {
        let s = state::STATUS.read();
        if let Some(s) = s.as_ref()
            && !s.needs_setup {
                nav.replace(if s.user.is_some() { Route::Sites {} } else { Route::Login { next: String::new() } });
            }
    });
    rsx! {
        crate::components::common::PageTitle { title: "Einrichtung – nrzAnalytics" }
        AuthFrame {
            title: "Willkommen bei nrzAnalytics",
            subtitle: "Lege das erste Konto an. Es erhält Administratorrechte für diese Instanz.",
            if needs_setup {
                RegisterForm { setup: true }
            }
        }
    }
}
