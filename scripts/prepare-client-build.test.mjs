import assert from 'node:assert/strict';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { prepare, productName } from './prepare-client-build.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..');

function workspace({ logo = false, icon = false, slug = 'acme-cafe' } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'pos-build-'));
  mkdirSync(join(root, 'apps/pos-client/src-tauri'), { recursive: true });
  cpSync(
    join(repo, 'apps/pos-client/src-tauri/tauri.conf.json'),
    join(root, 'apps/pos-client/src-tauri/tauri.conf.json'),
  );
  const dir = join(root, 'clients', slug);
  mkdirSync(dir, { recursive: true });
  const config = JSON.parse(
    readFileSync(join(repo, 'packages/shared/contracts/client-config.example.json'), 'utf8'),
  );
  config.client_slug = slug;
  config.display_name = 'مقهى أكمي Acme Café';
  config.receipt.logo_asset = logo ? 'receipt-logo.png' : null;
  writeFileSync(join(dir, 'client.json'), JSON.stringify(config));
  writeFileSync(join(dir, 'license-public-key.pem'), '-----BEGIN PUBLIC KEY-----\nabc\n');
  if (logo) writeFileSync(join(dir, 'receipt-logo.png'), 'png');
  if (icon) writeFileSync(join(dir, 'app-icon.png'), 'png');
  return root;
}

test('product names are file-name safe', () => {
  assert.equal(productName('Acme Café', 'acme'), 'Acme Cafe');
  assert.equal(productName('مقهى', 'acme'), 'POS acme');
  assert.equal(productName('A/B:C*D', 'x'), 'ABCD');
});

test('overrides per client: identity, title, logo resource, icons', () => {
  const root = workspace({ logo: true, icon: true });
  const result = prepare({ root, slug: 'acme-cafe' });
  const written = JSON.parse(readFileSync(result.overridesPath, 'utf8'));
  assert.deepEqual(written, result.overrides);
  assert.equal(written.identifier, 'com.posfactory.pos.acme-cafe');
  assert.equal(written.productName, 'Acme Cafe');
  assert.equal(written.app.windows[0].title, 'مقهى أكمي Acme Café');
  assert.equal(written.app.windows[0].label, 'main', 'rest of the window kept');
  assert.deepEqual(written.bundle.resources, {
    'client-assets/receipt-logo.png': 'client-assets/receipt-logo.png',
  });
  assert.ok(existsSync(join(root, 'apps/pos-client/src-tauri/client-assets/receipt-logo.png')));
  assert.equal(written.bundle.icon.length, 5);
  assert.ok(result.hasIcon);
  assert.match(result.env.POS_CLIENT_CONFIG, /clients[/\\]acme-cafe[/\\]client\.json$/);
});

test('no logo, no icon: nothing extra is bundled', () => {
  const result = prepare({ root: workspace(), slug: 'acme-cafe' });
  assert.deepEqual(result.overrides.bundle.resources, {});
  assert.equal(result.overrides.bundle.icon, undefined);
  assert.equal(result.hasIcon, false);
});

test('refuses bad input', () => {
  const root = workspace();
  assert.throws(() => prepare({ root, slug: '../etc' }), /invalid client slug/);
  assert.throws(() => prepare({ root, slug: 'other' }), /missing/);
  writeFileSync(join(root, 'clients/acme-cafe/license-public-key.pem'), 'nope');
  assert.throws(() => prepare({ root, slug: 'acme-cafe' }), /not a public key/);
});
