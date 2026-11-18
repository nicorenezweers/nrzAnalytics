//! Laender, Regionen und Staedte aus der IP-Adresse (MaxMind-Format `.mmdb`).
//!
//! Standard: DB-IP Lite Country (CC BY 4.0, https://db-ip.com), monatlich selbst geladen nach
//! `<data>/geoip/`. Die IP-Adresse selbst wird nie gespeichert.

use crate::config::GeoipMode;
use maxminddb::{Reader, geoip2};
use nrzanalytics_shared::api::GeoipStatus;
use std::{
    io::Read,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

/// Ergebnis einer Abfrage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Location {
    /// ISO-3166-1, z. B. `CH`.
    pub country: String,
    /// ISO-3166-2, z. B. `CH-ZH` (leer ohne Stadt-Datenbank).
    pub region: String,
    pub region_name: String,
    pub city: String,
}

pub struct GeoIp {
    mode: GeoipMode,
    dir: PathBuf,
    reader: RwLock<Option<Arc<Reader<Vec<u8>>>>>,
    message: RwLock<String>,
}

fn name_of<'a>(names: &Option<std::collections::BTreeMap<&'a str, &'a str>>) -> String {
    names
        .as_ref()
        .and_then(|n| n.get("de").or_else(|| n.get("en")).map(|s| (*s).to_string()))
        .unwrap_or_default()
}

impl GeoIp {
    pub fn new(mode: GeoipMode, data_dir: &Path) -> Arc<Self> {
        let geo = Arc::new(Self {
            mode,
            dir: data_dir.join("geoip"),
            reader: RwLock::new(None),
            message: RwLock::new(String::new()),
        });
        geo.load();
        geo
    }

    fn file(&self) -> Option<PathBuf> {
        match &self.mode {
            GeoipMode::Off => None,
            GeoipMode::DbipCountry => Some(self.dir.join("dbip-country-lite.mmdb")),
            GeoipMode::DbipCity => Some(self.dir.join("dbip-city-lite.mmdb")),
            GeoipMode::File(p) => Some(p.clone()),
        }
    }

    fn set_message(&self, msg: impl Into<String>) {
        if let Ok(mut m) = self.message.write() {
            *m = msg.into();
        }
    }

    /// Datei (neu) laden.
    pub fn load(&self) {
        let Some(path) = self.file() else {
            self.set_message("Länderzuordnung ist ausgeschaltet (NRZANALYTICS_GEOIP=off).");
            return;
        };
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| Reader::from_source(b).map_err(|e| e.to_string()))
        {
            Ok(r) => {
                tracing::info!("GeoIP: {} ({})", path.display(), r.metadata.database_type);
                if let Ok(mut slot) = self.reader.write() {
                    *slot = Some(Arc::new(r));
                }
                self.set_message("Bereit.");
            }
            Err(e) => {
                if path.exists() {
                    tracing::warn!("GeoIP-Datei {} unlesbar: {e}", path.display());
                    self.set_message(format!("Datei unlesbar: {e}"));
                } else {
                    self.set_message("Datenbank wird geladen …");
                }
            }
        }
    }

    pub fn lookup(&self, ip: IpAddr) -> Option<Location> {
        let reader = self.reader.read().ok()?.clone()?;
        let city: geoip2::City<'_> = reader.lookup(ip).ok()??;
        let country = city
            .country
            .as_ref()
            .and_then(|c| c.iso_code)
            .unwrap_or_default()
            .to_ascii_uppercase();
        if country.len() != 2 {
            return None;
        }
        let (region, region_name) = city
            .subdivisions
            .as_ref()
            .and_then(|s| s.first())
            .map(|s| match s.iso_code {
                Some(code) if !code.is_empty() => (
                    format!("{country}-{}", code.to_ascii_uppercase()),
                    name_of(&s.names),
                ),
                _ => (String::new(), String::new()),
            })
            .unwrap_or_default();
        let city_name = city.city.as_ref().map(|c| name_of(&c.names)).unwrap_or_default();
        Some(Location {
            country,
            region,
            region_name,
            city: city_name,
        })
    }

    pub fn status(&self) -> GeoipStatus {
        let reader = self.reader.read().ok().and_then(|r| r.clone());
        let message = self.message.read().map(|m| m.clone()).unwrap_or_default();
        match reader {
            Some(r) => {
                let build = chrono::DateTime::<chrono::Utc>::from_timestamp(r.metadata.build_epoch as i64, 0)
                    .map(|d| d.format("%Y-%m-%d").to_string());
                let db_type = r.metadata.database_type.clone();
                let city_level = db_type.to_ascii_lowercase().contains("city");
                GeoipStatus {
                    enabled: true,
                    database: Some(db_type),
                    build_date: build,
                    city_level,
                    message,
                }
            }
            None => GeoipStatus {
                enabled: false,
                database: None,
                build_date: None,
                city_level: false,
                message,
            },
        }
    }

    /// Hintergrund: fehlende oder veraltete DB-IP-Datei laden (beim Start und taeglich pruefen).
    pub async fn maintain(self: Arc<Self>) {
        let kind = match self.mode {
            GeoipMode::DbipCountry => "country",
            GeoipMode::DbipCity => "city",
            _ => return,
        };
        loop {
            if let Some(path) = self.file() {
                let stale = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .map(|t| {
                        SystemTime::now().duration_since(t).unwrap_or_default()
                            > Duration::from_secs(35 * 86_400)
                    })
                    .unwrap_or(true);
                if stale {
                    match download(kind, &path).await {
                        Ok(()) => self.load(),
                        Err(e) => {
                            tracing::warn!("GeoIP-Download fehlgeschlagen: {e}");
                            if self.reader.read().map(|r| r.is_none()).unwrap_or(true) {
                                self.set_message(format!("Download fehlgeschlagen: {e}"));
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(86_400)).await;
        }
    }
}

/// DB-IP Lite fuer diesen oder (Anfang Monat) den letzten Monat laden.
async fn download(kind: &str, target: &Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("nrzAnalytics/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().date_naive();
    let months = [
        now,
        now - chrono::Duration::days(28),
        now - chrono::Duration::days(56),
    ];
    let mut last_err = String::from("keine Datei gefunden");
    for d in months {
        let url = format!(
            "https://download.db-ip.com/free/dbip-{kind}-lite-{}.mmdb.gz",
            d.format("%Y-%m")
        );
        tracing::info!("GeoIP: lade {url}");
        let resp = match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                last_err = format!("{url}: HTTP {}", r.status());
                continue;
            }
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        let target = target.to_path_buf();
        return tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut data = Vec::new();
            flate2::read::GzDecoder::new(&bytes[..])
                .read_to_end(&mut data)
                .map_err(|e| e.to_string())?;
            Reader::from_source(data.clone()).map_err(|e| format!("ungültige Datei: {e}"))?;
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let tmp = target.with_extension("mmdb.tmp");
            std::fs::write(&tmp, &data).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    Err(last_err)
}
