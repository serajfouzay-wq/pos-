# Cross-language contracts

Golden fixtures shared by the TypeScript (`@pos/shared`) and Rust (`pos-core`)
implementations. Each language keeps its own native definitions — a Rust
`match` for RBAC, a TS `const` for the UI — and **both test suites assert they
equal these files**. If you change a rule, change it here first; CI fails
until both sides agree.

| File                         | Pins                                                     |
| ---------------------------- | -------------------------------------------------------- |
| `rbac.json`                  | Role → permission matrix                                 |
| `currencies.json`            | Supported ISO 4217 codes and minor-unit exponents        |
| `money-vectors.json`         | Rounding, tax, quantity, FX, allocation, parse/format    |
| `client-config.example.json` | A valid per-client build config (both parsers accept it) |
