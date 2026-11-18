//! Abschnitt „Allgemein“: Domain, Zeitzone, Snippet.

use super::helpers::{ReadOnlyNote, can_admin, site_api};
use crate::api;
use crate::components::common::{CopyField, Field, Section};
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::input::Input;
use crate::components::ui::select::Select;
use crate::route::Route;
use crate::state;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{Site, TimezoneOption, UpdateSite};

#[component]
pub fn GeneralSection(site: Site, on_saved: EventHandler<()>) -> Element {
    let admin = can_admin(site.role);
    let domain = site.domain.clone();
    let nav = navigator();

    // --- Domain ---
    let mut new_domain = use_signal(|| site.domain.clone());
    let mut domain_busy = use_signal(|| false);
    let domain_for_submit = domain.clone();
    let save_domain = move |e: FormEvent| {
        e.prevent_default();
        let neu = new_domain().trim().to_string();
        if neu.is_empty() {
            state::toast_error("Bitte eine Domain eingeben.");
            return;
        }
        if neu == domain_for_submit || domain_busy() {
            return;
        }
        let path = site_api(&domain_for_submit, "");
        domain_busy.set(true);
        spawn(async move {
            let body = UpdateSite { domain: Some(neu), ..Default::default() };
            match api::put::<_, Site>(&path, &body).await {
                Ok(s) => {
                    state::toast("Domain geändert – bitte das Snippet auf der Website anpassen.");
                    *state::SITES_REVISION.write() += 1;
                    nav.replace(Route::site_settings(&s.domain, "general"));
                }
                Err(e) => state::toast_error(e.message),
            }
            domain_busy.set(false);
        });
    };

    // --- Zeitzone ---
    let timezones = use_resource(|| async { api::get::<Vec<TimezoneOption>>("/api/timezones").await });
    let mut tz = use_signal(|| site.timezone.clone());
    let mut tz_busy = use_signal(|| false);
    let mut tz_options: Vec<(String, String)> = match &*timezones.read() {
        Some(Ok(list)) => list.iter().map(|t| (t.name.clone(), t.label.clone())).collect(),
        _ => Vec::new(),
    };
    // Aktuelle Zeitzone immer anbieten, auch wenn die Liste (noch) fehlt.
    if !tz_options.iter().any(|(n, _)| *n == site.timezone) {
        tz_options.insert(0, (site.timezone.clone(), site.timezone.clone()));
    }
    let tz_error = matches!(&*timezones.read(), Some(Err(_)));
    let current_tz = site.timezone.clone();
    let domain_for_tz = domain.clone();
    let save_tz = move |e: FormEvent| {
        e.prevent_default();
        if tz_busy() || tz() == current_tz {
            return;
        }
        let path = site_api(&domain_for_tz, "");
        let body = UpdateSite { timezone: Some(tz()), ..Default::default() };
        tz_busy.set(true);
        spawn(async move {
            match api::put::<_, Site>(&path, &body).await {
                Ok(_) => {
                    state::toast("Zeitzone gespeichert.");
                    *state::SITES_REVISION.write() += 1;
                    on_saved.call(());
                }
                Err(e) => state::toast_error(e.message),
            }
            tz_busy.set(false);
        });
    };

    rsx! {
        if !admin {
            ReadOnlyNote { text: "Nur Admins können die allgemeinen Einstellungen ändern." }
        }
        Section { title: "Domain", description: "Die Domain, unter der die Website erfasst wird.",
            form { class: "grid gap-4", onsubmit: save_domain,
                Field { label: "Domain", html_for: "site-domain", hint: "Ohne https:// und ohne Pfad, z. B. example.com. Nach einer Änderung muss das Snippet auf der Website angepasst werden.",
                    Input {
                        id: "site-domain",
                        value: new_domain(),
                        disabled: !admin,
                        autocomplete: "off",
                        oninput: move |e: FormEvent| new_domain.set(e.value()),
                    }
                }
                if admin {
                    div {
                        Button { button_type: "submit", disabled: domain_busy() || new_domain().trim().is_empty() || new_domain().trim() == domain,
                            if domain_busy() {
                                icons::LoaderCircle { class: "animate-spin" }
                            }
                            "Domain ändern"
                        }
                    }
                }
            }
        }
        Section { title: "Zeitzone", description: "Bestimmt, wann ein Tag in den Berichten beginnt und endet.",
            form { class: "grid gap-4", onsubmit: save_tz,
                Field { label: "Zeitzone", html_for: "site-timezone",
                    Select {
                        id: "site-timezone",
                        value: tz(),
                        options: tz_options,
                        disabled: !admin,
                        on_change: move |v: String| tz.set(v),
                    }
                    if tz_error {
                        p { class: "text-xs text-destructive", "Die Liste der Zeitzonen konnte nicht geladen werden." }
                    }
                }
                if admin {
                    div {
                        Button { button_type: "submit", disabled: tz_busy() || tz() == site.timezone,
                            if tz_busy() {
                                icons::LoaderCircle { class: "animate-spin" }
                            }
                            "Zeitzone speichern"
                        }
                    }
                }
            }
        }
        Section { title: "Snippet", description: "Dieses Snippet gehört in den <head> jeder Seite.",
            div { class: "grid gap-3",
                CopyField { value: site.snippet.clone(), multiline: true, aria_label: "Snippet" }
                div {
                    Button { variant: ButtonVariant::Outline, href: Route::Snippet { domain: domain.clone() }.to_string(),
                        icons::Code {}
                        "Zur Snippet-Seite"
                    }
                }
            }
        }
    }
}
