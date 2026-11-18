//! Filter: Chips der aktiven Filter und das Filter-Modal mit Vorschlaegen.

use super::Dash;
use super::params::op_label;
use crate::api;
use crate::components::ui::button::{Button, ButtonVariant};
use crate::components::ui::modal::{Modal, ModalSize};
use crate::components::ui::select::Select;
use dioxus::prelude::*;
use nrzanalytics_shared::api::{Suggestion, SuggestionQuery};
use nrzanalytics_shared::labels;
use nrzanalytics_shared::query::{Filter, FilterOp, dimensions as d};

/// Gruppen des Filter-Modals: (Gruppe, Dimensionen).
const GROUPS: [(&str, &[&str]); 5] = [
    ("Seite", &[d::EVENT_PAGE, d::VISIT_ENTRY_PAGE, d::VISIT_EXIT_PAGE, d::EVENT_HOSTNAME]),
    (
        "Quelle",
        &[
            d::VISIT_SOURCE,
            d::VISIT_CHANNEL,
            d::VISIT_REFERRER,
            d::VISIT_UTM_MEDIUM,
            d::VISIT_UTM_SOURCE,
            d::VISIT_UTM_CAMPAIGN,
            d::VISIT_UTM_CONTENT,
            d::VISIT_UTM_TERM,
        ],
    ),
    ("Standort", &[d::VISIT_COUNTRY, d::VISIT_REGION, d::VISIT_CITY]),
    ("Technik", &[d::VISIT_BROWSER, d::VISIT_BROWSER_VERSION, d::VISIT_OS, d::VISIT_OS_VERSION, d::VISIT_DEVICE]),
    ("Verhalten", &[d::EVENT_GOAL]),
];

const OPS: [FilterOp; 4] = [FilterOp::Is, FilterOp::IsNot, FilterOp::Contains, FilterOp::ContainsNot];

/// Lesbarer Text einer Filterbedingung.
pub fn filter_text(f: &Filter) -> (String, String, String) {
    match f {
        Filter::Condition { op, dimension, clauses, .. } => {
            let values = clauses.iter().map(|c| labels::dimension_value(dimension, c)).collect::<Vec<_>>().join(" oder ");
            (labels::dimension(dimension), op_label(*op).to_string(), values)
        }
        Filter::Not(inner) => {
            let (a, b, c) = filter_text(inner);
            (format!("nicht ({a}"), b, format!("{c})"))
        }
        Filter::And(v) | Filter::Or(v) => ("Kombinierter Filter".into(), String::new(), format!("{} Bedingungen", v.len())),
    }
}

#[component]
pub fn FilterChips(dash: Dash) -> Element {
    let nav = navigator();
    let mut editing = use_signal(|| None::<usize>);
    if dash.params.filters.is_empty() {
        return rsx! {};
    }
    let filters = dash.params.filters.clone();
    rsx! {
        div { class: "flex flex-wrap items-center gap-2",
            for (i, f) in filters.iter().enumerate() {
                {
                    let (dim, op, values) = filter_text(f);
                    let remove = dash.route(dash.params.without_filter(i));
                    let editable = matches!(f, Filter::Condition { .. });
                    rsx! {
                        div { key: "{i}", class: "flex max-w-full items-center rounded-full border bg-card pl-3 text-sm shadow-xs",
                            button {
                                r#type: "button",
                                class: "flex min-w-0 items-center gap-1 py-1 outline-none hover:underline focus-visible:underline",
                                disabled: !editable,
                                title: "Filter bearbeiten",
                                onclick: move |_| editing.set(Some(i)),
                                span { class: "text-muted-foreground", "{dim}" }
                                span { class: "text-muted-foreground", "{op}" }
                                span { class: "truncate font-medium", "{values}" }
                            }
                            button {
                                r#type: "button",
                                class: "ml-1 mr-1 rounded-full p-1 text-muted-foreground hover:bg-accent hover:text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring",
                                "aria-label": "Filter {dim} entfernen",
                                onclick: move |_| { nav.push(remove.clone()); },
                                icons::X { class: "size-3.5" }
                            }
                        }
                    }
                }
            }
            if filters.len() > 1 {
                button {
                    r#type: "button",
                    class: "rounded-md px-2 py-1 text-sm text-muted-foreground hover:text-foreground hover:underline",
                    onclick: {
                        let r = dash.route(dash.params.without_filters());
                        move |_| { nav.push(r.clone()); }
                    },
                    "Alle entfernen"
                }
            }
        }
        FilterModal { dash: dash.clone(), open: editing().is_some(), initial: editing(), on_close: move |_| editing.set(None) }
    }
}

/// Filter-Modal. `initial` = Index eines bestehenden Filters zum Bearbeiten.
#[component]
pub fn FilterModal(dash: Dash, open: bool, initial: Option<usize>, on_close: EventHandler<()>) -> Element {
    rsx! {
        Modal {
            open,
            on_close: move |_| on_close.call(()),
            title: if initial.is_some() { "Filter bearbeiten" } else { "Filter hinzufügen" },
            size: ModalSize::Lg,
            if open {
                FilterForm { dash: dash.clone(), initial, on_close }
            }
        }
    }
}

#[component]
fn FilterForm(dash: Dash, initial: Option<usize>, on_close: EventHandler<()>) -> Element {
    let nav = navigator();
    let existing = initial.and_then(|i| dash.params.filters.get(i).cloned());
    let (init_dim, init_op, init_values) = match &existing {
        Some(Filter::Condition { op, dimension, clauses, .. }) => (dimension.clone(), *op, clauses.clone()),
        _ => (String::new(), FilterOp::Is, Vec::new()),
    };
    let init_prop = d::prop_key(&init_dim).map(str::to_string).unwrap_or_default();
    let mut dimension = use_signal(|| if d::prop_key(&init_dim).is_some() { "event:props".to_string() } else { init_dim.clone() });
    let mut prop_key = use_signal(|| init_prop);
    let mut op = use_signal(|| if OPS.contains(&init_op) { init_op } else { FilterOp::Is });
    let mut values = use_signal(|| init_values);
    let mut text = use_signal(String::new);
    let mut suggestions = use_signal(Vec::<Suggestion>::new);
    let mut highlighted = use_signal(|| None::<usize>);
    let mut generation = use_signal(|| 0u32);

    // Effektive Dimension (bei Eigenschaften `event:props:<name>`).
    let effective = move || {
        let dim = dimension();
        if dim == "event:props" {
            let k = prop_key();
            if k.is_empty() { String::new() } else { format!("{}{k}", d::EVENT_PROPS_PREFIX) }
        } else {
            dim
        }
    };

    // Vorschlaege laden (entprellt 250 ms).
    let base_filters: Vec<Filter> = dash.params.filters.iter().enumerate().filter(|(i, _)| Some(*i) != initial).map(|(_, f)| f.clone()).collect();
    let date_range = dash.params.date_range();
    let date = dash.params.date_opt();
    let domain = dash.domain.clone();
    use_effect(move || {
        let dim = effective();
        let q = text();
        let g = generation.peek().wrapping_add(1);
        generation.set(g);
        if dim.is_empty() {
            suggestions.set(Vec::new());
            return;
        }
        let req = SuggestionQuery {
            dimension: dim.clone(),
            q,
            date_range: date_range.clone(),
            date: date.clone(),
            filters: base_filters.iter().filter(|f| !f.dimensions().contains(&dim.as_str())).cloned().collect(),
        };
        let domain = domain.clone();
        spawn(async move {
            gloo_timers::future::TimeoutFuture::new(250).await;
            if *generation.peek() != g {
                return;
            }
            let r = api::post::<_, Vec<Suggestion>>(&format!("/api/stats/{}/suggestions", api::enc(&domain)), &req).await;
            if *generation.peek() == g {
                suggestions.set(r.unwrap_or_default());
                highlighted.set(None);
            }
        });
    });

    let mut add_value = move |v: String| {
        let v = v.trim().to_string();
        if v.is_empty() {
            return;
        }
        if !values.peek().contains(&v) {
            values.write().push(v);
        }
        text.set(String::new());
    };

    let shown: Vec<Suggestion> = suggestions().into_iter().filter(|s| !values().contains(&s.value)).take(12).collect();
    let shown_len = shown.len();
    let shown_for_keys = shown.clone();

    let dim_now = effective();
    let can_apply = !dim_now.is_empty() && (!values().is_empty() || !text().trim().is_empty());

    let apply = {
        let dash = dash.clone();
        move |_| {
            let dim = effective();
            let mut vals = values();
            let t = text().trim().to_string();
            if !t.is_empty() && !vals.contains(&t) {
                vals.push(t);
            }
            if dim.is_empty() || vals.is_empty() {
                return;
            }
            let o = op();
            let case_sensitive = !matches!(o, FilterOp::Contains | FilterOp::ContainsNot);
            let filter = Filter::Condition { op: o, dimension: dim.clone(), clauses: vals, case_sensitive };
            let mut p = dash.params.clone();
            match initial {
                Some(i) if i < p.filters.len() => p.filters[i] = filter,
                _ => p = p.with_filter(filter),
            }
            nav.push(dash.route(p));
            on_close.call(());
        }
    };

    let mut dim_options: Vec<(String, String)> = vec![(String::new(), "Dimension wählen …".into())];
    for (group, dims) in GROUPS.iter() {
        for dim in dims.iter() {
            if *dim == d::EVENT_GOAL && dash.site.goals.is_empty() {
                continue;
            }
            dim_options.push((dim.to_string(), format!("{group} · {}", labels::dimension(dim))));
        }
    }
    dim_options.push(("event:props".into(), "Verhalten · Eigenschaft".into()));
    let op_options: Vec<(String, String)> = OPS.iter().map(|o| (o.as_str().to_string(), op_label(*o).to_string())).collect();
    let prop_options: Vec<(String, String)> = std::iter::once((String::new(), "Eigenschaft wählen …".to_string()))
        .chain(dash.site.props.iter().map(|p| (p.clone(), p.clone())))
        .collect();
    let dim_for_labels = dim_now.clone();

    rsx! {
        div { class: "grid gap-4",
            div { class: "grid gap-3 sm:grid-cols-[1fr_10rem]",
                div { class: "grid gap-1.5",
                    label { r#for: "filter-dim", class: "text-sm font-medium", "Dimension" }
                    Select {
                        id: "filter-dim",
                        value: dimension(),
                        options: dim_options,
                        on_change: move |v: String| {
                            dimension.set(v);
                            values.set(Vec::new());
                            text.set(String::new());
                        },
                    }
                }
                div { class: "grid gap-1.5",
                    label { r#for: "filter-op", class: "text-sm font-medium", "Operator" }
                    Select {
                        id: "filter-op",
                        value: op().as_str().to_string(),
                        options: op_options,
                        on_change: move |v: String| op.set(FilterOp::parse(&v).unwrap_or(FilterOp::Is)),
                    }
                }
            }
            if dimension() == "event:props" {
                div { class: "grid gap-1.5",
                    label { r#for: "filter-prop", class: "text-sm font-medium", "Eigenschaft" }
                    if dash.site.props.is_empty() {
                        p { class: "text-sm text-muted-foreground", "Für diese Website sind noch keine Eigenschaften bekannt." }
                    } else {
                        Select {
                            id: "filter-prop",
                            value: prop_key(),
                            options: prop_options,
                            on_change: move |v: String| {
                                prop_key.set(v);
                                values.set(Vec::new());
                            },
                        }
                    }
                }
            }
            if !dim_now.is_empty() {
                div { class: "grid gap-1.5",
                    label { r#for: "filter-value", class: "text-sm font-medium", "Wert" }
                    div { class: "flex min-h-9 flex-wrap items-center gap-1.5 rounded-md border border-input px-2 py-1 shadow-xs focus-within:border-ring focus-within:ring-2 focus-within:ring-ring/50 dark:bg-input/30",
                        for (i, v) in values().iter().enumerate() {
                            span { key: "{v}", class: "flex items-center gap-1 rounded bg-secondary px-2 py-0.5 text-xs",
                                {labels::dimension_value(&dim_for_labels, v)}
                                button {
                                    r#type: "button",
                                    class: "text-muted-foreground hover:text-foreground",
                                    "aria-label": "Wert entfernen",
                                    onclick: move |_| {
                                        if i < values.peek().len() {
                                            values.write().remove(i);
                                        }
                                    },
                                    icons::X { class: "size-3" }
                                }
                            }
                        }
                        input {
                            id: "filter-value",
                            class: "h-7 min-w-[8rem] flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground",
                            placeholder: if values().is_empty() { "Wert eingeben oder auswählen …" } else { "weiterer Wert (oder) …" },
                            autocomplete: "off",
                            role: "combobox",
                            "aria-expanded": "{shown_len > 0}",
                            "aria-controls": "filter-suggestions",
                            value: text(),
                            onmounted: move |e| {
                                spawn(async move {
                                    let _ = e.data().set_focus(true).await;
                                });
                            },
                            oninput: move |e| {
                                text.set(e.value());
                                highlighted.set(None);
                            },
                            onkeydown: move |e: KeyboardEvent| {
                                match e.key() {
                                    Key::ArrowDown => {
                                        e.prevent_default();
                                        if shown_len > 0 {
                                            highlighted.set(Some(highlighted().map(|h| (h + 1) % shown_len).unwrap_or(0)));
                                        }
                                    }
                                    Key::ArrowUp => {
                                        e.prevent_default();
                                        if shown_len > 0 {
                                            highlighted.set(Some(highlighted().map(|h| (h + shown_len - 1) % shown_len).unwrap_or(shown_len - 1)));
                                        }
                                    }
                                    Key::Enter => {
                                        e.prevent_default();
                                        if let Some(s) = highlighted().and_then(|h| shown_for_keys.get(h)) {
                                            add_value(s.value.clone());
                                        } else {
                                            add_value(text());
                                        }
                                    }
                                    Key::Backspace if text().is_empty() => {
                                        values.write().pop();
                                    }
                                    _ => {}
                                }
                            },
                        }
                    }
                    p { class: "text-xs text-muted-foreground", "Mehrere Werte werden mit „oder“ verknüpft. Enter übernimmt den Wert." }
                    if !shown.is_empty() {
                        ul { id: "filter-suggestions", role: "listbox", class: "max-h-60 overflow-y-auto rounded-md border bg-popover p-1",
                            for (i, s) in shown.iter().enumerate() {
                                li {
                                    key: "{s.value}",
                                    role: "option",
                                    "aria-selected": "{highlighted() == Some(i)}",
                                    class: if highlighted() == Some(i) { "flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm bg-accent" } else { "flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm hover:bg-accent" },
                                    onclick: {
                                        let v = s.value.clone();
                                        move |_| add_value(v.clone())
                                    },
                                    if dim_for_labels == d::VISIT_COUNTRY {
                                        span { {nrzanalytics_shared::geo::flag(&s.value)} }
                                    }
                                    span { class: "truncate", "{s.label}" }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "flex justify-end gap-2 pt-2",
                Button { variant: ButtonVariant::Ghost, onclick: move |_| on_close.call(()), "Abbrechen" }
                Button { disabled: !can_apply, onclick: apply,
                    icons::Check {}
                    "Filter anwenden"
                }
            }
        }
    }
}
