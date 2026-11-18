//! Beispieldaten fuer Entwicklung und Vorfuehrung: `nrzanalytics demo-data <domain> [tage]`.
//!
//! Erzeugt realistische Besuche ueber die echte Erfassung (Sitzungen, Salz, GeoIP) mit Zeitstempeln
//! in der Vergangenheit: Tagesverlauf, Wochentage, Quellen, Kampagnen, Seiten, Ziele, Umsatz.

use crate::{
    config::Config,
    db::{Db, now},
    error::AppResult,
    geoip::GeoIp,
    ingest::{IncomingEvent, Ingest, RequestInfo},
};
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde_json::json;

const UAS: &[(&str, i64)] = &[
    (
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        1920,
    ),
    (
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15",
        1512,
    ),
    (
        "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1",
        393,
    ),
    (
        "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
        412,
    ),
    (
        "Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0",
        1366,
    ),
    (
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
        1280,
    ),
    (
        "Mozilla/5.0 (iPad; CPU OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1",
        820,
    ),
    (
        "Mozilla/5.0 (Linux; Android 14; SAMSUNG SM-A546B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/26.0 Chrome/122.0.0.0 Mobile Safari/537.36",
        384,
    ),
];

/// (Netz-Praefix, Gewicht) – oeffentliche Adressbereiche verschiedener Laender.
const NETS: &[(&str, u32)] = &[
    ("178.197", 30), // CH
    ("85.0", 10),    // CH
    ("91.64", 20),   // DE
    ("87.123", 10),  // DE
    ("77.116", 8),   // AT
    ("90.84", 6),    // FR
    ("81.2", 5),     // GB
    ("8.8", 6),      // US
    ("151.38", 3),   // IT
    ("145.53", 3),   // NL
];

const REFERRERS: &[(&str, u32)] = &[
    ("", 38),
    ("https://www.google.com/", 24),
    ("https://duckduckgo.com/", 7),
    ("https://www.bing.com/", 3),
    ("https://news.ycombinator.com/item?id=4711", 5),
    ("https://www.reddit.com/r/rust/comments/abc", 5),
    ("https://t.co/xyz", 4),
    ("https://github.com/nicorenezweers", 4),
    ("https://www.linkedin.com/feed/", 3),
    ("https://mastodon.social/@nrz", 2),
    ("https://chatgpt.com/", 2),
];

const PAGES: &[(&str, u32)] = &[
    ("/", 30),
    ("/blog", 10),
    ("/blog/rust-tipps", 9),
    ("/blog/umbrel-apps", 7),
    ("/blog/nrzui-komponenten", 5),
    ("/preise", 8),
    ("/docs/start", 8),
    ("/docs/api", 5),
    ("/kontakt", 4),
    ("/download", 6),
];

fn pick<'a, T>(rng: &mut StdRng, list: &'a [(T, u32)]) -> &'a T {
    let total: u32 = list.iter().map(|(_, w)| *w).sum();
    let mut x = rng.gen_range(0..total.max(1));
    for (item, w) in list {
        if x < *w {
            return item;
        }
        x -= w;
    }
    &list[0].0
}

pub async fn run(domain: &str, days: i64) -> AppResult<usize> {
    let config = Config::from_env();
    let db = Db::open(&config.database).map_err(crate::error::AppError::internal)?;
    let domain = crate::ingest::normalize_domain(domain);
    let d = domain.clone();
    db.write(move |c| {
        c.execute(
            "INSERT OR IGNORE INTO sites (domain, timezone, created_at) VALUES (?1, 'Europe/Zurich', ?2)",
            rusqlite::params![d, now()],
        )?;
        let site: i64 = c.query_row("SELECT id FROM sites WHERE domain = ?1", [&d], |r| r.get(0))?;
        // Dem ersten Admin zuordnen, damit die Website in der Liste erscheint.
        c.execute(
            "INSERT OR IGNORE INTO memberships (site_id, user_id, role, created_at) SELECT ?1, id, 'owner', ?2 FROM users WHERE is_admin = 1 ORDER BY id LIMIT 1",
            rusqlite::params![site, now()],
        )?;
        for (kind, event, page, name, currency) in [
            ("event", Some("Signup"), None, "Signup", None),
            ("event", Some("Download"), None, "Download", None),
            ("event", Some("Kauf"), None, "Kauf", Some("CHF")),
            ("page", None, Some("/preise"), "Besuch /preise", None),
            ("page", None, Some("/blog/**"), "Besuch /blog/**", None),
        ] {
            c.execute(
                "INSERT OR IGNORE INTO goals (site_id, kind, event_name, page_path, display_name, currency, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![site, kind, event, page, name, currency, now()],
            )?;
        }
        Ok(())
    })
    .await?;
    let geo = GeoIp::new(config.geoip.clone(), &config.data_dir);
    let (ingest, rx) = Ingest::new(db.clone(), geo);
    let writer = tokio::spawn(crate::ingest::writer(db.clone(), rx));
    ingest.reload_sites().await?;
    ingest.rotate_salts().await?;

    let mut rng = StdRng::seed_from_u64(42);
    let mut sent = 0usize;
    let end = now();
    let tz = crate::stats::time::tz("Europe/Zurich");
    let today = crate::stats::time::today(tz, end);
    for day_offset in (0..days).rev() {
        let date = today - chrono::Duration::days(day_offset);
        let day_start = crate::stats::time::midnight(tz, date);
        let weekday = chrono::Datelike::weekday(&date).num_days_from_monday();
        let trend = 1.0 + (days - day_offset) as f64 / days.max(1) as f64 * 0.6;
        let weekly = if weekday >= 5 { 0.65 } else { 1.0 };
        let visitors = (rng.gen_range(45.0..75.0) * trend * weekly) as usize;
        for _ in 0..visitors {
            // Tagesverlauf: mehr Besuche tagsueber.
            let hour = loop {
                let h: f64 = rng.gen_range(0.0..24.0);
                let weight = 0.15 + (-((h - 14.0) / 4.5).powi(2)).exp();
                if rng.gen_range(0.0..1.2) < weight {
                    break h;
                }
            };
            let mut ts = day_start + (hour * 3600.0) as i64;
            if ts > end {
                continue;
            }
            let (ua, width) = UAS[rng.gen_range(0..UAS.len())];
            let net = pick(&mut rng, NETS);
            let ip = format!("{net}.{}.{}", rng.gen_range(1..250), rng.gen_range(1..250));
            let req = RequestInfo {
                ip: ip.parse().ok(),
                user_agent: ua.into(),
                country_header: None,
            };
            let referrer = *pick(&mut rng, REFERRERS);
            let campaign = rng.gen_range(0..100) < 8;
            let mut page = *pick(&mut rng, PAGES);
            let views = 1 + (rng.gen_range(0.0f64..1.0).powi(2) * 5.0) as usize;
            for v in 0..views {
                let url = if v == 0 && campaign {
                    format!(
                        "https://{domain}{page}?utm_source=newsletter&utm_medium=email&utm_campaign=herbst-2026"
                    )
                } else {
                    format!("https://{domain}{page}")
                };
                let ev: IncomingEvent = serde_json::from_value(json!({
                    "n": "pageview", "u": url, "d": domain, "r": if v == 0 && !referrer.is_empty() { Some(referrer) } else { None }, "w": width
                }))
                .map_err(crate::error::AppError::internal)?;
                sent += ingest.process_at(&ev, &req, ts)?;
                // Lesezeit und Scrolltiefe
                let read = rng.gen_range(4..240);
                let eng: IncomingEvent = serde_json::from_value(json!({
                    "n": "engagement", "u": format!("https://{domain}{page}"), "d": domain, "e": read * 1000, "sd": rng.gen_range(20..100)
                }))
                .map_err(crate::error::AppError::internal)?;
                ts += read;
                ingest.process_at(&eng, &req, ts.min(end))?;
                // Ziele
                let roll = rng.gen_range(0..1000);
                let custom = if page == "/preise" && roll < 120 {
                    Some(
                        json!({"n": "Signup", "u": format!("https://{domain}{page}"), "d": domain, "p": {"plan": if roll < 40 { "pro" } else { "basic" }}}),
                    )
                } else if page == "/download" && roll < 400 {
                    Some(
                        json!({"n": "Download", "u": format!("https://{domain}{page}"), "d": domain, "p": {"os": if roll < 200 { "macOS" } else { "Linux" }}}),
                    )
                } else if page == "/preise" && roll < 160 {
                    Some(
                        json!({"n": "Kauf", "u": format!("https://{domain}{page}"), "d": domain, "$": {"amount": if roll < 140 { 29.0 } else { 99.0 }, "currency": "CHF"}}),
                    )
                } else {
                    None
                };
                if let Some(c) = custom {
                    let ev: IncomingEvent =
                        serde_json::from_value(c).map_err(crate::error::AppError::internal)?;
                    ingest.process_at(&ev, &req, (ts + 5).min(end))?;
                }
                page = *pick(&mut rng, PAGES);
                ts += rng.gen_range(5..60);
                if ts > end {
                    break;
                }
            }
        }
        ingest.flush().await;
    }
    ingest.flush().await;
    drop(ingest);
    writer.abort();
    Ok(sent)
}
