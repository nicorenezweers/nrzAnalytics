//! Verwaltung (nur Instanz-Admins): Konten und Einstellungen der Instanz.

use crate::{
    AppState,
    auth::{self, CurrentUser},
    db,
    error::{AppError, AppResult},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use nrzanalytics_shared::api::{AdminUser, InstanceSettings, NewUser, UpdateInstanceSettings, UpdateUser};
use rusqlite::params;

fn require_admin(user: &auth::User) -> AppResult<()> {
    if user.is_admin {
        Ok(())
    } else {
        Err(AppError::forbidden())
    }
}

fn load_users(c: &rusqlite::Connection) -> AppResult<Vec<AdminUser>> {
    let mut stmt = c.prepare(
        "SELECT u.id, u.email, u.name, u.is_admin, u.totp_secret IS NOT NULL, u.created_at, u.last_login_at, \
         (SELECT COUNT(*) FROM memberships m WHERE m.user_id = u.id) FROM users u ORDER BY u.id",
    )?;
    let list = stmt
        .query_map([], |r| {
            Ok(AdminUser {
                id: r.get(0)?,
                email: r.get(1)?,
                name: r.get(2)?,
                is_admin: r.get::<_, i64>(3)? != 0,
                totp_enabled: r.get(4)?,
                created_at: r.get(5)?,
                last_login_at: r.get(6)?,
                sites: r.get::<_, i64>(7)?.max(0) as u32,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(list)
}

pub async fn users(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> AppResult<Json<Vec<AdminUser>>> {
    require_admin(&user)?;
    Ok(Json(state.db.read(load_users).await?))
}

pub async fn create_user(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<NewUser>,
) -> AppResult<Json<AdminUser>> {
    require_admin(&user)?;
    let created = state
        .db
        .write(move |c| {
            let u = auth::create_user(c, &req.email, &req.name, &req.password, req.is_admin)?;
            load_users(c)?
                .into_iter()
                .find(|x| x.id == u.id)
                .ok_or_else(AppError::not_found)
        })
        .await?;
    Ok(Json(created))
}

pub async fn update_user(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUser>,
) -> AppResult<Json<AdminUser>> {
    require_admin(&user)?;
    if id == user.id && req.is_admin == Some(false) {
        return Err(AppError::invalid(
            "Das eigene Admin-Recht kann man nicht entfernen.",
        ));
    }
    if let Some(p) = &req.password {
        auth::check_password(p)?;
    }
    let updated = state
        .db
        .write(move |c| {
            auth::user_by_id(c, id)?.ok_or_else(AppError::not_found)?;
            if let Some(name) = &req.name {
                c.execute("UPDATE users SET name = ?1 WHERE id = ?2", params![name.trim().chars().take(100).collect::<String>(), id])?;
            }
            if let Some(a) = req.is_admin {
                c.execute("UPDATE users SET is_admin = ?1 WHERE id = ?2", params![i64::from(a), id])?;
            }
            if let Some(p) = &req.password {
                c.execute(
                    "UPDATE users SET password_hash = ?1, failed_logins = 0, locked_until = 0 WHERE id = ?2",
                    params![auth::hash_password(p)?, id],
                )?;
                c.execute("DELETE FROM auth_sessions WHERE user_id = ?1", [id])?;
            }
            if req.disable_totp {
                c.execute("UPDATE users SET totp_secret = NULL, totp_pending = NULL, recovery_codes = '[]' WHERE id = ?1", [id])?;
            }
            load_users(c)?.into_iter().find(|x| x.id == id).ok_or_else(AppError::not_found)
        })
        .await?;
    Ok(Json(updated))
}

pub async fn delete_user(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    require_admin(&user)?;
    if id == user.id {
        return Err(AppError::invalid("Das eigene Konto kann man hier nicht löschen."));
    }
    state
        .db
        .write(move |c| {
            // Websites, deren einziger Besitzer das Konto ist, gehen an den loeschenden Admin.
            let orphaned: Vec<i64> = {
                let mut stmt = c.prepare(
                    "SELECT site_id FROM memberships m WHERE user_id = ?1 AND role = 'owner' AND \
                     (SELECT COUNT(*) FROM memberships o WHERE o.site_id = m.site_id AND o.role = 'owner') = 1",
                )?;
                stmt.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?
            };
            let tx = c.transaction()?;
            for site in orphaned {
                tx.execute(
                    "INSERT INTO memberships (site_id, user_id, role, created_at) VALUES (?1, ?2, 'owner', ?3) \
                     ON CONFLICT(site_id, user_id) DO UPDATE SET role = 'owner'",
                    params![site, user.id, db::now()],
                )?;
            }
            let n = tx.execute("DELETE FROM users WHERE id = ?1", [id])?;
            tx.commit()?;
            if n == 0 { Err(AppError::not_found()) } else { Ok(()) }
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn current_settings(state: &AppState) -> AppResult<InstanceSettings> {
    let (registration_open, public_url) = state
        .instance
        .read()
        .map(|i| {
            (
                state.config.registration.unwrap_or(i.registration_open),
                state.config.public_url.clone().unwrap_or(i.public_url.clone()),
            )
        })
        .map_err(AppError::internal)?;
    let total_events = state
        .db
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))?))
        .await?;
    let path = state.config.database.clone();
    let database_bytes = ["", "-wal"]
        .iter()
        .filter_map(|suffix| std::fs::metadata(format!("{}{suffix}", path.display())).ok())
        .map(|m| m.len())
        .sum();
    Ok(InstanceSettings {
        registration_open,
        public_url,
        geoip: state.geo.status(),
        total_events: total_events.max(0) as u64,
        database_bytes,
    })
}

pub async fn settings(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> AppResult<Json<InstanceSettings>> {
    require_admin(&user)?;
    Ok(Json(current_settings(&state).await?))
}

pub async fn update_settings(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<UpdateInstanceSettings>,
) -> AppResult<Json<InstanceSettings>> {
    require_admin(&user)?;
    let public_url = match req.public_url.as_deref().map(str::trim) {
        Some("") => Some(String::new()),
        Some(u) => {
            let parsed = url::Url::parse(u).map_err(|_| {
                AppError::invalid("Bitte eine vollständige URL angeben, z. B. https://analytics.example.com")
            })?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                return Err(AppError::invalid("Nur http(s)-Adressen."));
            }
            Some(u.trim_end_matches('/').to_string())
        }
        None => None,
    };
    let reg = req.registration_open;
    let url_clone = public_url.clone();
    state
        .db
        .write(move |c| {
            if let Some(r) = reg {
                db::set_setting(c, "registration_open", if r { "1" } else { "0" })?;
            }
            if let Some(u) = &url_clone {
                db::set_setting(c, "public_url", u)?;
            }
            Ok(())
        })
        .await?;
    if let Ok(mut i) = state.instance.write() {
        if let Some(r) = reg {
            i.registration_open = r;
        }
        if let Some(u) = public_url {
            i.public_url = u;
        }
    }
    Ok(Json(current_settings(&state).await?))
}
