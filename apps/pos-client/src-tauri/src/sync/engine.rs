//! One sync round = push the outbox, then pull remote changes.
//!
//! - Push: pending `sync_queue` rows in outbox order, 500 per request. An
//!   acknowledged event is marked `sent_at`; a retryable rejection (or an
//!   event the server did not mention) backs off exponentially; a permanent
//!   rejection is *parked* (`deleted_at` + `last_error`) so it can never block
//!   the queue — it stays on disk for diagnosis.
//! - Pull: pages after the stored cursor. Each page and the cursor advance
//!   commit in one SQLite transaction, so a crash re-pulls the page, and
//!   applying a page twice is harmless (upserts / insert-once).
//!
//! The database lock is never held across a network call.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use chrono::Duration;
use pos_core::time::{Clock, Timestamp};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

use super::apply::{self, Applied};
use super::protocol::{
    PullRequest, PushRequest, SyncEvent, PROTOCOL_VERSION, PULL_PAGE, PUSH_BATCH,
};
use super::transport::{SyncError, SyncTransport};
use crate::db::Database;
use crate::license::SyncCredentials;
use crate::repo::{device, settings};

pub const CURSOR_KEY: &str = "sync.cursor";
pub const LAST_SYNCED_KEY: &str = "sync.last_synced_at";
/// Upper bound per round so one huge backlog cannot monopolise the worker.
const MAX_PUSH_BATCHES: usize = 20;
const MAX_PULL_PAGES: usize = 50;
const BACKOFF_BASE_SECS: i64 = 30;
const BACKOFF_MAX_SECS: i64 = 60 * 60;

/// Mirrors `SyncStatusSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncStatus {
    pub state: SyncState,
    pub pending: u64,
    pub parked: u64,
    pub last_synced_at: Option<Timestamp>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Disabled,
    Idle,
    Syncing,
    Offline,
    Error,
}

/// Mirrors `SyncReportSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub online: bool,
    pub pushed: u64,
    pub rejected: u64,
    pub pulled: u64,
    pub pending: u64,
    pub last_synced_at: Option<Timestamp>,
}

type StatusListener = Box<dyn Fn(&SyncStatus) + Send + Sync>;

struct Runtime {
    state: SyncState,
    last_error: Option<String>,
}

pub struct SyncEngine {
    transport: Option<Arc<dyn SyncTransport>>,
    clock: Arc<dyn Clock>,
    /// Serialises rounds (worker vs. "Sync now").
    round: Mutex<()>,
    runtime: Mutex<Runtime>,
    listener: OnceLock<StatusListener>,
    nudge: tokio::sync::Notify,
    attempted: AtomicBool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct PushOutcome {
    acknowledged: u64,
    rejected: u64,
}

struct Pending {
    id: Uuid,
    event_type: String,
    entity_type: String,
    entity_id: Uuid,
    payload: String,
    created_at: Timestamp,
    attempt_count: i64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn local(e: impl std::fmt::Display) -> SyncError {
    SyncError::Local(e.to_string())
}

/// `min(2^attempts · 30 s, 1 h)`.
pub fn backoff(attempts: i64) -> Duration {
    let exp = u32::try_from(attempts.clamp(0, 16)).unwrap_or(16);
    let secs = BACKOFF_BASE_SECS
        .saturating_mul(1_i64 << exp)
        .min(BACKOFF_MAX_SECS);
    Duration::seconds(secs)
}

impl SyncEngine {
    /// `transport: None` = offline-only install (no cloud configured).
    pub fn new(transport: Option<Arc<dyn SyncTransport>>, clock: Arc<dyn Clock>) -> Self {
        let state = if transport.is_some() {
            SyncState::Idle
        } else {
            SyncState::Disabled
        };
        Self {
            transport,
            clock,
            round: Mutex::new(()),
            runtime: Mutex::new(Runtime {
                state,
                last_error: None,
            }),
            listener: OnceLock::new(),
            nudge: tokio::sync::Notify::new(),
            attempted: AtomicBool::new(false),
        }
    }

    pub fn enabled(&self) -> bool {
        self.transport.is_some()
    }

    /// Whether a round has been attempted in this process.
    pub fn attempted(&self) -> bool {
        self.attempted.load(Ordering::Relaxed)
    }

    /// Called with the new status after every state change (`sync://status`).
    pub fn set_listener(&self, listener: impl Fn(&SyncStatus) + Send + Sync + 'static) {
        let _ = self.listener.set(Box::new(listener));
    }

    /// Asks the worker to run a round soon (after a sale, a catalogue edit…).
    pub fn nudge(&self) {
        if self.enabled() {
            self.nudge.notify_one();
        }
    }

    pub async fn nudged(&self) {
        self.nudge.notified().await;
    }

    pub fn status(&self, db: &Database) -> SyncStatus {
        let conn = db.conn();
        let (pending, parked) = queue_counts(&conn).unwrap_or((0, 0));
        let last_synced_at = settings::get::<Timestamp>(&conn, LAST_SYNCED_KEY)
            .ok()
            .flatten();
        drop(conn);
        let runtime = lock(&self.runtime);
        SyncStatus {
            state: runtime.state,
            pending,
            parked,
            last_synced_at,
            last_error: runtime.last_error.clone(),
        }
    }

    fn set_state(&self, db: &Database, state: SyncState, last_error: Option<String>) {
        {
            let mut runtime = lock(&self.runtime);
            runtime.state = state;
            runtime.last_error = last_error;
        }
        if let Some(listener) = self.listener.get() {
            listener(&self.status(db));
        }
    }

    /// Runs one full round. `credentials: None` means the license is not
    /// currently valid, which is reported as an error without contacting
    /// the cloud. Blocking: call from a blocking thread.
    pub fn run(
        &self,
        db: &Database,
        credentials: Option<&SyncCredentials>,
    ) -> Result<SyncReport, SyncError> {
        let _round = lock(&self.round);
        self.attempted.store(true, Ordering::Relaxed);
        let Some(transport) = self.transport.clone() else {
            return Ok(self.report(db, false, PushOutcome::default(), 0));
        };
        let Some(credentials) = credentials else {
            let err = SyncError::Unauthorized("the license is not valid".into());
            self.set_state(db, SyncState::Error, Some(err.to_string()));
            return Err(err);
        };
        self.set_state(db, SyncState::Syncing, None);

        let result = self
            .push(db, transport.as_ref(), credentials)
            .and_then(|pushed| Ok((pushed, self.pull(db, transport.as_ref(), credentials)?)));
        match result {
            Ok((pushed, pulled)) => {
                let now = self.clock.now();
                settings::put(&db.conn(), LAST_SYNCED_KEY, &now, now).map_err(local)?;
                self.set_state(db, SyncState::Idle, None);
                Ok(self.report(db, true, pushed, pulled))
            }
            Err(SyncError::Offline(message)) => {
                self.set_state(db, SyncState::Offline, Some(message));
                Ok(self.report(db, false, PushOutcome::default(), 0))
            }
            Err(err) => {
                self.set_state(db, SyncState::Error, Some(err.to_string()));
                Err(err)
            }
        }
    }

    fn report(&self, db: &Database, online: bool, pushed: PushOutcome, pulled: u64) -> SyncReport {
        let status = self.status(db);
        SyncReport {
            online,
            pushed: pushed.acknowledged,
            rejected: pushed.rejected,
            pulled,
            pending: status.pending,
            last_synced_at: status.last_synced_at,
        }
    }

    fn push(
        &self,
        db: &Database,
        transport: &dyn SyncTransport,
        credentials: &SyncCredentials,
    ) -> Result<PushOutcome, SyncError> {
        let mut outcome = PushOutcome::default();
        for _ in 0..MAX_PUSH_BATCHES {
            let now = self.clock.now();
            let (device_id, batch) = {
                let conn = db.conn();
                (
                    device::id(&conn).map_err(local)?,
                    pending_batch(&conn, now).map_err(local)?,
                )
            };
            if batch.is_empty() {
                break;
            }
            let events = batch
                .iter()
                .map(|p| {
                    Ok(SyncEvent {
                        event_id: p.id,
                        device_id,
                        event_type: p.event_type.clone(),
                        entity_type: p.entity_type.clone(),
                        entity_id: p.entity_id,
                        payload: serde_json::from_str(&p.payload).map_err(local)?,
                        occurred_at: p.created_at,
                    })
                })
                .collect::<Result<Vec<_>, SyncError>>()?;
            let full = events.len() == PUSH_BATCH;
            let response = transport.push(
                credentials,
                &PushRequest {
                    protocol_version: PROTOCOL_VERSION,
                    device_id,
                    events,
                },
            )?;

            let now = self.clock.now();
            let mut conn = db.conn();
            let tx = conn.transaction().map_err(local)?;
            for event in &batch {
                if response.acknowledged.contains(&event.id) {
                    tx.execute(
                        "UPDATE sync_queue SET sent_at = ?2, updated_at = ?2, last_error = NULL WHERE id = ?1",
                        params![event.id.to_string(), now.to_string()],
                    )
                    .map_err(local)?;
                    outcome.acknowledged += 1;
                    continue;
                }
                let rejection = response.rejected.iter().find(|r| r.event_id == event.id);
                match rejection {
                    Some(r) if !r.retryable => {
                        tx.execute(
                            "UPDATE sync_queue SET deleted_at = ?2, updated_at = ?2, last_error = ?3 WHERE id = ?1",
                            params![event.id.to_string(), now.to_string(), r.reason],
                        )
                        .map_err(local)?;
                        outcome.rejected += 1;
                    }
                    _ => {
                        let reason = rejection
                            .map_or("not acknowledged by the server", |r| r.reason.as_str());
                        let attempts = event.attempt_count + 1;
                        let next = now.checked_add(backoff(attempts)).unwrap_or(now);
                        tx.execute(
                            "UPDATE sync_queue SET attempt_count = ?2, next_attempt_at = ?3, updated_at = ?4, last_error = ?5
                             WHERE id = ?1",
                            params![
                                event.id.to_string(),
                                attempts,
                                next.to_string(),
                                now.to_string(),
                                reason
                            ],
                        )
                        .map_err(local)?;
                    }
                }
            }
            tx.commit().map_err(local)?;
            if !full {
                break;
            }
        }
        Ok(outcome)
    }

    fn pull(
        &self,
        db: &Database,
        transport: &dyn SyncTransport,
        credentials: &SyncCredentials,
    ) -> Result<u64, SyncError> {
        let mut pulled = 0;
        let (device_id, mut cursor) = {
            let conn = db.conn();
            (
                device::id(&conn).map_err(local)?,
                settings::get::<String>(&conn, CURSOR_KEY).map_err(local)?,
            )
        };
        for _ in 0..MAX_PULL_PAGES {
            let page = transport.pull(
                credentials,
                &PullRequest {
                    protocol_version: PROTOCOL_VERSION,
                    device_id,
                    cursor: cursor.clone(),
                    limit: PULL_PAGE,
                },
            )?;
            let now = self.clock.now();
            let mut conn = db.conn();
            let tx = conn.transaction().map_err(local)?;
            let mut poisoned = None;
            for change in &page.changes {
                match apply::apply(&tx, change) {
                    Ok(Applied::Written) => pulled += 1,
                    Ok(Applied::Kept) => {}
                    // One bad row must not wedge sync for the whole shop:
                    // skip it and surface the error in the status.
                    Err(e) => poisoned = Some(e.to_string()),
                }
            }
            if let Some(next) = &page.next_cursor {
                settings::put(&tx, CURSOR_KEY, next, now).map_err(local)?;
            }
            tx.commit().map_err(local)?;
            drop(conn);
            if let Some(message) = poisoned {
                lock(&self.runtime).last_error = Some(message);
            }
            if page.next_cursor.is_some() {
                cursor = page.next_cursor;
            }
            if !page.has_more {
                break;
            }
        }
        Ok(pulled)
    }
}

fn pending_batch(conn: &Connection, now: Timestamp) -> rusqlite::Result<Vec<Pending>> {
    let limit = i64::try_from(PUSH_BATCH).unwrap_or(500);
    let mut stmt = conn.prepare(
        "SELECT id, event_type, entity_type, entity_id, payload, created_at, attempt_count
         FROM sync_queue
         WHERE sent_at IS NULL AND deleted_at IS NULL
           AND (next_attempt_at IS NULL OR next_attempt_at <= ?1)
         ORDER BY created_at, id
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![now.to_string(), limit], |r| {
        Ok(Pending {
            id: crate::repo::uuid_at(r, 0)?,
            event_type: r.get(1)?,
            entity_type: r.get(2)?,
            entity_id: crate::repo::uuid_at(r, 3)?,
            payload: r.get(4)?,
            created_at: crate::repo::ts_at(r, 5)?,
            attempt_count: r.get(6)?,
        })
    })?;
    rows.collect()
}

/// `(pending, parked)` outbox counts.
pub fn queue_counts(conn: &Connection) -> rusqlite::Result<(u64, u64)> {
    let (pending, parked): (i64, i64) = conn
        .query_row(
            "SELECT
               COALESCE(SUM(sent_at IS NULL AND deleted_at IS NULL), 0),
               COALESCE(SUM(sent_at IS NULL AND deleted_at IS NOT NULL), 0)
             FROM sync_queue",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .unwrap_or((0, 0));
    Ok((
        u64::try_from(pending).unwrap_or(0),
        u64::try_from(parked).unwrap_or(0),
    ))
}
