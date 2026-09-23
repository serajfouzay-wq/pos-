//! Parity tests: the Rust core must agree with the JSON fixtures that the
//! TypeScript package is also tested against.

use std::collections::{BTreeMap, BTreeSet};

use pos_core::config::ClientConfig;
use pos_core::currency::{CurrencyCode, ExchangeRate};
use pos_core::money::{self, RoundingMode};
use pos_core::rbac::{permissions_for, Permission, Role};
use serde::Deserialize;
use serde_json::Value;

const RBAC: &str = include_str!("../../../packages/shared/contracts/rbac.json");
const CURRENCIES: &str = include_str!("../../../packages/shared/contracts/currencies.json");
const MONEY: &str = include_str!("../../../packages/shared/contracts/money-vectors.json");
const CLIENT_CONFIG: &str =
    include_str!("../../../packages/shared/contracts/client-config.example.json");

#[derive(Deserialize)]
struct RbacContract {
    roles: BTreeMap<String, Vec<Permission>>,
}

#[test]
fn rbac_matches_contract() {
    let contract: RbacContract = serde_json::from_str(RBAC).expect("rbac.json parses");
    assert_eq!(contract.roles.len(), Role::ALL.len());
    for role in Role::ALL {
        let expected: BTreeSet<Permission> =
            contract.roles[role.as_str()].iter().copied().collect();
        let actual: BTreeSet<Permission> = permissions_for(role).into_iter().collect();
        assert_eq!(actual, expected, "role {}", role.as_str());
    }
}

#[test]
fn currencies_match_contract() {
    let contract: BTreeMap<CurrencyCode, BTreeMap<String, u32>> =
        serde_json::from_str(CURRENCIES).expect("currencies.json parses");
    assert_eq!(contract.len(), CurrencyCode::ALL.len());
    for code in CurrencyCode::ALL {
        assert_eq!(
            contract[&code]["exponent"],
            code.exponent(),
            "{}",
            code.as_str()
        );
    }
}

#[test]
fn example_client_config_is_valid() {
    let config = ClientConfig::parse(CLIENT_CONFIG).expect("example config validates");
    assert_eq!(config.client_slug, "dev-demo-cafe");
}

#[test]
fn client_config_rejects_unknown_fields() {
    let mut value: Value = serde_json::from_str(CLIENT_CONFIG).expect("parses");
    value["typo_field"] = Value::Bool(true);
    assert!(ClientConfig::parse(&value.to_string()).is_err());
}

fn mode(v: &Value) -> RoundingMode {
    serde_json::from_value(v["mode"].clone()).expect("rounding mode")
}

fn int(v: &Value, key: &str) -> i64 {
    v[key]
        .as_i64()
        .unwrap_or_else(|| panic!("{key} is an integer"))
}

fn vectors(section: &str) -> Vec<Value> {
    let all: Value = serde_json::from_str(MONEY).expect("money-vectors.json parses");
    all[section]
        .as_array()
        .expect("section is an array")
        .clone()
}

#[test]
fn div_round_vectors() {
    for v in vectors("div_round") {
        let got = money::div_round(i128::from(int(&v, "n")), i128::from(int(&v, "d")), mode(&v));
        assert_eq!(got, Ok(i128::from(int(&v, "expected"))), "{v}");
    }
}

#[test]
fn apply_bps_vectors() {
    for v in vectors("apply_bps") {
        let got = money::apply_basis_points(int(&v, "amount"), int(&v, "bps"), mode(&v));
        assert_eq!(got, Ok(int(&v, "expected")), "{v}");
    }
}

#[test]
fn multiply_by_quantity_vectors() {
    for v in vectors("multiply_by_quantity") {
        let got =
            money::multiply_by_quantity(int(&v, "unit_price"), int(&v, "quantity_milli"), mode(&v));
        assert_eq!(got, Ok(int(&v, "expected")), "{v}");
    }
}

#[test]
fn inclusive_tax_vectors() {
    for v in vectors("extract_inclusive_tax") {
        let split = money::extract_inclusive_tax(int(&v, "gross"), int(&v, "rate_bps"), mode(&v))
            .expect("valid input");
        assert_eq!(
            (split.net, split.tax),
            (int(&v, "net"), int(&v, "tax")),
            "{v}"
        );
        assert_eq!(split.net + split.tax, int(&v, "gross"));
    }
}

#[test]
fn convert_currency_vectors() {
    for v in vectors("convert_currency") {
        let rate = ExchangeRate {
            base: serde_json::from_value(v["base"].clone()).expect("base"),
            quote: serde_json::from_value(v["quote"].clone()).expect("quote"),
            numerator: v["numerator"].as_u64().expect("numerator"),
            denominator: v["denominator"].as_u64().expect("denominator"),
        };
        let got = money::convert_currency(int(&v, "amount"), &rate, mode(&v));
        assert_eq!(got, Ok(int(&v, "expected")), "{v}");
    }
}

#[test]
fn allocate_vectors() {
    for v in vectors("allocate") {
        let weights: Vec<i64> = serde_json::from_value(v["weights"].clone()).expect("weights");
        let expected: Vec<i64> = serde_json::from_value(v["expected"].clone()).expect("expected");
        let got = money::allocate(int(&v, "amount"), &weights).expect("valid input");
        assert_eq!(got, expected, "{v}");
        assert_eq!(got.iter().sum::<i64>(), int(&v, "amount"));
    }
}

#[test]
fn decimal_string_vectors() {
    for v in vectors("to_decimal_string") {
        let currency: CurrencyCode = serde_json::from_value(v["currency"].clone()).expect("code");
        assert_eq!(
            money::to_decimal_string(int(&v, "amount"), currency),
            v["expected"].as_str().expect("string"),
            "{v}"
        );
    }
    for v in vectors("parse_decimal_string") {
        let currency: CurrencyCode = serde_json::from_value(v["currency"].clone()).expect("code");
        let got = money::parse_decimal_string(v["input"].as_str().expect("input"), currency);
        match v["expected"].as_i64() {
            Some(expected) => assert_eq!(got, Ok(expected), "{v}"),
            None => assert!(got.is_err(), "{v} should be rejected"),
        }
    }
}
