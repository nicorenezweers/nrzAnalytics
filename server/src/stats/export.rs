//! CSV-Export (ZIP mit einer Datei je Bericht, wie bei Plausible).

use super::{SiteCtx, execute};
use crate::error::AppResult;
use nrzanalytics_shared::{
    api::ExportQuery,
    labels,
    query::{Query, QueryResult, dimensions as d, metrics as m},
};
use rusqlite::Connection;
use std::io::Write;

/// Feld fuer CSV (mit Anfuehrungszeichen, wenn noetig).
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn csv(header: &[&str], result: &QueryResult, label: impl Fn(&str) -> String) -> String {
    let mut out = header.join(",");
    out.push('\n');
    for row in &result.results {
        let mut cols: Vec<String> = row.dimensions.iter().map(|v| csv_field(&label(v))).collect();
        cols.extend(row.metrics.iter().map(|v| match v {
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        }));
        out.push_str(&cols.join(","));
        out.push('\n');
    }
    out
}

pub fn export_zip(c: &Connection, site: &SiteCtx, eq: &ExportQuery, now: i64) -> AppResult<Vec<u8>> {
    let base = |metrics: &[&str]| {
        Query::new(metrics, eq.date_range.clone())
            .date(eq.date.clone())
            .filters(eq.filters.clone())
            .limit(10_000, 0)
    };
    let mut files: Vec<(String, String)> = Vec::new();

    let series = execute(
        c,
        site,
        &base(&[
            m::VISITORS,
            m::PAGEVIEWS,
            m::VISITS,
            m::VIEWS_PER_VISIT,
            m::BOUNCE_RATE,
            m::VISIT_DURATION,
        ])
        .dimension(d::TIME_DAY)
        .zero_fill(),
        now,
    )?;
    files.push((
        "visitors.csv".into(),
        csv(
            &[
                "date",
                "visitors",
                "pageviews",
                "visits",
                "views_per_visit",
                "bounce_rate",
                "visit_duration",
            ],
            &series,
            str::to_string,
        ),
    ));

    let visit_reports: [(&str, &str); 14] = [
        ("channels.csv", d::VISIT_CHANNEL),
        ("sources.csv", d::VISIT_SOURCE),
        ("referrers.csv", d::VISIT_REFERRER),
        ("utm_mediums.csv", d::VISIT_UTM_MEDIUM),
        ("utm_sources.csv", d::VISIT_UTM_SOURCE),
        ("utm_campaigns.csv", d::VISIT_UTM_CAMPAIGN),
        ("utm_contents.csv", d::VISIT_UTM_CONTENT),
        ("utm_terms.csv", d::VISIT_UTM_TERM),
        ("countries.csv", d::VISIT_COUNTRY),
        ("regions.csv", d::VISIT_REGION),
        ("cities.csv", d::VISIT_CITY),
        ("browsers.csv", d::VISIT_BROWSER),
        ("operating_systems.csv", d::VISIT_OS),
        ("devices.csv", d::VISIT_DEVICE),
    ];
    for (file, dim) in visit_reports {
        let r = execute(
            c,
            site,
            &base(&[m::VISITORS, m::VISITS, m::BOUNCE_RATE, m::VISIT_DURATION]).dimension(dim),
            now,
        )?;
        let dim_owned = dim.to_string();
        files.push((
            file.into(),
            csv(
                &["name", "visitors", "visits", "bounce_rate", "visit_duration"],
                &r,
                move |v| labels::dimension_value(&dim_owned, v),
            ),
        ));
    }

    let pages = execute(
        c,
        site,
        &base(&[
            m::VISITORS,
            m::PAGEVIEWS,
            m::BOUNCE_RATE,
            m::TIME_ON_PAGE,
            m::SCROLL_DEPTH,
        ])
        .dimension(d::EVENT_PAGE),
        now,
    )?;
    files.push((
        "pages.csv".into(),
        csv(
            &[
                "name",
                "visitors",
                "pageviews",
                "bounce_rate",
                "time_on_page",
                "scroll_depth",
            ],
            &pages,
            str::to_string,
        ),
    ));
    let entry = execute(
        c,
        site,
        &base(&[m::VISITORS, m::VISITS, m::BOUNCE_RATE, m::VISIT_DURATION]).dimension(d::VISIT_ENTRY_PAGE),
        now,
    )?;
    files.push((
        "entry_pages.csv".into(),
        csv(
            &[
                "name",
                "unique_entrances",
                "total_entrances",
                "bounce_rate",
                "visit_duration",
            ],
            &entry,
            str::to_string,
        ),
    ));
    let exit = execute(
        c,
        site,
        &base(&[m::VISITORS, m::VISITS, m::EXIT_RATE]).dimension(d::VISIT_EXIT_PAGE),
        now,
    )?;
    files.push((
        "exit_pages.csv".into(),
        csv(
            &["name", "unique_exits", "total_exits", "exit_rate"],
            &exit,
            str::to_string,
        ),
    ));

    if !site.goals.is_empty() {
        let goals = execute(
            c,
            site,
            &base(&[m::VISITORS, m::EVENTS, m::CONVERSION_RATE]).dimension(d::EVENT_GOAL),
            now,
        )?;
        files.push((
            "conversions.csv".into(),
            csv(
                &[
                    "name",
                    "unique_conversions",
                    "total_conversions",
                    "conversion_rate",
                ],
                &goals,
                str::to_string,
            ),
        ));
    }

    zip(&files, now)
}

/// Minimaler ZIP-Schreiber (Deflate, UTF-8-Namen).
pub fn zip(files: &[(String, String)], now: i64) -> AppResult<Vec<u8>> {
    use chrono::{Datelike, Timelike};
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(now, 0).unwrap_or_default();
    let dos_time = ((dt.hour() << 11) | (dt.minute() << 5) | (dt.second() / 2)) as u16;
    let dos_date = (((dt.year().max(1980) - 1980) as u32) << 9 | (dt.month() << 5) | dt.day()) as u16;
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, content) in files {
        let data = content.as_bytes();
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let crc = crc.sum();
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(data).map_err(crate::error::AppError::internal)?;
        let compressed = enc.finish().map_err(crate::error::AppError::internal)?;
        let offset = out.len() as u32;
        let name_bytes = name.as_bytes();
        let header = |sig: u32, central_entry: bool| -> Vec<u8> {
            let mut h = Vec::new();
            h.extend_from_slice(&sig.to_le_bytes());
            if central_entry {
                h.extend_from_slice(&20u16.to_le_bytes()); // erstellt mit
            }
            h.extend_from_slice(&20u16.to_le_bytes()); // benoetigt
            h.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8
            h.extend_from_slice(&8u16.to_le_bytes()); // Deflate
            h.extend_from_slice(&dos_time.to_le_bytes());
            h.extend_from_slice(&dos_date.to_le_bytes());
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes()); // extra
            if central_entry {
                h.extend_from_slice(&0u16.to_le_bytes()); // Kommentar
                h.extend_from_slice(&0u16.to_le_bytes()); // Disk
                h.extend_from_slice(&0u16.to_le_bytes()); // intern
                h.extend_from_slice(&0u32.to_le_bytes()); // extern
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(name_bytes);
            h
        };
        out.extend_from_slice(&header(0x0403_4b50, false));
        out.extend_from_slice(&compressed);
        central.extend_from_slice(&header(0x0201_4b50, true));
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}
