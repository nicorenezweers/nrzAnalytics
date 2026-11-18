//! Zeitraeume und Zeitscheiben in der Zeitzone der Website.
//!
//! Gespeichert wird in UTC (Unix-Sekunden). Ein lokaler Tag wird ueber chrono-tz in UTC-Grenzen
//! umgerechnet (Sommerzeit, halbe Stunden-Versaetze). Zeitscheiben sind Listen von Startzeitpunkten;
//! SQLite ordnet Ereignisse per `nrz_bucket(ts, grenzen)` zu.

use crate::error::{AppError, AppResult};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use nrzanalytics_shared::query::{Comparison, ComparisonMode, DateRange};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interval {
    Minute,
    Hour,
    Day,
    Week,
    Month,
}

impl Interval {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }

    pub fn from_dimension(dimension: &str) -> Option<Self> {
        match dimension {
            "time:minute" => Some(Self::Minute),
            "time:hour" => Some(Self::Hour),
            "time:day" => Some(Self::Day),
            "time:week" => Some(Self::Week),
            "time:month" => Some(Self::Month),
            _ => None,
        }
    }
}

/// Aufgeloester Zeitraum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    /// Erster lokaler Tag (eingeschlossen).
    pub from: NaiveDate,
    /// Letzter lokaler Tag (eingeschlossen).
    pub to: NaiveDate,
    /// UTC-Beginn (eingeschlossen).
    pub start: i64,
    /// UTC-Ende (ausgeschlossen).
    pub end: i64,
    pub realtime: bool,
}

impl Range {
    pub fn days(&self) -> i64 {
        (self.to - self.from).num_days() + 1
    }

    pub fn labels(&self) -> (String, String) {
        (
            self.from.format("%Y-%m-%d").to_string(),
            self.to.format("%Y-%m-%d").to_string(),
        )
    }
}

/// Zeitzone aus dem Namen (unbekannt → UTC).
pub fn tz(name: &str) -> Tz {
    name.parse::<Tz>().unwrap_or(Tz::UTC)
}

/// Heute in der Zeitzone.
pub fn today(tz: Tz, now: i64) -> NaiveDate {
    DateTime::<Utc>::from_timestamp(now, 0)
        .unwrap_or_default()
        .with_timezone(&tz)
        .date_naive()
}

/// Lokales Datum eines Zeitstempels.
pub fn local_date(tz: Tz, ts: i64) -> NaiveDate {
    today(tz, ts)
}

/// UTC-Zeitstempel von lokal 00:00 (bei Zeitumstellung um Mitternacht die erste gueltige Zeit).
pub fn midnight(tz: Tz, date: NaiveDate) -> i64 {
    let naive = date.and_time(NaiveTime::MIN);
    for offset_hours in 0..4 {
        let candidate = naive + Duration::hours(offset_hours);
        if let Some(dt) = tz.from_local_datetime(&candidate).earliest() {
            return dt.timestamp();
        }
    }
    naive.and_utc().timestamp()
}

pub fn parse_date(s: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::invalid(format!("Ungültiges Datum '{s}' (erwartet JJJJ-MM-TT)")))
}

fn first_of_month(d: NaiveDate) -> NaiveDate {
    d.with_day(1).unwrap_or(d)
}

fn add_months(d: NaiveDate, months: i32) -> NaiveDate {
    let total = d.year() * 12 + d.month0() as i32 + months;
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
    let mut day = d.day();
    loop {
        if let Some(r) = NaiveDate::from_ymd_opt(y, m, day) {
            return r;
        }
        day -= 1;
        if day == 0 {
            return d;
        }
    }
}

fn last_of_month(d: NaiveDate) -> NaiveDate {
    add_months(first_of_month(d), 1).pred_opt().unwrap_or(d)
}

fn minus_year(d: NaiveDate) -> NaiveDate {
    d.with_year(d.year() - 1)
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(d.year() - 1, 2, 28).unwrap_or(d))
}

fn range_of(tz: Tz, from: NaiveDate, to: NaiveDate) -> Range {
    let to_next = to.succ_opt().unwrap_or(to);
    Range {
        from,
        to,
        start: midnight(tz, from),
        end: midnight(tz, to_next),
        realtime: false,
    }
}

/// Echtzeit: die letzten 30 Minuten (bis jetzt).
fn realtime(tz: Tz, now: i64) -> Range {
    let end_minute = now - now.rem_euclid(60) + 60;
    let start = end_minute - 30 * 60;
    let d = local_date(tz, now);
    Range {
        from: local_date(tz, start),
        to: d,
        start,
        end: end_minute,
        realtime: true,
    }
}

/// Zeitraum aufloesen. `first_day` = erster Tag mit Daten (fuer `all`).
pub fn resolve(
    date_range: &DateRange,
    date: Option<&str>,
    tz: Tz,
    now: i64,
    first_day: Option<NaiveDate>,
) -> AppResult<Range> {
    let today = today(tz, now);
    let reference = match date {
        Some(d) if !d.trim().is_empty() => parse_date(d)?,
        _ => today,
    };
    let r = match date_range {
        DateRange::Custom(a, b) => {
            let (from, to) = (parse_date(a)?, parse_date(b)?);
            if from > to {
                return Err(AppError::invalid("Der Zeitraum endet vor seinem Beginn."));
            }
            if (to - from).num_days() > 366 * 30 {
                return Err(AppError::invalid("Zeitraum zu lang."));
            }
            range_of(tz, from, to)
        }
        DateRange::Named(name) => match name.as_str() {
            "realtime" | "realtime_30m" | "30m" => realtime(tz, now),
            "day" | "today" => range_of(tz, reference, reference),
            "month" => range_of(tz, first_of_month(reference), last_of_month(reference)),
            "6mo" => range_of(
                tz,
                first_of_month(add_months(reference, -5)),
                last_of_month(reference),
            ),
            "12mo" => range_of(
                tz,
                first_of_month(add_months(reference, -11)),
                last_of_month(reference),
            ),
            "year" => {
                let from = NaiveDate::from_ymd_opt(reference.year(), 1, 1).unwrap_or(reference);
                let to = NaiveDate::from_ymd_opt(reference.year(), 12, 31).unwrap_or(reference);
                range_of(tz, from, to)
            }
            "all" => {
                let from = first_day.filter(|d| *d <= today).unwrap_or(today);
                range_of(tz, from, today)
            }
            other => {
                let days: i64 = other
                    .strip_suffix('d')
                    .and_then(|n| n.parse().ok())
                    .filter(|n| (1..=3660).contains(n))
                    .ok_or_else(|| AppError::invalid(format!("Unbekannter Zeitraum '{other}'")))?;
                range_of(tz, reference - Duration::days(days - 1), reference)
            }
        },
    };
    Ok(r)
}

/// Vergleichszeitraum. Laeuft der Zeitraum noch (endet in der Zukunft) und ist `to_date` gesetzt,
/// wird nur der bisher vergangene Teil verglichen (z. B. 1.–17. November mit 1.–17. Oktober).
pub fn comparison(
    range: &Range,
    date_range: &DateRange,
    cmp: &Comparison,
    tz: Tz,
    now: i64,
    to_date: bool,
) -> AppResult<Range> {
    if range.realtime {
        let len = range.end - range.start;
        return Ok(Range {
            start: range.start - len,
            end: range.start,
            ..range.clone()
        });
    }
    let today = today(tz, now);
    let effective_to = if to_date && range.to > today && range.from <= today {
        today
    } else {
        range.to
    };
    let (mut from, mut to) = match cmp.mode {
        ComparisonMode::Custom => {
            let (a, b) = cmp
                .date_range
                .as_ref()
                .ok_or_else(|| AppError::invalid("Vergleich 'custom' braucht date_range."))?;
            let (from, to) = (parse_date(a)?, parse_date(b)?);
            if from > to {
                return Err(AppError::invalid(
                    "Der Vergleichszeitraum endet vor seinem Beginn.",
                ));
            }
            return Ok(range_of(tz, from, to));
        }
        ComparisonMode::YearOverYear => (minus_year(range.from), minus_year(effective_to)),
        ComparisonMode::PreviousPeriod => match date_range {
            DateRange::Named(n) if n == "month" => {
                let from = add_months(range.from, -1);
                let to = if effective_to < range.to {
                    (from + (effective_to - range.from)).min(last_of_month(from))
                } else {
                    last_of_month(from)
                };
                (from, to)
            }
            DateRange::Named(n) if n == "year" => (minus_year(range.from), minus_year(effective_to)),
            DateRange::Named(n) if n == "6mo" || n == "12mo" => {
                let months = if n == "6mo" { 6 } else { 12 };
                let from = add_months(range.from, -months);
                let to = if effective_to < range.to {
                    add_months(effective_to, -months)
                } else {
                    last_of_month(add_months(range.to, -months))
                };
                (from, to)
            }
            _ => {
                let len = (effective_to - range.from).num_days() + 1;
                let to = range.from - Duration::days(1);
                (to - Duration::days(len - 1), to)
            }
        },
    };
    if cmp.match_day_of_week {
        let shift = (from.weekday().num_days_from_monday() as i64
            - range.from.weekday().num_days_from_monday() as i64)
            .rem_euclid(7);
        from -= Duration::days(shift);
        to -= Duration::days(shift);
    }
    Ok(range_of(tz, from, to))
}

/// Passendes Intervall fuer die Zeit-Dimension `time`.
pub fn default_interval(range: &Range, date_range: &DateRange) -> Interval {
    if range.realtime {
        return Interval::Minute;
    }
    if let DateRange::Named(n) = date_range {
        match n.as_str() {
            "6mo" | "12mo" | "year" => return Interval::Month,
            _ => {}
        }
    }
    match range.days() {
        ..=1 => Interval::Hour,
        2..=92 => Interval::Day,
        _ => Interval::Month,
    }
}

/// Erlaubt? (keine Riesenmengen an Zeitscheiben)
pub fn interval_allowed(range: &Range, interval: Interval) -> bool {
    match interval {
        Interval::Minute => range.realtime || (range.end - range.start) <= 6 * 3600,
        Interval::Hour => range.days() <= 31,
        Interval::Day => range.days() <= 3660,
        Interval::Week | Interval::Month => true,
    }
}

/// Zeitscheiben: Startzeitpunkte (UTC) und Beschriftungen.
pub fn buckets(range: &Range, interval: Interval, tz: Tz) -> (Vec<i64>, Vec<String>) {
    let mut bounds = Vec::new();
    let mut labels = Vec::new();
    let fmt_local = |ts: i64, pattern: &str| -> String {
        DateTime::<Utc>::from_timestamp(ts, 0)
            .unwrap_or_default()
            .with_timezone(&tz)
            .format(pattern)
            .to_string()
    };
    match interval {
        Interval::Minute => {
            let mut t = range.start - range.start.rem_euclid(60);
            while t < range.end {
                bounds.push(t);
                labels.push(fmt_local(t, "%Y-%m-%d %H:%M:00"));
                t += 60;
            }
        }
        Interval::Hour => {
            let mut t = range.start;
            while t < range.end {
                bounds.push(t);
                let local = DateTime::<Utc>::from_timestamp(t, 0)
                    .unwrap_or_default()
                    .with_timezone(&tz);
                labels.push(format!("{} {:02}:00:00", local.format("%Y-%m-%d"), local.hour()));
                t += 3600;
            }
        }
        Interval::Day => {
            let mut d = range.from;
            while d <= range.to {
                bounds.push(midnight(tz, d));
                labels.push(d.format("%Y-%m-%d").to_string());
                d = match d.succ_opt() {
                    Some(n) => n,
                    None => break,
                };
            }
        }
        Interval::Week => {
            let mut d = range.from;
            while d <= range.to {
                bounds.push(midnight(tz, d));
                labels.push(d.format("%Y-%m-%d").to_string());
                // naechster Montag
                let ahead = 7 - d.weekday().num_days_from_monday() as i64;
                d += Duration::days(if d.weekday() == Weekday::Mon { 7 } else { ahead });
            }
        }
        Interval::Month => {
            let mut d = range.from;
            while d <= range.to {
                bounds.push(midnight(tz, d));
                labels.push(d.format("%Y-%m-%d").to_string());
                d = first_of_month(add_months(d, 1));
            }
        }
    }
    (bounds, labels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp()
    }

    #[test]
    fn named_ranges() {
        let tz = tz("Europe/Zurich");
        let now = ts("2026-11-17T10:00:00Z");
        let r = resolve(&DateRange::named("7d"), None, tz, now, None).unwrap();
        assert_eq!(r.labels(), ("2026-11-11".into(), "2026-11-17".into()));
        assert_eq!(r.start, ts("2026-11-10T23:00:00Z"));
        let r = resolve(&DateRange::named("month"), None, tz, now, None).unwrap();
        assert_eq!(r.labels(), ("2026-11-01".into(), "2026-11-30".into()));
        let r = resolve(&DateRange::named("12mo"), None, tz, now, None).unwrap();
        assert_eq!(r.labels(), ("2025-12-01".into(), "2026-11-30".into()));
        let r = resolve(&DateRange::named("day"), Some("2026-03-29"), tz, now, None).unwrap();
        // Tag der Zeitumstellung hat 23 Stunden.
        assert_eq!(r.end - r.start, 23 * 3600);
        assert_eq!(buckets(&r, Interval::Hour, tz).0.len(), 23);
    }

    #[test]
    fn comparisons() {
        let tz = tz("UTC");
        let now = ts("2026-11-17T10:00:00Z");
        let dr = DateRange::named("month");
        let r = resolve(&dr, None, tz, now, None).unwrap();
        let c = comparison(&r, &dr, &Comparison::previous_period(), tz, now, true).unwrap();
        assert_eq!(c.labels(), ("2026-10-01".into(), "2026-10-17".into()));
        let c = comparison(&r, &dr, &Comparison::previous_period(), tz, now, false).unwrap();
        assert_eq!(c.labels(), ("2026-10-01".into(), "2026-10-31".into()));
        let dr = DateRange::named("7d");
        let r = resolve(&dr, None, tz, now, None).unwrap();
        let c = comparison(&r, &dr, &Comparison::previous_period(), tz, now, true).unwrap();
        assert_eq!(c.labels(), ("2026-11-04".into(), "2026-11-10".into()));
        let c = comparison(&r, &dr, &Comparison::year_over_year(), tz, now, true).unwrap();
        assert_eq!(c.labels(), ("2025-11-11".into(), "2025-11-17".into()));
    }

    #[test]
    fn week_and_month_buckets() {
        let tz = tz("UTC");
        let r = range_of(
            tz,
            NaiveDate::from_ymd_opt(2026, 11, 4).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 17).unwrap(),
        );
        let (_, labels) = buckets(&r, Interval::Week, tz);
        assert_eq!(labels, vec!["2026-11-04", "2026-11-09", "2026-11-16"]);
        let r = range_of(
            tz,
            NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 2).unwrap(),
        );
        let (_, labels) = buckets(&r, Interval::Month, tz);
        assert_eq!(labels, vec!["2026-01-15", "2026-02-01", "2026-03-01"]);
    }
}
