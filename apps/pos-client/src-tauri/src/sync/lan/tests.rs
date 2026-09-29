//! Two tills and a hub on this machine's loopback, over real HTTP: the
//! shop network with no internet.

use std::sync::Mutex;

use pos_core::rbac::Role;
use pos_core::sales::{OrderType, PaymentMethod};
use pos_core::time::Timestamp;
use pos_hwid::HardwareComponents;
use rusqlite::params;

use super::*;
use crate::db::Database;
use crate::repo::catalog::{self, Product, Unit};
use crate::repo::sales::{self, PayloadItem, PayloadPayment, SaleActor, TransactionPayload};
use crate::repo::{shifts, users, Meta};
use crate::sync::engine::{SyncEngine, SyncMode, SyncState};

const CLIENT: Uuid = Uuid::from_u128(0x0199_0000_0000_7000_8000_0000_0000_0c1a);
const CONFIG: &str =
    include_str!("../../../../../../packages/shared/contracts/client-config.example.json");

struct Clockwork(Mutex<Timestamp>);

impl Clock for Clockwork {
    fn now(&self) -> Timestamp {
        *self.0.lock().expect("clock")
    }
}

fn t0() -> Timestamp {
    "2026-09-29T09:00:00.000Z".parse().expect("ts")
}

struct Till {
    db: Arc<Database>,
    device: Uuid,
    engine: SyncEngine,
}

fn credentials() -> SyncCredentials {
    SyncCredentials {
        token: "unused on the shop network".into(),
        device_key: "unused".to_owned().into(),
    }
}

fn till(name: &str) -> Till {
    let hw = HardwareComponents::new("CPU", name, "BOARD", "VOL").expect("hw");
    let db = Arc::new(Database::open_in_memory(&hw.database_key(CLIENT)).expect("db"));
    let device = Uuid::now_v7();
    db.conn()
        .execute(
            "INSERT INTO device (id, created_at, updated_at, name) VALUES (?1, ?2, ?2, ?3)",
            params![device.to_string(), t0().to_string(), name],
        )
        .expect("device");
    let engine = SyncEngine::new(None, Arc::new(Clockwork(Mutex::new(t0()))));
    Till { db, device, engine }
}

impl Till {
    fn sync(&self) -> crate::sync::SyncReport {
        self.engine
            .run(&self.db, Some(&credentials()))
            .expect("round")
    }

    fn count(&self, sql: &str) -> i64 {
        self.db
            .conn()
            .query_row(sql, [], |r| r.get(0))
            .expect("count")
    }
}

fn product(name: &str) -> Product {
    Product {
        meta: Meta::new(t0()),
        name: name.into(),
        name_localized: serde_json::json!({}),
        category_id: None,
        sku: None,
        barcode: None,
        price: 1_500,
        cost: None,
        tax_rate_bps: 0,
        unit: Unit::Each,
        sold_by_weight: false,
        track_stock: true,
        stock_on_hand_milli: 0,
        reorder_threshold_milli: None,
        reorder_quantity_milli: None,
        image_asset: None,
        quick_key_position: None,
        is_active: true,
    }
}

fn fixed(db: &Arc<Database>) -> HubDb {
    let db = Arc::clone(db);
    Arc::new(move || Ok(Arc::clone(&db)))
}

#[test]
fn two_tills_share_the_shop_through_the_hub_without_internet() {
    // The hub already sold on its own before the network was set up.
    let hub = till("HUB");
    let dates = product("Dates");
    catalog::save(&hub.db.conn(), &dates, t0()).expect("product");
    let seeded = super::hub::seed(&hub.db.conn(), hub.device, t0()).expect("seed");
    assert_eq!(seeded, 1, "the product sold before the network existed");
    assert_eq!(
        hub.count("SELECT count(*) FROM sync_queue WHERE sent_at IS NULL"),
        0
    );

    let code = new_pairing_code();
    assert_eq!(code.len(), 9);
    let server = HubServer::start(fixed(&hub.db), CLIENT, code.clone(), "Front till".into(), 0)
        .expect("server");
    hub.engine.set_transport(
        SyncMode::Hub,
        Some(Arc::new(LocalHubTransport { db: fixed(&hub.db) })),
        "",
    );
    assert_eq!(hub.engine.mode(), SyncMode::Hub);

    // A second till joins with the code (typed in lower case, with spaces).
    let address = format!("127.0.0.1:{}", server.port);
    let typed = code.to_lowercase().replace('-', " ");
    let bar = till("BAR");
    let transport = LanTransport::new(&address, CLIENT, &typed);
    let hello = transport.hello().expect("hello");
    assert_eq!(hello.hub_name, "Front till");
    bar.engine
        .set_transport(SyncMode::Lan, Some(Arc::new(transport)), &address);

    // The bar gets the catalogue, then sells.
    bar.sync();
    let pulled = catalog::get(&bar.db.conn(), dates.meta.id)
        .expect("q")
        .expect("dates on the bar");
    assert_eq!(pulled.price, 1_500);
    let hash = users::hash_pin("1234").expect("hash");
    let cashier = users::create(&bar.db.conn(), "Sara", Role::Cashier, hash, t0()).expect("user");
    shifts::open(&bar.db.conn(), bar.device, cashier.meta.id, 0, t0()).expect("shift");
    let sale = TransactionPayload {
        idempotency_key: Uuid::new_v4(),
        customer_id: None,
        order_type: OrderType::Counter,
        table_label: None,
        items: vec![PayloadItem {
            product_id: dates.meta.id,
            quantity_milli: 2000,
            modifier_ids: vec![],
            course: None,
            note: None,
            combo: None,
        }],
        discount_rule_ids: vec![],
        loyalty_points_to_redeem: 0,
        payments: vec![PayloadPayment {
            method: PaymentMethod::Cash,
            tendered_currency: pos_core::currency::CurrencyCode::KWD,
            tendered_amount: 3_000,
            reference: None,
        }],
        notes: None,
    };
    let actor = SaleActor {
        user_id: cashier.meta.id,
        role: Role::Cashier,
    };
    let config = pos_core::config::ClientConfig::parse(CONFIG).expect("config");
    sales::create(&mut bar.db.conn(), &actor, &sale, &config, t0()).expect("sale");
    let report = bar.sync();
    assert!(report.online && report.pushed > 0);

    // The hub sees the bar's sale and its stock movement.
    hub.sync();
    assert_eq!(hub.count("SELECT count(*) FROM transactions"), 1);
    let stock = catalog::get(&hub.db.conn(), dates.meta.id)
        .expect("q")
        .expect("dates");
    assert_eq!(
        stock.stock_on_hand_milli, -2000,
        "the sale's stock movement arrived"
    );

    // A price change on the hub reaches the bar.
    let dearer = Product {
        meta: Meta {
            updated_at: t0().checked_add(chrono::Duration::minutes(5)).expect("ts"),
            ..dates.meta.clone()
        },
        price: 1_750,
        ..stock
    };
    catalog::save(&hub.db.conn(), &dearer, dearer.meta.updated_at).expect("save");
    hub.sync();
    bar.sync();
    assert_eq!(
        catalog::get(&bar.db.conn(), dates.meta.id)
            .expect("q")
            .expect("dates")
            .price,
        1_750
    );

    // A till with the wrong code is refused, and says so.
    let intruder = till("X");
    intruder.engine.set_transport(
        SyncMode::Lan,
        Some(Arc::new(LanTransport::new(&address, CLIENT, "WRONG-CODE"))),
        &address,
    );
    assert!(intruder
        .engine
        .run(&intruder.db, Some(&credentials()))
        .is_err());
    assert_eq!(intruder.engine.status(&intruder.db).state, SyncState::Error);

    // With the hub off (nothing listening), the bar keeps selling and
    // reports it is offline.
    drop(server);
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
    let gone = format!("127.0.0.1:{}", closed.local_addr().expect("addr").port());
    drop(closed);
    bar.engine.set_transport(
        SyncMode::Lan,
        Some(Arc::new(LanTransport::new(&gone, CLIENT, &code))),
        &gone,
    );
    let report = bar.sync();
    assert!(!report.online);
    assert_eq!(bar.engine.status(&bar.db).state, SyncState::Offline);
}

#[test]
fn tills_find_their_hub_by_broadcast() {
    let hub = till("HUB");
    let server = HubServer::start(fixed(&hub.db), CLIENT, new_pairing_code(), "Hub".into(), 0)
        .expect("server");
    let found = discover(CLIENT, server.port);
    assert!(
        found
            .iter()
            .any(|h| h.address.ends_with(&format!(":{}", server.port))),
        "{found:?}"
    );
    // Another business's tills never see it.
    assert!(discover(Uuid::from_u128(9), server.port).is_empty());
    assert!(normalize_address("192.168.1.10", 47800).expect("addr") == "192.168.1.10:47800");
    assert!(normalize_address("", 47800).is_err());
    assert!(same_code("abcd efgh", "ABCD-EFGH"));
    assert!(!same_code("ABCD-EFGX", "ABCD-EFGH"));
}
