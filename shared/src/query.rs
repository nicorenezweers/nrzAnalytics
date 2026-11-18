//! Abfragemodell – kompatibel mit der Stats-API v2 von Plausible (`POST /api/v2/query`).
//!
//! Dieselbe Struktur benutzt die Oberflaeche fuer alle Auswertungen (`POST /api/stats/{domain}/query`),
//! der MCP-Server und der Rust-Client.
//!
//! ```json
//! {
//!   "site_id": "example.com",
//!   "metrics": ["visitors", "pageviews"],
//!   "date_range": "7d",
//!   "dimensions": ["visit:source"],
//!   "filters": [["is", "visit:country", ["DE", "CH"]]],
//!   "order_by": [["visitors", "desc"]],
//!   "include": {"comparisons": {"mode": "previous_period"}},
//!   "pagination": {"limit": 10, "offset": 0}
//! }
//! ```

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

/// Eine Abfrage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Query {
    /// Domain der Website. In der Oberflaeche leer (die Domain steht im Pfad).
    #[serde(default)]
    pub site_id: String,
    /// Kennzahlen, siehe [`metrics`].
    pub metrics: Vec<String>,
    /// Zeitraum, z. B. `"7d"` oder `["2026-01-01", "2026-01-31"]`.
    #[serde(default)]
    pub date_range: DateRange,
    /// Bezugsdatum `YYYY-MM-DD` fuer `day`, `month`, `year`, `7d` … (Standard: heute in der
    /// Zeitzone der Website). Erweiterung gegenueber Plausible v2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Dimensionen, siehe [`dimensions`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dimensions: Vec<String>,
    /// Filter (UND-verknuepft).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<Filter>,
    /// Sortierung, z. B. `[["visitors", "desc"]]`. Standard: erste Kennzahl absteigend, bei
    /// Zeit-Dimensionen die Zeit aufsteigend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_by: Option<Vec<(String, SortDirection)>>,
    #[serde(default, skip_serializing_if = "Include::is_default")]
    pub include: Include,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pagination: Option<Pagination>,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            site_id: String::new(),
            metrics: vec![metrics::VISITORS.into()],
            date_range: DateRange::default(),
            date: None,
            dimensions: Vec::new(),
            filters: Vec::new(),
            order_by: None,
            include: Include::default(),
            pagination: None,
        }
    }
}

impl Query {
    /// Neue Abfrage mit Kennzahlen und Zeitraum.
    pub fn new(metrics: &[&str], date_range: DateRange) -> Self {
        Self {
            metrics: metrics.iter().map(|m| (*m).to_string()).collect(),
            date_range,
            ..Self::default()
        }
    }

    pub fn site(mut self, domain: impl Into<String>) -> Self {
        self.site_id = domain.into();
        self
    }

    pub fn dimension(mut self, dimension: impl Into<String>) -> Self {
        self.dimensions.push(dimension.into());
        self
    }

    pub fn filter(mut self, filter: Filter) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn filters(mut self, filters: Vec<Filter>) -> Self {
        self.filters.extend(filters);
        self
    }

    pub fn date(mut self, date: Option<String>) -> Self {
        self.date = date;
        self
    }

    pub fn limit(mut self, limit: u32, offset: u32) -> Self {
        self.pagination = Some(Pagination { limit, offset });
        self
    }

    pub fn order(mut self, by: &str, direction: SortDirection) -> Self {
        self.order_by
            .get_or_insert_with(Vec::new)
            .push((by.to_string(), direction));
        self
    }

    pub fn compare(mut self, comparison: Option<Comparison>) -> Self {
        self.include.comparisons = comparison;
        self
    }

    pub fn time_labels(mut self) -> Self {
        self.include.time_labels = true;
        self
    }

    pub fn zero_fill(mut self) -> Self {
        self.include.zero_fill = true;
        self.include.time_labels = true;
        self
    }

    pub fn total_rows(mut self) -> Self {
        self.include.total_rows = true;
        self
    }
}

/// Zeitraum: benannt (`"day"`, `"7d"`, `"28d"`, `"30d"`, `"91d"`, `"month"`, `"6mo"`, `"12mo"`,
/// `"year"`, `"all"`, `"realtime"`) oder benutzerdefiniert `["YYYY-MM-DD", "YYYY-MM-DD"]`
/// (beide Tage eingeschlossen).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DateRange {
    Named(String),
    Custom(String, String),
}

impl Default for DateRange {
    fn default() -> Self {
        Self::Named("28d".into())
    }
}

impl DateRange {
    pub fn named(name: &str) -> Self {
        Self::Named(name.to_string())
    }

    pub fn custom(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self::Custom(from.into(), to.into())
    }

    pub fn is_realtime(&self) -> bool {
        matches!(self, Self::Named(n) if n == "realtime" || n == "realtime_30m")
    }

    /// Schluessel fuer URLs der Oberflaeche (`period=`).
    pub fn key(&self) -> String {
        match self {
            Self::Named(n) => n.clone(),
            Self::Custom(..) => "custom".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    Asc,
    #[default]
    Desc,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Include {
    /// `meta.time_labels` mit allen Zeitstempeln des Zeitraums liefern.
    #[serde(default)]
    pub time_labels: bool,
    /// `meta.total_rows` (Anzahl Zeilen ohne Seitenaufteilung) liefern.
    #[serde(default)]
    pub total_rows: bool,
    /// Vergleich mit einem anderen Zeitraum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparisons: Option<Comparison>,
    /// Zeitreihen ohne Luecken: jede Zeile aus `time_labels` erscheint, fehlende mit 0.
    /// Erweiterung gegenueber Plausible v2 (fuer Diagramme).
    #[serde(default)]
    pub zero_fill: bool,
    /// Wird ignoriert (Kompatibilitaet mit Plausible, es gibt keine importierten Daten).
    #[serde(default, skip_serializing)]
    pub imports: bool,
}

impl Include {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comparison {
    pub mode: ComparisonMode,
    /// Nur bei `mode = "custom"`: `["YYYY-MM-DD", "YYYY-MM-DD"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_range: Option<(String, String)>,
    /// Vergleichszeitraum so verschieben, dass die Wochentage uebereinstimmen.
    #[serde(default)]
    pub match_day_of_week: bool,
}

impl Comparison {
    pub fn previous_period() -> Self {
        Self {
            mode: ComparisonMode::PreviousPeriod,
            date_range: None,
            match_day_of_week: false,
        }
    }

    pub fn year_over_year() -> Self {
        Self {
            mode: ComparisonMode::YearOverYear,
            date_range: None,
            match_day_of_week: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonMode {
    PreviousPeriod,
    YearOverYear,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pagination {
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

// ---------------------------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------------------------

/// Filter. JSON wie bei Plausible v2:
///
/// - `["is", "visit:country", ["DE", "CH"]]`
/// - `["contains", "event:page", ["/blog"], {"case_sensitive": false}]`
/// - `["and", [filter, filter]]`, `["or", [filter, filter]]`, `["not", filter]`
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Filter {
    Condition {
        op: FilterOp,
        dimension: String,
        clauses: Vec<String>,
        case_sensitive: bool,
    },
    And(Vec<Filter>),
    Or(Vec<Filter>),
    Not(Box<Filter>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    /// Gleich einem der Werte.
    Is,
    IsNot,
    /// Enthaelt einen der Werte.
    Contains,
    ContainsNot,
    /// Regulaerer Ausdruck (Rust-`regex`).
    Matches,
    MatchesNot,
    /// Platzhalter wie bei Plausible v1: `*` (ohne `/`) und `**` (alles).
    MatchesWildcard,
    MatchesWildcardNot,
}

impl FilterOp {
    pub const ALL: [FilterOp; 8] = [
        Self::Is,
        Self::IsNot,
        Self::Contains,
        Self::ContainsNot,
        Self::Matches,
        Self::MatchesNot,
        Self::MatchesWildcard,
        Self::MatchesWildcardNot,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Is => "is",
            Self::IsNot => "is_not",
            Self::Contains => "contains",
            Self::ContainsNot => "contains_not",
            Self::Matches => "matches",
            Self::MatchesNot => "matches_not",
            Self::MatchesWildcard => "matches_wildcard",
            Self::MatchesWildcardNot => "matches_wildcard_not",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.as_str() == s)
    }

    /// Verneinender Operator (`is_not`, `contains_not`, …)?
    pub fn is_negated(self) -> bool {
        matches!(
            self,
            Self::IsNot | Self::ContainsNot | Self::MatchesNot | Self::MatchesWildcardNot
        )
    }
}

impl Filter {
    /// Einfache Bedingung, Gross-/Kleinschreibung wird beachtet.
    pub fn new(op: FilterOp, dimension: &str, clauses: &[&str]) -> Self {
        Self::Condition {
            op,
            dimension: dimension.to_string(),
            clauses: clauses.iter().map(|c| (*c).to_string()).collect(),
            case_sensitive: true,
        }
    }

    pub fn is(dimension: &str, value: &str) -> Self {
        Self::new(FilterOp::Is, dimension, &[value])
    }

    /// Alle Dimensionen, die in diesem Filter (auch verschachtelt) vorkommen.
    pub fn dimensions(&self) -> Vec<&str> {
        match self {
            Self::Condition { dimension, .. } => vec![dimension.as_str()],
            Self::And(v) | Self::Or(v) => v.iter().flat_map(Filter::dimensions).collect(),
            Self::Not(f) => f.dimensions(),
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Self::Condition {
                op,
                dimension,
                clauses,
                case_sensitive,
            } => {
                if *case_sensitive {
                    json!([op.as_str(), dimension, clauses])
                } else {
                    json!([op.as_str(), dimension, clauses, {"case_sensitive": false}])
                }
            }
            Self::And(v) => json!(["and", v.iter().map(Filter::to_value).collect::<Vec<_>>()]),
            Self::Or(v) => json!(["or", v.iter().map(Filter::to_value).collect::<Vec<_>>()]),
            Self::Not(f) => json!(["not", f.to_value()]),
        }
    }

    pub fn from_value(v: &Value) -> Result<Self, String> {
        let arr = v
            .as_array()
            .ok_or_else(|| format!("Filter muss eine Liste sein: {v}"))?;
        let op = arr
            .first()
            .and_then(Value::as_str)
            .ok_or("Filter ohne Operator")?;
        match op {
            "and" | "or" => {
                let list = arr
                    .get(1)
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("'{op}' braucht eine Liste von Filtern"))?;
                let filters = list.iter().map(Self::from_value).collect::<Result<Vec<_>, _>>()?;
                if filters.is_empty() {
                    return Err(format!("'{op}' mit leerer Liste"));
                }
                Ok(if op == "and" {
                    Self::And(filters)
                } else {
                    Self::Or(filters)
                })
            }
            "not" => {
                let inner = arr.get(1).ok_or("'not' braucht einen Filter")?;
                Ok(Self::Not(Box::new(Self::from_value(inner)?)))
            }
            _ => {
                let op = FilterOp::parse(op).ok_or_else(|| format!("Unbekannter Filter-Operator: {op}"))?;
                let dimension = arr
                    .get(1)
                    .and_then(Value::as_str)
                    .ok_or("Filter ohne Dimension")?
                    .to_string();
                let clauses = match arr.get(2) {
                    Some(Value::Array(items)) => items
                        .iter()
                        .map(|c| match c {
                            Value::String(s) => Ok(s.clone()),
                            Value::Number(n) => Ok(n.to_string()),
                            Value::Bool(b) => Ok(b.to_string()),
                            other => Err(format!("Ungültiger Filterwert: {other}")),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    Some(Value::String(s)) => vec![s.clone()],
                    _ => return Err(format!("Filter für {dimension} ohne Werte")),
                };
                if clauses.is_empty() {
                    return Err(format!("Filter für {dimension} ohne Werte"));
                }
                let case_sensitive = arr
                    .get(3)
                    .and_then(|m| m.get("case_sensitive"))
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                Ok(Self::Condition {
                    op,
                    dimension,
                    clauses,
                    case_sensitive,
                })
            }
        }
    }
}

impl Serialize for Filter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Filter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(deserializer)?;
        Self::from_value(&v).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------------------------
// Ergebnis
// ---------------------------------------------------------------------------------------------

/// Ergebnis einer Abfrage (wie Plausible v2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct QueryResult {
    pub results: Vec<ResultRow>,
    #[serde(default)]
    pub meta: QueryMeta,
    /// Die tatsaechlich ausgefuehrte Abfrage (aufgeloester Zeitraum, Kennzahlen …).
    #[serde(default)]
    pub query: Value,
}

/// Eine Zeile: Kennzahlen in der Reihenfolge von `query.metrics`, Dimensionswerte in der
/// Reihenfolge von `query.dimensions`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct ResultRow {
    pub metrics: Vec<Value>,
    #[serde(default)]
    pub dimensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<RowComparison>,
}

impl ResultRow {
    /// Kennzahl als Zahl (`null` = 0).
    pub fn metric(&self, index: usize) -> f64 {
        self.metrics.get(index).and_then(Value::as_f64).unwrap_or(0.0)
    }

    /// Kennzahl oder `None`, wenn sie `null` ist (z. B. Absprungrate ohne Besuche).
    pub fn metric_opt(&self, index: usize) -> Option<f64> {
        self.metrics.get(index).and_then(Value::as_f64)
    }

    pub fn dimension(&self, index: usize) -> &str {
        self.dimensions.get(index).map(String::as_str).unwrap_or("")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct RowComparison {
    #[serde(default)]
    pub dimensions: Vec<String>,
    pub metrics: Vec<Value>,
    /// Veraenderung je Kennzahl: Prozent, bei Raten (Absprungrate, Konversionsrate) Prozentpunkte.
    /// `null`, wenn der Vergleichswert 0 ist.
    pub change: Vec<Value>,
}

impl RowComparison {
    pub fn metric(&self, index: usize) -> f64 {
        self.metrics.get(index).and_then(Value::as_f64).unwrap_or(0.0)
    }

    pub fn change(&self, index: usize) -> Option<f64> {
        self.change.get(index).and_then(Value::as_f64)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct QueryMeta {
    /// Alle Zeitstempel des Zeitraums (bei einer Zeit-Dimension und `include.time_labels`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_labels: Option<Vec<String>>,
    /// Zeitstempel des Vergleichszeitraums, gleich lang wie `time_labels` (oder kuerzer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_time_labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_rows: Option<u64>,
    /// Aufgeloester Zeitraum als lokale Daten `[von, bis]` (bis eingeschlossen).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_range: Option<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_date_range: Option<(String, String)>,
    /// Gewaehltes Zeitintervall bei Zeit-Dimension `time` (`minute`, `hour`, `day`, `week`, `month`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// Namen
// ---------------------------------------------------------------------------------------------

/// Namen der Kennzahlen.
pub mod metrics {
    /// Eindeutige Besucher.
    pub const VISITORS: &str = "visitors";
    /// Besuche (Sitzungen).
    pub const VISITS: &str = "visits";
    pub const PAGEVIEWS: &str = "pageviews";
    pub const VIEWS_PER_VISIT: &str = "views_per_visit";
    /// Prozent der Besuche mit nur einer Seite und ohne Interaktion.
    pub const BOUNCE_RATE: &str = "bounce_rate";
    /// Durchschnittliche Besuchsdauer in Sekunden.
    pub const VISIT_DURATION: &str = "visit_duration";
    /// Alle Ereignisse (Seitenaufrufe und eigene Ereignisse).
    pub const EVENTS: &str = "events";
    /// Prozent der Besucher, die das gefilterte Ziel erreicht haben.
    pub const CONVERSION_RATE: &str = "conversion_rate";
    /// Konversionsrate je Gruppe (bei Aufschluesselung nach Dimension).
    pub const GROUP_CONVERSION_RATE: &str = "group_conversion_rate";
    /// Durchschnittliche Zeit auf der Seite in Sekunden (aus Engagement-Ereignissen).
    pub const TIME_ON_PAGE: &str = "time_on_page";
    /// Durchschnittliche Scrolltiefe in Prozent.
    pub const SCROLL_DEPTH: &str = "scroll_depth";
    /// Anteil der Besucher an allen Besuchern in Prozent (bei Aufschluesselungen).
    pub const PERCENTAGE: &str = "percentage";
    /// Ausstiegsrate in Prozent (nur mit `visit:exit_page`).
    pub const EXIT_RATE: &str = "exit_rate";
    pub const TOTAL_REVENUE: &str = "total_revenue";
    pub const AVERAGE_REVENUE: &str = "average_revenue";

    pub const ALL: [&str; 15] = [
        VISITORS,
        VISITS,
        PAGEVIEWS,
        VIEWS_PER_VISIT,
        BOUNCE_RATE,
        VISIT_DURATION,
        EVENTS,
        CONVERSION_RATE,
        GROUP_CONVERSION_RATE,
        TIME_ON_PAGE,
        SCROLL_DEPTH,
        PERCENTAGE,
        EXIT_RATE,
        TOTAL_REVENUE,
        AVERAGE_REVENUE,
    ];

    /// Raten in Prozent: Veraenderungen in Prozentpunkten.
    pub fn is_rate(metric: &str) -> bool {
        matches!(
            metric,
            BOUNCE_RATE | CONVERSION_RATE | GROUP_CONVERSION_RATE | EXIT_RATE | PERCENTAGE
        )
    }
}

/// Namen der Dimensionen.
pub mod dimensions {
    pub const EVENT_NAME: &str = "event:name";
    pub const EVENT_GOAL: &str = "event:goal";
    pub const EVENT_PAGE: &str = "event:page";
    pub const EVENT_HOSTNAME: &str = "event:hostname";
    /// Vorsilbe fuer eigene Eigenschaften: `event:props:<name>`.
    pub const EVENT_PROPS_PREFIX: &str = "event:props:";

    pub const VISIT_ENTRY_PAGE: &str = "visit:entry_page";
    pub const VISIT_EXIT_PAGE: &str = "visit:exit_page";
    pub const VISIT_ENTRY_PAGE_HOSTNAME: &str = "visit:entry_page_hostname";
    pub const VISIT_EXIT_PAGE_HOSTNAME: &str = "visit:exit_page_hostname";
    pub const VISIT_SOURCE: &str = "visit:source";
    pub const VISIT_REFERRER: &str = "visit:referrer";
    pub const VISIT_CHANNEL: &str = "visit:channel";
    pub const VISIT_UTM_MEDIUM: &str = "visit:utm_medium";
    pub const VISIT_UTM_SOURCE: &str = "visit:utm_source";
    pub const VISIT_UTM_CAMPAIGN: &str = "visit:utm_campaign";
    pub const VISIT_UTM_CONTENT: &str = "visit:utm_content";
    pub const VISIT_UTM_TERM: &str = "visit:utm_term";
    /// Geraeteklasse aus der Bildschirmbreite: `Desktop`, `Laptop`, `Tablet`, `Mobile`.
    pub const VISIT_DEVICE: &str = "visit:device";
    pub const VISIT_BROWSER: &str = "visit:browser";
    pub const VISIT_BROWSER_VERSION: &str = "visit:browser_version";
    pub const VISIT_OS: &str = "visit:os";
    pub const VISIT_OS_VERSION: &str = "visit:os_version";
    /// ISO-3166-1-Code, z. B. `CH`.
    pub const VISIT_COUNTRY: &str = "visit:country";
    /// ISO-3166-2-Code, z. B. `CH-ZH`.
    pub const VISIT_REGION: &str = "visit:region";
    /// Name der Stadt.
    pub const VISIT_CITY: &str = "visit:city";
    pub const VISIT_COUNTRY_NAME: &str = "visit:country_name";
    pub const VISIT_REGION_NAME: &str = "visit:region_name";
    pub const VISIT_CITY_NAME: &str = "visit:city_name";

    /// Zeit; das Intervall waehlt der Server passend zum Zeitraum (siehe `meta.interval`).
    pub const TIME: &str = "time";
    pub const TIME_MINUTE: &str = "time:minute";
    pub const TIME_HOUR: &str = "time:hour";
    pub const TIME_DAY: &str = "time:day";
    pub const TIME_WEEK: &str = "time:week";
    pub const TIME_MONTH: &str = "time:month";

    /// Alle festen Dimensionen (ohne `event:props:*`).
    pub const ALL: [&str; 33] = [
        EVENT_NAME,
        EVENT_GOAL,
        EVENT_PAGE,
        EVENT_HOSTNAME,
        VISIT_ENTRY_PAGE,
        VISIT_EXIT_PAGE,
        VISIT_ENTRY_PAGE_HOSTNAME,
        VISIT_EXIT_PAGE_HOSTNAME,
        VISIT_SOURCE,
        VISIT_REFERRER,
        VISIT_CHANNEL,
        VISIT_UTM_MEDIUM,
        VISIT_UTM_SOURCE,
        VISIT_UTM_CAMPAIGN,
        VISIT_UTM_CONTENT,
        VISIT_UTM_TERM,
        VISIT_DEVICE,
        VISIT_BROWSER,
        VISIT_BROWSER_VERSION,
        VISIT_OS,
        VISIT_OS_VERSION,
        VISIT_COUNTRY,
        VISIT_REGION,
        VISIT_CITY,
        VISIT_COUNTRY_NAME,
        VISIT_REGION_NAME,
        VISIT_CITY_NAME,
        TIME,
        TIME_MINUTE,
        TIME_HOUR,
        TIME_DAY,
        TIME_WEEK,
        TIME_MONTH,
    ];

    pub fn is_time(dimension: &str) -> bool {
        dimension == TIME || dimension.starts_with("time:")
    }

    /// Name der Eigenschaft bei `event:props:<name>`.
    pub fn prop_key(dimension: &str) -> Option<&str> {
        dimension
            .strip_prefix(EVENT_PROPS_PREFIX)
            .filter(|k| !k.is_empty())
    }

    pub fn is_known(dimension: &str) -> bool {
        ALL.contains(&dimension) || prop_key(dimension).is_some()
    }
}

/// Wert fuer fehlende Eigenschaften und leere Dimensionen (wie Plausible).
pub const NONE_VALUE: &str = "(none)";

/// Quelle ohne Verweis (direkter Aufruf).
pub const DIRECT_SOURCE: &str = "Direct / None";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_roundtrip() {
        let raw = r#"[["is","visit:country",["DE","CH"]],["contains","event:page",["blog"],{"case_sensitive":false}],["or",[["is","visit:source",["Google"]],["not",["is","visit:device",["Mobile"]]]]]]"#;
        let filters: Vec<Filter> = serde_json::from_str(raw).unwrap();
        assert_eq!(filters.len(), 3);
        assert!(matches!(
            &filters[1],
            Filter::Condition {
                case_sensitive: false,
                ..
            }
        ));
        let back = serde_json::to_string(&filters).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn date_range_forms() {
        let q: Query = serde_json::from_str(
            r#"{"site_id":"a.ch","metrics":["visitors"],"date_range":["2026-01-01","2026-01-31"]}"#,
        )
        .unwrap();
        assert_eq!(q.date_range, DateRange::custom("2026-01-01", "2026-01-31"));
        let q: Query = serde_json::from_str(r#"{"metrics":["visitors"],"date_range":"7d"}"#).unwrap();
        assert_eq!(q.date_range, DateRange::named("7d"));
        let q: Query = serde_json::from_str(r#"{"metrics":["visitors"]}"#).unwrap();
        assert_eq!(q.date_range, DateRange::named("28d"));
    }

    #[test]
    fn rejects_bad_filters() {
        assert!(serde_json::from_str::<Filter>(r#"["equals","visit:country",["DE"]]"#).is_err());
        assert!(serde_json::from_str::<Filter>(r#"["is","visit:country",[]]"#).is_err());
        assert!(serde_json::from_str::<Filter>(r#"["and",[]]"#).is_err());
    }
}
