/**
 * POST /functions/v1/license-validate
 *
 * Body:     { token, fingerprint, device_name, app_version }
 * Response: { status: 'active' | 'revoked' | 'device_limit' | 'rejected',
 *             server_time: '2026-09-23T10:15:30.123Z', reason?: string }
 *
 * Answers "is this signed token for this device still in good standing?" and
 * records `last_seen_at` (the till's 7-day offline grace is measured from it).
 */
import type { Db } from './db.ts';
import {
  LicenseTokenError,
  MAX_TOKEN_LEN,
  verifyLicenseToken,
  type VerificationKey,
} from './license.ts';

type Status = 'active' | 'revoked' | 'device_limit' | 'rejected';

export interface ValidateDeps {
  db: Db;
  key: () => Promise<VerificationKey>;
  now?: () => Date;
}

function reply(status: Status, reason?: string, serverTime = new Date()): Response {
  return Response.json({ status, server_time: serverTime.toISOString(), reason: reason ?? null });
}

interface Body {
  token: string;
  fingerprint: string;
  device_name: string;
  app_version: string;
}

function parseBody(value: unknown): Body | null {
  if (typeof value !== 'object' || value === null) return null;
  const b = value as Record<string, unknown>;
  const ok =
    typeof b['token'] === 'string' &&
    b['token'].length <= MAX_TOKEN_LEN &&
    typeof b['fingerprint'] === 'string' &&
    /^[0-9a-f]{64}$/.test(b['fingerprint']) &&
    typeof b['device_name'] === 'string' &&
    b['device_name'].length >= 1 &&
    b['device_name'].length <= 120 &&
    typeof b['app_version'] === 'string' &&
    b['app_version'].length <= 32;
  return ok ? (b as unknown as Body) : null;
}

export async function handleValidate(request: Request, deps: ValidateDeps): Promise<Response> {
  if (request.method !== 'POST') return new Response('method not allowed', { status: 405 });
  const body = parseBody(await request.json().catch(() => null));
  if (!body) return new Response('invalid request body', { status: 400 });

  let claims;
  try {
    claims = await verifyLicenseToken(body.token, await deps.key(), deps.now?.() ?? new Date());
  } catch (error) {
    if (error instanceof LicenseTokenError) return reply('rejected', error.message);
    throw error;
  }
  if (claims.fp !== body.fingerprint)
    return reply('rejected', 'the token does not belong to this device');

  try {
    const row = await deps.db.validateDevice({
      clientId: claims.sub,
      clientSlug: claims.client_slug,
      maxDevices: claims.max_devices,
      tokenIssuedAt: new Date(claims.iat * 1000).toISOString(),
      tokenId: claims.jti,
      fingerprint: claims.fp,
      deviceName: body.device_name,
    });
    return reply(row.status as Status, row.reason ?? undefined, new Date(row.server_time));
  } catch (error) {
    // A database failure must look like "unreachable" to the till (5xx),
    // never like a revocation.
    console.error(error);
    return new Response('validation unavailable', { status: 503 });
  }
}
