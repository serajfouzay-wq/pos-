//! Tauri IPC commands. Contract: `GENERATOR_IPC` in `@pos/shared`.
//! Same conventions as the POS client (see its `commands/mod.rs`): snake_case
//! arguments, `IpcResult`, blocking work (SQLite, scrypt, RSA, HTTP) on the
//! blocking pool, and registration in `generate_handler!`, `build.rs` and the
//! capability.

pub mod app_info;
pub mod builds;
pub mod clients;
pub mod license;

use pos_core::{IpcError, IpcResult};

pub async fn blocking<T, F>(f: F) -> IpcResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> IpcResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| IpcError::internal(format!("background task failed: {e}")))?
}
