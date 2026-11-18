//! Routen der Oberflaeche. Statische Pfade stehen vor `/:domain`, damit sie Vorrang haben.

use crate::components::dashboard::Dashboard;
use crate::components::dashboard::params::DashParams;
use crate::components::layout::AppShell;
use crate::pages::account::Account;
use crate::pages::admin::Admin;
use crate::pages::auth::{Login, Register, Setup};
use crate::pages::misc::{NotFound, SiteSettingsIndex};
use crate::pages::share::Share;
use crate::pages::site_settings::SiteSettings;
use crate::pages::sites::{Home, NewSite, Sites};
use crate::pages::snippet::Snippet;
use dioxus::prelude::*;

#[derive(Routable, Clone, PartialEq, Debug)]
#[rustfmt::skip]
#[allow(clippy::enum_variant_names, clippy::large_enum_variant)]
pub enum Route {
    #[route("/login?:next")]
    Login { next: String },
    #[route("/register")]
    Register {},
    #[route("/setup")]
    Setup {},
    #[route("/share/:domain?:..params")]
    Share { domain: String, params: DashParams },
    #[layout(AppShell)]
        #[route("/")]
        Home {},
        #[route("/sites")]
        Sites {},
        #[route("/sites/new")]
        NewSite {},
        #[route("/settings?:tab")]
        Account { tab: String },
        #[route("/admin?:tab")]
        Admin { tab: String },
        #[route("/:domain/snippet")]
        Snippet { domain: String },
        #[route("/:domain/settings")]
        SiteSettingsIndex { domain: String },
        #[route("/:domain/settings/:section")]
        SiteSettings { domain: String, section: String },
        #[route("/:domain?:..params")]
        Dashboard { domain: String, params: DashParams },
        #[route("/:..segments")]
        NotFound { segments: Vec<String> },
}

impl Route {
    /// Dashboard einer Website mit Standard-Zustand.
    pub fn dashboard(domain: &str) -> Self {
        Self::Dashboard { domain: domain.to_string(), params: DashParams::default() }
    }

    /// Einstellungsbereich einer Website.
    pub fn site_settings(domain: &str, section: &str) -> Self {
        Self::SiteSettings { domain: domain.to_string(), section: section.to_string() }
    }

    /// Anmeldeseite, die danach hierher zurueckkehrt.
    pub fn login_here() -> Self {
        Self::Login { next: crate::util::current_path() }
    }
}
