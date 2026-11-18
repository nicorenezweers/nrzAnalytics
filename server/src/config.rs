//! Konfiguration aus Umgebungsvariablen (siehe README).

use std::{env, net::SocketAddr, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeoipMode {
    /// Keine Laenderzuordnung (ausser per `CF-IPCountry`).
    Off,
    /// DB-IP Lite Country (CC BY 4.0) monatlich selbst laden.
    DbipCountry,
    /// DB-IP Lite City (gross, ~130 MB) monatlich selbst laden.
    DbipCity,
    /// Eigene MaxMind-/DB-IP-Datei (`.mmdb`).
    File(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Config {
    /// Oberflaeche und alle APIs.
    pub bind: SocketAddr,
    /// Optional: nur oeffentliche Teile (Skript, Ereignisse, geteilte Dashboards, Stats-API).
    pub public_bind: Option<SocketAddr>,
    pub data_dir: PathBuf,
    pub database: PathBuf,
    /// Fertige Oberflaeche (Ordner mit index.html); sonst eingebettet.
    pub web_dir: Option<PathBuf>,
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub secret: Option<String>,
    pub public_url: Option<String>,
    pub trust_proxy: bool,
    pub secure_cookies: bool,
    pub geoip: GeoipMode,
    /// Rohdaten nach so vielen Tagen loeschen (0 = nie).
    pub retention_days: u32,
    /// Registrierung per Umgebung erzwingen (sonst Einstellung in der Oberflaeche).
    pub registration: Option<bool>,
}

fn var(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn flag(name: &str) -> Option<bool> {
    var(name).map(|v| {
        matches!(
            v.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on" | "ja" | "open"
        )
    })
}

impl Config {
    pub fn from_env() -> Self {
        let bind = var("NRZANALYTICS_BIND")
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 8580)));
        let public_bind = var("NRZANALYTICS_PUBLIC_BIND").and_then(|v| v.parse().ok());
        let data_dir = var("NRZANALYTICS_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data"));
        let database = var("NRZANALYTICS_DATABASE")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("analytics.db"));
        let geoip = match var("NRZANALYTICS_GEOIP")
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            None | Some("dbip") | Some("dbip-country") | Some("on") => GeoipMode::DbipCountry,
            Some("dbip-city") | Some("city") => GeoipMode::DbipCity,
            Some("off") | Some("0") | Some("false") | Some("none") => GeoipMode::Off,
            Some(_) => GeoipMode::File(PathBuf::from(var("NRZANALYTICS_GEOIP").unwrap_or_default())),
        };
        Self {
            bind,
            public_bind,
            web_dir: var("NRZANALYTICS_WEB_DIR").map(PathBuf::from),
            admin_email: var("NRZANALYTICS_ADMIN_EMAIL"),
            admin_password: var("NRZANALYTICS_ADMIN_PASSWORD"),
            secret: var("NRZANALYTICS_SECRET"),
            public_url: var("NRZANALYTICS_PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string()),
            trust_proxy: flag("NRZANALYTICS_TRUST_PROXY").unwrap_or(false),
            secure_cookies: flag("NRZANALYTICS_SECURE_COOKIES").unwrap_or(false),
            geoip,
            retention_days: var("NRZANALYTICS_RETENTION_DAYS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            registration: flag("NRZANALYTICS_REGISTRATION"),
            data_dir,
            database,
        }
    }
}
