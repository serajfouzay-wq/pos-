/// Every `#[tauri::command]` exposed by the generator. Adding one here
/// generates an `allow-<name>` permission that a capability must grant.
const COMMANDS: &[&str] = &["app_info"];

fn main() {
    let target = std::env::var("TARGET").expect("cargo sets it");
    println!("cargo:rustc-env=POS_TARGET_TRIPLE={target}");

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri build step failed");
}
