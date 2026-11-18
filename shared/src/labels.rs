//! Deutsche Beschriftungen fuer Kennzahlen, Dimensionen, Zeitraeume, Kanaele und Geraete.
//!
//! Gespeichert und in der API stehen die englischen Werte (kompatibel mit Plausible), angezeigt
//! werden diese Beschriftungen.

use crate::query::{dimensions as d, metrics as m};

/// Beschriftung einer Kennzahl.
pub fn metric(metric: &str) -> &'static str {
    match metric {
        m::VISITORS => "Besucher",
        m::VISITS => "Besuche",
        m::PAGEVIEWS => "Seitenaufrufe",
        m::VIEWS_PER_VISIT => "Seiten pro Besuch",
        m::BOUNCE_RATE => "Absprungrate",
        m::VISIT_DURATION => "Besuchsdauer",
        m::EVENTS => "Ereignisse",
        m::CONVERSION_RATE => "Konversionsrate",
        m::GROUP_CONVERSION_RATE => "Konversionsrate",
        m::TIME_ON_PAGE => "Zeit auf der Seite",
        m::SCROLL_DEPTH => "Scrolltiefe",
        m::PERCENTAGE => "Anteil",
        m::EXIT_RATE => "Ausstiegsrate",
        m::TOTAL_REVENUE => "Umsatz",
        m::AVERAGE_REVENUE => "Ø Umsatz",
        _ => "Kennzahl",
    }
}

/// Lange Beschriftung fuer die Kennzahlen-Leiste oben im Dashboard.
pub fn metric_long(metric: &str) -> &'static str {
    match metric {
        m::VISITORS => "Eindeutige Besucher",
        m::VISITS => "Besuche insgesamt",
        m::PAGEVIEWS => "Seitenaufrufe insgesamt",
        m::EVENTS => "Ereignisse insgesamt",
        other => self::metric(other),
    }
}

/// Beschriftung einer Dimension.
pub fn dimension(dimension: &str) -> String {
    if let Some(key) = d::prop_key(dimension) {
        return format!("Eigenschaft {key}");
    }
    match dimension {
        d::EVENT_NAME => "Ereignis",
        d::EVENT_GOAL => "Ziel",
        d::EVENT_PAGE => "Seite",
        d::EVENT_HOSTNAME => "Hostname",
        d::VISIT_ENTRY_PAGE => "Einstiegsseite",
        d::VISIT_EXIT_PAGE => "Ausstiegsseite",
        d::VISIT_ENTRY_PAGE_HOSTNAME => "Einstiegs-Hostname",
        d::VISIT_EXIT_PAGE_HOSTNAME => "Ausstiegs-Hostname",
        d::VISIT_SOURCE => "Quelle",
        d::VISIT_REFERRER => "Verweis-URL",
        d::VISIT_CHANNEL => "Kanal",
        d::VISIT_UTM_MEDIUM => "UTM-Medium",
        d::VISIT_UTM_SOURCE => "UTM-Quelle",
        d::VISIT_UTM_CAMPAIGN => "UTM-Kampagne",
        d::VISIT_UTM_CONTENT => "UTM-Inhalt",
        d::VISIT_UTM_TERM => "UTM-Begriff",
        d::VISIT_DEVICE => "Gerät",
        d::VISIT_BROWSER => "Browser",
        d::VISIT_BROWSER_VERSION => "Browser-Version",
        d::VISIT_OS => "Betriebssystem",
        d::VISIT_OS_VERSION => "Version des Betriebssystems",
        d::VISIT_COUNTRY | d::VISIT_COUNTRY_NAME => "Land",
        d::VISIT_REGION | d::VISIT_REGION_NAME => "Region",
        d::VISIT_CITY | d::VISIT_CITY_NAME => "Stadt",
        d::TIME | d::TIME_MINUTE | d::TIME_HOUR | d::TIME_DAY | d::TIME_WEEK | d::TIME_MONTH => "Zeit",
        _ => "Dimension",
    }
    .to_string()
}

/// Benannte Zeitraeume fuer die Auswahl im Dashboard: (Schluessel, Beschriftung, Tastenkuerzel).
pub const PERIODS: [(&str, &str, &str); 11] = [
    ("day", "Heute", "D"),
    ("realtime", "Echtzeit", "R"),
    ("7d", "Letzte 7 Tage", "W"),
    ("28d", "Letzte 28 Tage", "F"),
    ("30d", "Letzte 30 Tage", "T"),
    ("91d", "Letzte 91 Tage", "N"),
    ("month", "Dieser Monat", "M"),
    ("6mo", "Letzte 6 Monate", ""),
    ("12mo", "Letzte 12 Monate", "L"),
    ("year", "Dieses Jahr", "Y"),
    ("all", "Gesamter Zeitraum", "A"),
];

/// Beschriftung eines benannten Zeitraums.
pub fn period(key: &str) -> &'static str {
    PERIODS
        .iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, l, _)| *l)
        .unwrap_or("Benutzerdefiniert")
}

/// Beschriftung eines Zeitintervalls.
pub fn interval(interval: &str) -> &'static str {
    match interval {
        "minute" => "Minuten",
        "hour" => "Stunden",
        "day" => "Tage",
        "week" => "Wochen",
        "month" => "Monate",
        _ => "Intervall",
    }
}

/// Kanaele (Werte wie bei Plausible).
pub const CHANNELS: [(&str, &str); 19] = [
    ("Direct", "Direkt"),
    ("Organic Search", "Organische Suche"),
    ("Paid Search", "Bezahlte Suche"),
    ("Organic Social", "Organische soziale Medien"),
    ("Paid Social", "Bezahlte soziale Medien"),
    ("Organic Video", "Organisches Video"),
    ("Paid Video", "Bezahltes Video"),
    ("Organic Shopping", "Organisches Shopping"),
    ("Paid Shopping", "Bezahltes Shopping"),
    ("Email", "E-Mail"),
    ("Affiliates", "Partnerprogramme"),
    ("Display", "Display-Werbung"),
    ("Referral", "Verweise"),
    ("Cross-network", "Netzwerkübergreifend"),
    ("SMS", "SMS"),
    ("Audio", "Audio"),
    ("Mobile Push Notifications", "Push-Benachrichtigungen"),
    ("Paid Other", "Sonstige bezahlte"),
    ("(other)", "Sonstige"),
];

/// Deutsche Beschriftung eines Kanals (unbekannte unveraendert).
pub fn channel(value: &str) -> String {
    CHANNELS
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, l)| (*l).to_string())
        .unwrap_or_else(|| value.to_string())
}

/// Deutsche Beschriftung einer Geraeteklasse.
pub fn device(value: &str) -> String {
    match value {
        "Desktop" => "Desktop".into(),
        "Laptop" => "Laptop".into(),
        "Tablet" => "Tablet".into(),
        "Mobile" => "Smartphone".into(),
        "" => crate::query::NONE_VALUE.into(),
        other => other.to_string(),
    }
}

/// Anzeige einer Quelle (`Direct / None` auf Deutsch).
pub fn source(value: &str) -> String {
    if value == crate::query::DIRECT_SOURCE {
        "Direkt / keine".into()
    } else {
        value.to_string()
    }
}

/// Anzeigewert einer Dimension, z. B. Laendercode → Name, Kanal → deutsch.
pub fn dimension_value(dimension: &str, value: &str) -> String {
    match dimension {
        d::VISIT_COUNTRY => crate::geo::country_name(value)
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string()),
        d::VISIT_CHANNEL => channel(value),
        d::VISIT_DEVICE => device(value),
        d::VISIT_SOURCE => source(value),
        _ => value.to_string(),
    }
}

/// Dauer in Sekunden lesbar: `42s`, `3m 05s`, `1h 02m`.
pub fn duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0s".into();
    }
    let s = seconds.round() as u64;
    let (h, rest) = (s / 3600, s % 3600);
    let (min, sec) = (rest / 60, rest % 60);
    if h > 0 {
        format!("{h}h {min:02}m")
    } else if min > 0 {
        format!("{min}m {sec:02}s")
    } else {
        format!("{sec}s")
    }
}

/// Zahl kurz und mit Schweizer Tausendertrennung: `987`, `1,2k`, `12k`, `3,4 Mio.`.
pub fn number_short(value: f64) -> String {
    let v = value.abs();
    let sign = if value < 0.0 { "-" } else { "" };
    if v < 1_000.0 {
        if (v - v.round()).abs() < f64::EPSILON {
            format!("{sign}{}", v.round() as u64)
        } else {
            format!("{sign}{v:.1}")
        }
    } else if v < 10_000.0 {
        format!("{sign}{}k", trim_decimal(v / 1_000.0))
    } else if v < 1_000_000.0 {
        format!("{sign}{}k", (v / 1_000.0).round() as u64)
    } else if v < 10_000_000.0 {
        format!("{sign}{} Mio.", trim_decimal(v / 1_000_000.0))
    } else {
        format!("{sign}{} Mio.", (v / 1_000_000.0).round() as u64)
    }
}

/// Ganze Zahl mit Tausendertrennzeichen (`12’345`).
pub fn number_full(value: f64) -> String {
    let n = value.round() as i64;
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('’');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn trim_decimal(v: f64) -> String {
    let s = format!("{v:.1}");
    let s = s.strip_suffix(".0").unwrap_or(&s).to_string();
    s.replace('.', ",")
}

/// Kennzahl passend formatiert (Prozent, Dauer, Zahl, Geld).
pub fn format_metric(metric: &str, value: Option<f64>) -> String {
    let Some(v) = value else { return "–".into() };
    match metric {
        m::BOUNCE_RATE
        | m::CONVERSION_RATE
        | m::GROUP_CONVERSION_RATE
        | m::EXIT_RATE
        | m::PERCENTAGE
        | m::SCROLL_DEPTH => {
            if v >= 10.0 || (v - v.round()).abs() < f64::EPSILON {
                format!("{}%", v.round() as i64)
            } else {
                format!("{}%", format!("{v:.1}").replace('.', ","))
            }
        }
        m::VISIT_DURATION | m::TIME_ON_PAGE => duration(v),
        m::VIEWS_PER_VISIT => format!("{v:.2}").replace('.', ","),
        m::TOTAL_REVENUE | m::AVERAGE_REVENUE => format!("{v:.2}").replace('.', ","),
        _ => number_short(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(duration(5.0), "5s");
        assert_eq!(duration(185.0), "3m 05s");
        assert_eq!(duration(3720.0), "1h 02m");
        assert_eq!(number_short(987.0), "987");
        assert_eq!(number_short(1234.0), "1,2k");
        assert_eq!(number_short(12_345.0), "12k");
        assert_eq!(number_short(3_400_000.0), "3,4 Mio.");
        assert_eq!(number_full(1_234_567.0), "1’234’567");
        assert_eq!(format_metric(m::BOUNCE_RATE, Some(42.4)), "42%");
        assert_eq!(format_metric(m::CONVERSION_RATE, Some(2.5)), "2,5%");
        assert_eq!(format_metric(m::VIEWS_PER_VISIT, Some(1.5)), "1,50");
    }
}
