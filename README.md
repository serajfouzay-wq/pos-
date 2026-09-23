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
supabase/
  migrations/     Cloud tables (client_licenses, device_activations) + validation function
  functions/      Edge Functions (license-validate) — Deno, WebCrypto
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
fails the build. Without the variable, the example (dev) config is used. In
Phase 5 the generator drives this through a GitHub Actions workflow.

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
| 2     | Hardware fingerprint, RS256 licensing, SQLCipher, `verify_license`      |        |
| 3     | Core POS UI: products, cart, payment, receipt print, cash drawer        |        |
| 4     | Offline sync engine: outbox, background worker, conflict resolution     |        |
| 5     | Generator: client dashboard, asset upload, GitHub Actions build trigger |        |
| 6     | Business-type layouts: retail / cafe / restaurant                       |        |
| 7     | Analytics, Z-reports, audit trail, role-based views                     |        |
| 8     | Polish: animations, KDS window, loyalty, auto-updater                   |        |
