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
