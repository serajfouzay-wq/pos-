/**
 * Authenticates a till for the sync API.
 *
 * The license token alone is NOT a credential: it is pasted into the till and
 * often travels through chat apps. The request must also carry the device's
 * sync key, whose SHA-256 the generator signed into the token (`dkh`). Only
 * the machine holding the hardware the key derives from can produce it.
 */
import {
  LicenseTokenError,
  verifyLicenseToken,
  type LicenseClaims,
  type VerificationKey,
} from './license.ts';

export class DeviceAuthError extends Error {
  override readonly name = 'DeviceAuthError';
}

function hexToBytes(hex: string): Uint8Array<ArrayBuffer> | null {
  if (!/^[0-9a-f]{64}$/.test(hex)) return null;
  return Uint8Array.from(hex.match(/../g) ?? [], (b) => parseInt(b, 16));
}

function constantTimeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

export async function authenticateDevice(
  request: Request,
  key: VerificationKey,
  now: Date,
): Promise<LicenseClaims> {
  const token = request.headers.get('x-pos-license') ?? '';
  const deviceKey = hexToBytes(request.headers.get('x-pos-device-key') ?? '');
  if (!token || !deviceKey) throw new DeviceAuthError('missing device credentials');

  let claims: LicenseClaims;
  try {
    claims = await verifyLicenseToken(token, key, now);
  } catch (error) {
    if (error instanceof LicenseTokenError) throw new DeviceAuthError(error.message);
    throw error;
  }
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', deviceKey));
  const hash = Array.from(digest, (b) => b.toString(16).padStart(2, '0')).join('');
  if (!constantTimeEqual(hash, claims.dkh))
    throw new DeviceAuthError('device key does not match the license');
  return claims;
}
