//! Geteilter Link: `/share/{domain}?auth={slug}[&embed=true][&theme=light|dark]`.
//!
//! Alle API-Aufrufe tragen `x-nrz-share: slug`. Ist der Link mit Passwort geschuetzt, erscheint
//! zuerst ein Passwortformular (`POST /api/share/{slug}/auth`).

use crate::api;
use crate::components::common::{Brand, ErrorState, Field, PageSkeleton};
use crate::components::dashboard::DashboardView;
use crate::components::dashboard::params::DashParams;
use crate::components::layout::ThemeButton;
use crate::components::ui::alert::{Alert, AlertDescription, AlertVariant};
use crate::components::ui::button::Button;
use crate::components::ui::input::{Input, InputType};
use crate::state;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{ShareInfo, SharePassword};

#[component]
pub fn Share(domain: String, params: DashParams) -> Element {
    let slug = params.auth.clone();
    // Share-Slug global setzen, bevor Kinder Anfragen stellen.
    {
        let slug = slug.clone();
        use_hook(move || {
            *state::SHARE_SLUG.write() = (!slug.is_empty()).then_some(slug);
        });
    }
    if state::SHARE_SLUG.peek().as_deref() != (!slug.is_empty()).then_some(slug.as_str()) {
        *state::SHARE_SLUG.write() = (!slug.is_empty()).then(|| slug.clone());
    }
    // Thema aus der URL (ohne es dauerhaft zu speichern).
    let theme = params.theme.clone();
    use_effect(use_reactive((&theme,), move |(theme,)| match theme.as_str() {
        "light" => crate::util::set_html_dark(false),
        "dark" => crate::util::set_html_dark(true),
        _ => crate::util::set_html_dark(*state::DARK.peek()),
    }));
    use_drop(move || {
        *state::SHARE_SLUG.write() = None;
        crate::util::set_html_dark(*state::DARK.peek());
    });

    let mut revision = use_signal(|| 0u32);
    let info = use_resource(use_reactive((&slug,), move |(slug,)| async move {
        let _ = revision();
        if slug.is_empty() {
            return Err(api::ApiErr { status: 400, code: "invalid".into(), message: "Der Link ist unvollständig (Parameter auth fehlt).".into() });
        }
        api::get::<ShareInfo>(&format!("/api/share/{}", api::enc(&slug))).await
    }));

    let embed = params.embed;
    let content = match &*info.read() {
        None => rsx! { PageSkeleton {} },
        Some(Err(e)) => rsx! {
            ErrorState { message: if e.status == 404 { "Dieser geteilte Link existiert nicht (mehr).".to_string() } else { e.message.clone() } }
        },
        Some(Ok(i)) if i.needs_password => rsx! {
            SharePasswordForm { slug: slug.clone(), name: i.name.clone(), on_success: move |_| revision += 1 }
        },
        Some(Ok(_)) => rsx! { DashboardView { domain: domain.clone(), params: params.clone(), shared: true } },
    };

    if embed {
        return rsx! {
            div { class: "min-h-dvh bg-transparent p-2 sm:p-3", {content} }
        };
    }
    rsx! {
        header { class: "border-b",
            div { class: "container flex h-14 items-center gap-3",
                Brand {}
                span { class: "hidden text-sm text-muted-foreground sm:inline", "Geteiltes Dashboard" }
                div { class: "ml-auto", ThemeButton {} }
            }
        }
        main { class: "container pb-16 pt-4", {content} }
    }
}

#[component]
fn SharePasswordForm(slug: String, name: String, on_success: EventHandler<()>) -> Element {
    let mut password = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let submit = move |e: FormEvent| {
        e.prevent_default();
        if busy() || password().is_empty() {
            return;
        }
        busy.set(true);
        error.set(None);
        let slug = slug.clone();
        spawn(async move {
            let body = SharePassword { password: password() };
            match api::post::<_, ()>(&format!("/api/share/{}/auth", api::enc(&slug)), &body).await {
                Ok(()) => on_success.call(()),
                Err(e) => error.set(Some(if e.status == 401 || e.status == 403 { "Falsches Passwort.".into() } else { e.message })),
            }
            busy.set(false);
        });
    };
    rsx! {
        div { class: "mx-auto mt-16 w-full max-w-sm rounded-xl border bg-card p-6 shadow-sm",
            div { class: "mb-4 flex justify-center", Brand {} }
            h1 { class: "text-lg font-semibold", "Passwort erforderlich" }
            p { class: "mt-1 mb-5 text-sm text-muted-foreground",
                if name.is_empty() { "Dieses Dashboard ist mit einem Passwort geschützt." } else { "„{name}“ ist mit einem Passwort geschützt." }
            }
            form { class: "grid gap-4", onsubmit: submit,
                if let Some(e) = error() {
                    Alert { variant: AlertVariant::Destructive, AlertDescription { "{e}" } }
                }
                Field { label: "Passwort", html_for: "share-password",
                    Input { id: "share-password", r#type: InputType::Password, value: password(), autofocus: true, autocomplete: "current-password", oninput: move |e: FormEvent| password.set(e.value()) }
                }
                Button { button_type: "submit", class: "w-full", disabled: busy(),
                    if busy() { icons::LoaderCircle { class: "animate-spin" } } else { icons::LockOpen {} }
                    "Dashboard öffnen"
                }
            }
        }
    }
}
