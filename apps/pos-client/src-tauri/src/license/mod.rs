//! The license gate.
//!
//! Order of checks — nothing touches the database until the machine is proven
//! to be the licensed one:
//!
//! 1. Read hardware identifiers (cached for the process lifetime).
//! 2. Read `license.jwt`; verify RS256 signature, issuer/audience, client id
//!    and **fingerprint** against the embedded public key.
//! 3. Only then derive the SQLCipher key and open `pos.db`.
//! 4. Apply persisted revocation, expiry and the 7-day offline grace window
//!    using the tamper-resistant "effective now".
//!
//! Business data is reachable only through [`LicenseService::database`], which
//! refuses unless the current status is `valid`.

pub mod cloud;
mod store;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use pos_core::config::ClientConfig;
use pos_core::time::{Clock, Timestamp};
use pos_core::{IpcError, IpcErrorCode};
use pos_hwid::{HardwareComponents, HwidError};
use pos_license::activation::ActivationRequest;
use pos_license::grace::{self, Grace};
use pos_license::status::{HaltState, LicenseStatus, ValidLicense, ValidState};
use pos_license::verify::{
    check_validity_window, verify_identity, verify_license, Expected, Rejection, VerifiedLicense,
};
use rsa::RsaPublicKey;

use self::cloud::{CloudDecision, CloudRequest, CloudValidator};
use self::store::LicenseBookkeeping;
use crate::db::{Database, DbError};

pub const TOKEN_FILE: &str = "license.jwt";
pub const DATABASE_FILE: &str = "pos.db";

type HardwareProbe = Box<dyn Fn() -> Result<HardwareComponents, HwidError> + Send + Sync>;
type StatusListener = Box<dyn Fn(&LicenseStatus) + Send + Sync>;

/// Everything the gate depends on, injectable for tests.
pub struct LicenseEnv {
    pub client: Arc<ClientConfig>,
    pub public_key: RsaPublicKey,
    pub key_id: String,
    pub data_dir: PathBuf,
    pub hardware: HardwareProbe,
    pub clock: Arc<dyn Clock>,
    /// `None` = no cloud configured: grace is not enforced.
    pub cloud: Option<Arc<dyn CloudValidator>>,
    pub app_version: String,
    pub device_name: String,
}

/// Outcome of a background cloud check, used to schedule the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudCheck {
    /// Nothing to check (no cloud, or no locally valid token to present).
    Skipped,
    Reached,
    Unreachable,
}

/// The token that verified for this machine in the latest evaluation.
#[derive(Clone)]
struct ActiveToken {
    token: String,
    license_id: uuid::Uuid,
}

struct Inner {
    status: LicenseStatus,
    /// Open once the fingerprint matched. Handed out only while `valid`.
    db: Option<Arc<Database>>,
    active: Option<ActiveToken>,
    /// Result of the most recent cloud attempt in this process.
    cloud_reachable: Option<bool>,
}

pub struct LicenseService {
    env: LicenseEnv,
    /// Cached after the first *successful* read; failures are retried.
    hardware: OnceLock<HardwareComponents>,
    inner: Mutex<Inner>,
    listener: OnceLock<StatusListener>,
}

/// Verified token + the open database, as established by one evaluation.
struct Established {
    token: String,
    verified: VerifiedLicense,
    db: Arc<Database>,
}

impl LicenseService {
    pub fn new(env: LicenseEnv) -> Self {
        Self {
            env,
            hardware: OnceLock::new(),
            inner: Mutex::new(Inner {
                status: LicenseStatus::halted(HaltState::Missing, "License not checked yet."),
                db: None,
                active: None,
                cloud_reachable: None,
            }),
            listener: OnceLock::new(),
        }
    }

    /// Called with the new status whenever it changes (used to emit the
    /// `license://status` event).
    pub fn set_listener(&self, listener: impl Fn(&LicenseStatus) + Send + Sync + 'static) {
        let _ = self.listener.set(Box::new(listener));
    }

    fn token_path(&self) -> PathBuf {
        self.env.data_dir.join(TOKEN_FILE)
    }

    fn db_path(&self) -> PathBuf {
        self.env.data_dir.join(DATABASE_FILE)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A transient failure (e.g. WMI timing out during boot) must not lock
    /// the till for the whole process lifetime, so only success is cached.
    fn hardware(&self) -> Result<&HardwareComponents, HwidError> {
        if let Some(hw) = self.hardware.get() {
            return Ok(hw);
        }
        let hw = (self.env.hardware)()?;
        Ok(self.hardware.get_or_init(|| hw))
    }

    fn fingerprint(&self) -> Result<String, HwidError> {
        self.hardware()
            .map(|hw| hw.fingerprint(self.env.client.client_id).to_string())
    }

    #[cfg_attr(not(test), allow(dead_code))] // read by Phase 3 session commands
    pub fn status(&self) -> LicenseStatus {
        self.lock().status.clone()
    }

    /// The only way to business data. Fails closed.
    #[cfg_attr(not(test), allow(dead_code))] // consumed by the Phase 3 data commands
    pub fn database(&self) -> Result<Arc<Database>, IpcError> {
        let inner = self.lock();
        match (&inner.status, &inner.db) {
            (LicenseStatus::Valid(_), Some(db)) => Ok(Arc::clone(db)),
            _ => Err(IpcError::new(
                IpcErrorCode::LicenseInvalid,
                "This till is not licensed. Resolve the license first.",
            )),
        }
    }

    /// Re-runs the full gate and returns the resulting status.
    pub fn evaluate(&self) -> LicenseStatus {
        let mut inner = self.lock();
        let status = self.evaluate_locked(&mut inner);
        self.publish(&mut inner, status)
    }

    fn publish(&self, inner: &mut Inner, status: LicenseStatus) -> LicenseStatus {
        let changed = inner.status != status;
        inner.status = status.clone();
        if changed {
            if let Some(listener) = self.listener.get() {
                listener(&status);
            }
        }
        status
    }

    fn evaluate_locked(&self, inner: &mut Inner) -> LicenseStatus {
        match self.establish(inner) {
            Ok(established) => self.apply_policy(inner, established),
            Err(halted) => {
                // No proven match with this machine: the database must not stay open.
                inner.db = None;
                inner.active = None;
                halted
            }
        }
    }

    /// Steps 1–3: hardware, token, fingerprint, then (and only then) the database.
    fn establish(&self, inner: &mut Inner) -> Result<Established, LicenseStatus> {
        let hardware = match self.hardware() {
            Ok(hw) => hw,
            Err(e) => {
                return Err(LicenseStatus::halted(
                    HaltState::HardwareError,
                    e.to_string(),
                ))
            }
        };
        let token = match store::read_token(&self.token_path()) {
            Ok(Some(token)) => token,
            Ok(None) => {
                return Err(LicenseStatus::halted(
                    HaltState::Missing,
                    "This till has not been activated yet.",
                ))
            }
            Err(e) => {
                return Err(LicenseStatus::halted(
                    HaltState::StorageError,
                    format!("The license file cannot be read: {e}"),
                ))
            }
        };

        // Identity only: time is judged in `apply_policy` against the
        // database's high-water clock, which must be updated first.
        let client_id = self.env.client.client_id;
        let fingerprint = hardware.fingerprint(client_id);
        let verified = verify_identity(
            &token,
            &self.env.public_key,
            &self.env.key_id,
            &Expected {
                client_id,
                fingerprint: fingerprint.as_str(),
            },
        )
        .map_err(rejection_status)?;

        let db = match &inner.db {
            Some(db) => Arc::clone(db),
            None => {
                let db = Arc::new(self.open_database(hardware)?);
                inner.db = Some(Arc::clone(&db));
                db
            }
        };
        Ok(Established {
            token,
            verified,
            db,
        })
    }

    fn open_database(&self, hardware: &HardwareComponents) -> Result<Database, LicenseStatus> {
        let storage_error = |e: &dyn std::fmt::Display| {
            LicenseStatus::halted(
                HaltState::StorageError,
                format!("The local database cannot be opened: {e}"),
            )
        };
        std::fs::create_dir_all(&self.env.data_dir).map_err(|e| storage_error(&e))?;
        let key = hardware.database_key(self.env.client.client_id);
        match Database::open(&self.db_path(), &key) {
            Ok(db) => Ok(db),
            Err(DbError::WrongKey) => {
                // The token matches this machine, yet the database was
                // encrypted under different hardware: the till was
                // re-activated after a hardware change. Keep the old file
                // (recoverable if the hardware returns) and start fresh; sync
                // restores cloud data.
                orphan_database(&self.db_path(), self.env.clock.now())
                    .map_err(|e| storage_error(&e))?;
                Database::open(&self.db_path(), &key).map_err(|e| storage_error(&e))
            }
            Err(e) => Err(storage_error(&e)),
        }
    }

    /// Step 4: revocation, expiry and grace against the effective clock.
    fn apply_policy(&self, inner: &mut Inner, established: Established) -> LicenseStatus {
        let Established {
            token,
            verified,
            db,
        } = established;
        let wall = self.env.clock.now();
        inner.active = Some(ActiveToken {
            token: token.clone(),
            license_id: verified.claims.jti,
        });

        let bookkeeping = {
            let mut conn = db.conn();
            let result = store::ensure_device(&conn, &self.env.device_name, wall)
                .and_then(|_| store::upsert_active_license(&mut conn, &token, &verified, wall))
                .and_then(|bk| {
                    store::raise_high_water(&conn, verified.claims.jti, wall).map(|()| bk)
                });
            match result {
                Ok(bk) => bk,
                Err(e) => {
                    return LicenseStatus::halted(
                        HaltState::StorageError,
                        format!("The local database rejected the license: {e}"),
                    )
                }
            }
        };
        let LicenseBookkeeping {
            last_seen_at,
            revoked_at,
            clock_high_water_at,
        } = bookkeeping;

        if revoked_at.is_some() {
            return LicenseStatus::halted(
                HaltState::Revoked,
                "This till's license has been revoked. Contact your POS provider.",
            );
        }

        let now = grace::effective_now(wall, clock_high_water_at);
        if let Err(rejection) = check_validity_window(&verified, now) {
            return rejection_status(rejection);
        }

        let cloud_enabled = self.env.cloud.is_some();
        let grace_days_remaining = match grace::evaluate(cloud_enabled, verified.issued_at, last_seen_at, now) {
            Grace::NotEnforced => None,
            Grace::Within { days_remaining, .. } => Some(days_remaining),
            Grace::Exhausted { deadline } => {
                return LicenseStatus::halted(
                    HaltState::GraceExhausted,
                    format!(
                        "This till has been offline since before {deadline}. Connect it to the internet to continue."
                    ),
                )
            }
        };

        LicenseStatus::Valid(ValidLicense {
            state: ValidState::Valid,
            license_id: verified.claims.jti,
            client_id: verified.claims.sub,
            expires_at: verified.expires_at,
            last_seen_at,
            offline: cloud_enabled && inner.cloud_reachable != Some(true),
            grace_days_remaining,
        })
    }

    /// The code an unlicensed till shows the operator.
    pub fn activation_request(&self) -> Result<ActivationRequest, HwidError> {
        Ok(ActivationRequest {
            client_id: self.env.client.client_id,
            fingerprint: self.fingerprint()?,
            device_name: self.env.device_name.clone(),
            app_version: self.env.app_version.clone(),
        })
    }

    /// Installs `token` if — and only if — it verifies for this machine.
    /// A rejected token never replaces the current one.
    pub fn activate(&self, token: &str) -> LicenseStatus {
        let mut inner = self.lock();
        let fingerprint = match self.fingerprint() {
            Ok(fp) => fp,
            Err(e) => return LicenseStatus::halted(HaltState::HardwareError, e.to_string()),
        };
        let check = verify_license(
            token,
            &self.env.public_key,
            &self.env.key_id,
            &Expected {
                client_id: self.env.client.client_id,
                fingerprint: &fingerprint,
            },
            self.env.clock.now(),
        );
        if let Err(rejection) = check {
            // Report why, but leave the installed license (and status) untouched.
            return rejection_status(rejection);
        }
        let write = std::fs::create_dir_all(&self.env.data_dir)
            .and_then(|()| store::write_token(&self.token_path(), token));
        if let Err(e) = write {
            let status = LicenseStatus::halted(
                HaltState::StorageError,
                format!("The license could not be saved: {e}"),
            );
            return self.publish(&mut inner, status);
        }
        let status = self.evaluate_locked(&mut inner);
        self.publish(&mut inner, status)
    }

    /// Contacts the cloud (without holding the lock during the request) and
    /// records the verdict. Runs on a blocking thread.
    pub fn cloud_check(&self) -> CloudCheck {
        let Some(cloud) = self.env.cloud.clone() else {
            return CloudCheck::Skipped;
        };
        // Only a token that verified locally for this machine is presented.
        let (active, fingerprint) = {
            let mut inner = self.lock();
            let status = self.evaluate_locked(&mut inner);
            self.publish(&mut inner, status);
            match (inner.active.clone(), self.fingerprint()) {
                (Some(active), Ok(fingerprint)) => (active, fingerprint),
                _ => return CloudCheck::Skipped,
            }
        };

        let verdict = cloud.validate(&CloudRequest {
            token: &active.token,
            fingerprint: &fingerprint,
            device_name: &self.env.device_name,
            app_version: &self.env.app_version,
        });

        let mut inner = self.lock();
        let outcome = match verdict {
            Ok(verdict) => {
                inner.cloud_reachable = Some(true);
                let revoked = verdict.status != CloudDecision::Active;
                if let Some(db) = &inner.db {
                    let conn = db.conn();
                    // A failed write only delays the update to the next check.
                    let _ = store::record_cloud_verdict(
                        &conn,
                        active.license_id,
                        verdict.server_time,
                        revoked,
                    );
                }
                CloudCheck::Reached
            }
            Err(_) => {
                inner.cloud_reachable = Some(false);
                CloudCheck::Unreachable
            }
        };
        let status = self.evaluate_locked(&mut inner);
        self.publish(&mut inner, status);
        outcome
    }
}

fn rejection_status(rejection: Rejection) -> LicenseStatus {
    let state = match &rejection {
        Rejection::InvalidToken(_) | Rejection::WrongClient | Rejection::NotYetValid(_) => {
            HaltState::InvalidToken
        }
        Rejection::FingerprintMismatch => HaltState::FingerprintMismatch,
        Rejection::Expired(_) => HaltState::Expired,
    };
    LicenseStatus::halted(state, rejection.to_string())
}

/// Moves an undecryptable database (and its WAL/SHM) aside.
fn orphan_database(path: &Path, now: Timestamp) -> std::io::Result<()> {
    let suffix = now.to_string().replace([':', '.'], "-");
    for ext in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{ext}", path.display()));
        if from.exists() {
            let to = PathBuf::from(format!("{}.orphaned-{suffix}{ext}", path.display()));
            std::fs::rename(from, to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
