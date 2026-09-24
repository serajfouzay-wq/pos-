//! Modifiers, combos, open orders (courses, split bills), stock — against a
//! real encrypted in-memory database.

use std::sync::Arc;

use pos_core::config::ClientConfig;
use pos_core::currency::CurrencyCode;
use pos_core::rbac::Role;
use pos_core::sales::{OrderType, PaymentMethod};
use pos_core::time::Timestamp;
use pos_core::IpcErrorCode;
use pos_hwid::HardwareComponents;
use rusqlite::params;
use uuid::Uuid;

use super::*;
use crate::db::Database;
use crate::inventory::{self, StockAdjustment, StockMode};
use crate::repo::catalog::{Product, Unit};
use crate::repo::menu::{Combo, ComboItem, DiningTable, Modifier, ModifierGroup, TableShape};
use crate::repo::sales::PayloadPayment;
use crate::repo::{shifts, users};

const CONFIG: &str =
    include_str!("../../../../../packages/shared/contracts/client-config.example.json");

fn at(seconds: i64) -> Timestamp {
    "2026-09-24T12:00:00.000Z"
        .parse::<Timestamp>()
        .expect("ts")
        .checked_add(chrono::Duration::seconds(seconds))
        .expect("ts")
}

fn config() -> ClientConfig {
    ClientConfig::parse(CONFIG).expect("config")
}

struct World {
    db: Arc<Database>,
    cashier: OrderActor,
    manager: OrderActor,
    latte: Uuid,
    croissant: Uuid,
    juice: Uuid,
    size_small: Uuid,
    size_large: Uuid,
    oat: Uuid,
    combo: Uuid,
    table: Uuid,
}

fn product(conn: &Connection, name: &str, price: i64, track: bool) -> Uuid {
    let p = Product {
        meta: Meta::new(at(0)),
        name: name.into(),
        name_localized: serde_json::json!({}),
        category_id: None,
        sku: None,
        barcode: None,
        price,
        cost: None,
        tax_rate_bps: 0,
        unit: Unit::Each,
        sold_by_weight: false,
        track_stock: track,
        stock_on_hand_milli: 0,
        reorder_threshold_milli: None,
        reorder_quantity_milli: None,
        image_asset: None,
        quick_key_position: None,
        is_active: true,
    };
    catalog::save(conn, &p, at(0)).expect("product");
    p.meta.id
}

fn group(
    conn: &Connection,
    name: &str,
    min: i64,
    max: i64,
    options: &[(&str, i64)],
) -> (Uuid, Vec<Uuid>) {
    let g = ModifierGroup {
        meta: Meta::new(at(0)),
        name: name.into(),
        name_localized: serde_json::json!({}),
        min_select: min,
        max_select: max,
        sort_order: 0,
        is_active: true,
    };
    let mods: Vec<Modifier> = options
        .iter()
        .enumerate()
        .map(|(i, (n, delta))| Modifier {
            meta: Meta::new(at(0)),
            group_id: g.meta.id,
            name: (*n).into(),
            name_localized: serde_json::json!({}),
            price_delta: *delta,
            is_default: i == 0,
            sort_order: i64::try_from(i).expect("i"),
            is_active: true,
        })
        .collect();
    menu::save_group(conn, &g, &mods, at(0)).expect("group");
    (g.meta.id, mods.iter().map(|m| m.meta.id).collect())
}

fn world() -> World {
    let hw = HardwareComponents::new("CPU", "GUID", "BOARD", "VOL").expect("hw");
    let db = Arc::new(Database::open_in_memory(&hw.database_key(Uuid::nil())).expect("db"));
    let device = Uuid::now_v7();
    let conn = db.conn();
    conn.execute(
        "INSERT INTO device (id, created_at, updated_at, name) VALUES (?1, ?2, ?2, 'TILL')",
        params![device.to_string(), at(0).to_string()],
    )
    .expect("device");
    let hash = users::hash_pin("1234").expect("hash");
    let cashier =
        users::create(&conn, "Sara", Role::Cashier, hash.clone(), at(0)).expect("cashier");
    let manager = users::create(&conn, "Omar", Role::Manager, hash, at(0)).expect("manager");
    shifts::open(&conn, device, manager.meta.id, 10_000, at(0)).expect("shift");

    let latte = product(&conn, "Latte", 1_500, false);
    let croissant = product(&conn, "Croissant", 900, true);
    let juice = product(&conn, "Juice", 1_100, false);
    let (size, sizes) = group(&conn, "Size", 1, 1, &[("Small", 0), ("Large", 500)]);
    let (milk, milks) = group(&conn, "Milk", 0, 1, &[("Oat", 200), ("Almond", 200)]);
    menu::set_product_groups(&conn, latte, &[size, milk], at(0)).expect("links");

    let combo = Combo {
        meta: Meta::new(at(0)),
        name: "Breakfast".into(),
        name_localized: serde_json::json!({}),
        price: 2_000,
        color: None,
        sort_order: 0,
        is_active: true,
    };
    let items = [latte, croissant]
        .iter()
        .enumerate()
        .map(|(i, p)| ComboItem {
            meta: Meta::new(at(0)),
            combo_id: combo.meta.id,
            product_id: *p,
            quantity_milli: 1000,
            sort_order: i64::try_from(i).expect("i"),
        })
        .collect::<Vec<_>>();
    menu::save_combo(&conn, &combo, &items, at(0)).expect("combo");
    let table = DiningTable {
        meta: Meta::new(at(0)),
        label: "T4".into(),
        area: "Terrace".into(),
        seats: 4,
        shape: TableShape::Round,
        grid_x: 3,
        grid_y: 2,
        sort_order: 0,
        is_active: true,
    };
    menu::save_table(&conn, &table, at(0)).expect("table");
    drop(conn);

    let actor = |u: &users::UserRow| OrderActor {
        user_id: u.meta.id,
        display_name: u.display_name.clone(),
        role: u.role,
        device_id: device,
    };
    World {
        cashier: actor(&cashier),
        manager: actor(&manager),
        db,
        latte,
        croissant,
        juice,
        size_small: sizes[0],
        size_large: sizes[1],
        oat: milks[0],
        combo: combo.meta.id,
        table: table.meta.id,
    }
}

fn item(product: Uuid, qty: i64, modifiers: &[Uuid]) -> PayloadItem {
    PayloadItem {
        product_id: product,
        quantity_milli: qty,
        modifier_ids: modifiers.to_vec(),
        course: None,
        note: None,
        combo: None,
    }
}

fn line(product: Uuid, qty: i64, modifiers: &[Uuid], course: Option<i64>) -> ItemInput {
    ItemInput {
        line_id: Uuid::now_v7(),
        product_id: product,
        quantity_milli: qty,
        modifier_ids: modifiers.to_vec(),
        course,
        note: None,
        combo: None,
    }
}

fn keep(item: &OpenOrderItem) -> ItemInput {
    ItemInput {
        line_id: item.line_id,
        product_id: item.product_id,
        quantity_milli: item.quantity_milli,
        modifier_ids: item.modifier_ids.clone(),
        course: item.course,
        note: item.note.clone(),
        combo: item.combo,
    }
}

fn cash(amount: i64) -> Vec<PayloadPayment> {
    vec![PayloadPayment {
        method: PaymentMethod::Cash,
        tendered_currency: CurrencyCode::KWD,
        tendered_amount: amount,
        reference: None,
    }]
}

fn sale_actor(a: &OrderActor) -> SaleActor {
    SaleActor {
        user_id: a.user_id,
        role: a.role,
    }
}

fn set_items(
    w: &World,
    order: &OpenOrder,
    items: Vec<ItemInput>,
    actor: &OrderActor,
    now: Timestamp,
) -> IpcResult<OpenOrder> {
    update(
        &w.db.conn(),
        actor,
        UpdateInput {
            order_id: order.meta.id,
            expected_updated_at: order.meta.updated_at,
            items,
            table_id: order.table_id,
            label: order.label.clone(),
            guests: order.guests,
            notes: None,
        },
        &config(),
        now,
    )
}

// ── Modifiers and combos in pricing ─────────────────────────────────────

#[test]
fn modifiers_are_required_priced_and_snapshotted() {
    let w = world();
    let conn = w.db.conn();
    let err = sales::quote(&conn, &[item(w.latte, 1000, &[])], &[], &config(), at(1))
        .expect_err("size missing");
    assert!(err.message.contains("choose Size"), "{}", err.message);
    let err = sales::quote(
        &conn,
        &[item(w.latte, 1000, &[w.size_small, w.size_large])],
        &[],
        &config(),
        at(1),
    )
    .expect_err("two sizes");
    assert!(err.message.contains("at most 1"), "{}", err.message);
    let err = sales::quote(
        &conn,
        &[item(w.juice, 1000, &[w.oat])],
        &[],
        &config(),
        at(1),
    )
    .expect_err("not asked");
    assert!(err.message.contains("no longer available"));

    let q = sales::quote(
        &conn,
        &[item(w.latte, 2000, &[w.size_large, w.oat])],
        &[],
        &config(),
        at(1),
    )
    .expect("quote");
    assert_eq!(q.lines[0].unit_price, 1_500 + 500 + 200);
    assert_eq!(q.total, 4_400);
    drop(conn);

    let payload = TransactionPayload {
        idempotency_key: Uuid::new_v4(),
        customer_id: None,
        order_type: OrderType::Counter,
        table_label: None,
        items: vec![item(w.latte, 1000, &[w.size_large, w.oat])],
        discount_rule_ids: vec![],
        loyalty_points_to_redeem: 0,
        payments: cash(5_000),
        notes: None,
    };
    let created = sales::create(
        &mut w.db.conn(),
        &sale_actor(&w.cashier),
        &payload,
        &config(),
        at(2),
    )
    .expect("sale");
    let receipt =
        sales::load_receipt(&w.db.conn(), created.transaction_id, false).expect("receipt");
    let names: Vec<_> = receipt.lines[0]
        .modifiers
        .iter()
        .map(|m| (m.name.as_str(), m.price_delta))
        .collect();
    assert_eq!(names, [("Large", 500), ("Oat", 200)]);
    let text = pos_hardware::receipt::render_text(
        &receipt,
        &pos_hardware::receipt::ReceiptTemplate::for_client(&config(), None),
        false,
    );
    assert!(text.contains("+ Large") && text.contains("+ Oat"), "{text}");
}

#[test]
fn combos_price_as_a_set_and_must_be_complete() {
    let w = world();
    let conn = w.db.conn();
    let instance = Uuid::now_v7();
    let combo = Some(ComboRef {
        combo_id: w.combo,
        instance,
    });
    let with_combo = |mut i: PayloadItem| {
        i.combo = combo;
        i
    };
    // Latte (Small) 1.500 + croissant 0.900 = 2.400 → combo 2.000; oat milk +0.200 still charged.
    let q = sales::quote(
        &conn,
        &[
            with_combo(item(w.latte, 1000, &[w.size_small, w.oat])),
            with_combo(item(w.croissant, 1000, &[])),
            item(w.juice, 1000, &[]),
        ],
        &[],
        &config(),
        at(1),
    )
    .expect("quote");
    assert_eq!(
        q.lines[0].line_total + q.lines[1].line_total,
        2_200,
        "combo + surcharge"
    );
    assert_eq!(q.lines[2].line_total, 1_100, "juice outside the combo");
    let err = sales::quote(
        &conn,
        &[with_combo(item(w.latte, 1000, &[w.size_small]))],
        &[],
        &config(),
        at(1),
    )
    .expect_err("incomplete");
    assert!(err.message.contains("exactly its items"));
    let err = sales::quote(
        &conn,
        &[
            with_combo(item(w.latte, 1000, &[w.size_small])),
            with_combo(item(w.juice, 1000, &[])),
        ],
        &[],
        &config(),
        at(1),
    )
    .expect_err("wrong item");
    assert!(err.message.contains("exactly its items"));
}

// ── Open orders ─────────────────────────────────────────────────────────

fn open_table(w: &World) -> OpenOrder {
    open(
        &w.db.conn(),
        &w.cashier,
        OpenInput {
            table_id: Some(w.table),
            label: None,
            guests: 2,
            order_type: OrderType::DineIn,
        },
        at(10),
    )
    .expect("open")
}

#[test]
fn one_open_order_per_table_and_tabs_need_a_name() {
    let w = world();
    let order = open_table(&w);
    assert_eq!(order.order_type, OrderType::DineIn);
    let err = open(
        &w.db.conn(),
        &w.cashier,
        OpenInput {
            table_id: Some(w.table),
            label: None,
            guests: 1,
            order_type: OrderType::DineIn,
        },
        at(11),
    )
    .expect_err("table busy");
    assert_eq!(err.code, IpcErrorCode::Conflict);
    assert!(err.message.contains("T4"));
    let err = open(
        &w.db.conn(),
        &w.cashier,
        OpenInput {
            table_id: None,
            label: Some("  ".into()),
            guests: 0,
            order_type: OrderType::Takeaway,
        },
        at(11),
    )
    .expect_err("unnamed tab");
    assert_eq!(err.code, IpcErrorCode::Validation);
}

#[test]
fn edits_are_versioned_and_validated() {
    let w = world();
    let order = open_table(&w);
    let updated = set_items(
        &w,
        &order,
        vec![line(w.latte, 1000, &[w.size_small], Some(1))],
        &w.cashier,
        at(20),
    )
    .expect("add");
    assert_eq!(updated.items[0].added_by, w.cashier.user_id);
    // A till still holding the old version is refused.
    let err = set_items(&w, &order, vec![], &w.cashier, at(21)).expect_err("stale");
    assert_eq!(err.code, IpcErrorCode::Conflict);
    // Invalid content never reaches the order.
    let err = set_items(
        &w,
        &updated,
        vec![line(w.latte, 1000, &[], None)],
        &w.cashier,
        at(22),
    )
    .expect_err("no size");
    assert!(err.message.contains("choose Size"));
}

#[test]
fn courses_fire_to_the_kitchen_and_fired_items_need_a_manager() {
    let w = world();
    let order = open_table(&w);
    let order = set_items(
        &w,
        &order,
        vec![
            line(w.juice, 2000, &[], Some(1)),
            line(w.latte, 1000, &[w.size_large, w.oat], Some(2)),
        ],
        &w.cashier,
        at(20),
    )
    .expect("items");
    let fired = fire(
        &w.db.conn(),
        &w.cashier,
        order.meta.id,
        Some(1),
        order.meta.updated_at,
        at(30),
    )
    .expect("fire 1");
    assert_eq!(fired.ticket.title, "Table T4");
    assert_eq!(fired.ticket.lines.len(), 1);
    assert_eq!(fired.ticket.lines[0].name, "Juice");
    assert!(fired.order.items[0].fired_at.is_some() && fired.order.items[1].fired_at.is_none());
    let err = fire(
        &w.db.conn(),
        &w.cashier,
        order.meta.id,
        Some(1),
        fired.order.meta.updated_at,
        at(31),
    )
    .expect_err("nothing new");
    assert!(err.message.contains("Nothing new"));
    let second = fire(
        &w.db.conn(),
        &w.cashier,
        order.meta.id,
        None,
        fired.order.meta.updated_at,
        at(32),
    )
    .expect("rest");
    assert_eq!(second.ticket.lines[0].modifiers, ["Large", "Oat"]);
    let order = second.order;

    // Removing a fired item: cashier refused, manager allowed (and audited).
    let remaining = vec![keep(&order.items[1])];
    let err = set_items(&w, &order, remaining.clone(), &w.cashier, at(40)).expect_err("cashier");
    assert_eq!(err.code, IpcErrorCode::Forbidden);
    let err = cancel(
        &w.db.conn(),
        &w.cashier,
        order.meta.id,
        order.meta.updated_at,
        at(40),
    )
    .expect_err("cancel");
    assert_eq!(err.code, IpcErrorCode::Forbidden);
    let voided = set_items(&w, &order, remaining, &w.manager, at(41)).expect("manager");
    assert_eq!(voided.items.len(), 1);
    assert!(voided.items[0].fired_at.is_some(), "kept items stay sent");
    let audits: i64 =
        w.db.conn()
            .query_row(
                "SELECT count(*) FROM audit_log WHERE action = 'sale.void'",
                [],
                |r| r.get(0),
            )
            .expect("audit");
    assert_eq!(audits, 1);
}

#[test]
fn split_bill_pays_lines_separately_then_settles() {
    let w = world();
    let order = open_table(&w);
    let order = set_items(
        &w,
        &order,
        vec![
            line(w.juice, 3000, &[], None),
            line(w.croissant, 1000, &[], None),
        ],
        &w.cashier,
        at(20),
    )
    .expect("items");
    let order = split_line(
        &w.db.conn(),
        order.meta.id,
        order.items[0].line_id,
        order.meta.updated_at,
        at(21),
    )
    .expect("split");
    assert_eq!(order.items.len(), 4, "3 juices + croissant");

    // Guest 1 pays one juice + the croissant.
    let first = PayInput {
        order_id: order.meta.id,
        idempotency_key: Uuid::new_v4(),
        line_ids: Some(vec![order.items[0].line_id, order.items[3].line_id]),
        discount_rule_ids: vec![],
        payments: cash(2_000),
    };
    let (after, sale) = {
        let mut conn = w.db.conn();
        let tx = conn.transaction().expect("tx");
        let paid = pay(&tx, &sale_actor(&w.cashier), &first, &config(), at(30)).expect("pay 1");
        tx.commit().expect("commit");
        paid
    };
    assert_eq!(after.status, OpenOrderStatus::Open);
    assert_eq!(after.items.len(), 2);
    let receipt = sales::load_receipt(&w.db.conn(), sale.transaction_id, false).expect("receipt");
    assert_eq!(receipt.total, 1_100 + 900);

    // A retried payment changes nothing.
    let (again, replay) = pay(
        &w.db.conn(),
        &sale_actor(&w.cashier),
        &first,
        &config(),
        at(31),
    )
    .expect("replay");
    assert!(!replay.is_new);
    assert_eq!(again.items.len(), 2);

    // The rest settles the order; stock moved for the croissant only.
    let rest = PayInput {
        idempotency_key: Uuid::new_v4(),
        line_ids: None,
        payments: cash(3_000),
        ..first
    };
    let (settled, _) = pay(
        &w.db.conn(),
        &sale_actor(&w.cashier),
        &rest,
        &config(),
        at(40),
    )
    .expect("pay rest");
    assert_eq!(settled.status, OpenOrderStatus::Settled);
    assert_eq!(settled.transaction_ids.len(), 2);
    let sales_count: i64 =
        w.db.conn()
            .query_row(
                "SELECT count(*) FROM transactions WHERE table_label = 'T4'",
                [],
                |r| r.get(0),
            )
            .expect("count");
    assert_eq!(sales_count, 2);
    assert_eq!(
        catalog::get(&w.db.conn(), w.croissant)
            .expect("q")
            .expect("p")
            .stock_on_hand_milli,
        -1000
    );
    // The table is free again.
    open_table(&w);
}

#[test]
fn a_combo_is_paid_as_a_whole() {
    let w = world();
    let order = open(
        &w.db.conn(),
        &w.cashier,
        OpenInput {
            table_id: None,
            label: Some("Sara".into()),
            guests: 0,
            order_type: OrderType::Takeaway,
        },
        at(10),
    )
    .expect("tab");
    let combo = Some(ComboRef {
        combo_id: w.combo,
        instance: Uuid::now_v7(),
    });
    let mut a = line(w.latte, 1000, &[w.size_small], None);
    let mut b = line(w.croissant, 1000, &[], None);
    a.combo = combo;
    b.combo = combo;
    let order = set_items(&w, &order, vec![a, b], &w.cashier, at(20)).expect("combo on tab");
    let partial = PayInput {
        order_id: order.meta.id,
        idempotency_key: Uuid::new_v4(),
        line_ids: Some(vec![order.items[0].line_id]),
        discount_rule_ids: vec![],
        payments: cash(2_000),
    };
    let err = pay(
        &w.db.conn(),
        &sale_actor(&w.cashier),
        &partial,
        &config(),
        at(30),
    )
    .expect_err("half a combo");
    assert!(err.message.contains("as a whole"));
    let views = views(&w.db.conn(), &config(), at(31)).expect("views");
    assert_eq!(views[0].total, Some(2_000));
    assert_eq!(views[0].unfired, 2);
}

// ── Stock ───────────────────────────────────────────────────────────────

#[test]
fn stock_adjustments_are_deltas() {
    let w = world();
    let conn = w.db.conn();
    let actor = crate::repo::audit::Actor {
        user_id: w.manager.user_id,
        role: Role::Owner,
        device_id: w.manager.device_id,
    };
    let adjust = |mode, q| {
        inventory::adjust(
            &conn,
            &actor,
            &StockAdjustment {
                product_id: w.croissant,
                mode,
                quantity_milli: q,
                note: None,
            },
            at(5),
        )
    };
    assert_eq!(
        adjust(StockMode::Receive, 24_000)
            .expect("receive")
            .stock_on_hand_milli,
        24_000
    );
    assert_eq!(
        adjust(StockMode::Waste, 2_000)
            .expect("waste")
            .stock_on_hand_milli,
        22_000
    );
    assert_eq!(
        adjust(StockMode::Adjust, -1_000)
            .expect("adjust")
            .stock_on_hand_milli,
        21_000
    );
    assert_eq!(
        adjust(StockMode::Count, 18_000)
            .expect("count")
            .stock_on_hand_milli,
        18_000
    );
    assert!(adjust(StockMode::Count, 18_000).is_err(), "no-op count");
    assert!(adjust(StockMode::Waste, -5).is_err());
    let movements: i64 = conn
        .query_row("SELECT count(*) FROM stock_movements", [], |r| r.get(0))
        .expect("n");
    assert_eq!(movements, 4);
    let err = inventory::adjust(
        &conn,
        &actor,
        &StockAdjustment {
            product_id: w.juice,
            mode: StockMode::Receive,
            quantity_milli: 1,
            note: None,
        },
        at(6),
    )
    .expect_err("untracked");
    assert!(err.message.contains("does not track stock"));
}

#[test]
fn menu_rows_round_trip_and_reach_the_outbox() {
    let w = world();
    let conn = w.db.conn();
    let menu = menu::menu(&conn, false).expect("menu");
    assert_eq!(menu.modifier_groups.len(), 2);
    assert_eq!(menu.product_modifier_groups[&w.latte].len(), 2);
    assert_eq!(menu.combos[0].items.len(), 2);
    assert!(menu.dining_tables[0].is_active);
    for entity in [
        "modifier_groups",
        "modifiers",
        "product_modifier_groups",
        "combos",
        "combo_items",
        "dining_tables",
    ] {
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sync_queue WHERE entity_type = ?1",
                [entity],
                |r| r.get(0),
            )
            .expect("count");
        assert!(n > 0, "{entity} has outbox events");
    }
    // Deleting a group soft-deletes it, its options and its product links.
    let size = menu.product_modifier_groups[&w.latte][0];
    assert!(menu::delete_group(&conn, size, at(50)).expect("delete"));
    let after = menu::menu(&conn, true).expect("menu");
    assert_eq!(after.modifier_groups.len(), 1);
    assert_eq!(after.product_modifier_groups[&w.latte].len(), 1);
    let live: i64 = conn
        .query_row(
            "SELECT count(*) FROM modifiers WHERE group_id = ?1 AND deleted_at IS NULL",
            [size.to_string()],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(live, 0);
}
