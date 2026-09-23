/**
 * POST /functions/v1/license-validate
 *
 * Body:     { token, fingerprint, device_name, app_version }
 * Response: { status: 'active' | 'revoked' | 'device_limit' | 'rejected',
 *             server_time: '2026-09-23T10:15:30.123Z', reason?: string }
 *
 * Tills call this periodically. It never issues licenses; it only answers
 * "is this signed token for this device still in good standing?" and records
 * `last_seen_at`, which is what the till's 7-day offline grace is measured from.
 *
 * Secrets (supabase secrets set …):
 *   LICENSE_PUBLIC_KEY_PEM   generator's public key (Licenses → public key)
 *   SUPABASE_URL / SUPABASE_SERVICE_ROLE_KEY are provided by the platform.
 */
import { createClient } from '@supabase/supabase-js';
import {
  importVerificationKey,
  LicenseTokenError,
  MAX_TOKEN_LEN,
  verifyLicenseToken,
  type VerificationKey,
} from '../_shared/license.ts';

type Status = 'active' | 'revoked' | 'device_limit' | 'rejected';

const publicKeyPem = Deno.env.get('LICENSE_PUBLIC_KEY_PEM');
const supabase = createClient(
  Deno.env.get('SUPABASE_URL') ?? '',
  Deno.env.get('SUPABASE_SERVICE_ROLE_KEY') ?? '',
  { auth: { persistSession: false } },
);
let verificationKey: Promise<VerificationKey> | undefined;

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

Deno.serve(async (request) => {
  if (request.method !== 'POST') return new Response('method not allowed', { status: 405 });
  if (!publicKeyPem)
    return new Response('LICENSE_PUBLIC_KEY_PEM is not configured', { status: 500 });

  const body = parseBody(await request.json().catch(() => null));
  if (!body) return new Response('invalid request body', { status: 400 });

  verificationKey ??= importVerificationKey(publicKeyPem);
  let claims;
  try {
    claims = await verifyLicenseToken(body.token, await verificationKey, new Date());
  } catch (error) {
    if (error instanceof LicenseTokenError) return reply('rejected', error.message);
    throw error;
  }
  if (claims.fp !== body.fingerprint) {
    return reply('rejected', 'the token does not belong to this device');
  }

  const { data, error } = await supabase
    .rpc('validate_device_activation', {
      p_client_id: claims.sub,
      p_client_slug: claims.client_slug,
      p_max_devices: claims.max_devices,
      p_token_issued_at: new Date(claims.iat * 1000).toISOString(),
      p_token_id: claims.jti,
      p_fingerprint: claims.fp,
      p_device_name: body.device_name,
    })
    .single<{ status: Status; server_time: string; reason: string | null }>();
  // A database failure must look like "unreachable" to the till (5xx), never
  // like a revocation.
  if (error || !data) return new Response('validation unavailable', { status: 503 });

  return reply(data.status, data.reason ?? undefined, new Date(data.server_time));
});
