//! # nrzanalytics-client
//!
//! Rust-Client fuer [nrzAnalytics](http://100.78.107.162:8390/nicorenezweers/nrzAnalytics) – fuer
//! andere nrz-Apps (axum, Dioxus-fullstack, Leptos …):
//!
//! - **Ereignisse vom Server senden** (`POST /api/event`, kompatibel mit Plausible), z. B. eine
//!   Registrierung oder einen Kauf, ohne JavaScript im Browser.
//! - **Statistiken abfragen** ueber die Stats-API v2 (`POST /api/v2/query`) mit API-Schluessel.
//! - **Snippet** fuer serverseitig gerenderte Seiten.
//!
//! ```no_run
//! # async fn demo() -> Result<(), nrzanalytics_client::Error> {
//! use nrzanalytics_client::{Client, query::{DateRange, Query, metrics, dimensions}};
//!
//! let analytics = Client::new("https://analytics.example.com").with_api_key("nrzan_…");
//!
//! // Ereignis (User-Agent und IP des Besuchers mitgeben, sonst zaehlt alles als ein Besucher)
//! analytics
//!     .event("example.com", "Signup", "https://example.com/register")
//!     .user_agent("Mozilla/5.0 …")
//!     .client_ip("203.0.113.7")
//!     .prop("plan", "pro")
//!     .send()
//!     .await?;
//!
//! // Top-Quellen der letzten 7 Tage
//! let q = Query::new(&[metrics::VISITORS], DateRange::named("7d"))
//!     .site("example.com")
//!     .dimension(dimensions::VISIT_SOURCE)
//!     .limit(10, 0);
//! let result = analytics.query(&q).await?;
//! for row in result.results {
//!     println!("{}: {}", row.dimension(0), row.metric(0));
//! }
//! # Ok(()) }
//! ```

pub use nrzanalytics_shared::query;

use nrzanalytics_shared::query::{Query, QueryResult};
use serde_json::{Map, Value, json};
use std::time::Duration;

/// Fehler des Clients.
#[derive(Debug)]
pub enum Error {
    /// Netzwerk oder Zeitueberschreitung.
    Http(reqwest::Error),
    /// Antwort mit Fehlerstatus (Status, Text).
    Api(u16, String),
    /// Fuer diese Anfrage fehlt der API-Schluessel.
    MissingApiKey,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "nrzAnalytics nicht erreichbar: {e}"),
            Self::Api(status, text) => write!(f, "nrzAnalytics antwortet {status}: {text}"),
            Self::MissingApiKey => write!(f, "API-Schlüssel fehlt (Client::with_api_key)"),
        }
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

/// Client fuer eine nrzAnalytics-Instanz.
#[derive(Clone, Debug)]
pub struct Client {
    base: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

impl Client {
    /// `base_url` z. B. `https://analytics.example.com` (ohne `/` am Ende).
    pub fn new(base_url: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("nrzanalytics-client/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            base: base_url.into().trim_end_matches('/').to_string(),
            api_key: None,
            http,
        }
    }

    /// API-Schluessel (Konto → Einstellungen → API-Schluessel) fuer Abfragen.
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Eigener reqwest-Client (Proxy, Zeitlimits …).
    pub fn with_http_client(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    /// Ereignis vorbereiten. `name` ist `pageview` oder ein eigener Name (`Signup`).
    pub fn event(&self, domain: &str, name: &str, url: &str) -> EventBuilder<'_> {
        EventBuilder {
            client: self,
            body: json!({ "name": name, "url": url, "domain": domain }),
            user_agent: None,
            client_ip: None,
        }
    }

    fn key(&self) -> Result<&str, Error> {
        self.api_key.as_deref().ok_or(Error::MissingApiKey)
    }

    async fn check(resp: reqwest::Response) -> Result<reqwest::Response, Error> {
        let status = resp.status();
        if status.is_success() {
            Ok(resp)
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(Error::Api(status.as_u16(), text))
        }
    }

    /// Abfrage ueber die Stats-API v2 (`site_id` in der Abfrage setzen, z. B. mit `.site(...)`).
    pub async fn query(&self, q: &Query) -> Result<QueryResult, Error> {
        let resp = self
            .http
            .post(format!("{}/api/v2/query", self.base))
            .bearer_auth(self.key()?)
            .json(q)
            .send()
            .await?;
        Ok(Self::check(resp).await?.json().await?)
    }

    /// Aktuelle Besucher (letzte 5 Minuten).
    pub async fn realtime_visitors(&self, site_id: &str) -> Result<u64, Error> {
        let resp = self
            .http
            .get(format!("{}/api/v1/stats/realtime/visitors", self.base))
            .query(&[("site_id", site_id)])
            .bearer_auth(self.key()?)
            .send()
            .await?;
        Ok(Self::check(resp).await?.json().await?)
    }

    /// Erreichbar? (`GET /api/health`)
    pub async fn health(&self) -> Result<String, Error> {
        let resp = self.http.get(format!("{}/api/health", self.base)).send().await?;
        let v: Value = Self::check(resp).await?.json().await?;
        Ok(v.get("version")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// Snippet fuer `<head>` (serverseitig gerenderte Seiten).
    pub fn snippet(&self, domain: &str) -> String {
        snippet(&self.base, domain)
    }
}

/// Snippet fuer `<head>`: `<script defer data-domain="…" src="…/js/script.js"></script>`.
pub fn snippet(base_url: &str, domain: &str) -> String {
    format!(
        "<script defer data-domain=\"{domain}\" src=\"{}/js/script.js\"></script>",
        base_url.trim_end_matches('/')
    )
}

/// Ereignis vor dem Senden.
#[derive(Debug)]
pub struct EventBuilder<'a> {
    client: &'a Client,
    body: Value,
    user_agent: Option<String>,
    client_ip: Option<String>,
}

impl EventBuilder<'_> {
    /// User-Agent des Besuchers (Pflicht fuer korrekte Besucherzaehlung und Bot-Erkennung).
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// IP-Adresse des Besuchers (wird als `X-Forwarded-For` gesendet und nicht gespeichert).
    pub fn client_ip(mut self, ip: impl Into<String>) -> Self {
        self.client_ip = Some(ip.into());
        self
    }

    /// User-Agent und IP aus den Headern einer eingehenden Anfrage uebernehmen
    /// (`User-Agent`, `X-Forwarded-For`, `X-Real-IP`, `CF-Connecting-IP`).
    pub fn from_headers(mut self, headers: &reqwest::header::HeaderMap) -> Self {
        let get = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        if let Some(ua) = get("user-agent") {
            self.user_agent = Some(ua.to_string());
        }
        let ip = get("cf-connecting-ip").or_else(|| get("x-real-ip")).or_else(|| {
            get("x-forwarded-for")
                .and_then(|v| v.split(',').next())
                .map(str::trim)
        });
        if let Some(ip) = ip {
            self.client_ip = Some(ip.to_string());
        }
        self
    }

    /// Verweis-URL (woher der Besucher kam).
    pub fn referrer(mut self, referrer: impl Into<String>) -> Self {
        self.body["referrer"] = Value::String(referrer.into());
        self
    }

    /// Eigene Eigenschaft (hoechstens 30 je Ereignis).
    pub fn prop(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if !self.body.get("props").is_some_and(Value::is_object) {
            self.body["props"] = Value::Object(Map::new());
        }
        if let Some(props) = self.body.get_mut("props").and_then(Value::as_object_mut) {
            props.insert(key.into(), Value::String(value.into()));
        }
        self
    }

    /// Umsatz (fuer Umsatz-Ziele), Waehrung als ISO-Code (`CHF`).
    pub fn revenue(mut self, amount: f64, currency: &str) -> Self {
        self.body["revenue"] = json!({ "amount": amount, "currency": currency });
        self
    }

    /// Fensterbreite in Pixeln (fuer die Geraeteklasse).
    pub fn screen_width(mut self, width: u32) -> Self {
        self.body["screen_width"] = json!(width);
        self
    }

    /// Ereignis zaehlt nicht als Interaktion (beeinflusst die Absprungrate nicht).
    pub fn non_interactive(mut self) -> Self {
        self.body["interactive"] = json!(false);
        self
    }

    /// Senden. Antwortet nrzAnalytics mit 202, ist alles gut (auch wenn das Ereignis z. B. als Bot
    /// verworfen wurde).
    pub async fn send(self) -> Result<(), Error> {
        let mut req = self
            .client
            .http
            .post(format!("{}/api/event", self.client.base))
            .json(&self.body);
        if let Some(ua) = &self.user_agent {
            req = req.header(reqwest::header::USER_AGENT, ua);
        }
        if let Some(ip) = &self.client_ip {
            req = req.header("X-Forwarded-For", ip);
        }
        Client::check(req.send().await?).await?;
        Ok(())
    }

    /// Anfrage-Body (zum Pruefen und Protokollieren).
    pub fn body(&self) -> &Value {
        &self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_events() {
        let c = Client::new("https://a.example.com/");
        assert_eq!(c.base_url(), "https://a.example.com");
        let e = c
            .event("example.com", "Signup", "https://example.com/")
            .prop("plan", "pro")
            .revenue(9.5, "CHF")
            .non_interactive();
        assert_eq!(e.body()["props"]["plan"], "pro");
        assert_eq!(e.body()["revenue"]["currency"], "CHF");
        assert_eq!(e.body()["interactive"], false);
        assert!(
            c.snippet("example.com")
                .contains("https://a.example.com/js/script.js")
        );
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("x-forwarded-for", "203.0.113.7, 10.0.0.1".parse().unwrap());
        h.insert("user-agent", "UA".parse().unwrap());
        let e = c
            .event("example.com", "pageview", "https://example.com/")
            .from_headers(&h);
        assert_eq!(e.client_ip.as_deref(), Some("203.0.113.7"));
        assert_eq!(e.user_agent.as_deref(), Some("UA"));
    }
}
