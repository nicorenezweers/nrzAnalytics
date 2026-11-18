//! Abschnitt „Sichtbarkeit“: oeffentliches Dashboard und geteilte Links.

use super::helpers::{EmptyBox, InlineLoading, ReadOnlyNote, can_admin, can_edit, site_api};
use crate::api;
use crate::components::common::{ConfirmModal, CopyField, ErrorState, Field, Section};
use crate::components::ui::badge::{Badge, BadgeVariant};
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::switch::Switch;
use crate::state;
use crate::util;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{NewSharedLink, SharedLink, Site, UpdateSite};

/// Einbettungscode fuer einen geteilten Link (nur als Text, wird nicht gerendert).
fn embed_code(url: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!(
        "<iframe plausible-embed src=\"{url}{sep}embed=true&theme=dark\" scrolling=\"no\" frameborder=\"0\" loading=\"lazy\" style=\"width: 1px; min-width: 100%; height: 1600px;\"></iframe>"
    )
}

#[component]
pub fn VisibilitySection(site: Site, on_saved: EventHandler<()>) -> Element {
    let admin = can_admin(site.role);
    let editor = can_edit(site.role);
    let domain = site.domain.clone();
    let mut public_busy = use_signal(|| false);
    let public_link = format!("{}/{}", state::public_url(), domain);

    let d_pub = domain.clone();
    let toggle_public = move |v: bool| {
        if public_busy() {
            return;
        }
        let path = site_api(&d_pub, "");
        public_busy.set(true);
        spawn(async move {
            let body = UpdateSite { public: Some(v), ..Default::default() };
            match api::put::<_, Site>(&path, &body).await {
                Ok(_) => {
                    state::toast(if v { "Dashboard ist jetzt öffentlich." } else { "Dashboard ist jetzt privat." });
                    *state::SITES_REVISION.write() += 1;
                    on_saved.call(());
                }
                Err(e) => state::toast_error(e.message),
            }
            public_busy.set(false);
        });
    };

    rsx! {
        Section { title: "Öffentliches Dashboard", description: "Jeder mit dem Link kann die Statistiken ansehen – ohne Anmeldung.",
            if !admin {
                ReadOnlyNote { text: "Nur Admins können die Sichtbarkeit ändern." }
            }
            div { class: "flex items-center justify-between gap-4",
                label { r#for: "site-public", class: "text-sm font-medium", "Öffentliches Dashboard" }
                Switch {
                    id: "site-public",
                    checked: site.public,
                    disabled: !admin || public_busy(),
                    aria_label: "Öffentliches Dashboard",
                    on_change: toggle_public,
                }
            }
            if site.public {
                div { class: "mt-4 grid gap-1.5",
                    p { class: "text-sm font-medium", "Link zum öffentlichen Dashboard" }
                    CopyField { value: public_link.clone(), aria_label: "Link zum öffentlichen Dashboard" }
                    a { href: "{public_link}", target: "_blank", rel: "noopener", class: "inline-flex w-fit items-center gap-1 text-xs text-muted-foreground hover:text-foreground [&>svg]:size-3.5",
                        icons::ExternalLink {}
                        "Öffnen"
                    }
                }
            }
        }
        SharedLinks { domain, editor }
    }
}

#[component]
fn SharedLinks(domain: String, editor: bool) -> Element {
    let mut links = use_resource(use_reactive!(|domain| async move { api::get::<Vec<SharedLink>>(&site_api(&domain, "/shared-links")).await }));
    let mut name = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut embed_open = use_signal(|| None::<i64>);
    let mut remove = use_signal(|| None::<SharedLink>);
    let mut remove_busy = use_signal(|| false);

    let d_add = domain.clone();
    let create = move |e: FormEvent| {
        e.prevent_default();
        let n = name().trim().to_string();
        if n.is_empty() || busy() {
            return;
        }
        let pw = password();
        let body = NewSharedLink { name: n, password: if pw.is_empty() { None } else { Some(pw) } };
        let path = site_api(&d_add, "/shared-links");
        busy.set(true);
        spawn(async move {
            match api::post::<_, SharedLink>(&path, &body).await {
                Ok(_) => {
                    state::toast("Geteilter Link angelegt.");
                    name.set(String::new());
                    password.set(String::new());
                    links.restart();
                }
                Err(e) => state::toast_error(e.message),
            }
            busy.set(false);
        });
    };

    let d_rm = domain.clone();
    let confirm_remove = move |_| {
        let Some(l) = remove() else { return };
        let path = site_api(&d_rm, &format!("/shared-links/{}", l.id));
        remove_busy.set(true);
        spawn(async move {
            match api::delete(&path).await {
                Ok(()) => {
                    state::toast("Geteilter Link gelöscht.");
                    links.restart();
                }
                Err(e) => state::toast_error(e.message),
            }
            remove_busy.set(false);
            remove.set(None);
        });
    };

    let data = links.read().clone();
    let remove_name = remove().map(|l| l.name).unwrap_or_default();

    rsx! {
        Section { title: "Geteilte Links", description: "Private Links zum Dashboard, optional mit Passwort. Lassen sich auch in andere Seiten einbetten.",
            if !editor {
                ReadOnlyNote { text: "Nur Bearbeiter und Admins können geteilte Links verwalten." }
            }
            match data {
                None => rsx! { InlineLoading {} },
                Some(Err(e)) => rsx! { ErrorState { message: e.message, on_retry: move |_| links.restart() } },
                Some(Ok(list)) if list.is_empty() => rsx! {
                    EmptyBox { icon: rsx! { icons::Link {} }, title: "Noch keine geteilten Links",
                        "Mit einem geteilten Link können andere das Dashboard ansehen, ohne ein Konto zu haben."
                    }
                },
                Some(Ok(list)) => rsx! {
                    ul { class: "grid gap-3",
                        for l in list {
                            li { key: "{l.id}", class: "grid gap-2 rounded-lg border p-3",
                                div { class: "flex flex-wrap items-center gap-2",
                                    span { class: "font-medium", "{l.name}" }
                                    if l.has_password {
                                        Badge { variant: BadgeVariant::Secondary, icons::Lock { class: "size-3" } "Passwort" }
                                    }
                                    span { class: "text-xs text-muted-foreground", title: util::datetime(l.created_at), "angelegt {util::relative(l.created_at)}" }
                                    div { class: "ml-auto flex items-center gap-1",
                                        Button {
                                            variant: ButtonVariant::Ghost,
                                            size: ButtonSize::Sm,
                                            onclick: {
                                                let id = l.id;
                                                move |_| embed_open.set(if embed_open() == Some(id) { None } else { Some(id) })
                                            },
                                            icons::Code {}
                                            if embed_open() == Some(l.id) { "Einbettung ausblenden" } else { "Einbetten" }
                                        }
                                        if editor {
                                            Button {
                                                variant: ButtonVariant::Ghost,
                                                size: ButtonSize::IconSm,
                                                aria_label: "Link „{l.name}“ löschen",
                                                title: "Löschen",
                                                onclick: {
                                                    let l = l.clone();
                                                    move |_| remove.set(Some(l.clone()))
                                                },
                                                icons::Trash2 {}
                                            }
                                        }
                                    }
                                }
                                CopyField { value: l.url.clone(), aria_label: "URL von {l.name}" }
                                if embed_open() == Some(l.id) {
                                    div { class: "grid gap-1.5",
                                        p { class: "text-xs text-muted-foreground", "Diesen Code in die eigene Seite einfügen:" }
                                        CopyField { value: embed_code(&l.url), multiline: true, aria_label: "Einbettungscode" }
                                    }
                                }
                            }
                        }
                    }
                },
            }
            if editor {
                form { class: "mt-5 grid gap-4 border-t pt-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] sm:items-end", onsubmit: create,
                    Field { label: "Name", html_for: "share-name",
                        Input {
                            id: "share-name",
                            value: name(),
                            placeholder: "z. B. Kunde",
                            autocomplete: "off",
                            oninput: move |e: FormEvent| name.set(e.value()),
                        }
                    }
                    Field { label: "Passwort (optional)", html_for: "share-password",
                        Input {
                            id: "share-password",
                            r#type: InputType::Password,
                            value: password(),
                            autocomplete: "new-password",
                            oninput: move |e: FormEvent| password.set(e.value()),
                        }
                    }
                    Button { button_type: "submit", disabled: busy() || name().trim().is_empty(),
                        if busy() {
                            icons::LoaderCircle { class: "animate-spin" }
                        } else {
                            icons::Plus {}
                        }
                        "Link anlegen"
                    }
                }
            }
        }
        ConfirmModal {
            open: remove().is_some(),
            on_close: move |_| remove.set(None),
            on_confirm: confirm_remove,
            title: "Geteilten Link löschen?",
            message: "Der Link „{remove_name}“ funktioniert danach nicht mehr – auch nicht in Einbettungen.",
            confirm_label: "Löschen",
            busy: remove_busy(),
        }
    }
}
