//! Auto-update. The till asks its shop's release channel (the `app-update`
//! edge function, authenticated like sync) in the background, downloads the
//! signed installer and verifies it against the public key compiled into
//! this binary, then installs it:
//! - when the till is closed (quietly; it starts on the new version next
//!   time), or
//! - at once, restarting the till, when a manager chooses "Restart now".
//!
//! After an update the first start shows what changed (`updated_from`),
//! until someone dismisses the notice.
//!
//! Builds without an updater key (`POS_UPDATER_PUBLIC_KEY`) or without a
//! cloud report `unavailable`: new versions arrive as installers.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use pos_core::config::ClientConfig;
use pos_core::time::{Clock, SystemClock, Timestamp};
use pos_core::{IpcError, IpcResult};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::db::Database;
use crate::license::LicenseService;
use crate::repo::settings;

/// Minisign public key of the release signing key, set by the build.
pub const PUBLIC_KEY: Option<&str> = option_env!("POS_UPDATER_PUBLIC_KEY");
/// The tills are Windows machines; the channel only carries Windows builds.
const TARGET: &str = "windows";
const CHECK_TIMEOUT: Duration = Duration::from_secs(60);
const LAST_VERSION: &str = "updater.last_version";
const PENDING: &str = "updater.pending";
const NOTICE: &str = "updater.notice";

/// Mirrors `UpdateStateSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState {
    Unavailable,
    Idle,
    Checking,
    Downloading,
    Ready,
    UpToDate,
    Error,
}

/// Mirrors `UpdateStatusSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateStatus {
    pub state: UpdateState,
    pub current_version: String,
    pub available_version: Option<String>,
    pub notes: Option<String>,
    pub progress_bps: Option<i64>,
    pub error: Option<String>,
    pub last_checked_at: Option<Timestamp>,
    pub updated_from: Option<String>,
    pub updated_notes: Option<String>,
}

/// A downloaded release's notes, kept for the notice after it installs.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pending {
    version: String,
    notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Notice {
    from: String,
    notes: Option<String>,
}

/// `MAJOR.MINOR.PATCH` → comparable parts (pre-release tags ignored).
pub fn version_parts(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let parsed = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(parsed)
}

pub fn is_newer(candidate: &str, than: &str) -> bool {
    matches!((version_parts(candidate), version_parts(than)), (Some(a), Some(b)) if a > b)
}

/// What the notice should say at start-up, given the version that ran
/// last time. Pure (unit-tested); [`UpdateService::start`] persists it.
fn notice_after_start(
    current: &str,
    last: Option<&str>,
    pending: Option<&Pending>,
    existing: Option<Notice>,
) -> Option<Notice> {
    match last {
        Some(last) if is_newer(current, last) => Some(Notice {
            from: last.to_owned(),
            notes: pending
                .filter(|p| p.version == current)
                .and_then(|p| p.notes.clone()),
        }),
        _ => existing,
    }
}

type Listener = Box<dyn Fn(&UpdateStatus) + Send + Sync>;

pub struct UpdateService {
    configured: bool,
    status: Mutex<UpdateStatus>,
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    busy: tokio::sync::Mutex<()>,
    listener: OnceLock<Listener>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl UpdateService {
    pub fn new(current_version: String, client: &ClientConfig) -> Self {
        let configured =
            PUBLIC_KEY.is_some_and(|k| !k.trim().is_empty()) && client.cloud.endpoint().is_some();
        Self {
            configured,
            status: Mutex::new(UpdateStatus {
                state: if configured {
                    UpdateState::Idle
                } else {
                    UpdateState::Unavailable
                },
                current_version,
                available_version: None,
                notes: None,
                progress_bps: None,
                error: None,
                last_checked_at: None,
                updated_from: None,
                updated_notes: None,
            }),
            ready: Mutex::new(None),
            busy: tokio::sync::Mutex::new(()),
            listener: OnceLock::new(),
        }
    }

    pub fn configured(&self) -> bool {
        self.configured
    }

    pub fn set_listener(&self, listener: impl Fn(&UpdateStatus) + Send + Sync + 'static) {
        let _ = self.listener.set(Box::new(listener));
    }

    pub fn status(&self) -> UpdateStatus {
        lock(&self.status).clone()
    }

    fn update(&self, f: impl FnOnce(&mut UpdateStatus)) {
        let status = {
            let mut guard = lock(&self.status);
            f(&mut guard);
            guard.clone()
        };
        if let Some(listener) = self.listener.get() {
            listener(&status);
        }
    }

    /// Records the running version and raises the "updated" notice when it
    /// is newer than the one that ran before.
    pub fn start(&self, db: &Database, now: Timestamp) -> IpcResult<()> {
        let conn = db.conn();
        let current = self.status().current_version;
        let last: Option<String> = settings::get(&conn, LAST_VERSION).map_err(internal)?;
        let pending: Option<Pending> = settings::get(&conn, PENDING).map_err(internal)?;
        let existing: Option<Notice> = settings::get(&conn, NOTICE).map_err(internal)?;
        let notice = notice_after_start(&current, last.as_deref(), pending.as_ref(), existing);
        if last.as_deref() != Some(current.as_str()) {
            settings::put(&conn, LAST_VERSION, &current, now).map_err(internal)?;
            settings::put(&conn, NOTICE, &notice, now).map_err(internal)?;
        }
        drop(conn);
        self.update(|s| {
            s.updated_from = notice.as_ref().map(|n| n.from.clone());
            s.updated_notes = notice.and_then(|n| n.notes);
        });
        Ok(())
    }

    pub fn dismiss(&self, db: &Database, now: Timestamp) -> IpcResult<UpdateStatus> {
        settings::put(&db.conn(), NOTICE, &None::<Notice>, now).map_err(internal)?;
        self.update(|s| {
            s.updated_from = None;
            s.updated_notes = None;
        });
        Ok(self.status())
    }

    /// One check (and download when there is something new).
    pub async fn check(
        &self,
        app: &AppHandle,
        license: &LicenseService,
        client: &ClientConfig,
    ) -> UpdateStatus {
        if !self.configured {
            return self.status();
        }
        let Ok(_busy) = self.busy.try_lock() else {
            return self.status();
        };
        if self.status().state == UpdateState::Ready {
            return self.status();
        }
        self.update(|s| {
            s.state = UpdateState::Checking;
            s.error = None;
        });
        let outcome = self.check_and_download(app, license, client).await;
        let now = SystemClock.now();
        match outcome {
            Ok(None) => self.update(|s| {
                s.state = UpdateState::UpToDate;
                s.available_version = None;
                s.notes = None;
                s.progress_bps = None;
                s.last_checked_at = Some(now);
            }),
            Ok(Some((update, bytes))) => {
                if let Ok(db) = license.database() {
                    let pending = Pending {
                        version: update.version.clone(),
                        notes: update.body.clone(),
                    };
                    let _ = settings::put(&db.conn(), PENDING, &pending, now);
                }
                let (version, notes) = (update.version.clone(), update.body.clone());
                *lock(&self.ready) = Some((update, bytes));
                self.update(|s| {
                    s.state = UpdateState::Ready;
                    s.available_version = Some(version);
                    s.notes = notes;
                    s.progress_bps = None;
                    s.last_checked_at = Some(now);
                });
            }
            Err(message) => self.update(|s| {
                s.state = UpdateState::Error;
                s.error = Some(message);
                s.progress_bps = None;
                s.last_checked_at = Some(now);
            }),
        }
        self.status()
    }

    async fn check_and_download(
        &self,
        app: &AppHandle,
        license: &LicenseService,
        client: &ClientConfig,
    ) -> Result<Option<(Update, Vec<u8>)>, String> {
        let (base, anon_key) = client
            .cloud
            .endpoint()
            .ok_or_else(|| "no cloud configured".to_owned())?;
        let credentials = license
            .sync_credentials()
            .ok_or_else(|| "this till is not activated with the cloud yet".to_owned())?;
        let endpoint = format!(
            "{base}/functions/v1/app-update?current_version={{{{current_version}}}}&target={{{{target}}}}&arch={{{{arch}}}}"
        );
        let url = endpoint.parse().map_err(|e| format!("update URL: {e}"))?;
        let updater = app
            .updater_builder()
            .target(TARGET)
            .endpoints(vec![url])
            .and_then(|b| b.header("apikey", anon_key))
            .and_then(|b| b.header("Authorization", format!("Bearer {anon_key}")))
            .and_then(|b| b.header("x-pos-license", credentials.token.as_str()))
            .and_then(|b| b.header("x-pos-device-key", credentials.device_key.as_str()))
            .map(|b| b.timeout(CHECK_TIMEOUT))
            .and_then(|b| b.build())
            .map_err(|e| e.to_string())?;
        let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        let version = update.version.clone();
        self.update(|s| {
            s.state = UpdateState::Downloading;
            s.available_version = Some(version);
            s.notes = update.body.clone();
            s.progress_bps = Some(0);
        });
        let mut received: u64 = 0;
        // Verifies the signature against the compiled-in key before returning.
        let bytes = update
            .download(
                |chunk, total| {
                    received += chunk as u64;
                    if let Some(total) = total.filter(|t| *t > 0) {
                        let bps = i64::try_from(received.saturating_mul(10_000) / total)
                            .unwrap_or(10_000)
                            .min(10_000);
                        self.update(|s| s.progress_bps = Some(bps));
                    }
                },
                || {},
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some((update, bytes)))
    }

    /// Installs the downloaded update now and restarts the till.
    pub fn install_now(&self) -> IpcResult<()> {
        let ready = lock(&self.ready).take();
        let Some((update, bytes)) = ready else {
            return Err(IpcError::validation("No update is ready to install."));
        };
        if !cfg!(windows) {
            *lock(&self.ready) = Some((update, bytes));
            return Err(IpcError::validation(
                "Updates install on Windows tills only.",
            ));
        }
        update
            .restart_after_install(true)
            .install(&bytes)
            .map_err(|e| IpcError::internal(format!("the update did not install: {e}")))
    }

    /// Called as the till exits: a downloaded update installs quietly and
    /// the next start runs the new version.
    pub fn install_on_exit(&self) {
        if !cfg!(windows) {
            return;
        }
        if let Some((update, bytes)) = lock(&self.ready).take() {
            let _ = update.restart_after_install(false).install(&bytes);
        }
    }
}

fn internal(e: rusqlite::Error) -> IpcError {
    IpcError::internal(format!("storage error: {e}"))
}

/// Background checks: shortly after start-up, then every few hours (sooner
/// after a failure).
pub fn spawn_worker(
    app: AppHandle,
    updates: Arc<UpdateService>,
    license: Arc<LicenseService>,
    client: Arc<ClientConfig>,
) {
    const FIRST_AFTER: Duration = Duration::from_secs(if cfg!(debug_assertions) { 20 } else { 90 });
    const EVERY: Duration = Duration::from_secs(4 * 60 * 60);
    const RETRY: Duration = Duration::from_secs(30 * 60);
    tauri::async_runtime::spawn(async move {
        let mut started = false;
        tokio::time::sleep(FIRST_AFTER).await;
        loop {
            if let Ok(db) = license.database() {
                if !started {
                    started = updates.start(&db, SystemClock.now()).is_ok();
                }
                let status = updates.check(&app, &license, &client).await;
                let wait = match status.state {
                    UpdateState::Error => RETRY,
                    UpdateState::Unavailable => return,
                    _ => EVERY,
                };
                tokio::time::sleep(wait).await;
            } else {
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.99"));
        assert!(!is_newer("0.1.2", "0.1.2"));
        assert!(!is_newer("0.1.2", "0.1.3"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert_eq!(version_parts("0.2.3-beta.1"), Some((0, 2, 3)));
        assert_eq!(version_parts("0.2"), None);
    }

    #[test]
    fn the_notice_follows_a_version_change() {
        let pending = Pending {
            version: "0.1.5".into(),
            notes: Some("Loyalty points".into()),
        };
        // First install: nothing ran before.
        assert!(notice_after_start("0.1.4", None, None, None).is_none());
        // Updated from 0.1.4 to the pending 0.1.5: its notes are shown.
        let notice =
            notice_after_start("0.1.5", Some("0.1.4"), Some(&pending), None).expect("notice");
        assert_eq!(notice.from, "0.1.4");
        assert_eq!(notice.notes.as_deref(), Some("Loyalty points"));
        // Installed by hand to a version that was not the downloaded one.
        let other = notice_after_start("0.1.6", Some("0.1.4"), Some(&pending), None).expect("n");
        assert!(other.notes.is_none());
        // Same version again: an undismissed notice stays, none is made up.
        let kept = notice_after_start("0.1.5", Some("0.1.5"), None, Some(notice.clone()));
        assert_eq!(kept.map(|n| n.from), Some("0.1.4".into()));
        assert!(notice_after_start("0.1.5", Some("0.1.5"), None, None).is_none());
        // A downgrade (reinstalling an older build) raises nothing.
        assert!(notice_after_start("0.1.3", Some("0.1.5"), None, None).is_none());
    }
}
