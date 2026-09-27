//! Kitchen tickets from every path that sends food to the kitchen, and the
//! board the display shows. A restaurant build with the kitchen display.

use std::sync::Arc;

use pos_core::config::{BusinessType, ClientConfig};
use pos_core::currency::CurrencyCode;
use pos_core::rbac::Role;
use pos_core::sales::PaymentMethod;
use pos_hwid::HardwareComponents;
use rusqlite::params;

use super::*;
use crate::db::Database;
use crate::open_orders::{self, ItemInput, OpenInput, OrderActor, PayInput, UpdateInput};
use crate::repo::catalog::{self, Product, Unit};
use crate::repo::orders::OpenOrder;
use crate::repo::sales::{self, PayloadItem, PayloadPayment, SaleActor, TransactionPayload};
use crate::repo::{shifts, users};

const CONFIG: &str =
    include_str!("../../../../../packages/shared/contracts/client-config.example.json");

fn at(minute: i64) -> Timestamp {
    "2026-09-27T18:00:00.000Z"
        .parse::<Timestamp>()
        .expect("ts")
        .checked_add(chrono::Duration::minutes(minute))
        .expect("ts")
}

fn config(kds: bool) -> ClientConfig {
    let mut config = ClientConfig::parse(CONFIG).expect("config");
    config.business_type = BusinessType::Restaurant;
    config.features.kitchen_display = kds;
    config
}

struct Kitchen {
    db: Arc<Database>,
    waiter: OrderActor,
    manager: OrderActor,
    burger: Uuid,
    soup: Uuid,
}

fn kitchen() -> Kitchen {
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
    let waiter = users::create(&conn, "Sara", Role::Cashier, hash.clone(), at(0)).expect("u");
    let manager = users::create(&conn, "Omar", Role::Manager, hash, at(0)).expect("u");
    shifts::open(&conn, device, manager.meta.id, 0, at(0)).expect("shift");
    let product = |name: &str, price: i64| {
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
            track_stock: false,
            stock_on_hand_milli: 0,
            reorder_threshold_milli: None,
            reorder_quantity_milli: None,
            image_asset: None,
            quick_key_position: None,
            is_active: true,
        };
        catalog::save(&conn, &p, at(0)).expect("product");
        p.meta.id
    };
    let burger = product("Burger", 3_000);
    let soup = product("Soup", 1_500);
    drop(conn);
    let actor = |user: &users::UserRow| OrderActor {
        user_id: user.meta.id,
        display_name: user.display_name.clone(),
        role: user.role,
        device_id: device,
    };
    Kitchen {
        db,
        waiter: actor(&waiter),
        manager: actor(&manager),
        burger,
        soup,
    }
}

fn item(
    product_id: Uuid,
    quantity_milli: i64,
    course: Option<i64>,
    note: Option<&str>,
) -> ItemInput {
    ItemInput {
        line_id: Uuid::now_v7(),
        product_id,
        quantity_milli,
        modifier_ids: vec![],
        course,
        note: note.map(str::to_owned),
        combo: None,
    }
}

fn cash() -> Vec<PayloadPayment> {
    vec![PayloadPayment {
        method: PaymentMethod::Cash,
        tendered_currency: CurrencyCode::KWD,
        tendered_amount: 50_000,
        reference: None,
    }]
}

impl Kitchen {
    fn tab(&self, items: Vec<ItemInput>, config: &ClientConfig) -> OpenOrder {
        let conn = self.db.conn();
        let order = open_orders::open(
            &conn,
            &self.waiter,
            OpenInput {
                order_type: OrderType::DineIn,
                table_id: None,
                label: Some("Window".into()),
                guests: 2,
            },
            at(1),
        )
        .expect("open");
        open_orders::update(
            &conn,
            &self.waiter,
            UpdateInput {
                order_id: order.meta.id,
                expected_updated_at: order.meta.updated_at,
                table_id: None,
                label: Some("Window".into()),
                guests: 2,
                items,
                notes: None,
            },
            config,
            at(2),
        )
        .expect("items")
    }

    fn board(&self, minute: i64) -> Board {
        board(&self.db.conn(), 30, at(minute)).expect("board")
    }
}

#[test]
fn courses_voids_and_bumps_reach_the_board() {
    let k = kitchen();
    let config = config(true);
    let order = k.tab(
        vec![
            item(k.soup, 2000, Some(1), None),
            item(k.burger, 1000, Some(2), Some("no onion")),
        ],
        &config,
    );
    let fired = open_orders::fire(
        &k.db.conn(),
        &k.waiter,
        order.meta.id,
        Some(1),
        order.meta.updated_at,
        &config,
        at(3),
    )
    .expect("fire");
    let ticket = fired.kitchen.expect("ticket");
    assert_eq!((ticket.ticket_number, ticket.kind), (1, TicketKind::Order));
    assert_eq!(ticket.title, "Tab Window");
    assert_eq!((ticket.course, ticket.guests), (Some(1), 2));
    assert_eq!(ticket.items.len(), 1);
    assert_eq!(ticket.items[0].name, "Soup");
    assert_eq!(ticket.items[0].line_id, fired.order.items[0].line_id);

    let second = open_orders::fire(
        &k.db.conn(),
        &k.waiter,
        order.meta.id,
        None,
        fired.order.meta.updated_at,
        &config,
        at(4),
    )
    .expect("fire rest")
    .kitchen
    .expect("ticket");
    assert_eq!(second.ticket_number, 2);
    assert_eq!(second.items[0].note.as_deref(), Some("no onion"));

    let open = k.board(5).open;
    assert_eq!(
        open.iter().map(|t| t.ticket_number).collect::<Vec<_>>(),
        [1, 2]
    );

    // The cook strikes the soup, then bumps the ticket.
    let struck = set_done(
        &k.db.conn(),
        ticket.meta.id,
        ticket.items[0].line_id,
        true,
        at(6),
    )
    .expect("done");
    assert!(struck.items[0].done_at.is_some());
    assert!(set_done(&k.db.conn(), ticket.meta.id, Uuid::now_v7(), true, at(6)).is_err());
    let bumped = bump(&k.db.conn(), ticket.meta.id, true, at(7)).expect("bump");
    assert_eq!(bumped.status, TicketStatus::Ready);
    let board = k.board(8);
    assert_eq!(board.open.len(), 1);
    assert_eq!(board.ready[0].meta.id, ticket.meta.id);
    // Recall puts it back; after 30 minutes a ready ticket leaves the recall list.
    bump(&k.db.conn(), ticket.meta.id, false, at(9)).expect("recall");
    assert_eq!(k.board(9).open.len(), 2);
    bump(&k.db.conn(), ticket.meta.id, true, at(10)).expect("bump");
    assert!(k.board(45).ready.is_empty());

    // A manager takes the (sent) burger off: the kitchen gets a void ticket.
    let current = crate::repo::orders::get(&k.db.conn(), order.meta.id)
        .expect("get")
        .expect("order");
    open_orders::update(
        &k.db.conn(),
        &k.manager,
        UpdateInput {
            order_id: current.meta.id,
            expected_updated_at: current.meta.updated_at,
            table_id: None,
            label: Some("Window".into()),
            guests: 2,
            items: vec![ItemInput {
                line_id: current.items[0].line_id,
                product_id: k.soup,
                quantity_milli: 2000,
                modifier_ids: vec![],
                course: Some(1),
                note: None,
                combo: None,
            }],
            notes: None,
        },
        &config,
        at(11),
    )
    .expect("void");
    let void = k
        .board(12)
        .open
        .into_iter()
        .find(|t| t.kind == TicketKind::Void);
    let void = void.expect("void ticket");
    assert_eq!(void.items[0].name, "Burger");
    assert_eq!(void.server_name, "Omar");
}

#[test]
fn pay_now_and_unsent_lines_go_to_the_kitchen() {
    let k = kitchen();
    let config = config(true);
    let payload = TransactionPayload {
        idempotency_key: Uuid::now_v7(),
        customer_id: None,
        order_type: OrderType::Takeaway,
        table_label: None,
        items: vec![PayloadItem {
            product_id: k.burger,
            quantity_milli: 2000,
            modifier_ids: vec![],
            course: None,
            note: Some("extra sauce".into()),
            combo: None,
        }],
        discount_rule_ids: vec![],
        loyalty_points_to_redeem: 0,
        payments: cash(),
        notes: None,
    };
    let actor = SaleActor {
        user_id: k.waiter.user_id,
        role: Role::Cashier,
    };
    let sale = sales::create(&mut k.db.conn(), &actor, &payload, &config, at(1)).expect("sale");
    // A retry of the same sale adds nothing.
    sales::create(&mut k.db.conn(), &actor, &payload, &config, at(2)).expect("retry");
    let open = k.board(3).open;
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].transaction_id, Some(sale.transaction_id));
    assert!(open[0].title.starts_with("Takeaway "), "{}", open[0].title);
    assert_eq!(open[0].items[0].note.as_deref(), Some("extra sauce"));
    assert_eq!(open[0].server_name, "Sara");

    // Voiding the sale calls the food off in the kitchen.
    crate::refunds::void(
        &mut k.db.conn(),
        &crate::refunds::ReverseActor {
            user_id: k.manager.user_id,
            role: Role::Manager,
        },
        &crate::refunds::VoidInput {
            transaction_id: sale.transaction_id,
            idempotency_key: Uuid::now_v7(),
            reason: "Customer left".into(),
        },
        at(3),
    )
    .expect("void");
    let open = k.board(3).open;
    let void = open
        .iter()
        .find(|t| t.kind == TicketKind::Void)
        .expect("void ticket");
    assert_eq!(void.transaction_id, Some(sale.transaction_id));
    assert_eq!(void.items[0].name, "Burger");
    assert_eq!(void.server_name, "Omar");
    bump(&k.db.conn(), void.meta.id, true, at(3)).expect("bump");
    let first = open
        .iter()
        .find(|t| t.kind == TicketKind::Order)
        .expect("order");
    bump(&k.db.conn(), first.meta.id, true, at(3)).expect("bump");

    // A tab paid without sending: its lines go to the kitchen with the payment.
    let order = k.tab(vec![item(k.soup, 1000, None, None)], &config);
    let mut conn = k.db.conn();
    let tx = conn.transaction().expect("tx");
    let (_, paid) = open_orders::pay(
        &tx,
        &actor,
        &PayInput {
            order_id: order.meta.id,
            idempotency_key: Uuid::now_v7(),
            line_ids: None,
            discount_rule_ids: vec![],
            customer_id: None,
            loyalty_points_to_redeem: 0,
            payments: cash(),
        },
        &config,
        at(5),
    )
    .expect("pay");
    tx.commit().expect("commit");
    drop(conn);
    let open = k.board(6).open;
    assert_eq!(open.len(), 1, "the takeaway and its void were bumped");
    assert_eq!(open[0].transaction_id, Some(paid.transaction_id));
    assert_eq!(open[0].title, "Tab Window");
}

#[test]
fn builds_without_the_display_write_no_tickets() {
    let k = kitchen();
    let config = config(false);
    let order = k.tab(vec![item(k.soup, 1000, Some(1), None)], &config);
    let fired = open_orders::fire(
        &k.db.conn(),
        &k.waiter,
        order.meta.id,
        None,
        order.meta.updated_at,
        &config,
        at(3),
    )
    .expect("fire");
    // The printed ticket still exists; the display's copy does not.
    assert!(fired.kitchen.is_none());
    assert_eq!(fired.ticket.lines.len(), 1);
    assert!(k.board(4).open.is_empty());
    let mut retail = config.clone();
    retail.features.kitchen_display = true;
    retail.business_type = BusinessType::Retail;
    assert!(!enabled(&retail));
}
