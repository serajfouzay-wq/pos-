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

/// Mirrors `AuditFilterSchema`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct AuditFilter {
    pub from: Option<Timestamp>,
    pub to: Option<Timestamp>,
    /// Exact action, or a prefix ending in `.` (`sale.`).
    pub action: Option<String>,
    pub user_id: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// Mirrors `AuditEntryViewSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct AuditEntryView {
    #[serde(flatten)]
    pub entry: AuditEntry,
    pub user_name: Option<String>,
}

/// Mirrors `AuditPageSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct AuditPage {
    pub entries: Vec<AuditEntryView>,
    pub total: i64,
    pub actions: Vec<String>,
}

pub fn list(conn: &Connection, filter: &AuditFilter) -> rusqlite::Result<AuditPage> {
    use super::{enum_at, opt_ts_at, opt_uuid_at, ts_at, uuid_at};
    let action = filter
        .action
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty());
    let (exact, prefix) = match action {
        Some(a) if a.ends_with('.') => (None, Some(format!("{a}%"))),
        Some(a) => (Some(a.to_owned()), None),
        None => (None, None),
    };
    let condition = "(?1 IS NULL OR a.occurred_at >= ?1) AND (?2 IS NULL OR a.occurred_at < ?2)
        AND (?3 IS NULL OR a.action = ?3) AND (?4 IS NULL OR a.action LIKE ?4)
        AND (?5 IS NULL OR a.user_id = ?5)";
    let args = params![
        filter.from.map(|t| t.to_string()),
        filter.to.map(|t| t.to_string()),
        exact,
        prefix,
        filter.user_id.map(|u| u.to_string()),
    ];
    let total: i64 = conn.query_row(
        &format!("SELECT count(*) FROM audit_log a WHERE {condition}"),
        args,
        |r| r.get(0),
    )?;
    let json = |text: Option<String>| text.and_then(|t| serde_json::from_str(&t).ok());
    let entries = conn
        .prepare(&format!(
            "SELECT a.id, a.created_at, a.updated_at, a.deleted_at, a.user_id, a.role, a.action,
                    a.entity_type, a.entity_id, a.before, a.after, a.device_id, a.occurred_at, u.display_name
             FROM audit_log a LEFT JOIN users u ON u.id = a.user_id
             WHERE {condition}
             ORDER BY a.occurred_at DESC, a.id DESC LIMIT ?6 OFFSET ?7"
        ))?
        .query_map(
            params![
                filter.from.map(|t| t.to_string()),
                filter.to.map(|t| t.to_string()),
                exact,
                prefix,
                filter.user_id.map(|u| u.to_string()),
                filter.limit.clamp(1, 500),
                filter.offset.max(0),
            ],
            |r| {
                Ok(AuditEntryView {
                    entry: AuditEntry {
                        meta: Meta {
                            id: uuid_at(r, 0)?,
                            created_at: ts_at(r, 1)?,
                            updated_at: ts_at(r, 2)?,
                            deleted_at: opt_ts_at(r, 3)?,
                        },
                        user_id: uuid_at(r, 4)?,
                        role: enum_at(r, 5)?,
                        action: r.get(6)?,
                        entity_type: r.get(7)?,
                        entity_id: opt_uuid_at(r, 8)?,
                        before: json(r.get(9)?),
                        after: json(r.get(10)?),
                        device_id: uuid_at(r, 11)?,
                        occurred_at: ts_at(r, 12)?,
                    },
                    user_name: r.get(13)?,
                })
            },
        )?
        .collect::<Result<_, _>>()?;
    let actions = conn
        .prepare("SELECT DISTINCT action FROM audit_log ORDER BY action")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(AuditPage {
        entries,
        total,
        actions,
    })
}
