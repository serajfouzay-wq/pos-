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

## Open items for upcoming phases

- **User PIN hashing (Phase 3).** Argon2id; `pin_hash` never crosses IPC
  (`UserSchema` omits it). Data commands obtain the DB only through
  `state.license.database()?`, then `rbac::authorize`.
- **Supabase mirror tables (Phase 4).** Everything in `contracts/db-schema.json`
  except `license`, `device` and `sync_queue`.
- **Delivering tokens (Phase 5).** Today the activation code and token are
  copy-pasted. The generator can push issued tokens to Supabase so tills fetch
  them online, with copy-paste as the offline fallback.
