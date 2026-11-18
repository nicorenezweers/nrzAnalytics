//! Erfassung: Ereignisse annehmen, auswerten, Sitzungen bilden und gebuendelt speichern.
//!
//! Ablauf je Ereignis (`POST /api/event`, kompatibel mit Plausible):
//! 1. Anfrage pruefen, Bots und Verweis-Spam verwerfen.
//! 2. Website(s) zur Domain finden, Schutzregeln anwenden (Hostname, Seite, IP, Land).
//! 3. URL, Verweis, UTM-Parameter, Kanal, Browser/System/Geraet und Standort bestimmen.
//! 4. Besucher-ID = Hash aus taeglich wechselndem Salz, Domain, IP und User-Agent (keine Cookies,
//!    IP und User-Agent werden nicht gespeichert).
//! 5. Sitzung fortsetzen (30 Minuten Inaktivitaet) oder neu anlegen; Ereignis und Sitzung gehen an
//!    den Schreib-Task, der jede Sekunde gebuendelt in SQLite schreibt.

pub mod bots;
pub mod channel;
pub mod referrer;
pub mod ua;

use crate::{
    db::{Db, now},
    error::{AppError, AppResult},
    geoip::GeoIp,
};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    net::IpAddr,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

/// Laenge einer Sitzung ohne Aktivitaet.
pub const SESSION_TIMEOUT: i64 = 30 * 60;

/// Ereignis wie vom Tracking-Skript (kurze Schluessel) oder vom Server gesendet (lange Schluessel).
#[derive(Debug, Deserialize)]
pub struct IncomingEvent {
    #[serde(alias = "n")]
    pub name: String,
    #[serde(alias = "u")]
    pub url: String,
    #[serde(alias = "d")]
    pub domain: String,
    #[serde(alias = "r", default)]
    pub referrer: Option<Value>,
    #[serde(alias = "w", default)]
    pub screen_width: Option<Value>,
    #[serde(alias = "h", default)]
    pub hash_mode: Option<Value>,
    #[serde(alias = "p", default)]
    pub props: Option<Value>,
    #[serde(alias = "m", default)]
    pub meta: Option<Value>,
    #[serde(rename = "revenue", alias = "$", default)]
    pub revenue: Option<Value>,
    #[serde(alias = "i", default)]
    pub interactive: Option<Value>,
    /// Engagement-Zeit in Millisekunden (nur `engagement`).
    #[serde(alias = "e", default)]
    pub engagement_time: Option<Value>,
    /// Scrolltiefe in Prozent (nur `engagement`).
    #[serde(alias = "sd", default)]
    pub scroll_depth: Option<Value>,
}

/// Angaben aus der HTTP-Anfrage.
#[derive(Clone, Debug, Default)]
pub struct RequestInfo {
    pub ip: Option<IpAddr>,
    pub user_agent: String,
    /// `CF-IPCountry` (Cloudflare), falls vorhanden.
    pub country_header: Option<String>,
}

/// Besuchs-Attribute einer Sitzung (werden in jedes Ereignis kopiert).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VisitAttrs {
    pub referrer: String,
    pub source: String,
    pub channel: String,
    pub utm_medium: String,
    pub utm_source: String,
    pub utm_campaign: String,
    pub utm_content: String,
    pub utm_term: String,
    pub country: String,
    pub region: String,
    pub city: String,
    pub device: String,
    pub browser: String,
    pub browser_version: String,
    pub os: String,
    pub os_version: String,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id: i64,
    pub site_id: i64,
    pub user_id: i64,
    pub start: i64,
    pub end: i64,
    pub entry_page: String,
    pub exit_page: String,
    pub hostname: String,
    pub exit_hostname: String,
    pub pageviews: i64,
    pub events: i64,
    pub is_bounce: bool,
    pub attrs: Arc<VisitAttrs>,
}

#[derive(Clone, Debug)]
pub struct EventRow {
    pub site_id: i64,
    pub ts: i64,
    pub name: String,
    pub pathname: String,
    pub hostname: String,
    pub user_id: i64,
    pub session_id: i64,
    pub attrs: Arc<VisitAttrs>,
    pub props: Option<String>,
    pub revenue: Option<f64>,
    pub currency: Option<String>,
    pub engagement_ms: Option<i64>,
    pub scroll_depth: Option<i64>,
}

pub enum WriteOp {
    Event(Box<EventRow>),
    Session(Box<Session>),
    GeoName(String, String),
    Flush(oneshot::Sender<()>),
}

/// Einstellungen einer Website fuer die Erfassung (aus dem Cache).
#[derive(Debug, Default)]
pub struct SiteConf {
    pub id: i64,
    pub domain: String,
    pub allowed_props: Option<HashSet<String>>,
    pub blocked_ips: Vec<ipnet::IpNet>,
    pub blocked_countries: HashSet<String>,
    pub blocked_pages: Vec<Regex>,
    pub allowed_hostnames: Vec<Regex>,
}

#[derive(Clone, Copy)]
struct Salts {
    current: [u8; 32],
    previous: Option<[u8; 32]>,
}

pub struct Ingest {
    db: Db,
    geo: Arc<GeoIp>,
    sites: RwLock<HashMap<String, Arc<SiteConf>>>,
    salts: RwLock<Option<Salts>>,
    sessions: Mutex<HashMap<(i64, i64), Session>>,
    tx: mpsc::Sender<WriteOp>,
    known_regions: Mutex<HashSet<String>>,
}

/// Domain vereinheitlichen: klein, ohne `www.`, ohne Leerzeichen und Punkt am Ende.
pub fn normalize_domain(d: &str) -> String {
    let d = d.trim().trim_end_matches('.').to_ascii_lowercase();
    d.strip_prefix("www.").map(str::to_string).unwrap_or(d)
}

/// Geraeteklasse aus der Fensterbreite (wie Plausible).
pub fn device_from_width(width: i64) -> &'static str {
    match width {
        ..576 => "Mobile",
        576..992 => "Tablet",
        992..1440 => "Laptop",
        _ => "Desktop",
    }
}

fn value_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn value_i64(v: &Option<Value>) -> Option<i64> {
    match v.as_ref()? {
        Value::Number(n) => n.as_f64().map(|f| f as i64),
        Value::String(s) => s.trim().parse::<f64>().ok().map(|f| f as i64),
        _ => None,
    }
}

fn truthy(v: &Option<Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        Some(Value::String(s)) => matches!(s.as_str(), "1" | "true"),
        _ => false,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// Eigene Eigenschaften: hoechstens 30, Namen bis 300, Werte bis 2000 Zeichen, nur einfache Werte.
pub fn clean_props(props: Option<&Value>, meta: Option<&Value>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for source in [props, meta].into_iter().flatten() {
        let obj = match source {
            Value::Object(o) => o.clone(),
            Value::String(s) => match serde_json::from_str::<Value>(s) {
                Ok(Value::Object(o)) => o,
                _ => continue,
            },
            _ => continue,
        };
        for (k, v) in obj {
            if out.len() >= 30 {
                break;
            }
            let key = k.trim();
            if key.is_empty() || key.len() > 300 || key.contains('"') || key.contains('\\') {
                continue;
            }
            if let Some(val) = value_str(&v) {
                let val = val.trim();
                if !val.is_empty() {
                    out.insert(key.to_string(), truncate(val, 2000));
                }
            }
        }
    }
    out
}

fn parse_revenue(v: Option<&Value>) -> Option<(f64, String)> {
    let v = match v? {
        Value::String(s) => serde_json::from_str::<Value>(s).ok()?,
        other => other.clone(),
    };
    let amount = match v.get("amount")? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().replace(',', ".").parse().ok()?,
        _ => return None,
    };
    let currency = v.get("currency")?.as_str()?.trim().to_ascii_uppercase();
    (amount.is_finite()
        && amount > 0.0
        && currency.len() == 3
        && currency.chars().all(|c| c.is_ascii_alphabetic()))
    .then_some((amount, currency))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Platzhalter fuer Hostnamen (`*.example.com`).
fn host_regex(pattern: &str) -> Option<Regex> {
    let mut re = String::from("^");
    for c in pattern.trim().to_ascii_lowercase().chars() {
        if c == '*' {
            re.push_str(".*");
        } else {
            re.push_str(&regex::escape(&c.to_string()));
        }
    }
    re.push('$');
    Regex::new(&re).ok()
}

impl Ingest {
    pub fn new(db: Db, geo: Arc<GeoIp>) -> (Arc<Self>, mpsc::Receiver<WriteOp>) {
        let (tx, rx) = mpsc::channel(50_000);
        let ingest = Arc::new(Self {
            db,
            geo,
            sites: RwLock::new(HashMap::new()),
            salts: RwLock::new(None),
            sessions: Mutex::new(HashMap::new()),
            tx,
            known_regions: Mutex::new(HashSet::new()),
        });
        (ingest, rx)
    }

    /// Website-Cache neu laden (nach Aenderungen an Websites oder Schutzregeln).
    pub async fn reload_sites(&self) -> AppResult<()> {
        let confs = self
            .db
            .read(|c| {
                let mut sites: HashMap<i64, SiteConf> = HashMap::new();
                let mut stmt = c.prepare("SELECT id, domain, allowed_props FROM sites")?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                })?;
                for row in rows {
                    let (id, domain, allowed) = row?;
                    let allowed_props = allowed
                        .and_then(|a| serde_json::from_str::<Vec<String>>(&a).ok())
                        .map(|v| v.into_iter().collect::<HashSet<_>>());
                    sites.insert(
                        id,
                        SiteConf {
                            id,
                            domain,
                            allowed_props,
                            ..SiteConf::default()
                        },
                    );
                }
                let mut stmt = c.prepare("SELECT site_id, kind, value FROM shields")?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })?;
                for row in rows {
                    let (site_id, kind, value) = row?;
                    let Some(conf) = sites.get_mut(&site_id) else {
                        continue;
                    };
                    match kind.as_str() {
                        "ip" => {
                            let net = value
                                .parse::<ipnet::IpNet>()
                                .ok()
                                .or_else(|| value.parse::<IpAddr>().ok().map(ipnet::IpNet::from));
                            if let Some(n) = net {
                                conf.blocked_ips.push(n);
                            }
                        }
                        "country" => {
                            conf.blocked_countries.insert(value.to_ascii_uppercase());
                        }
                        "page" => {
                            if let Ok(re) = Regex::new(&crate::stats::sql::wildcard_regex(&value)) {
                                conf.blocked_pages.push(re);
                            }
                        }
                        "hostname" => {
                            if let Some(re) = host_regex(&value) {
                                conf.allowed_hostnames.push(re);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(sites
                    .into_values()
                    .map(|s| (s.domain.clone(), Arc::new(s)))
                    .collect::<HashMap<_, _>>())
            })
            .await?;
        if let Ok(mut sites) = self.sites.write() {
            *sites = confs;
        }
        Ok(())
    }

    /// Salz des Tages pruefen und bei Tageswechsel (UTC) erneuern; alte Salze werden geloescht.
    pub async fn rotate_salts(&self) -> AppResult<()> {
        let salts = self
            .db
            .write(|c| {
                let day_start = now() - now().rem_euclid(86_400);
                let latest: Option<(Vec<u8>, i64)> = c
                    .query_row(
                        "SELECT salt, created_at FROM salts ORDER BY id DESC LIMIT 1",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .ok();
                if latest.as_ref().is_none_or(|(_, created)| *created < day_start) {
                    let salt: [u8; 32] = rand::random();
                    c.execute(
                        "INSERT INTO salts (salt, created_at) VALUES (?1, ?2)",
                        rusqlite::params![salt.to_vec(), now()],
                    )?;
                }
                c.execute(
                    "DELETE FROM salts WHERE id NOT IN (SELECT id FROM salts ORDER BY id DESC LIMIT 2)",
                    [],
                )?;
                let mut stmt = c.prepare("SELECT salt FROM salts ORDER BY id DESC LIMIT 2")?;
                let list: Vec<Vec<u8>> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
                let to_arr = |v: &Vec<u8>| -> [u8; 32] {
                    let mut a = [0u8; 32];
                    for (i, b) in v.iter().take(32).enumerate() {
                        a[i] = *b;
                    }
                    a
                };
                let current = list
                    .first()
                    .map(to_arr)
                    .ok_or_else(|| AppError::internal("kein Salz"))?;
                Ok(Salts {
                    current,
                    previous: list.get(1).map(to_arr),
                })
            })
            .await?;
        if let Ok(mut s) = self.salts.write() {
            *s = Some(salts);
        }
        Ok(())
    }

    /// Offene Sitzungen nach einem Neustart weiterfuehren.
    pub async fn load_sessions(&self) -> AppResult<()> {
        let list = self
            .db
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT id, site_id, user_id, start, end, entry_page, exit_page, hostname, exit_hostname, pageviews, events, \
                     is_bounce, referrer, source, channel, utm_medium, utm_source, utm_campaign, utm_content, utm_term, country, \
                     region, city, device, browser, browser_version, os, os_version FROM sessions WHERE end >= ?1",
                )?;
                let rows = stmt.query_map([now() - SESSION_TIMEOUT], |r| {
                    Ok(Session {
                        id: r.get(0)?,
                        site_id: r.get(1)?,
                        user_id: r.get(2)?,
                        start: r.get(3)?,
                        end: r.get(4)?,
                        entry_page: r.get(5)?,
                        exit_page: r.get(6)?,
                        hostname: r.get(7)?,
                        exit_hostname: r.get(8)?,
                        pageviews: r.get(9)?,
                        events: r.get(10)?,
                        is_bounce: r.get::<_, i64>(11)? != 0,
                        attrs: Arc::new(VisitAttrs {
                            referrer: r.get(12)?,
                            source: r.get(13)?,
                            channel: r.get(14)?,
                            utm_medium: r.get(15)?,
                            utm_source: r.get(16)?,
                            utm_campaign: r.get(17)?,
                            utm_content: r.get(18)?,
                            utm_term: r.get(19)?,
                            country: r.get(20)?,
                            region: r.get(21)?,
                            city: r.get(22)?,
                            device: r.get(23)?,
                            browser: r.get(24)?,
                            browser_version: r.get(25)?,
                            os: r.get(26)?,
                            os_version: r.get(27)?,
                        }),
                    })
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .await?;
        if let Ok(mut map) = self.sessions.lock() {
            for s in list {
                map.insert((s.site_id, s.user_id), s);
            }
        }
        Ok(())
    }

    /// Abgelaufene Sitzungen aus dem Speicher entfernen (sie sind schon gespeichert).
    pub fn prune_sessions(&self) {
        let cutoff = now() - SESSION_TIMEOUT - 60;
        if let Ok(mut map) = self.sessions.lock() {
            map.retain(|_, s| s.end >= cutoff);
        }
    }

    /// Alles Gepufferte sofort schreiben.
    pub async fn flush(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(WriteOp::Flush(tx)).await.is_ok() {
            let _ = rx.await;
        }
    }

    fn user_id(salt: &[u8; 32], domain: &str, ip: &str, ua: &str) -> i64 {
        let mut h = Sha256::new();
        h.update(salt);
        h.update(domain.as_bytes());
        h.update([0]);
        h.update(ip.as_bytes());
        h.update([0]);
        h.update(ua.as_bytes());
        let d = h.finalize();
        let mut b = [0u8; 8];
        b.copy_from_slice(&d[..8]);
        (i64::from_le_bytes(b) & i64::MAX).max(1)
    }

    /// Ein Ereignis verarbeiten. `Ok(n)` = fuer n Websites gespeichert.
    pub fn process(&self, ev: &IncomingEvent, req: &RequestInfo) -> Result<usize, AppError> {
        self.process_at(ev, req, now())
    }

    pub fn process_at(&self, ev: &IncomingEvent, req: &RequestInfo, ts: i64) -> Result<usize, AppError> {
        let name = ev.name.trim();
        if name.is_empty() || name.len() > 120 {
            return Err(AppError::invalid("Ungültiger Ereignisname."));
        }
        let url = url::Url::parse(ev.url.trim()).map_err(|_| AppError::invalid("Ungültige URL."))?;
        if ev.domain.trim().is_empty() {
            return Err(AppError::invalid("Domain fehlt."));
        }
        let ua = ua::parse(&req.user_agent);
        if ua.is_bot {
            return Ok(0);
        }
        let hostname = url.host_str().map(normalize_domain).unwrap_or_default();
        let mut pathname = percent_decode(url.path());
        if pathname.is_empty() {
            pathname = "/".into();
        }
        if truthy(&ev.hash_mode)
            && let Some(fragment) = url.fragment()
        {
            pathname = format!("{pathname}#{}", percent_decode(fragment));
        }
        let pathname = truncate(&pathname, 2000);
        let referrer_raw = ev.referrer.as_ref().and_then(value_str).unwrap_or_default();
        let referrer = referrer::parse(&referrer_raw, &hostname);
        if referrer.as_ref().is_some_and(|r| bots::is_spam_referrer(&r.host)) {
            return Ok(0);
        }
        let query: HashMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let q = |k: &str| query.get(k).map(|v| truncate(v.trim(), 300)).unwrap_or_default();

        let domains: Vec<String> = ev
            .domain
            .split(',')
            .map(normalize_domain)
            .filter(|d| !d.is_empty())
            .take(10)
            .collect();
        let confs: Vec<Arc<SiteConf>> = {
            let sites = self.sites.read().map_err(AppError::internal)?;
            let mut seen = HashSet::new();
            domains
                .iter()
                .filter_map(|d| sites.get(d).cloned())
                .filter(|c| seen.insert(c.id))
                .collect()
        };
        if confs.is_empty() {
            return Ok(0);
        }
        let salts = self
            .salts
            .read()
            .map_err(AppError::internal)?
            .ok_or_else(|| AppError::internal("Salz fehlt"))?;
        let ip_str = req.ip.map(|ip| ip.to_string()).unwrap_or_default();
        let location = req.ip.and_then(|ip| self.geo.lookup(ip)).or_else(|| {
            let c = req.country_header.as_deref()?.trim().to_ascii_uppercase();
            (c.len() == 2 && c != "XX" && c != "T1").then(|| crate::geoip::Location {
                country: c,
                ..Default::default()
            })
        });
        let width = value_i64(&ev.screen_width).filter(|w| *w > 0);
        let device = match width {
            Some(w) => device_from_width(w).to_string(),
            None => match ua.form {
                ua::FormFactor::Mobile => "Mobile".into(),
                ua::FormFactor::Tablet => "Tablet".into(),
                ua::FormFactor::Desktop => {
                    if ua.browser.is_empty() && ua.os.is_empty() {
                        String::new()
                    } else {
                        "Desktop".into()
                    }
                }
            },
        };
        let props = clean_props(ev.props.as_ref(), ev.meta.as_ref());
        let revenue = parse_revenue(ev.revenue.as_ref());
        let interactive = !matches!(ev.interactive, Some(Value::Bool(false)));
        let is_engagement = name == "engagement";
        let is_pageview = name == "pageview";

        let mut stored = 0;
        for conf in confs {
            if !conf.allowed_hostnames.is_empty()
                && !conf.allowed_hostnames.iter().any(|re| re.is_match(&hostname))
            {
                continue;
            }
            if conf.blocked_pages.iter().any(|re| re.is_match(&pathname)) {
                continue;
            }
            if let Some(ip) = req.ip
                && conf.blocked_ips.iter().any(|net| net.contains(&ip))
            {
                continue;
            }
            if location
                .as_ref()
                .is_some_and(|l| conf.blocked_countries.contains(&l.country))
            {
                continue;
            }
            let uid = Self::user_id(&salts.current, &conf.domain, &ip_str, &req.user_agent);
            let prev_uid = salts
                .previous
                .map(|s| Self::user_id(&s, &conf.domain, &ip_str, &req.user_agent));

            let props_json = {
                let filtered: BTreeMap<&String, &String> = props
                    .iter()
                    .filter(|(k, _)| conf.allowed_props.as_ref().is_none_or(|a| a.contains(*k)))
                    .collect();
                (!filtered.is_empty()).then(|| serde_json::to_string(&filtered).unwrap_or_default())
            };

            let (session, event) = {
                let mut map = self.sessions.lock().map_err(AppError::internal)?;
                let key_cur = (conf.id, uid);
                let existing_key = if map.get(&key_cur).is_some_and(|s| s.end >= ts - SESSION_TIMEOUT) {
                    Some(key_cur)
                } else {
                    prev_uid
                        .map(|p| (conf.id, p))
                        .filter(|k| map.get(k).is_some_and(|s| s.end >= ts - SESSION_TIMEOUT))
                };
                let session = match existing_key.and_then(|k| map.get_mut(&k)) {
                    Some(s) => {
                        s.end = s.end.max(ts);
                        if !is_engagement {
                            s.events += 1;
                            if is_pageview {
                                s.pageviews += 1;
                                s.exit_page = pathname.clone();
                                s.exit_hostname = hostname.clone();
                            }
                            if s.pageviews > 1 || (!is_pageview && interactive) {
                                s.is_bounce = false;
                            }
                        }
                        s.clone()
                    }
                    None => {
                        if is_engagement {
                            // Engagement ohne laufende Sitzung zaehlt nicht.
                            continue;
                        }
                        let source = {
                            let param = [q("utm_source"), q("source"), q("ref")]
                                .into_iter()
                                .find(|v| !v.is_empty());
                            match param {
                                Some(p) => referrer::source_from_param(&p),
                                None => referrer.as_ref().map(|r| r.source.clone()).unwrap_or_default(),
                            }
                        };
                        let utm_medium = q("utm_medium");
                        let utm_source = q("utm_source");
                        let utm_campaign = q("utm_campaign");
                        let channel = channel::classify(&channel::ChannelInput {
                            source: &source,
                            referrer_host: referrer.as_ref().map(|r| r.host.as_str()).unwrap_or(""),
                            utm_medium: &utm_medium,
                            utm_source: &utm_source,
                            utm_campaign: &utm_campaign,
                            google_click_id: query.contains_key("gclid")
                                || query.contains_key("gbraid")
                                || query.contains_key("wbraid"),
                            microsoft_click_id: query.contains_key("msclkid"),
                        })
                        .to_string();
                        let loc = location.clone().unwrap_or_default();
                        let attrs = VisitAttrs {
                            referrer: referrer.as_ref().map(|r| r.url.clone()).unwrap_or_default(),
                            source,
                            channel,
                            utm_medium,
                            utm_source,
                            utm_campaign,
                            utm_content: q("utm_content"),
                            utm_term: q("utm_term"),
                            country: loc.country,
                            region: loc.region,
                            city: loc.city,
                            device: device.clone(),
                            browser: ua.browser.clone(),
                            browser_version: ua.browser_version.clone(),
                            os: ua.os.clone(),
                            os_version: ua.os_version.clone(),
                        };
                        let s = Session {
                            id: (rand::random::<i64>() & i64::MAX).max(1),
                            site_id: conf.id,
                            user_id: uid,
                            start: ts,
                            end: ts,
                            entry_page: pathname.clone(),
                            exit_page: pathname.clone(),
                            hostname: hostname.clone(),
                            exit_hostname: hostname.clone(),
                            pageviews: i64::from(is_pageview),
                            events: 1,
                            is_bounce: is_pageview || !interactive,
                            attrs: Arc::new(attrs),
                        };
                        map.insert(key_cur, s.clone());
                        s
                    }
                };
                let event = EventRow {
                    site_id: conf.id,
                    ts,
                    name: truncate(name, 120),
                    pathname: pathname.clone(),
                    hostname: hostname.clone(),
                    user_id: session.user_id,
                    session_id: session.id,
                    attrs: session.attrs.clone(),
                    props: if is_engagement { None } else { props_json },
                    revenue: if is_engagement {
                        None
                    } else {
                        revenue.as_ref().map(|r| r.0)
                    },
                    currency: if is_engagement {
                        None
                    } else {
                        revenue.as_ref().map(|r| r.1.clone())
                    },
                    engagement_ms: is_engagement.then(|| {
                        value_i64(&ev.engagement_time)
                            .unwrap_or(0)
                            .clamp(0, SESSION_TIMEOUT * 1000)
                    }),
                    scroll_depth: is_engagement
                        .then(|| value_i64(&ev.scroll_depth).unwrap_or(0).clamp(0, 100)),
                };
                (session, event)
            };
            if let Some(loc) = &location
                && !loc.region.is_empty()
            {
                let new = self
                    .known_regions
                    .lock()
                    .map(|mut k| k.insert(loc.region.clone()))
                    .unwrap_or(false);
                if new {
                    let _ = self
                        .tx
                        .try_send(WriteOp::GeoName(loc.region.clone(), loc.region_name.clone()));
                }
            }
            if self.tx.try_send(WriteOp::Event(Box::new(event))).is_err()
                || self.tx.try_send(WriteOp::Session(Box::new(session))).is_err()
            {
                tracing::warn!("Schreibpuffer voll – Ereignis verworfen");
                continue;
            }
            stored += 1;
        }
        Ok(stored)
    }
}

/// Schreib-Task: sammelt Ereignisse und Sitzungen und schreibt sie gebuendelt (jede Sekunde oder
/// alle 2000 Eintraege) in einer Transaktion.
pub async fn writer(db: Db, mut rx: mpsc::Receiver<WriteOp>) {
    let mut events: Vec<EventRow> = Vec::new();
    let mut sessions: HashMap<i64, Session> = HashMap::new();
    let mut geo: Vec<(String, String)> = Vec::new();
    let mut waiters: Vec<oneshot::Sender<()>> = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(1000));
    loop {
        let mut closed = false;
        tokio::select! {
            op = rx.recv() => match op {
                Some(WriteOp::Event(e)) => events.push(*e),
                Some(WriteOp::Session(s)) => { sessions.insert(s.id, *s); }
                Some(WriteOp::GeoName(code, name)) => geo.push((code, name)),
                Some(WriteOp::Flush(w)) => waiters.push(w),
                None => closed = true,
            },
            _ = tick.tick() => {}
        }
        let due = !waiters.is_empty() || events.len() >= 2000 || closed || rx.is_empty();
        if due && (!events.is_empty() || !sessions.is_empty() || !geo.is_empty()) {
            let batch_e = std::mem::take(&mut events);
            let batch_s: Vec<Session> = std::mem::take(&mut sessions).into_values().collect();
            let batch_g = std::mem::take(&mut geo);
            let n = batch_e.len();
            if let Err(e) = db
                .write(move |c| write_batch(c, &batch_e, &batch_s, &batch_g))
                .await
            {
                tracing::error!("Schreiben von {n} Ereignissen fehlgeschlagen: {e}");
            }
        }
        for w in waiters.drain(..) {
            let _ = w.send(());
        }
        if closed {
            break;
        }
    }
}

fn write_batch(
    c: &mut rusqlite::Connection,
    events: &[EventRow],
    sessions: &[Session],
    geo: &[(String, String)],
) -> AppResult<()> {
    let tx = c.transaction()?;
    {
        let mut ins = tx.prepare_cached(
            "INSERT INTO events (site_id, ts, name, pathname, hostname, user_id, session_id, referrer, source, channel, \
             utm_medium, utm_source, utm_campaign, utm_content, utm_term, country, region, city, device, browser, \
             browser_version, os, os_version, props, revenue, currency, engagement_ms, scroll_depth) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, \
             ?23, ?24, ?25, ?26, ?27, ?28)",
        )?;
        for e in events {
            let a = &e.attrs;
            ins.execute(rusqlite::params![
                e.site_id,
                e.ts,
                e.name,
                e.pathname,
                e.hostname,
                e.user_id,
                e.session_id,
                a.referrer,
                a.source,
                a.channel,
                a.utm_medium,
                a.utm_source,
                a.utm_campaign,
                a.utm_content,
                a.utm_term,
                a.country,
                a.region,
                a.city,
                a.device,
                a.browser,
                a.browser_version,
                a.os,
                a.os_version,
                e.props,
                e.revenue,
                e.currency,
                e.engagement_ms,
                e.scroll_depth
            ])?;
        }
        let mut up = tx.prepare_cached(
            "INSERT INTO sessions (id, site_id, user_id, start, end, entry_page, exit_page, hostname, exit_hostname, \
             pageviews, events, is_bounce, referrer, source, channel, utm_medium, utm_source, utm_campaign, utm_content, \
             utm_term, country, region, city, device, browser, browser_version, os, os_version) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, \
             ?23, ?24, ?25, ?26, ?27, ?28) \
             ON CONFLICT(id) DO UPDATE SET end = excluded.end, exit_page = excluded.exit_page, \
             exit_hostname = excluded.exit_hostname, pageviews = excluded.pageviews, events = excluded.events, \
             is_bounce = excluded.is_bounce",
        )?;
        for s in sessions {
            let a = &s.attrs;
            up.execute(rusqlite::params![
                s.id,
                s.site_id,
                s.user_id,
                s.start,
                s.end,
                s.entry_page,
                s.exit_page,
                s.hostname,
                s.exit_hostname,
                s.pageviews,
                s.events,
                i64::from(s.is_bounce),
                a.referrer,
                a.source,
                a.channel,
                a.utm_medium,
                a.utm_source,
                a.utm_campaign,
                a.utm_content,
                a.utm_term,
                a.country,
                a.region,
                a.city,
                a.device,
                a.browser,
                a.browser_version,
                a.os,
                a.os_version
            ])?;
        }
        let mut g = tx.prepare_cached(
            "INSERT INTO geo_names (code, name) VALUES (?1, ?2) ON CONFLICT(code) DO UPDATE SET name = excluded.name",
        )?;
        for (code, name) in geo {
            if !name.is_empty() {
                g.execute(rusqlite::params![code, name])?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(normalize_domain(" WWW.Example.COM. "), "example.com");
        assert_eq!(device_from_width(400), "Mobile");
        assert_eq!(device_from_width(800), "Tablet");
        assert_eq!(device_from_width(1280), "Laptop");
        assert_eq!(device_from_width(1920), "Desktop");
        assert_eq!(percent_decode("/caf%C3%A9%20bar"), "/café bar");
        assert_eq!(percent_decode("/50%"), "/50%");
        let p = clean_props(
            Some(&serde_json::json!({"author": "Nico", "n": 3, "x": null, "": "y"})),
            None,
        );
        assert_eq!(p.len(), 2);
        assert_eq!(p.get("n").map(String::as_str), Some("3"));
        assert_eq!(
            parse_revenue(Some(&serde_json::json!({"amount": "12.50", "currency": "chf"}))),
            Some((12.5, "CHF".into()))
        );
        assert!(host_regex("*.example.com").unwrap().is_match("blog.example.com"));
    }
}
