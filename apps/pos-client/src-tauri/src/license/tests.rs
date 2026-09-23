//! License gate scenarios with fake hardware, a controllable clock and a
//! scripted cloud.

use std::sync::{Arc, Mutex};

use chrono::Duration;
use pos_core::config::ClientConfig;
use pos_core::time::{Clock, Timestamp};
use pos_hwid::HardwareComponents;
use pos_license::issuer::{issue_license, IssueOptions, SigningKey};
use pos_license::keys::parse_public_key_pem;
use pos_license::status::{HaltState, LicenseStatus};
use tempfile::TempDir;

use super::cloud::{CloudDecision, CloudError, CloudRequest, CloudValidator, CloudVerdict};
use super::*;

const CONFIG: &str =
    include_str!("../../../../../packages/shared/contracts/client-config.example.json");
const DEV_PRIVATE: &str = include_str!("../../../../../keys/dev/license-dev.private.pem");
const DEV_PUBLIC: &str = include_str!("../../../../../keys/dev/license-dev.public.pem");

struct FakeClock(Mutex<Timestamp>);

impl FakeClock {
    fn at(s: &str) -> Arc<Self> {
        Arc::new(Self(Mutex::new(s.parse().expect("timestamp"))))
    }
    fn advance(&self, duration: Duration) {
        let mut now = self.0.lock().expect("clock");
        *now = now.checked_add(duration).expect("in range");
    }
    fn set(&self, s: &str) {
        *self.0.lock().expect("clock") = s.parse().expect("timestamp");
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        *self.0.lock().expect("clock")
    }
}

#[derive(Default)]
struct FakeCloud {
    reply: Mutex<Option<Result<CloudVerdict, CloudError>>>,
    seen_tokens: Mutex<Vec<String>>,
}

impl FakeCloud {
    fn answer(&self, status: CloudDecision, server_time: &str) {
        *self.reply.lock().expect("reply") = Some(Ok(CloudVerdict {
            status,
            server_time: server_time.parse().expect("timestamp"),
            reason: None,
        }));
    }
    fn go_offline(&self) {
        *self.reply.lock().expect("reply") = Some(Err(CloudError::Unreachable("offline".into())));
    }
}

impl CloudValidator for FakeCloud {
    fn validate(&self, request: &CloudRequest<'_>) -> Result<CloudVerdict, CloudError> {
        self.seen_tokens
            .lock()
            .expect("seen")
            .push(request.token.to_owned());
        self.reply
            .lock()
            .expect("reply")
            .clone()
            .unwrap_or_else(|| Err(CloudError::Unreachable("no reply scripted".into())))
    }
}

fn machine_a() -> HardwareComponents {
    HardwareComponents::new("Intel i5", "GUID-A", "BOARD-A", "VOL-A").expect("hw")
}

fn machine_b() -> HardwareComponents {
    HardwareComponents::new("Intel i5", "GUID-B", "BOARD-B", "VOL-B").expect("hw")
}

struct Harness {
    dir: TempDir,
    clock: Arc<FakeClock>,
    cloud: Arc<FakeCloud>,
    config: Arc<ClientConfig>,
}

impl Harness {
    fn new() -> Self {
        Self {
            dir: TempDir::new().expect("tempdir"),
            clock: FakeClock::at("2026-09-23T09:00:00.000Z"),
            cloud: Arc::new(FakeCloud::default()),
            config: Arc::new(ClientConfig::parse(CONFIG).expect("config")),
        }
    }

    /// A fresh process on `hardware` (restarts share the data directory).
    fn boot(&self, hardware: HardwareComponents, with_cloud: bool) -> LicenseService {
        let public_key = parse_public_key_pem(DEV_PUBLIC).expect("public key");
        let cloud: Option<Arc<dyn CloudValidator>> = if with_cloud {
            Some(self.cloud.clone())
        } else {
            None
        };
        LicenseService::new(LicenseEnv {
            client: Arc::clone(&self.config),
            key_id: pos_license::keys::key_id(&public_key),
            public_key,
            data_dir: self.dir.path().to_path_buf(),
            hardware: Box::new(move || Ok(hardware.clone())),
            clock: self.clock.clone(),
            cloud,
            app_version: "0.1.0".into(),
            device_name: "TILL-01".into(),
        })
    }

    fn issue_for(&self, service: &LicenseService, expires_at: Option<&str>) -> String {
        let key = SigningKey::from_unencrypted_pem(DEV_PRIVATE).expect("dev key");
        let request = service.activation_request().expect("request");
        let options = IssueOptions {
            client_slug: "dev-demo-cafe".into(),
            business_type: pos_core::config::BusinessType::Cafe,
            max_devices: 2,
            expires_at: expires_at.map(|s| s.parse().expect("timestamp")),
        };
        issue_license(&key, &request, &options, self.clock.now())
            .expect("issue")
            .token
    }
}

fn state(status: &LicenseStatus) -> &'static str {
    match status {
        LicenseStatus::Valid(_) => "valid",
        LicenseStatus::Halted(h) => match h.state {
            HaltState::Missing => "missing",
            HaltState::InvalidToken => "invalid_token",
            HaltState::FingerprintMismatch => "fingerprint_mismatch",
            HaltState::Expired => "expired",
            HaltState::Revoked => "revoked",
            HaltState::GraceExhausted => "grace_exhausted",
            HaltState::HardwareError => "hardware_error",
            HaltState::StorageError => "storage_error",
        },
    }
}

#[test]
fn unactivated_till_is_locked_and_has_no_database() {
    let h = Harness::new();
    let service = h.boot(machine_a(), false);
    assert_eq!(state(&service.evaluate()), "missing");
    assert!(service.database().is_err());
    assert!(
        !h.dir.path().join(DATABASE_FILE).exists(),
        "no DB before a license"
    );
}

#[test]
fn activation_unlocks_and_survives_restart() {
    let h = Harness::new();
    let service = h.boot(machine_a(), false);
    let token = h.issue_for(&service, None);
    let status = service.activate(&token);
    assert_eq!(state(&status), "valid");
    let LicenseStatus::Valid(valid) = status else {
        unreachable!()
    };
    assert_eq!(
        valid.grace_days_remaining, None,
        "no cloud → grace not enforced"
    );
    assert!(!valid.offline);
    assert!(service.database().is_ok());

    // The database on disk is encrypted.
    drop(service);
    let raw = std::fs::read(h.dir.path().join(DATABASE_FILE)).expect("db file");
    assert!(!raw.starts_with(b"SQLite format 3"));

    let restarted = h.boot(machine_a(), false);
    assert_eq!(state(&restarted.evaluate()), "valid");
    let db = restarted.database().expect("db");
    let count: i64 = db
        .conn()
        .query_row(
            "SELECT count(*) FROM license WHERE deleted_at IS NULL",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(count, 1);
}

#[test]
fn a_token_for_another_machine_is_refused_and_not_installed() {
    let h = Harness::new();
    let other = h.boot(machine_b(), false);
    let token_for_b = h.issue_for(&other, None);

    let service = h.boot(machine_a(), false);
    assert_eq!(
        state(&service.activate(&token_for_b)),
        "fingerprint_mismatch"
    );
    assert!(!h.dir.path().join(TOKEN_FILE).exists());
    assert_eq!(state(&service.evaluate()), "missing");
}

#[test]
fn copying_an_activated_till_to_other_hardware_halts_before_opening_data() {
    let h = Harness::new();
    let service = h.boot(machine_a(), false);
    assert_eq!(
        state(&service.activate(&h.issue_for(&service, None))),
        "valid"
    );
    drop(service);
    let db_before = std::fs::read(h.dir.path().join(DATABASE_FILE)).expect("db");

    // Same files, different machine.
    let clone = h.boot(machine_b(), false);
    assert_eq!(state(&clone.evaluate()), "fingerprint_mismatch");
    assert!(clone.database().is_err());
    let db_after = std::fs::read(h.dir.path().join(DATABASE_FILE)).expect("db");
    assert_eq!(db_before, db_after, "database untouched");
}

#[test]
fn reactivation_after_a_hardware_change_starts_a_fresh_database() {
    let h = Harness::new();
    let service = h.boot(machine_a(), false);
    assert_eq!(
        state(&service.activate(&h.issue_for(&service, None))),
        "valid"
    );
    drop(service);

    let replaced = h.boot(machine_b(), false);
    assert_eq!(state(&replaced.evaluate()), "fingerprint_mismatch");
    let token = h.issue_for(&replaced, None);
    assert_eq!(state(&replaced.activate(&token)), "valid");
    let orphans = std::fs::read_dir(h.dir.path())
        .expect("dir")
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".orphaned-"))
        .count();
    assert!(orphans >= 1, "old database kept aside, not deleted");
}

#[test]
fn a_bad_token_never_replaces_a_good_one() {
    let h = Harness::new();
    let service = h.boot(machine_a(), false);
    assert_eq!(
        state(&service.activate(&h.issue_for(&service, None))),
        "valid"
    );
    assert_eq!(
        state(&service.activate("garbage.token.here")),
        "invalid_token"
    );
    assert_eq!(state(&service.status()), "valid", "status unchanged");
    assert!(service.database().is_ok());
}

#[test]
fn expiry_halts_and_cannot_be_dodged_by_winding_the_clock_back() {
    let h = Harness::new();
    let service = h.boot(machine_a(), false);
    let token = h.issue_for(&service, Some("2026-10-01T00:00:00.000Z"));
    assert_eq!(state(&service.activate(&token)), "valid");

    h.clock.set("2026-10-02T00:00:00.000Z");
    assert_eq!(state(&service.evaluate()), "expired");
    assert!(service.database().is_err());

    // Wall clock wound back: the token itself verifies again, but the
    // high-water mark recorded while running keeps it expired.
    h.clock.set("2026-09-24T00:00:00.000Z");
    assert_eq!(state(&service.evaluate()), "expired");
}

#[test]
fn offline_grace_counts_down_and_cloud_contact_restores_it() {
    let h = Harness::new();
    let service = h.boot(machine_a(), true);
    let token = h.issue_for(&service, None);
    let LicenseStatus::Valid(valid) = service.activate(&token) else {
        panic!("expected valid");
    };
    assert_eq!(valid.grace_days_remaining, Some(7));
    assert!(valid.offline, "no successful cloud check yet");

    h.cloud.go_offline();
    h.clock.advance(Duration::days(6) + Duration::hours(12));
    assert_eq!(service.cloud_check(), CloudCheck::Unreachable);
    let LicenseStatus::Valid(valid) = service.status() else {
        panic!("still within grace");
    };
    assert_eq!(valid.grace_days_remaining, Some(1));

    h.clock.advance(Duration::days(1));
    assert_eq!(state(&service.evaluate()), "grace_exhausted");
    assert!(service.database().is_err());

    // Back online: last_seen moves to server time, grace resets to 7 days.
    h.cloud
        .answer(CloudDecision::Active, "2026-10-01T10:00:00.000Z");
    assert_eq!(service.cloud_check(), CloudCheck::Reached);
    let LicenseStatus::Valid(valid) = service.status() else {
        panic!("expected valid after cloud contact");
    };
    assert_eq!(valid.grace_days_remaining, Some(7));
    assert!(!valid.offline);
    assert_eq!(
        h.cloud.seen_tokens.lock().expect("seen").last(),
        Some(&token)
    );
}

#[test]
fn revocation_persists_while_offline_and_across_restarts() {
    let h = Harness::new();
    let service = h.boot(machine_a(), true);
    assert_eq!(
        state(&service.activate(&h.issue_for(&service, None))),
        "valid"
    );

    h.cloud
        .answer(CloudDecision::Revoked, "2026-09-23T10:00:00.000Z");
    assert_eq!(service.cloud_check(), CloudCheck::Reached);
    assert_eq!(state(&service.status()), "revoked");
    assert!(service.database().is_err());
    drop(service);

    h.cloud.go_offline();
    let restarted = h.boot(machine_a(), true);
    assert_eq!(state(&restarted.evaluate()), "revoked");
    restarted.cloud_check();
    assert_eq!(state(&restarted.status()), "revoked");

    // Un-revoked in the cloud → recovers.
    h.cloud
        .answer(CloudDecision::Active, "2026-09-24T10:00:00.000Z");
    restarted.cloud_check();
    assert_eq!(state(&restarted.status()), "valid");
}

#[test]
fn a_transient_hardware_failure_is_retried() {
    let h = Harness::new();
    let public_key = parse_public_key_pem(DEV_PUBLIC).expect("public key");
    let attempts = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&attempts);
    let service = LicenseService::new(LicenseEnv {
        client: Arc::clone(&h.config),
        key_id: pos_license::keys::key_id(&public_key),
        public_key,
        data_dir: h.dir.path().to_path_buf(),
        hardware: Box::new(move || {
            let mut n = counter.lock().expect("counter");
            *n += 1;
            if *n == 1 {
                Err(pos_hwid::HwidError::Timeout)
            } else {
                Ok(machine_a())
            }
        }),
        clock: h.clock.clone(),
        cloud: None,
        app_version: "0.1.0".into(),
        device_name: "TILL-01".into(),
    });
    assert_eq!(state(&service.evaluate()), "hardware_error");
    assert_eq!(
        state(&service.evaluate()),
        "missing",
        "second read succeeds"
    );
    service.evaluate();
    assert_eq!(*attempts.lock().expect("attempts"), 2, "success is cached");
}

#[test]
fn status_changes_are_published() {
    let h = Harness::new();
    let service = h.boot(machine_a(), false);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    service.set_listener(move |status| sink.lock().expect("sink").push(state(status)));
    service.evaluate();
    service.evaluate(); // unchanged → no second event
    service.activate(&h.issue_for(&service, None));
    assert_eq!(*seen.lock().expect("seen"), vec!["missing", "valid"]);
}
