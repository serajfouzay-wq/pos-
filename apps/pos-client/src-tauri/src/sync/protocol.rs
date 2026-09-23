//! Wire types of the sync protocol. Mirror `@pos/shared` sync.ts
//! (`SyncPushRequestSchema`, `SyncPullResponseSchema`, …).

use pos_core::time::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u8 = 1;
pub const PUSH_BATCH: usize = 500;
pub const PULL_PAGE: u32 = 500;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncEvent {
    pub event_id: Uuid,
    pub device_id: Uuid,
    pub event_type: String,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub payload: serde_json::Value,
    pub occurred_at: Timestamp,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushRequest {
    pub protocol_version: u8,
    pub device_id: Uuid,
    pub events: Vec<SyncEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Rejected {
    pub event_id: Uuid,
    pub reason: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PushResponse {
    pub acknowledged: Vec<Uuid>,
    pub rejected: Vec<Rejected>,
    pub server_time: Timestamp,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullRequest {
    pub protocol_version: u8,
    pub device_id: Uuid,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Change {
    pub entity_type: String,
    pub row: serde_json::Value,
    /// Tie-breaker of the server's LWW version; `None` for append-only rows.
    pub event_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PullResponse {
    pub changes: Vec<Change>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}
