//! Konten, Sitzungen, Zwei-Faktor (TOTP), API-Schluessel und Zugriff auf Websites.

use crate::{
    AppState,
    db::now,
    error::{AppError, AppResult},
};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, header, request::Parts},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use nrzanalytics_shared::api::{ApiScope, Me, Role, SESSION_COOKIE, SHARE_HEADER};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

/// Sitzung bleibt 30 Tage gueltig (gleitend).
pub const SESSION_SECONDS: i64 = 30 * 86_400;
/// Nach so vielen Fehlversuchen wird ein Konto 15 Minuten gesperrt.
pub const MAX_FAILED_LOGINS: i64 = 8;

pub fn sha256(data: &[u8]) -> Vec<u8> {
    Sha256::digest(data).to_vec()
}

pub fn random_token(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(AppError::internal)
}

/// Echter Argon2-Hash fuer unbekannte Konten (gleich lange Antwortzeit).
pub static DUMMY_HASH: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| hash_password("nrzanalytics-dummy-passwort").unwrap_or_default());

pub fn verify_password(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

/// Passwortregeln: 10 bis 200 Zeichen.
pub fn check_password(password: &str) -> AppResult<()> {
    let n = password.chars().count();
    if n < 10 {
        return Err(AppError::invalid("Das Passwort braucht mindestens 10 Zeichen."));
    }
    if n > 200 {
        return Err(AppError::invalid("Das Passwort ist zu lang."));
    }
    Ok(())
}

/// E-Mail oder Benutzername: 3 bis 120 Zeichen, keine Leerzeichen.
pub fn check_login_name(email: &str) -> AppResult<String> {
    let e = email.trim().to_string();
    if e.chars().count() < 3 || e.len() > 120 || e.chars().any(char::is_whitespace) {
        return Err(AppError::invalid(
            "Bitte eine gültige E-Mail-Adresse oder einen Benutzernamen (3–120 Zeichen) angeben.",
        ));
    }
    Ok(e)
}

#[derive(Clone, Debug)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub is_admin: bool,
    pub totp_enabled: bool,
}

impl User {
    pub fn me(&self) -> Me {
        Me {
            id: self.id,
            email: self.email.clone(),
            name: self.name.clone(),
            is_admin: self.is_admin,
            totp_enabled: self.totp_enabled,
        }
    }
}

const USER_COLS: &str = "id, email, name, is_admin, totp_secret IS NOT NULL";

fn user_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        email: r.get(1)?,
        name: r.get(2)?,
        is_admin: r.get::<_, i64>(3)? != 0,
        totp_enabled: r.get::<_, bool>(4)?,
    })
}

pub fn user_by_id(c: &Connection, id: i64) -> AppResult<Option<User>> {
    Ok(c.query_row(
        &format!("SELECT {USER_COLS} FROM users WHERE id = ?1"),
        [id],
        user_from_row,
    )
    .optional()?)
}

pub fn user_by_email(c: &Connection, email: &str) -> AppResult<Option<User>> {
    Ok(c.query_row(
        &format!("SELECT {USER_COLS} FROM users WHERE email = ?1"),
        [email.trim()],
        user_from_row,
    )
    .optional()?)
}

pub fn create_user(
    c: &Connection,
    email: &str,
    name: &str,
    password: &str,
    is_admin: bool,
) -> AppResult<User> {
    let email = check_login_name(email)?;
    check_password(password)?;
    let hash = hash_password(password)?;
    c.execute(
        "INSERT INTO users (email, name, password_hash, is_admin, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![email, name.trim(), hash, i64::from(is_admin), now()],
    )
    .map_err(|e| match AppError::from(e) {
        err if err.code == "conflict" => AppError::conflict("Dieses Konto gibt es schon."),
        err => err,
    })?;
    user_by_id(c, c.last_insert_rowid())?.ok_or_else(|| AppError::internal("Konto fehlt nach dem Anlegen"))
}

/// Admin-Konto aus der Umgebung: anlegen oder Passwort setzen.
pub fn ensure_admin(c: &Connection, email: &str, password: &str) -> AppResult<()> {
    match user_by_email(c, email)? {
        Some(u) => {
            let hash: String = c.query_row("SELECT password_hash FROM users WHERE id = ?1", [u.id], |r| {
                r.get(0)
            })?;
            if !verify_password(&hash, password) {
                c.execute(
                    "UPDATE users SET password_hash = ?1, is_admin = 1, failed_logins = 0, locked_until = 0 WHERE id = ?2",
                    params![hash_password(password)?, u.id],
                )?;
                c.execute("DELETE FROM auth_sessions WHERE user_id = ?1", [u.id])?;
                tracing::info!("Passwort des Admin-Kontos '{email}' gesetzt");
            } else if !u.is_admin {
                c.execute("UPDATE users SET is_admin = 1 WHERE id = ?1", [u.id])?;
            }
        }
        None => {
            if password.chars().count() < 10 {
                tracing::warn!(
                    "NRZANALYTICS_ADMIN_PASSWORD hat weniger als 10 Zeichen – wird trotzdem gesetzt"
                );
                let hash = hash_password(password)?;
                c.execute(
                    "INSERT INTO users (email, name, password_hash, is_admin, created_at) VALUES (?1, ?2, ?3, 1, ?4)",
                    params![email.trim(), "Admin", hash, now()],
                )?;
            } else {
                create_user(c, email, "Admin", password, true)?;
            }
            tracing::info!("Admin-Konto '{email}' angelegt");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Sitzungen
// ---------------------------------------------------------------------------------------------

pub fn create_session(c: &Connection, user_id: i64) -> AppResult<String> {
    let token = random_token(32);
    let t = now();
    c.execute(
        "INSERT INTO auth_sessions (token_hash, user_id, created_at, expires_at, last_seen_at) VALUES (?1, ?2, ?3, ?4, ?3)",
        params![sha256(token.as_bytes()), user_id, t, t + SESSION_SECONDS],
    )?;
    // Hoechstens 20 Geraete je Konto.
    c.execute(
        "DELETE FROM auth_sessions WHERE user_id = ?1 AND token_hash NOT IN \
         (SELECT token_hash FROM auth_sessions WHERE user_id = ?1 ORDER BY last_seen_at DESC LIMIT 20)",
        [user_id],
    )?;
    c.execute(
        "UPDATE users SET last_login_at = ?1, failed_logins = 0, locked_until = 0 WHERE id = ?2",
        params![t, user_id],
    )?;
    Ok(token)
}

/// Konto zur Sitzung (verlaengert die Sitzung hoechstens einmal pro Stunde).
/// Konto zur Sitzung und ob die Sitzung verlaengert werden soll (aelter als eine Stunde).
pub fn session_user(c: &Connection, token: &str) -> AppResult<Option<(User, bool)>> {
    let hash = sha256(token.as_bytes());
    let row: Option<(i64, i64, i64)> = c
        .query_row(
            "SELECT user_id, expires_at, last_seen_at FROM auth_sessions WHERE token_hash = ?1",
            [&hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((user_id, expires, last_seen)) = row else {
        return Ok(None);
    };
    let t = now();
    if expires < t {
        return Ok(None);
    }
    Ok(user_by_id(c, user_id)?.map(|u| (u, t - last_seen > 3600)))
}

/// Sitzung gleitend verlaengern.
pub fn touch_session(c: &Connection, token: &str) -> AppResult<()> {
    let t = now();
    c.execute(
        "UPDATE auth_sessions SET last_seen_at = ?1, expires_at = ?2 WHERE token_hash = ?3",
        params![t, t + SESSION_SECONDS, sha256(token.as_bytes())],
    )?;
    Ok(())
}

pub fn delete_session(c: &Connection, token: &str) -> AppResult<()> {
    c.execute(
        "DELETE FROM auth_sessions WHERE token_hash = ?1",
        [sha256(token.as_bytes())],
    )?;
    Ok(())
}

/// Cookie-Wert aus den Headern.
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|part| {
            let (k, v) = part.trim().split_once('=')?;
            (k == name).then(|| v.to_string())
        })
}

pub fn session_cookie(token: &str, secure: bool, max_age: i64) -> String {
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
        if secure { "; Secure" } else { "" }
    )
}

// ---------------------------------------------------------------------------------------------
// TOTP (RFC 6238: SHA-1, 30 Sekunden, 6 Ziffern)
// ---------------------------------------------------------------------------------------------

pub fn totp_new_secret() -> String {
    let mut b = [0u8; 20];
    rand::rngs::OsRng.fill_bytes(&mut b);
    data_encoding::BASE32_NOPAD.encode(&b)
}

fn totp_at(secret: &[u8], counter: u64) -> u32 {
    let Ok(mut mac) = Hmac::<sha1::Sha1>::new_from_slice(secret) else {
        return 0;
    };
    mac.update(&counter.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let offset = (h[h.len() - 1] & 0x0f) as usize;
    let bin = ((u32::from(h[offset]) & 0x7f) << 24)
        | (u32::from(h[offset + 1]) << 16)
        | (u32::from(h[offset + 2]) << 8)
        | u32::from(h[offset + 3]);
    bin % 1_000_000
}

/// Aktueller Code (fuer Tests und Werkzeuge).
#[cfg(test)]
pub fn totp_code(secret_b32: &str, at: i64) -> String {
    let secret = data_encoding::BASE32_NOPAD
        .decode(secret_b32.as_bytes())
        .unwrap_or_default();
    format!("{:06}", totp_at(&secret, (at / 30) as u64))
}

/// Prueft einen Code (±1 Zeitschritt Toleranz).
pub fn totp_verify(secret_b32: &str, code: &str, at: i64) -> bool {
    let code: String = code.chars().filter(char::is_ascii_digit).collect();
    if code.len() != 6 {
        return false;
    }
    let Ok(secret) = data_encoding::BASE32_NOPAD.decode(secret_b32.trim().to_ascii_uppercase().as_bytes())
    else {
        return false;
    };
    let Ok(expected) = code.parse::<u32>() else {
        return false;
    };
    let step = at / 30;
    (-1..=1).any(|d| totp_at(&secret, (step + d).max(0) as u64) == expected)
}

pub fn otpauth_url(secret: &str, account: &str) -> String {
    let enc = |s: &str| {
        url::form_urlencoded::byte_serialize(s.as_bytes())
            .collect::<String>()
            .replace('+', "%20")
    };
    format!(
        "otpauth://totp/nrzAnalytics:{}?secret={secret}&issuer=nrzAnalytics&algorithm=SHA1&digits=6&period=30",
        enc(account)
    )
}

/// Zehn Wiederherstellungscodes (`xxxx-xxxx`), gespeichert als SHA-256.
pub fn recovery_codes() -> (Vec<String>, String) {
    let alphabet = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::rngs::OsRng;
    let codes: Vec<String> = (0..10)
        .map(|_| {
            let mut s = String::new();
            for i in 0..8 {
                if i == 4 {
                    s.push('-');
                }
                s.push(alphabet[(rng.next_u32() as usize) % alphabet.len()] as char);
            }
            s
        })
        .collect();
    let hashes: Vec<String> = codes.iter().map(|c| hex(&sha256(c.as_bytes()))).collect();
    (
        codes,
        serde_json::to_string(&hashes).unwrap_or_else(|_| "[]".into()),
    )
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Wiederherstellungscode einloesen (wird danach entfernt).
pub fn use_recovery_code(c: &Connection, user_id: i64, code: &str) -> AppResult<bool> {
    let stored: String = c.query_row("SELECT recovery_codes FROM users WHERE id = ?1", [user_id], |r| {
        r.get(0)
    })?;
    let mut list: Vec<String> = serde_json::from_str(&stored).unwrap_or_default();
    let h = hex(&sha256(code.trim().to_lowercase().as_bytes()));
    let Some(pos) = list.iter().position(|x| *x == h) else {
        return Ok(false);
    };
    list.remove(pos);
    c.execute(
        "UPDATE users SET recovery_codes = ?1 WHERE id = ?2",
        params![
            serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()),
            user_id
        ],
    )?;
    Ok(true)
}

// ---------------------------------------------------------------------------------------------
// API-Schluessel
// ---------------------------------------------------------------------------------------------

pub const API_KEY_PREFIX: &str = "nrzan_";

/// Neuer Schluessel: (vollstaendig, Anfang zur Anzeige, Hash).
pub fn new_api_key() -> (String, String, Vec<u8>) {
    let key = format!("{API_KEY_PREFIX}{}", random_token(30));
    let prefix: String = key.chars().take(API_KEY_PREFIX.len() + 4).collect();
    let hash = sha256(key.as_bytes());
    (key, prefix, hash)
}

/// Konto und Berechtigung zu einem Schluessel.
pub fn api_key_user(c: &Connection, key: &str) -> AppResult<Option<(User, ApiScope)>> {
    let row: Option<(i64, i64, String)> = c
        .query_row(
            "SELECT id, user_id, scope FROM api_keys WHERE key_hash = ?1",
            [sha256(key.trim().as_bytes())],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((key_id, user_id, scope)) = row else {
        return Ok(None);
    };
    let _ = c.execute(
        "UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2",
        params![now(), key_id],
    );
    let scope = if scope == "sites" {
        ApiScope::Sites
    } else {
        ApiScope::Stats
    };
    Ok(user_by_id(c, user_id)?.map(|u| (u, scope)))
}

/// Bearer-Token aus dem Authorization-Header.
pub fn bearer(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = v.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim().to_string())
        .filter(|t| !t.is_empty())
}

// ---------------------------------------------------------------------------------------------
// HMAC fuer Cookies geteilter Links
// ---------------------------------------------------------------------------------------------

pub fn sign(secret: &[u8], message: &str) -> String {
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return String::new();
    };
    mac.update(message.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

/// Traegt die Anfrage das Freigabe-Cookie eines passwortgeschuetzten geteilten Links?
/// (Die Signatur enthaelt den Passwort-Hash: ein neues Passwort macht alte Cookies ungueltig.)
pub fn share_cookie_ok(secret: &[u8], headers: &HeaderMap, link_id: i64, password_hash: &str) -> bool {
    let expected = sign(secret, &format!("share:{link_id}:{password_hash}"));
    !expected.is_empty() && cookie(headers, &share_cookie_name(link_id)).as_deref() == Some(expected.as_str())
}

pub fn share_cookie_name(link_id: i64) -> String {
    format!("nrz_share_{link_id}")
}

// ---------------------------------------------------------------------------------------------
// Extraktoren
// ---------------------------------------------------------------------------------------------

/// Angemeldetes Konto (sonst 401).
pub struct CurrentUser(pub User);

/// Angemeldetes Konto, falls vorhanden.
pub struct MaybeUser(pub Option<User>);

async fn load_user(state: &AppState, headers: &HeaderMap) -> AppResult<Option<User>> {
    let Some(token) = cookie(headers, SESSION_COOKIE) else {
        return Ok(None);
    };
    if token.len() > 100 {
        return Ok(None);
    }
    let t = token.clone();
    let Some((user, touch)) = state.db.read(move |c| session_user(c, &t)).await? else {
        return Ok(None);
    };
    if touch {
        state.db.write(move |c| touch_session(c, &token)).await?;
    }
    Ok(Some(user))
}

impl FromRequestParts<AppState> for MaybeUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(Self(load_user(state, &parts.headers).await?))
    }
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        load_user(state, &parts.headers)
            .await?
            .map(Self)
            .ok_or_else(AppError::unauthorized)
    }
}

// ---------------------------------------------------------------------------------------------
// Zugriff auf Websites
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct SiteRow {
    pub id: i64,
    pub domain: String,
    pub timezone: String,
    pub public: bool,
    pub allowed_props: Option<Vec<String>>,
    pub created_at: i64,
}

pub fn site_by_domain(c: &Connection, domain: &str) -> AppResult<Option<SiteRow>> {
    let d = crate::ingest::normalize_domain(domain);
    Ok(c.query_row(
        "SELECT id, domain, timezone, public, allowed_props, created_at FROM sites WHERE domain = ?1",
        [d],
        |r| {
            Ok(SiteRow {
                id: r.get(0)?,
                domain: r.get(1)?,
                timezone: r.get(2)?,
                public: r.get::<_, i64>(3)? != 0,
                allowed_props: r
                    .get::<_, Option<String>>(4)?
                    .and_then(|a| serde_json::from_str(&a).ok()),
                created_at: r.get(5)?,
            })
        },
    )
    .optional()?)
}

pub fn role_of(c: &Connection, site_id: i64, user: &User) -> AppResult<Option<Role>> {
    if user.is_admin {
        return Ok(Some(Role::Owner));
    }
    let role: Option<String> = c
        .query_row(
            "SELECT role FROM memberships WHERE site_id = ?1 AND user_id = ?2",
            params![site_id, user.id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(role.and_then(|r| Role::parse(&r)))
}

/// Benoetigte Berechtigung.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// Statistiken ansehen (auch oeffentlich / geteilter Link).
    View,
    Edit,
    Admin,
    Owner,
}

#[derive(Clone, Debug)]
pub struct SiteAccess {
    pub site: SiteRow,
    pub role: Option<Role>,
    pub shared: bool,
}

/// Zugriff pruefen: Mitgliedschaft, Instanz-Admin, oeffentliche Website oder geteilter Link.
/// `share` = Kurzname aus Header oder Parameter.
pub fn site_access(
    c: &Connection,
    secret: &[u8],
    headers: &HeaderMap,
    user: Option<&User>,
    domain: &str,
    share: Option<&str>,
    need: Need,
) -> AppResult<SiteAccess> {
    let site = site_by_domain(c, domain)?.ok_or_else(AppError::not_found)?;
    let role = match user {
        Some(u) => role_of(c, site.id, u)?,
        None => None,
    };
    let allowed = |r: Role| match need {
        Need::View => true,
        Need::Edit => r.can_edit(),
        Need::Admin => r.can_admin(),
        Need::Owner => r == Role::Owner,
    };
    if let Some(r) = role {
        return if allowed(r) {
            Ok(SiteAccess {
                site,
                role,
                shared: false,
            })
        } else {
            Err(AppError::forbidden())
        };
    }
    if need != Need::View {
        return Err(if user.is_some() {
            AppError::forbidden()
        } else {
            AppError::unauthorized()
        });
    }
    let share = share.map(str::to_string).or_else(|| {
        headers
            .get(SHARE_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    });
    if let Some(slug) = share {
        let link: Option<(i64, Option<String>)> = c
            .query_row(
                "SELECT id, password_hash FROM shared_links WHERE slug = ?1 AND site_id = ?2",
                params![slug, site.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((link_id, password_hash)) = link else {
            return Err(AppError::not_found());
        };
        if let Some(ph) = password_hash
            && !share_cookie_ok(secret, headers, link_id, &ph)
        {
            return Err(AppError::new(
                axum::http::StatusCode::UNAUTHORIZED,
                "share_password_required",
                "Dieser Link ist mit einem Passwort geschützt.",
            ));
        }
        return Ok(SiteAccess {
            site,
            role: None,
            shared: true,
        });
    }
    if site.public {
        return Ok(SiteAccess {
            site,
            role: None,
            shared: false,
        });
    }
    Err(if user.is_some() {
        AppError::not_found()
    } else {
        AppError::unauthorized()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totp_rfc6238() {
        // RFC 6238 Testvektor (SHA-1): Zeit 59 → 94287082 (8 Ziffern) → 287082 (6 Ziffern).
        let secret = data_encoding::BASE32_NOPAD.encode(b"12345678901234567890");
        assert!(totp_verify(&secret, "287082", 59));
        assert!(!totp_verify(&secret, "287083", 59));
        assert!(totp_verify(&secret, "287 082", 59));
    }

    #[test]
    fn passwords_and_keys() {
        let h = hash_password("geheim-passwort").unwrap();
        assert!(verify_password(&h, "geheim-passwort"));
        assert!(!verify_password(&h, "falsch"));
        let (key, prefix, hash) = new_api_key();
        assert!(key.starts_with(API_KEY_PREFIX) && key.starts_with(&prefix));
        assert_eq!(hash, sha256(key.as_bytes()));
        let (codes, json) = recovery_codes();
        assert_eq!(codes.len(), 10);
        assert!(json.contains(&hex(&sha256(codes[0].as_bytes()))));
    }
}
