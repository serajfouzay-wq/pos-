//! Backups against real encrypted files in a temporary folder.

use std::sync::Mutex;

use pos_hwid::HardwareComponents;
use rusqlite::params;

use super::*;

struct Clockwork(Mutex<Timestamp>);

impl Clock for Clockwork {
    fn now(&self) -> Timestamp {
        *self.0.lock().expect("clock")
    }
}

impl Clockwork {
    fn advance(&self, minutes: i64) {
        let mut now = self.0.lock().expect("clock");
        *now = now.checked_add(Duration::minutes(minutes)).expect("ts");
    }
}

const CLIENT: Uuid = Uuid::from_u128(0x0199_0000_0000_7000_8000_0000_0000_00c1);

fn machine(name: &str) -> pos_hwid::DatabaseKey {
    HardwareComponents::new("CPU", name, "BOARD", "VOL")
        .expect("hw")
        .database_key(CLIENT)
}

struct Till {
    dir: tempfile::TempDir,
    clock: Arc<Clockwork>,
    service: BackupService,
    db: Database,
}

fn till(name: &str) -> Till {
    let dir = tempfile::tempdir().expect("tempdir");
    let clock = Arc::new(Clockwork(Mutex::new(
        "2026-09-29T08:00:00.000Z".parse().expect("ts"),
    )));
    let db = Database::open(&dir.path().join("pos.db"), &machine(name)).expect("db");
    db.conn()
        .execute(
            "INSERT INTO device (id, created_at, updated_at, name) VALUES (?1, ?2, ?2, ?3)",
            params![Uuid::now_v7().to_string(), clock.now().to_string(), name],
        )
        .expect("device");
    let service = BackupService::new(
        dir.path().to_path_buf(),
        CLIENT,
        "0.9.0".into(),
        Arc::clone(&clock) as Arc<dyn Clock>,
    );
    Till {
        dir,
        clock,
        service,
        db,
    }
}

fn users(db: &Database) -> i64 {
    db.conn()
        .query_row("SELECT count(*) FROM users", [], |r| r.get(0))
        .expect("count")
}

fn add_user(db: &Database, name: &str) {
    let hash = crate::repo::users::hash_pin("1234").expect("hash");
    crate::repo::users::create(
        &db.conn(),
        name,
        pos_core::rbac::Role::Cashier,
        hash,
        "2026-09-29T08:00:00.000Z".parse().expect("ts"),
    )
    .expect("user");
}

#[test]
fn a_backup_restores_the_database_as_it_was() {
    let t = till("A");
    add_user(&t.db, "Sara");
    let backup = t.service.create(&t.db, Reason::Manual).expect("backup");
    assert!(!backup.meta.portable);
    assert!(Path::new(&backup.path).exists());
    assert_eq!(t.service.list(None).len(), 1);
    assert_eq!(t.service.check_integrity(&t.db), Integrity::Ok);

    // Later sales… then the owner restores the morning's backup.
    add_user(&t.db, "Omar");
    assert_eq!(users(&t.db), 2);
    let meta = t
        .service
        .stage_restore(Path::new(&backup.path), None, &t.db.export_key())
        .expect("stage");
    assert_eq!(meta.reason, Reason::Manual);
    assert!(t.service.status(&t.db).expect("status").restore_pending);
    drop(t.db);

    // At the next start the staged file replaces the database; the old one is kept.
    assert!(apply_staged_restore(t.dir.path(), "pos.db", t.clock.now()).expect("swap"));
    let reopened = Database::open(&t.dir.path().join("pos.db"), &machine("A")).expect("reopen");
    assert_eq!(users(&reopened), 1, "Sara only: the backup's state");
    let kept = std::fs::read_dir(t.dir.path())
        .expect("dir")
        .filter_map(Result::ok)
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("pos.db.before-restore-")
        });
    assert!(kept, "the replaced database is kept");
    assert!(!apply_staged_restore(t.dir.path(), "pos.db", t.clock.now()).expect("no-op"));
}

#[test]
fn a_password_protected_backup_restores_on_another_pc() {
    let old = till("OLD");
    add_user(&old.db, "Sara");
    old.service
        .set_password(&old.db, "123")
        .expect_err("too short");
    old.service
        .set_password(&old.db, "libya-2026")
        .expect("password");
    let backup = old.service.create(&old.db, Reason::Manual).expect("backup");
    assert!(backup.meta.portable && backup.meta.salt.is_some());

    // The new PC has other hardware, so another database key.
    let new = till("NEW");
    let err = new
        .service
        .stage_restore(
            Path::new(&backup.path),
            Some("wrong-password"),
            &new.db.export_key(),
        )
        .expect_err("wrong password");
    assert!(err.message.contains("password"), "{}", err.message);
    new.service
        .stage_restore(
            Path::new(&backup.path),
            Some("libya-2026"),
            &new.db.export_key(),
        )
        .expect("stage");
    drop(new.db);
    apply_staged_restore(new.dir.path(), "pos.db", new.clock.now()).expect("swap");
    let restored = Database::open(&new.dir.path().join("pos.db"), &machine("NEW")).expect("open");
    assert_eq!(users(&restored), 1);

    // Without a password, a backup only restores on the PC that made it.
    let plain = till("PLAIN");
    let local = plain
        .service
        .create(&plain.db, Reason::Manual)
        .expect("backup");
    let other = till("OTHER");
    let err = other
        .service
        .stage_restore(Path::new(&local.path), None, &other.db.export_key())
        .expect_err("other machine");
    assert!(err.message.contains("another PC"), "{}", err.message);
}

#[test]
fn automatic_backups_are_spaced_pruned_and_copied() {
    let t = till("A");
    let usb = tempfile::tempdir().expect("usb");
    let settings = BackupSettings {
        keep: 3,
        interval_hours: 12,
        extra_dir: Some(usb.path().display().to_string()),
        ..BackupSettings::default()
    };
    settings.validate().expect("valid");
    crate::repo::settings::put(&t.db.conn(), SETTINGS_KEY, &settings, t.clock.now())
        .expect("settings");

    assert!(t
        .service
        .maybe_backup(&t.db, Reason::Start)
        .expect("start")
        .is_some());
    t.clock.advance(60);
    assert!(
        t.service
            .maybe_backup(&t.db, Reason::Scheduled)
            .expect("soon")
            .is_none(),
        "not due for 12 hours"
    );
    for _ in 0..4 {
        t.clock.advance(13 * 60);
        t.service
            .maybe_backup(&t.db, Reason::Scheduled)
            .expect("due")
            .expect("made");
    }
    assert_eq!(t.service.list(None).len(), 3, "only the newest 3 are kept");
    assert_eq!(
        t.service.list(Some(usb.path())).len(),
        3,
        "and copied to the USB stick"
    );
    let status = t.service.status(&t.db).expect("status");
    assert_eq!(status.extra_dir_ok, Some(true));
    assert_eq!(status.last_backup_at, Some(t.clock.now()));

    // A backup from another business is refused.
    let other = BackupService::new(
        t.dir.path().to_path_buf(),
        Uuid::from_u128(7),
        "0.9.0".into(),
        Arc::clone(&t.clock) as Arc<dyn Clock>,
    );
    let newest = t.service.list(None).remove(0);
    let err = other
        .stage_restore(Path::new(&newest.path), None, &t.db.export_key())
        .expect_err("other client");
    assert!(err.message.contains("another business"));
    assert!(BackupSettings {
        keep: 1,
        ..BackupSettings::default()
    }
    .validate()
    .is_err());
}
