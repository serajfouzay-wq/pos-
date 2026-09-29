//! Memberships. Plans and members are visible to anyone who looks up
//! customers (the till shows a customer's membership); creating plans,
//! granting a membership without a sale and cancelling one needs
//! `customer.manage`. Selling a plan is an ordinary sale.

use std::sync::Arc;

use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use super::{authorize, blocking};
use crate::loyalty;
use crate::repo::memberships::{self, MemberFilter, MemberRow, Membership, Plan, PlanInput};
use crate::repo::{audit, SqlResultExt};
use crate::state::AppState;

/// Mirrors `MembershipPlanViewSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct PlanView {
    pub plan: Plan,
    /// Customers holding a period of it right now.
    pub active_members: i64,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_membership_plans(state: State<'_, AppState>) -> IpcResult<Vec<PlanView>> {
    let auth = authorize(&state, Permission::CustomerLookup)?;
    blocking(move || {
        let conn = auth.db.conn();
        let now = SystemClock.now().to_string();
        memberships::plans(&conn)?
            .into_iter()
            .map(|plan| {
                let active_members = conn
                    .query_row(
                        "SELECT count(DISTINCT customer_id) FROM memberships
                         WHERE plan_id = ?1 AND deleted_at IS NULL AND status = 'active'
                           AND starts_at <= ?2 AND ends_at > ?2",
                        rusqlite::params![plan.meta.id.to_string(), now],
                        |r| r.get(0),
                    )
                    .ipc()?;
                Ok(PlanView {
                    plan,
                    active_members,
                })
            })
            .collect()
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_membership_plan(state: State<'_, AppState>, plan: PlanInput) -> IpcResult<Plan> {
    let auth = authorize(&state, Permission::CustomerManage)?;
    let client = Arc::clone(&state.client);
    blocking(move || {
        let actor = auth.actor()?;
        let now = SystemClock.now();
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let before = plan
            .id
            .map(|id| memberships::plan(&tx, id))
            .transpose()?
            .flatten();
        let saved = memberships::save_plan(&tx, plan, &client, now)?;
        audit::record(
            &tx,
            &actor,
            "customer.manage",
            "membership_plans",
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
pub async fn delete_membership_plan(state: State<'_, AppState>, plan_id: Uuid) -> IpcResult<()> {
    let auth = authorize(&state, Permission::CustomerManage)?;
    blocking(move || {
        let actor = auth.actor()?;
        let now = SystemClock.now();
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let removed = memberships::delete_plan(&tx, plan_id, now)?;
        audit::record(
            &tx,
            &actor,
            "customer.manage",
            "membership_plans",
            Some(plan_id),
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

#[tauri::command(rename_all = "snake_case")]
pub async fn list_members(
    state: State<'_, AppState>,
    filter: MemberFilter,
) -> IpcResult<Vec<MemberRow>> {
    let auth = authorize(&state, Permission::CustomerLookup)?;
    blocking(move || memberships::members(&auth.db.conn(), &filter, SystemClock.now())).await
}

/// One customer's periods, newest first.
#[tauri::command(rename_all = "snake_case")]
pub async fn customer_memberships(
    state: State<'_, AppState>,
    customer_id: Uuid,
) -> IpcResult<Vec<MemberRow>> {
    let auth = authorize(&state, Permission::CustomerLookup)?;
    blocking(move || {
        let conn = auth.db.conn();
        let customer = loyalty::live_customer(&conn, customer_id)?;
        memberships::members(
            &conn,
            &MemberFilter {
                query: String::new(),
                state: None,
                customer_id: Some(customer.meta.id),
                limit: 500,
            },
            SystemClock.now(),
        )
    })
    .await
}

/// Mirrors `GrantMembershipSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct GrantMembership {
    pub customer_id: Uuid,
    pub plan_id: Uuid,
    pub notes: Option<String>,
}

/// Gives a customer a period of a plan without a sale (a gift, staff, a
/// correction). Audited.
#[tauri::command(rename_all = "snake_case")]
pub async fn grant_membership(
    state: State<'_, AppState>,
    grant: GrantMembership,
) -> IpcResult<Membership> {
    let auth = authorize(&state, Permission::CustomerManage)?;
    blocking(move || {
        let actor = auth.actor()?;
        let now = SystemClock.now();
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        loyalty::live_customer(&tx, grant.customer_id)?;
        let plan = memberships::plan(&tx, grant.plan_id)?
            .ok_or_else(|| IpcError::validation("That plan was removed."))?;
        let notes = grant
            .notes
            .map(|n| n.trim().chars().take(500).collect::<String>());
        let granted = memberships::grant(
            &tx,
            grant.customer_id,
            &plan,
            1,
            None,
            0,
            notes,
            &actor,
            now,
        )?;
        audit::record(
            &tx,
            &actor,
            "customer.manage",
            "memberships",
            Some(granted.meta.id),
            None,
            serde_json::to_value(&granted).ok(),
            now,
        )
        .ipc()?;
        tx.commit().ipc()?;
        Ok(granted)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn cancel_membership(
    state: State<'_, AppState>,
    membership_id: Uuid,
) -> IpcResult<Membership> {
    let auth = authorize(&state, Permission::CustomerManage)?;
    blocking(move || {
        let actor = auth.actor()?;
        let now = SystemClock.now();
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let before = memberships::get(&tx, membership_id)?;
        let cancelled = memberships::cancel(&tx, membership_id, now)?;
        audit::record(
            &tx,
            &actor,
            "customer.manage",
            "memberships",
            Some(membership_id),
            before.and_then(|b| serde_json::to_value(b).ok()),
            serde_json::to_value(&cancelled).ok(),
            now,
        )
        .ipc()?;
        tx.commit().ipc()?;
        Ok(cancelled)
    })
    .await
    .inspect(|_| state.sync.nudge())
}
