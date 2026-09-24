//! Menu structure (modifier groups, combos) and the dining floor.
//! Reading: any signed-in user. Editing: `catalog.manage`.

use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock, Timestamp};
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use rusqlite::Connection;
use serde::Deserialize;
use tauri::State;
use uuid::Uuid;

use super::{authorize, blocking, Authorized};
use crate::repo::menu::{
    self, Combo, ComboItem, DiningTable, Menu, Modifier, ModifierGroup, TableShape,
};
use crate::repo::{audit, catalog, Meta, SqlResultExt};
use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn get_menu(state: State<'_, AppState>, include_inactive: bool) -> IpcResult<Menu> {
    let auth = authorize(
        &state,
        if include_inactive {
            Permission::CatalogManage
        } else {
            Permission::CatalogView
        },
    )?;
    blocking(move || menu::menu(&auth.db.conn(), include_inactive).ipc()).await
}

fn name(value: &str, max: usize) -> IpcResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max {
        return Err(IpcError::validation(format!(
            "Names are 1–{max} characters."
        )));
    }
    Ok(trimmed.to_owned())
}

fn meta_for(existing: Option<&Meta>, now: Timestamp) -> Meta {
    existing.map_or_else(
        || Meta::new(now),
        |m| Meta {
            updated_at: now,
            deleted_at: None,
            ..m.clone()
        },
    )
}

/// Runs `f` in one transaction with an audit entry, then nudges sync.
async fn edit<T, F>(state: &State<'_, AppState>, entity: &'static str, f: F) -> IpcResult<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection, Timestamp) -> IpcResult<(T, Option<Uuid>, serde_json::Value)>
        + Send
        + 'static,
{
    let auth: Authorized = authorize(state, Permission::CatalogManage)?;
    let result = blocking(move || {
        let now = SystemClock.now();
        let actor = auth.actor()?;
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let (value, id, after) = f(&tx, now)?;
        audit::record(
            &tx,
            &actor,
            "catalog.manage",
            entity,
            id,
            None,
            Some(after),
            now,
        )
        .ipc()?;
        tx.commit().ipc()?;
        Ok(value)
    })
    .await;
    if result.is_ok() {
        state.sync.nudge();
    }
    result
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModifierInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub price_delta: i64,
    pub is_default: bool,
    pub is_active: bool,
}

/// Mirrors `ModifierGroupInputSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct ModifierGroupInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub min_select: i64,
    pub max_select: i64,
    pub sort_order: i64,
    pub is_active: bool,
    pub modifiers: Vec<ModifierInput>,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_modifier_group(
    state: State<'_, AppState>,
    group: ModifierGroupInput,
) -> IpcResult<Uuid> {
    let group_name = name(&group.name, 80)?;
    if !(0..=20).contains(&group.min_select)
        || !(1..=20).contains(&group.max_select)
        || group.min_select > group.max_select
    {
        return Err(IpcError::validation(
            "Choose between 0–20 (minimum) and 1–20 (maximum), minimum ≤ maximum.",
        ));
    }
    if group.modifiers.is_empty() || group.modifiers.len() > 40 {
        return Err(IpcError::validation("A group needs 1–40 options."));
    }
    let active = group.modifiers.iter().filter(|m| m.is_active).count();
    if i64::try_from(active).unwrap_or(0) < group.min_select {
        return Err(IpcError::validation(
            "There are fewer active options than the minimum to choose.",
        ));
    }
    if group
        .modifiers
        .iter()
        .any(|m| m.price_delta.abs() > 1_000_000_000)
    {
        return Err(IpcError::validation("That price change is too large."));
    }
    edit(&state, "modifier_groups", move |tx, now| {
        let existing = menu::modifier_groups(tx)
            .ipc()?
            .into_iter()
            .find(|g| Some(g.meta.id) == group.id);
        if group.id.is_some() && existing.is_none() {
            return Err(IpcError::new(
                IpcErrorCode::NotFound,
                "That group no longer exists.",
            ));
        }
        let saved = ModifierGroup {
            meta: meta_for(existing.as_ref().map(|g| &g.meta), now),
            name: group_name,
            name_localized: existing
                .as_ref()
                .map_or_else(|| serde_json::json!({}), |g| g.name_localized.clone()),
            min_select: group.min_select,
            max_select: group.max_select,
            sort_order: group.sort_order,
            is_active: group.is_active,
        };
        let current: Vec<Modifier> = menu::modifiers(tx)
            .ipc()?
            .into_iter()
            .filter(|m| m.group_id == saved.meta.id)
            .collect();
        let options = group
            .modifiers
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let old = current.iter().find(|c| Some(c.meta.id) == m.id);
                if m.id.is_some() && old.is_none() {
                    return Err(IpcError::validation(
                        "An option does not belong to this group.",
                    ));
                }
                Ok(Modifier {
                    meta: meta_for(old.map(|o| &o.meta), now),
                    group_id: saved.meta.id,
                    name: name(&m.name, 80)?,
                    name_localized: old
                        .map_or_else(|| serde_json::json!({}), |o| o.name_localized.clone()),
                    price_delta: m.price_delta,
                    is_default: m.is_default,
                    sort_order: i64::try_from(i).unwrap_or(0),
                    is_active: m.is_active,
                })
            })
            .collect::<IpcResult<Vec<_>>>()?;
        menu::save_group(tx, &saved, &options, now).ipc()?;
        let after = serde_json::json!({ "group": &saved, "modifiers": &options });
        Ok((saved.meta.id, Some(saved.meta.id), after))
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_modifier_group(state: State<'_, AppState>, group_id: Uuid) -> IpcResult<()> {
    edit(&state, "modifier_groups", move |tx, now| {
        if !menu::delete_group(tx, group_id, now).ipc()? {
            return Err(IpcError::new(
                IpcErrorCode::NotFound,
                "That group no longer exists.",
            ));
        }
        Ok(((), Some(group_id), serde_json::json!({ "deleted": true })))
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_product_modifier_groups(
    state: State<'_, AppState>,
    product_id: Uuid,
    group_ids: Vec<Uuid>,
) -> IpcResult<()> {
    if group_ids.len() > 10 {
        return Err(IpcError::validation("A product can ask at most 10 groups."));
    }
    edit(&state, "product_modifier_groups", move |tx, now| {
        catalog::get(tx, product_id).ipc()?.ok_or_else(|| {
            IpcError::new(IpcErrorCode::NotFound, "That product no longer exists.")
        })?;
        let known: Vec<Uuid> = menu::modifier_groups(tx)
            .ipc()?
            .into_iter()
            .map(|g| g.meta.id)
            .collect();
        let mut unique = Vec::new();
        for id in &group_ids {
            if !known.contains(id) {
                return Err(IpcError::validation("One of the groups no longer exists."));
            }
            if !unique.contains(id) {
                unique.push(*id);
            }
        }
        menu::set_product_groups(tx, product_id, &unique, now).ipc()?;
        Ok((
            (),
            Some(product_id),
            serde_json::json!({ "group_ids": unique }),
        ))
    })
    .await
}

#[derive(Debug, Clone, Deserialize)]
pub struct ComboItemInput {
    pub product_id: Uuid,
    pub quantity_milli: i64,
}

/// Mirrors `ComboInputSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct ComboInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub price: i64,
    pub color: Option<String>,
    pub sort_order: i64,
    pub is_active: bool,
    pub items: Vec<ComboItemInput>,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_combo(state: State<'_, AppState>, combo: ComboInput) -> IpcResult<Uuid> {
    let combo_name = name(&combo.name, 80)?;
    if combo.price < 0 {
        return Err(IpcError::validation("The combo price must be 0 or more."));
    }
    if combo.items.len() < 2 || combo.items.len() > 12 {
        return Err(IpcError::validation("A combo has 2–12 items."));
    }
    if combo
        .items
        .iter()
        .any(|i| i.quantity_milli <= 0 || i.quantity_milli % 1000 != 0)
    {
        return Err(IpcError::validation("Combo items are whole quantities."));
    }
    if combo
        .color
        .as_ref()
        .is_some_and(|c| c.len() != 7 || !c.starts_with('#'))
    {
        return Err(IpcError::validation("Colours look like #1F6FEB."));
    }
    edit(&state, "combos", move |tx, now| {
        for item in &combo.items {
            let product = catalog::get(tx, item.product_id)
                .ipc()?
                .ok_or_else(|| IpcError::validation("A combo item no longer exists."))?;
            if product.sold_by_weight {
                return Err(IpcError::validation(format!(
                    "{} is sold by weight and cannot be in a combo.",
                    product.name
                )));
            }
        }
        let existing = menu::combos(tx)
            .ipc()?
            .into_iter()
            .find(|c| Some(c.meta.id) == combo.id);
        if combo.id.is_some() && existing.is_none() {
            return Err(IpcError::new(
                IpcErrorCode::NotFound,
                "That combo no longer exists.",
            ));
        }
        let saved = Combo {
            meta: meta_for(existing.as_ref().map(|c| &c.meta), now),
            name: combo_name,
            name_localized: existing
                .as_ref()
                .map_or_else(|| serde_json::json!({}), |c| c.name_localized.clone()),
            price: combo.price,
            color: combo.color,
            sort_order: combo.sort_order,
            is_active: combo.is_active,
        };
        let items: Vec<ComboItem> = combo
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| ComboItem {
                meta: Meta::new(now),
                combo_id: saved.meta.id,
                product_id: item.product_id,
                quantity_milli: item.quantity_milli,
                sort_order: i64::try_from(i).unwrap_or(0),
            })
            .collect();
        menu::save_combo(tx, &saved, &items, now).ipc()?;
        let after = serde_json::json!({ "combo": &saved, "items": &items });
        Ok((saved.meta.id, Some(saved.meta.id), after))
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_combo(state: State<'_, AppState>, combo_id: Uuid) -> IpcResult<()> {
    edit(&state, "combos", move |tx, now| {
        if !menu::delete_combo(tx, combo_id, now).ipc()? {
            return Err(IpcError::new(
                IpcErrorCode::NotFound,
                "That combo no longer exists.",
            ));
        }
        Ok(((), Some(combo_id), serde_json::json!({ "deleted": true })))
    })
    .await
}

/// Mirrors `DiningTableInputSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct DiningTableInput {
    pub id: Option<Uuid>,
    pub label: String,
    pub area: String,
    pub seats: i64,
    pub shape: TableShape,
    pub grid_x: i64,
    pub grid_y: i64,
    pub is_active: bool,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_dining_table(
    state: State<'_, AppState>,
    table: DiningTableInput,
) -> IpcResult<DiningTable> {
    let label = name(&table.label, 16)?;
    let area = table.area.trim().to_owned();
    if area.chars().count() > 40 {
        return Err(IpcError::validation("Area names are up to 40 characters."));
    }
    if !(1..=50).contains(&table.seats)
        || !(0..24).contains(&table.grid_x)
        || !(0..16).contains(&table.grid_y)
    {
        return Err(IpcError::validation(
            "Seats 1–50; position within the 24 × 16 floor grid.",
        ));
    }
    edit(&state, "dining_tables", move |tx, now| {
        let all = menu::dining_tables(tx).ipc()?;
        if all
            .iter()
            .any(|t| t.label.eq_ignore_ascii_case(&label) && Some(t.meta.id) != table.id)
        {
            return Err(IpcError::new(
                IpcErrorCode::Conflict,
                format!("There is already a table {label}."),
            ));
        }
        if all.iter().any(|t| {
            t.grid_x == table.grid_x && t.grid_y == table.grid_y && Some(t.meta.id) != table.id
        }) {
            return Err(IpcError::new(
                IpcErrorCode::Conflict,
                "Another table is already at that spot.",
            ));
        }
        let existing = all.iter().find(|t| Some(t.meta.id) == table.id);
        if table.id.is_some() && existing.is_none() {
            return Err(IpcError::new(
                IpcErrorCode::NotFound,
                "That table no longer exists.",
            ));
        }
        let saved = DiningTable {
            meta: meta_for(existing.map(|t| &t.meta), now),
            label,
            area,
            seats: table.seats,
            shape: table.shape,
            grid_x: table.grid_x,
            grid_y: table.grid_y,
            sort_order: table.grid_y * 24 + table.grid_x,
            is_active: table.is_active,
        };
        menu::save_table(tx, &saved, now).ipc()?;
        let after = serde_json::to_value(&saved).unwrap_or_default();
        Ok((saved.clone(), Some(saved.meta.id), after))
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_dining_table(state: State<'_, AppState>, table_id: Uuid) -> IpcResult<()> {
    edit(&state, "dining_tables", move |tx, now| {
        if crate::repo::orders::at_table(tx, table_id).ipc()?.is_some() {
            return Err(IpcError::new(
                IpcErrorCode::Conflict,
                "Close the open order on this table first.",
            ));
        }
        if !menu::delete_table(tx, table_id, now).ipc()? {
            return Err(IpcError::new(
                IpcErrorCode::NotFound,
                "That table no longer exists.",
            ));
        }
        Ok(((), Some(table_id), serde_json::json!({ "deleted": true })))
    })
    .await
}
