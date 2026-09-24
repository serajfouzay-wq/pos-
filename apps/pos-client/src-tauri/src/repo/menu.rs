//! Modifier groups and options, product ↔ group links, combos and the dining
//! floor. Rows are saved whole (last-write-wins) through `rows::upsert`.

use std::collections::{BTreeMap, HashMap};

use pos_core::time::Timestamp;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::rows::{self, int_bool, json_text};
use super::Meta;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModifierGroup {
    #[serde(flatten)]
    pub meta: Meta,
    pub name: String,
    #[serde(with = "json_text")]
    pub name_localized: serde_json::Value,
    pub min_select: i64,
    pub max_select: i64,
    pub sort_order: i64,
    #[serde(deserialize_with = "int_bool")]
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Modifier {
    #[serde(flatten)]
    pub meta: Meta,
    pub group_id: Uuid,
    pub name: String,
    #[serde(with = "json_text")]
    pub name_localized: serde_json::Value,
    pub price_delta: i64,
    #[serde(deserialize_with = "int_bool")]
    pub is_default: bool,
    pub sort_order: i64,
    #[serde(deserialize_with = "int_bool")]
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductModifierGroup {
    #[serde(flatten)]
    pub meta: Meta,
    pub product_id: Uuid,
    pub group_id: Uuid,
    pub sort_order: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Combo {
    #[serde(flatten)]
    pub meta: Meta,
    pub name: String,
    #[serde(with = "json_text")]
    pub name_localized: serde_json::Value,
    pub price: i64,
    pub color: Option<String>,
    pub sort_order: i64,
    #[serde(deserialize_with = "int_bool")]
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComboItem {
    #[serde(flatten)]
    pub meta: Meta,
    pub combo_id: Uuid,
    pub product_id: Uuid,
    pub quantity_milli: i64,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableShape {
    Square,
    Round,
    Bar,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiningTable {
    #[serde(flatten)]
    pub meta: Meta,
    pub label: String,
    pub area: String,
    pub seats: i64,
    pub shape: TableShape,
    pub grid_x: i64,
    pub grid_y: i64,
    pub sort_order: i64,
    #[serde(deserialize_with = "int_bool")]
    pub is_active: bool,
}

const LIVE: &str = "deleted_at IS NULL";

pub fn modifier_groups(conn: &Connection) -> rusqlite::Result<Vec<ModifierGroup>> {
    rows::select(
        conn,
        &format!("SELECT * FROM modifier_groups WHERE {LIVE} ORDER BY sort_order, name"),
        [],
    )
}

pub fn modifiers(conn: &Connection) -> rusqlite::Result<Vec<Modifier>> {
    rows::select(
        conn,
        &format!("SELECT * FROM modifiers WHERE {LIVE} ORDER BY sort_order, name"),
        [],
    )
}

pub fn product_groups(conn: &Connection) -> rusqlite::Result<Vec<ProductModifierGroup>> {
    rows::select(
        conn,
        &format!("SELECT * FROM product_modifier_groups WHERE {LIVE} ORDER BY sort_order"),
        [],
    )
}

pub fn combos(conn: &Connection) -> rusqlite::Result<Vec<Combo>> {
    rows::select(
        conn,
        &format!("SELECT * FROM combos WHERE {LIVE} ORDER BY sort_order, name"),
        [],
    )
}

pub fn combo_items(conn: &Connection) -> rusqlite::Result<Vec<ComboItem>> {
    rows::select(
        conn,
        &format!("SELECT * FROM combo_items WHERE {LIVE} ORDER BY sort_order"),
        [],
    )
}

pub fn dining_tables(conn: &Connection) -> rusqlite::Result<Vec<DiningTable>> {
    rows::select(
        conn,
        &format!("SELECT * FROM dining_tables WHERE {LIVE} ORDER BY sort_order, label"),
        [],
    )
}

/// One group with its active options, as the modifier picker needs it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupWithOptions {
    #[serde(flatten)]
    pub group: ModifierGroup,
    pub modifiers: Vec<Modifier>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComboWithItems {
    #[serde(flatten)]
    pub combo: Combo,
    pub items: Vec<ComboItem>,
}

/// Mirrors `MenuSchema`: everything the sell screens need besides products.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Menu {
    pub modifier_groups: Vec<GroupWithOptions>,
    /// product id → the groups it asks, in order.
    pub product_modifier_groups: BTreeMap<Uuid, Vec<Uuid>>,
    pub combos: Vec<ComboWithItems>,
    pub dining_tables: Vec<DiningTable>,
}

/// `include_inactive` for the back office; the till sees active rows only.
pub fn menu(conn: &Connection, include_inactive: bool) -> rusqlite::Result<Menu> {
    let mut options: HashMap<Uuid, Vec<Modifier>> = HashMap::new();
    for m in modifiers(conn)?
        .into_iter()
        .filter(|m| include_inactive || m.is_active)
    {
        options.entry(m.group_id).or_default().push(m);
    }
    let groups: Vec<GroupWithOptions> = modifier_groups(conn)?
        .into_iter()
        .filter(|g| include_inactive || g.is_active)
        .map(|group| GroupWithOptions {
            modifiers: options.remove(&group.meta.id).unwrap_or_default(),
            group,
        })
        .collect();
    let live_groups: Vec<Uuid> = groups.iter().map(|g| g.group.meta.id).collect();
    let mut product_modifier_groups: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
    for link in product_groups(conn)?
        .into_iter()
        .filter(|l| live_groups.contains(&l.group_id))
    {
        product_modifier_groups
            .entry(link.product_id)
            .or_default()
            .push(link.group_id);
    }
    let mut components: HashMap<Uuid, Vec<ComboItem>> = HashMap::new();
    for item in combo_items(conn)? {
        components.entry(item.combo_id).or_default().push(item);
    }
    let combos = combos(conn)?
        .into_iter()
        .filter(|c| include_inactive || c.is_active)
        .map(|combo| ComboWithItems {
            items: components.remove(&combo.meta.id).unwrap_or_default(),
            combo,
        })
        .collect();
    let dining_tables = dining_tables(conn)?
        .into_iter()
        .filter(|t| include_inactive || t.is_active)
        .collect();
    Ok(Menu {
        modifier_groups: groups,
        product_modifier_groups,
        combos,
        dining_tables,
    })
}

/// Soft-deletes `rows` (already loaded) that are not in `keep`.
fn retire<T: Serialize + Clone>(
    conn: &Connection,
    table: &str,
    rows_: &[T],
    meta: impl Fn(&T) -> &Meta,
    keep: &[Uuid],
    now: Timestamp,
) -> rusqlite::Result<()> {
    for row in rows_ {
        let m = meta(row);
        if !keep.contains(&m.id) && m.deleted_at.is_none() {
            let mut gone = serde_json::to_value(row)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            gone["deleted_at"] = serde_json::Value::String(now.to_string());
            gone["updated_at"] = serde_json::Value::String(now.to_string());
            rows::upsert(conn, table, &gone, now)?;
        }
    }
    Ok(())
}

pub fn save_group(
    conn: &Connection,
    group: &ModifierGroup,
    options: &[Modifier],
    now: Timestamp,
) -> rusqlite::Result<()> {
    rows::upsert(conn, "modifier_groups", group, now)?;
    let existing: Vec<Modifier> = rows::select(
        conn,
        "SELECT * FROM modifiers WHERE group_id = ?1 AND deleted_at IS NULL",
        [group.meta.id.to_string()],
    )?;
    let keep: Vec<Uuid> = options.iter().map(|m| m.meta.id).collect();
    retire(conn, "modifiers", &existing, |m| &m.meta, &keep, now)?;
    for option in options {
        rows::upsert(conn, "modifiers", option, now)?;
    }
    Ok(())
}

pub fn delete_group(conn: &Connection, group_id: Uuid, now: Timestamp) -> rusqlite::Result<bool> {
    let groups: Vec<ModifierGroup> = rows::select(
        conn,
        "SELECT * FROM modifier_groups WHERE id = ?1 AND deleted_at IS NULL",
        [group_id.to_string()],
    )?;
    let Some(group) = groups.first() else {
        return Ok(false);
    };
    save_group(conn, group, &[], now)?;
    retire(conn, "modifier_groups", &groups, |g| &g.meta, &[], now)?;
    let links: Vec<ProductModifierGroup> = rows::select(
        conn,
        "SELECT * FROM product_modifier_groups WHERE group_id = ?1 AND deleted_at IS NULL",
        [group_id.to_string()],
    )?;
    retire(
        conn,
        "product_modifier_groups",
        &links,
        |l| &l.meta,
        &[],
        now,
    )?;
    Ok(true)
}

/// Makes the product ask exactly `group_ids`, in that order.
pub fn set_product_groups(
    conn: &Connection,
    product_id: Uuid,
    group_ids: &[Uuid],
    now: Timestamp,
) -> rusqlite::Result<()> {
    let existing: Vec<ProductModifierGroup> = rows::select(
        conn,
        "SELECT * FROM product_modifier_groups WHERE product_id = ?1 AND deleted_at IS NULL",
        [product_id.to_string()],
    )?;
    let mut keep = Vec::new();
    for (position, group_id) in group_ids.iter().enumerate() {
        let sort_order = i64::try_from(position).unwrap_or(i64::MAX);
        match existing.iter().find(|l| l.group_id == *group_id) {
            Some(link) if link.sort_order == sort_order => keep.push(link.meta.id),
            Some(link) => {
                keep.push(link.meta.id);
                let moved = ProductModifierGroup {
                    meta: Meta {
                        updated_at: now,
                        ..link.meta.clone()
                    },
                    sort_order,
                    ..link.clone()
                };
                rows::upsert(conn, "product_modifier_groups", &moved, now)?;
            }
            None => {
                let link = ProductModifierGroup {
                    meta: Meta::new(now),
                    product_id,
                    group_id: *group_id,
                    sort_order,
                };
                keep.push(link.meta.id);
                rows::upsert(conn, "product_modifier_groups", &link, now)?;
            }
        }
    }
    retire(
        conn,
        "product_modifier_groups",
        &existing,
        |l| &l.meta,
        &keep,
        now,
    )
}

pub fn save_combo(
    conn: &Connection,
    combo: &Combo,
    items: &[ComboItem],
    now: Timestamp,
) -> rusqlite::Result<()> {
    rows::upsert(conn, "combos", combo, now)?;
    let existing: Vec<ComboItem> = rows::select(
        conn,
        "SELECT * FROM combo_items WHERE combo_id = ?1 AND deleted_at IS NULL",
        [combo.meta.id.to_string()],
    )?;
    let keep: Vec<Uuid> = items.iter().map(|i| i.meta.id).collect();
    retire(conn, "combo_items", &existing, |i| &i.meta, &keep, now)?;
    for item in items {
        rows::upsert(conn, "combo_items", item, now)?;
    }
    Ok(())
}

pub fn delete_combo(conn: &Connection, combo_id: Uuid, now: Timestamp) -> rusqlite::Result<bool> {
    let found: Vec<Combo> = rows::select(
        conn,
        "SELECT * FROM combos WHERE id = ?1 AND deleted_at IS NULL",
        [combo_id.to_string()],
    )?;
    let Some(combo) = found.first() else {
        return Ok(false);
    };
    save_combo(conn, combo, &[], now)?;
    retire(conn, "combos", &found, |c| &c.meta, &[], now)?;
    Ok(true)
}

pub fn save_table(conn: &Connection, table: &DiningTable, now: Timestamp) -> rusqlite::Result<()> {
    rows::upsert(conn, "dining_tables", table, now)
}

pub fn delete_table(conn: &Connection, table_id: Uuid, now: Timestamp) -> rusqlite::Result<bool> {
    let found: Vec<DiningTable> = rows::select(
        conn,
        "SELECT * FROM dining_tables WHERE id = ?1 AND deleted_at IS NULL",
        [table_id.to_string()],
    )?;
    retire(conn, "dining_tables", &found, |t| &t.meta, &[], now)?;
    Ok(!found.is_empty())
}
