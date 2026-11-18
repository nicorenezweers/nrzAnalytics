//! Einstellungen einer Website (`/{domain}/settings/{section}`).
//!
//! Kopf mit Domain, Seitenleiste mit Abschnitten (mobil als waagrechte Leiste) und der Inhalt des
//! aktiven Abschnitts. Die Website wird hier geladen; jeder Abschnitt laedt seine Daten selbst.

mod danger;
mod funnels;
mod general;
mod goals;
mod helpers;
mod imports_exports;
mod people;
mod properties;
mod shields;
mod visibility;

use crate::api;
use crate::components::common::{ErrorState, LetterAvatar, PageSkeleton, RequireLogin};
use crate::route::Route;
use dioxus::prelude::*;
use helpers::{ReadOnlyNote, can_admin};
use nrzanalytics_shared::api::{Role, Site};

/// Abschnitte der Einstellungsseite.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Section {
    General,
    People,
    Visibility,
    Goals,
    Funnels,
    Properties,
    Shields,
    ImportsExports,
    Danger,
}

impl Section {
    const ALL: [Section; 9] = [
        Self::General,
        Self::People,
        Self::Visibility,
        Self::Goals,
        Self::Funnels,
        Self::Properties,
        Self::Shields,
        Self::ImportsExports,
        Self::Danger,
    ];

    fn slug(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::People => "people",
            Self::Visibility => "visibility",
            Self::Goals => "goals",
            Self::Funnels => "funnels",
            Self::Properties => "properties",
            Self::Shields => "shields",
            Self::ImportsExports => "imports-exports",
            Self::Danger => "danger",
        }
    }

    /// Unbekannte Abschnitte fallen auf „Allgemein“ zurueck.
    fn parse(s: &str) -> Self {
        Self::ALL.into_iter().find(|x| x.slug() == s).unwrap_or(Self::General)
    }

    fn label(self) -> &'static str {
        match self {
            Self::General => "Allgemein",
            Self::People => "Personen",
            Self::Visibility => "Sichtbarkeit",
            Self::Goals => "Ziele",
            Self::Funnels => "Trichter",
            Self::Properties => "Eigenschaften",
            Self::Shields => "Schutzregeln",
            Self::ImportsExports => "Import/Export",
            Self::Danger => "Gefahrenzone",
        }
    }

    /// Personen und Gefahrenzone sind erst ab Admin sichtbar.
    fn visible(self, role: Option<Role>) -> bool {
        match self {
            Self::People | Self::Danger => can_admin(role),
            _ => true,
        }
    }

    fn icon(self) -> Element {
        match self {
            Self::General => rsx! { icons::Settings {} },
            Self::People => rsx! { icons::Users {} },
            Self::Visibility => rsx! { icons::Eye {} },
            Self::Goals => rsx! { icons::Target {} },
            Self::Funnels => rsx! { icons::Funnel {} },
            Self::Properties => rsx! { icons::Tags {} },
            Self::Shields => rsx! { icons::Shield {} },
            Self::ImportsExports => rsx! { icons::Download {} },
            Self::Danger => rsx! { icons::TriangleAlert {} },
        }
    }
}

#[component]
pub fn SiteSettings(domain: String, section: String) -> Element {
    rsx! {
        RequireLogin {
            SettingsPage { domain, section }
        }
    }
}

#[component]
fn SettingsPage(domain: String, section: String) -> Element {
    // Website laden, neu bei Domainwechsel.
    let mut site = use_resource(use_reactive!(|domain| async move { api::get::<Site>(&format!("/api/sites/{}", api::enc(&domain))).await }));
    let current = Section::parse(&section);
    let loaded = site.read().clone();
    let site_data = match loaded {
        None => return rsx! { PageSkeleton {} },
        Some(Err(e)) => {
            return rsx! {
                ErrorState { message: e.message, on_retry: move |_| site.restart() }
            };
        }
        Some(Ok(s)) => s,
    };
    let role = site_data.role;
    let reload = move |_: ()| site.restart();

    rsx! {
        div { class: "mb-6 flex flex-wrap items-center gap-4",
            LetterAvatar { domain: site_data.domain.clone(), class: "size-10 text-base" }
            div { class: "min-w-0 flex-1",
                h1 { class: "truncate text-2xl font-semibold tracking-tight", "{site_data.domain}" }
                p { class: "text-sm text-muted-foreground",
                    "Website-Einstellungen"
                    if let Some(r) = role {
                        " · Deine Rolle: {r.label()}"
                    }
                }
            }
            Link {
                to: Route::dashboard(&site_data.domain),
                class: "inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-sm text-muted-foreground hover:bg-accent hover:text-accent-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&>svg]:size-4",
                icons::ArrowLeft {}
                "Zurück zum Dashboard"
            }
        }
        div { class: "grid gap-6 lg:grid-cols-[13rem_minmax(0,1fr)]",
            nav { "aria-label": "Einstellungsbereiche", class: "-mx-1 overflow-x-auto lg:mx-0 lg:overflow-visible",
                ul { class: "flex gap-1 px-1 pb-1 lg:flex-col lg:px-0",
                    for s in Section::ALL.into_iter().filter(|s| s.visible(role)) {
                        li { key: "{s.slug()}", class: "shrink-0",
                            Link {
                                to: Route::site_settings(&site_data.domain, s.slug()),
                                "aria-current": if s == current { "page" } else { "false" },
                                class: if s == current {
                                    "flex items-center gap-2 whitespace-nowrap rounded-md bg-accent px-3 py-2 text-sm font-medium text-accent-foreground [&>svg]:size-4"
                                } else {
                                    "flex items-center gap-2 whitespace-nowrap rounded-md px-3 py-2 text-sm text-muted-foreground hover:bg-accent/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&>svg]:size-4"
                                },
                                {s.icon()}
                                span { "{s.label()}" }
                            }
                        }
                    }
                }
            }
            div { key: "{site_data.domain}-{current.slug()}", class: "grid min-w-0 content-start gap-6",
                if !current.visible(role) {
                    ReadOnlyNote { text: "Für diesen Bereich fehlen dir die Rechte. Wende dich an einen Admin der Website." }
                } else {
                    match current {
                        Section::General => rsx! { general::GeneralSection { site: site_data.clone(), on_saved: reload } },
                        Section::People => rsx! { people::PeopleSection { domain: site_data.domain.clone(), role } },
                        Section::Visibility => rsx! { visibility::VisibilitySection { site: site_data.clone(), on_saved: reload } },
                        Section::Goals => rsx! { goals::GoalsSection { domain: site_data.domain.clone(), role } },
                        Section::Funnels => rsx! { funnels::FunnelsSection { domain: site_data.domain.clone(), role } },
                        Section::Properties => rsx! { properties::PropertiesSection { domain: site_data.domain.clone(), role } },
                        Section::Shields => rsx! { shields::ShieldsSection { domain: site_data.domain.clone(), role } },
                        Section::ImportsExports => rsx! { imports_exports::ImportsExportsSection { domain: site_data.domain.clone() } },
                        Section::Danger => rsx! { danger::DangerSection { domain: site_data.domain.clone(), role } },
                    }
                }
            }
        }
    }
}
