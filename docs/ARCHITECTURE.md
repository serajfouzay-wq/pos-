# Architecture notes

Living record of the decisions behind the POS Factory. Each entry states the
decision and why; revisit an entry rather than silently diverging from it.

## Process boundaries

```
┌──────────── pos-client.exe ────────────┐
│  WebView2 (React)                      │
│    └─ ipc.call('create_transaction')   │  ← typed, Zod-validated both ways
│  ────────────── IPC ────────────────── │
│  Rust core                             │
│    ├─ license gate (halts on mismatch) │
│    ├─ rbac::authorize(role, perm)      │
│    ├─ SQLite + SQLCipher (WAL)         │
│    ├─ sync worker ──────────────► Supabase
│    └─ printer / drawer / scanner       │
└────────────────────────────────────────┘
```

The frontend is untrusted UI. Everything with consequences — prices, totals,
permissions, persistence, hardware, network — happens in Rust.

## Decisions

### D1 — One contract, two languages, golden files in between

`@pos/shared` (TS) and `pos-core` (Rust) each keep native definitions: an
exhaustive Rust `match` for RBAC is a better security primitive than parsing
JSON at runtime. Both test suites compare against
`packages/shared/contracts/*.json`, so drift fails CI.

### D2 — IPC commands are deny-by-default per window

`build.rs` declares every command via `AppManifest::commands`, which makes Tauri
generate an `allow-<command>` permission and reject calls not granted by a
capability. When the KDS window arrives (Phase 8) it gets its own capability
with only kitchen commands.

### D3 — Command arguments are snake_case

Rust commands use `#[tauri::command(rename_all = "snake_case")]`, so contract
keys, Rust parameters and database columns share one spelling.

### D4 — `create_transaction` carries no prices

`TransactionPayload` has product ids, quantities, modifier ids, discount rule
ids and tenders only. Rust snapshots `unit_price`/`product_name` from the
catalogue and computes every total. A compromised or buggy UI cannot change
what anything costs.

### D5 — `print_receipt` takes a `transaction_id`, not a `Receipt`

The spec sketch was `print_receipt(receipt: Receipt)`. Accepting a receipt body
from the UI would let it print receipts for sales that never happened (a
classic refund-fraud vector). Rust re-renders from the stored, immutable
transaction instead. `create_transaction` still returns the `Receipt` for display.

### D6 — Transactions are append-only; refunds and voids are new rows

`transactions.kind ∈ {sale, refund, void}` with `original_transaction_id`. No
transaction row is ever updated, which makes sync trivially conflict-free for
sales data. Payments live in `transaction_payments` (one row per tender) so the
Z-report can break down by method.

### D7 — Integers everywhere, including quantities and rates

- Money: `i64` minor units; `i128` intermediates; results must fit JS
  `Number.MAX_SAFE_INTEGER` so they survive IPC.
- Quantities: `quantity_milli` (1000 = 1 unit) for weighed goods.
- Percentages: basis points (10 000 = 100 %).
- FX rates: exact rationals (`numerator/denominator`).
- Rounding is explicit (`half_up`, `half_even`, `toward_zero`).
- Clippy `float_arithmetic = deny` workspace-wide.

### D8 — Sync conflict strategy is a property of the table

`SYNC_ENTITY_STRATEGY` maps each table to `last_write_wins`, `append_only` or
`additive_delta`. LWW compares `(updated_at, event_id)` for a deterministic
tie-break; timestamps are fixed-format UTC with milliseconds so lexical order
equals time order. Stock and loyalty points are sums of delta rows
(`stock_movements`, `loyalty_ledger`), so concurrent offline sales never lose
a decrement.

### D9 — Client config is compiled in

`POS_CLIENT_CONFIG` → validated by `build.rs` → `include_str!` into the binary.
The UI reads it via `app_info`. There is no editable config file on the till
for someone to tamper with.

### D10 — Installer defaults

NSIS, `installMode: currentUser` (no UAC prompt, so the Tauri updater can apply
silent updates), WebView2 via embedded bootstrapper. Single-instance plugin so
two tills never open the same database.

### D11 — Cashier permissions

Cashiers get the spec's sales-only set plus what selling needs: `catalog.view`,
`customer.lookup` (attach a customer to earn points) and `loyalty.redeem`
(a customer entitlement, not a discretionary discount). Manual discounts remain
`discount.apply` (manager+).

## Open items for upcoming phases

- **SQLCipher driver (Phase 2).** `tauri-plugin-sql` is designed around a
  JavaScript API, which our rule forbids the frontend from using. The plan is to
  use the database layer from Rust only and link SQLite with SQLCipher
  (`libsqlite3-sys` `bundled-sqlcipher`); we'll confirm keying works with the
  plugin's sqlx pool or drop to sqlx/rusqlite directly, and record the outcome here.
- **User PIN hashing (Phase 3).** Argon2id; `pin_hash` never crosses IPC
  (`UserSchema` omits it).
