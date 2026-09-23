/**
 * RS256 license-token verification with WebCrypto only (no dependencies), so
 * the same code runs in Supabase Edge (Deno) and is testable anywhere.
 *
 * Mirrors `crates/pos-license` (jwt.rs + verify.rs) minus the hardware check,
 * which only the till can perform. Pinned by the shared fixture
 * `packages/shared/contracts/license-fixture.json`.
 */

export const ISSUER = 'pos-factory';
export const AUDIENCE = 'pos-client';
export const MAX_TOKEN_LEN = 8 * 1024;
const CLOCK_SKEW_SECONDS = 300;

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const FINGERPRINT = /^[0-9a-f]{64}$/;

export interface LicenseClaims {
  iss: string;
  aud: string;
  sub: string;
  jti: string;
  iat: number;
  nbf?: number;
  exp?: number;
  fp: string;
  client_slug: string;
  business_type: 'retail' | 'cafe' | 'restaurant';
  max_devices: number;
}

export class LicenseTokenError extends Error {
  override readonly name = 'LicenseTokenError';
}

function b64urlDecode(part: string): Uint8Array<ArrayBuffer> {
  if (!/^[A-Za-z0-9_-]*$/.test(part)) throw new LicenseTokenError('token is not a well-formed JWT');
  const padded = part
    .replace(/-/g, '+')
    .replace(/_/g, '/')
    .padEnd(Math.ceil(part.length / 4) * 4, '=');
  const binary = atob(padded);
  return Uint8Array.from(binary, (c) => c.charCodeAt(0));
}

function pemToDer(pem: string): Uint8Array<ArrayBuffer> {
  const body = pem
    .replace(/-----BEGIN PUBLIC KEY-----/, '')
    .replace(/-----END PUBLIC KEY-----/, '')
    .replace(/\s+/g, '');
  return Uint8Array.from(atob(body), (c) => c.charCodeAt(0));
}

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

export interface VerificationKey {
  key: CryptoKey;
  /** First 8 bytes of SHA-256(SPKI DER), hex — the JWT `kid`. */
  kid: string;
}

export async function importVerificationKey(spkiPem: string): Promise<VerificationKey> {
  const der = pemToDer(spkiPem);
  const key = await crypto.subtle.importKey(
    'spki',
    der,
    { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' },
    false,
    ['verify'],
  );
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', der));
  return { key, kid: toHex(digest.slice(0, 8)) };
}

function isClaims(value: unknown): value is LicenseClaims {
  if (typeof value !== 'object' || value === null) return false;
  const c = value as Record<string, unknown>;
  return (
    typeof c['iss'] === 'string' &&
    typeof c['aud'] === 'string' &&
    typeof c['sub'] === 'string' &&
    UUID.test(c['sub']) &&
    typeof c['jti'] === 'string' &&
    UUID.test(c['jti']) &&
    Number.isSafeInteger(c['iat']) &&
    (c['nbf'] === undefined || Number.isSafeInteger(c['nbf'])) &&
    (c['exp'] === undefined || Number.isSafeInteger(c['exp'])) &&
    typeof c['fp'] === 'string' &&
    FINGERPRINT.test(c['fp']) &&
    typeof c['client_slug'] === 'string' &&
    ['retail', 'cafe', 'restaurant'].includes(c['business_type'] as string) &&
    Number.isSafeInteger(c['max_devices']) &&
    (c['max_devices'] as number) > 0
  );
}

/** Signature, issuer/audience and validity window. Throws {@link LicenseTokenError}. */
export async function verifyLicenseToken(
  token: string,
  verification: VerificationKey,
  now: Date,
): Promise<LicenseClaims> {
  const trimmed = token.trim();
  if (trimmed.length === 0 || trimmed.length > MAX_TOKEN_LEN) {
    throw new LicenseTokenError('token is not a well-formed JWT');
  }
  const parts = trimmed.split('.');
  if (parts.length !== 3) throw new LicenseTokenError('token is not a well-formed JWT');
  const [headerB64, payloadB64, signatureB64] = parts as [string, string, string];

  let header: { alg?: unknown; kid?: unknown };
  try {
    header = JSON.parse(new TextDecoder().decode(b64urlDecode(headerB64)));
  } catch {
    throw new LicenseTokenError('token is not a well-formed JWT');
  }
  if (header.alg !== 'RS256') {
    throw new LicenseTokenError(
      `token uses unsupported algorithm ${JSON.stringify(header.alg)}; only RS256 is accepted`,
    );
  }
  if (header.kid !== undefined && header.kid !== verification.kid) {
    throw new LicenseTokenError(
      `token was signed with a different key (kid ${String(header.kid)})`,
    );
  }

  const ok = await crypto.subtle.verify(
    'RSASSA-PKCS1-v1_5',
    verification.key,
    b64urlDecode(signatureB64),
    new TextEncoder().encode(`${headerB64}.${payloadB64}`),
  );
  if (!ok) throw new LicenseTokenError('token signature is invalid');

  let claims: unknown;
  try {
    claims = JSON.parse(new TextDecoder().decode(b64urlDecode(payloadB64)));
  } catch {
    throw new LicenseTokenError('token claims are invalid');
  }
  if (!isClaims(claims)) throw new LicenseTokenError('token claims are invalid');
  if (claims.iss !== ISSUER || claims.aud !== AUDIENCE) {
    throw new LicenseTokenError('token was not issued for the POS client');
  }
  const nowSeconds = Math.floor(now.getTime() / 1000);
  if ((claims.nbf ?? claims.iat) > nowSeconds + CLOCK_SKEW_SECONDS) {
    throw new LicenseTokenError('token is not valid yet');
  }
  if (claims.exp !== undefined && nowSeconds >= claims.exp) {
    throw new LicenseTokenError('token has expired');
  }
  return claims;
}
