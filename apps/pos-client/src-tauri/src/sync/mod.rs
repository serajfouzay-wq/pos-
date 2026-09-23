//! Offline sync engine: drains the `sync_queue` outbox to the cloud and
//! applies remote changes locally. Protocol: `@pos/shared` sync.ts; server:
//! `supabase/migrations/20260924000000_sync.sql`.

pub mod apply;
pub mod engine;
pub mod protocol;
pub mod transport;

pub use engine::{SyncEngine, SyncReport, SyncStatus};
pub use transport::{HttpTransport, SyncError, SyncTransport};

#[cfg(test)]
mod tests;

use crate::license::LicenseService;

/// One round for the worker and the "Sync now" command. A 401/403 usually
/// means the cloud has not seen this activation yet (or re-activated it), so
/// the license is re-validated with the cloud once and the round retried.
/// Blocking.
pub fn round(license: &LicenseService, sync: &SyncEngine) -> Result<SyncReport, SyncError> {
    let db = license
        .database()
        .map_err(|e| SyncError::Unauthorized(e.message))?;
    match sync.run(&db, license.sync_credentials().as_ref()) {
        Err(SyncError::Unauthorized(_)) => {
            let _ = license.cloud_check();
            sync.run(&db, license.sync_credentials().as_ref())
        }
        other => other,
    }
}
