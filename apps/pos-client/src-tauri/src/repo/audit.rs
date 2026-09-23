//! Append-only audit trail: who (user + role at the time), what, and the
//! entity before/after.

use pos_core::rbac::Role;
use pos_core::time::Timestamp;
use rusqlite::{params, Connection};
use serde::Serialize;
use uuid::Uuid;

use super::outbox::{self, EventType};
use super::{enum_str, Meta};

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    #[serde(flatten)]
    pub meta: Meta,
    pub user_id: Uuid,
    pub role: Role,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<Uuid>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub device_id: Uuid,
    pub occurred_at: Timestamp,
}

pub struct Actor {
    pub user_id: Uuid,
    pub role: Role,
    pub device_id: Uuid,
}

#[allow(clippy::too_many_arguments)]
pub fn record(
    conn: &Connection,
    actor: &Actor,
    action: &str,
    entity_type: &str,
    entity_id: Option<Uuid>,
    before: Option<serde_json::Value>,
    after: Option<serde_json::Value>,
    now: Timestamp,
) -> rusqlite::Result<()> {
    let entry = AuditEntry {
        meta: Meta::new(now),
        user_id: actor.user_id,
        role: actor.role,
        action: action.to_owned(),
        entity_type: entity_type.to_owned(),
        entity_id,
        before,
        after,
        device_id: actor.device_id,
        occurred_at: now,
    };
    let json = |v: &Option<serde_json::Value>| v.as_ref().map(serde_json::Value::to_string);
    conn.execute(
        "INSERT INTO audit_log (id, created_at, updated_at, user_id, role, action, entity_type,
                                entity_id, before, after, device_id, occurred_at)
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?2)",
        params![
            entry.meta.id.to_string(),
            now.to_string(),
            actor.user_id.to_string(),
            enum_str(&actor.role),
            action,
            entity_type,
            entity_id.map(|id| id.to_string()),
            json(&entry.before),
            json(&entry.after),
            actor.device_id.to_string(),
        ],
    )?;
    outbox::record(
        conn,
        "audit_log",
        EventType::Append,
        entry.meta.id,
        &entry,
        now,
    )
}
