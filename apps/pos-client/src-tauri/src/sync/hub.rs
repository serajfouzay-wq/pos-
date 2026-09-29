//! The shop-network hub: one till keeps the shop's shared state and the
//! other tills sync with it over the local network, with no internet.
//!
//! It answers the same push/pull protocol as the cloud (`sync_push` /
//! `sync_pull` in `supabase/migrations/20260924000000_sync.sql`) with the
//! same rules: events are applied once (`hub_events`), last-write-wins rows
//! keep the version with the greater `(updated_at, event_id)`, append-only
//! and delta rows are inserted once, derived aggregates are never taken from
//! a till, and every stored version gets the next change number (`seq`) so
//! a till pulls what it has not seen, never its own versions.
//!
//! The hub is also a till: its own changes reach `hub_rows` through the
//! same push (see [`super::lan::LocalHubTransport`]), and the other tills'
//! changes reach its tables through the same pull as everyone else's.

use pos_core::time::Timestamp;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value as Json;
use uuid::Uuid;

use super::apply::{self, Strategy, ENTITIES};
use super::protocol::{Change, PullRequest, PullResponse, PushRequest, PushResponse, Rejected};

#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error("hub storage: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("bad request: {0}")]
    Invalid(String),
}

/// Aggregates the hub keeps itself (as the cloud does): never a till's cache.
const DERIVED: [&str; 2] = ["stock_on_hand_milli", "loyalty_points"];

fn next_seq(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM hub_rows", [], |r| {
        r.get(0)
    })
}

fn updated_at(row: &Json) -> String {
    row["updated_at"].as_str().unwrap_or_default().to_owned()
}

/// Applies one till's batch. Runs in one SQLite transaction (the caller's).
pub fn push(
    conn: &Connection,
    request: &PushRequest,
    now: Timestamp,
) -> Result<PushResponse, HubError> {
    let mut acknowledged = Vec::with_capacity(request.events.len());
    let mut rejected = Vec::new();
    for event in &request.events {
        let seen: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM hub_events WHERE id = ?1)",
            [event.event_id.to_string()],
            |r| r.get(0),
        )?;
        if seen {
            acknowledged.push(event.event_id);
            continue;
        }
        let Some(strategy) = apply::strategy(&event.entity_type) else {
            rejected.push(Rejected {
                event_id: event.event_id,
                reason: format!("unknown entity {}", event.entity_type),
                retryable: false,
            });
            continue;
        };
        if !event.payload.is_object() {
            rejected.push(Rejected {
                event_id: event.event_id,
                reason: "the row is not an object".into(),
                retryable: false,
            });
            continue;
        }
        let mut row = event.payload.clone();
        for derived in DERIVED {
            if let Some(v) = row.get_mut(derived) {
                *v = Json::from(0);
            }
        }
        let current: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT row, event_id FROM hub_rows WHERE entity_type = ?1 AND entity_id = ?2",
                params![event.entity_type, event.entity_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let wins = match (strategy, &current) {
            (_, None) => true,
            (Strategy::LastWriteWins, Some((stored, stored_event))) => {
                let stored: Json = serde_json::from_str(stored).unwrap_or(Json::Null);
                (updated_at(&row), event.event_id.to_string())
                    > (
                        updated_at(&stored),
                        stored_event.clone().unwrap_or_default(),
                    )
            }
            (Strategy::AppendOnly | Strategy::AdditiveDelta, Some(_)) => false,
        };
        if wins {
            let seq = next_seq(conn)?;
            let lww_event =
                (strategy == Strategy::LastWriteWins).then(|| event.event_id.to_string());
            conn.execute(
                "INSERT INTO hub_rows (id, created_at, updated_at, entity_type, entity_id, row, seq,
                                       origin_device_id, event_id)
                 VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (entity_type, entity_id) DO UPDATE SET
                   updated_at = excluded.updated_at, row = excluded.row, seq = excluded.seq,
                   origin_device_id = excluded.origin_device_id, event_id = excluded.event_id",
                params![
                    Uuid::now_v7().to_string(),
                    now.to_string(),
                    event.entity_type,
                    event.entity_id.to_string(),
                    row.to_string(),
                    seq,
                    request.device_id.to_string(),
                    lww_event,
                ],
            )?;
        }
        conn.execute(
            "INSERT INTO hub_events (id, created_at, updated_at, device_id) VALUES (?1, ?2, ?2, ?3)",
            params![
                event.event_id.to_string(),
                now.to_string(),
                request.device_id.to_string()
            ],
        )?;
        acknowledged.push(event.event_id);
    }
    Ok(PushResponse {
        acknowledged,
        rejected,
        server_time: now,
    })
}

/// The versions after `cursor` a till has not written itself.
pub fn pull(conn: &Connection, request: &PullRequest) -> Result<PullResponse, HubError> {
    let cursor: i64 = match request.cursor.as_deref() {
        None | Some("") => 0,
        Some(c) => c
            .parse()
            .map_err(|_| HubError::Invalid(format!("bad cursor {c}")))?,
    };
    let limit = i64::from(request.limit.clamp(1, 1000));
    let mut stmt = conn.prepare(
        "SELECT entity_type, row, seq, origin_device_id, event_id FROM hub_rows
         WHERE seq > ?1 ORDER BY seq LIMIT ?2",
    )?;
    let rows: Vec<(String, String, i64, String, Option<String>)> = stmt
        .query_map(params![cursor, limit + 1], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<Result<_, _>>()?;
    let has_more = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
    let mut next = cursor;
    let mut changes = Vec::new();
    let me = request.device_id.to_string();
    for (entity_type, row, seq, origin, event_id) in rows.into_iter().take(limit as usize) {
        next = seq;
        if origin == me {
            continue;
        }
        changes.push(Change {
            entity_type,
            row: serde_json::from_str(&row).map_err(|e| HubError::Invalid(e.to_string()))?,
            event_id: event_id.and_then(|e| Uuid::parse_str(&e).ok()),
        });
    }
    Ok(PullResponse {
        changes,
        next_cursor: Some(next.to_string()),
        has_more,
    })
}

/// Seeds an empty hub with every synced row this till already holds (a till
/// that becomes the hub after selling on its own, or after syncing with the
/// cloud), so the other tills start from the shop's data.
pub fn seed(conn: &Connection, device_id: Uuid, now: Timestamp) -> Result<u64, HubError> {
    let empty: bool = conn.query_row("SELECT NOT EXISTS (SELECT 1 FROM hub_rows)", [], |r| {
        r.get(0)
    })?;
    if !empty {
        return Ok(0);
    }
    let mut seeded = 0;
    for (entity, strategy) in ENTITIES {
        let rows: Vec<Json> =
            crate::repo::rows::select(conn, &format!("SELECT * FROM {entity}"), [])?;
        for row in rows {
            let Some(id) = row["id"].as_str().map(str::to_owned) else {
                continue;
            };
            let seq = next_seq(conn)?;
            let event = (*strategy == Strategy::LastWriteWins).then(|| Uuid::now_v7().to_string());
            conn.execute(
                "INSERT INTO hub_rows (id, created_at, updated_at, entity_type, entity_id, row, seq,
                                       origin_device_id, event_id)
                 VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (entity_type, entity_id) DO NOTHING",
                params![
                    Uuid::now_v7().to_string(),
                    now.to_string(),
                    entity,
                    id,
                    row.to_string(),
                    seq,
                    device_id.to_string(),
                    event,
                ],
            )?;
            seeded += 1;
        }
    }
    // Everything this till had queued is now in the hub: mark it sent so
    // its own push does not offer it again.
    conn.execute(
        "UPDATE sync_queue SET sent_at = ?1, updated_at = ?1 WHERE sent_at IS NULL AND deleted_at IS NULL",
        [now.to_string()],
    )?;
    Ok(seeded)
}

/// How many rows the hub holds, and the tills that pushed to it.
pub fn stats(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    conn.query_row(
        "SELECT (SELECT count(*) FROM hub_rows), (SELECT count(DISTINCT device_id) FROM hub_events)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}
