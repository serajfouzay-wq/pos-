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

### D12 — SQLite access is `rusqlite` + SQLCipher, not `tauri-plugin-sql`

The spec named `tauri-plugin-sql`. We use `rusqlite` with
`bundled-sqlcipher-vendored-openssl` (SQLCipher 4.14 / SQLite 3.51) from Rust only:

- `tauri-plugin-sql` exists to give **JavaScript** a SQL API. Our rule is that
  the frontend never touches SQLite; shipping the plugin would put a raw-SQL
  surface one capability typo away.
- Its connection is opened from JS (`Database.load("sqlite:…")`), so the
  SQLCipher key would have to pass **through the webview**. Here the key is
  derived and used entirely inside Rust and never crosses IPC.
- SQLite is single-writer anyway: one `Mutex<Connection>` on a blocking thread
  is simpler and faster than an async pool for a till.

Swapping later is contained to `apps/pos-client/src-tauri/src/db`.

### D13 — Hardware fingerprint: two independent derivations

`pos-hwid` reads CPU brand + `MachineGuid` (registry) + baseboard serial (WMI)

- `C:` volume serial (WMI) — never the MAC — normalises them and encodes them
  length-prefixed (no boundary collisions).

* **License fingerprint** = `HMAC-SHA256(key = client_id, components)`. It is
  public (inside the JWT). Keying by client id makes one PC's fingerprints
  unlinkable across clients.
* **Database key** = `HKDF-SHA256(components, salt = H(client_id))`. It is _not_
  derived from the public fingerprint, so a leaked license file doesn't reveal
  the key. It is zeroized after use and never persisted.

`MachineGuid` is mandatory; other components may be empty (cheap boards often
report no serial). WMI runs on its own thread (COM apartment isolation).

Threat model, stated plainly: this binds data and licenses to a machine and
defeats copying the install to other hardware or pulling the `.db` off to read
it. It does **not** stop an attacker who has the whole disk _and_ reverse-
engineers the binary, because every input is on that disk. If that becomes a
requirement, wrap an extra random secret with Windows DPAPI/TPM and mix it in.

### D14 — License verification happens before the database exists

Order in `license::LicenseService`:

1. hardware → 2. `license.jwt` → 3. RS256 signature, issuer/audience, client
   id, **fingerprint** (`verify_identity`) → 4. only now derive the key and open
   `pos.db` → 5. revocation, validity window, grace.

The token is kept in a plain file beside the database because it must be read
before the (fingerprint-keyed) database can be opened. It is signed, so it is
tamper-evident. Business data is reachable only via `LicenseService::database()`,
which fails closed unless the status is `valid`. The frontend's `LicenseGate`
mirrors this for UX only.

Hardware changes: the old token no longer matches, so the till halts with
`fingerprint_mismatch`. Once the operator issues a new token, the old database
can't be decrypted. It is renamed to `pos.db.orphaned-<ts>` (never deleted)
and a fresh one is created; cloud sync (Phase 4) restores the data.

### D15 — RS256 implemented narrowly, not via a JWT library

`pos-license::jwt` accepts exactly `alg: RS256` and checks `kid` against the
embedded key's id (first 8 bytes of SHA-256(SPKI)). `alg: none` and HS/RS
confusion can't happen by construction. PKCS#1 v1.5 signatures are
deterministic, which makes `contracts/license-fixture.json` a reproducible
golden token. The Rust verifier, the Rust issuer and the edge function's
WebCrypto verifier are all tested against it.

### D16 — Offline grace, and why the clock can't be wound back

- `last_seen_at` only moves forward on a successful cloud check (server time).
- The grace deadline is `max(iat, last_seen_at) + 7 days`. Anchoring on `iat`
  means deleting the local database does **not** reset the window.
- The database keeps `clock_high_water_at`, the highest time ever observed.
  All time checks (grace, `exp`, `nbf`) use `max(wall clock, high-water)`. That
  is why time checks run _after_ the database opens (the regression test
  `expiry_halts_and_cannot_be_dodged_by_winding_the_clock_back` pins this).
  Server time resets the mark, which forgives a clock that ran fast.
- Revocation is persisted locally, so going offline cannot undo it.
- No cloud configured (`cloud: null`) = fully offline deployment; grace is not
  enforced because there is nothing to validate against.
- The till re-evaluates every 15 minutes and validates every 6 h (every 15 min
  while unreachable). Status changes are pushed to the UI as
  `license://status` events.

### D17 — Cloud validation contract

`POST /functions/v1/license-validate` with `{ token, fingerprint, device_name,
app_version }`. The function verifies the signature (WebCrypto), then calls
`validate_device_activation` (plpgsql, advisory lock per client) to:

- enforce `max_devices` across the client's non-revoked activations;
- honour client-level and device-level revocation;
- record `last_seen_at`.

Seat limits only move with a _newer_ token (`limits_issued_at`), so replaying an
old token can't raise them. A database failure returns 5xx, which the till
treats as "unreachable" and never as "revoked". Tables have RLS with no
policies; only the service role touches them.

### D18 — Signing key custody (generator)

RSA-3072, stored only as encrypted PKCS#8 (scrypt + AES-256-CBC), unlocked per
session by passphrase (≥ 12 chars), never overwritten. Signing uses blinding
(`RandomizedSigner`) as a mitigation for RUSTSEC-2023-0071 ("Marvin"), which
needs an attacker timing many private-key operations. That doesn't fit a
local, operator-driven signer, but the note stays here until `rsa` ships a
constant-time release.

### D19 — The development key is committed, and fenced off

`keys/dev/` holds a documented development key pair so builds and tests work
out of the box. `build.rs` recognises it by key id. Release builds refuse to
embed it unless `POS_ALLOW_DEV_LICENSE_KEY=1` (used for CI demo installers),
and the app shows a red "development license key" banner whenever it's
embedded.

### D20 — Every business command passes one guard, in this order

`commands::authorize(state, permission)`: **license** (`license.database()`
— no valid license, no database handle) → **session** (signed in) →
**`rbac::authorize(role, permission)`**. Extra permissions are checked inline
where they depend on the request (`discount.apply` when discounts are sent,
`receipt.reprint` when a receipt was already printed). The only commands
without a role check are app info, licensing and sign-in, and those still sit
behind the license gate wherever they touch data.

### D21 — PIN sign-in: Argon2id, lockout, tap-then-PIN

Users are tapped on a tile, then enter a 4–6 digit PIN on the numpad (no
keyboard). PINs are hashed with Argon2id (19 MiB, t = 2). Five consecutive
misses lock that user for 5 minutes. Selecting the user first, rather than
searching every hash for a matching PIN, keeps login O(1) and lets PINs
repeat across staff. The first person to open a new till creates the owner
(`bootstrap_owner`, refused once any user exists). Sessions live in memory
only: restarting the app signs everyone out. `pin_hash` syncs (Phase 4) so
staff can sign in on any till, but it never crosses IPC.

### D22 — The UI never totals money

The cart asks Rust for a quote (`quote_transaction`) on every change;
`create_transaction` runs the same `pos_core::pricing::price` on catalogue
prices read inside the write transaction. Discounts: line-scoped first
(capped at the line), then order-scoped, allocated by largest remainder so
per-line tax stays exact. Tenders (`pos_core::tender::settle`): card/wallet
cannot exceed what is owed, change comes only from cash, and the applied
amounts always sum to the total. The payment dialog's running "remaining /
change" is guidance; Rust re-validates.

### D23 — A sale is one SQLite transaction

Transaction row, lines (with price/name snapshots), payments, additive stock
movements for tracked items, outbox events for every synced row, the audit
entry and the receipt print job all commit together or not at all. The
idempotency key makes a retried `create_transaction` return the original sale.
Receipt numbers are `<first 4 hex of device id>-<per-device sequence>`, which
is unique without coordination.

### D24 — Printing: transports, fallback chain, offline queue

- **USB** goes through the Windows spooler in RAW mode, which works with the
  driver Windows installs. The spec said `hidapi`, but thermal printers are
  USB _printer_ class, not HID, so `hidapi` can't reach them without swapping
  the driver (Zadig). That isn't acceptable for a zero-terminal install.
  **Bluetooth SPP** and USB-serial appear as COM ports on Windows.
  **Network** printers use TCP/9100.
- Settings hold an ordered chain of up to 3 printers (primary + fallbacks).
  Discovery lists spooler printers and COM ports (Bluetooth labelled).
- Every receipt is a `print_jobs` row, rendered from the stored transaction
  at print time and printed oldest-first. The queue drains after each sale,
  on printer-settings save and every 30 s. A failure stops the drain, so
  order is kept. The DB lock is never held during printer I/O.
- The drawer kick (`1B 70 00 19 19`) is sent right after a cash sale commits
  (if enabled), or on "No sale" (`drawer.kick`, audited). It is **never
  queued**: a drawer popping open later, unattended, is worse than an error.
- Text-mode ESC/POS uses code page WPC1252. **Arabic text cannot print in
  this mode** (it becomes `?`). See open items.

### D25 — The database enforces the invariants too

`STRICT` tables reject floats in money columns. Triggers abort `DELETE` on
every table and `UPDATE` on append-only tables. One open shift per device is
a partial unique index. These back up the Rust code; they don't replace it.

### D26 — Scanner detection

A capture-phase `keydown` listener feeds a pure `ScanDetector`. Keys < 100 ms
apart are a scanner burst; a gap of ≥ 100 ms restarts the buffer from that
key. Enter completes the scan (≥ 4 chars), and the buffer also clears on
Enter or a 300 ms timeout. While focus is in a text field the keys belong to
the field (a barcode input simply receives the scan).

## Open items for upcoming phases

- **Arabic receipts (next).** Text-mode ESC/POS cannot render Arabic.
  The plan is to rasterise the receipt layout (shaping + bidi, a bundled
  OFL font) through the existing `GS v 0` path, which logos already use.
- **Refunds and voids.** The permissions and the append-only model are ready
  (`kind = refund | void` + `original_transaction_id`); the flow and UI are
  still to build.
- **Discount rules UI, customers and loyalty, multi-currency tenders.** The
  engine supports discount rules (tested); creating them needs back-office
  UI. Customer and loyalty payloads and foreign-currency tenders are
  rejected with a clear message until their phases.
- **Supabase mirror tables (Phase 4).** Everything in `contracts/db-schema.json`
  except `license`, `device`, `sync_queue`, `settings` and `print_jobs`. The
  server derives `products.stock_on_hand_milli` from `stock_movements` and
  ignores the device's cached value.
- **Delivering tokens (Phase 5).** Today the activation code and token are
  copy-pasted. The generator can push issued tokens to Supabase so tills fetch
  them online, with copy-paste as the offline fallback.
