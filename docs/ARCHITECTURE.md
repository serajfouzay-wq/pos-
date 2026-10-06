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

### D27 — Sync rounds: outbox in, pages out

A round pushes, then pulls. It runs every 60 s, right after any change (the
commands nudge the worker), when the webview reports `online`, and from the
status-bar "Sync now".

- **Push.** Pending `sync_queue` rows go out in outbox order, 500 per request,
  and `event_id` is the server's dedupe key. The row is marked `sent_at` only
  on an explicit acknowledgement, so a lost response simply replays.
- **Rejections.** A retryable rejection backs off `min(2^n · 30 s, 1 h)`. A
  permanent one (SQLSTATE 22xxx/23xxx: bad data) is **parked**: it gets
  `deleted_at` and `last_error`, stays on disk for diagnosis, and never blocks
  the queue. The status shows the parked count.
- **Pull.** Pages are applied after a server cursor. Each page commits in one
  SQLite transaction with the cursor advance, and re-applying a page is
  harmless.
- **Pulled rows bypass the repo layer.** They produce no outbox event, so
  nothing echoes back. One bad row is skipped, reported, and doesn't wedge
  the shop.
- **Locking.** The database lock is never held across a network call. An
  offline round is a cheap no-op, and changes wait in the outbox.

### D28 — A license token is not a sync credential

Tokens travel through chat apps and email, so possession must not grant access
to a shop's data. Activation now binds a **device key** as well:
`HKDF(hardware components, client_id, "pos-factory/device-key/v1")`, which is
independent of both the fingerprint and the database key.

- The activation code carries `sha256(device key)`, and the signed token
  pins it as the `dkh` claim.
- Every sync request sends the token (`x-pos-license`) and the raw key
  (`x-pos-device-key`). The edge function verifies the signature, then
  checks `sha256(key) == dkh` in constant time.
- The till also re-checks `dkh` locally, so a token can never unlock a
  different machine.

A stolen token without the hardware gets `401`, and the live E2E asserts
this. Revocation and seat checks come from `device_activations`
(`28000` → `403`).

### D29 — Server: mirror tables, one global sequence, no foreign keys

Every synced table has a Postgres mirror, generated from the same schema.
Each mirror has the same columns, plus `client_id`, `server_seq`,
`origin_device_id`, `last_event_id` and `received_at`.

- **Applying events.** `sync_push` applies events through one generic
  plpgsql function:
  - LWW is `ON CONFLICT … DO UPDATE WHERE (updated_at, last_event_id) <
(excluded…)`, guarded by `client_id`;
  - appends are `DO NOTHING`;
  - `sync_received_events` dedupes.
- **Sequence ordering.** A per-client advisory lock makes `server_seq`
  values commit in order, so a pull cursor can never skip a row that commits
  late.
- **Pulls.** `sync_pull` excludes the caller's own versions (by origin), but
  its cursor moves past them.
- **No foreign keys.** There are none server-side: rows arrive in any order
  from many tills.
- **Direct Postgres.** Edge functions talk to Postgres directly
  (`SUPABASE_DB_URL`, `npm:postgres`) through a small `Db` interface. This
  lets `supabase/functions/dev-server.ts` serve the real handlers locally for
  E2E.

### D30 — Both sides pick the same LWW winner

A pull carries the server version's `event_id`. The till keeps its own row
only if it holds a **pending** edit whose `(updated_at, event_id)` beats the
incoming pair: the same tuple comparison the server makes. When that edit is
pushed, the server reaches the same verdict, so tills converge regardless of
push order. The tests cover both orders, equal timestamps, and pending edits.

### D31 — Aggregates are derived on both sides

`products.stock_on_hand_milli` and `customers.loyalty_points` are never
taken from a synced row (`DERIVED_COLUMNS`).

- **Server.** Triggers maintain them from `stock_movements` and
  `loyalty_ledger`.
- **Till.** It recomputes them from its local deltas when a product or
  customer arrives, and adds each newly inserted delta.

Concurrent offline sales on two tills therefore always sum correctly.

### D32 — Reinstalls and second tills

- **Reinstalls.** A different `device_id` on an existing activation means
  that till's database was recreated (the device key already proved it's
  the same hardware). The activation is rebound, and the new database
  bootstraps everything, including rows the old one pushed.
- **Second tills.** On a new till of an existing shop, `session_status`
  runs one sync round before offering owner setup, so staff sign in with
  their existing PINs. Argon2id hashes sync, and so do lockouts.

### D33 — The generator's workspace is a local SQLite database

`generator.db` (plain SQLite, same conventions as the till) holds:

- clients, each with its full `ClientConfig`;
- uploaded images, as blobs with their SHA-256;
- every signed license;
- the build history.

It is not encrypted: it holds settings and public material only. The two
secrets live elsewhere:

- **The signing key** is an encrypted PKCS#8 file (D18).
- **The GitHub token** lives in Windows Credential Manager (macOS Keychain).
  Linux is dev-only and its kernel keyring is unreliable in sessions and
  containers, so there it is a 0600 file.

**Fixed identity.** A client's `client_id` and slug never change after
creation. The slug names the installer, the app identifier
(`com.posfactory.pos.<slug>`) and so the tills' data folder.
`receipt.logo_asset` is derived from the uploaded logo and never taken from
the form. Archiving is a soft delete that frees the slug.

### D34 — Builds are GitOps: one commit, one dispatch, one run

1. **Publish.** "Build installers" commits `clients/<slug>/` (`client.json`,
   the public key, the logo, the icon) to the build repository in a single
   commit through the Git Data API:
   - blobs → tree with deletions → commit;
   - a fast-forward-only ref update, retried when someone pushed in between;
   - the commit message carries `[skip ci]` so the regular CI doesn't run;
   - an unchanged folder makes no commit.
2. **Dispatch.** The generator dispatches `build-client.yml` with `client`
   and `build_id`.
3. **Follow the run.** The workflow's `run-name` contains the build id, so the
   generator finds the run without guessing by timestamp. It polls while a
   build is in flight and records the run URL and the artifact
   `pos-<slug>-<build id>`. It can download the zip for you.

The repository then records exactly what each build was made from, and anyone
can rebuild a client from the Actions tab.

In the workflow:

- Inputs reach shell steps only through the environment and are
  re-validated.
- `scripts/prepare-client-build.mjs` turns the folder into Tauri config
  overrides:
  - product name and per-client identifier;
  - window title;
  - the logo as a bundled resource;
  - icons generated by `tauri icon`.
- It also sets `POS_CLIENT_CONFIG` / `POS_LICENSE_PUBLIC_KEY`, so `build.rs`
  validates the config again and refuses the development key in a release
  build.

### D35 — A license is issued for a client, not for a typed slug

The Issue form takes a client, and the slug and business type come from its
record. The activation code carries the client id of the build it came from.
A code from a till of another client is refused, so a license can't be
minted for the wrong shop by a copy-paste slip. Every signed license is
recorded under its client, with device name, fingerprint, seats, expiry and
the token, which can be copied again later.

### D36 — The receipt preview is the till's own code

`preview_receipt` prices a sample sale with `pos_core::pricing` under the
draft tax settings. It renders the result with
`pos_hardware::receipt::render_text` through
`ReceiptTemplate::for_client`, the same constructor the till uses. The logo
goes through the same PNG → 1-bit dither conversion the printer receives.
What the operator sees is what prints, including the column width for 58 or
80 mm paper.

### D37 — A combo is a group target, not a product

Combo components are sold as ordinary lines. They keep their own stock,
kitchen routing, tax rate and receipt line, and are tagged with
`combo: {combo_id, instance}`. Pricing gains a `DiscountScope::Group` with
`DiscountValue::Target(price)`. The group's remaining total is brought down
to the combo price (plus option surcharges) and the difference is allocated
across the lines by largest remainder. It never raises a price, and it runs
after line discounts and before order discounts, so tax is still computed
per line on what was actually charged. Rust accepts a combo only when its
lines match the combo's components exactly.

### D38 — Options are validated and snapshotted by Rust

`price_cart` checks every chosen modifier: it must be live, belong to one of
the product's groups, and keep each group's count within `min..=max`.
`unit_price = product price + Σ price_delta`. The sale stores the chosen
options (name + delta) as JSON on `transaction_items`. The receipt and
later reports then show what was sold even after the menu changes.

### D39 — Open orders: optimistic versions locally, LWW between tills

A tab or table is one `open_orders` row whose items live in a JSON array.
On the till every change carries the `expected_updated_at` it was based on.
A stale edit is refused and the UI reloads, and the order editor sends its
changes one at a time, each against the version the previous save returned.
Between tills the row syncs last-write-wins like any mutable entity. Only one
open order may sit at a table. Changing or removing an item already sent to
the kitchen needs `sale.void` and is audited as `sale.void`.

### D40 — Courses and kitchen tickets

Items carry an optional course (1–9). `fire_course` stamps `fired_at` on the
unsent items of one course (or all) and returns a `KitchenTicket`. It prints
on the kitchen printer when one is configured (`PrinterSettings.kitchen`).
The rendered text is always returned, so a shop without a kitchen printer
reads it off the screen. The KDS window (Phase 8) will consume the same
tickets.

### D41 — Split bills are several sales against one order

`pay_open_order` takes the line ids to pay (null = all). It creates an
ordinary transaction for them (same pricing, same idempotency key rules) and
removes them from the order in the same database transaction. It appends
the sale to `transaction_ids`. Combos can only be paid whole. `split_order_line` turns a
line of N whole units into N lines so guests can pay single items. When no
lines remain the order is `settled` and the table is free.

### D42 — Stock changes are movements, never overwrites

Receiving, corrections, waste and counts all write `stock_movements` deltas.
A count stores `counted − on hand` as its delta. On-hand stays a sum of
deltas, so two tills adjusting stock offline still converge (the additive
strategy of D8). Every adjustment is also written to the audit log.

### D43 — One generic row helper for LWW tables

`repo::rows::{upsert, select}` serialise a `#[derive(Serialize,
Deserialize)]` struct straight to its columns plus the outbox event
(`json_text` and `int_bool` adapters for JSON and boolean columns). The
seven Phase 6 tables use it instead of hand-written SQL per table. Adding a
synced table is now a migration, a struct, and a line in the sync entity
list on both sides.

### D44 — The layout is chosen by the build, not by a setting

`SellScreen` switches on `app_info.client.business_type`, which is compiled
in from the generator's client config. There is no runtime toggle, so a
retail till never shows tabs or a floor plan. The pieces are shared: product
grid, cart panel, payment, option picker. Only the composition differs
(`QuickSale`, `CafeSell`, `RestaurantSell`). Plural forms missing from a
locale (Arabic: two, few, many…) reuse that locale's `_other` text rather
than falling back to English.

### D45 — Refunds and voids are signed rows that point at the sale

A refund or void is a new transaction (`kind = refund | void`,
`original_transaction_id`). Its header totals and payments are negative, and
its item rows carry positive quantities and amounts, with the original
`line_number`. Every report multiplies item figures by the transaction's sign
(`CASE WHEN kind = 'sale' THEN 1 ELSE -1 END`), so one query sums sales net
of what came back. Reversals find their sale line by `line_number`, so the
refundable quantity is sold − already reversed. Nothing on the sale changes.

### D46 — Pro-rata amounts are cumulative shares

Refunding `q` of a line sold as `Q` returns
`share(amount, before + q) − share(amount, before)` of each amount
(line total, discount, tax), where `before` is what was already reversed and
`share` rounds half up in i128. Refunding a line in several goes therefore
adds up exactly to the line, with no stray fils. Card and wallet refunds are
capped at what that method still holds (paid − already refunded that way).
Cash is not capped, because it is how a shop settles any refund.
`quote_refund` runs the same plan without writing, and the UI shows the
figure from it instead of doing money arithmetic.

### D47 — Voids only on the open shift

A void cancels a sale as if it never happened. It is allowed only for a
sale on this till's open shift that has no reversals, and it reverses each
tender by its own method. Anything older is a refund. This keeps closed
shifts, and the Z reports built from them, stable.

### D48 — Z reports: per till, numbered, snapshotted, append-only

`run_z_report` requires this till's shifts to be closed. The period runs
from the previous Z's `period_end + 1 ms` (or from the till's first
transaction or shift) to now. The numbers come from the same engine as the
X report. The row stores the totals as columns and the full report as a
`report` JSON snapshot, so a reprint shows exactly what was closed even
after later syncs. `z_number` is per device (`UNIQUE(device_id, z_number)`),
and `grand_total` is the previous grand total plus this period's net sales.
The table is append-only on both sides (triggers) and syncs with the
append-only strategy. Running a Z is audited as `report.z_run`. A Z with
no sales is allowed, so a till can close a quiet day.

### D49 — One reports engine over half-open windows

`reports::totals(conn, window, device)` computes counts, gross, discounts,
refunds, voids, net, tax per rate and tenders for `[from, to)`. X, Z and
the dashboard all call it. The dashboard adds hour/day buckets, top
products, categories, cashiers and order types, and the same totals for the
previous window of equal length. It is limited to 400 days. The dashboard
reads the local database, which holds every till's synced rows, so the
shop-wide view works offline with whatever has arrived.

### D50 — Local time comes from the OS

`pos_core::time::Zone` is `Utc`, `System` or `Fixed(minutes)`. The till
prints receipts, kitchen tickets and reports with `System`, and buckets the
dashboard's hours and days in it. The frontend computes ranges from local
midnight. Stored timestamps stay UTC. The generator's receipt preview uses
UTC and says so. Tests pin `Fixed(180)`, so they don't depend on the
machine's zone.

### D51 — Role-based views from the session

The navigation is built from `session.permissions`, which Rust computes
from the role matrix. History needs `receipt.reprint`, Reports
`report.view`, Dashboard `analytics.view`, and Audit `audit.view`. The
back-office screens sit in one menu. Hiding a button is a convenience only:
every Phase 7 command starts with `authorize` on the same permission.

### D52 — Points are an order discount, applied last

Redeeming points adds an order-scoped `Fixed` discount with the fixed id
`shop::LOYALTY_ID`, priced after every other discount by the same engine,
so tax, rounding and allocation to lines work as for any discount. The UI
sends only a customer id and a point count. Rust checks them against the
programme (`loyalty::max_redeemable`: never more than the balance, never
more than `max_redeem_bps` of the payable, nothing below
`min_redeem_points`) and returns the money value. Points earned are
`floor(total × points_per_unit / 10^exp)`, computed after redemption. A bill
fully paid by points has a total of 0 and completes with one cash tender of
0; zero-amount payment rows are not written. Redeeming needs
`loyalty.redeem`.

### D53 — The points ledger is additive; balances are a cache

`loyalty_ledger` is append-only (`earn`, `redeem`, `adjust`,
`refund_reversal`) and syncs with the append strategy, like stock
movements. `customers.loyalty_points` is a cached sum that `add_points`
moves in the same transaction as the ledger row. Points earned on two tills
offline both count. The price is that two tills can redeem the same points
offline, so a balance can go negative. It is shown as such and the next
purchases pay it back. Receipts print `balance_at(customer, issued_at)`,
which keeps reprints stable.

### D54 — Refunds and voids return points by subtotal share

A reversal takes back earned points and returns redeemed points in
proportion to the gross subtotal it reverses (cumulative, so several
partial refunds never exceed the sale's points). The share uses the
subtotal, not the total, because a sale fully paid by points has a total
of 0 and must still return them. On a refund or void row,
`loyalty_points_earned` holds the points taken back and
`loyalty_points_redeemed` the points returned; the receipt prints them as
"Points taken back" and "Points returned".

### D55 — Shop settings are synced rows with fixed ids

`shop_settings` holds shop-wide JSON values by key and syncs
last-write-wins. Each key has a fixed row id (loyalty is
`0199a000-0000-7000-8000-000000000001`), so two tills that save the setting
offline update one row instead of creating two. Without a row the programme
uses `LoyaltySettings::default_for(currency)` (one point per currency unit,
a point worth 1/100 of it). The programme also needs `features.loyalty` in
the build. Changing it needs `settings.manage`. Looking up and registering
customers needs `customer.lookup` (every role); editing, deleting and
adjusting points needs `customer.manage` (owner). Adjustments are audited.

### D56 — Kitchen tickets are rows, written when food is sent

With `features.kitchen_display` in a non-retail build, `kitchen_tickets`
rows are written at the same points the kitchen printer prints: firing a
course, paying an order with unsent lines, and a pay-now sale. Changing or
cancelling already-sent items, and voiding a sale that reached the kitchen,
writes a `void` ticket, so the kitchen sees what to stop. Items carry
names, options and notes as sent (a snapshot, like a printed ticket).
Tickets sync last-write-wins, so any till's display can bump them. A
ticket is `open` or `ready`; per-item `done_at` is the cook's strike.

### D57 — The kitchen display is a second window with its own capability

The display is the `kds` window (`index.html?window=kds`), opened from the
back office. `capabilities/kitchen.json` grants that window only the board
commands and fullscreen. Board commands accept the `kds` window without a
session, so the display keeps working while the till is signed out; any
other window needs `sale.create`. The device setting
`kitchen_display.enabled` reopens it at start. While it is open, sync runs
every 5 s instead of the normal interval, so tickets from other tills
arrive quickly. `KitchenHub` emits `kitchen://changed` on every local
change, and the main window shows a toast when a ticket is marked ready.

### D58 — Updates are signed, offered per client, installed on exit

`tauri-plugin-updater` is registered only when the build embeds
`POS_UPDATER_PUBLIC_KEY`. Tills ask the client's own Supabase project
through the `app-update` edge function, which checks the till's license and
device key and asks `app_update_check` for a newer release
(`app_releases`, compared as semver integers). It answers with a signed,
short-lived storage URL, or 204. The installer's minisign signature is
checked against the embedded key, so the download location is not trusted.
The till checks in the background and downloads the update. It then
installs it when the app exits (`installMode: quiet`), or at once with
"Restart now", which needs `shift.close`. After the restart the till shows
"Updated from X to Y" with the release notes, once.

### D59 — Each client has its own version series

The generator gives every build a version: the app's MAJOR.MINOR and a
build number per client (`next_client_version`), so each client's releases
always increase. The build workflow passes the version and notes. (Signing
and publishing moved from the workflow to the generator in Phase 9, see
D65.)

### D60 — Motion is decoration, and it follows the OS setting

Framer Motion animates page changes, ticket lines, the receipt check, the
kitchen board and the update banner. Page changes take 160 ms and the rest
are short springs. Nothing waits for an animation to finish before taking
input. `MotionConfig reducedMotion="user"` turns movement off when the OS
asks for reduced motion.

### D61 — Offline is the normal case

The target shops rarely have internet and lose power often. So no feature
may need the cloud: selling, reports, loyalty, memberships, discounts,
printing, backups and updates all work offline. The cloud is optional, per
client. `cloud.offline_grace_days` is `null` (never enforced) by default, so
a till that cannot reach the cloud is never locked out. A client can be
given a limit in the generator.

### D62 — Backups are whole encrypted databases, restored by staging

A backup is `sqlcipher_export` into a new file, with `user_version` copied
(export drops it), plus a JSON description. It runs at start, on a timer,
at shift close and after a Z report, and can be copied to a second folder.
With a backup password the file is keyed with Argon2id(password), so it
opens on another PC. Otherwise it uses this machine's key. Restoring checks
the file on a copy, re-keys it for this machine into a staged file, and
restarts. The staged file is swapped in before the database opens, and the
old database is kept aside. `quick_check` at start reports damage. Deleting
old backups is file cleanup, not deleting records.

### D63 — The shop network is the cloud protocol on a till

One till can be the hub. It stores every synced row in `hub_rows` (with a
change number `seq`) and the events it applied in `hub_events`. It answers
the same push and pull as the cloud, with the same rules: insert-once
events, last-write-wins on `(updated_at, event_id)`, append-only and delta
rows inserted once, derived columns zeroed, and a till never pulling its
own versions. The other tills swap the cloud transport for `LanTransport`
(HTTP on port 47800, UDP discovery on 47801). The hub uses
`LocalHubTransport`, so its own changes follow the same path. Each target
has its own pull cursor. Every request carries the client id and a pairing
code. A till that already has data seeds an empty hub. A till syncs with
one target at a time: the cloud or the hub.

### D64 — Receipts print as text or as an image, chosen per job

The receipt, kitchen, report and label layouts build a `Doc` (lines with
alignment, size and weight). `PrintMode::Auto` prints it as ESC/POS text
when every character is in the printer's code page. Otherwise it rasterises
the doc and prints it as `GS v 0` bands of 128 rows: rustybuzz shaping,
unicode-bidi with each line's direction taken from its first strong
letter, ab_glyph drawing, and the bundled Tajawal font (OFL). Kitchen
tickets go through their own queue (`kitchen_print_jobs`), like receipts,
so a kitchen printer that is off loses nothing.

### D65 — The generator holds the update key; updates can travel on USB

GitHub never holds a secret. The generator makes a minisign key with the
first build and keeps it in the OS credential store. Every build commits
its public half (`updater-public-key.txt`), which becomes
`POS_UPDATER_PUBLIC_KEY`. After downloading a build, the generator signs
each installer. The trusted comment names the client, version, target and
file. It then writes `.posupdate` zips (a manifest plus the installer). The
till trusts only the signature: it checks the signed client, target, file
and version against its own, and installs only a newer version. It backs up
first, then runs the NSIS setup (`/P /UPDATE /R`) or swaps the AppImage,
and exits. The same signature serves the online channel. The generator
uploads to the client's project with a per-client service key from the
credential store. `build-client.yml` builds Windows and Linux into one
artifact and needs no secrets.

### D66 — Discount rules are scheduled, priced in Rust

`discount_rules` gained `apply_mode` (automatic or manual), `days_mask`
(Monday = bit 0) and a local `time_from`/`time_to` window, which may cross
midnight. The engine takes every automatic rule running at the sale's local
time, plus the manual rules the manager picked (`discount.apply`). It
applies them before points, and records each on the receipt. The back
office shows when a rule runs with a TypeScript copy of the schedule check,
which has its own tests.

### D67 — A membership plan is a product

A plan owns a product in the "Memberships" category. So selling, refunds,
reports and printing need nothing new. Selling it to a named customer
starts a period or adds one after the current period. A refund or void
cancels the periods that sale bought. An active member gets the plan's
percentage as an order discount, before points, and a points multiplier.
Plans and memberships sync last-write-wins. Card numbers are EAN-13 with
the in-store prefix 29, so any scanner reads them.

### D68 — Linux is a shipped platform

Client builds make an AppImage and a .deb (Ubuntu 22.04 glibc) next to the
Windows setup. Printing on Linux goes to raw devices (`/dev/usb/lp*`,
serial) or CUPS queues (`lp -o raw`). The online channel has a
`linux-x86_64` target. The generator itself is released for both systems
by `release-generator.yml`.

### D69 — Nothing at the shop needs the internet

The shops are set up from a USB stick and may never be online. Both
installers carry the WebView2 offline installer (`offlineInstaller`, about
130 MB more): the setup installs it only where it is missing, which is
often the case on older Windows 10 PCs. The binaries link the C runtime
statically (tauri-build), so no Visual C++ package is needed. Fonts are
system fonts on screen, and Tajawal is built in for printing. The till's
online paths (license check, updates, sync) run only when a cloud is
configured, and the offline grace period is off unless the generator sets
it. Only the generator's builds need the internet (GitHub Actions). The
installer download gets three hours, not the two minutes an API call gets,
so slow connections finish. While GitHub is out of reach, the build list
shows what is saved and says so; nothing else in the generator is online.

### D70 — Activation by files on the USB stick

The activation code and the license are long strings. To spare copying
them by hand, the till saves its code as `<till>.posactivate` (save
dialog). The generator opens that file into the license form, then saves
the signed license as `<till>.poslicense` under
`<Downloads>/POS Factory/<slug>/licenses/`, next to the installers. The
till's activation screen keeps looking for `.poslicense` files on USB
sticks and in Downloads, with the update-file search (D65), and activates
with the one pressed. It reads only `.poslicense` files no bigger than a
token. Pasting text still works.

## Open items for upcoming phases

- **Multi-currency tenders.** Foreign-currency tenders are rejected with a
  clear message.
- **Hub failover.** If the hub PC dies, another till can become the hub
  (it seeds from its own copy), but tills must be pointed at it by hand.
- **Update key rotation.** A lost update key means reinstalling every till.
  A signed "trust this new key" message would let tills move to a new key.
- **Points expiry.** The ledger has an `expire` reason, but nothing writes
  it yet. Expiry needs a rule (for example, points unused for 12 months)
  and a job that runs on one till only, so points don't expire twice.
- **Negative balances.** Offline double redemption is allowed and shows as
  a negative balance (D53). A shop that wants a hard limit needs an online
  check before redeeming.
- **Outbox retention.** Acknowledged `sync_queue` rows are kept (no hard
  deletes). A later phase can compact old sent rows into an archive table,
  or soft-delete them, once the Z-report period is closed.
- **Cloud back office.** The dashboard runs on the till, over synced data.
  An owner view in the browser needs RLS policies on the mirror tables
  (service-role only today) and an auth story.
- **Exporting reports.** X/Z and the dashboard are on screen and on paper.
  CSV/PDF export for the accountant is still to add.
- **History detail.** The detail panel shows lines, options and totals but
  not the line notes sent to the kitchen.
- **Delivering tokens online.** Tokens go by USB file or copy-paste
  (D70). The generator could also push issued tokens to Supabase so tills
  with internet fetch them.
- **Code signing.** Installers are unsigned, so SmartScreen warns once.
  Authenticode signing could be done by the generator after download, like
  update signing, with the certificate kept on the operator's PC.
- **Kitchen-only PCs.** A PC that only runs the display still shows the
  main window at the login screen. A device setting could start it
  display-only.
- **Ready notices across tills.** The "ready" toast shows on the till
  running the display. Other tills see the ticket change after sync but
  don't raise a toast yet.
- **Staged rollouts.** A published release goes to every till of the
  client at once. A percentage or per-till rollout would need a column on
  `app_releases`.
- **Floor plan overlap.** Tables can be placed overlapping on the grid; the
  editor could refuse overlapping cells.
- **Moving and merging tables.** An order can change table through
  `update_open_order`, but the floor has no drag-to-move or merge yet.
