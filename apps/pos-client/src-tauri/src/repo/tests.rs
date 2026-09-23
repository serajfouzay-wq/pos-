//! End-to-end sale path against a real (encrypted, in-memory) database.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Duration;
use pos_core::config::ClientConfig;
use pos_core::currency::CurrencyCode;
use pos_core::rbac::Role;
use pos_core::sales::{OrderType, PaymentMethod};
use pos_core::time::Timestamp;
use pos_core::IpcErrorCode;
use pos_hardware::transport::{DiscoveredPrinter, PrinterTarget, TransportError};
use pos_hwid::HardwareComponents;
use rusqlite::{params, Connection};
use uuid::Uuid;

use super::catalog::{self, Product, Unit};
use super::sales::{self, PayloadItem, PayloadPayment, SaleActor, TransactionPayload};
use super::users::{self, LoginOutcome};
use super::{print_jobs, settings, shifts, Meta};
use crate::db::Database;
use crate::printing::{template_for, PrintService, PrinterIo, PrinterSettings, SETTINGS_KEY};

const CONFIG: &str =
    include_str!("../../../../../packages/shared/contracts/client-config.example.json");

fn now() -> Timestamp {
    "2026-09-23T10:00:00.000Z".parse().expect("ts")
}

fn config() -> ClientConfig {
    ClientConfig::parse(CONFIG).expect("config")
}

struct World {
    db: Arc<Database>,
    device: Uuid,
    cashier: Uuid,
    manager: Uuid,
}

fn world() -> World {
    let hw = HardwareComponents::new("CPU", "GUID", "BOARD", "VOL").expect("hw");
    let db = Arc::new(Database::open_in_memory(&hw.database_key(Uuid::nil())).expect("db"));
    let device = Uuid::now_v7();
    let (cashier, manager) = {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO device (id, created_at, updated_at, name) VALUES (?1, ?2, ?2, 'TILL')",
            params![device.to_string(), now().to_string()],
        )
        .expect("device");
        let hash = users::hash_pin("1234").expect("hash");
        let cashier =
            users::create(&conn, "Sara", Role::Cashier, hash.clone(), now()).expect("cashier");
        let manager = users::create(&conn, "Omar", Role::Manager, hash, now()).expect("manager");
        (cashier.meta.id, manager.meta.id)
    };
    World {
        db,
        device,
        cashier,
        manager,
    }
}

fn product(conn: &Connection, name: &str, price: i64, unit: Unit, track_stock: bool) -> Uuid {
    let p = Product {
        meta: Meta::new(now()),
        name: name.into(),
        name_localized: serde_json::json!({}),
        category_id: None,
        sku: None,
        barcode: None,
        price,
        cost: None,
        tax_rate_bps: 500,
        unit,
        sold_by_weight: unit != Unit::Each,
        track_stock,
        stock_on_hand_milli: 0,
        reorder_threshold_milli: None,
        reorder_quantity_milli: None,
        image_asset: None,
        quick_key_position: None,
        is_active: true,
    };
    catalog::save(conn, &p, now()).expect("product");
    p.meta.id
}

fn item(product_id: Uuid, quantity_milli: i64) -> PayloadItem {
    PayloadItem {
        product_id,
        quantity_milli,
        modifier_ids: vec![],
        course: None,
        note: None,
    }
}

fn pay(method: PaymentMethod, amount: i64) -> PayloadPayment {
    PayloadPayment {
        method,
        tendered_currency: CurrencyCode::KWD,
        tendered_amount: amount,
        reference: None,
    }
}

fn payload(items: Vec<PayloadItem>, payments: Vec<PayloadPayment>) -> TransactionPayload {
    TransactionPayload {
        idempotency_key: Uuid::new_v4(),
        customer_id: None,
        order_type: OrderType::Counter,
        table_label: None,
        items,
        discount_rule_ids: vec![],
        loyalty_points_to_redeem: 0,
        payments,
        notes: None,
    }
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).expect("count")
}

fn cashier(w: &World) -> SaleActor {
    SaleActor {
        user_id: w.cashier,
        role: Role::Cashier,
    }
}

#[test]
fn a_sale_writes_everything_in_one_transaction() {
    let w = world();
    let (latte, dates) = {
        let conn = w.db.conn();
        shifts::open(&conn, w.device, w.manager, 20_000, now()).expect("shift");
        (
            product(&conn, "Latte", 1_250, Unit::Each, false),
            product(&conn, "Dates", 2_500, Unit::Kg, true),
        )
    };
    let sale = payload(
        vec![item(latte, 2000), item(dates, 500)],
        vec![pay(PaymentMethod::Cash, 5_000)],
    );
    let created =
        sales::create(&mut w.db.conn(), &cashier(&w), &sale, &config(), now()).expect("sale");
    assert!(created.is_new && created.includes_cash);

    let conn = w.db.conn();
    let receipt = sales::load_receipt(&conn, created.transaction_id, false).expect("receipt");
    // 2 × 1.250 + 0.5 kg × 2.500 = 3.750, prices include 5 % tax.
    assert_eq!(receipt.total, 3_750);
    assert_eq!(receipt.change_due, 1_250);
    assert_eq!(receipt.cashier_name, "Sara");
    assert_eq!(
        receipt.tax_lines.iter().map(|t| t.tax_amount).sum::<i64>(),
        179
    );
    assert!(receipt.receipt_number.ends_with("-000001"));
    assert_eq!(receipt.lines[0].name, "Latte", "name snapshot");

    // Stock only for the tracked product, as an additive delta.
    assert_eq!(count(&conn, "SELECT count(*) FROM stock_movements"), 1);
    let stock = catalog::get(&conn, dates)
        .expect("q")
        .expect("product")
        .stock_on_hand_milli;
    assert_eq!(stock, -500);

    // Outbox: every synced row, nothing for local-only tables.
    for (entity, expected) in [
        ("transactions", 1),
        ("transaction_items", 2),
        ("transaction_payments", 1),
        ("stock_movements", 1),
    ] {
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sync_queue WHERE entity_type = ?1",
                [entity],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(n, expected, "{entity}");
    }
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM audit_log WHERE action = 'sale.create'"
        ),
        1
    );
    assert_eq!(print_jobs::pending_count(&conn).expect("jobs"), 1);
}

#[test]
fn a_retried_sale_is_recorded_once() {
    let w = world();
    let latte = {
        let conn = w.db.conn();
        shifts::open(&conn, w.device, w.manager, 0, now()).expect("shift");
        product(&conn, "Latte", 1_250, Unit::Each, false)
    };
    let sale = payload(
        vec![item(latte, 1000)],
        vec![pay(PaymentMethod::Card, 1_250)],
    );
    let first =
        sales::create(&mut w.db.conn(), &cashier(&w), &sale, &config(), now()).expect("first");
    let second =
        sales::create(&mut w.db.conn(), &cashier(&w), &sale, &config(), now()).expect("retry");
    assert_eq!(first.transaction_id, second.transaction_id);
    assert!(!second.is_new);
    assert_eq!(count(&w.db.conn(), "SELECT count(*) FROM transactions"), 1);
}

#[test]
fn a_rejected_sale_leaves_no_trace() {
    let w = world();
    let latte = {
        let conn = w.db.conn();
        shifts::open(&conn, w.device, w.manager, 0, now()).expect("shift");
        product(&conn, "Latte", 1_250, Unit::Each, true)
    };
    let before = count(&w.db.conn(), "SELECT count(*) FROM sync_queue");
    for bad in [
        payload(
            vec![item(latte, 1000)],
            vec![pay(PaymentMethod::Card, 2_000)],
        ), // card overpays
        payload(
            vec![item(latte, 1000)],
            vec![pay(PaymentMethod::Cash, 1_000)],
        ), // short
        payload(
            vec![item(latte, 1500)],
            vec![pay(PaymentMethod::Cash, 5_000)],
        ), // half a latte
        payload(
            vec![item(Uuid::now_v7(), 1000)],
            vec![pay(PaymentMethod::Cash, 5_000)],
        ), // unknown
    ] {
        let err = sales::create(&mut w.db.conn(), &cashier(&w), &bad, &config(), now())
            .expect_err("rejected");
        assert_eq!(err.code, IpcErrorCode::Validation, "{err:?}");
    }
    let conn = w.db.conn();
    assert_eq!(count(&conn, "SELECT count(*) FROM transactions"), 0);
    assert_eq!(count(&conn, "SELECT count(*) FROM stock_movements"), 0);
    assert_eq!(count(&conn, "SELECT count(*) FROM sync_queue"), before);
}

#[test]
fn selling_requires_an_open_shift() {
    let w = world();
    let latte = product(&w.db.conn(), "Latte", 1_250, Unit::Each, false);
    let sale = payload(
        vec![item(latte, 1000)],
        vec![pay(PaymentMethod::Cash, 2_000)],
    );
    let err = sales::create(&mut w.db.conn(), &cashier(&w), &sale, &config(), now())
        .expect_err("no shift");
    assert_eq!(err.code, IpcErrorCode::Conflict);
}

#[test]
fn closing_a_shift_reconciles_the_drawer() {
    let w = world();
    let latte = {
        let conn = w.db.conn();
        shifts::open(&conn, w.device, w.manager, 20_000, now()).expect("shift");
        product(&conn, "Latte", 1_250, Unit::Each, false)
    };
    for payments in [
        vec![pay(PaymentMethod::Cash, 5_000)], // cash 1.250, change 3.750
        vec![
            pay(PaymentMethod::Card, 1_000),
            pay(PaymentMethod::Cash, 250),
        ], // split
    ] {
        let sale = payload(vec![item(latte, 1000)], payments);
        sales::create(&mut w.db.conn(), &cashier(&w), &sale, &config(), now()).expect("sale");
    }
    let conn = w.db.conn();
    let open = shifts::current_open(&conn, w.device)
        .expect("q")
        .expect("open");
    let totals = shifts::totals(&conn, open.meta.id).expect("totals");
    assert_eq!(
        (
            totals.cash_total,
            totals.card_total,
            totals.transaction_count
        ),
        (1_500, 1_000, 2)
    );
    let closed =
        shifts::close(&conn, &open, w.manager, 21_400, 20_000, None, now()).expect("close");
    assert_eq!(closed.expected_cash, Some(21_500));
    assert_eq!(closed.variance, Some(-100), "100 fils short");
    assert!(shifts::current_open(&conn, w.device).expect("q").is_none());
}

#[derive(Default)]
struct FakePrinter {
    online: AtomicBool,
    received: Mutex<Vec<Vec<u8>>>,
}

impl PrinterIo for FakePrinter {
    fn send(&self, target: &PrinterTarget, bytes: &[u8]) -> Result<(), TransportError> {
        if self.online.load(Ordering::SeqCst) {
            self.received.lock().expect("lock").push(bytes.to_vec());
            Ok(())
        } else {
            Err(TransportError::Unreachable {
                target: target.label(),
                reason: "off".into(),
            })
        }
    }
    fn discover(&self) -> Vec<DiscoveredPrinter> {
        Vec::new()
    }
}

#[test]
fn receipts_queue_while_offline_and_print_in_order_later() {
    let w = world();
    let printer = Arc::new(FakePrinter::default());
    let service = PrintService::new(printer.clone(), template_for(&config(), None));
    let latte = {
        let conn = w.db.conn();
        shifts::open(&conn, w.device, w.manager, 0, now()).expect("shift");
        let chain = PrinterSettings {
            chain: vec![PrinterTarget::Tcp {
                host: "printer".into(),
                port: 9100,
            }],
            open_drawer_on_cash: true,
        };
        settings::put(&conn, SETTINGS_KEY, &chain, now()).expect("settings");
        product(&conn, "Latte", 1_250, Unit::Each, false)
    };
    let mut ids = Vec::new();
    for _ in 0..3 {
        let sale = payload(
            vec![item(latte, 1000)],
            vec![pay(PaymentMethod::Card, 1_250)],
        );
        ids.push(
            sales::create(&mut w.db.conn(), &cashier(&w), &sale, &config(), now())
                .expect("sale")
                .transaction_id,
        );
        assert!(
            !service.drain(&w.db, ids.last().copied()).expect("drain"),
            "printer is off"
        );
    }
    assert_eq!(service.status(&w.db).expect("status").pending_jobs, 3);
    assert_eq!(service.status(&w.db).expect("status").online, Some(false));
    assert!(
        service.kick_drawer(&w.db).is_err(),
        "drawer kicks are not queued"
    );

    printer.online.store(true, Ordering::SeqCst);
    service.drain(&w.db, None).expect("drain");
    let received = printer.received.lock().expect("lock");
    assert_eq!(received.len(), 3);
    for (i, bytes) in received.iter().enumerate() {
        let text = String::from_utf8_lossy(bytes);
        assert!(
            text.contains(&format!("-00000{}", i + 1)),
            "receipt {} printed in order",
            i + 1
        );
    }
    assert_eq!(service.status(&w.db).expect("status").pending_jobs, 0);
}

#[test]
fn five_wrong_pins_lock_the_user() {
    let w = world();
    let conn = w.db.conn();
    for left in (1..=4).rev() {
        assert_eq!(
            users::attempt_login(&conn, w.cashier, "0000", now()).expect("attempt"),
            LoginOutcome::WrongPin {
                attempts_left: left
            }
        );
    }
    let locked = users::attempt_login(&conn, w.cashier, "0000", now()).expect("attempt");
    assert!(matches!(locked, LoginOutcome::Locked { .. }));
    // Even the right PIN is refused while locked…
    assert!(matches!(
        users::attempt_login(&conn, w.cashier, "1234", now()).expect("attempt"),
        LoginOutcome::Locked { .. }
    ));
    // …and accepted afterwards.
    let later = now().checked_add(Duration::minutes(6)).expect("ts");
    assert!(matches!(
        users::attempt_login(&conn, w.cashier, "1234", later).expect("attempt"),
        LoginOutcome::Success(_)
    ));
}

#[test]
fn the_sample_catalogue_loads_for_every_business_type() {
    use pos_core::config::BusinessType;
    for business in [
        BusinessType::Retail,
        BusinessType::Cafe,
        BusinessType::Restaurant,
    ] {
        let w = world();
        let conn = w.db.conn();
        let actor = super::audit::Actor {
            user_id: w.manager,
            role: Role::Owner,
            device_id: w.device,
        };
        let n = crate::sample_catalog::load(&conn, business, CurrencyCode::KWD, 0, &actor, now())
            .expect("load");
        assert!(n >= 10);
        assert_eq!(
            catalog::count(&conn).expect("count"),
            i64::try_from(n).expect("n")
        );
    }
}

/// Emits one example of every Phase 3 response shape to
/// `contracts/pos-examples.json`; the TS test parses each with its Zod schema.
/// Values vary run to run (ids, timestamps), so the committed file is compared
/// by *shape* (keys and JSON types) — a renamed or added field fails here
/// until the fixture is regenerated and the TS schemas agree.
#[test]
fn pos_response_shapes_match_the_contract_fixture() {
    use serde_json::{json, Value};

    use crate::commands::sales::{PrintOutcome, SaleReceipt};
    use crate::commands::session::{LoginUser, SessionStatus};
    use crate::commands::shifts::ShiftSummary;
    use crate::printing::PrinterStatus;
    use crate::session::Session;

    let w = world();
    let latte = {
        let conn = w.db.conn();
        shifts::open(&conn, w.device, w.manager, 20_000, now()).expect("shift");
        product(&conn, "Latte", 1_250, Unit::Each, false)
    };
    let sale = payload(
        vec![item(latte, 2000)],
        vec![pay(PaymentMethod::Cash, 5_000)],
    );
    let created =
        sales::create(&mut w.db.conn(), &cashier(&w), &sale, &config(), now()).expect("sale");
    let conn = w.db.conn();
    let quote = sales::quote_view(
        sales::quote(&conn, &sale.items, &[], &config(), now()).expect("quote"),
        CurrencyCode::KWD,
    );
    let receipt = sales::load_receipt(&conn, created.transaction_id, true).expect("receipt");
    let open = shifts::current_open(&conn, w.device)
        .expect("q")
        .expect("open");
    let totals = shifts::totals(&conn, open.meta.id).expect("totals");
    let user = users::get(&conn, w.cashier).expect("q").expect("user");
    let session = Session::new(user.meta.id, user.display_name.clone(), user.role, now());
    let category = catalog::new_category("Coffee", 0, Some("#8B5E3C"), now());

    let examples = json!({
        "session": session,
        "session_status": SessionStatus { needs_setup: false, session: Some(session.clone()) },
        "login_user": LoginUser { id: user.meta.id, display_name: user.display_name.clone(), role: user.role, locked_until: Some(now()) },
        "user": users::PublicUser::from(&user),
        "product": catalog::get(&conn, latte).expect("q").expect("product"),
        "category": category,
        "shift_summary": ShiftSummary { expected_cash: open.opening_float + totals.cash_total, shift: open, totals },
        "quote": quote,
        "sale_receipt": SaleReceipt { receipt, drawer_opened: true },
        "print_outcome": PrintOutcome { printed: false, queued: true },
        "printer_settings": PrinterSettings {
            chain: vec![
                PrinterTarget::WindowsPrinter { name: "EPSON TM-T20III".into() },
                PrinterTarget::Tcp { host: "192.168.1.50".into(), port: 9100 },
                PrinterTarget::Serial { port: "COM5".into(), baud_rate: 9600 },
            ],
            open_drawer_on_cash: true,
        },
        "printer_status": PrinterStatus { configured: true, online: Some(false), pending_jobs: 2, last_error: Some("offline".into()) },
        "discovered_printer": DiscoveredPrinter {
            target: PrinterTarget::Serial { port: "COM5".into(), baud_rate: 9600 },
            connection: pos_hardware::transport::Connection::Bluetooth,
            label: "COM5 (Bluetooth)".into(),
        },
    });

    fn shape(v: &Value) -> Value {
        match v {
            Value::Object(map) => {
                Value::Object(map.iter().map(|(k, v)| (k.clone(), shape(v))).collect())
            }
            Value::Array(items) => Value::Array(items.first().map(shape).into_iter().collect()),
            Value::String(_) => json!("string"),
            Value::Number(_) => json!("number"),
            Value::Bool(_) => json!("bool"),
            Value::Null => json!("null"),
        }
    }

    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../packages/shared/contracts/pos-examples.json"
    );
    let document = json!({
        "_comment": "Generated by apps/pos-client/src-tauri/src/repo/tests.rs (POS_REGENERATE_FIXTURES=1). Compared by shape.",
        "examples": examples,
    });
    if std::env::var_os("POS_REGENERATE_FIXTURES").is_some() {
        std::fs::write(
            path,
            serde_json::to_string_pretty(&document).expect("json") + "\n",
        )
        .expect("write");
    }
    let committed: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("parse");
    assert_eq!(
        shape(&committed),
        shape(&document),
        "regenerate with POS_REGENERATE_FIXTURES=1"
    );
}
