#!/usr/bin/env node
/**
 * Publishes a client build as an update for its tills (Phase 8):
 *
 * 1. uploads the signed NSIS installer from `target/release/bundle/nsis/`
 *    to the private `releases` bucket of the client's Supabase project, at
 *    `<client_id>/<version>/<file>`;
 * 2. records it with `publish_app_release` (service role), which makes the
 *    `app-update` edge function offer it to the client's activated tills.
 *
 * The signature (`<installer>.sig`, written by `tauri build` when
 * TAURI_SIGNING_PRIVATE_KEY is set) is what the tills check against the
 * public key compiled into them; the upload location is not trusted.
 *
 * Usage: node scripts/publish-release.mjs <slug>
 * Environment: POS_CLIENT_VERSION, POS_SUPABASE_SERVICE_ROLE_KEY, and
 * optionally POS_RELEASE_NOTES and POS_BUNDLE_DIR.
 */
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const VERSION = /^\d{1,5}\.\d{1,5}\.\d{1,9}$/;
export const TARGET = 'windows-x86_64';
const MAX_NOTES = 4000;

/** The installer of `version` and its signature in `bundleDir`. */
export function findInstaller(bundleDir, version) {
  if (!existsSync(bundleDir)) throw new Error(`no bundle directory ${bundleDir}`);
  const installers = readdirSync(bundleDir).filter(
    (f) => f.endsWith('-setup.exe') && f.includes(`_${version}_`),
  );
  if (installers.length !== 1) {
    throw new Error(
      `expected one ${version} installer in ${bundleDir}, found ${installers.length}`,
    );
  }
  const file = join(bundleDir, installers[0]);
  if (!existsSync(`${file}.sig`)) {
    throw new Error(`${basename(file)}.sig is missing: build with TAURI_SIGNING_PRIVATE_KEY set`);
  }
  return { file, signature: readFileSync(`${file}.sig`, 'utf8').trim() };
}

export async function publish({ root, slug, version, notes, serviceKey, bundleDir, fetchFn }) {
  if (!SLUG.test(slug) || slug.length > 40) throw new Error(`invalid client slug: ${slug}`);
  if (!VERSION.test(version ?? '')) throw new Error(`invalid version ${String(version)}`);
  if (!serviceKey) throw new Error('POS_SUPABASE_SERVICE_ROLE_KEY is not set');
  const config = JSON.parse(readFileSync(join(root, 'clients', slug, 'client.json'), 'utf8'));
  const base = config.cloud?.supabase_url;
  if (!base) throw new Error(`${slug} has no cloud: its tills cannot receive updates`);
  const { file, signature } = findInstaller(bundleDir, version);
  const name = basename(file);
  const path = `${config.client_id}/${version}/${name}`;
  const headers = { authorization: `Bearer ${serviceKey}`, apikey: serviceKey };

  const upload = await fetchFn(
    `${base}/storage/v1/object/releases/${path.split('/').map(encodeURIComponent).join('/')}`,
    {
      method: 'POST',
      headers: { ...headers, 'content-type': 'application/octet-stream', 'x-upsert': 'true' },
      body: readFileSync(file),
    },
  );
  if (!upload.ok) throw new Error(`upload failed: HTTP ${upload.status} ${await upload.text()}`);

  const record = await fetchFn(`${base}/rest/v1/rpc/publish_app_release`, {
    method: 'POST',
    headers: { ...headers, 'content-type': 'application/json' },
    body: JSON.stringify({
      p_client: config.client_id,
      p_version: version,
      p_target: TARGET,
      p_notes: (notes ?? '').trim().slice(0, MAX_NOTES),
      p_path: path,
      p_signature: signature,
    }),
  });
  if (!record.ok) throw new Error(`recording failed: HTTP ${record.status} ${await record.text()}`);
  return { path, version, client_id: config.client_id };
}

async function main() {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
  const result = await publish({
    root,
    slug: process.argv[2] ?? '',
    version: process.env.POS_CLIENT_VERSION,
    notes: process.env.POS_RELEASE_NOTES ?? '',
    serviceKey: process.env.POS_SUPABASE_SERVICE_ROLE_KEY,
    bundleDir: process.env.POS_BUNDLE_DIR ?? join(root, 'target', 'release', 'bundle', 'nsis'),
    fetchFn: fetch,
  });
  console.log(`published ${result.version} for ${result.client_id} (${result.path})`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(`::error::${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  });
}
