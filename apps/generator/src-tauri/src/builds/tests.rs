//! Build orchestration end to end: store + real HTTP client + fake GitHub.

use pos_core::config::BusinessType;
use pos_core::currency::CurrencyCode;
use serde_json::json;

use super::*;
use crate::clients::{new_client_config, NewClientInput};
use crate::github::tests::{FakeGitHubServer, TOKEN};
use crate::github::HttpGitHub;
use crate::secrets::{memory_store, SecretFactory};
use crate::updates::channel::{ChannelRelease, ReleaseChannel};

fn at(seconds: i64) -> Timestamp {
    "2026-09-24T10:00:00.000Z"
        .parse::<Timestamp>()
        .expect("ts")
        .checked_add(Duration::seconds(seconds))
        .expect("ts")
}

/// Records what would go to a client's cloud.
#[derive(Default)]
struct FakeChannel(std::sync::Mutex<Vec<(String, String, String, String)>>);

impl ReleaseChannel for FakeChannel {
    fn publish(
        &self,
        base_url: &str,
        service_key: &str,
        release: &ChannelRelease<'_>,
    ) -> Result<(), String> {
        self.0.lock().expect("channel").push((
            base_url.to_owned(),
            service_key.to_owned(),
            release.path(),
            release.target.to_owned(),
        ));
        Ok(())
    }
}

struct World {
    service: BuildService,
    secrets: SecretFactory,
    channel: Arc<FakeChannel>,
    update_key: Arc<UpdateKey>,
    downloads: PathBuf,
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
    let secrets = memory_store();
    let channel = Arc::new(FakeChannel::default());
    let update_key = Arc::new(UpdateKey::new(secrets(updates::SECRET_NAME)));
    let service = BuildService::new(
        Arc::clone(&store),
        BuildDeps {
            github: Arc::new(HttpGitHub::new()),
            secrets: Arc::clone(&secrets),
            channel: Arc::clone(&channel) as Arc<dyn ReleaseChannel>,
        },
        keys,
        Arc::clone(&update_key),
        "0.1.0".into(),
        downloads.path().to_path_buf(),
    );
    World {
        service,
        secrets,
        channel,
        update_key,
        downloads: downloads.path().to_path_buf(),
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

    let release = ReleaseOptions {
        release_notes: "  Loyalty points  ".into(),
        publish_update: true,
    };
    let build = w
        .service
        .start(w.client_id, &release, at(2))
        .expect("start");
    assert_eq!(build.status, BuildStatus::Queued, "{:?}", build.message);
    // The app is 0.x.y: this client's first build is x.1 of that series.
    assert!(build.app_version.ends_with(".1"), "{}", build.app_version);
    assert_eq!(build.release_notes, "Loyalty points");
    assert!(build.publish_update);
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
        // The tills are built to trust the generator's update key.
        let updater = String::from_utf8_lossy(&files["clients/acme-cafe/updater-public-key.txt"])
            .trim()
            .to_owned();
        assert_eq!(
            Some(updater),
            w.update_key.status().expect("key").public_key,
            "made with the first build"
        );
        assert!(repo.head_message().ends_with("[skip ci]"));
        assert_eq!(
            repo.dispatches,
            vec![json!({
                "client": "acme-cafe",
                "build_id": build.build_id.to_string(),
                "version": build.app_version,
                "notes": "Loyalty points",
            })]
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

    // …and it succeeds with installers for both platforms (the merged
    // artifact wins over a platform's own).
    let artifact = {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in [
            ("windows/Acme Cafe_0.1.1_x64-setup.exe", &b"MZ windows"[..]),
            ("linux/Acme Cafe_0.1.1_amd64.AppImage", b"ELF appimage"),
            ("linux/Acme Cafe_0.1.1_amd64.deb", b"deb"),
        ] {
            zip.start_file(name, options).expect("file");
            zip.write_all(body).expect("write");
        }
        zip.finish().expect("zip").into_inner()
    };
    {
        let mut repo = w.server.repo.lock().expect("repo");
        repo.runs[0]["status"] = json!("completed");
        repo.runs[0]["conclusion"] = json!("success");
        let name = format!("pos-acme-cafe-{}", build.build_id);
        repo.artifacts.insert(
            run_id,
            vec![
                json!({ "id": 76, "name": format!("{name}-windows"), "size_in_bytes": 4, "expired": false }),
                json!({ "id": 77, "name": name, "size_in_bytes": 4, "expired": false }),
            ],
        );
        repo.artifact_zips.insert(77, artifact);
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

    // Publishing online needs a cloud and its service key; the files are
    // saved either way.
    let downloaded = w
        .service
        .download(build.build_id, at(700))
        .expect("download");
    let dir = PathBuf::from(downloaded.download_path.expect("path"));
    assert_eq!(
        dir,
        w.downloads
            .join("POS Factory")
            .join("acme-cafe")
            .join(&build.app_version)
    );
    assert_eq!(
        std::fs::read(dir.join("Acme Cafe_0.1.1_amd64.deb")).expect("deb"),
        b"deb"
    );
    assert!(downloaded
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("no cloud"));
    let windows = dir.join(format!("acme-cafe-{}-windows.posupdate", build.app_version));
    let linux = dir.join(format!("acme-cafe-{}-linux.posupdate", build.app_version));
    assert!(windows.exists() && linux.exists());
    let mut package =
        zip::ZipArchive::new(std::fs::File::open(&windows).expect("open")).expect("zip");
    let manifest: serde_json::Value =
        serde_json::from_reader(package.by_name(updates::MANIFEST).expect("manifest"))
            .expect("json");
    assert_eq!(manifest["target"], "windows-x86_64");
    assert_eq!(manifest["version"], json!(build.app_version));
    assert_eq!(manifest["notes"], "Loyalty points");

    // With a cloud and its key, both platforms are published.
    let mut client = w.store.client(w.client_id).expect("client");
    client.config.cloud.supabase_url = Some("https://acme.supabase.co".into());
    client.config.cloud.supabase_anon_key = Some("anon".into());
    w.store
        .save_client(w.client_id, client.config, "", at(701))
        .expect("save");
    assert!(!w.service.has_service_key(w.client_id).expect("key"));
    assert!(w
        .service
        .set_service_key(w.client_id, Some("bad key"))
        .is_err());
    assert!(w
        .service
        .set_service_key(w.client_id, Some("service-role-jwt"))
        .expect("key"));
    let published = w
        .service
        .download(build.build_id, at(800))
        .expect("download");
    assert!(published
        .message
        .as_deref()
        .unwrap_or_default()
        .starts_with("Published"));
    let sent = w.channel.0.lock().expect("channel").clone();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].0, "https://acme.supabase.co");
    assert_eq!(sent[0].1, "service-role-jwt");
    assert_eq!(
        sent[0].2,
        format!(
            "{}/{}/Acme Cafe_0.1.1_x64-setup.exe",
            w.client_id, build.app_version
        )
    );
    assert_eq!(sent[1].3, "linux-x86_64");
    // The service key never lands in the workspace database.
    assert!(!w.secrets.as_ref()("github-token")
        .get()
        .expect("t")
        .unwrap_or_default()
        .contains("service-role"));
    assert!(!w
        .service
        .set_service_key(w.client_id, None)
        .expect("removed"));
}

#[test]
fn failures_are_recorded_on_the_build() {
    let w = world(false);
    configure(&w);
    let err = w
        .service
        .start(w.client_id, &ReleaseOptions::default(), at(1))
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
    let build = w
        .service
        .start(w.client_id, &ReleaseOptions::default(), at(2))
        .expect("recorded");
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
    let build = w
        .service
        .start(w.client_id, &ReleaseOptions::default(), at(1))
        .expect("start");
    w.server.repo.lock().expect("repo").runs[0]["status"] = json!("completed");
    w.server.repo.lock().expect("repo").runs[0]["conclusion"] = json!("failure");
    let failed = w.service.refresh(build.build_id, at(60)).expect("refresh");
    assert_eq!(failed.status, BuildStatus::Failed);
    assert!(failed.run_url.is_some());

    let lost = w
        .service
        .start(w.client_id, &ReleaseOptions::default(), at(100))
        .expect("start");
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
