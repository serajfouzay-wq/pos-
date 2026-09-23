use pos_core::rbac::{Permission, Role};
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use tauri::State;

use super::session::{validate_name, validate_pin};
use super::{authorize, blocking};
use crate::repo::users::{self, PublicUser};
use crate::repo::{audit, SqlResultExt};
use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn list_users(state: State<'_, AppState>) -> IpcResult<Vec<PublicUser>> {
    let auth = authorize(&state, Permission::UserManage)?;
    blocking(move || {
        Ok(users::list(&auth.db.conn(), false)
            .ipc()?
            .iter()
            .map(PublicUser::from)
            .collect())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_user(
    state: State<'_, AppState>,
    display_name: String,
    role: Role,
    pin: String,
) -> IpcResult<PublicUser> {
    let auth = authorize(&state, Permission::UserManage)?;
    validate_name(&display_name)?;
    validate_pin(&pin)?;
    blocking(move || {
        let hash = users::hash_pin(&pin).map_err(IpcError::internal)?;
        let now = SystemClock.now();
        let actor = auth.actor()?;
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let user = users::create(&tx, &display_name, role, hash, now).ipc()?;
        let public = PublicUser::from(&user);
        audit::record(
            &tx,
            &actor,
            "user.manage",
            "users",
            Some(user.meta.id),
            None,
            serde_json::to_value(&public).ok(),
            now,
        )
        .ipc()?;
        tx.commit().ipc()?;
        Ok(public)
    })
    .await
    .inspect(|_| state.sync.nudge())
}
