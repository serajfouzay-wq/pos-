//! Starter catalogues so a new till can trade immediately. Prices are written
//! in fils (1/1000) and scaled to the client's currency exponent, integer only.

use pos_core::config::BusinessType;
use pos_core::currency::CurrencyCode;
use pos_core::time::Timestamp;
use rusqlite::Connection;

use crate::repo::audit::Actor;
use crate::repo::catalog::{self, Product, StockReason, Unit};
use crate::repo::menu::{self, Combo, ComboItem, DiningTable, Modifier, ModifierGroup, TableShape};
use crate::repo::Meta;

struct Item {
    name: &'static str,
    price_fils: i64,
    barcode: Option<&'static str>,
    unit: Unit,
    stock: Option<i64>,
}

const fn each(name: &'static str, price_fils: i64) -> Item {
    Item {
        name,
        price_fils,
        barcode: None,
        unit: Unit::Each,
        stock: None,
    }
}

const fn stocked(name: &'static str, price_fils: i64, barcode: &'static str, stock: i64) -> Item {
    Item {
        name,
        price_fils,
        barcode: Some(barcode),
        unit: Unit::Each,
        stock: Some(stock),
    }
}

fn catalogue(business: BusinessType) -> Vec<(&'static str, &'static str, Vec<Item>)> {
    match business {
        BusinessType::Cafe => vec![
            (
                "Coffee",
                "#8B5E3C",
                vec![
                    each("Espresso", 900),
                    each("Americano", 1_100),
                    each("Cappuccino", 1_350),
                    each("Flat White", 1_400),
                    each("Spanish Latte", 1_500),
                    each("Iced Latte", 1_500),
                ],
            ),
            (
                "Tea & more",
                "#2F855A",
                vec![
                    each("Karak Tea", 350),
                    each("Green Tea", 800),
                    each("Hot Chocolate", 1_300),
                    each("Fresh Orange", 1_250),
                ],
            ),
            (
                "Bakery",
                "#D69E2E",
                vec![
                    each("Croissant", 750),
                    each("Cheese Croissant", 950),
                    each("Muffin", 850),
                    each("Cheesecake Slice", 1_750),
                    each("Cookie", 450),
                ],
            ),
        ],
        BusinessType::Restaurant => vec![
            (
                "Starters",
                "#C05621",
                vec![
                    each("Hummus", 1_250),
                    each("Fattoush", 1_500),
                    each("Lentil Soup", 1_000),
                    each("Mutabbal", 1_250),
                ],
            ),
            (
                "Mains",
                "#9B2C2C",
                vec![
                    each("Chicken Machboos", 3_500),
                    each("Lamb Kabsa", 4_750),
                    each("Mixed Grill", 5_500),
                    each("Grilled Hammour", 5_250),
                    each("Vegetable Biryani", 3_000),
                ],
            ),
            (
                "Desserts",
                "#6B46C1",
                vec![
                    each("Umm Ali", 1_750),
                    each("Kunafa", 2_000),
                    each("Ice Cream", 1_000),
                ],
            ),
            (
                "Drinks",
                "#2B6CB0",
                vec![
                    each("Water", 250),
                    each("Soft Drink", 500),
                    each("Fresh Juice", 1_250),
                    each("Laban", 400),
                ],
            ),
        ],
        BusinessType::Retail => vec![
            (
                "Groceries",
                "#2F855A",
                vec![
                    stocked("Rice 5kg", 3_250, "6281000000014", 40),
                    stocked("Sugar 2kg", 950, "6281000000021", 60),
                    stocked("Olive Oil 1L", 2_750, "6281000000038", 25),
                    Item {
                        name: "Dates (loose)",
                        price_fils: 2_500,
                        barcode: None,
                        unit: Unit::Kg,
                        stock: None,
                    },
                ],
            ),
            (
                "Drinks",
                "#2B6CB0",
                vec![
                    stocked("Water 1.5L", 150, "6281000000045", 200),
                    stocked("Orange Juice 1L", 750, "6281000000052", 50),
                    stocked("Cola 330ml", 200, "6281000000069", 150),
                ],
            ),
            (
                "Household",
                "#718096",
                vec![
                    stocked("Dish Soap", 850, "6281000000076", 30),
                    stocked("Paper Towels", 1_250, "6281000000083", 30),
                    stocked("Laundry Detergent", 3_500, "6281000000090", 20),
                ],
            ),
        ],
    }
}

/// Converts a fils price to `currency` minor units (round half up).
fn scale(price_fils: i64, currency: CurrencyCode) -> i64 {
    let exponent = currency.exponent();
    match exponent {
        3 => price_fils,
        e if e < 3 => {
            let div = 10_i64.pow(3 - e);
            (price_fils + div / 2) / div
        }
        e => price_fils * 10_i64.pow(e - 3),
    }
}

/// A modifier group: name, min, max, options (name, fils, default?), and the
/// products that ask it.
struct GroupSpec {
    name: &'static str,
    min: i64,
    max: i64,
    options: &'static [(&'static str, i64, bool)],
    products: &'static [&'static str],
}

fn groups(business: BusinessType) -> Vec<GroupSpec> {
    const COFFEE: &[&str] = &[
        "Espresso",
        "Americano",
        "Cappuccino",
        "Flat White",
        "Spanish Latte",
        "Iced Latte",
    ];
    const MILKY: &[&str] = &[
        "Cappuccino",
        "Flat White",
        "Spanish Latte",
        "Iced Latte",
        "Hot Chocolate",
    ];
    match business {
        BusinessType::Cafe => vec![
            GroupSpec {
                name: "Size",
                min: 1,
                max: 1,
                options: &[
                    ("Small", 0, false),
                    ("Medium", 250, true),
                    ("Large", 500, false),
                ],
                products: COFFEE,
            },
            GroupSpec {
                name: "Milk",
                min: 0,
                max: 1,
                options: &[
                    ("Full cream", 0, true),
                    ("Oat", 200, false),
                    ("Almond", 200, false),
                    ("Lactose-free", 150, false),
                ],
                products: MILKY,
            },
            GroupSpec {
                name: "Sugar",
                min: 0,
                max: 1,
                options: &[
                    ("No sugar", 0, false),
                    ("Less sugar", 0, false),
                    ("Normal", 0, false),
                    ("Extra sugar", 0, false),
                ],
                products: &[
                    "Espresso",
                    "Americano",
                    "Cappuccino",
                    "Flat White",
                    "Spanish Latte",
                    "Iced Latte",
                    "Karak Tea",
                ],
            },
            GroupSpec {
                name: "Extras",
                min: 0,
                max: 3,
                options: &[
                    ("Extra shot", 300, false),
                    ("Vanilla syrup", 250, false),
                    ("Caramel syrup", 250, false),
                ],
                products: COFFEE,
            },
        ],
        BusinessType::Restaurant => vec![
            GroupSpec {
                name: "Doneness",
                min: 1,
                max: 1,
                options: &[
                    ("Rare", 0, false),
                    ("Medium rare", 0, false),
                    ("Medium", 0, true),
                    ("Well done", 0, false),
                ],
                products: &["Mixed Grill"],
            },
            GroupSpec {
                name: "Side",
                min: 1,
                max: 1,
                options: &[
                    ("Rice", 0, true),
                    ("Fries", 0, false),
                    ("Salad", 0, false),
                    ("Bread", 0, false),
                ],
                products: &["Mixed Grill", "Grilled Hammour"],
            },
            GroupSpec {
                name: "Spice level",
                min: 0,
                max: 1,
                options: &[("Mild", 0, false), ("Medium", 0, false), ("Hot", 0, false)],
                products: &["Chicken Machboos", "Lamb Kabsa", "Vegetable Biryani"],
            },
        ],
        BusinessType::Retail => vec![],
    }
}

/// Combo name, price in fils, components.
fn combos(business: BusinessType) -> Vec<(&'static str, i64, &'static [&'static str])> {
    match business {
        BusinessType::Cafe => vec![
            ("Breakfast Set", 1_750, &["Cappuccino", "Croissant"]),
            (
                "Afternoon Treat",
                2_750,
                &["Spanish Latte", "Cheesecake Slice"],
            ),
        ],
        BusinessType::Restaurant => vec![(
            "Lunch Set",
            4_250,
            &["Lentil Soup", "Chicken Machboos", "Soft Drink"],
        )],
        BusinessType::Retail => vec![],
    }
}

/// (label, area, seats, shape, x, y) on the 24 × 16 floor grid.
fn tables(business: BusinessType) -> Vec<(String, &'static str, i64, TableShape, i64, i64)> {
    let mut out = Vec::new();
    match business {
        BusinessType::Restaurant => {
            for i in 0..8 {
                out.push((
                    format!("T{}", i + 1),
                    "Main hall",
                    4,
                    TableShape::Square,
                    2 + (i % 4) * 4,
                    2 + (i / 4) * 4,
                ));
            }
            for i in 0..4 {
                out.push((
                    format!("P{}", i + 1),
                    "Terrace",
                    2,
                    TableShape::Round,
                    19 + (i % 2) * 3,
                    2 + (i / 2) * 4,
                ));
            }
            for i in 0..4 {
                out.push((
                    format!("B{}", i + 1),
                    "Bar",
                    1,
                    TableShape::Bar,
                    3 + i * 3,
                    12,
                ));
            }
        }
        BusinessType::Cafe => {
            for i in 0..6 {
                out.push((
                    format!("{}", i + 1),
                    "Seating",
                    2,
                    TableShape::Round,
                    2 + (i % 3) * 4,
                    3 + (i / 3) * 4,
                ));
            }
        }
        BusinessType::Retail => {}
    }
    out
}

fn load_menu(
    conn: &Connection,
    business: BusinessType,
    currency: CurrencyCode,
    products: &[(String, uuid::Uuid)],
    now: Timestamp,
) -> rusqlite::Result<()> {
    let id_of = |name: &str| products.iter().find(|(n, _)| n == name).map(|(_, id)| *id);
    let mut asked: Vec<(uuid::Uuid, Vec<uuid::Uuid>)> = Vec::new();
    for (sort, spec) in groups(business).into_iter().enumerate() {
        let group = ModifierGroup {
            meta: Meta::new(now),
            name: spec.name.to_owned(),
            name_localized: serde_json::json!({}),
            min_select: spec.min,
            max_select: spec.max,
            sort_order: i64::try_from(sort).unwrap_or(0),
            is_active: true,
        };
        let options: Vec<Modifier> = spec
            .options
            .iter()
            .enumerate()
            .map(|(i, (name, fils, default))| Modifier {
                meta: Meta::new(now),
                group_id: group.meta.id,
                name: (*name).to_owned(),
                name_localized: serde_json::json!({}),
                price_delta: scale(*fils, currency),
                is_default: *default,
                sort_order: i64::try_from(i).unwrap_or(0),
                is_active: true,
            })
            .collect();
        menu::save_group(conn, &group, &options, now)?;
        for product in spec.products.iter().filter_map(|p| id_of(p)) {
            match asked.iter_mut().find(|(p, _)| *p == product) {
                Some((_, list)) => list.push(group.meta.id),
                None => asked.push((product, vec![group.meta.id])),
            }
        }
    }
    for (product, group_ids) in asked {
        menu::set_product_groups(conn, product, &group_ids, now)?;
    }
    for (sort, (name, fils, parts)) in combos(business).into_iter().enumerate() {
        let combo = Combo {
            meta: Meta::new(now),
            name: name.to_owned(),
            name_localized: serde_json::json!({}),
            price: scale(fils, currency),
            color: Some("#B7791F".to_owned()),
            sort_order: i64::try_from(sort).unwrap_or(0),
            is_active: true,
        };
        let items: Vec<ComboItem> = parts
            .iter()
            .filter_map(|p| id_of(p))
            .enumerate()
            .map(|(i, product_id)| ComboItem {
                meta: Meta::new(now),
                combo_id: combo.meta.id,
                product_id,
                quantity_milli: 1000,
                sort_order: i64::try_from(i).unwrap_or(0),
            })
            .collect();
        menu::save_combo(conn, &combo, &items, now)?;
    }
    for (label, area, seats, shape, x, y) in tables(business) {
        let table = DiningTable {
            meta: Meta::new(now),
            sort_order: y * 24 + x,
            label,
            area: area.to_owned(),
            seats,
            shape,
            grid_x: x,
            grid_y: y,
            is_active: true,
        };
        menu::save_table(conn, &table, now)?;
    }
    Ok(())
}

/// Inserts the starter catalogue (and, for cafes and restaurants, options,
/// combos and a floor plan). Returns the number of products created.
pub fn load(
    conn: &Connection,
    business: BusinessType,
    currency: CurrencyCode,
    tax_rate_bps: i64,
    actor: &Actor,
    now: Timestamp,
) -> rusqlite::Result<usize> {
    let mut created = 0;
    let mut position = 0;
    let mut products = Vec::new();
    for (sort, (category_name, color, items)) in catalogue(business).into_iter().enumerate() {
        let category = catalog::new_category(
            category_name,
            i64::try_from(sort).unwrap_or(0),
            Some(color),
            now,
        );
        catalog::save_category(conn, &category, now)?;
        for item in items {
            let product = Product {
                meta: Meta::new(now),
                name: item.name.to_owned(),
                name_localized: serde_json::json!({}),
                category_id: Some(category.meta.id),
                sku: None,
                barcode: item.barcode.map(str::to_owned),
                price: scale(item.price_fils, currency),
                cost: None,
                tax_rate_bps,
                unit: item.unit,
                sold_by_weight: item.unit != Unit::Each,
                track_stock: item.stock.is_some(),
                stock_on_hand_milli: 0,
                reorder_threshold_milli: item.stock.map(|_| 5_000),
                reorder_quantity_milli: None,
                image_asset: None,
                quick_key_position: Some(position),
                is_active: true,
            };
            catalog::save(conn, &product, now)?;
            products.push((product.name.clone(), product.meta.id));
            if let Some(stock) = item.stock {
                catalog::move_stock(
                    conn,
                    product.meta.id,
                    stock * 1000,
                    StockReason::StockCount,
                    None,
                    actor,
                    now,
                )?;
            }
            position += 1;
            created += 1;
        }
    }
    load_menu(conn, business, currency, &products, now)?;
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_scale_to_the_currency() {
        assert_eq!(scale(1_250, CurrencyCode::KWD), 1_250);
        assert_eq!(scale(1_250, CurrencyCode::USD), 125);
        assert_eq!(scale(1_255, CurrencyCode::USD), 126);
        assert_eq!(scale(1_250, CurrencyCode::JPY), 1);
    }

    #[test]
    fn every_sample_catalogue_prices_its_combos_and_options() {
        use pos_core::config::ClientConfig;
        use pos_hwid::HardwareComponents;

        use crate::db::Database;
        use crate::repo::sales::{self, ComboRef, PayloadItem};

        let now: Timestamp = "2026-09-24T12:00:00.000Z".parse().expect("ts");
        let config = ClientConfig::parse(include_str!(
            "../../../../packages/shared/contracts/client-config.example.json"
        ))
        .expect("config");
        for business in [
            BusinessType::Retail,
            BusinessType::Cafe,
            BusinessType::Restaurant,
        ] {
            let hw = HardwareComponents::new("CPU", "GUID", "BOARD", "VOL").expect("hw");
            let db = Database::open_in_memory(&hw.database_key(uuid::Uuid::nil())).expect("db");
            let conn = db.conn();
            let actor = Actor {
                user_id: uuid::Uuid::nil(),
                role: pos_core::rbac::Role::Owner,
                device_id: uuid::Uuid::nil(),
            };
            load(&conn, business, CurrencyCode::KWD, 0, &actor, now).expect("load");
            let menu = menu::menu(&conn, false).expect("menu");
            for combo in &menu.combos {
                let instance = uuid::Uuid::now_v7();
                // Each component with the default choice of every group it asks.
                let items: Vec<PayloadItem> = combo
                    .items
                    .iter()
                    .map(|c| PayloadItem {
                        product_id: c.product_id,
                        quantity_milli: c.quantity_milli,
                        modifier_ids: menu
                            .product_modifier_groups
                            .get(&c.product_id)
                            .into_iter()
                            .flatten()
                            .filter_map(|g| {
                                menu.modifier_groups.iter().find(|x| x.group.meta.id == *g)
                            })
                            .flat_map(|g| {
                                g.modifiers
                                    .iter()
                                    .filter(|m| m.is_default)
                                    .map(|m| m.meta.id)
                            })
                            .collect(),
                        course: None,
                        note: None,
                        combo: Some(ComboRef {
                            combo_id: combo.combo.meta.id,
                            instance,
                        }),
                    })
                    .collect();
                let quote = sales::quote(&conn, &items, &[], &config, now).expect("combo prices");
                assert!(quote.total >= combo.combo.price, "{}", combo.combo.name);
                assert!(quote.discount_total > 0, "{} saves money", combo.combo.name);
            }
            let expected_tables = match business {
                BusinessType::Restaurant => 16,
                BusinessType::Cafe => 6,
                BusinessType::Retail => 0,
            };
            assert_eq!(menu.dining_tables.len(), expected_tables);
        }
        for (_, _, items) in catalogue(BusinessType::Retail) {
            for code in items.iter().filter_map(|i| i.barcode) {
                assert_eq!(
                    pos_hardware::label::symbology(code),
                    Some(pos_hardware::label::Symbology::Ean13),
                    "{code} is a valid EAN-13"
                );
            }
        }
    }
}
