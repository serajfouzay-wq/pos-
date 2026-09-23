# ⚠️ Development license keys — NEVER use in production

This RSA-3072 key pair is committed so that local builds, tests and the
example client work out of the box. Anyone can sign licenses with it.

- `pos-client` release builds **refuse** to embed this public key unless
  `POS_ALLOW_DEV_LICENSE_KEY=1` is set (CI does so for demo installers), and
  the app shows a "development license key" warning when it is embedded.
- Production: create the signing key in the generator (Licenses → Create key),
  export its public key, and build clients with
  `POS_LICENSE_PUBLIC_KEY=/path/to/license-public-key.pem`.
