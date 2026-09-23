// deno test --allow-read supabase/functions
// Verifies the WebCrypto implementation against the token the Rust issuer
// produced (packages/shared/contracts/license-fixture.json).
import assert from 'node:assert/strict';
import { importVerificationKey, LicenseTokenError, verifyLicenseToken } from './license.ts';

const fixture = JSON.parse(
  await Deno.readTextFile(
    new URL('../../../packages/shared/contracts/license-fixture.json', import.meta.url),
  ),
) as { key_id: string; public_key_pem: string; token: string; claims: Record<string, unknown> };

const NOW = new Date('2026-09-23T00:00:00.000Z');

Deno.test('derives the same key id as Rust', async () => {
  const key = await importVerificationKey(fixture.public_key_pem);
  assert.equal(key.kid, fixture.key_id);
});

Deno.test('accepts the Rust-signed fixture token', async () => {
  const key = await importVerificationKey(fixture.public_key_pem);
  const claims = await verifyLicenseToken(fixture.token, key, NOW);
  assert.deepEqual(claims, fixture.claims);
});

Deno.test('rejects tampering, alg games and expiry', async () => {
  const key = await importVerificationKey(fixture.public_key_pem);
  const [h, p, s] = fixture.token.split('.') as [string, string, string];

  const claims = JSON.parse(atob(p.replace(/-/g, '+').replace(/_/g, '/')));
  claims.max_devices = 999;
  const forged = btoa(JSON.stringify(claims))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
  await assert.rejects(verifyLicenseToken(`${h}.${forged}.${s}`, key, NOW), LicenseTokenError);

  const none = btoa('{"alg":"none"}').replace(/=+$/, '');
  await assert.rejects(verifyLicenseToken(`${none}.${p}.`, key, NOW), /RS256/);

  await assert.rejects(
    verifyLicenseToken(fixture.token, key, new Date('2100-01-02T00:00:00Z')),
    /expired/,
  );
  await assert.rejects(verifyLicenseToken('a.b', key, NOW), LicenseTokenError);
});
