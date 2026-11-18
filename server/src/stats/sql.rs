//! Bausteine fuer die SQL-Abfragen der Auswertung: Parameter, Dimensionen, Filter.
//!
//! Ereignisse (`events e`) tragen die Besuchs-Attribute (Quelle, Land, Geraet …) ihrer Sitzung mit,
//! deshalb lassen sich Besuchs-Filter direkt auf Ereignisse anwenden. Nur Einstiegs-/Ausstiegsseite
//! gibt es ausschliesslich in `sessions s`; Ereignis-Filter (Seite, Ziel, Eigenschaft) auf Sitzungen
//! werden zu `s.id IN (SELECT session_id FROM events …)`.

use super::time::Interval;
use crate::error::{AppError, AppResult};
use nrzanalytics_shared::{
    api::{Goal, GoalKind},
    query::{DIRECT_SOURCE, Filter, FilterOp, NONE_VALUE, dimensions as d},
};
use rusqlite::types::Value;

/// Gebundene Parameter (`?1`, `?2`, …) in der Reihenfolge ihres Auftretens.
#[derive(Default, Debug)]
pub struct Params {
    pub values: Vec<Value>,
}

impl Params {
    pub fn add(&mut self, v: impl Into<Value>) -> String {
        self.values.push(v.into());
        format!("?{}", self.values.len())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    Events,
    Sessions,
}

/// Spalte, die Ereignisse und Sitzungen gleichermassen haben.
pub fn visit_column(dimension: &str) -> Option<&'static str> {
    Some(match dimension {
        d::VISIT_SOURCE => "source",
        d::VISIT_REFERRER => "referrer",
        d::VISIT_CHANNEL => "channel",
        d::VISIT_UTM_MEDIUM => "utm_medium",
        d::VISIT_UTM_SOURCE => "utm_source",
        d::VISIT_UTM_CAMPAIGN => "utm_campaign",
        d::VISIT_UTM_CONTENT => "utm_content",
        d::VISIT_UTM_TERM => "utm_term",
        d::VISIT_DEVICE => "device",
        d::VISIT_BROWSER => "browser",
        d::VISIT_BROWSER_VERSION => "browser_version",
        d::VISIT_OS => "os",
        d::VISIT_OS_VERSION => "os_version",
        d::VISIT_COUNTRY => "country",
        d::VISIT_REGION => "region",
        d::VISIT_CITY => "city",
        _ => return None,
    })
}

/// Pfad fuer `json_extract` auf eine Eigenschaft.
pub fn prop_path(key: &str) -> AppResult<String> {
    if key.is_empty() || key.contains('"') || key.contains('\\') || key.len() > 300 {
        return Err(AppError::invalid(format!(
            "Ungültiger Name einer Eigenschaft: {key}"
        )));
    }
    Ok(format!("$.\"{key}\""))
}

/// Eine Dimension der Abfrage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dim {
    Time(Interval),
    Goal,
    Name,
    Page,
    Hostname,
    Prop(String),
    EntryPage,
    ExitPage,
    EntryHost,
    ExitHost,
    Visit(&'static str),
    CountryName,
    RegionName,
    CityName,
}

impl Dim {
    pub fn parse(s: &str, default_interval: Interval) -> AppResult<Self> {
        if let Some(key) = d::prop_key(s) {
            prop_path(key)?;
            return Ok(Self::Prop(key.to_string()));
        }
        if s == d::TIME {
            return Ok(Self::Time(default_interval));
        }
        if let Some(i) = Interval::from_dimension(s) {
            return Ok(Self::Time(i));
        }
        if let Some(col) = visit_column(s) {
            return Ok(Self::Visit(col));
        }
        Ok(match s {
            d::EVENT_GOAL => Self::Goal,
            d::EVENT_NAME => Self::Name,
            d::EVENT_PAGE => Self::Page,
            d::EVENT_HOSTNAME => Self::Hostname,
            d::VISIT_ENTRY_PAGE => Self::EntryPage,
            d::VISIT_EXIT_PAGE => Self::ExitPage,
            d::VISIT_ENTRY_PAGE_HOSTNAME => Self::EntryHost,
            d::VISIT_EXIT_PAGE_HOSTNAME => Self::ExitHost,
            d::VISIT_COUNTRY_NAME => Self::CountryName,
            d::VISIT_REGION_NAME => Self::RegionName,
            d::VISIT_CITY_NAME => Self::CityName,
            other => return Err(AppError::invalid(format!("Unbekannte Dimension: {other}"))),
        })
    }

    /// Gibt es nur auf Ereignis-Ebene (Seite, Hostname, Name, Eigenschaft, Ziel).
    pub fn is_event_level(&self) -> bool {
        matches!(
            self,
            Self::Goal | Self::Name | Self::Page | Self::Hostname | Self::Prop(_)
        )
    }

    /// Gibt es nur in Sitzungen (Einstieg/Ausstieg).
    pub fn is_session_only(&self) -> bool {
        matches!(
            self,
            Self::EntryPage | Self::ExitPage | Self::EntryHost | Self::ExitHost
        )
    }

    pub fn is_time(&self) -> bool {
        matches!(self, Self::Time(_))
    }

    /// Spalte, deren leere Werte in Aufschluesselungen weggelassen werden (wie Plausible).
    pub fn skip_empty_column(&self) -> Option<&'static str> {
        match self {
            Self::Visit(c)
                if c.starts_with("utm_") || matches!(*c, "referrer" | "country" | "region" | "city") =>
            {
                Some(c)
            }
            Self::CountryName => Some("country"),
            Self::RegionName => Some("region"),
            Self::CityName => Some("city"),
            _ => None,
        }
    }

    /// SQL-Ausdruck fuer die Gruppierung. `bounds` = Parameter-Referenz der Zeitgrenzen.
    pub fn expr(&self, t: Table, p: &mut Params, bounds: Option<&str>) -> AppResult<String> {
        let a = match t {
            Table::Events => "e",
            Table::Sessions => "s",
        };
        Ok(match (self, t) {
            (Self::Time(_), Table::Events) => {
                format!(
                    "nrz_bucket(e.ts, {})",
                    bounds.ok_or_else(|| AppError::internal("Zeitgrenzen fehlen"))?
                )
            }
            (Self::Time(_), Table::Sessions) => {
                format!(
                    "nrz_bucket(s.start, {})",
                    bounds.ok_or_else(|| AppError::internal("Zeitgrenzen fehlen"))?
                )
            }
            (Self::Visit(c), _) => format!("{a}.{c}"),
            (Self::CountryName, _) => format!("{a}.country"),
            (Self::RegionName, _) => format!("{a}.region"),
            (Self::CityName, _) => format!("{a}.city"),
            (Self::Name, Table::Events) => "e.name".into(),
            (Self::Page, Table::Events) => "e.pathname".into(),
            (Self::Hostname, Table::Events) => "e.hostname".into(),
            (Self::Prop(k), Table::Events) => {
                format!(
                    "COALESCE(json_extract(e.props, {}), '{NONE_VALUE}')",
                    p.add(prop_path(k)?)
                )
            }
            (Self::EntryPage, Table::Sessions) => "s.entry_page".into(),
            (Self::ExitPage, Table::Sessions) => "s.exit_page".into(),
            (Self::EntryHost, Table::Sessions) => "s.hostname".into(),
            (Self::ExitHost, Table::Sessions) => "s.exit_hostname".into(),
            (dim, table) => {
                return Err(AppError::internal(format!(
                    "Dimension {dim:?} nicht in {table:?}"
                )));
            }
        })
    }
}

/// Platzhalter-Pfad (`/blog/*`, `/docs/**`) als regulaerer Ausdruck.
pub fn wildcard_regex(pattern: &str) -> String {
    let mut out = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '*' {
            if chars.peek() == Some(&'*') {
                chars.next();
                out.push_str(".*");
            } else {
                out.push_str("[^/]*");
            }
        } else {
            out.push_str(&regex::escape(&c.to_string()));
        }
    }
    out.push('$');
    out
}

/// Bedingung eines Ziels auf Ereignissen.
pub fn goal_predicate(goal: &Goal, p: &mut Params) -> String {
    match goal.kind {
        GoalKind::Event => format!("e.name = {}", p.add(goal.event_name.clone().unwrap_or_default())),
        GoalKind::Page => {
            let path = goal.page_path.clone().unwrap_or_default();
            if path.contains('*') {
                format!(
                    "(e.name = 'pageview' AND e.pathname REGEXP {})",
                    p.add(wildcard_regex(&path))
                )
            } else {
                format!("(e.name = 'pageview' AND e.pathname = {})", p.add(path))
            }
        }
    }
}

/// Umgebung fuer Filter: Website, Zeitraum (fuer Unterabfragen) und Ziele.
pub struct FilterEnv<'a> {
    pub goals: &'a [Goal],
    pub site: String,
    pub start: String,
    pub end: String,
}

fn check_regex(pattern: &str) -> AppResult<()> {
    regex::RegexBuilder::new(pattern)
        .size_limit(1 << 20)
        .build()
        .map(|_| ())
        .map_err(|e| AppError::invalid(format!("Ungültiger regulärer Ausdruck '{pattern}': {e}")))
}

/// Bedingung `op` mit Werten auf einen Spalten-Ausdruck.
fn predicate(
    x: &str,
    op: FilterOp,
    clauses: &[String],
    case_sensitive: bool,
    dimension: &str,
    p: &mut Params,
) -> AppResult<String> {
    let lower = |e: &str| {
        if case_sensitive {
            e.to_string()
        } else {
            format!("lower({e})")
        }
    };
    let positive = match op {
        FilterOp::Is | FilterOp::IsNot => {
            let mut parts = Vec::new();
            let mut values = Vec::new();
            for c in clauses {
                // "(none)" und bei der Quelle "Direct / None" stehen fuer einen leeren Wert.
                let empty = c == NONE_VALUE
                    || (dimension == d::VISIT_SOURCE && (c == DIRECT_SOURCE || c == "Direct"));
                if empty {
                    parts.push(format!("{x} = ''"));
                } else {
                    values.push(lower(&p.add(c.clone())));
                }
            }
            if !values.is_empty() {
                parts.push(format!("{} IN ({})", lower(x), values.join(", ")));
            }
            format!("({})", parts.join(" OR "))
        }
        FilterOp::Contains | FilterOp::ContainsNot => {
            let parts: Vec<String> = clauses
                .iter()
                .map(|c| format!("instr({}, {}) > 0", lower(x), lower(&p.add(c.clone()))))
                .collect();
            format!("({})", parts.join(" OR "))
        }
        FilterOp::Matches | FilterOp::MatchesNot => {
            let mut parts = Vec::new();
            for c in clauses {
                let pattern = if case_sensitive {
                    c.clone()
                } else {
                    format!("(?i){c}")
                };
                check_regex(&pattern)?;
                parts.push(format!("{x} REGEXP {}", p.add(pattern)));
            }
            format!("({})", parts.join(" OR "))
        }
        FilterOp::MatchesWildcard | FilterOp::MatchesWildcardNot => {
            let mut parts = Vec::new();
            for c in clauses {
                let re = wildcard_regex(c);
                let pattern = if case_sensitive { re } else { format!("(?i){re}") };
                parts.push(format!("{x} REGEXP {}", p.add(pattern)));
            }
            format!("({})", parts.join(" OR "))
        }
    };
    Ok(if op.is_negated() {
        format!("NOT {positive}")
    } else {
        positive
    })
}

/// Passende Ziele fuer einen Filter auf `event:goal`.
pub fn matching_goals<'a>(
    goals: &'a [Goal],
    op: FilterOp,
    clauses: &[String],
    case_sensitive: bool,
) -> AppResult<Vec<&'a Goal>> {
    let norm = |s: &str| {
        if case_sensitive {
            s.to_string()
        } else {
            s.to_lowercase()
        }
    };
    let mut out = Vec::new();
    for g in goals {
        let name = norm(&g.display_name);
        let hit = match op {
            FilterOp::Is | FilterOp::IsNot => clauses.iter().any(|c| norm(c) == name),
            FilterOp::Contains | FilterOp::ContainsNot => clauses.iter().any(|c| name.contains(&norm(c))),
            FilterOp::Matches | FilterOp::MatchesNot => {
                let mut any = false;
                for c in clauses {
                    let pattern = if case_sensitive {
                        c.clone()
                    } else {
                        format!("(?i){c}")
                    };
                    check_regex(&pattern)?;
                    if regex::Regex::new(&pattern)
                        .map(|r| r.is_match(&g.display_name))
                        .unwrap_or(false)
                    {
                        any = true;
                    }
                }
                any
            }
            FilterOp::MatchesWildcard | FilterOp::MatchesWildcardNot => clauses.iter().any(|c| {
                regex::Regex::new(&wildcard_regex(c))
                    .map(|r| r.is_match(&g.display_name))
                    .unwrap_or(false)
            }),
        };
        if hit {
            out.push(g);
        }
    }
    Ok(out)
}

/// Filter als SQL-Bedingung fuer die Tabelle `t`.
pub fn filter_expr(f: &Filter, t: Table, env: &FilterEnv<'_>, p: &mut Params) -> AppResult<String> {
    match f {
        Filter::And(list) => {
            let parts = list
                .iter()
                .map(|x| filter_expr(x, t, env, p))
                .collect::<AppResult<Vec<_>>>()?;
            Ok(format!("({})", parts.join(" AND ")))
        }
        Filter::Or(list) => {
            let parts = list
                .iter()
                .map(|x| filter_expr(x, t, env, p))
                .collect::<AppResult<Vec<_>>>()?;
            Ok(format!("({})", parts.join(" OR ")))
        }
        Filter::Not(inner) => Ok(format!("NOT {}", filter_expr(inner, t, env, p)?)),
        Filter::Condition {
            op,
            dimension,
            clauses,
            case_sensitive,
        } => condition(*op, dimension, clauses, *case_sensitive, t, env, p),
    }
}

fn condition(
    op: FilterOp,
    dimension: &str,
    clauses: &[String],
    case_sensitive: bool,
    t: Table,
    env: &FilterEnv<'_>,
    p: &mut Params,
) -> AppResult<String> {
    // Besuchs-Spalten gibt es in beiden Tabellen.
    if let Some(col) = visit_column(dimension) {
        let a = if t == Table::Events { "e" } else { "s" };
        let x = format!("{a}.{col}");
        // Laender auch per Name filterbar.
        let clauses: Vec<String> = if col == "country" {
            clauses
                .iter()
                .map(|c| {
                    nrzanalytics_shared::geo::country_code(c)
                        .map(str::to_string)
                        .unwrap_or_else(|| c.clone())
                })
                .collect()
        } else {
            clauses.to_vec()
        };
        return predicate(&x, op, &clauses, case_sensitive, dimension, p);
    }

    // Nur in Sitzungen: Einstieg/Ausstieg.
    let session_col = match dimension {
        d::VISIT_ENTRY_PAGE => Some("s.entry_page"),
        d::VISIT_EXIT_PAGE => Some("s.exit_page"),
        d::VISIT_ENTRY_PAGE_HOSTNAME => Some("s.hostname"),
        d::VISIT_EXIT_PAGE_HOSTNAME => Some("s.exit_hostname"),
        _ => None,
    };
    if let Some(col) = session_col {
        let pred = predicate(col, op, clauses, case_sensitive, dimension, p)?;
        return Ok(match t {
            Table::Sessions => pred,
            Table::Events => format!(
                "e.session_id IN (SELECT s.id FROM sessions s WHERE s.site_id = {} AND s.end >= {} AND s.start < {} AND {pred})",
                env.site, env.start, env.end
            ),
        });
    }

    // Ereignis-Ebene: Seite, Hostname, Name, Eigenschaft, Ziel.
    let positive_op = match op {
        FilterOp::IsNot => FilterOp::Is,
        FilterOp::ContainsNot => FilterOp::Contains,
        FilterOp::MatchesNot => FilterOp::Matches,
        FilterOp::MatchesWildcardNot => FilterOp::MatchesWildcard,
        other => other,
    };
    let event_pred = |op: FilterOp, p: &mut Params| -> AppResult<String> {
        if dimension == d::EVENT_GOAL {
            let goals = matching_goals(env.goals, op, clauses, case_sensitive)?;
            if goals.is_empty() {
                return Ok("0".into());
            }
            let parts: Vec<String> = goals.iter().map(|g| goal_predicate(g, p)).collect();
            return Ok(format!("({})", parts.join(" OR ")));
        }
        let x = match dimension {
            d::EVENT_PAGE => "e.pathname".to_string(),
            d::EVENT_HOSTNAME => "e.hostname".to_string(),
            d::EVENT_NAME => "e.name".to_string(),
            other => match d::prop_key(other) {
                Some(key) => format!("COALESCE(json_extract(e.props, {}), '')", p.add(prop_path(key)?)),
                None => {
                    return Err(AppError::invalid(format!(
                        "Nach '{other}' kann nicht gefiltert werden."
                    )));
                }
            },
        };
        predicate(&x, op, clauses, case_sensitive, dimension, p)
    };
    match t {
        Table::Events => {
            if dimension == d::EVENT_GOAL && op.is_negated() {
                Ok(format!("NOT {}", event_pred(positive_op, p)?))
            } else {
                event_pred(op, p)
            }
        }
        Table::Sessions => {
            // Sitzungen mit (bzw. ohne) passendes Ereignis.
            let pred = event_pred(positive_op, p)?;
            let sub = format!(
                "SELECT e.session_id FROM events e WHERE e.site_id = {} AND e.ts >= {} AND e.ts < {} AND {pred}",
                env.site, env.start, env.end
            );
            Ok(if op.is_negated() {
                format!("s.id NOT IN ({sub})")
            } else {
                format!("s.id IN ({sub})")
            })
        }
    }
}

/// Filter ohne die obersten Ziel-Filter (fuer die Basis der Konversionsrate).
pub fn without_goal_filters(filters: &[Filter]) -> Vec<Filter> {
    filters
        .iter()
        .filter(|f| !matches!(f, Filter::Condition { dimension, .. } if dimension == d::EVENT_GOAL))
        .cloned()
        .collect()
}

pub fn has_goal_filter(filters: &[Filter]) -> bool {
    filters.iter().any(|f| f.dimensions().contains(&d::EVENT_GOAL))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        let re = regex::Regex::new(&wildcard_regex("/blog/*")).unwrap();
        assert!(re.is_match("/blog/hallo"));
        assert!(!re.is_match("/blog/a/b"));
        let re = regex::Regex::new(&wildcard_regex("/docs/**")).unwrap();
        assert!(re.is_match("/docs/a/b/c"));
        let re = regex::Regex::new(&wildcard_regex("/a.b")).unwrap();
        assert!(!re.is_match("/aXb"));
    }
}
