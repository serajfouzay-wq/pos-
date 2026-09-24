//! Multi-device sync against an in-memory server that mirrors the semantics
//! of `sync_push` / `sync_pull` (supabase/migrations/20260924000000_sync.sql,
//! itself tested in supabase/tests/sync.test.sql): dedupe by event id, LWW on
//! `(updated_at, event_id)`, insert-once appends, a global change sequence,
//! and pulls that skip the caller's own versions.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use chrono::Duration;
use pos_core::config::ClientConfig;
use pos_core::currency::CurrencyCode;
use pos_core::rbac::Role;
use pos_core::sales::{OrderType, PaymentMethod};
use pos_core::time::{Clock, Timestamp};
use pos_hwid::HardwareComponents;
use rusqlite::params;
use serde_json::Value as Json;
use uuid::Uuid;

use super::apply::{self, Strategy, ENTITIES};
use super::engine::{backoff, SyncEngine, SyncState, SyncStatus};
use super::protocol::{Change, PullRequest, PullResponse, PushRequest, PushResponse, Rejected};
use super::transport::{SyncError, SyncTransport};
use crate::db::Database;
use crate::license::SyncCredentials;
use crate::repo::catalog::{self, Product, Unit};
use crate::repo::sales::{self, PayloadItem, PayloadPayment, SaleActor, TransactionPayload};
use crate::repo::{shifts, users, Meta};

const CONFIG: &str =
    include_str!("../../../../../packages/shared/contracts/client-config.example.json");
const DB_SCHEMA: &str = include_str!("../../../../../packages/shared/contracts/db-schema.json");

fn t0() -> Timestamp {
    "2026-09-24T09:00:00.000Z".parse().expect("ts")
}

fn at(seconds: i64) -> Timestamp {
    t0().checked_add(Duration::seconds(seconds)).expect("ts")
}

// ── In-memory server ─────────────────────────────────────────────────────

struct Stored {
    entity: String,
    row: Json,
    seq: u64,
    origin: Uuid,
    event_id: Option<Uuid>,
}

#[derive(Default)]
struct ServerState {
    seq: u64,
    /// `(entity, id)` → current version.
    rows: BTreeMap<(String, String), Stored>,
    seen: HashSet<Uuid>,
    offline: bool,
    /// Apply the batch, then fail as if the response was lost.
    lose_ack: bool,
    /// Reject every event of this entity (`retryable` flag).
    reject: Option<(String, bool)>,
    page_size: Option<usize>,
    pushed_event_ids: Vec<Uuid>,
}

#[derive(Default)]
struct MemoryServer(Mutex<ServerState>);

impl MemoryServer {
    fn state(&self) -> std::sync::MutexGuard<'_, ServerState> {
        self.0.lock().expect("server")
    }

    fn count(&self, entity: &str) -> usize {
        self.state()
            .rows
            .values()
            .filter(|s| s.entity == entity)
            .count()
    }

    fn row(&self, entity: &str, id: Uuid) -> Option<Json> {
        self.state()
            .rows
            .get(&(entity.to_owned(), id.to_string()))
            .map(|s| s.row.clone())
    }
}

fn updated_at(row: &Json) -> String {
    row["updated_at"].as_str().unwrap_or_default().to_owned()
}

impl SyncTransport for MemoryServer {
    fn push(&self, _: &SyncCredentials, request: &PushRequest) -> Result<PushResponse, SyncError> {
        let mut s = self.state();
        if s.offline {
            return Err(SyncError::Offline("connection refused".into()));
        }
        let mut acknowledged = Vec::new();
        let mut rejected = Vec::new();
        for event in &request.events {
            s.pushed_event_ids.push(event.event_id);
            if s.seen.contains(&event.event_id) {
                acknowledged.push(event.event_id);
                continue;
            }
            if let Some((entity, retryable)) = s.reject.clone() {
                if entity == event.entity_type {
                    rejected.push(Rejected {
                        event_id: event.event_id,
                        reason: "rejected by test".into(),
                        retryable,
                    });
                    continue;
                }
            }
            let Some(strategy) = apply::strategy(&event.entity_type) else {
                rejected.push(Rejected {
                    event_id: event.event_id,
                    reason: "unknown entity".into(),
                    retryable: false,
                });
                continue;
            };
            let key = (event.entity_type.clone(), event.entity_id.to_string());
            let mut row = event.payload.clone();
            // Derived aggregates are the server's own, never the device's cache.
            for derived in ["stock_on_hand_milli", "loyalty_points"] {
                if let Some(v) = row.get_mut(derived) {
                    *v = Json::from(0);
                }
            }
            let wins = match (strategy, s.rows.get(&key)) {
                (_, None) => true,
                (Strategy::LastWriteWins, Some(current)) => {
                    let current_event = current.event_id.map(|e| e.to_string()).unwrap_or_default();
                    (updated_at(&row), event.event_id.to_string())
                        > (updated_at(&current.row), current_event)
                }
                (Strategy::AppendOnly | Strategy::AdditiveDelta, Some(_)) => false,
            };
            if wins {
                s.seq += 1;
                let stored = Stored {
                    entity: event.entity_type.clone(),
                    row,
                    seq: s.seq,
                    origin: request.device_id,
                    event_id: (strategy == Strategy::LastWriteWins).then_some(event.event_id),
                };
                s.rows.insert(key, stored);
            }
            s.seen.insert(event.event_id);
            acknowledged.push(event.event_id);
        }
        if s.lose_ack {
            return Err(SyncError::Offline("connection reset".into()));
        }
        Ok(PushResponse {
            acknowledged,
            rejected,
            server_time: at(0),
        })
    }

    fn pull(&self, _: &SyncCredentials, request: &PullRequest) -> Result<PullResponse, SyncError> {
        let s = self.state();
        if s.offline {
            return Err(SyncError::Offline("connection refused".into()));
        }
        let cursor: u64 = request
            .cursor
            .as_deref()
            .map_or(0, |c| c.parse().expect("cursor"));
        let limit = s.page_size.unwrap_or(request.limit as usize);
        let mut after: Vec<&Stored> = s.rows.values().filter(|r| r.seq > cursor).collect();
        after.sort_by_key(|r| r.seq);
        let mut changes = Vec::new();
        let mut next = cursor;
        let mut has_more = false;
        for stored in after {
            if changes.len() == limit {
                has_more = true;
                break;
            }
            next = stored.seq;
            if stored.origin != request.device_id {
                changes.push(Change {
                    entity_type: stored.entity.clone(),
                    row: stored.row.clone(),
                    event_id: stored.event_id,
                });
            }
        }
        Ok(PullResponse {
            changes,
            next_cursor: Some(next.to_string()),
            has_more,
        })
    }
}

// ── Tills ────────────────────────────────────────────────────────────────

struct TestClock(Mutex<Timestamp>);

impl Clock for TestClock {
    fn now(&self) -> Timestamp {
        *self.0.lock().expect("clock")
    }
}

impl TestClock {
    fn set(&self, ts: Timestamp) {
        *self.0.lock().expect("clock") = ts;
    }
}

struct Till {
    db: Arc<Database>,
    device: Uuid,
    engine: SyncEngine,
    clock: Arc<TestClock>,
    creds: SyncCredentials,
}

fn credentials() -> SyncCredentials {
    SyncCredentials {
        token: "token".into(),
        device_key: "key".to_owned().into(),
    }
}

fn till(server: &Arc<MemoryServer>, name: &str) -> Till {
    let hw = HardwareComponents::new("CPU", name, "BOARD", "VOL").expect("hw");
    let db = Arc::new(Database::open_in_memory(&hw.database_key(Uuid::nil())).expect("db"));
    let device = Uuid::now_v7();
    db.conn()
        .execute(
            "INSERT INTO device (id, created_at, updated_at, name) VALUES (?1, ?2, ?2, ?3)",
            params![device.to_string(), t0().to_string(), name],
        )
        .expect("device");
    let clock = Arc::new(TestClock(Mutex::new(t0())));
    let engine = SyncEngine::new(
        Some(Arc::clone(server) as Arc<dyn SyncTransport>),
        Arc::clone(&clock) as Arc<dyn Clock>,
    );
    Till {
        db,
        device,
        engine,
        clock,
        creds: credentials(),
    }
}

impl Till {
    fn sync(&self) -> super::SyncReport {
        self.engine
            .run(&self.db, Some(&self.creds))
            .expect("sync round")
    }

    fn status(&self) -> SyncStatus {
        self.engine.status(&self.db)
    }

    fn count(&self, sql: &str) -> i64 {
        self.db
            .conn()
            .query_row(sql, [], |r| r.get(0))
            .expect("count")
    }

    fn product(&self, id: Uuid) -> Option<Product> {
        catalog::get(&self.db.conn(), id).expect("get")
    }

    fn save(&self, product: &Product) {
        catalog::save(&self.db.conn(), product, product.meta.updated_at).expect("save");
    }

    fn open_shift(&self, user: Uuid) {
        shifts::open(&self.db.conn(), self.device, user, 10_000, t0()).expect("shift");
    }

    fn sell(&self, cashier: Uuid, product: Uuid, quantity_milli: i64, now: Timestamp) -> Uuid {
        let config = ClientConfig::parse(CONFIG).expect("config");
        let payload = TransactionPayload {
            idempotency_key: Uuid::new_v4(),
            customer_id: None,
            order_type: OrderType::Counter,
            table_label: None,
            items: vec![PayloadItem {
                product_id: product,
                quantity_milli,
                modifier_ids: vec![],
                course: None,
                note: None,
                combo: None,
            }],
            discount_rule_ids: vec![],
            loyalty_points_to_redeem: 0,
            payments: vec![PayloadPayment {
                method: PaymentMethod::Cash,
                tendered_currency: CurrencyCode::KWD,
                tendered_amount: 1_000_000,
                reference: None,
            }],
            notes: None,
        };
        let actor = SaleActor {
            user_id: cashier,
            role: Role::Cashier,
        };
        sales::create(&mut self.db.conn(), &actor, &payload, &config, now)
            .expect("sale")
            .transaction_id
    }
}

fn new_product(price: i64, track_stock: bool) -> Product {
    Product {
        meta: Meta::new(t0()),
        name: "Dates".into(),
        name_localized: serde_json::json!({ "ar": "تمر" }),
        category_id: None,
        sku: None,
        barcode: Some("6281000000011".into()),
        price,
        cost: None,
        tax_rate_bps: 0,
        unit: Unit::Each,
        sold_by_weight: false,
        track_stock,
        stock_on_hand_milli: 0,
        reorder_threshold_milli: None,
        reorder_quantity_milli: None,
        image_asset: None,
        quick_key_position: None,
        is_active: true,
    }
}

fn edited(product: &Product, price: i64, when: Timestamp) -> Product {
    let mut p = product.clone();
    p.price = price;
    p.meta.updated_at = when;
    p
}

/// Two tills of one shop sharing a cashier and a tracked product.
fn shop() -> (Arc<MemoryServer>, Till, Till, Uuid, Product) {
    let server = Arc::new(MemoryServer::default());
    let (a, b) = (till(&server, "A"), till(&server, "B"));
    let hash = users::hash_pin("1234").expect("hash");
    let cashier = users::create(&a.db.conn(), "Sara", Role::Cashier, hash, t0()).expect("user");
    let product = new_product(1_250, true);
    a.save(&product);
    a.sync();
    b.sync();
    (server, a, b, cashier.meta.id, product)
}

// ── Tests ────────────────────────────────────────────────────────────────

#[test]
fn entity_strategies_match_the_contract() {
    let schema: Json = serde_json::from_str(DB_SCHEMA).expect("schema");
    let tables = schema["tables"].as_object().expect("tables");
    let synced: BTreeMap<&str, &str> = tables
        .iter()
        .filter_map(|(name, t)| t["sync"].as_str().map(|s| (name.as_str(), s)))
        .collect();
    let ours: BTreeMap<&str, &str> = ENTITIES
        .iter()
        .map(|(name, s)| {
            (
                *name,
                match s {
                    Strategy::LastWriteWins => "last_write_wins",
                    Strategy::AppendOnly => "append_only",
                    Strategy::AdditiveDelta => "additive_delta",
                },
            )
        })
        .collect();
    assert_eq!(ours, synced);
}

#[test]
fn backoff_doubles_and_caps_at_an_hour() {
    assert_eq!(backoff(1), Duration::seconds(60));
    assert_eq!(backoff(2), Duration::seconds(120));
    assert_eq!(backoff(10), Duration::hours(1));
    assert_eq!(backoff(i64::MAX), Duration::hours(1));
}

#[test]
fn a_new_till_bootstraps_the_shop_without_echoing_it_back() {
    let (server, a, b, cashier, product) = shop();
    assert_eq!(a.status().pending, 0, "A's outbox drained");
    let pulled = b.product(product.meta.id).expect("B has the product");
    assert_eq!(pulled.price, 1_250);
    assert_eq!(pulled.name_localized, serde_json::json!({ "ar": "تمر" }));
    assert!(
        users::get(&b.db.conn(), cashier).expect("q").is_some(),
        "users follow"
    );
    // Rows written by sync are not local changes: nothing to push back.
    assert_eq!(b.count("SELECT count(*) FROM sync_queue"), 0);
    let pushes = server.state().pushed_event_ids.len();
    b.sync();
    assert_eq!(server.state().pushed_event_ids.len(), pushes);
    // A's own versions are not sent back to A.
    assert_eq!(a.sync().pulled, 0);
}

#[test]
fn sales_on_two_tills_converge_including_stock() {
    let (server, a, b, cashier, product) = shop();
    a.open_shift(cashier);
    b.open_shift(cashier);
    let sale_a = a.sell(cashier, product.meta.id, 2_000, at(60));
    let sale_b = b.sell(cashier, product.meta.id, 1_000, at(61));
    a.sync();
    b.sync();
    a.sync();

    for till in [&a, &b] {
        assert_eq!(till.count("SELECT count(*) FROM transactions"), 2);
        assert_eq!(till.count("SELECT count(*) FROM transaction_items"), 2);
        assert_eq!(till.count("SELECT count(*) FROM stock_movements"), 2);
        assert_eq!(till.count("SELECT count(*) FROM shifts"), 2);
        assert_eq!(
            till.product(product.meta.id)
                .expect("p")
                .stock_on_hand_milli,
            -3_000,
            "stock is the sum of both tills' deltas"
        );
        assert_eq!(till.status().pending, 0);
    }
    assert!(
        sales::load_receipt(&b.db.conn(), sale_a, false).is_ok(),
        "A's sale on B"
    );
    assert!(
        sales::load_receipt(&a.db.conn(), sale_b, false).is_ok(),
        "B's sale on A"
    );
    assert_eq!(server.count("transactions"), 2);
}

fn lww_case(newer_pushes_first: bool) {
    let (server, a, b, _, product) = shop();
    let older = edited(&product, 1_300, at(10));
    let newer = edited(&product, 1_400, at(20));
    a.save(&older);
    b.save(&newer);
    if newer_pushes_first {
        b.sync();
        a.sync();
    } else {
        a.sync();
        b.sync();
    }
    a.sync();
    b.sync();
    let id = product.meta.id;
    assert_eq!(server.row("products", id).expect("row")["price"], 1_400);
    assert_eq!(a.product(id).expect("a").price, 1_400);
    assert_eq!(b.product(id).expect("b").price, 1_400);
}

#[test]
fn last_write_wins_when_the_older_edit_arrives_first() {
    lww_case(false);
}

#[test]
fn last_write_wins_when_the_newer_edit_arrives_first() {
    lww_case(true);
}

#[test]
fn a_pull_never_clobbers_a_newer_pending_local_edit() {
    let (server, a, b, _, product) = shop();
    let id = product.meta.id;
    a.save(&edited(&product, 1_300, at(10)));
    a.sync();
    // B edited later but cannot push yet (server refuses products for now).
    b.save(&edited(&product, 1_400, at(20)));
    server.state().reject = Some(("products".into(), true));
    b.sync();
    assert_eq!(
        b.product(id).expect("b").price,
        1_400,
        "pending local edit kept"
    );
    assert_eq!(b.status().pending, 1);

    server.state().reject = None;
    b.clock.set(at(3_600)); // past the retry back-off
    b.sync();
    a.sync();
    assert_eq!(a.product(id).expect("a").price, 1_400);
    assert_eq!(server.row("products", id).expect("row")["price"], 1_400);
}

#[test]
fn equal_timestamps_resolve_to_the_same_winner_everywhere() {
    let (server, a, b, _, product) = shop();
    let id = product.meta.id;
    let when = at(30);
    a.save(&edited(&product, 1_300, when));
    b.save(&edited(&product, 1_400, when));
    // B's edit is pending while it pulls A's same-timestamp version.
    server.state().reject = Some(("products".into(), true));
    a.sync();
    b.sync();
    server.state().reject = None;
    a.sync();
    b.clock.set(at(3_600));
    b.sync();
    a.sync();
    b.sync();
    let winner = server.row("products", id).expect("row")["price"]
        .as_i64()
        .expect("price");
    assert_eq!(a.product(id).expect("a").price, winner);
    assert_eq!(b.product(id).expect("b").price, winner);
}

#[test]
fn a_lost_acknowledgement_is_replayed_without_duplicates() {
    let (server, a, b, cashier, product) = shop();
    a.open_shift(cashier);
    a.sell(cashier, product.meta.id, 1_000, at(60));
    server.state().lose_ack = true;
    let report = a.sync();
    assert!(!report.online);
    assert_eq!(a.status().state, SyncState::Offline);
    assert!(a.status().pending > 0, "nothing marked sent without an ack");
    assert_eq!(
        server.count("transactions"),
        1,
        "…although the server applied it"
    );

    server.state().lose_ack = false;
    let report = a.sync();
    assert!(report.online && report.pushed > 0);
    assert_eq!(a.status().pending, 0);
    assert_eq!(server.count("transactions"), 1);
    assert_eq!(server.count("stock_movements"), 1);
    b.sync();
    assert_eq!(
        b.product(product.meta.id).expect("b").stock_on_hand_milli,
        -1_000
    );
}

#[test]
fn permanent_rejections_are_parked_and_the_rest_flows() {
    let (server, a, b, cashier, product) = shop();
    a.open_shift(cashier);
    server.state().reject = Some(("shifts".into(), false));
    a.sell(cashier, product.meta.id, 1_000, at(60));
    let report = a.sync();
    assert_eq!(report.rejected, 1);
    let status = a.status();
    assert_eq!((status.pending, status.parked), (0, 1));
    assert_eq!(status.state, SyncState::Idle);
    assert_eq!(
        a.count("SELECT count(*) FROM sync_queue WHERE deleted_at IS NOT NULL AND last_error = 'rejected by test'"),
        1
    );
    b.sync();
    assert_eq!(
        b.count("SELECT count(*) FROM transactions"),
        1,
        "the sale still synced"
    );
    // Parked events are not retried.
    server.state().reject = None;
    let pushes = server.state().pushed_event_ids.len();
    a.clock.set(at(86_400));
    a.sync();
    assert_eq!(server.state().pushed_event_ids.len(), pushes);
}

#[test]
fn retryable_rejections_back_off() {
    let (server, a, _, _, product) = shop();
    a.save(&edited(&product, 1_300, at(10)));
    server.state().reject = Some(("products".into(), true));
    a.sync();
    let (attempts, next): (i64, String) =
        a.db.conn()
            .query_row(
                "SELECT attempt_count, next_attempt_at FROM sync_queue WHERE sent_at IS NULL",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("row");
    assert_eq!(attempts, 1);
    assert_eq!(next, at(60).to_string());

    server.state().reject = None;
    let pushes = server.state().pushed_event_ids.len();
    a.sync();
    assert_eq!(
        server.state().pushed_event_ids.len(),
        pushes,
        "not before the back-off"
    );
    a.clock.set(at(60));
    a.sync();
    assert_eq!(a.status().pending, 0);
    assert_eq!(
        server.row("products", product.meta.id).expect("row")["price"],
        1_300
    );
}

#[test]
fn soft_deletes_propagate() {
    let (_, a, b, _, product) = shop();
    let mut gone = edited(&product, product.price, at(10));
    gone.meta.deleted_at = Some(at(10));
    a.save(&gone);
    a.sync();
    b.sync();
    let deleted_at: Option<String> =
        b.db.conn()
            .query_row(
                "SELECT deleted_at FROM products WHERE id = ?1",
                [product.meta.id.to_string()],
                |r| r.get(0),
            )
            .expect("row");
    assert_eq!(deleted_at, Some(at(10).to_string()));
    assert!(
        b.product(product.meta.id).is_none(),
        "hidden from the catalogue"
    );
}

#[test]
fn menus_floor_plans_and_open_orders_reach_the_other_till() {
    use crate::repo::menu::{self, DiningTable, Modifier, ModifierGroup, TableShape};
    use crate::repo::orders::{self, OpenOrder, OpenOrderItem, OpenOrderStatus};

    let (_, a, b, cashier, product) = shop();
    let group = ModifierGroup {
        meta: Meta::new(at(1)),
        name: "Milk".into(),
        name_localized: serde_json::json!({ "ar": "حليب" }),
        min_select: 0,
        max_select: 1,
        sort_order: 0,
        is_active: true,
    };
    let oat = Modifier {
        meta: Meta::new(at(1)),
        group_id: group.meta.id,
        name: "Oat".into(),
        name_localized: serde_json::json!({}),
        price_delta: 200,
        is_default: false,
        sort_order: 0,
        is_active: true,
    };
    let table = DiningTable {
        meta: Meta::new(at(1)),
        label: "T1".into(),
        area: "Hall".into(),
        seats: 4,
        shape: TableShape::Square,
        grid_x: 1,
        grid_y: 1,
        sort_order: 0,
        is_active: true,
    };
    let order = OpenOrder {
        meta: Meta::new(at(2)),
        device_id: a.device,
        order_type: OrderType::DineIn,
        table_id: Some(table.meta.id),
        label: None,
        guests: 2,
        status: OpenOrderStatus::Open,
        items: vec![OpenOrderItem {
            line_id: Uuid::now_v7(),
            product_id: product.meta.id,
            quantity_milli: 2000,
            modifier_ids: vec![oat.meta.id],
            course: Some(1),
            note: Some("no ice".into()),
            combo: None,
            fired_at: None,
            added_by: cashier,
            added_at: at(2),
        }],
        transaction_ids: vec![],
        opened_by: cashier,
        opened_at: at(2),
        closed_at: None,
        notes: None,
    };
    {
        let conn = a.db.conn();
        menu::save_group(&conn, &group, &[oat], at(1)).expect("group");
        menu::set_product_groups(&conn, product.meta.id, &[group.meta.id], at(1)).expect("link");
        menu::save_table(&conn, &table, at(1)).expect("table");
        orders::save(&conn, &order, at(2)).expect("order");
    }
    a.sync();
    b.sync();
    let conn = b.db.conn();
    let pulled = menu::menu(&conn, false).expect("menu");
    assert_eq!(
        pulled.modifier_groups[0].group.name_localized,
        serde_json::json!({ "ar": "حليب" })
    );
    assert_eq!(pulled.modifier_groups[0].modifiers[0].price_delta, 200);
    assert_eq!(
        pulled.product_modifier_groups[&product.meta.id],
        vec![group.meta.id]
    );
    assert_eq!(pulled.dining_tables[0].label, "T1");
    let orders_on_b = orders::open(&conn).expect("orders");
    assert_eq!(orders_on_b, vec![order], "items survive the round trip");
}

#[test]
fn pulls_page_through_everything_and_resume_from_the_cursor() {
    let server = Arc::new(MemoryServer::default());
    let (a, b) = (till(&server, "A"), till(&server, "B"));
    for i in 0..7 {
        let mut p = new_product(100 + i, false);
        p.barcode = None;
        a.save(&p);
    }
    a.sync();
    server.state().page_size = Some(2);
    assert_eq!(b.sync().pulled, 7);
    assert_eq!(b.count("SELECT count(*) FROM products"), 7);
    assert_eq!(b.sync().pulled, 0, "cursor persisted");
}

#[test]
fn offline_keeps_changes_queued() {
    let (server, a, _, _, product) = shop();
    a.save(&edited(&product, 1_300, at(10)));
    server.state().offline = true;
    let report = a.sync();
    assert!(!report.online);
    let status = a.status();
    assert_eq!((status.state, status.pending), (SyncState::Offline, 1));
    // Offline is not the event's fault: no back-off is recorded.
    assert_eq!(
        a.count("SELECT attempt_count FROM sync_queue WHERE sent_at IS NULL"),
        0
    );
    server.state().offline = false;
    assert_eq!(a.sync().pushed, 1);
}

#[test]
fn without_credentials_or_cloud_nothing_is_sent() {
    let server = Arc::new(MemoryServer::default());
    let a = till(&server, "A");
    let err = a.engine.run(&a.db, None).expect_err("no credentials");
    assert!(matches!(err, SyncError::Unauthorized(_)));
    assert_eq!(a.status().state, SyncState::Error);

    let disabled = SyncEngine::new(None, Arc::clone(&a.clock) as Arc<dyn Clock>);
    let report = disabled.run(&a.db, Some(&a.creds)).expect("disabled");
    assert!(!report.online);
    assert_eq!(disabled.status(&a.db).state, SyncState::Disabled);
}

#[test]
fn status_changes_are_published() {
    let server = Arc::new(MemoryServer::default());
    let a = till(&server, "A");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    a.engine
        .set_listener(move |s| sink.lock().expect("sink").push(s.state));
    a.sync();
    assert_eq!(
        *seen.lock().expect("seen"),
        vec![SyncState::Syncing, SyncState::Idle]
    );
}

#[test]
fn bad_rows_are_skipped_not_fatal() {
    let server = Arc::new(MemoryServer::default());
    let a = till(&server, "A");
    let conn = a.db.conn();
    let bad = Change {
        entity_type: "products".into(),
        row: serde_json::json!({ "id": Uuid::now_v7().to_string(), "price": 1.5 }),
        event_id: None,
    };
    assert!(apply::apply(&conn, &bad).is_err(), "floats are never money");
    let unknown = Change {
        entity_type: "license".into(),
        row: serde_json::json!({}),
        event_id: None,
    };
    assert!(matches!(
        apply::apply(&conn, &unknown),
        Err(apply::ApplyError::UnknownEntity(_))
    ));
}

/// Full stack against a running cloud: real HTTP transport, edge functions
/// (`supabase/functions/dev-server.ts`) and the SQL in Postgres. Run by
/// `scripts/e2e-sync.sh`, which starts both and sets `POS_E2E_SUPABASE_URL`.
mod live {
    use pos_core::time::SystemClock;
    use pos_license::activation::ActivationRequest;
    use pos_license::issuer::{issue_license, IssueOptions, SigningKey};

    use super::*;
    use crate::license::cloud::{CloudDecision, CloudRequest, CloudValidator, SupabaseValidator};
    use crate::sync::HttpTransport;

    const DEV_PRIVATE: &str = include_str!("../../../../../keys/dev/license-dev.private.pem");
    const ANON_KEY: &str = "e2e-anon-key";

    struct LiveTill {
        db: Arc<Database>,
        device: Uuid,
        engine: SyncEngine,
        creds: SyncCredentials,
    }

    /// A till on its own (simulated) machine, activated with the cloud.
    fn live_till(url: &str, machine: &str) -> LiveTill {
        let config = ClientConfig::parse(CONFIG).expect("config");
        let hw = HardwareComponents::new("CPU", machine, "BOARD", "VOL").expect("hw");
        let request = ActivationRequest {
            client_id: config.client_id,
            fingerprint: hw.fingerprint(config.client_id).to_string(),
            device_key_hash: hw.device_key(config.client_id).public_hash(),
            device_name: machine.into(),
            app_version: "e2e".into(),
        };
        let key = SigningKey::from_unencrypted_pem(DEV_PRIVATE).expect("dev key");
        let issued = issue_license(
            &key,
            &request,
            &IssueOptions {
                client_slug: config.client_slug.clone(),
                business_type: config.business_type,
                max_devices: 5,
                expires_at: None,
            },
            SystemClock.now(),
        )
        .expect("issue");
        let verdict = SupabaseValidator::new(url, ANON_KEY)
            .validate(&CloudRequest {
                token: &issued.token,
                fingerprint: &request.fingerprint,
                device_name: machine,
                app_version: "e2e",
            })
            .expect("license-validate reachable");
        assert_eq!(verdict.status, CloudDecision::Active, "{verdict:?}");

        let db =
            Arc::new(Database::open_in_memory(&hw.database_key(config.client_id)).expect("db"));
        let device = Uuid::now_v7();
        db.conn()
            .execute(
                "INSERT INTO device (id, created_at, updated_at, name) VALUES (?1, ?2, ?2, ?3)",
                params![device.to_string(), SystemClock.now().to_string(), machine],
            )
            .expect("device");
        let transport = Arc::new(HttpTransport::new(url, ANON_KEY)) as Arc<dyn SyncTransport>;
        LiveTill {
            db,
            device,
            engine: SyncEngine::new(Some(transport), Arc::new(SystemClock)),
            creds: SyncCredentials {
                token: issued.token,
                device_key: hw.device_key(config.client_id).secret_hex(),
            },
        }
    }

    impl LiveTill {
        fn sync(&self) -> super::super::SyncReport {
            self.engine
                .run(&self.db, Some(&self.creds))
                .expect("sync round")
        }

        fn as_till(&self) -> Till {
            Till {
                db: Arc::clone(&self.db),
                device: self.device,
                engine: SyncEngine::new(None, Arc::new(SystemClock)),
                clock: Arc::new(TestClock(Mutex::new(SystemClock.now()))),
                creds: credentials(),
            }
        }
    }

    #[test]
    #[ignore = "needs a running cloud: scripts/e2e-sync.sh"]
    fn two_tills_converge_through_the_real_cloud() {
        let url = std::env::var("POS_E2E_SUPABASE_URL").expect("POS_E2E_SUPABASE_URL");
        let run = Uuid::now_v7().simple().to_string();
        let a = live_till(&url, &format!("A-{run}"));
        let b = live_till(&url, &format!("B-{run}"));
        let (ta, tb) = (a.as_till(), b.as_till());

        let hash = users::hash_pin("1234").expect("hash");
        let cashier = users::create(&a.db.conn(), "Sara", Role::Cashier, hash, SystemClock.now())
            .expect("user")
            .meta
            .id;
        let mut product = new_product(1_250, true);
        product.meta = Meta::new(SystemClock.now());
        product.barcode = None;
        ta.save(&product);
        let first = a.sync();
        assert!(first.online && first.pushed >= 2, "{first:?}");
        assert!(b.sync().pulled >= 2, "B bootstraps the users and catalogue");
        assert!(tb.product(product.meta.id).is_some());

        ta.open_shift(cashier);
        tb.open_shift(cashier);
        ta.sell(cashier, product.meta.id, 2_000, SystemClock.now());
        tb.sell(cashier, product.meta.id, 1_000, SystemClock.now());
        a.sync();
        b.sync();
        a.sync();
        for till in [&ta, &tb] {
            assert_eq!(
                till.product(product.meta.id)
                    .expect("p")
                    .stock_on_hand_milli,
                -3_000
            );
            assert_eq!(till.count("SELECT count(*) FROM transactions"), 2);
        }

        // Newest price edit wins everywhere.
        let now = SystemClock.now();
        ta.save(&edited(&product, 1_300, now));
        tb.save(&edited(
            &product,
            1_400,
            now.checked_add(Duration::seconds(1)).expect("ts"),
        ));
        a.sync();
        b.sync();
        a.sync();
        assert_eq!(ta.product(product.meta.id).expect("a").price, 1_400);
        assert_eq!(tb.product(product.meta.id).expect("b").price, 1_400);
        assert_eq!(a.engine.status(&a.db).pending, 0);

        // A table seated on one till shows as occupied on the other.
        use crate::repo::menu::{self, DiningTable, TableShape};
        use crate::repo::orders::{self, OpenOrder, OpenOrderItem, OpenOrderStatus};
        let now = SystemClock.now();
        let table = DiningTable {
            meta: Meta::new(now),
            label: "T1".into(),
            area: "Hall".into(),
            seats: 4,
            shape: TableShape::Square,
            grid_x: 1,
            grid_y: 1,
            sort_order: 0,
            is_active: true,
        };
        menu::save_table(&a.db.conn(), &table, now).expect("table");
        let order = OpenOrder {
            meta: Meta::new(now),
            device_id: a.device,
            order_type: OrderType::DineIn,
            table_id: Some(table.meta.id),
            label: None,
            guests: 2,
            status: OpenOrderStatus::Open,
            items: vec![OpenOrderItem {
                line_id: Uuid::now_v7(),
                product_id: product.meta.id,
                quantity_milli: 1000,
                modifier_ids: vec![],
                course: Some(1),
                note: Some("no ice".into()),
                combo: None,
                fired_at: None,
                added_by: cashier,
                added_at: now,
            }],
            transaction_ids: vec![],
            opened_by: cashier,
            opened_at: now,
            closed_at: None,
            notes: None,
        };
        orders::save(&a.db.conn(), &order, now).expect("order");
        a.sync();
        b.sync();
        let seen = orders::at_table(&b.db.conn(), table.meta.id)
            .expect("read")
            .expect("B sees the open order");
        assert_eq!(seen.items, order.items);

        // The license token alone is not a credential.
        let stolen = SyncCredentials {
            token: a.creds.token.clone(),
            device_key: "00".repeat(32).into(),
        };
        let err = a
            .engine
            .run(&a.db, Some(&stolen))
            .expect_err("wrong device key");
        assert!(matches!(err, SyncError::Unauthorized(_)), "{err:?}");
    }
}
