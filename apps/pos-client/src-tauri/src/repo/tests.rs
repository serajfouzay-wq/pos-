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
use crate::printing::{PrintService, PrinterIo, PrinterSettings, SETTINGS_KEY};

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
        combo: None,
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
    let service = PrintService::new(printer.clone(), &config(), None);
    let latte = {
        let conn = w.db.conn();
        shifts::open(&conn, w.device, w.manager, 0, now()).expect("shift");
        let chain = PrinterSettings {
            chain: vec![PrinterTarget::Tcp {
                host: "printer".into(),
                port: 9100,
            }],
            ..PrinterSettings::default()
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
fn receipts_print_in_the_printer_language_and_can_wait_to_be_asked_for() {
    let w = world();
    let printer = Arc::new(FakePrinter::default());
    printer.online.store(true, Ordering::SeqCst);
    let service = PrintService::new(printer.clone(), &config(), None);
    let latte = {
        let conn = w.db.conn();
        shifts::open(&conn, w.device, w.manager, 0, now()).expect("shift");
        let settings = PrinterSettings {
            chain: vec![PrinterTarget::Tcp {
                host: "printer".into(),
                port: 9100,
            }],
            language: Some(pos_core::config::Locale::Ar),
            ..PrinterSettings::default()
        };
        settings::put(&conn, SETTINGS_KEY, &settings, now()).expect("settings");
        product(&conn, "Latte", 1_250, Unit::Each, false)
    };
    let sale = payload(
        vec![item(latte, 1000)],
        vec![pay(PaymentMethod::Card, 1_250)],
    );
    let id = sales::create(&mut w.db.conn(), &cashier(&w), &sale, &config(), now())
        .expect("sale")
        .transaction_id;
    assert!(service.drain(&w.db, Some(id)).expect("drain"));
    let bytes = printer.received.lock().expect("lock").remove(0);
    assert!(
        bytes.windows(4).any(|w| w == [0x1D, b'v', b'0', 0]),
        "Arabic receipts print as an image"
    );

    // Receipts only on request: nothing is queued for the next sale.
    {
        let conn = w.db.conn();
        let settings = PrinterSettings {
            auto_print_receipt: false,
            ..PrintService::settings(&conn).expect("settings")
        };
        settings::put(&conn, SETTINGS_KEY, &settings, now()).expect("settings");
    }
    sales::create(
        &mut w.db.conn(),
        &cashier(&w),
        &payload(
            vec![item(latte, 1000)],
            vec![pay(PaymentMethod::Card, 1_250)],
        ),
        &config(),
        now(),
    )
    .expect("sale");
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
    let mut conn = w.db.conn();
    let quote = sales::quote_view(
        sales::quote(&conn, &sale.items, &[], &config(), now()).expect("quote"),
        CurrencyCode::KWD,
        None,
        vec![sales::AppliedDiscount {
            id: Uuid::from_u128(0x0199_a000_0000_7000_8000_0000_0000_00d1),
            name: "Happy hour".into(),
            automatic: true,
        }],
    );
    let receipt = sales::load_receipt(&conn, created.transaction_id, true).expect("receipt");
    let open = shifts::current_open(&conn, w.device)
        .expect("q")
        .expect("open");
    let totals = shifts::totals(&conn, open.meta.id).expect("totals");
    let user = users::get(&conn, w.cashier).expect("q").expect("user");
    let session = Session::new(user.meta.id, user.display_name.clone(), user.role, now());
    let category = catalog::new_category("Coffee", 0, Some("#8B5E3C"), now());

    // Phase 6: a menu with options and a combo, an open table order.
    let (menu_example, order_view, fired_view, paid) = {
        use crate::commands::orders::{FireOutcome, PaidOrder};
        use crate::open_orders::{self, ItemInput, OpenInput, OrderActor, PayInput, UpdateInput};
        use crate::repo::menu::{
            self, Combo, ComboItem, DiningTable, Modifier, ModifierGroup, TableShape,
        };

        let group = ModifierGroup {
            meta: Meta::new(now()),
            name: "Milk".into(),
            name_localized: json!({ "ar": "حليب" }),
            min_select: 0,
            max_select: 1,
            sort_order: 0,
            is_active: true,
        };
        let oat = Modifier {
            meta: Meta::new(now()),
            group_id: group.meta.id,
            name: "Oat".into(),
            name_localized: json!({}),
            price_delta: 200,
            is_default: false,
            sort_order: 0,
            is_active: true,
        };
        menu::save_group(&conn, &group, std::slice::from_ref(&oat), now()).expect("group");
        menu::set_product_groups(&conn, latte, &[group.meta.id], now()).expect("link");
        let croissant = product(&conn, "Croissant", 750, Unit::Each, false);
        let combo = Combo {
            meta: Meta::new(now()),
            name: "Breakfast".into(),
            name_localized: json!({}),
            price: 1_750,
            color: Some("#B7791F".into()),
            sort_order: 0,
            is_active: true,
        };
        let parts: Vec<ComboItem> = [latte, croissant]
            .iter()
            .map(|p| ComboItem {
                meta: Meta::new(now()),
                combo_id: combo.meta.id,
                product_id: *p,
                quantity_milli: 1000,
                sort_order: 0,
            })
            .collect();
        menu::save_combo(&conn, &combo, &parts, now()).expect("combo");
        let table = DiningTable {
            meta: Meta::new(now()),
            label: "T4".into(),
            area: "Hall".into(),
            seats: 4,
            shape: TableShape::Square,
            grid_x: 1,
            grid_y: 1,
            sort_order: 0,
            is_active: true,
        };
        menu::save_table(&conn, &table, now()).expect("table");
        let actor = OrderActor {
            user_id: w.cashier,
            display_name: "Sara".into(),
            role: Role::Cashier,
            device_id: w.device,
        };
        let order = open_orders::open(
            &conn,
            &actor,
            OpenInput {
                table_id: Some(table.meta.id),
                label: None,
                guests: 2,
                order_type: OrderType::DineIn,
            },
            now(),
        )
        .expect("open");
        let later = now().checked_add(Duration::seconds(1)).expect("ts");
        let order = open_orders::update(
            &conn,
            &actor,
            UpdateInput {
                order_id: order.meta.id,
                expected_updated_at: order.meta.updated_at,
                items: vec![ItemInput {
                    line_id: Uuid::now_v7(),
                    product_id: latte,
                    quantity_milli: 1000,
                    modifier_ids: vec![oat.meta.id],
                    course: Some(1),
                    note: Some("extra hot".into()),
                    combo: None,
                }],
                table_id: Some(table.meta.id),
                label: None,
                guests: 2,
                notes: None,
            },
            &config(),
            later,
        )
        .expect("update");
        let later2 = later.checked_add(Duration::seconds(1)).expect("ts");
        let fired = open_orders::fire(
            &conn,
            &actor,
            order.meta.id,
            Some(1),
            order.meta.updated_at,
            &config(),
            later2,
        )
        .expect("fire");
        let fired_order =
            open_orders::view(&conn, fired.order.clone(), &config(), later2).expect("view");
        let outcome = FireOutcome {
            order: fired_order.clone(),
            printed: false,
            print_error: Some("no kitchen printer".into()),
            ticket_text: pos_hardware::kitchen::render_text(&fired.ticket, 80),
        };
        let listed = open_orders::views(&conn, &config(), later2).expect("views");
        let (_, paid) = open_orders::pay(
            &conn,
            &SaleActor {
                user_id: w.cashier,
                role: Role::Cashier,
            },
            &PayInput {
                order_id: order.meta.id,
                idempotency_key: Uuid::new_v4(),
                line_ids: None,
                discount_rule_ids: vec![],
                customer_id: None,
                loyalty_points_to_redeem: 0,
                payments: vec![pay(PaymentMethod::Card, 1_450)],
            },
            &config(),
            later2,
        )
        .expect("pay");
        let paid_receipt = sales::load_receipt(&conn, paid.transaction_id, false).expect("receipt");
        let paid = PaidOrder {
            sale: SaleReceipt {
                receipt: paid_receipt,
                drawer_opened: false,
                print_queued: true,
            },
            order: Some(fired_order),
        };
        (
            menu::menu(&conn, false).expect("menu"),
            listed[0].clone(),
            outcome,
            paid,
        )
    };

    // Phase 7: a refund, history, X then Z, the dashboard, audit, shifts.
    let phase7 = {
        use crate::commands::reports::{shift_history, ReportPrint, ShiftFilter};
        use crate::refunds::{self, RefundInput, RefundLine, RefundMethod, ReverseActor};
        use crate::reports::{self, ReportActor, Window};
        use crate::{history, repo::audit};

        let at = |s: i64| now().checked_add(Duration::seconds(s)).expect("ts");
        let line =
            refunds::original_lines(&conn, created.transaction_id).expect("lines")[0].item_id;
        refunds::refund(
            &mut conn,
            &ReverseActor {
                user_id: w.manager,
                role: Role::Manager,
            },
            &RefundInput {
                transaction_id: created.transaction_id,
                idempotency_key: Uuid::new_v4(),
                lines: vec![RefundLine {
                    item_id: line,
                    quantity_milli: 1000,
                }],
                method: RefundMethod::Cash,
                restock: true,
                reason: "Spilled".into(),
            },
            at(10),
        )
        .expect("refund");
        let by = ReportActor {
            user_id: w.manager,
            role: Role::Manager,
            display_name: "Omar".into(),
        };
        let detail = history::detail(&conn, created.transaction_id).expect("detail");
        let summary = history::list(
            &conn,
            &history::TransactionFilter {
                limit: 5,
                ..Default::default()
            },
        )
        .expect("list")[0]
            .clone();
        let x = reports::x_report(&conn, &by, CurrencyCode::KWD, at(20)).expect("x");
        let print = ReportPrint {
            text: pos_hardware::report::render_text(
                &reports::to_doc(
                    &x,
                    &config(),
                    pos_core::time::Zone::Utc,
                    pos_core::config::Locale::En,
                ),
                80,
            ),
            report: x,
            printed: false,
            print_error: Some("No printer is set up.".into()),
        };
        let shift = shifts::current_open(&conn, w.device)
            .expect("q")
            .expect("open");
        shifts::close(&conn, &shift, w.manager, 24_000, 20_000, None, at(30)).expect("close");
        let z = reports::run_z(&mut conn, &by, CurrencyCode::KWD, at(40)).expect("z");
        let z_summary = reports::z_list(&conn, None, 5, 0).expect("z list")[0].clone();
        let dashboard = reports::dashboard(
            &conn,
            Window {
                from: at(-3_600),
                to: at(3_600),
                device_id: None,
            },
            pos_core::time::Zone::Utc,
            CurrencyCode::KWD,
        )
        .expect("dashboard");
        let audit_page = audit::list(
            &conn,
            &audit::AuditFilter {
                limit: 5,
                ..Default::default()
            },
        )
        .expect("audit");
        let shift_item = shift_history(
            &conn,
            &ShiftFilter {
                limit: 5,
                ..Default::default()
            },
        )
        .expect("shifts")
        .remove(0);
        json!({
            "transaction_detail": detail,
            "transaction_summary": summary,
            "period_report": z,
            "report_print": print,
            "z_report_summary": z_summary,
            "dashboard_data": dashboard,
            "audit_page": audit_page,
            "shift_history_item": shift_item,
        })
    };

    // Phase 8: a customer earning and redeeming points, the kitchen board,
    // the kitchen window and updater status.
    let phase8 = {
        use crate::commands::customers::LoyaltyProgram;
        use crate::commands::kitchen::KitchenDisplayStatus;
        use crate::kitchen::{self, Draft, TicketItem, TicketKind};
        use crate::loyalty::{self, CustomerInput, LoyaltyRequest, PointsAdjustment};
        use crate::repo::{audit::Actor, customers, shop};
        use crate::updater::{UpdateState, UpdateStatus};

        let at = |s: i64| now().checked_add(Duration::seconds(s)).expect("ts");
        shifts::open(&conn, w.device, w.manager, 0, at(50)).expect("shift");
        let owner = Actor {
            user_id: w.manager,
            role: Role::Manager,
            device_id: w.device,
        };
        let layla = loyalty::save(
            &conn,
            &owner,
            CustomerInput {
                id: None,
                display_name: "Layla".into(),
                phone: Some("+965 5555 1234".into()),
                email: Some("layla@example.com".into()),
                notes: None,
            },
            at(51),
        )
        .expect("customer");
        loyalty::adjust(
            &conn,
            &owner,
            &PointsAdjustment {
                customer_id: layla.meta.id,
                points_delta: 500,
                note: "Welcome".into(),
            },
            at(52),
        )
        .expect("adjust");
        // Phase 9: Layla is a Gold member (10% off, double points).
        let gold = crate::repo::memberships::save_plan(
            &conn,
            crate::repo::memberships::PlanInput {
                id: None,
                name: "Gold".into(),
                description: Some("10% off, double points".into()),
                price: 10_000,
                duration_days: 365,
                discount_bps: 1_000,
                points_multiplier_bps: 20_000,
                color: Some("#f59e0b".into()),
                is_active: true,
            },
            &config(),
            at(52),
        )
        .expect("plan");
        crate::repo::memberships::grant(
            &conn,
            layla.meta.id,
            &gold,
            1,
            None,
            0,
            Some("Opening".into()),
            &owner,
            at(52),
        )
        .expect("grant");
        let rule = crate::repo::discounts::save(
            &conn,
            crate::repo::discounts::DiscountRuleInput {
                id: None,
                name: "Happy hour".into(),
                kind: crate::repo::discounts::DiscountKind::Percentage,
                value: 1_500,
                scope: crate::repo::discounts::RuleScope::Order,
                target_id: None,
                min_subtotal: Some(5_000),
                starts_at: None,
                ends_at: None,
                is_active: true,
                apply_mode: crate::repo::discounts::ApplyMode::Automatic,
                days_mask: Some(31),
                time_from: Some(16 * 60),
                time_to: Some(18 * 60),
            },
            at(52),
        )
        .expect("rule");
        let request = LoyaltyRequest {
            customer_id: layla.meta.id,
            redeem_points: 100,
        };
        let priced = loyalty::price(&conn, &sale.items, &[], Some(request), &config(), at(53))
            .expect("quote");
        let loyalty_quote =
            sales::quote_view(priced.cart.quote, CurrencyCode::KWD, priced.loyalty, vec![]);
        let with_points = TransactionPayload {
            idempotency_key: Uuid::new_v4(),
            customer_id: Some(layla.meta.id),
            loyalty_points_to_redeem: 100,
            ..sale.clone()
        };
        let paid = sales::create(&mut conn, &cashier(&w), &with_points, &config(), at(54))
            .expect("sale with points");
        let loyalty_receipt = SaleReceipt {
            receipt: sales::load_receipt(&conn, paid.transaction_id, true).expect("receipt"),
            drawer_opened: false,
            print_queued: true,
        };
        let customer = customers::get(&conn, layla.meta.id).expect("q").expect("c");
        let detail = customers::detail(&conn, customer.clone()).expect("detail");
        let program = LoyaltyProgram {
            available: true,
            settings: shop::loyalty(&conn, &config()).expect("settings"),
        };
        let ticket = kitchen::create(
            &conn,
            w.device,
            Draft {
                kind: TicketKind::Order,
                order_id: Some(Uuid::now_v7()),
                transaction_id: None,
                title: "Table T4".into(),
                order_type: OrderType::DineIn,
                course: Some(1),
                server_name: "Sara".into(),
                guests: 2,
                items: vec![TicketItem {
                    line_id: Uuid::now_v7(),
                    quantity_milli: 2000,
                    name: "Soup".into(),
                    modifiers: vec!["No croutons".into()],
                    note: Some("hot".into()),
                    course: Some(1),
                    done_at: Some(at(56)),
                }],
            },
            at(55),
        )
        .expect("ticket");
        let ready = kitchen::create(
            &conn,
            w.device,
            Draft {
                kind: TicketKind::Void,
                order_id: None,
                transaction_id: Some(paid.transaction_id),
                title: "Takeaway".into(),
                order_type: OrderType::Takeaway,
                course: None,
                server_name: "Sara".into(),
                guests: 0,
                items: ticket.items.clone(),
            },
            at(55),
        )
        .expect("ticket");
        kitchen::bump(&conn, ready.meta.id, true, at(57)).expect("bump");
        let board = kitchen::board(&conn, 30, at(58)).expect("board");
        json!({
            "customer": customer,
            "customer_detail": detail,
            "loyalty_program": program,
            "loyalty_quote": loyalty_quote,
            "loyalty_receipt": loyalty_receipt,
            "kitchen_board": board,
            "kitchen_change": kitchen::Change::from(&ticket),
            "kitchen_display_status": KitchenDisplayStatus { available: true, enabled: true, open: false },
            "discount_rule_view": crate::commands::discounts::DiscountRuleView {
                target_name: None,
                live: false,
                rule,
            },
            "membership_plan_view": crate::commands::memberships::PlanView {
                plan: gold,
                active_members: 1,
            },
            "member_row": crate::repo::memberships::members(
                &conn,
                &crate::repo::memberships::MemberFilter {
                    query: "layla".into(),
                    state: None,
                    customer_id: None,
                    limit: 5,
                },
                at(60),
            )
            .expect("members")
            .remove(0),
            "update_status": UpdateStatus {
                state: UpdateState::Ready,
                current_version: "0.1.3".into(),
                available_version: Some("0.1.4".into()),
                notes: Some("Loyalty points".into()),
                progress_bps: Some(10_000),
                error: None,
                last_checked_at: Some(at(60)),
                updated_from: Some("0.1.2".into()),
                updated_notes: Some("Kitchen display".into()),
            },
        })
    };

    // Map keys are product ids (random per run): pin them for the shape.
    let mut menu_example = serde_json::to_value(&menu_example).expect("menu");
    if let Some(links) = menu_example["product_modifier_groups"].as_object_mut() {
        let values: Vec<Value> = links.values().cloned().collect();
        links.clear();
        for (i, v) in values.into_iter().enumerate() {
            links.insert(format!("00000000-0000-4000-8000-{i:012}"), v);
        }
    }
    let mut examples = json!({
        "menu": menu_example,
        "fire_outcome": fired_view,
        "paid_order": paid,
        "open_order_view": order_view,
        "session": session,
        "session_status": SessionStatus { needs_setup: false, session: Some(session.clone()) },
        "login_user": LoginUser { id: user.meta.id, display_name: user.display_name.clone(), role: user.role, locked_until: Some(now()) },
        "user": users::PublicUser::from(&user),
        "product": catalog::get(&conn, latte).expect("q").expect("product"),
        "category": category,
        "shift_summary": ShiftSummary { expected_cash: open.opening_float + totals.cash_total, shift: open, totals },
        "quote": quote,
        "sale_receipt": SaleReceipt { receipt, drawer_opened: true, print_queued: false },
        "print_outcome": PrintOutcome { printed: false, queued: true },
        "printer_settings": PrinterSettings {
            chain: vec![
                PrinterTarget::WindowsPrinter { name: "EPSON TM-T20III".into() },
                PrinterTarget::Tcp { host: "192.168.1.50".into(), port: 9100 },
                PrinterTarget::Serial { port: "COM5".into(), baud_rate: 9600 },
            ],
            open_drawer_on_cash: true,
            kitchen: Some(PrinterTarget::Tcp { host: "192.168.1.60".into(), port: 9100 }),
            language: Some(pos_core::config::Locale::Ar),
            mode: pos_hardware::doc::PrintMode::Auto,
            paper_width_mm: None,
            kitchen_paper_width_mm: Some(58),
            auto_print_receipt: true,
        },
        "printer_status": PrinterStatus {
            configured: true,
            online: Some(false),
            pending_jobs: 2,
            last_error: Some("offline".into()),
            kitchen_pending: 1,
            kitchen_error: Some("printer 192.168.1.60:9100 is unreachable: timed out".into()),
        },
        "discovered_printer": DiscoveredPrinter {
            target: PrinterTarget::Serial { port: "COM5".into(), baud_rate: 9600 },
            connection: pos_hardware::transport::Connection::Bluetooth,
            label: "COM5 (Bluetooth)".into(),
        },
    });

    if let Some(all) = examples.as_object_mut() {
        for extra in [&phase7, &phase8] {
            if let Some(extra) = extra.as_object() {
                all.extend(extra.clone());
            }
        }
    }

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

fn rule(name: &str, mode: super::discounts::ApplyMode) -> super::discounts::DiscountRuleInput {
    super::discounts::DiscountRuleInput {
        id: None,
        name: name.into(),
        kind: super::discounts::DiscountKind::Percentage,
        value: 1_000,
        scope: super::discounts::RuleScope::Order,
        target_id: None,
        min_subtotal: None,
        starts_at: None,
        ends_at: None,
        is_active: true,
        apply_mode: mode,
        days_mask: None,
        time_from: None,
        time_to: None,
    }
}

#[test]
fn automatic_discounts_price_themselves_on_schedule_and_manual_ones_are_asked_for() {
    use super::discounts::{self, ApplyMode, DiscountKind, RuleScope};
    let w = world();
    let conn = w.db.conn();
    let latte = product(&conn, "Latte", 2_000, Unit::Each, false);
    let cake = product(&conn, "Cake", 1_000, Unit::Each, false);
    let items = [item(latte, 1000), item(cake, 1000)];

    // Wednesday 13:00 in the test zone (UTC+3). A weekday lunch deal on the
    // latte, and a weekend-only offer that must not apply.
    discounts::save(
        &conn,
        discounts::DiscountRuleInput {
            kind: DiscountKind::FixedAmount,
            value: 500,
            scope: RuleScope::Product,
            target_id: Some(latte),
            days_mask: Some(0b001_1111),
            time_from: Some(12 * 60),
            time_to: Some(14 * 60),
            ..rule("Lunch latte", ApplyMode::Automatic)
        },
        now(),
    )
    .expect("lunch");
    discounts::save(
        &conn,
        discounts::DiscountRuleInput {
            days_mask: Some(0b110_0000),
            ..rule("Weekend 10%", ApplyMode::Automatic)
        },
        now(),
    )
    .expect("weekend");
    let staff = discounts::save(&conn, rule("Staff 10%", ApplyMode::Manual), now()).expect("staff");

    let priced = sales::price_cart(&conn, &items, &[], &config(), now()).expect("quote");
    assert_eq!(
        priced.quote.discount_total, 500,
        "only the lunch deal runs now"
    );
    let applied = priced.applied_rules(&priced.quote);
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].name, "Lunch latte");
    assert!(applied[0].automatic);

    // After lunch the deal is over.
    let later = now().checked_add(Duration::hours(2)).expect("later");
    let priced = sales::price_cart(&conn, &items, &[], &config(), later).expect("quote");
    assert_eq!(priced.quote.discount_total, 0);

    // A manual rule applies only when asked for: 10% of what is left.
    let priced =
        sales::price_cart(&conn, &items, &[staff.meta.id], &config(), now()).expect("quote");
    assert_eq!(priced.quote.discount_total, 500 + 250);
    assert_eq!(priced.applied_rules(&priced.quote).len(), 2);

    // A sale records the same amounts.
    shifts::open(&conn, w.device, w.manager, 0, now()).expect("shift");
    drop(conn);
    let mut sale = payload(items.to_vec(), vec![pay(PaymentMethod::Card, 2_250)]);
    sale.discount_rule_ids = vec![staff.meta.id];
    let created =
        sales::create(&mut w.db.conn(), &cashier(&w), &sale, &config(), now()).expect("sale");
    let receipt = sales::load_receipt(&w.db.conn(), created.transaction_id, false).expect("r");
    assert_eq!((receipt.discount_total, receipt.total), (750, 2_250));

    // A rule switched off or outside its dates is refused when asked for.
    let conn = w.db.conn();
    discounts::save(
        &conn,
        discounts::DiscountRuleInput {
            id: Some(staff.meta.id),
            is_active: false,
            ..rule("Staff 10%", ApplyMode::Manual)
        },
        now(),
    )
    .expect("off");
    let err =
        sales::price_cart(&conn, &items, &[staff.meta.id], &config(), now()).expect_err("inactive");
    assert!(err.message.contains("not running"), "{}", err.message);
}

#[test]
fn discount_rules_are_validated() {
    use super::discounts::{self, ApplyMode, DiscountKind, RuleScope};
    let w = world();
    let conn = w.db.conn();
    let bad = [
        discounts::DiscountRuleInput {
            value: 10_001,
            ..rule("Too much", ApplyMode::Manual)
        },
        discounts::DiscountRuleInput {
            kind: DiscountKind::FixedAmount,
            value: 0,
            ..rule("Nothing", ApplyMode::Manual)
        },
        discounts::DiscountRuleInput {
            scope: RuleScope::Product,
            ..rule("No product", ApplyMode::Manual)
        },
        discounts::DiscountRuleInput {
            scope: RuleScope::Category,
            target_id: Some(Uuid::now_v7()),
            ..rule("Gone", ApplyMode::Manual)
        },
        discounts::DiscountRuleInput {
            time_from: Some(600),
            time_to: Some(600),
            ..rule("Empty window", ApplyMode::Manual)
        },
        discounts::DiscountRuleInput {
            starts_at: Some(now()),
            ends_at: Some(now()),
            ..rule("Backwards", ApplyMode::Manual)
        },
        rule("   ", ApplyMode::Manual),
    ];
    for input in bad {
        let name = input.name.clone();
        assert!(discounts::save(&conn, input, now()).is_err(), "{name}");
    }
    // Every day is stored as "no day filter"; deleting keeps the row.
    let saved = discounts::save(
        &conn,
        discounts::DiscountRuleInput {
            days_mask: Some(127),
            ..rule("Always", ApplyMode::Automatic)
        },
        now(),
    )
    .expect("save");
    assert_eq!(saved.days_mask, None);
    discounts::delete(&conn, saved.meta.id, now()).expect("delete");
    assert!(discounts::list(&conn).expect("list").is_empty());
    assert_eq!(count(&conn, "SELECT count(*) FROM discount_rules"), 1);
}
