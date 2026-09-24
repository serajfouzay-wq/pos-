//! Build script for the POS client.
//!
//! 1. Embeds the per-client configuration. The build pipeline points
//!    `POS_CLIENT_CONFIG` at the generator's output; local builds fall back to
//!    the example config. The file is validated with the same rules the app
//!    uses at runtime, so a bad config fails the build, not the shop.
//! 2. Embeds the license-verification PUBLIC key (`POS_LICENSE_PUBLIC_KEY`,
//!    default: the committed development key). A release build refuses the
//!    development key unless `POS_ALLOW_DEV_LICENSE_KEY=1`.
//! 3. Declares the app's IPC commands so each one must be granted explicitly
//!    through a capability file (deny-by-default per window).

use std::path::PathBuf;

const DEFAULT_CONFIG: &str = "../../../packages/shared/contracts/client-config.example.json";
const DEV_LICENSE_KEY: &str = "../../../keys/dev/license-dev.public.pem";

/// Every `#[tauri::command]` exposed by this app. Adding one here generates an
/// `allow-<name>` permission that capabilities must grant.
const COMMANDS: &[&str] = &[
    "app_info",
    "verify_license",
    "get_activation_request",
    "activate_license",
    "session_status",
    "list_login_users",
    "bootstrap_owner",
    "login",
    "logout",
    "list_users",
    "create_user",
    "get_products",
    "get_categories",
    "save_product",
    "load_sample_catalog",
    "current_shift",
    "open_shift",
    "close_shift",
    "quote_transaction",
    "create_transaction",
    "print_receipt",
    "kick_cash_drawer",
    "printer_status",
    "list_printers",
    "get_printer_settings",
    "save_printer_settings",
    "test_printer",
    "sync_to_cloud",
    "sync_status",
    "get_menu",
    "save_modifier_group",
    "delete_modifier_group",
    "set_product_modifier_groups",
    "save_combo",
    "delete_combo",
    "save_dining_table",
    "delete_dining_table",
    "list_open_orders",
    "open_order",
    "update_open_order",
    "split_order_line",
    "fire_course",
    "cancel_open_order",
    "pay_open_order",
    "adjust_stock",
    "print_product_labels",
];

fn main() {
    embed_client_config();
    embed_license_public_key();
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

fn embed_license_public_key() {
    println!("cargo:rerun-if-env-changed=POS_LICENSE_PUBLIC_KEY");
    println!("cargo:rerun-if-env-changed=POS_ALLOW_DEV_LICENSE_KEY");
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let dev_path = manifest_dir.join(DEV_LICENSE_KEY);
    let path = std::env::var("POS_LICENSE_PUBLIC_KEY")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dev_path.clone());
    println!("cargo:rerun-if-changed={}", path.display());

    let load = |p: &PathBuf| {
        let pem = std::fs::read_to_string(p)
            .unwrap_or_else(|e| panic!("cannot read license public key {}: {e}", p.display()));
        let key = pos_license::keys::parse_public_key_pem(&pem)
            .unwrap_or_else(|e| panic!("invalid license public key {}: {e}", p.display()));
        (pem, pos_license::keys::key_id(&key))
    };
    let (pem, key_id) = load(&path);
    let (_, dev_key_id) = load(&dev_path);
    let is_dev = key_id == dev_key_id;

    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    let allow_dev = std::env::var("POS_ALLOW_DEV_LICENSE_KEY").is_ok_and(|v| v == "1");
    if is_dev && release && !allow_dev {
        panic!(
            "refusing to embed the DEVELOPMENT license key in a release build: anyone could mint \
             licenses. Set POS_LICENSE_PUBLIC_KEY to the generator's public key (or \
             POS_ALLOW_DEV_LICENSE_KEY=1 for a throwaway demo build)."
        );
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets it"));
    std::fs::write(out.join("license_public_key.pem"), pem).expect("write embedded public key");
    println!("cargo:rustc-env=POS_LICENSE_KEY_ID={key_id}");
    println!("cargo:rustc-env=POS_LICENSE_KEY_IS_DEV={is_dev}");
}

fn expose_target_triple() {
    let target = std::env::var("TARGET").expect("cargo sets it");
    println!("cargo:rustc-env=POS_TARGET_TRIPLE={target}");
}
