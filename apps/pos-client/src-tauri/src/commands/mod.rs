//! Tauri IPC commands. Contract: `POS_IPC` in `@pos/shared`.
//!
//! Conventions for every command:
//! - `#[tauri::command(rename_all = "snake_case")]` so argument names match the
//!   TypeScript contract verbatim.
//! - Returns `IpcResult<T>` so failures reach the UI as `{ code, message }`.
//! - Business commands start with [`authorize`]: license valid (else no data
//!   at all) → signed-in session → `rbac::authorize(role, permission)`.
//!   Pre-authentication commands (app info, license, session bootstrap/login)
//!   are the documented exceptions.
//! - Blocking work (SQLite, Argon2, printers, WMI, HTTP) runs via [`blocking`].
//! - Register it in `generate_handler!` by full path (`commands::x::x`; the
//!   macro cannot see through `pub use`), in `build.rs::COMMANDS` and in a
//!   capability.

pub mod app_info;
pub mod catalog;
pub mod hardware;
pub mod inventory;
pub mod license;
pub mod menu;
pub mod orders;
pub mod sales;
pub mod session;
pub mod shifts;
pub mod sync;
pub mod users;

use std::sync::Arc;

use pos_core::rbac::{self, Permission};
use pos_core::{IpcError, IpcErrorCode, IpcResult};

use crate::db::Database;
use crate::repo::audit::Actor;
use crate::repo::{device, SqlResultExt};
use crate::session::Session;
use crate::state::AppState;

/// Runs `f` on Tauri's blocking pool so the async runtime never stalls.
pub async fn blocking<T, F>(f: F) -> IpcResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> IpcResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| IpcError::internal(format!("background task failed: {e}")))?
}

/// A caller that passed the license gate, is signed in and holds the permission.
pub struct Authorized {
    pub db: Arc<Database>,
    pub session: Session,
}

impl Authorized {
    pub fn actor(&self) -> IpcResult<Actor> {
        Ok(Actor {
            user_id: self.session.user_id,
            role: self.session.role,
            device_id: device::id(&self.db.conn()).ipc()?,
        })
    }
}

/// Pure decision, unit-tested: signed in, and the role holds the permission.
pub fn check_session(session: Option<Session>, permission: Permission) -> IpcResult<Session> {
    let session =
        session.ok_or_else(|| IpcError::new(IpcErrorCode::Unauthenticated, "Sign in first."))?;
    rbac::authorize(session.role, permission)?;
    Ok(session)
}

pub fn authorize(state: &AppState, permission: Permission) -> IpcResult<Authorized> {
    // License first: without it there is no database handle to give out.
    let db = state.license.database()?;
    let session = check_session(state.session.current(), permission)?;
    Ok(Authorized { db, session })
}

#[cfg(test)]
mod tests {
    use pos_core::rbac::Role;
    use pos_core::time::Timestamp;
    use uuid::Uuid;

    use super::*;

    fn session(role: Role) -> Session {
        Session::new(
            Uuid::nil(),
            "x".into(),
            role,
            Timestamp::from_unix_seconds(0).expect("ts"),
        )
    }

    #[test]
    fn guard_requires_a_session_and_the_permission() {
        let err = check_session(None, Permission::SaleCreate).expect_err("anonymous");
        assert_eq!(err.code, IpcErrorCode::Unauthenticated);
        assert!(check_session(Some(session(Role::Cashier)), Permission::SaleCreate).is_ok());
        let err = check_session(Some(session(Role::Cashier)), Permission::SaleRefund)
            .expect_err("cashier");
        assert_eq!(err.code, IpcErrorCode::Forbidden);
        assert!(check_session(Some(session(Role::Manager)), Permission::ShiftOpen).is_ok());
        assert!(check_session(Some(session(Role::Manager)), Permission::UserManage).is_err());
    }
}
