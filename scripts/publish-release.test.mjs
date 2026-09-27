import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { findInstaller, publish } from './publish-release.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..');

function workspace({ cloud = true, sig = true } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'pos-release-'));
  const config = JSON.parse(
    readFileSync(join(repo, 'packages/shared/contracts/client-config.example.json'), 'utf8'),
  );
  config.client_slug = 'acme-cafe';
  config.cloud = cloud
    ? { supabase_url: 'https://acme.supabase.co', supabase_anon_key: 'anon' }
    : { supabase_url: null, supabase_anon_key: null };
  mkdirSync(join(root, 'clients/acme-cafe'), { recursive: true });
  writeFileSync(join(root, 'clients/acme-cafe/client.json'), JSON.stringify(config));
  const bundle = join(root, 'bundle');
  mkdirSync(bundle);
  writeFileSync(join(bundle, 'Acme Cafe_0.8.3_x64-setup.exe'), 'installer-bytes');
  if (sig) writeFileSync(join(bundle, 'Acme Cafe_0.8.3_x64-setup.exe.sig'), 'c2lnbmF0dXJl\n');
  writeFileSync(join(bundle, 'Acme Cafe_0.8.2_x64-setup.exe'), 'older');
  return { root, bundle, config };
}

function recorder(status = 200) {
  const calls = [];
  const fetchFn = (url, init) => {
    calls.push({ url, init });
    return Promise.resolve({ ok: status < 300, status, text: () => Promise.resolve('nope') });
  };
  return { calls, fetchFn };
}

test('uploads the installer, then records the release with its signature', async () => {
  const { root, bundle, config } = workspace();
  const { calls, fetchFn } = recorder();
  const result = await publish({
    root,
    slug: 'acme-cafe',
    version: '0.8.3',
    notes: '  Loyalty points  ',
    serviceKey: 'service-key',
    bundleDir: bundle,
    fetchFn,
  });
  const base = config.cloud.supabase_url;
  assert.equal(result.path, `${config.client_id}/0.8.3/Acme Cafe_0.8.3_x64-setup.exe`);
  assert.equal(
    calls[0].url,
    `${base}/storage/v1/object/releases/${config.client_id}/0.8.3/Acme%20Cafe_0.8.3_x64-setup.exe`,
  );
  assert.equal(calls[0].init.headers['x-upsert'], 'true');
  assert.equal(calls[0].init.headers.authorization, 'Bearer service-key');
  assert.equal(String(calls[0].init.body), 'installer-bytes');
  assert.equal(calls[1].url, `${base}/rest/v1/rpc/publish_app_release`);
  assert.deepEqual(JSON.parse(calls[1].init.body), {
    p_client: config.client_id,
    p_version: '0.8.3',
    p_target: 'windows-x86_64',
    p_notes: 'Loyalty points',
    p_path: result.path,
    p_signature: 'c2lnbmF0dXJl',
  });
});

test('refuses to publish what the tills could not verify or reach', async () => {
  const { root, bundle } = workspace({ sig: false });
  const args = { root, slug: 'acme-cafe', version: '0.8.3', serviceKey: 'k', bundleDir: bundle };
  await assert.rejects(publish({ ...args, fetchFn: recorder().fetchFn }), /\.sig is missing/);
  await assert.rejects(publish({ ...args, version: 'x', fetchFn: recorder().fetchFn }), /version/);
  const offline = workspace({ cloud: false });
  await assert.rejects(
    publish({
      ...args,
      root: offline.root,
      bundleDir: offline.bundle,
      fetchFn: recorder().fetchFn,
    }),
    /no cloud/,
  );
  const signed = workspace();
  await assert.rejects(
    publish({
      ...args,
      root: signed.root,
      bundleDir: signed.bundle,
      fetchFn: recorder(500).fetchFn,
    }),
    /upload failed: HTTP 500/,
  );
  assert.throws(() => findInstaller(signed.bundle, '0.9.0'), /found 0/);
});
