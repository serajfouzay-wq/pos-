//! Discount rules. Anyone selling sees the list (manual rules are offered
//! to managers at the till); creating, changing and removing rules needs
//! `catalog.manage`, and every change is audited.

use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::IpcResult;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::{authorize, blocking};
use crate::repo::discounts::{self, DiscountRule, DiscountRuleInput, RuleScope, RULE_ZONE};
use crate::repo::{audit, catalog, SqlResultExt};
use crate::state::AppState;

/// Mirrors `DiscountRuleViewSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct DiscountRuleView {
    pub rule: DiscountRule,
    /// The product or category it applies to.
    pub target_name: Option<String>,
    /// Running right now (active, in its dates, days and hours).
    pub live: bool,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_discount_rules(state: State<'_, AppState>) -> IpcResult<Vec<DiscountRuleView>> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    blocking(move || {
        let conn = auth.db.conn();
        let now = SystemClock.now();
        let categories = catalog::categories(&conn).ipc()?;
        discounts::list(&conn)?
            .into_iter()
            .map(|rule| {
                let target_name = match (rule.scope, rule.target_id) {
                    (RuleScope::Product, Some(id)) => {
                        catalog::get(&conn, id).ipc()?.map(|p| p.name)
                    }
                    (RuleScope::Category, Some(id)) => categories
                        .iter()
                        .find(|c| c.meta.id == id)
                        .map(|c| c.name.clone()),
                    _ => None,
                };
                Ok(DiscountRuleView {
                    live: rule.is_live(now, RULE_ZONE),
                    target_name,
                    rule,
                })
            })
            .collect()
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_discount_rule(
    state: State<'_, AppState>,
    rule: DiscountRuleInput,
) -> IpcResult<DiscountRule> {
    let auth = authorize(&state, Permission::CatalogManage)?;
    blocking(move || {
        let actor = auth.actor()?;
        let now = SystemClock.now();
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let before = rule
            .id
            .map(|id| discounts::get(&tx, id))
            .transpose()?
            .flatten();
        let saved = discounts::save(&tx, rule, now)?;
        audit::record(
            &tx,
            &actor,
            "catalog.manage",
            "discount_rules",
            Some(saved.meta.id),
            before.and_then(|b| serde_json::to_value(b).ok()),
            serde_json::to_value(&saved).ok(),
            now,
        )
        .ipc()?;
        tx.commit().ipc()?;
        Ok(saved)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_discount_rule(state: State<'_, AppState>, rule_id: Uuid) -> IpcResult<()> {
    let auth = authorize(&state, Permission::CatalogManage)?;
    blocking(move || {
        let actor = auth.actor()?;
        let now = SystemClock.now();
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let removed = discounts::delete(&tx, rule_id, now)?;
        audit::record(
            &tx,
            &actor,
            "catalog.manage",
            "discount_rules",
            Some(rule_id),
            serde_json::to_value(&removed).ok(),
            None,
            now,
        )
        .ipc()?;
        tx.commit().ipc()?;
        Ok(())
    })
    .await
    .inspect(|_| state.sync.nudge())
}
