/// Every `#[tauri::command]` exposed by the generator. Adding one here
/// generates an `allow-<name>` permission that a capability must grant.
const COMMANDS: &[&str] = &[
    "app_info",
    "license_key_status",
    "create_license_key",
    "unlock_license_key",
    "lock_license_key",
    "decode_activation_request",
    "issue_license",
    "list_issued_licenses",
    "list_clients",
    "get_client",
    "create_client",
    "save_client",
    "archive_client",
    "upload_client_asset",
    "remove_client_asset",
    "get_client_asset",
    "preview_receipt",
    "get_build_settings",
    "save_build_settings",
    "clear_github_token",
    "check_build_settings",
    "start_build",
    "list_builds",
    "refresh_builds",
    "download_build",
    "open_build_run",
    "reveal_build_download",
];

fn main() {
    let target = std::env::var("TARGET").expect("cargo sets it");
    println!("cargo:rustc-env=POS_TARGET_TRIPLE={target}");

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri build step failed");
}
