//! App-Rahmen: Kopfzeile mit Logo, Website-Umschalter und Benutzermenue; darunter der Inhalt.

use crate::api;
use crate::components::common::{Brand, LetterAvatar};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::dropdown::{Dropdown, DropdownAlign, DropdownItem, DropdownLabel, DropdownLink, DropdownSeparator};
use crate::route::Route;
use crate::state::{self, DARK, SITES_REVISION};
use dioxus::prelude::*;
use nrzanalytics_shared::api::SiteSummary;

#[component]
pub fn AppShell() -> Element {
    let route = use_route::<Route>();
    let nav = navigator();
    let needs_setup = state::STATUS.read().as_ref().is_some_and(|s| s.needs_setup);
    use_effect(move || {
        if state::STATUS.read().as_ref().is_some_and(|s| s.needs_setup) {
            nav.replace(Route::Setup {});
        }
    });
    // Seitenwechsel: nach oben scrollen.
    use_effect(use_reactive!(|route| {
        if !matches!(route, Route::Dashboard { .. }) {
            crate::util::scroll_top();
        }
    }));
    if needs_setup {
        return rsx! {};
    }
    rsx! {
        a { href: "#inhalt", class: "sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-[90] focus:rounded-lg focus:bg-primary focus:px-3 focus:py-2 focus:text-primary-foreground", "Zum Inhalt springen" }
        Header {}
        main { id: "inhalt", class: "container pb-16 pt-6", Outlet::<Route> {} }
    }
}

#[component]
fn Header() -> Element {
    let me = state::me();
    let route = use_route::<Route>();
    // Das Dashboard hat einen eigenen Website-Umschalter in seiner Leiste; im Kopf nur auf den
    // Seiten einer Website ohne eigenen Umschalter (Einstellungen, Snippet).
    let current_domain = match &route {
        Route::SiteSettings { domain, .. } | Route::Snippet { domain } | Route::SiteSettingsIndex { domain } => Some(domain.clone()),
        _ => None,
    };
    rsx! {
        header { class: "border-b bg-background/80 backdrop-blur-xl",
            div { class: "container flex h-14 items-center gap-3",
                Link { to: if me.is_some() { Route::Sites {} } else { Route::Login { next: String::new() } }, class: "rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring", "aria-label": "nrzAnalytics – Startseite",
                    Brand {}
                }
                if let (Some(d), true) = (&current_domain, me.is_some()) {
                    span { class: "hidden text-muted-foreground/50 sm:inline", "/" }
                    div { class: "hidden sm:block", SiteSwitcher { current: d.clone() } }
                }
                div { class: "ml-auto flex items-center gap-1",
                    if me.is_some() {
                        UserMenu {}
                    } else {
                        ThemeButton {}
                        Button { href: Route::login_here().to_string(), variant: ButtonVariant::Outline, size: ButtonSize::Sm,
                            icons::LogIn {}
                            "Anmelden"
                        }
                    }
                }
            }
        }
    }
}

/// Knopf hell/dunkel.
#[component]
pub fn ThemeButton() -> Element {
    let dark = DARK();
    rsx! {
        Button {
            variant: ButtonVariant::Ghost,
            size: ButtonSize::Icon,
            aria_label: if dark { "Helles Thema" } else { "Dunkles Thema" },
            title: if dark { "Helles Thema" } else { "Dunkles Thema" },
            onclick: move |_| state::set_dark(!dark),
            if dark { icons::Sun {} } else { icons::Moon {} }
        }
    }
}

/// Website-Umschalter (Aufklappmenue mit allen Websites des Kontos).
#[component]
pub fn SiteSwitcher(#[props(into)] current: String, #[props(into, optional)] class: Option<String>) -> Element {
    let sites = use_resource(move || async move {
        let _ = SITES_REVISION();
        if !state::is_logged_in() {
            return Vec::new();
        }
        api::get::<Vec<SiteSummary>>("/api/sites").await.unwrap_or_default()
    });
    let list = sites.read().clone().unwrap_or_default();
    let nav = navigator();
    rsx! {
        Dropdown {
            class: class.unwrap_or_default(),
            aria_label: "Website wechseln",
            trigger_class: "h-9 max-w-[16rem] px-2 hover:bg-accent",
            trigger: rsx! {
                LetterAvatar { domain: current.clone() }
                span { class: "truncate font-semibold", "{current}" }
                icons::ChevronsUpDown { class: "size-4 text-muted-foreground" }
            },
            DropdownLabel { "Websites" }
            for s in list.iter() {
                DropdownItem {
                    key: "{s.domain}",
                    active: s.domain == current,
                    onclick: {
                        let d = s.domain.clone();
                        move |_| {
                            nav.push(Route::dashboard(&d));
                        }
                    },
                    LetterAvatar { domain: s.domain.clone(), class: "size-5 text-[10px]" }
                    span { class: "truncate", "{s.domain}" }
                }
            }
            if list.is_empty() {
                div { class: "px-2 py-1.5 text-sm text-muted-foreground", "Keine weiteren Websites" }
            }
            DropdownSeparator {}
            DropdownLink { to: Route::Sites {}, icons::List {} "Alle Websites" }
            DropdownLink { to: Route::NewSite {}, icons::Plus {} "Website hinzufügen" }
        }
    }
}

#[component]
fn UserMenu() -> Element {
    let Some(me) = state::me() else { return rsx! {} };
    let dark = DARK();
    let nav = navigator();
    let name = if me.name.trim().is_empty() { me.email.clone() } else { me.name.clone() };
    let letter = crate::util::initial(&name);
    rsx! {
        Dropdown {
            align: DropdownAlign::End,
            aria_label: "Benutzermenü",
            trigger_class: "h-9 rounded-full pl-1 pr-2 hover:bg-accent",
            trigger: rsx! {
                span { class: "flex size-7 items-center justify-center rounded-full bg-secondary text-xs font-semibold", "{letter}" }
                span { class: "hidden max-w-[10rem] truncate md:inline", "{name}" }
                icons::ChevronDown { class: "size-4 text-muted-foreground" }
            },
            div { class: "px-2 py-1.5",
                div { class: "truncate text-sm font-medium", "{name}" }
                div { class: "truncate text-xs text-muted-foreground", "{me.email}" }
            }
            DropdownSeparator {}
            DropdownLink { to: Route::Sites {}, icons::Globe {} "Websites" }
            DropdownLink { to: Route::Account { tab: String::new() }, icons::Settings {} "Einstellungen" }
            if me.is_admin {
                DropdownLink { to: Route::Admin { tab: String::new() }, icons::ShieldUser {} "Verwaltung" }
            }
            DropdownItem { onclick: move |_| state::set_dark(!dark),
                if dark { icons::Sun {} } else { icons::Moon {} }
                if dark { "Helles Thema" } else { "Dunkles Thema" }
            }
            DropdownSeparator {}
            DropdownItem {
                onclick: move |_| {
                    spawn(async move {
                        let _ = api::post_empty::<()>("/api/auth/logout").await;
                        state::set_me(None);
                        nav.replace(Route::Login { next: String::new() });
                    });
                },
                icons::LogOut {}
                "Abmelden"
            }
        }
    }
}
