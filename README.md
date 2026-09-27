# POS Factory System

A Windows desktop **generator** that produces fully compiled, client-specific,
offline-first **Point of Sale** installers.

```
apps/
  generator/      Master app (your laptop): configure clients, sign licenses, trigger builds
  pos-client/     Generated POS (client site): offline-first till, one build per client
packages/
  shared/         @pos/shared — Zod schemas, types, integer money, RBAC, sync + IPC contracts
    contracts/    JSON golden files both TypeScript and Rust are tested against
crates/
  pos-core/       Rust twin of the domain rules (RBAC enforcement, money, config, time, IPC errors)
  pos-hwid/       Hardware fingerprint (registry + WMI) → license fingerprint & SQLCipher key
  pos-license/    RS256 license tokens: verify, offline grace, activation codes, signing (`issuer`)
  pos-hardware/   ESC/POS encoder, receipt layout, logo raster, TCP / serial / Windows-spooler printers
supabase/
  migrations/     Licensing (client_licenses, device_activations) + sync mirror tables & functions
  functions/      Edge Functions (license-validate, sync-push, sync-pull) — Deno + a local dev router
  tests/          SQL tests for the sync functions (scripts/test-supabase.sh)
clients/          Client build inputs, committed by the generator (one folder per client)
scripts/          prepare-client-build.mjs (CI), Supabase test and E2E runners
keys/dev/         DEVELOPMENT license key pair (never for real customers)
```

**Stack:** Tauri 2 (Rust) · React 18 · Vite · TypeScript (strict) · Zustand ·
React Query · Framer Motion · react-i18next (RTL) · SQLite/SQLCipher · Supabase ·
pnpm workspaces + Turborepo · GitHub Actions.

## Prerequisites

| Tool    | Version                                                                                    |
| ------- | ------------------------------------------------------------------------------------------ |
| Node.js | ≥ 22                                                                                       |
| pnpm    | 10 (`corepack enable`)                                                                     |
| Rust    | stable (via `rustup`; `rust-toolchain.toml` pins the channel)                              |
| Windows | [Tauri prerequisites](https://tauri.app/start/prerequisites/): MSVC Build Tools + WebView2 |

Linux works for development and CI checks (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
`libsoup-3.0-dev`); shipped installers target Windows only.

## Commands

```bash
pnpm install

pnpm dev:pos          # POS client in a Tauri window (Vite on :1420)
pnpm dev:generator    # Generator in a Tauri window (Vite on :1430)

pnpm check            # typecheck + lint + test (all JS/TS packages, via Turborepo)
pnpm rust:check       # cargo fmt --check, clippy -D warnings, cargo test
pnpm format           # prettier

# Windows NSIS installer (.exe) for one app
pnpm --filter @pos/pos-client tauri build
```

### Building a client-specific POS

The client's configuration is embedded at **compile time**. Point
`POS_CLIENT_CONFIG` at a JSON file matching `ClientConfigSchema`
(see `packages/shared/contracts/client-config.example.json`):

```bash
POS_CLIENT_CONFIG=/path/to/acme.json pnpm --filter @pos/pos-client tauri build
```

`build.rs` validates it with the same rules as the Zod schema; an invalid config
fails the build. Without the variable, the example (dev) config is used.
Normally the generator drives all of this (see below).

## The generator: clients → installers

1. **Licenses → Create signing key** (once). Back up the key file and remember
   the passphrase.
2. **Settings → Build repository**: the GitHub repository with this code base,
   plus a fine-grained token for that repository only, with **Contents** and
   **Actions** set to read and write. The token is kept in the Windows
   Credential Manager. Use **Test connection** to check it.
3. **Clients → New client**: name, slug, business type and currency. Then edit:
   - **Details**: languages, extra currencies, tax, features, Supabase
     project, notes.
   - **Receipt & branding**: header and footer, paper width, tax number,
     colours, the receipt logo and the app icon. The live preview is the
     till's own receipt layout, with the logo dithered exactly as printed.
4. **Builds → Build installer**: the generator commits `clients/<slug>/` in
   one commit and runs [`build-client.yml`](.github/workflows/build-client.yml)
   on GitHub Actions. It follows the run and offers **Download installer**
   when it's done (about 15–25 min). Each client gets its own app identity
   (`com.posfactory.pos.<slug>`), name, icon and embedded config. Every
   build of a client gets a higher version (`MAJOR.MINOR` of the app, then
   the client's build number). Add **release notes** and keep **Publish to
   the tills as an update** ticked to ship it to that client's tills (see
   [Updates](#updates)).
5. **Licenses** (on the client, or in the Licenses section): paste the till's
   activation code and sign. A code from another client's till is refused,
   and every license is kept in the client's history.

`build-client.yml` must be on the repository's default branch for dispatch to
work. It can also be run by hand from the Actions tab for any committed
client.

## Licensing

```
 till (unlicensed)                      generator (your laptop)
 ─────────────────                      ───────────────────────
 shows activation code  ──── copy ───►  Licenses → paste code → Sign
 POSACT1.eyJjbGllbnRf…                  (RSA-3072 key, passphrase-unlocked)
 paste license          ◄─── copy ────  eyJhbGciOiJSUzI1NiIs…
 ✓ verified in Rust against the embedded public key + this PC's fingerprint
 ↻ every 6 h: license-validate edge function (seats, revocation, last_seen)
```

- **First-time setup.** In the generator, go to **Licenses → Create signing key**.
  Back up both files in the app-data `keys/` folder. Copy the public key:
  - build clients with `POS_LICENSE_PUBLIC_KEY=/path/to/license-public-key.pem`;
  - `supabase secrets set LICENSE_PUBLIC_KEY_PEM="$(cat license-public-key.pem)"`;
  - then `supabase db push` and `supabase functions deploy license-validate`.
- **Offline grace.** A till trades up to 7 days without reaching the cloud. The
  count starts from the last successful check (or token issuance). Clients built
  with `cloud: null` are fully offline and have no grace limit.
- **Local development.** Debug builds embed the dev key. To activate your own
  dev POS without the generator:
  `cargo run -p pos-license --features issuer --example issue-dev-license -- --this-machine`
  prints a token to paste into the activation screen.
- Local data lives in the app-data dir: `license.jwt` (signed, public) and
  `pos.db` (SQLCipher; the key is derived from this PC's hardware and never stored).

| Build variable                | Purpose                                                          |
| ----------------------------- | ---------------------------------------------------------------- |
| `POS_CLIENT_CONFIG`           | Client config JSON to embed (default: example config)            |
| `POS_LICENSE_PUBLIC_KEY`      | Public key PEM to embed (default: dev key)                       |
| `POS_ALLOW_DEV_LICENSE_KEY=1` | Let a **release** build embed the dev key (throwaway demos only) |

## Cloud sync

Tills work fully offline. Each change is written to the local outbox in the
same SQLite transaction as the change itself.

- **When it syncs.** A background round pushes the outbox and pulls other
  tills' changes:
  - every minute;
  - right after a sale or edit;
  - when the network comes back;
  - when you tap the status-bar pill.
- **Status pill.** It shows the state: `Synced`, `Sync pending`, `Offline`
  (changes wait locally) or `Sync error`. Hover it to see the last sync time.
- **Conflicts.** Sales and stock movements are append-only, so every till's
  sales count. Edits to the same product resolve to the newest (see
  ARCHITECTURE D27–D32).
- **Requirements.** Sync needs `cloud.supabase_url` and
  `cloud.supabase_anon_key` in the client config, plus a license issued
  with this version, since tokens now carry the device-key hash. Deploy with:

  ```
  supabase db push
  supabase secrets set LICENSE_PUBLIC_KEY_PEM="$(cat license-public-key.pem)"
  supabase functions deploy license-validate sync-push sync-pull app-update
  ```

**Local cloud for development** (Postgres ≥ 15 and Deno 2):

```bash
PGHOST=localhost PGUSER=postgres ./scripts/test-supabase.sh   # SQL tests
PGHOST=localhost PGUSER=postgres ./scripts/e2e-sync.sh        # two tills over real HTTP

# Run the edge functions against a local database, then build a POS whose
# config has "supabase_url": "http://127.0.0.1:54321" (loopback http is allowed):
SUPABASE_DB_URL=postgres://postgres@127.0.0.1:5432/pos \
LICENSE_PUBLIC_KEY_PEM="$(cat keys/dev/license-dev.public.pem)" \
DENO_NO_PACKAGE_JSON=1 deno run --no-config -A supabase/functions/dev-server.ts
```

## Updates

Tills update themselves from their shop's cloud project. A published build
is downloaded in the background and checked against the release key compiled
into the till. It installs quietly the next time the till is closed; a
manager can choose **Restart now** from the banner instead. After the update,
the first sign-in shows what changed.

One-time setup:

1. `pnpm --filter @pos/pos-client tauri signer generate -w updater.key` and
   keep the private key safe (losing it means reinstalling every till by hand).
2. In the build repository: variable `POS_UPDATER_PUBLIC_KEY` (the `.pub`
   contents); secrets `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`) and
   `POS_SUPABASE_SERVICE_ROLE_KEY` (uploads to the private `releases`
   bucket).
3. Deploy the `app-update` function (above). Tills built before this key was
   set, or without a cloud, keep updating by installer.

A release can be withdrawn by setting `withdrawn_at` on its `app_releases`
row; tills then get the previous one offered (they never downgrade).

## Using the till

1. **Activate** (see Licensing). 2. The first person creates the **owner**
   (name + 4–6 digit PIN). 3. The owner opens **Products → Start with a sample
   catalogue** (or adds products), **Staff** to add managers/cashiers, and
   **Printer** to choose printers: detected USB/Bluetooth ones, or a network
   printer by IP. Up to 3 are tried in order, and the cash drawer can open
   automatically on cash sales. 4. A manager or the owner **opens the shift**
   with the opening float. 5. Sell: tap products or scan barcodes, **Pay**
   (cash / card / wallet, split allowed), and the receipt prints. If the printer
   is down, receipts queue and print once it's back. 6. **Close shift**: count
   the drawer blind, then see expected cash and variance.

| Role    | Can                                                                                         |
| ------- | ------------------------------------------------------------------------------------------- |
| Cashier | Sell, print the receipt, "No sale" drawer open, find/register customers and spend points    |
| Manager | + open/close shifts, discounts, history and reprints, refunds/voids, X/Z, shifts, updates   |
| Owner   | Everything: products, staff, printers, loyalty rules, customers, kitchen display, audit log |

The top bar shows only what the signed-in role may use: **Sell**, **History**,
**Reports**, **Dashboard**, and the **Back office ▾** menu (Customers,
Products, Menu, Floor, Stock, Staff, Printer, Audit). Rust checks the
permission on every command anyway.

## Selling by business type

The layout follows the `business_type` compiled into the build.

- **Retail.** Opens on the quick-keys grid (Products → _Quick-key slot_). The
  search box takes a scanned or typed barcode on Enter, and the USB scanner
  works anywhere on the screen. Weighed goods ask their weight. **Stock**
  lists on-hand quantities with a low-stock filter. It records deliveries,
  corrections, waste and counts as additive movements, and prints shelf
  labels (EAN-13/EAN-8/UPC-A when the check digit is valid, otherwise
  Code 128). A top-bar chip counts items at or below their alert level.
- **Cafe.** Pay-now at the **Counter**, or **Hold as tab** / **New tab** (a
  name, optionally a table). Products with options (size, milk, sugar…) open
  a picker with the defaults pre-selected; each option's price delta is
  added to the line. **Combos** sell fixed sets at one price, with a choice
  per component. The ✎ on a line adds a kitchen note.
- **Restaurant.** The **floor** shows every table as free, occupied (with
  its running total) or with items not yet sent. Tap a free table to seat it
  (guests), then add items by **course**, and **Send course N** (or _Send
  all_) to the kitchen. With a kitchen printer set up (Printer → Kitchen
  printer) the ticket prints; otherwise it is shown on screen. **Split bill**
  pays chosen lines (combos go whole; _One each_ splits a line of N), or pay
  everything and split the amount equally in the payment dialog. The table
  frees itself when the last line is paid. **Takeaway** is a pay-now sale.

Back office (owner): **Menu** for option groups and combos, **Floor plan**
for placing tables on the 24 × 16 grid, and option groups per product in
**Products**. Tabs and tables are shared between the shop's tills through
sync.

## History, reports and the audit trail

- **History** (manager) lists sales, refunds and voids with date, type and
  receipt/cashier search. Open one to reprint it, or to refund or void it.
  Both print a receipt of their own, and the sale shows what came back.
  - **Refund**: pick quantities per line (weighed goods take a weight), the
    tender (cash, card, wallet) and whether to put the stock back, plus a
    reason. Rust prices the refund from the stored sale, so discounts and tax
    come back pro rata. Card and wallet refunds can't exceed what was paid
    that way.
  - **Void**: reverses the whole sale, tender by tender. Only for a sale on
    this till's open shift that has no refunds yet; later, use a refund.
- **Reports** (manager): the **X report** is a live read of the till since
  its last Z (sales, discounts, refunds, voids, tenders, tax per rate, the
  drawer and each shift's variance). **Run Z** closes the period: it needs
  every shift closed, gets the next Z number for this till, and stores a
  snapshot that can be printed again from the **Z reports** tab. **Shifts**
  lists every shift with expected, counted and variance.
- **Dashboard** (owner): today / yesterday / last 7 / last 30 days for the
  whole shop (every till that has synced) or one till. Net sales and sale
  count compared with the previous period, average ticket, refunds, voids,
  sales by hour and by day, top products, categories, tenders, cashiers,
  order types, and the low-stock count.
- **Audit log** (owner): every sensitive action (refunds, voids, Z runs,
  discounts, shift close, stock and staff changes, sign-ins) with who, when
  and the before/after detail, filterable by action and person.

Times on screen, receipts, kitchen tickets and reports follow the Windows
time zone.

## Customers, points and the kitchen display

- **Loyalty.** In the payment dialog, **+ Customer** finds someone by name or
  phone, or registers them on the spot. With the programme on, the dialog
  shows their balance, what this bill earns, and lets them spend points: Rust
  prices the bill again with the points taken off (an order discount, so tax
  follows it), up to the whole bill. The receipt prints the points earned and
  the balance. Refunds and voids give back the same share of redeemed points
  and take back the share earned. **Customers** (back office) lists everyone
  with their visits, spending and points history; the owner edits the rules
  (points per unit spent, what a point is worth, the smallest redemption, the
  largest share of a bill) and can adjust a balance with a reason. Points
  earned on several tills offline all count (additive sync, like stock).
- **Kitchen display.** In builds with it (cafe or restaurant,
  `features.kitchen_display`), **Printer → Kitchen display → Show** opens a
  second window: put it on a kitchen screen, or use a PC in the kitchen
  running the same app. It reopens whenever the till starts, and needs no
  sign-in (it can only read and bump tickets). Courses sent from a table,
  pay-now sales and changes to food already sent (a red **VOID** ticket)
  appear with a timer that turns amber after 10 minutes and red after 20.
  Tap an item to strike it, **Ready** to bump the ticket, and the chips at
  the bottom recall anything bumped in the last 30 minutes. **All day** sums
  what is still to make. The till that sent the order hears "#12 Table T4 is
  ready". Tickets sync, and a till showing the display pulls every 5 seconds.

## Ground rules

These are enforced by tooling where possible — see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

- **Frontend never touches SQLite, hardware or Supabase.** Only the typed IPC
  client (`src/ipc`). ESLint blocks raw `invoke`, `@tauri-apps/plugin-sql` and
  `@supabase/supabase-js` imports in app code.
- **RBAC is enforced in Rust** (`pos_core::rbac::authorize`) at the top of every
  privileged command. The TS matrix only hides UI.
- **Money is integer minor units** end to end. Clippy denies float arithmetic;
  Zod rejects non-integers; quantities are thousandths (`quantity_milli`).
- **No hard deletes.** Every table has `deleted_at`; transactions are
  append-only (refunds/voids are new rows). SQLite triggers reject `DELETE`
  (and `UPDATE` on append-only tables), and `STRICT` tables reject floats.
- **Sync events are idempotent** (`event_id` is the dedupe key).
- **TypeScript strict, no `any`** (`@typescript-eslint/no-explicit-any: error`).
- Rules that exist in both languages are pinned to `packages/shared/contracts/*.json`.

## Roadmap

| Phase | Scope                                                                   | Status |
| ----- | ----------------------------------------------------------------------- | ------ |
| 1     | Monorepo, shared types/contracts, Tauri 2 shells for both apps          | ✅     |
| 2     | Hardware fingerprint, RS256 licensing, SQLCipher, `verify_license`      | ✅     |
| 3     | Core POS UI: products, cart, payment, receipt print, cash drawer        | ✅     |
| 4     | Offline sync engine: outbox, background worker, conflict resolution     | ✅     |
| 5     | Generator: client dashboard, asset upload, GitHub Actions build trigger | ✅     |
| 6     | Business-type layouts: retail / cafe / restaurant                       | ✅     |
| 7     | Analytics, Z-reports, audit trail, role-based views                     | ✅     |
| 8     | Polish: animations, KDS window, loyalty, auto-updater                   | ✅     |
