//! Persistence for the license gate: the token file and the `license` /
//! `device` rows.

use std::io::{self, Write};
use std::path::Path;

use pos_core::time::Timestamp;
use pos_license::verify::VerifiedLicense;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub fn read_token(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(token) => Ok(Some(token.trim().to_owned())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Write-then-rename so a crash never leaves a half-written token.
pub fn write_token(path: &Path, token: &str) -> io::Result<()> {
    let tmp = path.with_extension("jwt.tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(token.trim().as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// The license row's mutable bookkeeping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LicenseBookkeeping {
    pub last_seen_at: Option<Timestamp>,
    pub revoked_at: Option<Timestamp>,
    pub clock_high_water_at: Option<Timestamp>,
}

fn parse_ts(value: Option<String>) -> Option<Timestamp> {
    value.and_then(|v| v.parse().ok())
}

/// Ensures the single `device` row exists.
pub fn ensure_device(conn: &Connection, name: &str, now: Timestamp) -> rusqlite::Result<Uuid> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM device WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = existing.and_then(|id| Uuid::parse_str(&id).ok()) {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    conn.execute(
        "INSERT INTO device (id, created_at, updated_at, name) VALUES (?1, ?2, ?2, ?3)",
        params![id.to_string(), now.to_string(), name],
    )?;
    Ok(id)
}

/// Records the verified token as the active license (soft-deleting any
/// previous one) and returns its bookkeeping.
pub fn upsert_active_license(
    conn: &mut Connection,
    token: &str,
    verified: &VerifiedLicense,
    now: Timestamp,
) -> rusqlite::Result<LicenseBookkeeping> {
    let tx = conn.transaction()?;
    let license_id = verified.claims.jti.to_string();
    let now_s = now.to_string();
    tx.execute(
        "INSERT INTO license (id, created_at, updated_at, license_id, client_id, token,
                              fingerprint_hash, issued_at, expires_at)
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (license_id) DO UPDATE SET deleted_at = NULL, updated_at = ?2
           WHERE license.deleted_at IS NOT NULL",
        params![
            Uuid::now_v7().to_string(),
            now_s,
            license_id,
            verified.claims.sub.to_string(),
            token,
            verified.claims.fp,
            verified.issued_at.to_string(),
            verified.expires_at.map(|t| t.to_string()),
        ],
    )?;
    tx.execute(
        "UPDATE license SET deleted_at = ?1, updated_at = ?1
         WHERE license_id <> ?2 AND deleted_at IS NULL",
        params![now_s, license_id],
    )?;
    let bookkeeping = tx.query_row(
        "SELECT last_seen_at, revoked_at, clock_high_water_at FROM license WHERE license_id = ?1",
        [&license_id],
        |row| {
            Ok(LicenseBookkeeping {
                last_seen_at: parse_ts(row.get(0)?),
                revoked_at: parse_ts(row.get(1)?),
                clock_high_water_at: parse_ts(row.get(2)?),
            })
        },
    )?;
    tx.commit()?;
    Ok(bookkeeping)
}

/// Raises the clock high-water mark (never lowers it).
pub fn raise_high_water(
    conn: &Connection,
    license_id: Uuid,
    now: Timestamp,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE license SET clock_high_water_at = ?1, updated_at = ?1
         WHERE license_id = ?2 AND (clock_high_water_at IS NULL OR clock_high_water_at < ?1)",
        params![now.to_string(), license_id.to_string()],
    )?;
    Ok(())
}

/// Applies a cloud verdict. Server time is trusted over the local clock, so it
/// also *resets* the high-water mark (a clock that ran ahead is forgiven).
pub fn record_cloud_verdict(
    conn: &Connection,
    license_id: Uuid,
    server_time: Timestamp,
    revoked: bool,
) -> rusqlite::Result<()> {
    let t = server_time.to_string();
    if revoked {
        conn.execute(
            "UPDATE license SET revoked_at = COALESCE(revoked_at, ?1), updated_at = ?1,
                                clock_high_water_at = ?1
             WHERE license_id = ?2",
            params![t, license_id.to_string()],
        )?;
    } else {
        conn.execute(
            "UPDATE license SET last_seen_at = ?1, revoked_at = NULL, updated_at = ?1,
                                clock_high_water_at = ?1
             WHERE license_id = ?2",
            params![t, license_id.to_string()],
        )?;
    }
    Ok(())
}
