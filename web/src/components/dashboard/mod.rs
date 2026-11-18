//! Das Dashboard einer Website (wie Plausible, im nrz-Stil).
//!
//! Der gesamte Zustand (Zeitraum, Vergleich, Filter, Kennzahl, Intervall) steht in der URL
//! ([`params::DashParams`]); jede Aenderung ist eine Navigation (teilbar, Zurueck-Taste).

pub mod behaviours;
pub mod details;
pub mod empty;
pub mod filters;
pub mod params;
pub mod reports;
pub mod top_stats;
pub mod topbar;

use crate::api::{self, ApiErr, ApiResult};
use crate::components::common::{ErrorState, PageSkeleton};
use crate::route::Route;
use crate::state;
use dioxus::prelude::*;
use nrzanalytics_shared::api::Site;
use nrzanalytics_shared::query::Query;
use params::DashParams;
use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

/// Alles, was Karten des Dashboards brauchen. Aendert sich etwas, laden die Karten neu.
#[derive(Clone, PartialEq, Debug)]
pub struct Dash {
    pub domain: String,
    pub site: Site,
    pub params: DashParams,
    /// Dashboard ueber einen geteilten Link.
    pub shared: bool,
    /// Zaehler fuer das Neuladen im Echtzeit-Modus.
    pub tick: u32,
}

impl Dash {
    /// Route fuer einen anderen Zustand.
    pub fn route(&self, params: DashParams) -> Route {
        if self.shared {
            Route::Share { domain: self.domain.clone(), params }
        } else {
            Route::Dashboard { domain: self.domain.clone(), params }
        }
    }

    pub fn query(&self, metrics: &[&str]) -> Query {
        self.params.query(metrics)
    }

    /// Heute in der Zeitzone der Website.
    pub fn today(&self) -> String {
        crate::util::today_in(&self.site.timezone)
    }

    pub fn can_settings(&self) -> bool {
        !self.shared && self.site.role.is_some()
    }

    pub fn has_revenue(&self) -> bool {
        self.site.goals.iter().any(|g| g.currency.is_some())
    }
}

/// Ergebnis eines Ladevorgangs; alte Daten bleiben stehen, bis neue da sind.
pub struct Loaded<T: 'static> {
    pub data: Signal<Option<ApiResult<T>>>,
    pub loading: Signal<bool>,
}

impl<T: 'static> Clone for Loaded<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> Copy for Loaded<T> {}

/// Laedt Daten neu, sobald sich `key` aendert. Veraltete Antworten werden verworfen.
pub fn use_loader<K, T, F, Fut>(key: K, f: F) -> Loaded<T>
where
    K: Clone + PartialEq + 'static,
    T: 'static,
    F: Fn(K) -> Fut + 'static,
    Fut: Future<Output = ApiResult<T>> + 'static,
{
    let mut data = use_signal(|| None::<ApiResult<T>>);
    let mut loading = use_signal(|| true);
    let mut generation = use_signal(|| 0u32);
    let f = Rc::new(f);
    use_effect(use_reactive((&key,), move |(key,)| {
        let g = generation.peek().wrapping_add(1);
        generation.set(g);
        loading.set(true);
        let f = f.clone();
        spawn(async move {
            let r = f(key).await;
            if *generation.peek() == g {
                data.set(Some(r));
                loading.set(false);
            }
        });
    }));
    Loaded { data, loading }
}

#[component]
pub fn Dashboard(domain: String, params: DashParams) -> Element {
    rsx! { DashboardView { domain, params, shared: false } }
}

/// Dashboard fuer normale, oeffentliche und geteilte Ansicht.
#[component]
pub fn DashboardView(domain: String, params: DashParams, shared: bool) -> Element {
    let nav = navigator();
    let site = use_loader(domain.clone(), |d: String| async move {
        api::get::<Site>(&format!("/api/sites/{}", api::enc(&d))).await
    });
    // Nicht angemeldet und nicht oeffentlich → Anmeldung.
    use_effect(move || {
        if let Some(Err(e)) = &*site.data.read()
            && e.unauthorized() && !shared && !state::is_logged_in() {
                nav.replace(Route::login_here());
            }
    });

    // Echtzeit: alle 15 s neu laden.
    let mut tick = use_signal(|| 0u32);
    let realtime = params.is_realtime();
    let mut rt = use_signal(|| realtime);
    use_effect(use_reactive((&realtime,), move |(r,)| rt.set(r)));
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(15_000).await;
            if *rt.peek() {
                tick += 1;
            }
        }
    });

    let current = site.data.read();
    let s = match &*current {
        None => return rsx! { PageSkeleton {} },
        Some(Err(e)) => return rsx! { SiteError { error: e.clone() } },
        Some(Ok(s)) => s.clone(),
    };
    drop(current);
    let dash = Dash { domain: domain.clone(), site: s, params: params.clone(), shared, tick: tick() };
    rsx! {
        crate::components::common::PageTitle { title: "{domain} – nrzAnalytics" }
        Shortcuts { dash: dash.clone() }
        DashboardBody { dash }
    }
}

#[component]
fn SiteError(error: ApiErr) -> Element {
    let text = if error.status == 404 {
        "Diese Website gibt es nicht oder du hast keinen Zugriff darauf.".to_string()
    } else {
        error.message.clone()
    };
    rsx! {
        ErrorState { message: text, on_retry: move |_| crate::util::reload() }
    }
}

#[component]
fn DashboardBody(dash: Dash) -> Element {
    let embed = dash.params.embed;
    let no_data = dash.site.stats_start.is_none() && !dash.params.is_realtime();
    rsx! {
        div { class: if embed { "flex flex-col gap-4" } else { "flex flex-col gap-5" },
            topbar::TopBar { dash: dash.clone() }
            filters::FilterChips { dash: dash.clone() }
            if no_data {
                empty::EmptyState { dash: dash.clone() }
            } else {
                top_stats::GraphCard { dash: dash.clone() }
                div { class: "grid grid-cols-1 gap-5 lg:grid-cols-2",
                    reports::SourcesCard { dash: dash.clone() }
                    reports::PagesCard { dash: dash.clone() }
                    reports::LocationsCard { dash: dash.clone() }
                    reports::DevicesCard { dash: dash.clone() }
                }
                if !dash.site.goals.is_empty() || !dash.site.props.is_empty() || !dash.site.funnels.is_empty() {
                    behaviours::BehavioursCard { dash: dash.clone() }
                }
            }
            if !embed {
                div { class: "pt-2 text-center text-xs text-muted-foreground",
                    "Zeitzone: {dash.site.timezone}"
                    if dash.shared {
                        " · bereitgestellt mit nrzAnalytics"
                    }
                }
            }
        }
    }
}

/// Tastenkuerzel fuer Zeitraeume (aus `labels::PERIODS`), ← → zum Blaettern, X fuer Vergleich.
#[component]
fn Shortcuts(dash: Dash) -> Element {
    let nav = navigator();
    let latest = use_hook(|| Rc::new(RefCell::new(dash.clone())));
    *latest.borrow_mut() = dash.clone();
    let holder = use_hook(|| Rc::new(RefCell::new(None::<wasm_bindgen::closure::Closure<dyn FnMut(web_sys::KeyboardEvent)>>)));
    {
        let latest = latest.clone();
        let holder = holder.clone();
        use_hook(move || {
            use wasm_bindgen::JsCast;
            let cb = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
                if e.ctrl_key() || e.meta_key() || e.alt_key() || e.default_prevented() {
                    return;
                }
                let editable = e
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .map(|el| {
                        let tag = el.tag_name().to_ascii_uppercase();
                        tag == "INPUT" || tag == "TEXTAREA" || tag == "SELECT" || el.closest("[role=dialog]").ok().flatten().is_some()
                    })
                    .unwrap_or(false);
                if editable {
                    return;
                }
                let d = latest.borrow().clone();
                if let Some(p) = topbar::shortcut(&d, &e.key()) {
                    e.prevent_default();
                    nav.push(d.route(p));
                }
            });
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                let _ = doc.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
            }
            *holder.borrow_mut() = Some(cb);
        });
    }
    use_drop(move || {
        use wasm_bindgen::JsCast;
        if let (Some(cb), Some(doc)) = (holder.borrow_mut().take(), web_sys::window().and_then(|w| w.document())) {
            let _ = doc.remove_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
        }
    });
    rsx! {}
}
