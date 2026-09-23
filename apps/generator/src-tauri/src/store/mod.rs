//! The generator's workspace database: clients, their uploaded assets, the
//! licenses signed for them and their builds.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use pos_core::config::{BusinessType, ClientConfig};
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use rusqlite::types::Type;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("0001_init.sql"))];

pub trait SqlResultExt<T> {
    fn ipc(self) -> IpcResult<T>;
}

impl<T> SqlResultExt<T> for rusqlite::Result<T> {
    fn ipc(self) -> IpcResult<T> {
        self.map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                IpcError::new(IpcErrorCode::NotFound, "Not found.")
            }
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                IpcError::new(IpcErrorCode::Conflict, "That conflicts with existing data.")
            }
            other => IpcError::internal(format!("storage error: {other}")),
        })
    }
}

fn conversion(i: usize, e: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(i, Type::Text, Box::new(e))
}

fn uuid_at(row: &Row<'_>, i: usize) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&row.get::<_, String>(i)?).map_err(|e| conversion(i, e))
}

fn ts_at(row: &Row<'_>, i: usize) -> rusqlite::Result<Timestamp> {
    row.get::<_, String>(i)?
        .parse()
        .map_err(|e| conversion(i, e))
}

fn opt_ts_at(row: &Row<'_>, i: usize) -> rusqlite::Result<Option<Timestamp>> {
    row.get::<_, Option<String>>(i)?
        .map(|s| s.parse().map_err(|e| conversion(i, e)))
        .transpose()
}

fn u64_at(row: &Row<'_>, i: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(i)?).map_err(|e| conversion(i, e))
}

fn opt_u64_at(row: &Row<'_>, i: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(i)?
        .map(|v| u64::try_from(v).map_err(|e| conversion(i, e)))
        .transpose()
}

fn opt_i64(value: Option<u64>) -> Option<i64> {
    value.and_then(|v| i64::try_from(v).ok())
}

fn enum_at<T: DeserializeOwned>(row: &Row<'_>, i: usize) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(row.get(i)?)).map_err(|e| conversion(i, e))
}

fn enum_str<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

// ── Records (mirror the schemas in @pos/shared generator-contract.ts) ─────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    ReceiptLogo,
    AppIcon,
}

impl AssetKind {
    /// File name inside the client's folder in the build repository (and, for
    /// the logo, inside the POS's bundled `client-assets/`).
    pub fn file_name(self) -> &'static str {
        match self {
            AssetKind::ReceiptLogo => "receipt-logo.png",
            AssetKind::AppIcon => "app-icon.png",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetInfo {
    pub kind: AssetKind,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub byte_length: u64,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClientDetail {
    pub client_id: Uuid,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub config: ClientConfig,
    pub notes: String,
    pub assets: Vec<AssetInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClientSummary {
    pub client_id: Uuid,
    pub client_slug: String,
    pub display_name: String,
    pub business_type: BusinessType,
    pub updated_at: Timestamp,
    pub has_receipt_logo: bool,
    pub has_app_icon: bool,
    pub license_count: u64,
    pub last_build: Option<BuildRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IssuedLicenseRecord {
    pub license_id: Uuid,
    pub client_id: Uuid,
    pub device_name: String,
    pub fingerprint_hash: String,
    pub max_devices: u32,
    pub issued_at: Timestamp,
    pub expires_at: Option<Timestamp>,
    pub token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildStatus {
    /// Committing the client's files to the build repository.
    Publishing,
    /// Workflow dispatched; waiting for a runner.
    Queued,
    InProgress,
    Succeeded,
    Failed,
    Cancelled,
    /// The generator could not publish or dispatch (see `message`).
    Error,
}

impl BuildStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Publishing | Self::Queued | Self::InProgress)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildRecord {
    pub build_id: Uuid,
    pub client_id: Uuid,
    pub client_slug: String,
    pub status: BuildStatus,
    pub config_sha256: String,
    pub app_version: String,
    pub commit_sha: Option<String>,
    pub run_id: Option<u64>,
    pub run_url: Option<String>,
    pub artifact_id: Option<u64>,
    pub artifact_name: Option<String>,
    pub artifact_size: Option<u64>,
    pub download_path: Option<String>,
    pub message: Option<String>,
    pub requested_at: Timestamp,
    pub updated_at: Timestamp,
    pub completed_at: Option<Timestamp>,
}

/// Fields a build update may change (`None` = leave as is).
#[derive(Debug, Clone, Default)]
pub struct BuildUpdate {
    pub status: Option<BuildStatus>,
    pub commit_sha: Option<String>,
    pub run_id: Option<u64>,
    pub run_url: Option<String>,
    pub artifact_id: Option<u64>,
    pub artifact_name: Option<String>,
    pub artifact_size: Option<u64>,
    pub download_path: Option<String>,
    pub message: Option<String>,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> IpcResult<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| IpcError::internal(format!("create {}: {e}", dir.display())))?;
        }
        Self::setup(Connection::open(path).ipc()?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> IpcResult<Self> {
        Self::setup(Connection::open_in_memory().ipc()?)
    }

    fn setup(mut conn: Connection) -> IpcResult<Self> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .ipc()?;
        let current: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .ipc()?;
        for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
            let tx = conn.transaction().ipc()?;
            tx.execute_batch(sql).ipc()?;
            tx.pragma_update(None, "user_version", version).ipc()?;
            tx.commit().ipc()?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ── Clients ─────────────────────────────────────────────────────────

    fn slug_taken(conn: &Connection, slug: &str, except: Option<Uuid>) -> IpcResult<bool> {
        let other: Option<String> = conn
            .query_row(
                "SELECT id FROM clients WHERE slug = ?1 AND deleted_at IS NULL",
                [slug],
                |r| r.get(0),
            )
            .optional()
            .ipc()?;
        Ok(other.is_some_and(|id| Some(id) != except.map(|e| e.to_string())))
    }

    pub fn create_client(&self, config: &ClientConfig, now: Timestamp) -> IpcResult<ClientDetail> {
        config
            .validate()
            .map_err(|e| IpcError::validation(e.to_string()))?;
        let conn = self.conn();
        if Self::slug_taken(&conn, &config.client_slug, None)? {
            return Err(IpcError::new(
                IpcErrorCode::Conflict,
                format!(
                    "Another client already uses the slug “{}”.",
                    config.client_slug
                ),
            ));
        }
        conn.execute(
            "INSERT INTO clients (id, created_at, updated_at, slug, display_name, config)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5)",
            params![
                config.client_id.to_string(),
                now.to_string(),
                config.client_slug,
                config.display_name,
                to_json(config)?,
            ],
        )
        .ipc()?;
        Self::detail(&conn, config.client_id)
    }

    pub fn client(&self, client_id: Uuid) -> IpcResult<ClientDetail> {
        Self::detail(&self.conn(), client_id)
    }

    fn detail(conn: &Connection, client_id: Uuid) -> IpcResult<ClientDetail> {
        let (created_at, updated_at, config, notes) = conn
            .query_row(
                "SELECT created_at, updated_at, config, notes FROM clients
                 WHERE id = ?1 AND deleted_at IS NULL",
                [client_id.to_string()],
                |r| {
                    Ok((
                        ts_at(r, 0)?,
                        ts_at(r, 1)?,
                        r.get::<_, String>(2)?,
                        r.get(3)?,
                    ))
                },
            )
            .ipc()?;
        Ok(ClientDetail {
            client_id,
            created_at,
            updated_at,
            config: serde_json::from_str(&config)
                .map_err(|e| IpcError::internal(format!("stored config: {e}")))?,
            notes,
            assets: Self::assets(conn, client_id)?,
        })
    }

    pub fn list_clients(&self) -> IpcResult<Vec<ClientSummary>> {
        let conn = self.conn();
        let ids: Vec<Uuid> = conn
            .prepare(
                "SELECT id FROM clients WHERE deleted_at IS NULL ORDER BY display_name COLLATE NOCASE",
            )
            .ipc()?
            .query_map([], |r| uuid_at(r, 0))
            .ipc()?
            .collect::<rusqlite::Result<_>>()
            .ipc()?;
        ids.into_iter()
            .map(|id| {
                let detail = Self::detail(&conn, id)?;
                let license_count: i64 = conn
                    .query_row(
                        "SELECT count(*) FROM issued_licenses WHERE client_id = ?1 AND deleted_at IS NULL",
                        [id.to_string()],
                        |r| r.get(0),
                    )
                    .ipc()?;
                let has = |kind| detail.assets.iter().any(|a| a.kind == kind);
                Ok(ClientSummary {
                    client_id: id,
                    client_slug: detail.config.client_slug.clone(),
                    display_name: detail.config.display_name.clone(),
                    business_type: detail.config.business_type,
                    updated_at: detail.updated_at,
                    has_receipt_logo: has(AssetKind::ReceiptLogo),
                    has_app_icon: has(AssetKind::AppIcon),
                    license_count: u64::try_from(license_count).unwrap_or(0),
                    last_build: Self::builds_where(&conn, Some(id), 1)?.into_iter().next(),
                })
            })
            .collect()
    }

    /// Saves an edited config. The client id and slug are fixed at creation
    /// (the slug names the installer, the app identifier and the tills' data
    /// folder), and `receipt.logo_asset` always reflects the uploaded logo.
    pub fn save_client(
        &self,
        client_id: Uuid,
        mut config: ClientConfig,
        notes: &str,
        now: Timestamp,
    ) -> IpcResult<ClientDetail> {
        if notes.chars().count() > 2000 {
            return Err(IpcError::validation(
                "Notes are limited to 2000 characters.",
            ));
        }
        let conn = self.conn();
        let current = Self::detail(&conn, client_id)?;
        if config.client_id != client_id {
            return Err(IpcError::validation("The client id cannot be changed."));
        }
        if config.client_slug != current.config.client_slug {
            return Err(IpcError::validation(
                "The slug cannot be changed after the client is created.",
            ));
        }
        config.receipt.logo_asset = current
            .assets
            .iter()
            .any(|a| a.kind == AssetKind::ReceiptLogo)
            .then(|| AssetKind::ReceiptLogo.file_name().to_owned());
        config
            .validate()
            .map_err(|e| IpcError::validation(e.to_string()))?;
        conn.execute(
            "UPDATE clients SET config = ?2, notes = ?3, display_name = ?4, updated_at = ?5
             WHERE id = ?1 AND deleted_at IS NULL",
            params![
                client_id.to_string(),
                to_json(&config)?,
                notes,
                config.display_name,
                now.to_string()
            ],
        )
        .ipc()?;
        Self::detail(&conn, client_id)
    }

    pub fn archive_client(&self, client_id: Uuid, now: Timestamp) -> IpcResult<()> {
        let changed = self
            .conn()
            .execute(
                "UPDATE clients SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
                params![client_id.to_string(), now.to_string()],
            )
            .ipc()?;
        if changed == 0 {
            return Err(IpcError::new(IpcErrorCode::NotFound, "Not found."));
        }
        Ok(())
    }

    // ── Assets ──────────────────────────────────────────────────────────

    fn assets(conn: &Connection, client_id: Uuid) -> IpcResult<Vec<AssetInfo>> {
        conn.prepare(
            "SELECT kind, sha256, width, height, length(data), updated_at FROM client_assets
             WHERE client_id = ?1 AND deleted_at IS NULL ORDER BY kind",
        )
        .ipc()?
        .query_map([client_id.to_string()], |r| {
            Ok(AssetInfo {
                kind: enum_at(r, 0)?,
                sha256: r.get(1)?,
                width: r.get(2)?,
                height: r.get(3)?,
                byte_length: u64_at(r, 4)?,
                updated_at: ts_at(r, 5)?,
            })
        })
        .ipc()?
        .collect::<rusqlite::Result<_>>()
        .ipc()
    }

    pub fn asset(&self, client_id: Uuid, kind: AssetKind) -> IpcResult<Option<Vec<u8>>> {
        self.conn()
            .query_row(
                "SELECT data FROM client_assets WHERE client_id = ?1 AND kind = ?2 AND deleted_at IS NULL",
                params![client_id.to_string(), enum_str(&kind)],
                |r| r.get(0),
            )
            .optional()
            .ipc()
    }

    /// Replaces the client's asset of `kind` (the old row is soft-deleted) and
    /// keeps `receipt.logo_asset` in step.
    pub fn put_asset(
        &self,
        client_id: Uuid,
        kind: AssetKind,
        bytes: &[u8],
        (width, height): (u32, u32),
        now: Timestamp,
    ) -> IpcResult<ClientDetail> {
        let mut conn = self.conn();
        let tx = conn.transaction().ipc()?;
        Self::detail(&tx, client_id)?;
        Self::retire_asset(&tx, client_id, kind, now)?;
        tx.execute(
            "INSERT INTO client_assets (id, created_at, updated_at, client_id, kind, sha256, width, height, data)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                Uuid::now_v7().to_string(),
                now.to_string(),
                client_id.to_string(),
                enum_str(&kind),
                sha256_hex(bytes),
                width,
                height,
                bytes,
            ],
        )
        .ipc()?;
        Self::sync_logo_reference(&tx, client_id, now)?;
        tx.commit().ipc()?;
        Self::detail(&conn, client_id)
    }

    pub fn remove_asset(
        &self,
        client_id: Uuid,
        kind: AssetKind,
        now: Timestamp,
    ) -> IpcResult<ClientDetail> {
        let mut conn = self.conn();
        let tx = conn.transaction().ipc()?;
        Self::detail(&tx, client_id)?;
        Self::retire_asset(&tx, client_id, kind, now)?;
        Self::sync_logo_reference(&tx, client_id, now)?;
        tx.commit().ipc()?;
        Self::detail(&conn, client_id)
    }

    fn retire_asset(
        conn: &Connection,
        client_id: Uuid,
        kind: AssetKind,
        now: Timestamp,
    ) -> IpcResult<()> {
        conn.execute(
            "UPDATE client_assets SET deleted_at = ?3, updated_at = ?3
             WHERE client_id = ?1 AND kind = ?2 AND deleted_at IS NULL",
            params![client_id.to_string(), enum_str(&kind), now.to_string()],
        )
        .ipc()?;
        Ok(())
    }

    fn sync_logo_reference(conn: &Connection, client_id: Uuid, now: Timestamp) -> IpcResult<()> {
        let mut detail = Self::detail(conn, client_id)?;
        let wanted = detail
            .assets
            .iter()
            .any(|a| a.kind == AssetKind::ReceiptLogo)
            .then(|| AssetKind::ReceiptLogo.file_name().to_owned());
        if detail.config.receipt.logo_asset != wanted {
            detail.config.receipt.logo_asset = wanted;
            conn.execute(
                "UPDATE clients SET config = ?2, updated_at = ?3 WHERE id = ?1",
                params![
                    client_id.to_string(),
                    to_json(&detail.config)?,
                    now.to_string()
                ],
            )
            .ipc()?;
        }
        Ok(())
    }

    // ── Licenses ────────────────────────────────────────────────────────

    pub fn record_license(&self, license: &IssuedLicenseRecord, now: Timestamp) -> IpcResult<()> {
        self.conn()
            .execute(
                "INSERT INTO issued_licenses (id, created_at, updated_at, client_id, device_name,
                   fingerprint_hash, max_devices, issued_at, expires_at, token)
                 VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    license.license_id.to_string(),
                    now.to_string(),
                    license.client_id.to_string(),
                    license.device_name,
                    license.fingerprint_hash,
                    license.max_devices,
                    license.issued_at.to_string(),
                    license.expires_at.map(|t| t.to_string()),
                    license.token,
                ],
            )
            .ipc()?;
        Ok(())
    }

    pub fn licenses(&self, client_id: Uuid) -> IpcResult<Vec<IssuedLicenseRecord>> {
        self.conn()
            .prepare(
                "SELECT id, client_id, device_name, fingerprint_hash, max_devices, issued_at, expires_at, token
                 FROM issued_licenses WHERE client_id = ?1 AND deleted_at IS NULL
                 ORDER BY issued_at DESC, id DESC",
            )
            .ipc()?
            .query_map([client_id.to_string()], |r| {
                Ok(IssuedLicenseRecord {
                    license_id: uuid_at(r, 0)?,
                    client_id: uuid_at(r, 1)?,
                    device_name: r.get(2)?,
                    fingerprint_hash: r.get(3)?,
                    max_devices: r.get(4)?,
                    issued_at: ts_at(r, 5)?,
                    expires_at: opt_ts_at(r, 6)?,
                    token: r.get(7)?,
                })
            })
            .ipc()?
            .collect::<rusqlite::Result<_>>()
            .ipc()
    }

    // ── Builds ──────────────────────────────────────────────────────────

    pub fn create_build(
        &self,
        client_id: Uuid,
        config_sha256: &str,
        app_version: &str,
        now: Timestamp,
    ) -> IpcResult<BuildRecord> {
        let conn = self.conn();
        Self::detail(&conn, client_id)?;
        let id = Uuid::now_v7();
        conn.execute(
            "INSERT INTO builds (id, created_at, updated_at, client_id, status, config_sha256, app_version)
             VALUES (?1, ?2, ?2, ?3, 'publishing', ?4, ?5)",
            params![
                id.to_string(),
                now.to_string(),
                client_id.to_string(),
                config_sha256,
                app_version
            ],
        )
        .ipc()?;
        Self::build_by_id(&conn, id)
    }

    pub fn update_build(
        &self,
        build_id: Uuid,
        update: &BuildUpdate,
        now: Timestamp,
    ) -> IpcResult<BuildRecord> {
        let conn = self.conn();
        let completed = update
            .status
            .filter(|s| !s.is_active())
            .map(|_| now.to_string());
        conn.execute(
            "UPDATE builds SET
               status = COALESCE(?2, status),
               commit_sha = COALESCE(?3, commit_sha),
               run_id = COALESCE(?4, run_id),
               run_url = COALESCE(?5, run_url),
               artifact_id = COALESCE(?6, artifact_id),
               artifact_name = COALESCE(?7, artifact_name),
               artifact_size = COALESCE(?8, artifact_size),
               download_path = COALESCE(?9, download_path),
               message = COALESCE(?10, message),
               completed_at = COALESCE(completed_at, ?11),
               updated_at = ?12
             WHERE id = ?1",
            params![
                build_id.to_string(),
                update.status.map(|s| enum_str(&s)),
                update.commit_sha,
                opt_i64(update.run_id),
                update.run_url,
                opt_i64(update.artifact_id),
                update.artifact_name,
                opt_i64(update.artifact_size),
                update.download_path,
                update.message,
                completed,
                now.to_string(),
            ],
        )
        .ipc()?;
        Self::build_by_id(&conn, build_id)
    }

    pub fn build(&self, build_id: Uuid) -> IpcResult<BuildRecord> {
        Self::build_by_id(&self.conn(), build_id)
    }

    fn build_by_id(conn: &Connection, build_id: Uuid) -> IpcResult<BuildRecord> {
        conn.query_row(
            &format!("{BUILD_SELECT} WHERE b.id = ?1"),
            [build_id.to_string()],
            read_build,
        )
        .ipc()
    }

    /// Newest first; `client_id: None` = every client.
    pub fn builds(&self, client_id: Option<Uuid>, limit: u32) -> IpcResult<Vec<BuildRecord>> {
        Self::builds_where(&self.conn(), client_id, limit)
    }

    fn builds_where(
        conn: &Connection,
        client_id: Option<Uuid>,
        limit: u32,
    ) -> IpcResult<Vec<BuildRecord>> {
        conn.prepare(&format!(
            "{BUILD_SELECT} WHERE b.deleted_at IS NULL AND (?1 IS NULL OR b.client_id = ?1)
             ORDER BY b.created_at DESC, b.id DESC LIMIT ?2"
        ))
        .ipc()?
        .query_map(params![client_id.map(|c| c.to_string()), limit], read_build)
        .ipc()?
        .collect::<rusqlite::Result<_>>()
        .ipc()
    }

    pub fn active_builds(&self) -> IpcResult<Vec<BuildRecord>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "{BUILD_SELECT} WHERE b.deleted_at IS NULL AND b.status IN ('publishing', 'queued', 'in_progress')
                 ORDER BY b.created_at"
            ))
            .ipc()?;
        let rows = stmt
            .query_map([], read_build)
            .ipc()?
            .collect::<rusqlite::Result<_>>()
            .ipc();
        rows
    }

    // ── Settings ────────────────────────────────────────────────────────

    pub fn setting<T: DeserializeOwned>(&self, key: &str) -> IpcResult<Option<T>> {
        let raw: Option<String> = self
            .conn()
            .query_row(
                "SELECT value FROM settings WHERE key = ?1 AND deleted_at IS NULL",
                [key],
                |r| r.get(0),
            )
            .optional()
            .ipc()?;
        Ok(raw.and_then(|v| serde_json::from_str(&v).ok()))
    }

    pub fn put_setting<T: Serialize>(&self, key: &str, value: &T, now: Timestamp) -> IpcResult<()> {
        self.conn()
            .execute(
                "INSERT INTO settings (id, created_at, updated_at, key, value) VALUES (?1, ?2, ?2, ?3, ?4)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at, deleted_at = NULL",
                params![Uuid::now_v7().to_string(), now.to_string(), key, to_json(value)?],
            )
            .ipc()?;
        Ok(())
    }
}

const BUILD_SELECT: &str = "SELECT b.id, b.client_id, c.slug, b.status, b.config_sha256, b.app_version,
    b.commit_sha, b.run_id, b.run_url, b.artifact_id, b.artifact_name, b.artifact_size, b.download_path,
    b.message, b.created_at, b.updated_at, b.completed_at
  FROM builds b JOIN clients c ON c.id = b.client_id";

fn read_build(r: &Row<'_>) -> rusqlite::Result<BuildRecord> {
    Ok(BuildRecord {
        build_id: uuid_at(r, 0)?,
        client_id: uuid_at(r, 1)?,
        client_slug: r.get(2)?,
        status: enum_at(r, 3)?,
        config_sha256: r.get(4)?,
        app_version: r.get(5)?,
        commit_sha: r.get(6)?,
        run_id: opt_u64_at(r, 7)?,
        run_url: r.get(8)?,
        artifact_id: opt_u64_at(r, 9)?,
        artifact_name: r.get(10)?,
        artifact_size: opt_u64_at(r, 11)?,
        download_path: r.get(12)?,
        message: r.get(13)?,
        requested_at: ts_at(r, 14)?,
        updated_at: ts_at(r, 15)?,
        completed_at: opt_ts_at(r, 16)?,
    })
}

fn to_json<T: Serialize>(value: &T) -> IpcResult<String> {
    serde_json::to_string(value).map_err(|e| IpcError::internal(format!("serialize: {e}")))
}

#[cfg(test)]
mod tests;
