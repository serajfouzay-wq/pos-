//! Products, categories and stock movements.

use pos_core::time::Timestamp;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::audit::Actor;
use super::outbox::{self, EventType};
use super::{enum_at, enum_str, json_at, new_id, opt_uuid_at, Meta, META_COLUMNS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Each,
    Kg,
    G,
    L,
    Ml,
}

/// Mirrors `ProductSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Product {
    #[serde(flatten)]
    pub meta: Meta,
    pub name: String,
    pub name_localized: serde_json::Value,
    pub category_id: Option<Uuid>,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub price: i64,
    pub cost: Option<i64>,
    pub tax_rate_bps: i64,
    pub unit: Unit,
    pub sold_by_weight: bool,
    pub track_stock: bool,
    pub stock_on_hand_milli: i64,
    pub reorder_threshold_milli: Option<i64>,
    pub reorder_quantity_milli: Option<i64>,
    pub image_asset: Option<String>,
    pub quick_key_position: Option<i64>,
    pub is_active: bool,
}

/// Mirrors `ProductFilterSchema` (defaults applied here as well).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProductFilter {
    pub search: Option<String>,
    pub category_id: Option<Uuid>,
    pub barcode: Option<String>,
    pub low_stock_only: Option<bool>,
    pub include_inactive: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

const PRODUCT_COLUMNS: &str = "name, name_localized, category_id, sku, barcode, price, cost, tax_rate_bps, unit,
    sold_by_weight, track_stock, stock_on_hand_milli, reorder_threshold_milli, reorder_quantity_milli,
    image_asset, quick_key_position, is_active";

fn read_product(row: &rusqlite::Row<'_>) -> rusqlite::Result<Product> {
    Ok(Product {
        meta: Meta::read(row, 0)?,
        name: row.get(4)?,
        name_localized: json_at(row, 5)?,
        category_id: opt_uuid_at(row, 6)?,
        sku: row.get(7)?,
        barcode: row.get(8)?,
        price: row.get(9)?,
        cost: row.get(10)?,
        tax_rate_bps: row.get(11)?,
        unit: enum_at(row, 12)?,
        sold_by_weight: row.get(13)?,
        track_stock: row.get(14)?,
        stock_on_hand_milli: row.get(15)?,
        reorder_threshold_milli: row.get(16)?,
        reorder_quantity_milli: row.get(17)?,
        image_asset: row.get(18)?,
        quick_key_position: row.get(19)?,
        is_active: row.get(20)?,
    })
}

fn like_pattern(term: &str) -> String {
    let escaped = term
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

pub fn query(conn: &Connection, filter: &ProductFilter) -> rusqlite::Result<Vec<Product>> {
    let mut clauses = vec!["deleted_at IS NULL".to_owned()];
    let mut args: Vec<String> = Vec::new();
    if !filter.include_inactive.unwrap_or(false) {
        clauses.push("is_active = 1".into());
    }
    if let Some(category) = filter.category_id {
        args.push(category.to_string());
        clauses.push(format!("category_id = ?{}", args.len()));
    }
    if let Some(barcode) = filter
        .barcode
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        args.push(barcode.to_owned());
        clauses.push(format!("barcode = ?{}", args.len()));
    }
    if let Some(term) = filter
        .search
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        args.push(like_pattern(term));
        let n = args.len();
        clauses.push(format!(
            "(name LIKE ?{n} ESCAPE '\\' OR sku LIKE ?{n} ESCAPE '\\' OR barcode LIKE ?{n} ESCAPE '\\')"
        ));
    }
    if filter.low_stock_only.unwrap_or(false) {
        clauses.push(
            "track_stock = 1 AND reorder_threshold_milli IS NOT NULL AND stock_on_hand_milli <= reorder_threshold_milli"
                .into(),
        );
    }
    let limit = filter.limit.unwrap_or(200).clamp(1, 1000);
    let offset = filter.offset.unwrap_or(0).max(0);
    let sql = format!(
        "SELECT {META_COLUMNS}, {PRODUCT_COLUMNS} FROM products WHERE {}
         ORDER BY quick_key_position IS NULL, quick_key_position, name COLLATE NOCASE
         LIMIT {limit} OFFSET {offset}",
        clauses.join(" AND ")
    );
    conn.prepare(&sql)?
        .query_map(params_from_iter(args.iter()), read_product)?
        .collect()
}

pub fn get(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<Product>> {
    conn.query_row(
        &format!("SELECT {META_COLUMNS}, {PRODUCT_COLUMNS} FROM products WHERE id = ?1 AND deleted_at IS NULL"),
        [id.to_string()],
        read_product,
    )
    .optional()
}

pub fn count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM products WHERE deleted_at IS NULL",
        [],
        |r| r.get(0),
    )
}

/// Another live product already using this barcode?
pub fn barcode_taken(
    conn: &Connection,
    barcode: &str,
    except: Option<Uuid>,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM products WHERE barcode = ?1 AND deleted_at IS NULL AND id IS NOT ?2)",
        params![barcode, except.map(|id| id.to_string())],
        |r| r.get(0),
    )
}

pub fn save(conn: &Connection, product: &Product, now: Timestamp) -> rusqlite::Result<()> {
    let p = product;
    conn.execute(
        &format!(
            "INSERT INTO products ({META_COLUMNS}, {PRODUCT_COLUMNS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
             ON CONFLICT (id) DO UPDATE SET updated_at = excluded.updated_at, deleted_at = excluded.deleted_at,
               name = excluded.name, name_localized = excluded.name_localized, category_id = excluded.category_id,
               sku = excluded.sku, barcode = excluded.barcode, price = excluded.price, cost = excluded.cost,
               tax_rate_bps = excluded.tax_rate_bps, unit = excluded.unit, sold_by_weight = excluded.sold_by_weight,
               track_stock = excluded.track_stock, reorder_threshold_milli = excluded.reorder_threshold_milli,
               reorder_quantity_milli = excluded.reorder_quantity_milli, image_asset = excluded.image_asset,
               quick_key_position = excluded.quick_key_position, is_active = excluded.is_active"
        ),
        params![
            p.meta.id.to_string(),
            p.meta.created_at.to_string(),
            p.meta.updated_at.to_string(),
            p.meta.deleted_at.map(|t| t.to_string()),
            p.name,
            p.name_localized.to_string(),
            p.category_id.map(|id| id.to_string()),
            p.sku,
            p.barcode,
            p.price,
            p.cost,
            p.tax_rate_bps,
            enum_str(&p.unit),
            p.sold_by_weight,
            p.track_stock,
            p.stock_on_hand_milli,
            p.reorder_threshold_milli,
            p.reorder_quantity_milli,
            p.image_asset,
            p.quick_key_position,
            p.is_active,
        ],
    )?;
    // `stock_on_hand_milli` is a local cache (the cloud derives it from
    // stock_movements); saving a product never rewrites stock.
    outbox::record(conn, "products", EventType::Upsert, p.meta.id, p, now)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockReason {
    Sale,
    Refund,
    PurchaseReceipt,
    Adjustment,
    Waste,
    StockCount,
}

#[derive(Debug, Clone, Serialize)]
struct StockMovement {
    #[serde(flatten)]
    meta: Meta,
    product_id: Uuid,
    quantity_delta_milli: i64,
    reason: StockReason,
    reference_id: Option<Uuid>,
    device_id: Uuid,
    user_id: Uuid,
    occurred_at: Timestamp,
}

/// Appends an additive stock delta and refreshes the cached on-hand figure.
pub fn move_stock(
    conn: &Connection,
    product_id: Uuid,
    delta_milli: i64,
    reason: StockReason,
    reference_id: Option<Uuid>,
    actor: &Actor,
    now: Timestamp,
) -> rusqlite::Result<()> {
    let movement = StockMovement {
        meta: Meta::new(now),
        product_id,
        quantity_delta_milli: delta_milli,
        reason,
        reference_id,
        device_id: actor.device_id,
        user_id: actor.user_id,
        occurred_at: now,
    };
    conn.execute(
        "INSERT INTO stock_movements (id, created_at, updated_at, product_id, quantity_delta_milli, reason,
                                      reference_id, device_id, user_id, occurred_at)
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?2)",
        params![
            movement.meta.id.to_string(),
            now.to_string(),
            product_id.to_string(),
            delta_milli,
            enum_str(&reason),
            reference_id.map(|id| id.to_string()),
            actor.device_id.to_string(),
            actor.user_id.to_string(),
        ],
    )?;
    conn.execute(
        "UPDATE products SET stock_on_hand_milli = stock_on_hand_milli + ?1 WHERE id = ?2",
        params![delta_milli, product_id.to_string()],
    )?;
    outbox::record(
        conn,
        "stock_movements",
        EventType::Append,
        movement.meta.id,
        &movement,
        now,
    )
}

/// Mirrors `CategorySchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Category {
    #[serde(flatten)]
    pub meta: Meta,
    pub name: String,
    pub name_localized: serde_json::Value,
    pub parent_id: Option<Uuid>,
    pub sort_order: i64,
    pub color: Option<String>,
}

pub fn categories(conn: &Connection) -> rusqlite::Result<Vec<Category>> {
    conn.prepare(&format!(
        "SELECT {META_COLUMNS}, name, name_localized, parent_id, sort_order, color FROM categories
         WHERE deleted_at IS NULL ORDER BY sort_order, name COLLATE NOCASE"
    ))?
    .query_map([], |row| {
        Ok(Category {
            meta: Meta::read(row, 0)?,
            name: row.get(4)?,
            name_localized: json_at(row, 5)?,
            parent_id: opt_uuid_at(row, 6)?,
            sort_order: row.get(7)?,
            color: row.get(8)?,
        })
    })?
    .collect()
}

pub fn save_category(
    conn: &Connection,
    category: &Category,
    now: Timestamp,
) -> rusqlite::Result<()> {
    let c = category;
    conn.execute(
        &format!(
            "INSERT INTO categories ({META_COLUMNS}, name, name_localized, parent_id, sort_order, color)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (id) DO UPDATE SET updated_at = excluded.updated_at, deleted_at = excluded.deleted_at,
               name = excluded.name, name_localized = excluded.name_localized, parent_id = excluded.parent_id,
               sort_order = excluded.sort_order, color = excluded.color"
        ),
        params![
            c.meta.id.to_string(),
            c.meta.created_at.to_string(),
            c.meta.updated_at.to_string(),
            c.meta.deleted_at.map(|t| t.to_string()),
            c.name,
            c.name_localized.to_string(),
            c.parent_id.map(|id| id.to_string()),
            c.sort_order,
            c.color,
        ],
    )?;
    outbox::record(conn, "categories", EventType::Upsert, c.meta.id, c, now)
}

pub fn new_category(name: &str, sort_order: i64, color: Option<&str>, now: Timestamp) -> Category {
    Category {
        meta: Meta {
            id: new_id(),
            ..Meta::new(now)
        },
        name: name.to_owned(),
        name_localized: serde_json::json!({}),
        parent_id: None,
        sort_order,
        color: color.map(str::to_owned),
    }
}
