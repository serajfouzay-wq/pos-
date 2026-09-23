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

1. **Publish.** "Build installer" commits `clients/<slug>/` (`client.json`,
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
- **Outbox retention.** Acknowledged `sync_queue` rows are kept (no hard
  deletes). A later phase can compact old sent rows into an archive table,
  or soft-delete them, once the Z-report period is closed.
- **Cloud back office.** The mirror tables are ready for dashboards (Phase 7)
  but are service-role only. Reads for owners need their own RLS policies and
  an auth story.
- **Delivering tokens.** The activation code and token are still
  copy-pasted. The generator could push issued tokens to Supabase so tills
  fetch them online, with copy-paste as the offline fallback.
- **Code signing.** Installers are unsigned. `build-client.yml` is the place
  to add Authenticode signing from repository secrets, so SmartScreen
  doesn't warn.
- **Versions per client.** Builds use the repository's app version; the
  auto-updater (Phase 8) brings per-client release channels.
