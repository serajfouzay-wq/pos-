//! Sign-in. Pre-authentication by nature, but still behind the license gate:
//! the user table lives in the encrypted database.

use std::sync::Arc;

use pos_core::rbac::Role;
use pos_core::time::{Clock, SystemClock, Timestamp};
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::blocking;
use crate::repo::audit::{self, Actor};
use crate::repo::users::{self, LoginOutcome};
use crate::repo::{device, SqlResultExt};
use crate::session::Session;
use crate::state::AppState;

/// Mirrors `SessionStatusSchema`.
#[derive(Debug, Serialize)]
pub struct SessionStatus {
    /// No users yet: the first person to use the till creates the owner.
    pub(crate) needs_setup: bool,
    pub(crate) session: Option<Session>,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_status(state: State<'_, AppState>) -> IpcResult<SessionStatus> {
    let db = state.license.database()?;
    let session = state.session.current();
    let (license, engine) = (Arc::clone(&state.license), Arc::clone(&state.sync));
    blocking(move || {
        let mut needs_setup = users::count_active(&db.conn()).ipc()? == 0;
        // A new till of an existing shop: fetch the shop's users before
        // offering owner setup. Best effort and once — offline falls through
        // to setup, and the worker's later rounds still bring the users in
        // (the UI re-checks on `sync://status`).
        if needs_setup && engine.enabled() && !engine.attempted() {
            let _ = crate::sync::round(&license, &engine);
            needs_setup = users::count_active(&db.conn()).ipc()? == 0;
        }
        Ok(SessionStatus {
            needs_setup,
            session,
        })
    })
    .await
}

/// Mirrors `LoginUserSchema`: just enough for the user-picker tiles.
#[derive(Debug, Serialize)]
pub struct LoginUser {
    pub(crate) id: Uuid,
    pub(crate) display_name: String,
    pub(crate) role: Role,
    pub(crate) locked_until: Option<Timestamp>,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_login_users(state: State<'_, AppState>) -> IpcResult<Vec<LoginUser>> {
    let db = state.license.database()?;
    blocking(move || {
        Ok(users::list(&db.conn(), true)
            .ipc()?
            .into_iter()
            .map(|u| LoginUser {
                id: u.meta.id,
                display_name: u.display_name,
                role: u.role,
                locked_until: u.locked_until,
            })
            .collect())
    })
    .await
}

pub fn validate_name(name: &str) -> IpcResult<()> {
    let len = name.trim().chars().count();
    if len == 0 || len > 80 {
        return Err(IpcError::validation("Name must be 1–80 characters."));
    }
    Ok(())
}

pub fn validate_pin(pin: &str) -> IpcResult<()> {
    if users::is_valid_pin(pin) {
        Ok(())
    } else {
        Err(IpcError::validation("PIN must be 4–6 digits."))
    }
}

/// Creates the first owner. Refused as soon as any active user exists.
#[tauri::command(rename_all = "snake_case")]
pub async fn bootstrap_owner(
    state: State<'_, AppState>,
    display_name: String,
    pin: String,
) -> IpcResult<Session> {
    validate_name(&display_name)?;
    validate_pin(&pin)?;
    let db = state.license.database()?;
    let session = blocking(move || {
        let hash = users::hash_pin(&pin).map_err(IpcError::internal)?;
        let now = SystemClock.now();
        let mut conn = db.conn();
        let tx = conn.transaction().ipc()?;
        if users::count_active(&tx).ipc()? > 0 {
            return Err(IpcError::new(
                IpcErrorCode::Conflict,
                "This till is already set up.",
            ));
        }
        let owner = users::create(&tx, &display_name, Role::Owner, hash, now).ipc()?;
        let actor = Actor {
            user_id: owner.meta.id,
            role: Role::Owner,
            device_id: device::id(&tx).ipc()?,
        };
        audit::record(
            &tx,
            &actor,
            "user.manage",
            "users",
            Some(owner.meta.id),
            None,
            Some(serde_json::json!({ "bootstrap": true, "role": "owner" })),
            now,
        )
        .ipc()?;
        tx.commit().ipc()?;
        Ok(Session::new(
            owner.meta.id,
            owner.display_name,
            Role::Owner,
            now,
        ))
    })
    .await?;
    state.session.set(session.clone());
    state.sync.nudge();
    Ok(session)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn login(state: State<'_, AppState>, user_id: Uuid, pin: String) -> IpcResult<Session> {
    if !users::is_valid_pin(&pin) {
        return Err(IpcError::new(IpcErrorCode::Unauthenticated, "Wrong PIN."));
    }
    let db = state.license.database()?;
    let session = blocking(move || {
        let now = SystemClock.now();
        let conn = db.conn();
        match users::attempt_login(&conn, user_id, &pin, now).ipc()? {
            LoginOutcome::Success(user) => {
                let actor = Actor {
                    user_id: user.id,
                    role: user.role,
                    device_id: device::id(&conn).ipc()?,
                };
                audit::record(
                    &conn,
                    &actor,
                    "session.login",
                    "users",
                    Some(user.id),
                    None,
                    None,
                    now,
                )
                .ipc()?;
                Ok(Session::new(user.id, user.display_name, user.role, now))
            }
            LoginOutcome::WrongPin { attempts_left } => Err(IpcError::new(
                IpcErrorCode::Unauthenticated,
                format!("Wrong PIN. {attempts_left} attempt(s) left before a 5-minute lock."),
            )),
            LoginOutcome::Locked { until } => Err(IpcError::new(
                IpcErrorCode::Unauthenticated,
                format!(
                    "Too many wrong PINs. Locked until {} UTC.",
                    &until.to_string()[11..16]
                ),
            )),
            LoginOutcome::Unknown => Err(IpcError::new(
                IpcErrorCode::Unauthenticated,
                "Unknown user.",
            )),
        }
    })
    .await?;
    state.session.set(session.clone());
    state.sync.nudge();
    Ok(session)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn logout(state: State<'_, AppState>) -> IpcResult<()> {
    if let (Some(session), Ok(db)) = (state.session.current(), state.license.database()) {
        let db = Arc::clone(&db);
        let _ = blocking(move || {
            let conn = db.conn();
            let actor = Actor {
                user_id: session.user_id,
                role: session.role,
                device_id: device::id(&conn).ipc()?,
            };
            audit::record(
                &conn,
                &actor,
                "session.logout",
                "users",
                Some(session.user_id),
                None,
                None,
                SystemClock.now(),
            )
            .ipc()
        })
        .await;
        state.sync.nudge();
    }
    state.session.clear();
    Ok(())
}
