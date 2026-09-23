//! Result of `verify_license`. Serializes to `LicenseStatusSchema`.

use pos_core::time::Timestamp;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HaltState {
    Missing,
    InvalidToken,
    FingerprintMismatch,
    Expired,
    Revoked,
    GraceExhausted,
    HardwareError,
    StorageError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidState {
    Valid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidLicense {
    pub state: ValidState,
    pub license_id: Uuid,
    pub client_id: Uuid,
    pub expires_at: Option<Timestamp>,
    pub last_seen_at: Option<Timestamp>,
    /// The most recent cloud validation attempt failed (or none succeeded yet).
    pub offline: bool,
    /// `None` when the grace period is not enforced (no cloud configured).
    pub grace_days_remaining: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HaltedLicense {
    pub state: HaltState,
    /// Safe to display. Never contains fingerprint inputs or key material.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum LicenseStatus {
    Valid(ValidLicense),
    Halted(HaltedLicense),
}

impl LicenseStatus {
    pub fn halted(state: HaltState, reason: impl Into<String>) -> Self {
        LicenseStatus::Halted(HaltedLicense {
            state,
            reason: reason.into(),
        })
    }

    pub fn is_valid(&self) -> bool {
        matches!(self, LicenseStatus::Valid(_))
    }
}
