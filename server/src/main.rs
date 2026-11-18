//! nrzAnalytics – Server.
//!
//! Ein Programm: Erfassung (`/api/event`, Tracking-Skript `/js/script.js`), JSON-API der Oberflaeche,
//! Stats-/Sites-API wie Plausible, MCP-Server und die Weboberflaeche (Dioxus/WASM, eingebettet).
//! Daten in SQLite. Konfiguration per Umgebungsvariablen (README).

mod api;
mod auth;
mod config;
mod db;
mod demo;
mod error;
mod geoip;
mod ingest;
mod mcp;
mod security;
mod stats;
mod tracker;
mod web;

#[cfg(test)]
mod tests;

use axum::{Router, extract::DefaultBodyLimit, middleware};
use config::Config;
use db::Db;
use geoip::GeoIp;
use ingest::Ingest;
use security::RateLimiter;
use std::{
    io::{Read, Write},
    net::SocketAddr,
    process::ExitCode,
    sync::{Arc, RwLock},
    time::Duration,
};
use tower_http::compression::CompressionLayer;

/// Einstellungen der Instanz, die sich zur Laufzeit aendern lassen.
#[derive(Clone, Debug, Default)]
pub struct Instance {
    pub registration_open: bool,
    pub public_url: String,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Db,
    pub ingest: Arc<Ingest>,
    pub geo: Arc<GeoIp>,
    pub limiter: Arc<RateLimiter>,
    /// Server-Geheimnis (Cookies geteilter Links).
    pub secret: Arc<Vec<u8>>,
    pub instance: Arc<RwLock<Instance>>,
}

pub fn app(state: AppState, web: Option<Arc<web::WebUi>>) -> Router {
    let mut router = api::router();
    router = match web {
        Some(ui) => router.fallback(move |method, uri| ui.clone().serve(method, uri)),
        None => router.fallback(|| async { error::AppError::not_found() }),
    };
    router
        .layer(middleware::from_fn_with_state(state.clone(), security::guard))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(CompressionLayer::new())
        .with_state(state)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => serve(),
        Some("healthcheck") => healthcheck(),
        Some("version") | Some("--version") | Some("-V") => {
            println!("nrzAnalytics {}", nrzanalytics_shared::VERSION);
            ExitCode::SUCCESS
        }
        Some("reset-password") => reset_password(&args[1..]),
        Some("demo-data") => demo_data(&args[1..]),
        Some("help") | Some("--help") | Some("-h") => {
            println!(
                "nrzAnalytics {} – datenschutzfreundliche Webanalyse\n\n\
                 nrzanalytics                         Server starten\n\
                 nrzanalytics healthcheck             Pruefen, ob der Server antwortet (Exit 0/1)\n\
                 nrzanalytics reset-password <login> <passwort>   Passwort setzen (2FA bleibt)\n\
                 nrzanalytics demo-data <domain> [tage]  Beispieldaten erzeugen (Entwicklung)\n\
                 nrzanalytics version                 Version ausgeben\n\n\
                 Konfiguration per Umgebungsvariablen, siehe README.",
                nrzanalytics_shared::VERSION
            );
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("Unbekannter Befehl '{other}' – nrzanalytics help");
            ExitCode::FAILURE
        }
    }
}

/// `GET /api/health` auf dem eigenen Port (fuer den Docker-Healthcheck, ohne curl im Image).
fn healthcheck() -> ExitCode {
    let config = Config::from_env();
    let mut addr = config.bind;
    if addr.ip().is_unspecified() {
        addr.set_ip(std::net::Ipv4Addr::LOCALHOST.into());
    }
    let ok = (|| -> std::io::Result<bool> {
        let mut s = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        s.set_read_timeout(Some(Duration::from_secs(3)))?;
        s.write_all(b"GET /api/health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut buf = String::new();
        s.read_to_string(&mut buf)?;
        Ok(buf.starts_with("HTTP/1.1 200") || buf.starts_with("HTTP/1.0 200"))
    })()
    .unwrap_or(false);
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn demo_data(args: &[String]) -> ExitCode {
    let Some(domain) = args.first() else {
        eprintln!("nrzanalytics demo-data <domain> [tage]");
        return ExitCode::FAILURE;
    };
    let days: i64 = args
        .get(1)
        .and_then(|d| d.parse().ok())
        .unwrap_or(60)
        .clamp(1, 730);
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread().enable_all().build() else {
        return ExitCode::FAILURE;
    };
    match runtime.block_on(demo::run(domain, days)) {
        Ok(n) => {
            println!("{n} Ereignisse für {domain} über {days} Tage erzeugt.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Fehler: {e}");
            ExitCode::FAILURE
        }
    }
}

fn reset_password(args: &[String]) -> ExitCode {
    let (Some(login), Some(password)) = (args.first(), args.get(1)) else {
        eprintln!("nrzanalytics reset-password <login> <passwort>");
        return ExitCode::FAILURE;
    };
    let config = Config::from_env();
    let result = (|| -> Result<(), String> {
        auth::check_password(password).map_err(|e| e.message)?;
        let db = Db::open(&config.database).map_err(|e| e.to_string())?;
        db.write_blocking(|c| {
            let n = c.execute(
                "UPDATE users SET password_hash = ?1, failed_logins = 0, locked_until = 0 WHERE email = ?2",
                rusqlite::params![auth::hash_password(password)?, login],
            )?;
            c.execute(
                "DELETE FROM auth_sessions WHERE user_id IN (SELECT id FROM users WHERE email = ?1)",
                [login],
            )?;
            if n == 0 {
                Err(error::AppError::not_found())
            } else {
                Ok(())
            }
        })
        .map_err(|e| e.message)
    })();
    match result {
        Ok(()) => {
            println!("Passwort für '{login}' gesetzt, alle Geräte abgemeldet.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Fehler: {e}");
            ExitCode::FAILURE
        }
    }
}

fn serve() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,nrzanalytics=info".into()),
        )
        .init();
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Tokio: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::from_env();
    let db = Db::open(&config.database)?;

    // Geheimnis, Admin-Konto, Instanz-Einstellungen
    let (secret, instance) = db.write_blocking(|c| {
        let secret = match &config.secret {
            Some(s) => auth::sha256(format!("nrzanalytics:{s}").as_bytes()),
            None => match db::setting(c, "secret")? {
                Some(s) => auth::sha256(s.as_bytes()),
                None => {
                    let s = auth::random_token(32);
                    db::set_setting(c, "secret", &s)?;
                    auth::sha256(s.as_bytes())
                }
            },
        };
        if let (Some(email), Some(password)) = (&config.admin_email, &config.admin_password) {
            auth::ensure_admin(c, email, password)?;
        }
        let instance = Instance {
            registration_open: db::setting(c, "registration_open")?.as_deref() == Some("1"),
            public_url: db::setting(c, "public_url")?.unwrap_or_default(),
        };
        c.execute("DELETE FROM auth_sessions WHERE expires_at < ?1", [db::now()])?;
        Ok((secret, instance))
    })?;

    let geo = GeoIp::new(config.geoip.clone(), &config.data_dir);
    let (ingest, rx) = Ingest::new(db.clone(), geo.clone());
    ingest.reload_sites().await?;
    ingest.rotate_salts().await?;
    ingest.load_sessions().await?;
    let writer = tokio::spawn(ingest::writer(db.clone(), rx));
    tokio::spawn(geo.clone().maintain());

    let state = AppState {
        config: Arc::new(config.clone()),
        db: db.clone(),
        ingest: ingest.clone(),
        geo,
        limiter: Arc::new(RateLimiter::default()),
        secret: Arc::new(secret),
        instance: Arc::new(RwLock::new(instance)),
    };

    // Hintergrund: Salz-Wechsel, Sitzungs-Cache, Rate-Limits, Aufbewahrung.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            let mut minutes: u64 = 0;
            loop {
                tick.tick().await;
                minutes += 1;
                if let Err(e) = state.ingest.rotate_salts().await {
                    tracing::warn!("Salz: {e}");
                }
                state.ingest.prune_sessions();
                if minutes.is_multiple_of(10) {
                    state.limiter.prune();
                    let _ = state
                        .db
                        .write(|c| {
                            Ok(c.execute("DELETE FROM auth_sessions WHERE expires_at < ?1", [db::now()])?)
                        })
                        .await;
                }
                if minutes % 60 == 1 && state.config.retention_days > 0 {
                    let cutoff = db::now() - i64::from(state.config.retention_days) * 86_400;
                    let r = state
                        .db
                        .write(move |c| {
                            let e = c.execute("DELETE FROM events WHERE ts < ?1", [cutoff])?;
                            c.execute("DELETE FROM sessions WHERE end < ?1", [cutoff])?;
                            Ok(e)
                        })
                        .await;
                    if let Ok(n) = r
                        && n > 0
                    {
                        tracing::info!("Aufbewahrung: {n} alte Ereignisse geloescht");
                    }
                }
            }
        });
    }

    let web = web::WebUi::load(config.web_dir.as_deref());
    if web.is_none() {
        tracing::warn!(
            "Keine Weboberflaeche gefunden (NRZANALYTICS_WEB_DIR oder eingebettet) – nur API aktiv"
        );
    }

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    if let Some(public) = config.public_bind {
        let router = app(state.clone(), web.clone()).layer(middleware::from_fn(security::public_only));
        let listener = tokio::net::TcpListener::bind(public).await?;
        tracing::info!(
            "Oeffentlicher Port (Skript, Erfassung, geteilte Dashboards, Stats-API): http://{public}"
        );
        let mut rx = stop_rx.clone();
        tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = rx.changed().await;
            })
            .await;
        });
    }

    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(
        "nrzAnalytics {} laeuft auf http://{}",
        nrzanalytics_shared::VERSION,
        config.bind
    );
    axum::serve(
        listener,
        app(state.clone(), web).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    let _ = stop_tx.send(true);
    // Gepufferte Ereignisse noch schreiben.
    state.ingest.flush().await;
    drop(state);
    writer.abort();
    tracing::info!("Beendet");
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
