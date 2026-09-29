//! Refunds, voids, X/Z reports, the dashboard, history and the audit trail
//! against a real encrypted in-memory database. The shop is in Kuwait
//! (UTC+3): 06:10Z is 09:10 on the till.

use std::sync::Arc;

use pos_core::config::ClientConfig;
use pos_core::currency::CurrencyCode;
use pos_core::rbac::Role;
use pos_core::sales::{OrderType, PaymentMethod, TransactionKind};
use pos_core::time::{Timestamp, Zone};
use pos_core::IpcErrorCode;
use pos_hwid::HardwareComponents;
use rusqlite::params;

use super::*;
use crate::db::Database;
use crate::history::{self, TransactionFilter};
use crate::refunds::{self, RefundInput, RefundLine, RefundMethod, ReverseActor, VoidInput};
use crate::repo::audit::{self, AuditFilter};
use crate::repo::catalog::{self, Category, Product, Unit};
use crate::repo::sales::{self, PayloadItem, PayloadPayment, SaleActor, TransactionPayload};
use crate::repo::{shifts, users};

const CONFIG: &str =
    include_str!("../../../../../packages/shared/contracts/client-config.example.json");
const KUWAIT: Zone = Zone::Fixed(180);

/// `hh:mm` on the till (24 Sep 2026, UTC+3) as a timestamp.
fn local(hh: i64, mm: i64) -> Timestamp {
    "2026-09-24T00:00:00.000Z"
        .parse::<Timestamp>()
        .expect("ts")
        .checked_add(chrono::Duration::minutes((hh - 3) * 60 + mm))
        .expect("ts")
}

struct Shop {
    db: Arc<Database>,
    config: ClientConfig,
    device: Uuid,
    cashier: Uuid,
    manager: users::UserRow,
    tea: Uuid,
    cake: Uuid,
    drinks: Uuid,
}

fn shop() -> Shop {
    let hw = HardwareComponents::new("CPU", "GUID", "BOARD", "VOL").expect("hw");
    let db = Arc::new(Database::open_in_memory(&hw.database_key(Uuid::nil())).expect("db"));
    let device = Uuid::now_v7();
    let t0 = local(8, 0);
    let conn = db.conn();
    conn.execute(
        "INSERT INTO device (id, created_at, updated_at, name) VALUES (?1, ?2, ?2, 'TILL')",
        params![device.to_string(), t0.to_string()],
    )
    .expect("device");
    let hash = users::hash_pin("1234").expect("hash");
    let cashier = users::create(&conn, "Sara", Role::Cashier, hash.clone(), t0).expect("cashier");
    let manager = users::create(&conn, "Omar", Role::Manager, hash, t0).expect("manager");
    let drinks = Category {
        meta: Meta::new(t0),
        name: "Drinks".into(),
        name_localized: serde_json::json!({}),
        parent_id: None,
        sort_order: 0,
        color: None,
    };
    catalog::save_category(&conn, &drinks, t0).expect("category");
    let product = |name: &str, price: i64, tax: i64, category: Option<Uuid>, track: bool| {
        let p = Product {
            meta: Meta::new(t0),
            name: name.into(),
            name_localized: serde_json::json!({}),
            category_id: category,
            sku: None,
            barcode: None,
            price,
            cost: None,
            tax_rate_bps: tax,
            unit: Unit::Each,
            sold_by_weight: false,
            track_stock: track,
            stock_on_hand_milli: 0,
            reorder_threshold_milli: Some(0),
            reorder_quantity_milli: None,
            image_asset: None,
            quick_key_position: None,
            is_active: true,
        };
        catalog::save(&conn, &p, t0).expect("product");
        p.meta.id
    };
    let tea = product("Tea", 1_000, 0, Some(drinks.meta.id), true);
    let cake = product("Cake", 2_500, 500, None, false);
    shifts::open(&conn, device, manager.meta.id, 10_000, t0).expect("shift");
    drop(conn);
    Shop {
        db,
        config: ClientConfig::parse(CONFIG).expect("config"),
        device,
        cashier: cashier.meta.id,
        manager,
        tea,
        cake,
        drinks: drinks.meta.id,
    }
}

impl Shop {
    fn sell(
        &self,
        lines: &[(Uuid, i64)],
        method: PaymentMethod,
        tendered: i64,
        at: Timestamp,
    ) -> Uuid {
        let payload = TransactionPayload {
            idempotency_key: Uuid::now_v7(),
            customer_id: None,
            order_type: OrderType::Counter,
            table_label: None,
            items: lines
                .iter()
                .map(|(product_id, qty)| PayloadItem {
                    product_id: *product_id,
                    quantity_milli: *qty,
                    modifier_ids: vec![],
                    course: None,
                    note: None,
                    combo: None,
                })
                .collect(),
            discount_rule_ids: vec![],
            loyalty_points_to_redeem: 0,
            payments: vec![PayloadPayment {
                method,
                tendered_currency: CurrencyCode::KWD,
                tendered_amount: tendered,
                reference: None,
            }],
            notes: None,
        };
        let actor = SaleActor {
            user_id: self.cashier,
            role: Role::Cashier,
        };
        sales::create(&mut self.db.conn(), &actor, &payload, &self.config, at)
            .expect("sale")
            .transaction_id
    }

    fn actor(&self) -> ReverseActor {
        ReverseActor {
            user_id: self.manager.meta.id,
            role: Role::Manager,
        }
    }

    fn by(&self) -> ReportActor {
        ReportActor {
            user_id: self.manager.meta.id,
            role: Role::Manager,
            display_name: "Omar".into(),
        }
    }

    fn item(&self, sale: Uuid, product: Uuid) -> Uuid {
        refunds::original_lines(&self.db.conn(), sale)
            .expect("lines")
            .into_iter()
            .find(|l| l.product_id == product)
            .expect("line")
            .item_id
    }

    fn refund(
        &self,
        sale: Uuid,
        lines: &[(Uuid, i64)],
        method: RefundMethod,
        at: Timestamp,
    ) -> pos_core::IpcResult<Uuid> {
        let input = RefundInput {
            transaction_id: sale,
            idempotency_key: Uuid::now_v7(),
            lines: lines
                .iter()
                .map(|(item_id, qty)| RefundLine {
                    item_id: *item_id,
                    quantity_milli: *qty,
                })
                .collect(),
            method,
            restock: true,
            reason: "Customer changed their mind".into(),
        };
        refunds::refund(&mut self.db.conn(), &self.actor(), &input, at).map(|c| c.transaction_id)
    }

    fn void(&self, sale: Uuid, at: Timestamp) -> pos_core::IpcResult<Uuid> {
        let input = VoidInput {
            transaction_id: sale,
            idempotency_key: Uuid::now_v7(),
            reason: "Rung up twice".into(),
        };
        refunds::void(&mut self.db.conn(), &self.actor(), &input, at).map(|c| c.transaction_id)
    }
}

/// S1 09:10 3 tea (cash 5.000), S2 10:30 cake (card), S3 10:45 2 tea + cake
/// (cash), void of S3 10:50, refund of one S1 tea 11:00.
fn trading_day() -> (Shop, Uuid, Uuid, Uuid) {
    let shop = shop();
    let s1 = shop.sell(
        &[(shop.tea, 3000)],
        PaymentMethod::Cash,
        5_000,
        local(9, 10),
    );
    let s2 = shop.sell(
        &[(shop.cake, 1000)],
        PaymentMethod::Card,
        2_500,
        local(10, 30),
    );
    let s3 = shop.sell(
        &[(shop.tea, 2000), (shop.cake, 1000)],
        PaymentMethod::Cash,
        4_500,
        local(10, 45),
    );
    shop.void(s3, local(10, 50)).expect("void");
    let tea_line = shop.item(s1, shop.tea);
    shop.refund(s1, &[(tea_line, 1000)], RefundMethod::Cash, local(11, 0))
        .expect("refund");
    (shop, s1, s2, s3)
}

#[test]
fn refunds_take_back_quantities_and_money() {
    let (shop, s1, _, _) = trading_day();
    let conn = shop.db.conn();
    let lines = refunds::original_lines(&conn, s1).expect("lines");
    assert_eq!(lines[0].reversed_milli, 1000);
    assert_eq!(lines[0].refundable_milli(), 2000);
    let refund = history::list(
        &conn,
        &TransactionFilter {
            kind: Some(TransactionKind::Refund),
            limit: 10,
            ..TransactionFilter::default()
        },
    )
    .expect("list");
    assert_eq!(refund.len(), 1);
    assert_eq!(refund[0].total, -1_000);
    assert_eq!(refund[0].original_id, Some(s1));
    assert_eq!(refund[0].approved_by_name.as_deref(), Some("Omar"));
    assert_eq!(refund[0].payment_methods, vec![PaymentMethod::Cash]);
    let receipt = sales::load_receipt(&conn, refund[0].id, false).expect("receipt");
    assert_eq!(receipt.kind, TransactionKind::Refund);
    assert_eq!(receipt.payments[0].amount, -1_000);
    // Restocked: 5 sold, 2 back from the void, 1 from the refund.
    let tea = catalog::get(&conn, shop.tea).expect("get").expect("tea");
    assert_eq!(tea.stock_on_hand_milli, -2_000);
    drop(conn);

    let tea_line = shop.item(s1, shop.tea);
    let err = shop
        .refund(s1, &[(tea_line, 3000)], RefundMethod::Cash, local(11, 5))
        .expect_err("more than is left");
    assert_eq!(err.code, IpcErrorCode::Validation);
    let err = shop
        .refund(s1, &[(tea_line, 500)], RefundMethod::Cash, local(11, 5))
        .expect_err("half a tea");
    assert!(err.message.contains("by the unit"), "{}", err.message);
    let err = shop
        .refund(s1, &[(tea_line, 1000)], RefundMethod::Card, local(11, 5))
        .expect_err("paid in cash");
    assert!(err.message.contains("in cash"), "{}", err.message);
    // The rest goes back; nothing is refundable afterwards.
    shop.refund(s1, &[(tea_line, 2000)], RefundMethod::Cash, local(11, 6))
        .expect("rest");
    let detail = history::detail(&shop.db.conn(), s1).expect("detail");
    assert!(!detail.can_refund);
    assert_eq!(detail.reversals.len(), 2);
    assert_eq!(detail.summary.reversed_total, 3_000);
}

#[test]
fn voids_reverse_every_tender_but_only_in_the_open_shift() {
    let (shop, s1, s2, s3) = trading_day();
    let conn = shop.db.conn();
    let void = history::summary(
        &conn,
        history::detail(&conn, s3).expect("s3").reversals[0].id,
    )
    .expect("void");
    assert_eq!(void.kind, TransactionKind::Void);
    assert_eq!(void.total, -4_500);
    let detail = history::detail(&conn, s3).expect("detail");
    assert!(!detail.can_refund, "a voided sale has nothing left");
    assert!(detail.void_blocker.is_some());
    // S1 was partly refunded: refund the rest instead.
    let header = refunds::header(&conn, s1).expect("s1");
    assert!(refunds::void_blocker(&conn, &header)
        .expect("check")
        .is_some());
    let header = refunds::header(&conn, s2).expect("s2");
    assert!(refunds::void_blocker(&conn, &header)
        .expect("check")
        .is_none());
    let shift = shifts::current_open(&conn, shop.device)
        .expect("q")
        .expect("open");
    shifts::close(
        &conn,
        &shift,
        shop.manager.meta.id,
        12_000,
        10_000,
        None,
        local(18, 0),
    )
    .expect("close");
    drop(conn);
    let err = shop.void(s2, local(18, 5)).expect_err("shift closed");
    assert_eq!(err.code, IpcErrorCode::Conflict);
    let err = shop
        .refund(
            s2,
            &[(shop.item(s2, shop.cake), 1000)],
            RefundMethod::Card,
            local(18, 5),
        )
        .expect_err("no open shift for the money");
    assert_eq!(err.code, IpcErrorCode::Conflict);
}

#[test]
fn x_and_z_reports_close_the_day() {
    let (shop, _, _, _) = trading_day();
    let currency = CurrencyCode::KWD;
    let x = x_report(&shop.db.conn(), &shop.by(), currency, local(12, 0)).expect("x");
    let t = &x.totals;
    assert_eq!((t.sale_count, t.gross_sales), (3, 10_000));
    assert_eq!((t.refund_count, t.refund_total), (1, 1_000));
    assert_eq!((t.void_count, t.void_total), (1, 4_500));
    assert_eq!(t.net_sales, 4_500);
    // Cake's 5 % inclusive tax: sold twice, one voided.
    assert_eq!(t.tax_total, 119);
    let cash = t
        .by_payment_method
        .iter()
        .find(|m| m.method == PaymentMethod::Cash)
        .expect("cash");
    assert_eq!(cash.amount, 3_000 + 4_500 - 4_500 - 1_000);
    assert_eq!(x.cash.expected, 10_000 + 7_500 - 5_500);
    assert_eq!(x.cash.counted, None, "the shift is still open");
    assert_eq!(x.grand_total, 4_500);
    assert_eq!(
        t.first_receipt.as_deref().map(|r| r.ends_with("000001")),
        Some(true)
    );

    let err =
        run_z(&mut shop.db.conn(), &shop.by(), currency, local(18, 0)).expect_err("open shift");
    assert_eq!(err.code, IpcErrorCode::Conflict);
    {
        let conn = shop.db.conn();
        let shift = shifts::current_open(&conn, shop.device)
            .expect("q")
            .expect("open");
        shifts::close(
            &conn,
            &shift,
            shop.manager.meta.id,
            12_100,
            10_000,
            None,
            local(18, 0),
        )
        .expect("close");
    }
    let z = run_z(&mut shop.db.conn(), &shop.by(), currency, local(18, 5)).expect("z");
    assert_eq!(z.z_number, Some(1));
    assert_eq!(z.totals, x.totals);
    assert_eq!(z.cash.counted, Some(12_100));
    assert_eq!(z.cash.variance, Some(100));
    assert_eq!(z.shifts.len(), 1);
    let stored = z_get(&shop.db.conn(), z.z_report_id.expect("id")).expect("stored");
    assert_eq!(stored, z, "a reprint is identical");

    // The next Z starts after this one and is empty.
    let z2 = run_z(&mut shop.db.conn(), &shop.by(), currency, local(18, 30)).expect("z2");
    assert_eq!(z2.z_number, Some(2));
    assert_eq!(z2.totals.sale_count, 0);
    assert_eq!(z2.grand_total, 4_500);
    assert!(z2.period_start > z.period_end);
    let list = z_list(&shop.db.conn(), None, 10, 0).expect("list");
    assert_eq!(
        list.iter().map(|z| z.z_number).collect::<Vec<_>>(),
        vec![2, 1]
    );
    // Append-only, and each run is audited.
    let conn = shop.db.conn();
    assert!(conn
        .execute("UPDATE z_reports SET net_sales = 0", [])
        .is_err());
    let audited = audit::list(
        &conn,
        &AuditFilter {
            action: Some("report.z_run".into()),
            limit: 10,
            ..AuditFilter::default()
        },
    )
    .expect("audit");
    assert_eq!(audited.total, 2);

    let text = pos_hardware::report::render_text(
        &to_doc(&z, &shop.config, KUWAIT, pos_core::config::Locale::En),
        80,
    );
    for expected in [
        "Z REPORT #1",
        "From 2026-09-24 08:00",
        "Net sales",
        "4.500",
        "Variance",
        "0.100",
    ] {
        assert!(text.contains(expected), "{expected} missing:\n{text}");
    }
}

#[test]
fn dashboard_buckets_by_local_hour_and_day() {
    let (shop, _, _, _) = trading_day();
    let window = Window {
        from: local(0, 0),
        to: local(24, 0),
        device_id: None,
    };
    let d = dashboard(&shop.db.conn(), window, KUWAIT, CurrencyCode::KWD).expect("dashboard");
    assert_eq!(d.totals.net_sales, 4_500);
    assert_eq!(d.by_hour[9].amount, 3_000);
    assert_eq!(d.by_hour[10].amount, 2_500 + 4_500 - 4_500);
    assert_eq!(d.by_hour[10].count, 2);
    assert_eq!(d.by_hour[11].amount, -1_000);
    assert_eq!(d.by_day.len(), 1);
    assert_eq!(d.by_day[0].date, "2026-09-24");
    assert_eq!(d.by_day[0].amount, 4_500);
    // Cake 2.500 net (one of two voided), tea 2 × 1.000 net.
    assert_eq!(d.top_products[0].name, "Cake");
    assert_eq!(d.top_products[0].amount, 2_500);
    assert_eq!(d.top_products[1].quantity_milli, 2_000);
    let drinks = d
        .by_category
        .iter()
        .find(|c| c.category_id == Some(shop.drinks))
        .expect("drinks");
    assert_eq!(drinks.name.as_deref(), Some("Drinks"));
    assert_eq!(drinks.amount, 2_000);
    assert_eq!(d.by_cashier[0].name, "Sara");
    assert_eq!((d.by_cashier[0].amount, d.by_cashier[0].count), (10_000, 3));
    assert_eq!(d.average_ticket, 3_333);
    assert_eq!(d.previous.sale_count, 0);
    // Tea is below its alert level (0) after selling stock it did not have.
    assert_eq!(d.low_stock_count, 1);

    // The same day seen from UTC splits nothing but shifts the hours.
    let utc = dashboard(&shop.db.conn(), window, Zone::Utc, CurrencyCode::KWD).expect("utc");
    assert_eq!(utc.by_hour[6].amount, 3_000);
    let err = dashboard(
        &shop.db.conn(),
        Window {
            from: local(0, 0),
            to: local(0, 0),
            device_id: None,
        },
        KUWAIT,
        CurrencyCode::KWD,
    )
    .expect_err("empty range");
    assert_eq!(err.code, IpcErrorCode::Validation);
}

#[test]
fn history_searches_and_the_audit_trail_filters() {
    let (shop, s1, _, _) = trading_day();
    let conn = shop.db.conn();
    let all = history::list(
        &conn,
        &TransactionFilter {
            limit: 50,
            ..TransactionFilter::default()
        },
    )
    .expect("all");
    assert_eq!(all.len(), 5);
    assert!(
        all.windows(2).all(|w| w[0].occurred_at >= w[1].occurred_at),
        "newest first"
    );
    let cakes = history::list(
        &conn,
        &TransactionFilter {
            search: Some("cak".into()),
            limit: 50,
            ..TransactionFilter::default()
        },
    )
    .expect("search");
    assert_eq!(cakes.len(), 3, "S2, S3 and the void of S3");
    let wildcard = history::list(
        &conn,
        &TransactionFilter {
            search: Some("%".into()),
            limit: 50,
            ..TransactionFilter::default()
        },
    )
    .expect("literal %");
    assert!(wildcard.is_empty());
    let detail = history::detail(&conn, s1).expect("detail");
    assert_eq!(detail.lines[0].refundable_quantity_milli, 2_000);
    assert_eq!(detail.receipt.total, 3_000);

    let sales_actions = audit::list(
        &conn,
        &AuditFilter {
            action: Some("sale.".into()),
            limit: 50,
            ..AuditFilter::default()
        },
    )
    .expect("audit");
    assert_eq!(sales_actions.total, 5, "3 sales, a void, a refund");
    let refund = sales_actions
        .entries
        .iter()
        .find(|e| e.entry.action == "sale.refund")
        .expect("refund entry");
    assert_eq!(refund.user_name.as_deref(), Some("Omar"));
    assert_eq!(
        refund
            .entry
            .after
            .as_ref()
            .and_then(|a| a["reason"].as_str()),
        Some("Customer changed their mind")
    );
    let paged = audit::list(
        &conn,
        &AuditFilter {
            action: Some("sale.".into()),
            limit: 2,
            offset: 4,
            ..AuditFilter::default()
        },
    )
    .expect("page");
    assert_eq!((paged.entries.len(), paged.total), (1, 5));
    assert!(sales_actions.actions.contains(&"sale.void".to_owned()));
}
