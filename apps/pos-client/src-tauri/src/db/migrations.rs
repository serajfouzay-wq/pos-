//! Forward-only schema migrations tracked in `PRAGMA user_version`.

use rusqlite::Connection;

use super::DbError;

/// `(version, sql)`, strictly increasing. Never edit a shipped migration —
/// add a new one.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/0001_init.sql")),
    (2, include_str!("migrations/0002_settings_print_queue.sql")),
];

pub const LATEST_VERSION: i64 = MIGRATIONS[MIGRATIONS.len() - 1].0;

pub(super) fn apply(conn: &mut Connection) -> Result<(), DbError> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > LATEST_VERSION {
        return Err(DbError::FromNewerVersion(current));
    }
    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use pos_hwid::HardwareComponents;
    use rusqlite::params;
    use serde::Deserialize;
    use uuid::Uuid;

    use super::super::Database;
    use super::LATEST_VERSION;

    const CONTRACT: &str = include_str!("../../../../../packages/shared/contracts/db-schema.json");

    #[derive(Deserialize)]
    struct Contract {
        tables: BTreeMap<String, TableContract>,
    }

    #[derive(Deserialize)]
    struct TableContract {
        columns: Vec<String>,
        append_only: bool,
    }

    fn db() -> Database {
        let hw = HardwareComponents::new("CPU", "GUID", "BOARD", "VOL").expect("hw");
        Database::open_in_memory(&hw.database_key(Uuid::nil())).expect("opens")
    }

    #[test]
    fn schema_matches_the_shared_contract() {
        let contract: Contract = serde_json::from_str(CONTRACT).expect("contract parses");
        let db = db();
        let conn = db.conn();

        let tables: BTreeSet<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows");
        assert_eq!(tables, contract.tables.keys().cloned().collect());

        let triggers: BTreeSet<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'trigger'")
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows");

        for (table, spec) in &contract.tables {
            let columns: BTreeSet<String> = conn
                .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .expect("prepare")
                .query_map([], |row| row.get(0))
                .expect("query")
                .collect::<Result<_, _>>()
                .expect("rows");
            let expected: BTreeSet<String> = spec.columns.iter().cloned().collect();
            assert_eq!(columns, expected, "columns of {table}");

            assert!(
                triggers.contains(&format!("{table}_no_delete")),
                "{table} lacks no-delete trigger"
            );
            assert_eq!(
                triggers.contains(&format!("{table}_append_only")),
                spec.append_only,
                "append-only trigger of {table}"
            );
        }
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("version");
        assert_eq!(version, LATEST_VERSION);
    }

    const TS: &str = "2026-09-23T10:00:00.000Z";

    fn insert_product(
        conn: &rusqlite::Connection,
        price: rusqlite::types::Value,
    ) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO products (id, created_at, updated_at, name, price, tax_rate_bps, unit,
                                   sold_by_weight, track_stock, is_active)
             VALUES (?1, ?2, ?2, 'Latte', ?3, 0, 'each', 0, 1, 1)",
            params![Uuid::new_v4().to_string(), TS, price],
        )
    }

    #[test]
    fn floats_cannot_be_stored_as_money() {
        let db = db();
        let conn = db.conn();
        assert!(insert_product(&conn, rusqlite::types::Value::Integer(1250)).is_ok());
        assert!(insert_product(&conn, rusqlite::types::Value::Real(12.5)).is_err());
    }

    #[test]
    fn hard_deletes_are_rejected() {
        let db = db();
        let conn = db.conn();
        insert_product(&conn, rusqlite::types::Value::Integer(100)).expect("insert");
        let err = conn
            .execute("DELETE FROM products", [])
            .expect_err("must abort");
        assert!(
            err.to_string().contains("hard deletes are forbidden"),
            "{err}"
        );
        conn.execute("UPDATE products SET deleted_at = ?1", [TS])
            .expect("soft delete allowed");
    }

    #[test]
    fn append_only_tables_reject_updates() {
        let db = db();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO audit_log (id, created_at, updated_at, user_id, role, action, entity_type, device_id, occurred_at)
             VALUES (?1, ?2, ?2, ?1, 'owner', 'settings.manage', 'settings', ?1, ?2)",
            params![Uuid::new_v4().to_string(), TS],
        )
        .expect("insert");
        let err = conn
            .execute("UPDATE audit_log SET action = 'x'", [])
            .expect_err("must abort");
        assert!(err.to_string().contains("append-only"), "{err}");
    }
}
