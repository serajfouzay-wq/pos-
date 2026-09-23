use pos_core::IpcResult;
use serde::Serialize;
use tauri::AppHandle;

/// Mirrors `GeneratorAppInfoSchema`.
#[derive(Debug, Serialize)]
pub struct AppInfo {
    app: &'static str,
    version: String,
    build_profile: &'static str,
    target: &'static str,
}

#[tauri::command(rename_all = "snake_case")]
pub fn app_info(app: AppHandle) -> IpcResult<AppInfo> {
    Ok(AppInfo {
        app: "generator",
        version: app.package_info().version.to_string(),
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        target: env!("POS_TARGET_TRIPLE"),
    })
}
