//! Backups of the till's database, made for shops that rarely see the
//! internet: power cuts, a dying disk or a stolen PC must not lose the
//! books.
//!
//! - Automatic: at start and every `interval_hours` (12 by default), when a
//!   shift closes and after a Z report; the newest `keep` are kept.
//! - Each backup is a complete encrypted SQLite file (`sqlcipher_export`)
//!   plus a small JSON description next to it. It is also copied to a
//!   second folder (a USB stick or another disk) when one is set.
//! - Without a backup password a backup is encrypted with this machine's
//!   key (it restores only here). With one, it is encrypted with a key
//!   derived from the password (Argon2id), so it restores on a new PC.
//! - Restoring stages the backup (re-encrypted for this machine) and
//!   restarts; the database in use is kept aside, never deleted.
//! - `PRAGMA quick_check` runs at start; a damaged database is reported so
//!   the owner restores the last good backup.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use argon2::{Algorithm, Argon2, Params, Version};
use chrono::Duration;
use pos_core::time::{Clock, Timestamp};
use pos_core::{IpcError, IpcResult};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::db::{migrations_latest, Database};
use crate::repo::{device, settings, SqlResultExt};

pub const SETTINGS_KEY: &str = "backup";
const KEY_SETTING: &str = "backup.key";
pub const DIR_NAME: &str = "backups";
const EXTENSION: &str = "posbak";
/// The staged restore, swapped in before the database is opened.
pub const RESTORE_FILE: &str = "pos.db.restore";
const FORMAT: u32 = 1;

fn yes() -> bool {
    true
}
fn default_keep() -> u32 {
    30
}
fn default_interval() -> u32 {
    12
}

/// Mirrors `BackupSettingsSchema` (device-local).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSettings {
    #[serde(default = "yes")]
    pub automatic: bool,
    #[serde(default = "default_interval")]
    pub interval_hours: u32,
    #[serde(default = "default_keep")]
    pub keep: u32,
    /// A second copy of every backup (USB stick, another disk).
    #[serde(default)]
    pub extra_dir: Option<String>,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            automatic: true,
            interval_hours: default_interval(),
            keep: default_keep(),
            extra_dir: None,
        }
    }
}

impl BackupSettings {
    pub fn validate(&self) -> IpcResult<()> {
        if !(1..=168).contains(&self.interval_hours) {
            return Err(IpcError::validation("Back up every 1 to 168 hours."));
        }
        if !(3..=365).contains(&self.keep) {
            return Err(IpcError::validation("Keep between 3 and 365 backups."));
        }
        if let Some(dir) = &self.extra_dir {
            if !Path::new(dir).is_absolute() {
                return Err(IpcError::validation(
                    "Choose a full folder path for the second copy.",
                ));
            }
        }
        Ok(())
    }
}

/// The password-derived key, kept in the (encrypted) local database so
/// automatic backups can use it. Never sent to the UI.
#[derive(Clone, Serialize, Deserialize)]
struct PortableKey {
    salt: String,
    key: String,
}

/// Why a backup was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Manual,
    Scheduled,
    Start,
    ShiftClose,
    ZReport,
    BeforeRestore,
    BeforeUpdate,
}

/// The JSON written next to each backup (and mirrors `BackupInfoSchema`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupMeta {
    pub format: u32,
    pub created_at: Timestamp,
    pub reason: Reason,
    pub app_version: String,
    pub schema_version: i64,
    pub client_id: Uuid,
    pub device_id: Uuid,
    pub device_name: String,
    /// Encrypted with the backup password (restores on any PC) or with
    /// this machine's key.
    pub portable: bool,
    /// Argon2id salt of a portable backup (hex).
    pub salt: Option<String>,
    pub size_bytes: u64,
}

/// Mirrors `BackupInfoSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupInfo {
    pub path: String,
    pub file_name: String,
    #[serde(flatten)]
    pub meta: BackupMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Integrity {
    Unknown,
    Ok,
    Damaged,
}

/// Mirrors `BackupStatusSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct BackupStatus {
    pub settings: BackupSettings,
    pub dir: String,
    pub backups: Vec<BackupInfo>,
    pub last_backup_at: Option<Timestamp>,
    pub password_set: bool,
    /// The second folder exists and can be written to.
    pub extra_dir_ok: Option<bool>,
    pub integrity: Integrity,
    pub integrity_detail: Option<String>,
    pub last_error: Option<String>,
    /// A restore is staged: it takes effect when the app restarts.
    pub restore_pending: bool,
}

fn argon2() -> IpcResult<Argon2<'static>> {
    let params = Params::new(19 * 1024, 2, 1, Some(32))
        .map_err(|e| IpcError::internal(format!("argon2 parameters: {e}")))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// `x'<hex>'` for a password and salt.
fn derive_key(password: &str, salt: &[u8]) -> IpcResult<Zeroizing<String>> {
    let mut key = Zeroizing::new([0u8; 32]);
    argon2()?
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| IpcError::internal(format!("derive backup key: {e}")))?;
    Ok(Zeroizing::new(format!("x'{}'", hex::encode(*key))))
}

fn escape(path: &Path) -> String {
    path.display().to_string()
}

pub struct BackupService {
    data_dir: PathBuf,
    client_id: Uuid,
    app_version: String,
    clock: Arc<dyn Clock>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    integrity: Option<(Integrity, Option<String>)>,
    last_error: Option<String>,
}

impl BackupService {
    pub fn new(
        data_dir: PathBuf,
        client_id: Uuid,
        app_version: String,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            data_dir,
            client_id,
            app_version,
            clock,
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn dir(&self) -> PathBuf {
        self.data_dir.join(DIR_NAME)
    }

    pub fn settings(db: &Database) -> IpcResult<BackupSettings> {
        Ok(settings::get(&db.conn(), SETTINGS_KEY)
            .ipc()?
            .unwrap_or_default())
    }

    fn portable_key(db: &Database) -> IpcResult<Option<PortableKey>> {
        settings::get(&db.conn(), KEY_SETTING).ipc()
    }

    /// Sets (or replaces) the backup password. Backups made from now on
    /// restore on any PC with it.
    pub fn set_password(&self, db: &Database, password: &str) -> IpcResult<()> {
        if password.chars().count() < 6 {
            return Err(IpcError::validation(
                "Use at least 6 characters for the backup password.",
            ));
        }
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        let key = derive_key(password, &salt)?;
        let stored = PortableKey {
            salt: hex::encode(salt),
            key: key.to_string(),
        };
        settings::put(&db.conn(), KEY_SETTING, &stored, self.clock.now()).ipc()
    }

    /// Runs `PRAGMA quick_check` and remembers the answer.
    pub fn check_integrity(&self, db: &Database) -> Integrity {
        let result: rusqlite::Result<Vec<String>> = db
            .conn()
            .prepare("PRAGMA quick_check")
            .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect());
        let (integrity, detail) = match result {
            Ok(rows) if rows.len() == 1 && rows[0] == "ok" => (Integrity::Ok, None),
            Ok(rows) => (Integrity::Damaged, Some(rows.join("; "))),
            Err(e) => (Integrity::Damaged, Some(e.to_string())),
        };
        self.state().integrity = Some((integrity, detail));
        integrity
    }

    /// Makes a backup now.
    pub fn create(&self, db: &Database, reason: Reason) -> IpcResult<BackupInfo> {
        let result = self.create_inner(db, reason);
        self.state().last_error = result.as_ref().err().map(|e| e.message.clone());
        result
    }

    fn create_inner(&self, db: &Database, reason: Reason) -> IpcResult<BackupInfo> {
        let now = self.clock.now();
        let settings = Self::settings(db)?;
        let dir = self.dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| IpcError::internal(format!("create {}: {e}", dir.display())))?;
        let portable = Self::portable_key(db)?;
        let (device_id, device_name) = {
            let conn = db.conn();
            let id = device::id(&conn).ipc()?;
            let name: String = conn
                .query_row(
                    "SELECT name FROM device WHERE id = ?1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .ipc()?;
            (id, name)
        };
        let stamp = now.datetime().format("%Y%m%d-%H%M%S%3f");
        let reason_name = serde_json::to_value(reason)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let file_name = format!(
            "pos-{}-{stamp}-{reason_name}.{EXTENSION}",
            device::receipt_prefix(device_id)
        );
        let path = dir.join(&file_name);
        let partial = path.with_extension(format!("{EXTENSION}.part"));
        let _ = std::fs::remove_file(&partial);
        let key = match &portable {
            Some(p) => Zeroizing::new(p.key.clone()),
            None => db.export_key(),
        };
        db.export(&partial, &key)?;
        std::fs::rename(&partial, &path)
            .map_err(|e| IpcError::internal(format!("save {}: {e}", path.display())))?;
        let meta = BackupMeta {
            format: FORMAT,
            created_at: now,
            reason,
            app_version: self.app_version.clone(),
            schema_version: migrations_latest(),
            client_id: self.client_id,
            device_id,
            device_name,
            portable: portable.is_some(),
            salt: portable.map(|p| p.salt),
            size_bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        };
        let json =
            serde_json::to_vec_pretty(&meta).map_err(|e| IpcError::internal(e.to_string()))?;
        std::fs::write(sidecar(&path), &json)
            .map_err(|e| IpcError::internal(format!("save backup description: {e}")))?;
        prune(&dir, settings.keep);

        // The second copy is best effort: a missing USB stick must not stop
        // the local backup.
        if let Some(extra) = settings.extra_dir.as_deref().map(PathBuf::from) {
            let copied = std::fs::create_dir_all(&extra)
                .and_then(|()| std::fs::copy(&path, extra.join(&file_name)).map(|_| ()))
                .and_then(|()| std::fs::write(sidecar(&extra.join(&file_name)), &json));
            match copied {
                Ok(()) => prune(&extra, settings.keep),
                Err(e) => {
                    return Err(IpcError::internal(format!(
                        "The backup was made, but the copy to {} failed: {e}",
                        extra.display()
                    )))
                }
            }
        }
        Ok(BackupInfo {
            path: path.display().to_string(),
            file_name,
            meta,
        })
    }

    /// A backup on a background thread (after a shift closes or a Z report):
    /// the till does not wait for it.
    pub fn in_background(self: &Arc<Self>, db: Arc<Database>, reason: Reason) {
        let service = Arc::clone(self);
        std::thread::spawn(move || {
            let _ = service.create(&db, reason);
        });
    }

    /// Backups in this till's folder (or `dir`), newest first.
    pub fn list(&self, dir: Option<&Path>) -> Vec<BackupInfo> {
        list_dir(dir.unwrap_or(&self.dir()))
    }

    /// A scheduled or start-up backup when the last one is old enough.
    pub fn maybe_backup(&self, db: &Database, reason: Reason) -> IpcResult<Option<BackupInfo>> {
        let settings = Self::settings(db)?;
        if !settings.automatic {
            return Ok(None);
        }
        let due = self
            .list(None)
            .first()
            .map(|b| b.meta.created_at)
            .and_then(|last| last.checked_add(Duration::hours(i64::from(settings.interval_hours))))
            .map_or(true, |next| self.clock.now() >= next);
        if !due {
            return Ok(None);
        }
        self.create(db, reason).map(Some)
    }

    pub fn status(&self, db: &Database) -> IpcResult<BackupStatus> {
        let settings = Self::settings(db)?;
        let backups = self.list(None);
        let extra_dir_ok = settings.extra_dir.as_deref().map(|dir| {
            let probe = Path::new(dir).join(".pos-write-test");
            let ok = std::fs::create_dir_all(dir)
                .and_then(|()| std::fs::write(&probe, b"ok"))
                .is_ok();
            let _ = std::fs::remove_file(&probe);
            ok
        });
        let state = self.state();
        let (integrity, integrity_detail) = state
            .integrity
            .clone()
            .unwrap_or((Integrity::Unknown, None));
        Ok(BackupStatus {
            dir: self.dir().display().to_string(),
            last_backup_at: backups.first().map(|b| b.meta.created_at),
            password_set: Self::portable_key(db)?.is_some(),
            restore_pending: self.data_dir.join(RESTORE_FILE).exists(),
            backups,
            extra_dir_ok,
            integrity,
            integrity_detail,
            last_error: state.last_error.clone(),
            settings,
        })
    }

    /// Checks a backup and stages it (re-encrypted for this machine) to
    /// replace the database at the next start. `machine_key` opens backups
    /// made here without a password.
    pub fn stage_restore(
        &self,
        backup: &Path,
        password: Option<&str>,
        machine_key: &str,
    ) -> IpcResult<BackupMeta> {
        let meta: BackupMeta = std::fs::read(sidecar(backup))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or_else(|| {
                IpcError::validation("That file is not a POS backup (its description is missing).")
            })?;
        if meta.client_id != self.client_id {
            return Err(IpcError::validation(
                "That backup belongs to another business.",
            ));
        }
        if meta.schema_version > migrations_latest() {
            return Err(IpcError::validation(
                "That backup was made by a newer version of the app. Update this till first.",
            ));
        }
        let key = match (&meta.salt, password) {
            (Some(salt), Some(password)) => {
                let salt = hex::decode(salt)
                    .map_err(|_| IpcError::validation("The backup description is damaged."))?;
                derive_key(password, &salt)?
            }
            (Some(_), None) => {
                return Err(IpcError::validation(
                    "This backup is protected: enter the backup password.",
                ))
            }
            (None, _) => Zeroizing::new(machine_key.to_owned()),
        };
        let staged = self.data_dir.join(RESTORE_FILE);
        let partial = staged.with_extension("restore.part");
        let _ = std::fs::remove_file(&partial);
        // Work on a copy: the backup itself (maybe on a USB stick) is never
        // written to.
        let copy = self.data_dir.join("restore-source.tmp");
        std::fs::copy(backup, &copy)
            .map_err(|e| IpcError::internal(format!("read backup: {e}")))?;
        let result = self.export_backup(&copy, &partial, &key, &meta, machine_key);
        let _ = std::fs::remove_file(&copy);
        result?;
        std::fs::rename(&partial, &staged)
            .map_err(|e| IpcError::internal(format!("stage restore: {e}")))?;
        Ok(meta)
    }

    fn export_backup(
        &self,
        copy: &Path,
        partial: &Path,
        key: &str,
        meta: &BackupMeta,
        machine_key: &str,
    ) -> IpcResult<()> {
        let source = rusqlite::Connection::open(copy)
            .map_err(|e| IpcError::internal(format!("open backup: {e}")))?;
        let wrong = || {
            if meta.portable {
                IpcError::validation("Wrong backup password.")
            } else {
                IpcError::validation(
                    "This backup was made on another PC without a backup password; it can only be restored there.",
                )
            }
        };
        source
            .pragma_update(None, "key", key)
            .map_err(|_| wrong())?;
        let check: String = source
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|_| wrong())?;
        if check != "ok" {
            return Err(IpcError::validation(format!(
                "That backup is damaged ({check})."
            )));
        }
        export_connection(&source, partial, machine_key, meta.schema_version)
    }
}

/// Swaps a staged restore in before the database is opened. The database
/// it replaces is kept as `pos.db.before-restore-<time>`.
pub fn apply_staged_restore(
    data_dir: &Path,
    db_file: &str,
    now: Timestamp,
) -> std::io::Result<bool> {
    let staged = data_dir.join(RESTORE_FILE);
    if !staged.exists() {
        return Ok(false);
    }
    let db = data_dir.join(db_file);
    if db.exists() {
        let kept = data_dir.join(format!(
            "{db_file}.before-restore-{}",
            now.datetime().format("%Y%m%d-%H%M%S")
        ));
        std::fs::rename(&db, &kept)?;
    }
    for suffix in ["-wal", "-shm"] {
        let side = data_dir.join(format!("{db_file}{suffix}"));
        if side.exists() {
            std::fs::remove_file(side)?;
        }
    }
    std::fs::rename(&staged, &db)?;
    Ok(true)
}

fn sidecar(backup: &Path) -> PathBuf {
    backup.with_extension(format!("{EXTENSION}.json"))
}

fn list_dir(dir: &Path) -> Vec<BackupInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<BackupInfo> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some(EXTENSION))
        .filter_map(|path| {
            let meta: BackupMeta =
                serde_json::from_slice(&std::fs::read(sidecar(&path)).ok()?).ok()?;
            Some(BackupInfo {
                file_name: path.file_name()?.to_string_lossy().into_owned(),
                path: escape(&path),
                meta,
            })
        })
        .collect();
    found.sort_by_key(|b| std::cmp::Reverse(b.meta.created_at));
    found
}

/// Keeps the newest `keep` backups in `dir`.
fn prune(dir: &Path, keep: u32) {
    for old in list_dir(dir).into_iter().skip(keep as usize) {
        let path = PathBuf::from(&old.path);
        let _ = std::fs::remove_file(sidecar(&path));
        let _ = std::fs::remove_file(&path);
    }
}

/// `sqlcipher_export` of `conn` into a new file encrypted with `key`.
pub fn export_connection(
    conn: &rusqlite::Connection,
    dest: &Path,
    key: &str,
    schema_version: i64,
) -> IpcResult<()> {
    let fail = |e: rusqlite::Error| IpcError::internal(format!("backup: {e}"));
    conn.execute(
        "ATTACH DATABASE ?1 AS backup KEY ?2",
        rusqlite::params![escape(dest), key],
    )
    .map_err(fail)?;
    let exported = conn
        .query_row("SELECT sqlcipher_export('backup')", [], |_| Ok(()))
        .and_then(|()| conn.pragma_update(Some("backup"), "user_version", schema_version));
    let detached = conn.execute_batch("DETACH DATABASE backup");
    exported.map_err(fail)?;
    detached.map_err(fail)
}

#[cfg(test)]
mod tests;
