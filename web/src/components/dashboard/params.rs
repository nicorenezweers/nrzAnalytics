//! Zustand des Dashboards in der URL (teilbar, Zurueck-Taste funktioniert).
//!
//! `?period=7d&date=2026-11-17&from=…&to=…&compare=previous_period&f=<Filter-JSON>&interval=day&metric=visitors`
//! Fuer geteilte Links zusaetzlich `auth`, `embed`, `theme`.

use nrzanalytics_shared::query::{Comparison, DateRange, Filter, FilterOp, Query};
use std::fmt;

/// Standard-Zeitraum.
pub const DEFAULT_PERIOD: &str = "28d";

#[derive(Clone, Debug, PartialEq, Default)]
pub struct DashParams {
    /// Schluessel des Zeitraums (`day`, `7d`, …, `custom`); leer = Standard.
    pub period: String,
    /// Bezugsdatum `YYYY-MM-DD` (fuer `day`, `month`, `year`).
    pub date: String,
    pub from: String,
    pub to: String,
    /// `previous_period` oder `year_over_year`; leer = kein Vergleich.
    pub compare: String,
    pub filters: Vec<Filter>,
    /// Intervall des Diagramms; leer = vom Server gewaehlt.
    pub interval: String,
    /// Kennzahl im Diagramm; leer = erste Kennzahl.
    pub metric: String,
    /// Gewaehlte Eigenschaft in der Karte "Eigenschaften".
    pub prop: String,
    /// Gewaehlter Trichter (ID).
    pub funnel: String,
    /// Slug eines geteilten Links.
    pub auth: String,
    /// Eingebettet (ohne Kopfzeile).
    pub embed: bool,
    /// `light` oder `dark` (nur bei geteilten Links).
    pub theme: String,
}

impl DashParams {
    pub fn period_key(&self) -> &str {
        if self.period.is_empty() { DEFAULT_PERIOD } else { &self.period }
    }

    pub fn is_realtime(&self) -> bool {
        self.period_key() == "realtime"
    }

    pub fn date_range(&self) -> DateRange {
        match self.period_key() {
            "custom" => {
                if crate::util::parse_date(&self.from).is_some() && crate::util::parse_date(&self.to).is_some() {
                    DateRange::custom(self.from.clone(), self.to.clone())
                } else {
                    DateRange::named(DEFAULT_PERIOD)
                }
            }
            "realtime" => DateRange::named("realtime"),
            key => DateRange::named(key),
        }
    }

    pub fn date_opt(&self) -> Option<String> {
        (crate::util::parse_date(&self.date).is_some() && self.period_key() != "custom" && !self.is_realtime())
            .then(|| self.date.clone())
    }

    pub fn comparison(&self) -> Option<Comparison> {
        if self.is_realtime() || self.period_key() == "all" {
            return None;
        }
        match self.compare.as_str() {
            "previous_period" => Some(Comparison::previous_period()),
            "year_over_year" => Some(Comparison::year_over_year()),
            _ => None,
        }
    }

    /// Basis-Abfrage mit Zeitraum, Bezugsdatum und Filtern.
    pub fn query(&self, metrics: &[&str]) -> Query {
        Query::new(metrics, self.date_range()).date(self.date_opt()).filters(self.filters.clone())
    }

    /// Ziel-Filter aktiv?
    pub fn has_goal_filter(&self) -> bool {
        self.filters.iter().any(|f| f.dimensions().contains(&"event:goal"))
    }

    /// Filter fuer diese Dimension (erste einfache Bedingung).
    pub fn filter_for(&self, dimension: &str) -> Option<&Filter> {
        self.filters.iter().find(|f| matches!(f, Filter::Condition { dimension: d, .. } if d == dimension))
    }

    pub fn has_filter(&self, dimension: &str) -> bool {
        self.filters.iter().any(|f| f.dimensions().contains(&dimension))
    }

    /// Kopie mit gesetztem Filter `is` (ersetzt bestehende Filter derselben Dimension).
    pub fn with_filter_is(&self, dimension: &str, value: &str) -> Self {
        self.with_filter(Filter::is(dimension, value))
    }

    /// Kopie mit Filter (ersetzt bestehende einfache Filter derselben Dimension).
    pub fn with_filter(&self, filter: Filter) -> Self {
        let mut p = self.clone();
        if let Filter::Condition { dimension, .. } = &filter {
            let d = dimension.clone();
            p.filters.retain(|f| !matches!(f, Filter::Condition { dimension: x, .. } if *x == d));
        }
        p.filters.push(filter);
        p
    }

    pub fn without_filter(&self, index: usize) -> Self {
        let mut p = self.clone();
        if index < p.filters.len() {
            p.filters.remove(index);
        }
        p
    }

    pub fn without_filters(&self) -> Self {
        Self { filters: Vec::new(), ..self.clone() }
    }

    /// Kopie mit anderem Zeitraum (Bezugsdatum, Intervall zurueckgesetzt).
    pub fn with_period(&self, period: &str, date: Option<String>) -> Self {
        Self {
            period: if period == DEFAULT_PERIOD { String::new() } else { period.to_string() },
            date: date.unwrap_or_default(),
            from: String::new(),
            to: String::new(),
            interval: String::new(),
            ..self.clone()
        }
    }

    pub fn with_custom(&self, from: &str, to: &str) -> Self {
        Self {
            period: "custom".into(),
            date: String::new(),
            from: from.to_string(),
            to: to.to_string(),
            interval: String::new(),
            ..self.clone()
        }
    }
}

fn decode(s: &str) -> String {
    let s = s.replace('+', " ");
    urlencoding::decode(&s).map(|c| c.into_owned()).unwrap_or(s)
}

impl From<&str> for DashParams {
    fn from(query: &str) -> Self {
        let mut p = Self::default();
        for pair in query.trim_start_matches('?').split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let v = decode(v);
            match k {
                "period" => p.period = v,
                "date" => p.date = v,
                "from" => p.from = v,
                "to" => p.to = v,
                "compare" => p.compare = v,
                "f" => p.filters = serde_json::from_str(&v).unwrap_or_default(),
                "interval" => p.interval = v,
                "metric" => p.metric = v,
                "prop" => p.prop = v,
                "funnel" => p.funnel = v,
                "auth" => p.auth = v,
                "embed" => p.embed = v == "true" || v == "1",
                "theme" => p.theme = v,
                _ => {}
            }
        }
        p
    }
}

impl fmt::Display for DashParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let filters = if self.filters.is_empty() {
            String::new()
        } else {
            serde_json::to_string(&self.filters).unwrap_or_default()
        };
        let pairs: [(&str, &str); 13] = [
            ("auth", &self.auth),
            ("embed", if self.embed { "true" } else { "" }),
            ("theme", &self.theme),
            ("period", &self.period),
            ("date", &self.date),
            ("from", &self.from),
            ("to", &self.to),
            ("compare", &self.compare),
            ("f", &filters),
            ("interval", &self.interval),
            ("metric", &self.metric),
            ("prop", &self.prop),
            ("funnel", &self.funnel),
        ];
        let mut first = true;
        for (k, v) in pairs {
            if v.is_empty() {
                continue;
            }
            f.write_str(if first { "" } else { "&" })?;
            first = false;
            write!(f, "{k}={}", urlencoding::encode(v))?;
        }
        Ok(())
    }
}

/// Deutscher Operator fuer Filter-Chips.
pub fn op_label(op: FilterOp) -> &'static str {
    match op {
        FilterOp::Is => "ist",
        FilterOp::IsNot => "ist nicht",
        FilterOp::Contains => "enthält",
        FilterOp::ContainsNot => "enthält nicht",
        FilterOp::Matches => "passt auf",
        FilterOp::MatchesNot => "passt nicht auf",
        FilterOp::MatchesWildcard => "entspricht",
        FilterOp::MatchesWildcardNot => "entspricht nicht",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let p = DashParams {
            period: "7d".into(),
            filters: vec![Filter::is("visit:country", "CH")],
            compare: "previous_period".into(),
            ..Default::default()
        };
        let s = p.to_string();
        assert_eq!(DashParams::from(s.as_str()), p);
    }
}
