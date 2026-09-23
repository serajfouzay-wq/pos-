//! Tauri IPC commands. Contract: `POS_IPC` in `@pos/shared`.
//!
//! Conventions for every command:
//! - `#[tauri::command(rename_all = "snake_case")]` so argument names match the
//!   TypeScript contract verbatim.
//! - Returns `IpcResult<T>` so failures reach the UI as `{ code, message }`.
//! - Commands touching business data obtain the database ONLY via
//!   `state.license.database()?` (fails closed unless the license is valid),
//!   then call `pos_core::rbac::authorize(session.role, Permission::…)`.
//! - Blocking work (SQLite, WMI, HTTP) runs through [`blocking`].
//! - Register it in `generate_handler!` by full path (`commands::x::x`; the
//!   macro cannot see through `pub use`), in `build.rs::COMMANDS` and in a
//!   capability.

pub mod app_info;
pub mod license;

use pos_core::{IpcError, IpcResult};

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
