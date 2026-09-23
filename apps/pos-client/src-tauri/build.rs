//! Build script for the POS client.
//!
//! 1. Embeds the per-client configuration. The build pipeline points
//!    `POS_CLIENT_CONFIG` at the generator's output; local builds fall back to
//!    the example config. The file is validated with the same rules the app
//!    uses at runtime, so a bad config fails the build, not the shop.
//! 2. Declares the app's IPC commands so each one must be granted explicitly
//!    through a capability file (deny-by-default per window).

use std::path::PathBuf;

const DEFAULT_CONFIG: &str = "../../../packages/shared/contracts/client-config.example.json";

/// Every `#[tauri::command]` exposed by this app. Adding one here generates an
/// `allow-<name>` permission that capabilities must grant.
const COMMANDS: &[&str] = &["app_info"];

fn main() {
    embed_client_config();
    expose_target_triple();

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri build step failed");
}

fn embed_client_config() {
    println!("cargo:rerun-if-env-changed=POS_CLIENT_CONFIG");
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let path = std::env::var("POS_CLIENT_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join(DEFAULT_CONFIG));
    println!("cargo:rerun-if-changed={}", path.display());

    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read client config {}: {e}", path.display()));
    let config = pos_core::config::ClientConfig::parse(&raw)
        .unwrap_or_else(|e| panic!("invalid client config {}: {e}", path.display()));

    let out =
        PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets it")).join("client_config.json");
    let normalized = serde_json::to_string(&config).expect("config serializes");
    std::fs::write(&out, normalized).expect("write embedded client config");
}

fn expose_target_triple() {
    let target = std::env::var("TARGET").expect("cargo sets it");
    println!("cargo:rustc-env=POS_TARGET_TRIPLE={target}");
}
