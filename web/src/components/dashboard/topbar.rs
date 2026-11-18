//! Leiste oben: Website-Umschalter, Live-Anzeige, Filter-Knopf, Zeitraum und Vergleich.

use super::params::DashParams;
use super::{Dash, use_loader};
use crate::api;
use crate::components::common::LetterAvatar;
use crate::components::layout::SiteSwitcher;
use crate::components::ui::button::{Button, ButtonSize, ButtonVariant};
use crate::components::ui::dropdown::{Dropdown, DropdownAlign, DropdownItem, DropdownLabel, DropdownSeparator};
use crate::components::ui::input::{Input, InputType};
use crate::components::ui::modal::{Modal, ModalSize};
use crate::route::Route;
use crate::util;
use dioxus::prelude::*;
use nrzanalytics_shared::api::Realtime;
use nrzanalytics_shared::labels;

#[component]
pub fn TopBar(dash: Dash) -> Element {
    let embed = dash.params.embed;
    let mut filter_open = use_signal(|| false);
    rsx! {
        div { class: if embed { "flex flex-wrap items-center gap-2" } else { "sticky top-0 z-30 -mx-4 flex flex-wrap items-center gap-2 border-b border-transparent bg-background/85 px-4 py-2 backdrop-blur-xl lg:-mx-6 lg:px-6" },
            div { class: "flex min-w-0 items-center gap-2",
                if !dash.shared && crate::state::is_logged_in() {
                    SiteSwitcher { current: dash.domain.clone() }
                } else {
                    div { class: "flex min-w-0 items-center gap-2 px-1",
                        LetterAvatar { domain: dash.domain.clone() }
                        span { class: "truncate font-semibold", "{dash.domain}" }
                    }
                }
                RealtimeIndicator { dash: dash.clone() }
            }
            div { class: "ml-auto flex flex-wrap items-center gap-2",
                Button {
                    variant: ButtonVariant::Outline,
                    size: ButtonSize::Sm,
                    aria_label: "Filter hinzufügen",
                    onclick: move |_| filter_open.set(true),
                    icons::ListFilter {}
                    span { class: "hidden sm:inline", "Filter" }
                }
                PeriodPicker { dash: dash.clone() }
                ComparePicker { dash: dash.clone() }
                if dash.can_settings() && dash.site.role.is_some_and(|r| r.can_edit()) && !embed {
                    Button {
                        href: Route::site_settings(&dash.domain, "general").to_string(),
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::IconSm,
                        aria_label: "Einstellungen der Website",
                        title: "Einstellungen",
                        icons::Settings {}
                    }
                }
            }
        }
        super::filters::FilterModal { dash: dash.clone(), open: filter_open(), initial: None, on_close: move |_| filter_open.set(false) }
    }
}

/// „N aktuelle Besucher“ mit pulsierendem Punkt, alle 30 s aktualisiert.
#[component]
fn RealtimeIndicator(dash: Dash) -> Element {
    let nav = navigator();
    let mut tick = use_signal(|| 0u32);
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(30_000).await;
            tick += 1;
        }
    });
    let domain = dash.domain.clone();
    let rt = use_loader((domain, tick()), |(d, _): (String, u32)| async move {
        api::get::<Realtime>(&format!("/api/stats/{}/realtime", api::enc(&d))).await
    });
    let visitors = match &*rt.data.read() {
        Some(Ok(r)) => Some(r.visitors),
        _ => None,
    };
    let Some(v) = visitors else { return rsx! {} };
    let text = if v == 1 { "1 aktueller Besucher".to_string() } else { format!("{} aktuelle Besucher", labels::number_full(v as f64)) };
    let target = dash.route(dash.params.with_period("realtime", None));
    rsx! {
        button {
            r#type: "button",
            class: "flex items-center gap-2 rounded-md px-2 py-1 text-sm text-muted-foreground hover:bg-accent hover:text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring whitespace-nowrap",
            title: "Echtzeit anzeigen",
            onclick: move |_| {
                nav.push(target.clone());
            },
            span { class: "relative flex size-2.5",
                span { class: "absolute inline-flex size-full rounded-full bg-live animate-live" }
                span { class: "relative inline-flex size-2.5 rounded-full bg-live" }
            }
            span { class: "font-medium text-foreground tabular-nums", "{text}" }
        }
    }
}

/// Beschriftung des aktuellen Zeitraums.
pub fn period_label(dash: &Dash) -> String {
    let p = &dash.params;
    let today = dash.today();
    match p.period_key() {
        "day" => {
            if p.date.is_empty() || p.date == today {
                "Heute".into()
            } else if util::add_days(&today, -1).as_deref() == Some(p.date.as_str()) {
                "Gestern".into()
            } else {
                util::date_long(&p.date)
            }
        }
        "month" => {
            let this = util::add_months(&today, 0).unwrap_or_default();
            let date = util::add_months(if p.date.is_empty() { &today } else { &p.date }, 0).unwrap_or_default();
            if date == this {
                "Dieser Monat".into()
            } else if util::add_months(&today, -1).as_deref() == Some(date.as_str()) {
                "Letzter Monat".into()
            } else {
                util::month_year_long(&date)
            }
        }
        "year" => {
            let y = |d: &str| d.get(..4).unwrap_or("").to_string();
            if p.date.is_empty() || y(&p.date) == y(&today) { "Dieses Jahr".into() } else { format!("Jahr {}", y(&p.date)) }
        }
        "custom" => {
            if util::parse_date(&p.from).is_some() && util::parse_date(&p.to).is_some() {
                format!("{} – {}", util::date_short(&p.from), util::date_medium(&p.to))
            } else {
                labels::period("28d").into()
            }
        }
        key => labels::period(key).into(),
    }
}

/// Zeitraum davor/danach (bei Tag, Monat, Jahr). `None` = nicht moeglich.
pub fn shift(dash: &Dash, dir: i32) -> Option<DashParams> {
    let p = &dash.params;
    let today = dash.today();
    let base = if p.date.is_empty() { today.clone() } else { p.date.clone() };
    let next = match p.period_key() {
        "day" => util::add_days(&base, i64::from(dir))?,
        "month" => util::add_months(&base, dir)?,
        "year" => util::add_years(&base, dir)?,
        _ => return None,
    };
    // Nicht in die Zukunft blaettern.
    let first_of_next = match p.period_key() {
        "day" => next.clone(),
        "month" => util::add_months(&next, 0)?,
        _ => next.clone(),
    };
    if dir > 0 && first_of_next > today {
        return None;
    }
    let is_current = match p.period_key() {
        "day" => next == today,
        "month" => util::add_months(&next, 0) == util::add_months(&today, 0),
        _ => next.get(..4) == today.get(..4),
    };
    let key = p.period_key().to_string();
    Some(p.with_period(&key, if is_current { None } else { Some(next) }))
}

/// Tastenkuerzel → neuer Zustand.
pub fn shortcut(dash: &Dash, key: &str) -> Option<DashParams> {
    let p = &dash.params;
    let today = dash.today();
    let upper = key.to_ascii_uppercase();
    match upper.as_str() {
        "ARROWLEFT" => return shift(dash, -1),
        "ARROWRIGHT" => return shift(dash, 1),
        "E" => return Some(p.with_period("day", util::add_days(&today, -1))),
        "X" => {
            let mut n = p.clone();
            n.compare = if n.compare.is_empty() { "previous_period".into() } else { String::new() };
            return Some(n);
        }
        _ => {}
    }
    labels::PERIODS
        .iter()
        .find(|(_, _, k)| !k.is_empty() && *k == upper)
        .map(|(period, _, _)| p.with_period(period, None))
}

#[component]
fn PeriodPicker(dash: Dash) -> Element {
    let nav = navigator();
    let mut custom_open = use_signal(|| false);
    let label = period_label(&dash);
    let p = dash.params.clone();
    let key = p.period_key().to_string();
    let today = dash.today();
    let yesterday = util::add_days(&today, -1);
    let last_month = util::add_months(&today, -1);
    let prev = shift(&dash, -1);
    let next = shift(&dash, 1);
    let can_shift = matches!(key.as_str(), "day" | "month" | "year");
    let go = {
        let dash = dash.clone();
        move |np: DashParams| {
            nav.push(dash.route(np));
        }
    };
    let item = |period: &'static str, date: Option<String>, text: &'static str, shortcut: &'static str, active: bool| {
        let np = p.with_period(period, date);
        let go = go.clone();
        rsx! {
            DropdownItem { active, shortcut, onclick: move |_| go(np.clone()), "{text}" }
        }
    };
    let is_label = |l: &str| label == l;
    rsx! {
        div { class: "flex items-center",
            if can_shift {
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    aria_label: "Vorheriger Zeitraum",
                    disabled: prev.is_none(),
                    onclick: {
                        let go = go.clone();
                        let prev = prev.clone();
                        move |_| if let Some(np) = prev.clone() { go(np) }
                    },
                    icons::ChevronLeft {}
                }
            }
            Dropdown {
                align: DropdownAlign::End,
                aria_label: "Zeitraum wählen",
                trigger_class: "h-8 border bg-background px-3 shadow-xs hover:bg-accent dark:bg-input/30 dark:border-input",
                menu_class: "w-60",
                trigger: rsx! {
                    icons::Calendar { class: "size-4 text-muted-foreground" }
                    span { class: "max-w-[11rem] truncate", "{label}" }
                    icons::ChevronDown { class: "size-4 text-muted-foreground" }
                },
                {item("day", None, "Heute", "D", is_label("Heute"))}
                {item("day", yesterday.clone(), "Gestern", "E", is_label("Gestern"))}
                {item("realtime", None, "Echtzeit", "R", key == "realtime")}
                DropdownSeparator {}
                {item("7d", None, "Letzte 7 Tage", "W", key == "7d")}
                {item("28d", None, "Letzte 28 Tage", "F", key == "28d")}
                {item("30d", None, "Letzte 30 Tage", "T", key == "30d")}
                {item("91d", None, "Letzte 91 Tage", "N", key == "91d")}
                DropdownSeparator {}
                {item("month", None, "Dieser Monat", "M", is_label("Dieser Monat"))}
                {item("month", last_month.clone(), "Letzter Monat", "", is_label("Letzter Monat"))}
                DropdownSeparator {}
                {item("year", None, "Dieses Jahr", "Y", is_label("Dieses Jahr"))}
                {item("6mo", None, "Letzte 6 Monate", "", key == "6mo")}
                {item("12mo", None, "Letzte 12 Monate", "L", key == "12mo")}
                {item("all", None, "Gesamter Zeitraum", "A", key == "all")}
                DropdownSeparator {}
                DropdownItem { active: key == "custom", onclick: move |_| custom_open.set(true),
                    icons::CalendarRange {}
                    "Benutzerdefiniert …"
                }
            }
            if can_shift {
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    aria_label: "Nächster Zeitraum",
                    disabled: next.is_none(),
                    onclick: {
                        let go = go.clone();
                        let next = next.clone();
                        move |_| if let Some(np) = next.clone() { go(np) }
                    },
                    icons::ChevronRight {}
                }
            }
        }
        CustomRangeModal { dash: dash.clone(), open: custom_open(), on_close: move |_| custom_open.set(false) }
    }
}

#[component]
fn CustomRangeModal(dash: Dash, open: bool, on_close: EventHandler<()>) -> Element {
    let nav = navigator();
    let today = dash.today();
    let p = dash.params.clone();
    let init_from = if p.period_key() == "custom" { p.from.clone() } else { util::add_days(&today, -6).unwrap_or_default() };
    let init_to = if p.period_key() == "custom" { p.to.clone() } else { today.clone() };
    let mut from = use_signal(|| init_from);
    let mut to = use_signal(|| init_to);
    let valid = util::parse_date(&from()).is_some() && util::parse_date(&to()).is_some() && from() <= to();
    let min = dash.site.stats_start.clone().unwrap_or_default();
    rsx! {
        Modal {
            open,
            on_close: move |_| on_close.call(()),
            title: "Benutzerdefinierter Zeitraum",
            size: ModalSize::Sm,
            footer: rsx! {
                Button { variant: ButtonVariant::Ghost, onclick: move |_| on_close.call(()), "Abbrechen" }
                Button {
                    disabled: !valid,
                    onclick: {
                        let dash = dash.clone();
                        move |_| {
                            nav.push(dash.route(dash.params.with_custom(&from(), &to())));
                            on_close.call(());
                        }
                    },
                    "Übernehmen"
                }
            },
            div { class: "grid grid-cols-2 gap-3",
                div { class: "grid gap-1.5",
                    label { r#for: "range-from", class: "text-sm font-medium", "Von" }
                    Input { id: "range-from", r#type: InputType::Date, value: from(), min: min.clone(), max: today.clone(), oninput: move |e: FormEvent| from.set(e.value()) }
                }
                div { class: "grid gap-1.5",
                    label { r#for: "range-to", class: "text-sm font-medium", "Bis" }
                    Input { id: "range-to", r#type: InputType::Date, value: to(), min: min.clone(), max: today.clone(), oninput: move |e: FormEvent| to.set(e.value()) }
                }
            }
            if !valid {
                p { class: "mt-3 text-xs text-destructive", "Bitte zwei gültige Daten wählen (Von ≤ Bis)." }
            }
        }
    }
}

#[component]
fn ComparePicker(dash: Dash) -> Element {
    let nav = navigator();
    let p = dash.params.clone();
    if p.is_realtime() || p.period_key() == "all" {
        return rsx! {};
    }
    let current = p.compare.clone();
    let label = match current.as_str() {
        "previous_period" => "Vorheriger Zeitraum",
        "year_over_year" => "Vorjahr",
        _ => "Kein Vergleich",
    };
    let opt = move |value: &'static str, text: &'static str| {
        let mut np = p.clone();
        np.compare = value.to_string();
        let route = dash.route(np);
        let active = current == value;
        rsx! {
            DropdownItem { active, onclick: move |_| { nav.push(route.clone()); },
                if active { icons::Check {} } else { span { class: "size-4" } }
                "{text}"
            }
        }
    };
    rsx! {
        Dropdown {
            align: DropdownAlign::End,
            aria_label: "Vergleich wählen",
            trigger_class: if label == "Kein Vergleich" { "h-8 border bg-background px-3 shadow-xs hover:bg-accent dark:bg-input/30 dark:border-input text-muted-foreground" } else { "h-8 border bg-background px-3 shadow-xs hover:bg-accent dark:bg-input/30 dark:border-input" },
            trigger: rsx! {
                icons::ArrowLeftRight { class: "size-4 text-muted-foreground" }
                span { class: "hidden md:inline", "{label}" }
            },
            DropdownLabel { "Vergleichen mit (X)" }
            {opt("", "Kein Vergleich")}
            {opt("previous_period", "Vorheriger Zeitraum")}
            {opt("year_over_year", "Vorjahr")}
        }
    }
}
