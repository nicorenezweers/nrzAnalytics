//! Hilfsfunktionen: Browser-Zugriffe per `web-sys`, Datumsrechnung, Formatierung.

use wasm_bindgen::JsCast;

/// Origin des Browsers, z. B. `https://analytics.example.com`.
pub fn origin() -> String {
    web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default()
}

pub fn storage_get(key: &str) -> Option<String> {
    web_sys::window()?.local_storage().ok()??.get_item(key).ok()?
}

pub fn storage_set(key: &str, value: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(key, value);
    }
}

/// Klasse `dark` auf `<html>` setzen oder entfernen.
pub fn set_html_dark(dark: bool) {
    if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
        let list = el.class_list();
        let _ = if dark { list.add_1("dark") } else { list.remove_1("dark") };
    }
}

/// Text in die Zwischenablage kopieren (mit Toast).
pub fn copy(text: &str) {
    let text = text.to_string();
    let Some(promise) = clipboard_write(&text) else {
        if fallback_copy(&text) {
            crate::state::toast("In die Zwischenablage kopiert.");
        } else {
            crate::state::toast_error("Kopieren nicht möglich – bitte von Hand markieren.");
        }
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        let copied = wasm_bindgen_futures::JsFuture::from(promise).await.is_ok() || fallback_copy(&text);
        if copied {
            crate::state::toast("In die Zwischenablage kopiert.");
        } else {
            crate::state::toast_error("Kopieren nicht möglich – bitte von Hand markieren.");
        }
    });
}

/// `navigator.clipboard.writeText`, falls vorhanden (nur in sicheren Kontexten).
fn clipboard_write(text: &str) -> Option<js_sys::Promise> {
    let nav = web_sys::window()?.navigator();
    let clipboard = js_sys::Reflect::get(&nav, &"clipboard".into()).ok()?;
    if clipboard.is_undefined() || clipboard.is_null() {
        return None;
    }
    let write_text = js_sys::Reflect::get(&clipboard, &"writeText".into()).ok()?;
    let write_text: js_sys::Function = write_text.dyn_into().ok()?;
    let arg = wasm_bindgen::JsValue::from_str(text);
    write_text
        .call1(&clipboard, &arg)
        .ok()?
        .dyn_into::<js_sys::Promise>()
        .ok()
}

/// Kopieren ueber ein verstecktes Textfeld, auch ohne Clipboard-API.
fn fallback_copy(text: &str) -> bool {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return false;
    };
    let Some(body) = doc.body() else { return false };
    let Ok(el) = doc.create_element("textarea") else {
        return false;
    };
    let Ok(ta) = el.dyn_into::<web_sys::HtmlTextAreaElement>() else {
        return false;
    };
    ta.set_value(text);
    let _ = ta.set_attribute("readonly", "");
    let _ = ta.set_attribute(
        "style",
        "position:fixed;top:0;left:0;width:1px;height:1px;padding:0;border:0;opacity:0;",
    );
    if body.append_child(&ta).is_err() {
        return false;
    }
    ta.focus().ok();
    ta.select();
    let copied = js_sys::Reflect::get(&doc, &"execCommand".into())
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&doc, &"copy".into()).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    ta.remove();
    copied
}

/// Zeitzone des Browsers (IANA), Standard `Europe/Zurich`.
pub fn browser_timezone() -> String {
    let fmt = js_sys::Intl::DateTimeFormat::new(&js_sys::Array::new(), &js_sys::Object::new());
    let opts = fmt.resolved_options();
    js_sys::Reflect::get(&opts, &"timeZone".into())
        .ok()
        .and_then(|v| v.as_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Europe/Zurich".into())
}

/// Aktuelles Datum des Browsers als `YYYY-MM-DD`.
pub fn today() -> String {
    let d = js_sys::Date::new_0();
    format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
}

/// Element mit dieser ID fokussieren.
pub fn focus_id(id: &str) {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = el.focus();
    }
}

/// Ganz nach oben scrollen.
pub fn scroll_top() {
    if let Some(w) = web_sys::window() {
        w.scroll_to_with_x_and_y(0.0, 0.0);
    }
}

/// Volle Seite neu laden.
pub fn reload() {
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
}

/// Aktueller Pfad inkl. Query (fuer `?next=`).
pub fn current_path() -> String {
    web_sys::window()
        .map(|w| {
            let l = w.location();
            format!("{}{}", l.pathname().unwrap_or_default(), l.search().unwrap_or_default())
        })
        .unwrap_or_else(|| "/".into())
}

// ---------------------------------------------------------------------------------------------
// Datumsrechnung (proleptischer gregorianischer Kalender, ohne Zeitzonen)
// ---------------------------------------------------------------------------------------------

/// `YYYY-MM-DD` → (Jahr, Monat, Tag).
pub fn parse_date(s: &str) -> Option<(i32, u32, u32)> {
    let mut it = s.get(..10)?.split('-');
    let y = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then_some((y, m, d))
}

pub fn fmt_date(y: i32, m: u32, d: u32) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((y + i64::from(m <= 2)) as i32, m, d)
}

/// Tage addieren.
pub fn add_days(date: &str, days: i64) -> Option<String> {
    let (y, m, d) = parse_date(date)?;
    let (y, m, d) = civil_from_days(days_from_civil(y, m, d) + days);
    Some(fmt_date(y, m, d))
}

/// Monate addieren (Tag auf den 1. gesetzt).
pub fn add_months(date: &str, months: i32) -> Option<String> {
    let (y, m, _) = parse_date(date)?;
    let total = y * 12 + (m as i32 - 1) + months;
    Some(fmt_date(total.div_euclid(12), (total.rem_euclid(12) + 1) as u32, 1))
}

/// Jahre addieren (1. Januar).
pub fn add_years(date: &str, years: i32) -> Option<String> {
    let (y, _, _) = parse_date(date)?;
    Some(fmt_date(y + years, 1, 1))
}

/// Tage von `a` bis `b` (b − a).
pub fn days_between(a: &str, b: &str) -> Option<i64> {
    let (y1, m1, d1) = parse_date(a)?;
    let (y2, m2, d2) = parse_date(b)?;
    Some(days_from_civil(y2, m2, d2) - days_from_civil(y1, m1, d1))
}

/// Wochentag (0 = Montag).
pub fn weekday(date: &str) -> Option<u32> {
    let (y, m, d) = parse_date(date)?;
    Some((days_from_civil(y, m, d) + 3).rem_euclid(7) as u32)
}

pub const MONTHS_SHORT: [&str; 12] =
    ["Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"];
pub const MONTHS_LONG: [&str; 12] = [
    "Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November",
    "Dezember",
];
pub const WEEKDAYS_SHORT: [&str; 7] = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];

/// `2026-11-17` → `17. Nov`.
pub fn date_short(date: &str) -> String {
    match parse_date(date) {
        Some((_, m, d)) => format!("{d}. {}", MONTHS_SHORT[(m - 1) as usize]),
        None => date.to_string(),
    }
}

/// `2026-11-17` → `17. Nov 2026`.
pub fn date_medium(date: &str) -> String {
    match parse_date(date) {
        Some((y, m, d)) => format!("{d}. {} {y}", MONTHS_SHORT[(m - 1) as usize]),
        None => date.to_string(),
    }
}

/// `2026-11-17` → `Di, 17. Nov 2026`.
pub fn date_long(date: &str) -> String {
    match (parse_date(date), weekday(date)) {
        (Some((y, m, d)), Some(w)) => {
            format!("{}, {d}. {} {y}", WEEKDAYS_SHORT[w as usize], MONTHS_SHORT[(m - 1) as usize])
        }
        _ => date.to_string(),
    }
}

/// `2026-11-17` → `Nov 2026`.
pub fn month_year(date: &str) -> String {
    match parse_date(date) {
        Some((y, m, _)) => format!("{} {y}", MONTHS_SHORT[(m - 1) as usize]),
        None => date.to_string(),
    }
}

/// `2026-11-17` → `November 2026`.
pub fn month_year_long(date: &str) -> String {
    match parse_date(date) {
        Some((y, m, _)) => format!("{} {y}", MONTHS_LONG[(m - 1) as usize]),
        None => date.to_string(),
    }
}

/// Uhrzeit aus `YYYY-MM-DD HH:MM:SS` → `13:00`.
pub fn time_of(label: &str) -> String {
    label.get(11..16).map(str::to_string).unwrap_or_else(|| label.to_string())
}

/// Unix-Sekunden → `17.11.2026, 13:05` (lokale Zeit des Browsers).
pub fn datetime(ts: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ts as f64 * 1000.0));
    format!(
        "{:02}.{:02}.{:04}, {:02}:{:02}",
        d.get_date(),
        d.get_month() + 1,
        d.get_full_year(),
        d.get_hours(),
        d.get_minutes()
    )
}

/// Unix-Sekunden → `17.11.2026`.
pub fn date_of_ts(ts: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ts as f64 * 1000.0));
    format!("{:02}.{:02}.{:04}", d.get_date(), d.get_month() + 1, d.get_full_year())
}

/// Unix-Sekunden relativ: `vor 5 Minuten`.
pub fn relative(ts: i64) -> String {
    let now = (js_sys::Date::now() / 1000.0) as i64;
    let diff = (now - ts).max(0);
    match diff {
        0..=59 => "gerade eben".into(),
        60..=3599 => format!("vor {} Min.", diff / 60),
        3600..=86_399 => format!("vor {} Std.", diff / 3600),
        86_400..=2_591_999 => {
            let d = diff / 86_400;
            if d == 1 { "gestern".into() } else { format!("vor {d} Tagen") }
        }
        _ => date_of_ts(ts),
    }
}

/// Bytes lesbar: `12,3 MB`.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{} {}", format!("{v:.1}").replace('.', ","), UNITS[i]) }
}

/// Erster Buchstabe einer Domain fuer den Avatar.
pub fn initial(domain: &str) -> String {
    domain
        .trim_start_matches("www.")
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into())
}

/// Stabile Farbnummer (1–5) fuer eine Zeichenkette (Avatar-Farbe).
pub fn hue_index(s: &str) -> usize {
    s.bytes().fold(7usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)) % 5 + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(add_days("2026-11-30", 1).as_deref(), Some("2026-12-01"));
        assert_eq!(add_days("2024-03-01", -1).as_deref(), Some("2024-02-29"));
        assert_eq!(add_months("2026-01-31", -1).as_deref(), Some("2025-12-01"));
        assert_eq!(weekday("2026-11-17"), Some(1));
    }
}

/// Jetzt in einer IANA-Zeitzone als `YYYY-MM-DD HH:MM:SS` (wie die Zeit-Beschriftungen der API,
/// also lexikografisch mit ihnen vergleichbar); bei Fehlern die Zeit des Browsers.
pub fn now_in(tz: &str) -> String {
    use js_sys::{Array, Function, Object, Reflect};
    use wasm_bindgen::JsValue;
    let run = || -> Result<String, JsValue> {
        let intl = Reflect::get(&js_sys::global(), &"Intl".into())?;
        let ctor: Function = Reflect::get(&intl, &"DateTimeFormat".into())?.dyn_into()?;
        let opts = Object::new();
        Reflect::set(&opts, &"timeZone".into(), &tz.into())?;
        for (k, v) in [
            ("year", "numeric"),
            ("month", "2-digit"),
            ("day", "2-digit"),
            ("hour", "2-digit"),
            ("minute", "2-digit"),
            ("second", "2-digit"),
            ("hourCycle", "h23"),
        ] {
            Reflect::set(&opts, &k.into(), &v.into())?;
        }
        // Schwedisch formatiert ISO-nah: "2026-11-17 04:25:31".
        let fmt = Reflect::construct(&ctor, &Array::of2(&"sv-SE".into(), &opts))?;
        let format: Function = Reflect::get(&fmt, &"format".into())?.dyn_into()?;
        let out = format.call1(&fmt, &js_sys::Date::new_0())?;
        out.as_string().ok_or(JsValue::NULL)
    };
    match run().map(|s| s.replace(['\u{a0}', '\u{202f}'], " ")) {
        Ok(s) if s.len() == 19 && parse_date(&s[..10]).is_some() => s,
        _ => {
            let d = js_sys::Date::new_0();
            format!("{} {:02}:{:02}:{:02}", today(), d.get_hours(), d.get_minutes(), d.get_seconds())
        }
    }
}

/// Heutiges Datum in einer IANA-Zeitzone (`YYYY-MM-DD`); bei Fehlern das Browser-Datum.
pub fn today_in(tz: &str) -> String {
    use js_sys::{Array, Function, Object, Reflect};
    use wasm_bindgen::JsValue;
    let run = || -> Result<String, JsValue> {
        let intl = Reflect::get(&js_sys::global(), &"Intl".into())?;
        let ctor: Function = Reflect::get(&intl, &"DateTimeFormat".into())?.dyn_into()?;
        let opts = Object::new();
        Reflect::set(&opts, &"timeZone".into(), &tz.into())?;
        let fmt = Reflect::construct(&ctor, &Array::of2(&"en-CA".into(), &opts))?;
        let format: Function = Reflect::get(&fmt, &"format".into())?.dyn_into()?;
        let out = format.call1(&fmt, &js_sys::Date::new_0())?;
        out.as_string().ok_or(JsValue::NULL)
    };
    match run() {
        Ok(s) if parse_date(&s).is_some() => s,
        _ => today(),
    }
}
