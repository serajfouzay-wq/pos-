// deno test --allow-read supabase/functions
// HTTP-layer tests of sync-push / sync-pull with a fake database. Tokens are
// signed in-test with the committed DEVELOPMENT key (keys/dev).
import assert from 'node:assert/strict';
import type { Db } from './db.ts';
import { importVerificationKey } from './license.ts';
import { handlePull, handlePush } from './sync.ts';

const root = new URL('../../../', import.meta.url);
const publicPem = await Deno.readTextFile(new URL('keys/dev/license-dev.public.pem', root));
const privatePem = await Deno.readTextFile(new URL('keys/dev/license-dev.private.pem', root));
const verification = await importVerificationKey(publicPem);

function b64url(bytes: Uint8Array | string): string {
  const raw = typeof bytes === 'string' ? new TextEncoder().encode(bytes) : bytes;
  return btoa(String.fromCharCode(...raw))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
}

async function sign(claims: Record<string, unknown>): Promise<string> {
  const der = Uint8Array.from(
    atob(privatePem.replace(/-----[^-]+-----/g, '').replace(/\s+/g, '')),
    (c) => c.charCodeAt(0),
  );
  const key = await crypto.subtle.importKey(
    'pkcs8',
    der,
    { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' },
    false,
    ['sign'],
  );
  const input = `${b64url(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid: verification.kid }))}.${b64url(JSON.stringify(claims))}`;
  const signature = new Uint8Array(
    await crypto.subtle.sign('RSASSA-PKCS1-v1_5', key, new TextEncoder().encode(input)),
  );
  return `${input}.${b64url(signature)}`;
}

const deviceKey = 'ab'.repeat(32);
const dkh = Array.from(
  new Uint8Array(
    await crypto.subtle.digest(
      'SHA-256',
      Uint8Array.from(deviceKey.match(/../g)!, (b) => parseInt(b, 16)),
    ),
  ),
  (b) => b.toString(16).padStart(2, '0'),
).join('');
const CLIENT = '8f14e45f-ceea-467a-9a4e-3b2f1c9d0a11';
const DEVICE = '0192f000-0000-7000-8000-000000000001';
const token = await sign({
  iss: 'pos-factory',
  aud: 'pos-client',
  sub: CLIENT,
  jti: '0192f000-0000-7000-8000-0000000000aa',
  iat: 1_790_000_000,
  fp: '3f'.repeat(32),
  dkh,
  client_slug: 'dev',
  business_type: 'cafe',
  max_devices: 2,
});

interface Call {
  op: string;
  args: unknown[];
}
function fakeDb(fail?: { code: string }): Db & { calls: Call[] } {
  const calls: Call[] = [];
  const maybeFail = () => {
    if (fail) throw Object.assign(new Error('boom'), fail);
  };
  return {
    calls,
    validateDevice: () => Promise.reject(new Error('unused')),
    push: (...args) => {
      calls.push({ op: 'push', args });
      maybeFail();
      return Promise.resolve({ acknowledged: [], rejected: [] });
    },
    pull: (...args) => {
      calls.push({ op: 'pull', args });
      maybeFail();
      return Promise.resolve({ changes: [], next_cursor: '0', has_more: false });
    },
  };
}

function request(
  body: unknown,
  headers: Record<string, string> = { 'x-pos-license': token, 'x-pos-device-key': deviceKey },
) {
  return new Request('http://x/functions/v1/sync', {
    method: 'POST',
    headers,
    body: JSON.stringify(body),
  });
}

const event = {
  event_id: '0192f000-0000-7000-8000-0000000000e1',
  device_id: DEVICE,
  event_type: 'upsert',
  entity_type: 'products',
  entity_id: '0192f000-0000-7000-8000-0000000000f1',
  payload: { id: '0192f000-0000-7000-8000-0000000000f1' },
  occurred_at: '2026-09-24T08:00:00.000Z',
};
const deps = (db: Db) => ({
  db,
  key: () => Promise.resolve(verification),
  now: () => new Date('2026-09-24T00:00:00Z'),
});

Deno.test(
  'push forwards the verified tenant and fingerprint, never client-supplied ones',
  async () => {
    const db = fakeDb();
    const res = await handlePush(
      request({ protocol_version: 1, device_id: DEVICE, events: [event], client_id: 'evil' }),
      deps(db),
    );
    assert.equal(res.status, 200);
    assert.deepEqual(db.calls[0]?.args.slice(0, 3), [CLIENT, '3f'.repeat(32), DEVICE]);
  },
);

Deno.test('the license token alone is not a credential', async () => {
  const db = fakeDb();
  const cases: Record<string, string>[] = [
    { 'x-pos-license': token },
    { 'x-pos-license': token, 'x-pos-device-key': 'cd'.repeat(32) },
    { 'x-pos-device-key': deviceKey },
  ];
  for (const headers of cases) {
    const res = await handlePush(
      request({ protocol_version: 1, device_id: DEVICE, events: [event] }, headers),
      deps(db),
    );
    assert.equal(res.status, 401);
  }
  assert.equal(db.calls.length, 0, 'database never reached');
});

Deno.test('malformed envelopes are rejected before the database', async () => {
  const db = fakeDb();
  const bad = [
    { protocol_version: 2, device_id: DEVICE, events: [event] },
    { protocol_version: 1, device_id: 'nope', events: [event] },
    { protocol_version: 1, device_id: DEVICE, events: [] },
    { protocol_version: 1, device_id: DEVICE, events: [{ ...event, event_type: 'delete' }] },
    { protocol_version: 1, device_id: DEVICE, events: Array(501).fill(event) },
  ];
  for (const body of bad) assert.equal((await handlePush(request(body), deps(db))).status, 400);
  assert.equal(
    (
      await handlePull(
        request({ protocol_version: 1, device_id: DEVICE, cursor: '1; drop', limit: 10 }),
        deps(db),
      )
    ).status,
    400,
  );
  assert.equal(
    (
      await handlePull(
        request({ protocol_version: 1, device_id: DEVICE, cursor: null, limit: 5000 }),
        deps(db),
      )
    ).status,
    400,
  );
  assert.equal(db.calls.length, 0);
});

Deno.test('authorization failures are 403, other database errors 503', async () => {
  const body = { protocol_version: 1, device_id: DEVICE, cursor: null, limit: 10 };
  assert.equal((await handlePull(request(body), deps(fakeDb({ code: '28000' })))).status, 403);
  assert.equal((await handlePull(request(body), deps(fakeDb({ code: '57P01' })))).status, 503);
  assert.equal((await handlePull(request(body), deps(fakeDb()))).status, 200);
});
