//! Build orchestration end to end: store + real HTTP client + fake GitHub.

use pos_core::config::BusinessType;
use pos_core::currency::CurrencyCode;
use serde_json::json;

use super::*;
use crate::clients::{new_client_config, NewClientInput};
use crate::github::tests::{FakeGitHubServer, TOKEN};
use crate::github::HttpGitHub;
use crate::secrets::MemorySecret;

fn at(seconds: i64) -> Timestamp {
    "2026-09-24T10:00:00.000Z"
        .parse::<Timestamp>()
        .expect("ts")
        .checked_add(Duration::seconds(seconds))
        .expect("ts")
}

struct World {
    service: BuildService,
    store: Arc<Store>,
    server: FakeGitHubServer,
    client_id: Uuid,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

fn world(with_key: bool) -> World {
    let server = FakeGitHubServer::start();
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let keys_dir = tempfile::TempDir::new().expect("tmp");
    let downloads = tempfile::TempDir::new().expect("tmp");
    let keys = Arc::new(KeyStore::new(keys_dir.path().to_path_buf()));
    if with_key {
        keys.create("correct horse battery staple").expect("key");
    }
    let config = new_client_config(&NewClientInput {
        display_name: "Acme Cafe".into(),
        client_slug: "acme-cafe".into(),
        business_type: BusinessType::Cafe,
        base_currency: CurrencyCode::KWD,
    });
    let client_id = store
        .create_client(&config, at(0))
        .expect("client")
        .client_id;
    let service = BuildService::new(
        Arc::clone(&store),
        Arc::new(HttpGitHub::new()),
        Arc::new(MemorySecret::default()),
        keys,
        "0.1.0".into(),
        downloads.path().to_path_buf(),
    );
    World {
        service,
        store,
        server,
        client_id,
        _dirs: (keys_dir, downloads),
    }
}

fn configure(w: &World) {
    let target = &w.server.target;
    w.service
        .save_settings(
            &BuildSettings {
                repo_owner: target.owner.clone(),
                repo_name: target.repo.clone(),
                branch: target.branch.clone(),
                workflow_file: target.workflow_file.clone(),
                api_base_url: target.api_base_url.clone(),
            },
            Some(TOKEN),
            at(0),
        )
        .expect("settings");
}

#[test]
fn settings_are_validated_and_the_token_is_not_stored_in_the_database() {
    let w = world(true);
    assert!(!w.service.settings().expect("settings").token_configured);
    let mut bad = BuildSettings {
        repo_owner: "acme".into(),
        repo_name: "../etc".into(),
        ..BuildSettings::default()
    };
    assert!(w.service.save_settings(&bad, None, at(0)).is_err());
    bad.repo_name = "pos".into();
    bad.api_base_url = "http://evil.example".into();
    assert!(
        w.service.save_settings(&bad, None, at(0)).is_err(),
        "plain http only on loopback"
    );

    configure(&w);
    let view = w.service.settings().expect("settings");
    assert!(view.token_configured);
    let raw: serde_json::Value = w.store.setting(SETTINGS_KEY).expect("q").expect("stored");
    assert!(!raw.to_string().contains(TOKEN));
    assert!(w.service.check().expect("check").workflow_found);
    assert!(!w.service.clear_token().expect("clear").token_configured);
    assert!(w.service.check().is_err());
}

#[test]
fn a_build_publishes_the_client_and_follows_the_run_to_the_installer() {
    let w = world(true);
    configure(&w);
    w.store
        .put_asset(
            w.client_id,
            AssetKind::ReceiptLogo,
            b"logo-bytes",
            (10, 10),
            at(1),
        )
        .expect("logo");

    let build = w.service.start(w.client_id, at(2)).expect("start");
    assert_eq!(build.status, BuildStatus::Queued, "{:?}", build.message);
    assert!(build.commit_sha.is_some());

    {
        let repo = w.server.repo.lock().expect("repo");
        let files = repo.files();
        let config: serde_json::Value =
            serde_json::from_slice(&files["clients/acme-cafe/client.json"]).expect("config");
        assert_eq!(config["client_slug"], "acme-cafe");
        assert_eq!(config["receipt"]["logo_asset"], "receipt-logo.png");
        assert_eq!(files["clients/acme-cafe/receipt-logo.png"], b"logo-bytes");
        assert!(
            String::from_utf8_lossy(&files["clients/acme-cafe/license-public-key.pem"])
                .starts_with("-----BEGIN PUBLIC KEY-----")
        );
        assert!(repo.head_message().ends_with("[skip ci]"));
        assert_eq!(
            repo.dispatches,
            vec![json!({ "client": "acme-cafe", "build_id": build.build_id.to_string() })]
        );
    }

    // GitHub runs it…
    let run_id = {
        let mut repo = w.server.repo.lock().expect("repo");
        repo.runs[0]["status"] = json!("in_progress");
        repo.runs[0]["id"].as_u64().expect("id")
    };
    let refreshed = w.service.refresh_active(at(60)).expect("refresh");
    assert_eq!(refreshed[0].status, BuildStatus::InProgress);
    assert_eq!(refreshed[0].run_id, Some(run_id));

    // …and it succeeds with an installer.
    {
        let mut repo = w.server.repo.lock().expect("repo");
        repo.runs[0]["status"] = json!("completed");
        repo.runs[0]["conclusion"] = json!("success");
        let name = format!("pos-acme-cafe-{}", build.build_id);
        repo.artifacts.insert(
            run_id,
            vec![json!({ "id": 77, "name": name, "size_in_bytes": 4, "expired": false })],
        );
        repo.artifact_zips.insert(77, b"PK\x03\x04".to_vec());
    }
    let done = w.service.refresh(build.build_id, at(600)).expect("refresh");
    assert_eq!(done.status, BuildStatus::Succeeded);
    assert_eq!(done.artifact_id, Some(77));
    assert_eq!(done.completed_at, Some(at(600)));
    assert!(w
        .service
        .refresh_active(at(700))
        .expect("refresh")
        .is_empty());

    let downloaded = w
        .service
        .download(build.build_id, at(700))
        .expect("download");
    let path = downloaded.download_path.expect("path");
    assert!(
        path.ends_with(&format!("acme-cafe/pos-acme-cafe-{}.zip", build.build_id)),
        "{path}"
    );
    assert_eq!(std::fs::read(path).expect("zip"), b"PK\x03\x04");
}

#[test]
fn failures_are_recorded_on_the_build() {
    let w = world(false);
    configure(&w);
    let err = w
        .service
        .start(w.client_id, at(1))
        .expect_err("no signing key");
    assert!(err.message.contains("signing key"));

    let w = world(true);
    configure(&w);
    w.service
        .save_settings(
            &w.service.settings().expect("s").settings,
            Some("wrong-token"),
            at(1),
        )
        .expect("settings");
    let build = w.service.start(w.client_id, at(2)).expect("recorded");
    assert_eq!(build.status, BuildStatus::Error);
    assert!(build
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("refused the token"));
    assert!(w.server.repo.lock().expect("repo").dispatches.is_empty());
}

#[test]
fn a_failed_run_and_a_run_that_never_appears() {
    let w = world(true);
    configure(&w);
    let build = w.service.start(w.client_id, at(1)).expect("start");
    w.server.repo.lock().expect("repo").runs[0]["status"] = json!("completed");
    w.server.repo.lock().expect("repo").runs[0]["conclusion"] = json!("failure");
    let failed = w.service.refresh(build.build_id, at(60)).expect("refresh");
    assert_eq!(failed.status, BuildStatus::Failed);
    assert!(failed.run_url.is_some());

    let lost = w.service.start(w.client_id, at(100)).expect("start");
    w.server.repo.lock().expect("repo").runs.clear();
    assert_eq!(
        w.service
            .refresh(lost.build_id, at(160))
            .expect("refresh")
            .status,
        BuildStatus::Queued
    );
    let gave_up = w
        .service
        .refresh(lost.build_id, at(100 + 31 * 60))
        .expect("refresh");
    assert_eq!(gave_up.status, BuildStatus::Error);
    assert!(gave_up
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("default branch"));
}

#[test]
fn status_mapping() {
    let run = |status: &str, conclusion: Option<&str>| WorkflowRun {
        id: 1,
        status: status.into(),
        conclusion: conclusion.map(str::to_owned),
        html_url: String::new(),
        display_title: String::new(),
    };
    assert_eq!(run_status(&run("queued", None)), BuildStatus::Queued);
    assert_eq!(run_status(&run("waiting", None)), BuildStatus::Queued);
    assert_eq!(
        run_status(&run("in_progress", None)),
        BuildStatus::InProgress
    );
    assert_eq!(
        run_status(&run("completed", Some("success"))),
        BuildStatus::Succeeded
    );
    assert_eq!(
        run_status(&run("completed", Some("timed_out"))),
        BuildStatus::Failed
    );
    assert_eq!(
        run_status(&run("completed", Some("cancelled"))),
        BuildStatus::Cancelled
    );
}
