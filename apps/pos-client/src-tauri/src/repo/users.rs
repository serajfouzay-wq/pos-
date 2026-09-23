//! Users and PIN verification.
//!
//! PINs are hashed with Argon2id (OWASP parameters: 19 MiB, t=2, p=1). A PIN
//! has little entropy, so the real defences are the encrypted database and
//! the lockout: 5 consecutive failures lock the user for 5 minutes.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use chrono::Duration;
use pos_core::rbac::Role;
use pos_core::time::Timestamp;
use rand::rngs::OsRng;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

use super::outbox::{self, EventType};
use super::{enum_at, enum_str, opt_ts_at, Meta, META_COLUMNS};

pub const MAX_FAILED_ATTEMPTS: i64 = 5;
pub const LOCKOUT: Duration = Duration::minutes(5);

fn argon2() -> Argon2<'static> {
    let params = Params::new(19 * 1024, 2, 1, None).expect("valid Argon2 parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn is_valid_pin(pin: &str) -> bool {
    (4..=6).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

pub fn hash_pin(pin: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    argon2()
        .hash_password(pin.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

fn verify_pin(pin: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| argon2().verify_password(pin.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

/// Full row, as stored and synced (includes `pin_hash`).
#[derive(Debug, Clone, Serialize)]
pub struct UserRow {
    #[serde(flatten)]
    pub meta: Meta,
    pub display_name: String,
    pub role: Role,
    pub pin_hash: String,
    pub is_active: bool,
    pub failed_pin_attempts: i64,
    pub locked_until: Option<Timestamp>,
}

/// What the UI may see — mirrors `UserSchema` (no hash, no attempt counter).
#[derive(Debug, Clone, Serialize)]
pub struct PublicUser {
    #[serde(flatten)]
    pub meta: Meta,
    pub display_name: String,
    pub role: Role,
    pub is_active: bool,
    pub locked_until: Option<Timestamp>,
}

impl From<&UserRow> for PublicUser {
    fn from(row: &UserRow) -> Self {
        Self {
            meta: row.meta.clone(),
            display_name: row.display_name.clone(),
            role: row.role,
            is_active: row.is_active,
            locked_until: row.locked_until,
        }
    }
}

const COLUMNS: &str = "display_name, role, pin_hash, is_active, failed_pin_attempts, locked_until";

fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserRow> {
    Ok(UserRow {
        meta: Meta::read(row, 0)?,
        display_name: row.get(4)?,
        role: enum_at(row, 5)?,
        pin_hash: row.get(6)?,
        is_active: row.get(7)?,
        failed_pin_attempts: row.get(8)?,
        locked_until: opt_ts_at(row, 9)?,
    })
}

pub fn count_active(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM users WHERE deleted_at IS NULL AND is_active = 1",
        [],
        |r| r.get(0),
    )
}

pub fn list(conn: &Connection, active_only: bool) -> rusqlite::Result<Vec<UserRow>> {
    let sql = format!(
        "SELECT {META_COLUMNS}, {COLUMNS} FROM users WHERE deleted_at IS NULL {}
         ORDER BY CASE role WHEN 'owner' THEN 0 WHEN 'manager' THEN 1 ELSE 2 END, display_name",
        if active_only { "AND is_active = 1" } else { "" }
    );
    conn.prepare(&sql)?.query_map([], read)?.collect()
}

pub fn get(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<UserRow>> {
    conn.query_row(
        &format!(
            "SELECT {META_COLUMNS}, {COLUMNS} FROM users WHERE id = ?1 AND deleted_at IS NULL"
        ),
        [id.to_string()],
        read,
    )
    .optional()
}

fn persist(conn: &Connection, user: &UserRow, now: Timestamp) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO users (id, created_at, updated_at, deleted_at, display_name, role, pin_hash,
                            is_active, failed_pin_attempts, locked_until)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT (id) DO UPDATE SET updated_at = excluded.updated_at, deleted_at = excluded.deleted_at,
           display_name = excluded.display_name, role = excluded.role, pin_hash = excluded.pin_hash,
           is_active = excluded.is_active, failed_pin_attempts = excluded.failed_pin_attempts,
           locked_until = excluded.locked_until",
        params![
            user.meta.id.to_string(),
            user.meta.created_at.to_string(),
            user.meta.updated_at.to_string(),
            user.meta.deleted_at.map(|t| t.to_string()),
            user.display_name,
            enum_str(&user.role),
            user.pin_hash,
            user.is_active,
            user.failed_pin_attempts,
            user.locked_until.map(|t| t.to_string()),
        ],
    )?;
    outbox::record(conn, "users", EventType::Upsert, user.meta.id, user, now)
}

pub fn create(
    conn: &Connection,
    display_name: &str,
    role: Role,
    pin_hash: String,
    now: Timestamp,
) -> rusqlite::Result<UserRow> {
    let user = UserRow {
        meta: Meta::new(now),
        display_name: display_name.trim().to_owned(),
        role,
        pin_hash,
        is_active: true,
        failed_pin_attempts: 0,
        locked_until: None,
    };
    persist(conn, &user, now)?;
    Ok(user)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginOutcome {
    Success(Box<UserRowSummary>),
    WrongPin { attempts_left: i64 },
    Locked { until: Timestamp },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRowSummary {
    pub id: Uuid,
    pub display_name: String,
    pub role: Role,
}

/// Verifies a PIN and updates the failure counter / lockout atomically.
pub fn attempt_login(
    conn: &Connection,
    user_id: Uuid,
    pin: &str,
    now: Timestamp,
) -> rusqlite::Result<LoginOutcome> {
    let Some(mut user) = get(conn, user_id)? else {
        return Ok(LoginOutcome::Unknown);
    };
    if !user.is_active {
        return Ok(LoginOutcome::Unknown);
    }
    if let Some(until) = user.locked_until.filter(|until| *until > now) {
        return Ok(LoginOutcome::Locked { until });
    }
    if verify_pin(pin, &user.pin_hash) {
        if user.failed_pin_attempts != 0 || user.locked_until.is_some() {
            user.failed_pin_attempts = 0;
            user.locked_until = None;
            user.meta.updated_at = now;
            persist(conn, &user, now)?;
        }
        return Ok(LoginOutcome::Success(Box::new(UserRowSummary {
            id: user.meta.id,
            display_name: user.display_name,
            role: user.role,
        })));
    }
    user.failed_pin_attempts += 1;
    user.meta.updated_at = now;
    let outcome = if user.failed_pin_attempts >= MAX_FAILED_ATTEMPTS {
        let until = now.checked_add(LOCKOUT).unwrap_or(now);
        user.locked_until = Some(until);
        user.failed_pin_attempts = 0;
        LoginOutcome::Locked { until }
    } else {
        LoginOutcome::WrongPin {
            attempts_left: MAX_FAILED_ATTEMPTS - user.failed_pin_attempts,
        }
    };
    persist(conn, &user, now)?;
    Ok(outcome)
}
