//! Starter catalogues so a new till can trade immediately. Prices are written
//! in fils (1/1000) and scaled to the client's currency exponent, integer only.

use pos_core::config::BusinessType;
use pos_core::currency::CurrencyCode;
use pos_core::time::Timestamp;
use rusqlite::Connection;

use crate::repo::audit::Actor;
use crate::repo::catalog::{self, Product, StockReason, Unit};
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
                    stocked("Rice 5kg", 3_250, "6281000000017", 40),
                    stocked("Sugar 2kg", 950, "6281000000024", 60),
                    stocked("Olive Oil 1L", 2_750, "6281000000031", 25),
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
                    stocked("Water 1.5L", 150, "6281000000048", 200),
                    stocked("Orange Juice 1L", 750, "6281000000055", 50),
                    stocked("Cola 330ml", 200, "6281000000062", 150),
                ],
            ),
            (
                "Household",
                "#718096",
                vec![
                    stocked("Dish Soap", 850, "6281000000079", 30),
                    stocked("Paper Towels", 1_250, "6281000000086", 30),
                    stocked("Laundry Detergent", 3_500, "6281000000093", 20),
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

/// Inserts the starter catalogue. Returns the number of products created.
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
}
