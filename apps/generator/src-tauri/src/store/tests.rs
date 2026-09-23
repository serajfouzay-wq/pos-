use pos_core::config::BusinessType;
use pos_core::currency::CurrencyCode;
use pos_core::IpcErrorCode;

use super::*;
use crate::clients::{new_client_config, NewClientInput};

fn now() -> Timestamp {
    "2026-09-24T10:00:00.000Z".parse().expect("ts")
}

fn later(seconds: i64) -> Timestamp {
    now()
        .checked_add(chrono::Duration::seconds(seconds))
        .expect("ts")
}

fn config(slug: &str) -> ClientConfig {
    new_client_config(&NewClientInput {
        display_name: format!("Client {slug}"),
        client_slug: slug.into(),
        business_type: BusinessType::Cafe,
        base_currency: CurrencyCode::KWD,
    })
}

#[test]
fn clients_round_trip_and_slugs_are_unique() {
    let store = Store::open_in_memory().expect("store");
    let a = store
        .create_client(&config("alpha"), now())
        .expect("create");
    assert_eq!(a.config.client_slug, "alpha");
    let err = store
        .create_client(&config("alpha"), now())
        .expect_err("duplicate slug");
    assert_eq!(err.code, IpcErrorCode::Conflict);

    store.create_client(&config("beta"), now()).expect("beta");
    let names: Vec<_> = store
        .list_clients()
        .expect("list")
        .into_iter()
        .map(|c| c.client_slug)
        .collect();
    assert_eq!(names, ["alpha", "beta"]);

    // Archiving frees the slug; the row stays.
    store
        .archive_client(a.client_id, later(1))
        .expect("archive");
    assert_eq!(store.list_clients().expect("list").len(), 1);
    assert!(store.client(a.client_id).is_err());
    store
        .create_client(&config("alpha"), later(2))
        .expect("slug reusable");
}

#[test]
fn edits_are_validated_and_identity_is_fixed() {
    let store = Store::open_in_memory().expect("store");
    let created = store
        .create_client(&config("gamma"), now())
        .expect("create");
    let id = created.client_id;

    let mut edited = created.config.clone();
    edited.display_name = "Gamma Coffee".into();
    edited.receipt.header_lines = vec!["Salmiya".into()];
    edited.receipt.logo_asset = Some("sneaky.png".into());
    let saved = store
        .save_client(id, edited.clone(), "VIP", later(1))
        .expect("save");
    assert_eq!(saved.config.display_name, "Gamma Coffee");
    assert_eq!(saved.notes, "VIP");
    assert_eq!(
        saved.config.receipt.logo_asset, None,
        "logo reference is derived"
    );

    let mut renamed = edited.clone();
    renamed.client_slug = "other".into();
    assert!(store.save_client(id, renamed, "", later(2)).is_err());

    let mut invalid = edited;
    invalid.branding.primary_color = "blue".into();
    assert_eq!(
        store
            .save_client(id, invalid, "", later(3))
            .expect_err("invalid")
            .code,
        IpcErrorCode::Validation
    );
}

#[test]
fn assets_replace_and_drive_the_logo_reference() {
    let store = Store::open_in_memory().expect("store");
    let id = store
        .create_client(&config("delta"), now())
        .expect("create")
        .client_id;
    let detail = store
        .put_asset(id, AssetKind::ReceiptLogo, b"one", (10, 5), later(1))
        .expect("logo");
    assert_eq!(
        detail.config.receipt.logo_asset.as_deref(),
        Some("receipt-logo.png")
    );
    store
        .put_asset(id, AssetKind::ReceiptLogo, b"two", (20, 5), later(2))
        .expect("replace");
    assert_eq!(
        store.asset(id, AssetKind::ReceiptLogo).expect("q"),
        Some(b"two".to_vec())
    );
    let detail = store.client(id).expect("detail");
    assert_eq!(detail.assets.len(), 1);
    assert_eq!(detail.assets[0].width, 20);
    assert_eq!(detail.assets[0].sha256, sha256_hex(b"two"));

    let detail = store
        .remove_asset(id, AssetKind::ReceiptLogo, later(3))
        .expect("remove");
    assert_eq!(detail.config.receipt.logo_asset, None);
    assert!(store
        .asset(id, AssetKind::ReceiptLogo)
        .expect("q")
        .is_none());
}

#[test]
fn builds_move_through_their_states() {
    let store = Store::open_in_memory().expect("store");
    let id = store
        .create_client(&config("epsilon"), now())
        .expect("create")
        .client_id;
    let build = store
        .create_build(id, &"ab".repeat(32), "0.1.0", now())
        .expect("build");
    assert_eq!(build.status, BuildStatus::Publishing);
    assert_eq!(store.active_builds().expect("active").len(), 1);

    let queued = store
        .update_build(
            build.build_id,
            &BuildUpdate {
                status: Some(BuildStatus::Queued),
                commit_sha: Some("c0ffee".into()),
                ..BuildUpdate::default()
            },
            later(1),
        )
        .expect("queued");
    assert_eq!(queued.commit_sha.as_deref(), Some("c0ffee"));
    assert!(queued.completed_at.is_none());

    let done = store
        .update_build(
            build.build_id,
            &BuildUpdate {
                status: Some(BuildStatus::Succeeded),
                run_id: Some(7),
                ..BuildUpdate::default()
            },
            later(2),
        )
        .expect("done");
    assert_eq!(done.completed_at, Some(later(2)));
    assert_eq!(done.commit_sha.as_deref(), Some("c0ffee"), "kept");
    assert!(store.active_builds().expect("active").is_empty());
    let summary = store.list_clients().expect("list");
    assert_eq!(
        summary[0].last_build.as_ref().map(|b| b.status),
        Some(BuildStatus::Succeeded)
    );
}

#[test]
fn licenses_are_append_only_history() {
    let store = Store::open_in_memory().expect("store");
    let id = store
        .create_client(&config("zeta"), now())
        .expect("create")
        .client_id;
    let record = IssuedLicenseRecord {
        license_id: Uuid::now_v7(),
        client_id: id,
        device_name: "TILL-1".into(),
        fingerprint_hash: "cd".repeat(32),
        max_devices: 3,
        issued_at: now(),
        expires_at: None,
        token: "eyJ...".into(),
    };
    store.record_license(&record, now()).expect("record");
    assert_eq!(store.licenses(id).expect("list"), vec![record]);
    assert_eq!(store.list_clients().expect("list")[0].license_count, 1);
    let err = store
        .conn()
        .execute("UPDATE issued_licenses SET device_name = 'x'", [])
        .expect_err("append-only");
    assert!(err.to_string().contains("append-only"));
    assert!(store.conn().execute("DELETE FROM clients", []).is_err());
}

#[test]
fn settings_round_trip() {
    let store = Store::open_in_memory().expect("store");
    assert_eq!(store.setting::<String>("k").expect("get"), None);
    store.put_setting("k", &"v", now()).expect("put");
    store.put_setting("k", &"w", later(1)).expect("put");
    assert_eq!(
        store.setting::<String>("k").expect("get").as_deref(),
        Some("w")
    );
}
