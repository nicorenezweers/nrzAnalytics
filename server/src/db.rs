//! SQLite: Schema, Verbindungen und eigene SQL-Funktionen.
//!
//! - Eine Schreib-Verbindung (Ereignisse kommen gebuendelt vom Schreib-Task, Einstellungen direkt).
//! - Mehrere Lese-Verbindungen fuer Auswertungen (WAL, parallel).
//! - Eigene Funktionen: `regexp(muster, text)` (fuer `REGEXP`) und `nrz_bucket(ts, grenzen)` (Zeitscheibe
//!   per Binaersuche in einer Liste von Zeitstempeln).

use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OpenFlags, functions::FunctionFlags};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;

/// Aktuelle Zeit in Unix-Sekunden.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

const SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY,
  email TEXT NOT NULL UNIQUE COLLATE NOCASE,
  name TEXT NOT NULL DEFAULT '',
  password_hash TEXT NOT NULL,
  is_admin INTEGER NOT NULL DEFAULT 0,
  totp_secret TEXT,
  totp_pending TEXT,
  recovery_codes TEXT NOT NULL DEFAULT '[]',
  failed_logins INTEGER NOT NULL DEFAULT 0,
  locked_until INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  last_login_at INTEGER
);

CREATE TABLE IF NOT EXISTS auth_sessions (
  token_hash BLOB PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS auth_sessions_user ON auth_sessions(user_id);

CREATE TABLE IF NOT EXISTS api_keys (
  id INTEGER PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  prefix TEXT NOT NULL,
  key_hash BLOB NOT NULL UNIQUE,
  scope TEXT NOT NULL DEFAULT 'stats',
  created_at INTEGER NOT NULL,
  last_used_at INTEGER
);

CREATE TABLE IF NOT EXISTS sites (
  id INTEGER PRIMARY KEY,
  domain TEXT NOT NULL UNIQUE,
  timezone TEXT NOT NULL DEFAULT 'Europe/Zurich',
  public INTEGER NOT NULL DEFAULT 0,
  allowed_props TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS memberships (
  site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (site_id, user_id)
);
CREATE INDEX IF NOT EXISTS memberships_user ON memberships(user_id);

CREATE TABLE IF NOT EXISTS goals (
  id INTEGER PRIMARY KEY,
  site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  event_name TEXT,
  page_path TEXT,
  display_name TEXT NOT NULL,
  currency TEXT,
  created_at INTEGER NOT NULL,
  UNIQUE (site_id, display_name)
);

CREATE TABLE IF NOT EXISTS funnels (
  id INTEGER PRIMARY KEY,
  site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS funnel_steps (
  funnel_id INTEGER NOT NULL REFERENCES funnels(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  goal_id INTEGER NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
  PRIMARY KEY (funnel_id, position)
);

CREATE TABLE IF NOT EXISTS shared_links (
  id INTEGER PRIMARY KEY,
  site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  slug TEXT NOT NULL UNIQUE,
  password_hash TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS shields (
  id INTEGER PRIMARY KEY,
  site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  value TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  UNIQUE (site_id, kind, value)
);

CREATE TABLE IF NOT EXISTS salts (
  id INTEGER PRIMARY KEY,
  salt BLOB NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY,
  site_id INTEGER NOT NULL,
  ts INTEGER NOT NULL,
  name TEXT NOT NULL,
  pathname TEXT NOT NULL,
  hostname TEXT NOT NULL,
  user_id INTEGER NOT NULL,
  session_id INTEGER NOT NULL,
  referrer TEXT NOT NULL DEFAULT '',
  source TEXT NOT NULL DEFAULT '',
  channel TEXT NOT NULL DEFAULT '',
  utm_medium TEXT NOT NULL DEFAULT '',
  utm_source TEXT NOT NULL DEFAULT '',
  utm_campaign TEXT NOT NULL DEFAULT '',
  utm_content TEXT NOT NULL DEFAULT '',
  utm_term TEXT NOT NULL DEFAULT '',
  country TEXT NOT NULL DEFAULT '',
  region TEXT NOT NULL DEFAULT '',
  city TEXT NOT NULL DEFAULT '',
  device TEXT NOT NULL DEFAULT '',
  browser TEXT NOT NULL DEFAULT '',
  browser_version TEXT NOT NULL DEFAULT '',
  os TEXT NOT NULL DEFAULT '',
  os_version TEXT NOT NULL DEFAULT '',
  props TEXT,
  revenue REAL,
  currency TEXT,
  engagement_ms INTEGER,
  scroll_depth INTEGER
);
CREATE INDEX IF NOT EXISTS events_site_ts ON events(site_id, ts);
CREATE INDEX IF NOT EXISTS events_site_name_ts ON events(site_id, name, ts);
CREATE INDEX IF NOT EXISTS events_session ON events(session_id);

CREATE TABLE IF NOT EXISTS sessions (
  id INTEGER PRIMARY KEY,
  site_id INTEGER NOT NULL,
  user_id INTEGER NOT NULL,
  start INTEGER NOT NULL,
  end INTEGER NOT NULL,
  entry_page TEXT NOT NULL,
  exit_page TEXT NOT NULL,
  hostname TEXT NOT NULL,
  exit_hostname TEXT NOT NULL,
  pageviews INTEGER NOT NULL DEFAULT 0,
  events INTEGER NOT NULL DEFAULT 0,
  is_bounce INTEGER NOT NULL DEFAULT 1,
  referrer TEXT NOT NULL DEFAULT '',
  source TEXT NOT NULL DEFAULT '',
  channel TEXT NOT NULL DEFAULT '',
  utm_medium TEXT NOT NULL DEFAULT '',
  utm_source TEXT NOT NULL DEFAULT '',
  utm_campaign TEXT NOT NULL DEFAULT '',
  utm_content TEXT NOT NULL DEFAULT '',
  utm_term TEXT NOT NULL DEFAULT '',
  country TEXT NOT NULL DEFAULT '',
  region TEXT NOT NULL DEFAULT '',
  city TEXT NOT NULL DEFAULT '',
  device TEXT NOT NULL DEFAULT '',
  browser TEXT NOT NULL DEFAULT '',
  browser_version TEXT NOT NULL DEFAULT '',
  os TEXT NOT NULL DEFAULT '',
  os_version TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS sessions_site_start ON sessions(site_id, start);
CREATE INDEX IF NOT EXISTS sessions_site_end ON sessions(site_id, end);

CREATE TABLE IF NOT EXISTS geo_names (
  code TEXT PRIMARY KEY,
  name TEXT NOT NULL
);
"#;

/// Verbindungs-Pool: eine Schreib- und mehrere Lese-Verbindungen.
#[derive(Clone)]
pub struct Db {
    writer: Arc<Mutex<Connection>>,
    readers: Arc<Mutex<Vec<Connection>>>,
    permits: Arc<Semaphore>,
}

fn configure(c: &Connection, reader: bool) -> rusqlite::Result<()> {
    c.busy_timeout(std::time::Duration::from_secs(10))?;
    c.execute_batch(
        "PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL; PRAGMA temp_store = MEMORY; \
         PRAGMA mmap_size = 268435456;",
    )?;
    if reader {
        c.execute_batch("PRAGMA cache_size = -32000; PRAGMA query_only = ON;")?;
    } else {
        c.execute_batch("PRAGMA cache_size = -16000;")?;
    }
    register_functions(c)
}

/// `regexp(muster, text)` und `nrz_bucket(ts, grenzen)`.
pub fn register_functions(c: &Connection) -> rusqlite::Result<()> {
    c.create_scalar_function(
        "regexp",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let re = ctx.get_or_create_aux(
                0,
                |v| -> Result<regex::Regex, Box<dyn std::error::Error + Send + Sync>> {
                    let pattern = v.as_str()?;
                    Ok(regex::RegexBuilder::new(pattern).size_limit(1 << 20).build()?)
                },
            )?;
            let text = match ctx.get_raw(1) {
                rusqlite::types::ValueRef::Text(t) => std::str::from_utf8(t).unwrap_or(""),
                rusqlite::types::ValueRef::Null => return Ok(false),
                _ => return Ok(false),
            };
            Ok(re.is_match(text))
        },
    )?;
    c.create_scalar_function(
        "nrz_bucket",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let bounds = ctx.get_or_create_aux(
                1,
                |v| -> Result<Vec<i64>, Box<dyn std::error::Error + Send + Sync>> {
                    let blob = v.as_blob()?;
                    Ok(blob
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                        .collect())
                },
            )?;
            let ts: i64 = ctx.get(0)?;
            // Index der letzten Grenze <= ts, -1 davor.
            let idx = bounds.partition_point(|b| *b <= ts) as i64 - 1;
            Ok(idx)
        },
    )?;
    Ok(())
}

/// Zeitgrenzen als Blob fuer `nrz_bucket`.
pub fn bounds_blob(bounds: &[i64]) -> Vec<u8> {
    bounds.iter().flat_map(|b| b.to_le_bytes()).collect()
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let writer = Connection::open(path)?;
        writer.execute_batch("PRAGMA journal_mode = WAL;")?;
        configure(&writer, false)?;
        migrate(&writer)?;
        let n = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 8);
        let mut readers = Vec::with_capacity(n);
        for _ in 0..n {
            let c = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX
                    | OpenFlags::SQLITE_OPEN_URI,
            )?;
            configure(&c, true)?;
            readers.push(c);
        }
        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            readers: Arc::new(Mutex::new(readers)),
            permits: Arc::new(Semaphore::new(n)),
        })
    }

    /// Nur fuer Tests: Datenbank im Speicher (eine geteilte Verbindung fuer alles).
    #[cfg(test)]
    pub fn memory() -> Self {
        let name = format!(
            "file:nrzanalytics-test-{}?mode=memory&cache=shared",
            rand::random::<u64>()
        );
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let writer = Connection::open_with_flags(&name, flags).expect("Test-DB");
        register_functions(&writer).expect("Funktionen");
        writer.execute_batch("PRAGMA foreign_keys = ON;").expect("pragma");
        migrate(&writer).expect("Schema");
        let mut readers = Vec::new();
        for _ in 0..2 {
            let c = Connection::open_with_flags(&name, flags).expect("Test-DB");
            register_functions(&c).expect("Funktionen");
            readers.push(c);
        }
        Self {
            writer: Arc::new(Mutex::new(writer)),
            readers: Arc::new(Mutex::new(readers)),
            permits: Arc::new(Semaphore::new(2)),
        }
    }

    /// Lesende Arbeit auf einer Lese-Verbindung (blockierend in einem eigenen Thread).
    pub async fn read<T, F>(&self, f: F) -> AppResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> AppResult<T> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(AppError::internal)?;
        let readers = self.readers.clone();
        tokio::task::spawn_blocking(move || {
            let conn = readers.lock().map_err(AppError::internal)?.pop();
            let Some(conn) = conn else {
                return Err(AppError::internal("keine Lese-Verbindung frei"));
            };
            let result = f(&conn);
            if let Ok(mut list) = readers.lock() {
                list.push(conn);
            }
            drop(permit);
            result
        })
        .await?
    }

    /// Schreibende Arbeit auf der Schreib-Verbindung.
    pub async fn write<T, F>(&self, f: F) -> AppResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> AppResult<T> + Send + 'static,
    {
        let writer = self.writer.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = writer.lock().map_err(AppError::internal)?;
            f(&mut conn)
        })
        .await?
    }

    /// Synchron schreiben (nur im Schreib-Task und beim Start).
    pub fn write_blocking<T>(&self, f: impl FnOnce(&mut Connection) -> AppResult<T>) -> AppResult<T> {
        let mut conn = self.writer.lock().map_err(AppError::internal)?;
        f(&mut conn)
    }
}

fn migrate(c: &Connection) -> rusqlite::Result<()> {
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        c.execute_batch(SCHEMA_V1)?;
    }
    if version < SCHEMA_VERSION {
        c.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
    }
    Ok(())
}

/// Einstellung lesen.
pub fn setting(c: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    match c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)) {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Einstellung schreiben.
pub fn set_setting(c: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_functions() {
        let c = Connection::open_in_memory().unwrap();
        register_functions(&c).unwrap();
        let m: bool = c
            .query_row("SELECT '/blog/a' REGEXP '^/blog/[^/]*$'", [], |r| r.get(0))
            .unwrap();
        assert!(m);
        let blob = bounds_blob(&[100, 200, 300]);
        let idx: Vec<i64> = [50_i64, 100, 150, 299, 300, 999]
            .iter()
            .map(|ts| {
                c.query_row("SELECT nrz_bucket(?1, ?2)", rusqlite::params![ts, blob], |r| {
                    r.get(0)
                })
                .unwrap()
            })
            .collect();
        assert_eq!(idx, vec![-1, 0, 0, 1, 2, 2]);
    }
}
