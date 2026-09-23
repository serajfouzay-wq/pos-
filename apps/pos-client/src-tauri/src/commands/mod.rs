//! Tauri IPC commands. Contract: `POS_IPC` in `@pos/shared`.
//!
//! Conventions for every command:
//! - `#[tauri::command(rename_all = "snake_case")]` so argument names match the
//!   TypeScript contract verbatim.
//! - Returns `IpcResult<T>` so failures reach the UI as `{ code, message }`.
//! - Commands touching business data first check the license, then call
//!   `pos_core::rbac::authorize(session.role, Permission::…)`.
//! - Register it in `generate_handler!` by full path (`commands::x::x`; the
//!   macro cannot see through `pub use`), in `build.rs::COMMANDS` and in a
//!   capability.

pub mod app_info;
