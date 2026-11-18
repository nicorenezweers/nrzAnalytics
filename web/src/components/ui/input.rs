use dioxus::prelude::*;
use tw_merge::tw_merge;

#[allow(dead_code)]
#[derive(Default, Clone, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum InputType {
    #[default]
    Text,
    Email,
    Password,
    Number,
    Tel,
    Url,
    Search,
    Time,
    DatetimeLocal,
    Date,
    Month,
    Week,
    Hidden,
    File,
    Checkbox,
    Radio,
    Color,
    Range,
}

impl InputType {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        self.into()
    }
}

#[component]
pub fn Input(
    #[props(into, optional)] class: Option<String>,
    #[props(default = InputType::default())] r#type: InputType,
    #[props(into, optional)] placeholder: Option<String>,
    #[props(into, optional)] name: Option<String>,
    #[props(into, optional)] id: Option<String>,
    #[props(into, optional)] value: Option<String>,
    #[props(optional)] disabled: bool,
    #[props(optional)] readonly: bool,
    #[props(optional)] required: bool,
    #[props(optional)] autofocus: bool,
    #[props(into, optional)] autocomplete: Option<String>,
    #[props(into, optional)] aria_label: Option<String>,
    #[props(into, optional)] maxlength: Option<i64>,
    #[props(into, optional)] min: Option<String>,
    #[props(into, optional)] max: Option<String>,
    #[props(into, optional)] inputmode: Option<String>,
    #[props(into, optional)] oninput: Option<EventHandler<FormEvent>>,
    #[props(into, optional)] onkeydown: Option<EventHandler<KeyboardEvent>>,
    #[props(into, optional)] onfocus: Option<EventHandler<FocusEvent>>,
    #[props(into, optional)] onblur: Option<EventHandler<FocusEvent>>,
) -> Element {
    let merged_class = tw_merge!(
        "text-foreground file:text-foreground placeholder:text-muted-foreground selection:bg-primary selection:text-primary-foreground dark:bg-input/30 border-input flex h-9 w-full min-w-0 rounded-md border bg-transparent px-3 py-1 text-base shadow-xs transition-[color,box-shadow] outline-none file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm",
        "focus-visible:border-ring focus-visible:ring-ring/50",
        "focus-visible:ring-2",
        "aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive",
        "read-only:bg-muted",
        class.as_deref().unwrap_or("")
    );

    rsx! {
        input {
            "data-name": "Input",
            r#type: r#type.as_str(),
            class: "{merged_class}",
            placeholder: placeholder,
            value: value,
            name: name,
            id: id,
            disabled: disabled,
            readonly: readonly,
            required: required,
            autofocus: autofocus,
            autocomplete: autocomplete,
            "aria-label": aria_label,
            maxlength: maxlength,
            min: min,
            max: max,
            inputmode: inputmode,
            onmounted: move |e| {
                if autofocus {
                    spawn(async move {
                        let _ = e.data().set_focus(true).await;
                    });
                }
            },
            onkeydown: move |e| {
                if let Some(handler) = &onkeydown {
                    handler.call(e);
                }
            },
            onfocus: move |e| {
                if let Some(handler) = &onfocus {
                    handler.call(e);
                }
            },
            onblur: move |e| {
                if let Some(handler) = &onblur {
                    handler.call(e);
                }
            },
            oninput: move |e| {
                if let Some(handler) = &oninput {
                    handler.call(e);
                }
            },
        }
    }
}
