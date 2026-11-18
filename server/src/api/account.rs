//! Anmeldung, Registrierung, Konto, Zwei-Faktor und API-Schluessel.

use super::base_url;
use crate::{
    AppState,
    auth::{self, CurrentUser, MaybeUser},
    db::now,
    error::{AppError, AppResult},
    security::client_ip,
};
use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use nrzanalytics_shared::api::{
    ApiKey, ApiScope, ChangePassword, CreatedApiKey, LoginRequest, Me, NewApiKey, RecoveryCodes,
    RegisterRequest, SESSION_COOKIE, Status, TotpDisable, TotpEnable, TotpSetup, UpdateAccount,
};
use rusqlite::{OptionalExtension, params};
use std::{net::SocketAddr, time::Duration};

pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    MaybeUser(user): MaybeUser,
) -> AppResult<Json<Status>> {
    let needs_setup = state
        .db
        .read(|c| {
            Ok(c.query_row("SELECT NOT EXISTS (SELECT 1 FROM users)", [], |r| {
                r.get::<_, bool>(0)
            })?)
        })
        .await?;
    let registration_open = registration_open(&state);
    Ok(Json(Status {
        version: nrzanalytics_shared::VERSION.into(),
        needs_setup,
        registration_open,
        public_url: base_url(&state, &headers),
        user: user.map(|u| u.me()),
    }))
}

fn registration_open(state: &AppState) -> bool {
    state.config.registration.unwrap_or_else(|| {
        state
            .instance
            .read()
            .map(|i| i.registration_open)
            .unwrap_or(false)
    })
}

fn with_session_cookie(state: &AppState, token: &str, body: Me) -> Response {
    let mut res = Json(body).into_response();
    if let Ok(v) = HeaderValue::from_str(&auth::session_cookie(
        token,
        state.config.secure_cookies,
        auth::SESSION_SECONDS,
    )) {
        res.headers_mut().insert(header::SET_COOKIE, v);
    }
    res
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> AppResult<Response> {
    let ip = client_ip(&headers, Some(peer), state.config.trust_proxy)
        .map(|i| i.to_string())
        .unwrap_or_default();
    if !state
        .limiter
        .check(&format!("login:{ip}"), 20, Duration::from_secs(600))
    {
        return Err(AppError::rate_limited());
    }
    let email = req.email.trim().to_string();
    let password = req.password.clone();
    let totp = req.totp.clone();
    let result = state
        .db
        .write(move |c| {
            let row: Option<(i64, String, Option<String>, i64, i64)> = c
                .query_row(
                    "SELECT id, password_hash, totp_secret, failed_logins, locked_until FROM users WHERE email = ?1",
                    [&email],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional()?;
            let Some((id, hash, totp_secret, failed, locked_until)) = row else {
                // Gleich lange rechnen wie bei einem echten Konto.
                let _ = auth::verify_password(&auth::DUMMY_HASH, &password);
                return Err(AppError::new(StatusCode::UNAUTHORIZED, "unauthorized", "E-Mail/Benutzername oder Passwort falsch."));
            };
            if locked_until > now() {
                return Err(AppError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limited",
                    "Zu viele Fehlversuche – das Konto ist für 15 Minuten gesperrt.",
                ));
            }
            if !auth::verify_password(&hash, &password) {
                let failed = failed + 1;
                let lock = if failed >= auth::MAX_FAILED_LOGINS { now() + 900 } else { 0 };
                c.execute(
                    "UPDATE users SET failed_logins = ?1, locked_until = ?2 WHERE id = ?3",
                    params![if lock > 0 { 0 } else { failed }, lock, id],
                )?;
                return Err(AppError::new(StatusCode::UNAUTHORIZED, "unauthorized", "E-Mail/Benutzername oder Passwort falsch."));
            }
            if let Some(secret) = totp_secret {
                let code = totp.unwrap_or_default();
                if code.trim().is_empty() {
                    return Err(AppError::new(StatusCode::UNAUTHORIZED, "totp_required", "Bitte den Code aus der Authenticator-App eingeben."));
                }
                let ok = auth::totp_verify(&secret, &code, now()) || auth::use_recovery_code(c, id, &code)?;
                if !ok {
                    let failed = failed + 1;
                    let lock = if failed >= auth::MAX_FAILED_LOGINS { now() + 900 } else { 0 };
                    c.execute(
                        "UPDATE users SET failed_logins = ?1, locked_until = ?2 WHERE id = ?3",
                        params![if lock > 0 { 0 } else { failed }, lock, id],
                    )?;
                    return Err(AppError::new(StatusCode::UNAUTHORIZED, "totp_required", "Der Code stimmt nicht."));
                }
            }
            let token = auth::create_session(c, id)?;
            let user = auth::user_by_id(c, id)?.ok_or_else(AppError::unauthorized)?;
            Ok((token, user))
        })
        .await;
    let (token, user) = result?;
    Ok(with_session_cookie(&state, &token, user.me()))
}

pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> AppResult<Response> {
    let ip = client_ip(&headers, Some(peer), state.config.trust_proxy)
        .map(|i| i.to_string())
        .unwrap_or_default();
    if !state
        .limiter
        .check(&format!("register:{ip}"), 5, Duration::from_secs(3600))
    {
        return Err(AppError::rate_limited());
    }
    let open = registration_open(&state);
    let (token, user) = state
        .db
        .write(move |c| {
            let first: bool = c.query_row("SELECT NOT EXISTS (SELECT 1 FROM users)", [], |r| r.get(0))?;
            if !first && !open {
                return Err(AppError::new(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "Die Registrierung ist geschlossen.",
                ));
            }
            let name = if req.name.trim().is_empty() {
                req.email.split('@').next().unwrap_or("").to_string()
            } else {
                req.name.clone()
            };
            let user = auth::create_user(c, &req.email, &name, &req.password, first)?;
            let token = auth::create_session(c, user.id)?;
            Ok((token, user))
        })
        .await?;
    Ok(with_session_cookie(&state, &token, user.me()))
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> AppResult<Response> {
    if let Some(token) = auth::cookie(&headers, SESSION_COOKIE) {
        state.db.write(move |c| auth::delete_session(c, &token)).await?;
    }
    let mut res = StatusCode::NO_CONTENT.into_response();
    if let Ok(v) = HeaderValue::from_str(&auth::session_cookie("", state.config.secure_cookies, 0)) {
        res.headers_mut().insert(header::SET_COOKIE, v);
    }
    Ok(res)
}

pub async fn update(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<UpdateAccount>,
) -> AppResult<Json<Me>> {
    let email = auth::check_login_name(&req.email)?;
    let name = req.name.trim().chars().take(100).collect::<String>();
    let me = state
        .db
        .write(move |c| {
            c.execute(
                "UPDATE users SET email = ?1, name = ?2 WHERE id = ?3",
                params![email, name, user.id],
            )
            .map_err(|e| match AppError::from(e) {
                err if err.code == "conflict" => {
                    AppError::conflict("Diese E-Mail bzw. dieser Name ist schon vergeben.")
                }
                err => err,
            })?;
            auth::user_by_id(c, user.id)?.ok_or_else(AppError::not_found)
        })
        .await?;
    Ok(Json(me.me()))
}

pub async fn password(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(user): CurrentUser,
    Json(req): Json<ChangePassword>,
) -> AppResult<StatusCode> {
    auth::check_password(&req.new)?;
    let keep = auth::cookie(&headers, SESSION_COOKIE).map(|t| auth::sha256(t.as_bytes()));
    state
        .db
        .write(move |c| {
            let hash: String =
                c.query_row("SELECT password_hash FROM users WHERE id = ?1", [user.id], |r| {
                    r.get(0)
                })?;
            if !auth::verify_password(&hash, &req.current) {
                return Err(AppError::invalid("Das aktuelle Passwort stimmt nicht."));
            }
            c.execute(
                "UPDATE users SET password_hash = ?1 WHERE id = ?2",
                params![auth::hash_password(&req.new)?, user.id],
            )?;
            // Andere Geraete abmelden.
            c.execute(
                "DELETE FROM auth_sessions WHERE user_id = ?1 AND token_hash != ?2",
                params![user.id, keep.unwrap_or_default()],
            )?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn totp_setup(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> AppResult<Json<TotpSetup>> {
    if user.totp_enabled {
        return Err(AppError::conflict("Zwei-Faktor ist schon aktiv."));
    }
    let secret = auth::totp_new_secret();
    let s = secret.clone();
    state
        .db
        .write(move |c| {
            Ok(c.execute(
                "UPDATE users SET totp_pending = ?1 WHERE id = ?2",
                params![s, user.id],
            )?)
        })
        .await?;
    Ok(Json(TotpSetup {
        otpauth_url: auth::otpauth_url(&secret, &user.email),
        secret,
    }))
}

pub async fn totp_enable(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<TotpEnable>,
) -> AppResult<Json<RecoveryCodes>> {
    let codes = state
        .db
        .write(move |c| {
            let pending: Option<String> =
                c.query_row("SELECT totp_pending FROM users WHERE id = ?1", [user.id], |r| {
                    r.get(0)
                })?;
            let Some(secret) = pending else {
                return Err(AppError::invalid("Bitte zuerst die Einrichtung starten."));
            };
            if !auth::totp_verify(&secret, &req.code, now()) {
                return Err(AppError::invalid(
                    "Der Code stimmt nicht – Uhrzeit des Handys prüfen und neu versuchen.",
                ));
            }
            let (codes, hashes) = auth::recovery_codes();
            c.execute(
                "UPDATE users SET totp_secret = ?1, totp_pending = NULL, recovery_codes = ?2 WHERE id = ?3",
                params![secret, hashes, user.id],
            )?;
            Ok(codes)
        })
        .await?;
    Ok(Json(RecoveryCodes { codes }))
}

pub async fn totp_disable(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<TotpDisable>,
) -> AppResult<StatusCode> {
    state
        .db
        .write(move |c| {
            let hash: String = c.query_row("SELECT password_hash FROM users WHERE id = ?1", [user.id], |r| r.get(0))?;
            if !auth::verify_password(&hash, &req.password) {
                return Err(AppError::invalid("Das Passwort stimmt nicht."));
            }
            c.execute("UPDATE users SET totp_secret = NULL, totp_pending = NULL, recovery_codes = '[]' WHERE id = ?1", [user.id])?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn key_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ApiKey> {
    Ok(ApiKey {
        id: r.get(0)?,
        name: r.get(1)?,
        prefix: r.get(2)?,
        scope: if r.get::<_, String>(3)? == "sites" {
            ApiScope::Sites
        } else {
            ApiScope::Stats
        },
        created_at: r.get(4)?,
        last_used_at: r.get(5)?,
    })
}

pub async fn list_keys(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> AppResult<Json<Vec<ApiKey>>> {
    let keys = state
        .db
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, name, prefix, scope, created_at, last_used_at FROM api_keys WHERE user_id = ?1 ORDER BY id DESC",
            )?;
            Ok(stmt.query_map([user.id], key_from_row)?.collect::<Result<Vec<_>, _>>()?)
        })
        .await?;
    Ok(Json(keys))
}

pub async fn create_key(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<NewApiKey>,
) -> AppResult<Json<CreatedApiKey>> {
    let name = req.name.trim().chars().take(100).collect::<String>();
    if name.is_empty() {
        return Err(AppError::invalid("Bitte einen Namen angeben."));
    }
    let (key, prefix, hash) = auth::new_api_key();
    let scope = if req.scope == ApiScope::Sites {
        "sites"
    } else {
        "stats"
    };
    let api_key = state
        .db
        .write(move |c| {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM api_keys WHERE user_id = ?1", [user.id], |r| r.get(0))?;
            if n >= 50 {
                return Err(AppError::invalid("Höchstens 50 API-Schlüssel je Konto."));
            }
            c.execute(
                "INSERT INTO api_keys (user_id, name, prefix, key_hash, scope, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![user.id, name, prefix, hash, scope, now()],
            )?;
            let id = c.last_insert_rowid();
            Ok(c.query_row("SELECT id, name, prefix, scope, created_at, last_used_at FROM api_keys WHERE id = ?1", [id], key_from_row)?)
        })
        .await?;
    Ok(Json(CreatedApiKey { key, api_key }))
}

pub async fn delete_key(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let n = state
        .db
        .write(move |c| {
            Ok(c.execute(
                "DELETE FROM api_keys WHERE id = ?1 AND user_id = ?2",
                params![id, user.id],
            )?)
        })
        .await?;
    if n == 0 {
        return Err(AppError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}
