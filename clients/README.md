# Client build inputs

Managed by the **POS Factory generator**. Don't edit these by hand. Change a
client in the generator and start a build; the generator commits the folder
for you.

```
clients/<slug>/
  client.json              ClientConfig (validated by the POS build.rs)
  license-public-key.pem   Public key of the generator's signing key (public by design)
  receipt-logo.png         Optional: printed on receipts (bundled as a resource)
  app-icon.png             Optional: square PNG, 1024×1024 recommended (icons are generated)
```

`.github/workflows/build-client.yml` builds `clients/<slug>/` into a Windows
NSIS installer via `scripts/prepare-client-build.mjs`. The installer is
uploaded as the artifact `pos-<slug>-<build id>`.
