//! Globaler Zustand: Instanzstatus inkl. angemeldetem Konto, Share-Slug, Toasts, Thema.

use dioxus::prelude::*;
use nrzanalytics_shared::api::{Me, Status};

/// Status vom Server (`None` = noch nicht geladen).
pub static STATUS: GlobalSignal<Option<Status>> = Signal::global(|| None);
/// Fehler beim Laden des Status (Server nicht erreichbar).
pub static STATUS_ERROR: GlobalSignal<Option<String>> = Signal::global(|| None);
/// Kurzname des geteilten Links, wenn das Dashboard ueber `/share/...` laeuft.
pub static SHARE_SLUG: GlobalSignal<Option<String>> = Signal::global(|| None);
/// Kurzmeldungen unten rechts.
pub static TOASTS: GlobalSignal<Vec<Toast>> = Signal::global(Vec::new);
/// Dunkles Thema aktiv (Standard).
pub static DARK: GlobalSignal<bool> = Signal::global(|| true);
/// Zaehler, um die Website-Liste nach Aenderungen neu zu laden.
pub static SITES_REVISION: GlobalSignal<u32> = Signal::global(|| 0);

#[derive(Clone, PartialEq, Debug)]
pub struct Toast {
    pub id: u32,
    pub text: String,
    pub error: bool,
}

/// Angemeldetes Konto (reaktiv).
pub fn me() -> Option<Me> {
    STATUS.read().as_ref().and_then(|s| s.user.clone())
}

pub fn is_logged_in() -> bool {
    STATUS.peek().as_ref().is_some_and(|s| s.user.is_some())
}

/// Oeffentliche Adresse der Instanz (ohne `/` am Ende); Standard: Origin des Browsers.
pub fn public_url() -> String {
    let url = STATUS.read().as_ref().map(|s| s.public_url.trim_end_matches('/').to_string()).unwrap_or_default();
    if url.is_empty() { crate::util::origin() } else { url }
}

pub fn share_slug() -> Option<String> {
    SHARE_SLUG.peek().clone()
}

pub async fn load_status() {
    match crate::api::get::<Status>("/api/status").await {
        Ok(s) => {
            *STATUS.write() = Some(s);
            *STATUS_ERROR.write() = None;
        }
        Err(e) => {
            if STATUS.peek().is_none() {
                *STATUS_ERROR.write() = Some(e.message);
            }
        }
    }
}

/// Angemeldetes Konto setzen (nach Anmeldung, Kontoaenderung).
pub fn set_me(me: Option<Me>) {
    if let Some(s) = STATUS.write().as_mut() {
        if me.is_some() {
            s.needs_setup = false;
        }
        s.user = me;
    }
}

fn push(text: String, error: bool) {
    let id = TOASTS.peek().iter().map(|t| t.id).max().unwrap_or(0) + 1;
    TOASTS.write().push(Toast { id, text, error });
    spawn(async move {
        gloo_timers::future::TimeoutFuture::new(if error { 6000 } else { 3000 }).await;
        TOASTS.write().retain(|t| t.id != id);
    });
}

/// Erfolgsmeldung anzeigen.
pub fn toast(text: impl Into<String>) {
    push(text.into(), false);
}

/// Fehlermeldung anzeigen.
pub fn toast_error(text: impl Into<String>) {
    push(text.into(), true);
}

pub fn dismiss_toast(id: u32) {
    TOASTS.write().retain(|t| t.id != id);
}

const THEME_KEY: &str = "nrz_theme";

/// Thema aus localStorage lesen und anwenden (Standard dunkel).
pub fn init_theme() {
    let dark = crate::util::storage_get(THEME_KEY).map(|v| v != "light").unwrap_or(true);
    *DARK.write() = dark;
    crate::util::set_html_dark(dark);
}

/// Thema wechseln und merken.
pub fn set_dark(dark: bool) {
    *DARK.write() = dark;
    crate::util::set_html_dark(dark);
    crate::util::storage_set(THEME_KEY, if dark { "dark" } else { "light" });
}
