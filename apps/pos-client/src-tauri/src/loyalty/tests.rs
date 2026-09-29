//! Customers and points against a real encrypted in-memory database: earn,
//! redeem as a discount, reverse on refunds and voids, owner adjustments.
//! KWD (3 decimals): by default 1 point per 1.000 paid, 1 point = 0.010 off.

use std::sync::Arc;

use pos_core::config::ClientConfig;
use pos_core::currency::CurrencyCode;
use pos_core::rbac::Role;
use pos_core::sales::{OrderType, PaymentMethod};
use pos_core::IpcErrorCode;
use pos_hwid::HardwareComponents;
use rusqlite::params;

use super::*;
use crate::db::Database;
use crate::refunds::{self, RefundInput, RefundLine, RefundMethod, ReverseActor, VoidInput};
use crate::repo::catalog::{self, Product, Unit};
use crate::repo::sales::{PayloadPayment, SaleActor, TransactionPayload};
use crate::repo::{shifts, users};

const CONFIG: &str =
    include_str!("../../../../../packages/shared/contracts/client-config.example.json");

fn at(minute: i64) -> Timestamp {
    "2026-09-27T09:00:00.000Z"
        .parse::<Timestamp>()
        .expect("ts")
        .checked_add(chrono::Duration::minutes(minute))
        .expect("ts")
}

struct Shop {
    db: Arc<Database>,
    config: ClientConfig,
    owner: Actor,
    cashier: Uuid,
    manager: Uuid,
    tea: Uuid,
    cake: Uuid,
}

fn shop() -> Shop {
    let hw = HardwareComponents::new("CPU", "GUID", "BOARD", "VOL").expect("hw");
    let db = Arc::new(Database::open_in_memory(&hw.database_key(Uuid::nil())).expect("db"));
    let device = Uuid::now_v7();
    let t0 = at(0);
    let conn = db.conn();
    conn.execute(
        "INSERT INTO device (id, created_at, updated_at, name) VALUES (?1, ?2, ?2, 'TILL')",
        params![device.to_string(), t0.to_string()],
    )
    .expect("device");
    let hash = users::hash_pin("1234").expect("hash");
    let owner = users::create(&conn, "Nadia", Role::Owner, hash.clone(), t0).expect("owner");
    let manager = users::create(&conn, "Omar", Role::Manager, hash.clone(), t0).expect("manager");
    let cashier = users::create(&conn, "Sara", Role::Cashier, hash, t0).expect("cashier");
    let product = |name: &str, price: i64, tax: i64| {
        let p = Product {
            meta: Meta::new(t0),
            name: name.into(),
            name_localized: serde_json::json!({}),
            category_id: None,
            sku: None,
            barcode: None,
            price,
            cost: None,
            tax_rate_bps: tax,
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
        catalog::save(&conn, &p, t0).expect("product");
        p.meta.id
    };
    let tea = product("Tea", 1_000, 0);
    let cake = product("Cake", 2_500, 500);
    shifts::open(&conn, device, manager.meta.id, 10_000, t0).expect("shift");
    drop(conn);
    Shop {
        db,
        config: ClientConfig::parse(CONFIG).expect("config"),
        owner: Actor {
            user_id: owner.meta.id,
            role: Role::Owner,
            device_id: device,
        },
        cashier: cashier.meta.id,
        manager: manager.meta.id,
        tea,
        cake,
    }
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

impl Shop {
    fn register(&self, name: &str, phone: Option<&str>) -> IpcResult<Customer> {
        save(
            &self.db.conn(),
            &self.owner,
            CustomerInput {
                id: None,
                display_name: name.into(),
                phone: phone.map(str::to_owned),
                email: None,
                notes: None,
            },
            at(1),
        )
    }

    fn sell(
        &self,
        items: Vec<PayloadItem>,
        customer: Option<Uuid>,
        redeem: i64,
        minute: i64,
    ) -> IpcResult<Uuid> {
        let payload = TransactionPayload {
            idempotency_key: Uuid::now_v7(),
            customer_id: customer,
            order_type: OrderType::Counter,
            table_label: None,
            items,
            discount_rule_ids: vec![],
            loyalty_points_to_redeem: redeem,
            payments: vec![PayloadPayment {
                method: PaymentMethod::Cash,
                tendered_currency: CurrencyCode::KWD,
                tendered_amount: 20_000,
                reference: None,
            }],
            notes: None,
        };
        let actor = SaleActor {
            user_id: self.cashier,
            role: Role::Cashier,
        };
        sales::create(
            &mut self.db.conn(),
            &actor,
            &payload,
            &self.config,
            at(minute),
        )
        .map(|c| c.transaction_id)
    }

    fn quote(&self, items: &[PayloadItem], customer: Uuid, redeem: i64) -> IpcResult<CustomerCart> {
        price(
            &self.db.conn(),
            items,
            &[],
            Some(LoyaltyRequest {
                customer_id: customer,
                redeem_points: redeem,
            }),
            &self.config,
            at(2),
        )
    }

    fn balance(&self, customer: Uuid) -> i64 {
        live_customer(&self.db.conn(), customer)
            .expect("customer")
            .loyalty_points
    }

    fn reverser(&self) -> ReverseActor {
        ReverseActor {
            user_id: self.manager,
            role: Role::Manager,
        }
    }
}

#[test]
fn registering_normalizes_phones_and_refuses_duplicates() {
    let shop = shop();
    let layla = shop
        .register("  Layla  ", Some("+965 5555-1234"))
        .expect("layla");
    assert_eq!(layla.display_name, "Layla");
    assert_eq!(layla.phone.as_deref(), Some("+96555551234"));
    let err = shop
        .register("Someone else", Some("+965 (5555) 1234"))
        .expect_err("same phone");
    assert_eq!(err.code, IpcErrorCode::Conflict);
    assert!(err.message.contains("Layla"), "{}", err.message);
    assert!(shop.register("Bad", Some("55-ab")).is_err());
    assert!(shop.register("", None).is_err());

    let conn = shop.db.conn();
    let by_phone = customers::search(&conn, "5555 1234", 10).expect("search");
    assert_eq!(by_phone.len(), 1);
    let by_name = customers::search(&conn, "lay", 10).expect("search");
    assert_eq!(by_name[0].meta.id, layla.meta.id);
    assert!(customers::search(&conn, "12", 10)
        .expect("search")
        .is_empty());
    drop(conn);

    delete(&shop.db.conn(), &shop.owner, layla.meta.id, at(3)).expect("delete");
    assert!(customers::search(&shop.db.conn(), "lay", 10)
        .expect("search")
        .is_empty());
    // The number is free again once the customer is gone.
    shop.register("Layla", Some("+96555551234")).expect("again");
}

#[test]
fn sales_earn_and_redeem_points_as_a_discount() {
    let shop = shop();
    let layla = shop
        .register("Layla", Some("55551234"))
        .expect("layla")
        .meta
        .id;

    // 3 teas = 3.000 → 3 points.
    let first = shop
        .sell(vec![item(shop.tea, 3000)], Some(layla), 0, 5)
        .expect("sale");
    assert_eq!(shop.balance(layla), 3);
    let receipt = sales::load_receipt(&shop.db.conn(), first, true).expect("receipt");
    assert_eq!(receipt.customer_name.as_deref(), Some("Layla"));
    let points = receipt.loyalty.expect("loyalty");
    assert_eq!((points.earned, points.redeemed, points.balance), (3, 0, 3));

    // Below the minimum (100) nothing can be redeemed yet.
    let cart = [item(shop.cake, 1000), item(shop.tea, 1000)];
    let quote = shop.quote(&cart, layla, 0).expect("quote");
    let lq = quote.loyalty.expect("loyalty");
    assert_eq!(
        (lq.balance, lq.max_redeem_points, lq.points_earned),
        (3, 0, 3)
    );
    assert!(shop.quote(&cart, layla, 50).is_err());

    adjust(
        &shop.db.conn(),
        &shop.owner,
        &PointsAdjustment {
            customer_id: layla,
            points_delta: 500,
            note: "Welcome bonus".into(),
        },
        at(6),
    )
    .expect("adjust");
    assert_eq!(shop.balance(layla), 503);

    // 3.500 bill: at most 350 points; 200 points take 2.000 off.
    let lq = shop
        .quote(&cart, layla, 0)
        .expect("quote")
        .loyalty
        .expect("l");
    assert_eq!(lq.max_redeem_points, 350);
    let err = shop.quote(&cart, layla, 400).expect_err("too many");
    assert!(err.message.contains("350"), "{}", err.message);
    assert!(shop.quote(&cart, layla, 60).is_err(), "below the minimum");
    let quoted = shop.quote(&cart, layla, 200).expect("quote");
    assert_eq!(quoted.cart.quote.total, 1_500);
    assert_eq!(quoted.cart.quote.discount_total, 2_000);
    let lq = quoted.loyalty.expect("loyalty");
    assert_eq!((lq.redeem_value, lq.points_earned), (2_000, 1));
    // Spread across the lines like any discount: the tax follows it.
    let cake_line = &quoted.cart.quote.lines[0];
    assert_eq!(
        cake_line.discount_amount + quoted.cart.quote.lines[1].discount_amount,
        2_000
    );

    let second = shop.sell(cart.to_vec(), Some(layla), 200, 7).expect("sale");
    assert_eq!(shop.balance(layla), 503 - 200 + 1);
    let receipt = sales::load_receipt(&shop.db.conn(), second, true).expect("receipt");
    assert_eq!(receipt.total, 1_500);
    let points = receipt.loyalty.expect("loyalty");
    assert_eq!(
        (points.earned, points.redeemed, points.balance),
        (1, 200, 304)
    );

    // Points without a customer, or a customer who is gone, are refused.
    assert!(shop.sell(cart.to_vec(), None, 100, 8).is_err());
    let customer = live_customer(&shop.db.conn(), layla).expect("customer");
    let detail = customers::detail(&shop.db.conn(), customer).expect("detail");
    assert_eq!(detail.visits, 2);
    assert_eq!(detail.spent, 3_000 + 1_500);
    let reasons: Vec<LedgerReason> = detail.ledger.iter().map(|l| l.reason).collect();
    assert_eq!(
        reasons,
        [
            LedgerReason::Earn,
            LedgerReason::Redeem,
            LedgerReason::Adjust,
            LedgerReason::Earn
        ]
    );
    assert_eq!(
        detail.ledger[0].receipt_number,
        Some(receipt.receipt_number.clone())
    );
    assert_eq!(detail.ledger[2].receipt_number, None, "the adjustment");
}

#[test]
fn refunds_and_voids_give_the_points_back() {
    let shop = shop();
    let layla = shop.register("Layla", None).expect("layla").meta.id;
    adjust(
        &shop.db.conn(),
        &shop.owner,
        &PointsAdjustment {
            customer_id: layla,
            points_delta: 1_000,
            note: "Opening balance".into(),
        },
        at(2),
    )
    .expect("adjust");

    // 4 teas (4.000), 100 points off (1.000): pays 3.000, earns 3.
    let sale = shop
        .sell(vec![item(shop.tea, 4000)], Some(layla), 100, 5)
        .expect("sale");
    assert_eq!(shop.balance(layla), 1_000 - 100 + 3);
    let line = refunds::original_lines(&shop.db.conn(), sale).expect("lines")[0].item_id;
    let refund = |qty: i64, minute: i64| {
        refunds::refund(
            &mut shop.db.conn(),
            &shop.reverser(),
            &RefundInput {
                transaction_id: sale,
                idempotency_key: Uuid::now_v7(),
                lines: vec![RefundLine {
                    item_id: line,
                    quantity_milli: qty,
                }],
                method: RefundMethod::Cash,
                restock: false,
                reason: "Cold".into(),
            },
            at(minute),
        )
        .expect("refund")
        .transaction_id
    };
    // One tea back (0.750 of 3.000): a quarter of the points.
    let first = refund(1000, 10);
    let receipt = sales::load_receipt(&shop.db.conn(), first, true).expect("receipt");
    assert_eq!(receipt.customer_name.as_deref(), Some("Layla"));
    let points = receipt.loyalty.expect("loyalty");
    // 25 of 100 redeemed come back; 3 × ¼ = 0.75 → 1 earned point goes.
    assert_eq!((points.redeemed, points.earned), (25, 1));
    assert_eq!(shop.balance(layla), 903 + 25 - 1);
    // The other three teas: the rest, exactly.
    refund(3000, 11);
    assert_eq!(shop.balance(layla), 1_000, "everything reversed");

    // A void (same shift) reverses the lot.
    let sale = shop
        .sell(vec![item(shop.tea, 2000)], Some(layla), 100, 20)
        .expect("sale");
    assert_eq!(shop.balance(layla), 1_000 - 100 + 1);
    refunds::void(
        &mut shop.db.conn(),
        &shop.reverser(),
        &VoidInput {
            transaction_id: sale,
            idempotency_key: Uuid::now_v7(),
            reason: "Wrong customer".into(),
        },
        at(21),
    )
    .expect("void");
    assert_eq!(shop.balance(layla), 1_000);
    let ledger: i64 = shop
        .db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM loyalty_ledger WHERE reason = 'refund_reversal'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(ledger, 6, "returned + taken back, per reversal");
}

#[test]
fn the_programme_can_be_switched_off() {
    let shop = shop();
    let layla = shop.register("Layla", None).expect("layla").meta.id;
    let off = LoyaltySettings {
        enabled: false,
        ..shop::loyalty(&shop.db.conn(), &shop.config).expect("settings")
    };
    shop::save_loyalty(&shop.db.conn(), &off, at(2)).expect("save");
    let lq = shop
        .quote(&[item(shop.tea, 5000)], layla, 0)
        .expect("quote")
        .loyalty
        .expect("loyalty");
    assert!(!lq.enabled);
    assert_eq!(lq.points_earned, 0);
    let sale = shop
        .sell(vec![item(shop.tea, 5000)], Some(layla), 0, 5)
        .expect("sale");
    assert_eq!(shop.balance(layla), 0);
    // The customer is still named on the receipt.
    let receipt = sales::load_receipt(&shop.db.conn(), sale, true).expect("receipt");
    assert_eq!(receipt.customer_name.as_deref(), Some("Layla"));
    assert!(receipt.loyalty.is_none());

    // A build without the loyalty feature never earns, whatever is stored.
    let mut no_feature = shop.config.clone();
    no_feature.features.loyalty = false;
    let on = LoyaltySettings {
        enabled: true,
        ..off
    };
    shop::save_loyalty(&shop.db.conn(), &on, at(6)).expect("save");
    assert!(
        !shop::effective_loyalty(&shop.db.conn(), &no_feature)
            .expect("effective")
            .enabled
    );
    // The row keeps its fixed id, so every till writes the same one.
    let id: String = shop
        .db
        .conn()
        .query_row(
            "SELECT id FROM shop_settings WHERE key = 'loyalty'",
            [],
            |r| r.get(0),
        )
        .expect("row");
    assert_eq!(id, shop::LOYALTY_ID.to_string());
}

#[test]
fn points_can_pay_a_whole_bill() {
    let shop = shop();
    let layla = shop.register("Layla", None).expect("layla").meta.id;
    adjust(
        &shop.db.conn(),
        &shop.owner,
        &PointsAdjustment {
            customer_id: layla,
            points_delta: 300,
            note: "Birthday".into(),
        },
        at(2),
    )
    .expect("adjust");
    // 2 teas = 2.000 = 200 points; nothing is tendered.
    let mut payload = TransactionPayload {
        idempotency_key: Uuid::now_v7(),
        customer_id: Some(layla),
        order_type: OrderType::Counter,
        table_label: None,
        items: vec![item(shop.tea, 2000)],
        discount_rule_ids: vec![],
        loyalty_points_to_redeem: 200,
        payments: vec![PayloadPayment {
            method: PaymentMethod::Cash,
            tendered_currency: CurrencyCode::KWD,
            tendered_amount: 0,
            reference: None,
        }],
        notes: None,
    };
    let actor = SaleActor {
        user_id: shop.cashier,
        role: Role::Cashier,
    };
    let sale = sales::create(&mut shop.db.conn(), &actor, &payload, &shop.config, at(5))
        .expect("sale")
        .transaction_id;
    let receipt = sales::load_receipt(&shop.db.conn(), sale, true).expect("receipt");
    assert_eq!(receipt.total, 0);
    assert!(receipt.payments.is_empty(), "no zero payment rows");
    assert_eq!(shop.balance(layla), 100);
    // Refunding it returns the points and no money.
    let line = refunds::original_lines(&shop.db.conn(), sale).expect("lines")[0].item_id;
    refunds::refund(
        &mut shop.db.conn(),
        &shop.reverser(),
        &RefundInput {
            transaction_id: sale,
            idempotency_key: Uuid::now_v7(),
            lines: vec![RefundLine {
                item_id: line,
                quantity_milli: 2000,
            }],
            method: RefundMethod::Cash,
            restock: false,
            reason: "Wrong order".into(),
        },
        at(6),
    )
    .expect("refund");
    assert_eq!(shop.balance(layla), 300);
    // The same bill without enough points is refused.
    payload.idempotency_key = Uuid::now_v7();
    payload.loyalty_points_to_redeem = 250;
    assert!(sales::create(&mut shop.db.conn(), &actor, &payload, &shop.config, at(7)).is_err());
}

#[test]
fn memberships_are_sold_renewed_priced_and_refunded() {
    use crate::repo::memberships::{self, MemberFilter, MemberState, PlanInput, Status};
    let shop = shop();
    let gold = memberships::save_plan(
        &shop.db.conn(),
        PlanInput {
            id: None,
            name: "Gold".into(),
            description: Some("10% off everything, double points".into()),
            price: 10_000,
            duration_days: 30,
            discount_bps: 1_000,
            points_multiplier_bps: 20_000,
            color: None,
            is_active: true,
        },
        &shop.config,
        at(0),
    )
    .expect("plan");
    // The plan is on sale as a product in the Memberships category.
    let product = catalog::get(&shop.db.conn(), gold.product_id)
        .expect("q")
        .expect("product");
    assert_eq!((product.name.as_str(), product.price), ("Gold", 10_000));
    assert!(product.category_id.is_some() && !product.track_stock);

    // Selling it needs a customer.
    let err = shop
        .sell(vec![item(gold.product_id, 1000)], None, 0, 3)
        .expect_err("no customer");
    assert!(err.message.contains("Gold"), "{}", err.message);

    let layla = shop
        .register("Layla", Some("0912345678"))
        .expect("layla")
        .meta
        .id;
    let sale = shop
        .sell(vec![item(gold.product_id, 1000)], Some(layla), 0, 5)
        .expect("sale");
    let (membership, _) = memberships::active(&shop.db.conn(), layla, at(6))
        .expect("q")
        .expect("member");
    assert_eq!(membership.transaction_id, Some(sale));
    assert_eq!(membership.starts_at, at(5));
    assert_eq!(membership.ends_at, at(5 + 30 * 24 * 60));
    let card = membership.card_number.clone();
    assert!(card.len() == 13 && card.starts_with("29"), "{card}");
    assert_eq!(
        pos_hardware::label::symbology(&card),
        Some(pos_hardware::label::Symbology::Ean13),
        "the card number is a valid barcode"
    );
    // The receipt names the membership.
    let receipt = sales::load_receipt(&shop.db.conn(), sale, true).expect("receipt");
    assert_eq!(receipt.member.expect("member").plan_name, "Gold");

    // Members pay 10% less and earn double points: 4 teas = 4.000 → 3.600,
    // 3 points → 6.
    let quote = price(
        &shop.db.conn(),
        &[item(shop.tea, 4000)],
        &[],
        Some(LoyaltyRequest {
            customer_id: layla,
            redeem_points: 0,
        }),
        &shop.config,
        at(10),
    )
    .expect("quote");
    let member_quote = quote.loyalty.expect("loyalty");
    assert_eq!(quote.cart.quote.total, 3_600);
    assert_eq!(member_quote.member.as_ref().expect("member").discount, 400);
    assert_eq!(member_quote.points_earned, 6);

    // Renewing adds a period after the current one, same card.
    shop.sell(vec![item(gold.product_id, 1000)], Some(layla), 0, 60)
        .expect("renew");
    let periods = memberships::members(
        &shop.db.conn(),
        &MemberFilter {
            query: String::new(),
            state: None,
            customer_id: Some(layla),
            limit: 10,
        },
        at(61),
    )
    .expect("members");
    assert_eq!(periods.len(), 2);
    assert_eq!(periods[0].state, MemberState::Upcoming);
    assert_eq!(periods[0].membership.starts_at, membership.ends_at);
    assert_eq!(periods[0].membership.card_number, card);

    // The card finds the customer at the till.
    let found = customers::search(&shop.db.conn(), &card, 5).expect("search");
    assert_eq!(found[0].meta.id, layla);

    // Refunding the first sale cancels the period it bought.
    let line = refunds::original_lines(&shop.db.conn(), sale).expect("lines")[0].item_id;
    refunds::refund(
        &mut shop.db.conn(),
        &shop.reverser(),
        &RefundInput {
            transaction_id: sale,
            idempotency_key: Uuid::now_v7(),
            lines: vec![RefundLine {
                item_id: line,
                quantity_milli: 1000,
            }],
            method: RefundMethod::Cash,
            restock: false,
            reason: "Changed mind".into(),
        },
        at(70),
    )
    .expect("refund");
    let first = memberships::get(&shop.db.conn(), membership.meta.id)
        .expect("q")
        .expect("row");
    assert_eq!(first.status, Status::Cancelled);
    assert!(
        memberships::active(&shop.db.conn(), layla, at(71))
            .expect("q")
            .is_none(),
        "the renewal starts later; nothing is active now"
    );
}
