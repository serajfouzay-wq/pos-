//! Shop-wide settings (`shop_settings`): synced last-write-wins between the
//! shop's tills. Each key has a fixed row id (`SHOP_SETTING_IDS` in
//! `@pos/shared`), so tills that write the same setting offline update one
//! row instead of clashing on the key.

use pos_core::config::ClientConfig;
use pos_core::loyalty::LoyaltySettings;
use pos_core::time::Timestamp;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use uuid::Uuid;

use super::{rows, Meta};

pub const LOYALTY_KEY: &str = "loyalty";
/// `SHOP_SETTING_IDS.loyalty`.
pub const LOYALTY_ID: Uuid = Uuid::from_u128(0x0199_a000_0000_7000_8000_0000_0000_0001);

/// Mirrors `ShopSettingRowSchema`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShopSetting {
    #[serde(flatten)]
    meta: Meta,
    key: String,
    #[serde(with = "rows::json_text")]
    value: Json,
}

fn row(conn: &Connection, key: &str) -> rusqlite::Result<Option<ShopSetting>> {
    Ok(rows::select(
        conn,
        "SELECT id, created_at, updated_at, deleted_at, key, value FROM shop_settings
         WHERE key = ?1 AND deleted_at IS NULL",
        [key],
    )?
    .into_iter()
    .next())
}

fn put(
    conn: &Connection,
    id: Uuid,
    key: &str,
    value: Json,
    now: Timestamp,
) -> rusqlite::Result<()> {
    let meta = match row(conn, key)? {
        Some(existing) => Meta {
            updated_at: now,
            ..existing.meta
        },
        None => Meta {
            id,
            ..Meta::new(now)
        },
    };
    rows::upsert(
        conn,
        "shop_settings",
        &ShopSetting {
            meta,
            key: key.to_owned(),
            value,
        },
        now,
    )
}

/// The stored programme, or the defaults for the shop's currency.
pub fn loyalty(conn: &Connection, client: &ClientConfig) -> rusqlite::Result<LoyaltySettings> {
    Ok(row(conn, LOYALTY_KEY)?
        .and_then(|r| serde_json::from_value(r.value).ok())
        .unwrap_or_else(|| LoyaltySettings::default_for(client.currency.base)))
}

/// What sales use: off unless the build has loyalty and the owner left it on.
pub fn effective_loyalty(
    conn: &Connection,
    client: &ClientConfig,
) -> rusqlite::Result<LoyaltySettings> {
    let mut settings = loyalty(conn, client)?;
    settings.enabled &= client.features.loyalty;
    Ok(settings)
}

pub fn save_loyalty(
    conn: &Connection,
    settings: &LoyaltySettings,
    now: Timestamp,
) -> rusqlite::Result<()> {
    let value = serde_json::to_value(settings)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    put(conn, LOYALTY_ID, LOYALTY_KEY, value, now)
}
