/**
 * Licensing contract.
 *
 * The generator signs an RS256 JWT with the claims below. The POS binary
 * embeds only the public key and verifies the signature, expiry and hardware
 * fingerprint in Rust before any database access. On mismatch it halts.
 */
import { z } from 'zod';
import { BusinessTypeSchema } from './business';
import { Sha256HexSchema } from './entities/licensing';
import { NonNegativeIntSchema, PositiveIntSchema, TimestampSchema, UuidSchema } from './primitives';

export const LICENSE_ISSUER = 'pos-factory';
export const LICENSE_AUDIENCE = 'pos-client';

/** JWT payload. Standard claims use JWT naming (seconds since epoch). */
export const LicenseClaimsSchema = z.object({
  iss: z.literal(LICENSE_ISSUER),
  aud: z.literal(LICENSE_AUDIENCE),
  /** Client id. */
  sub: UuidSchema,
  /** License id. */
  jti: UuidSchema,
  iat: PositiveIntSchema,
  nbf: PositiveIntSchema.optional(),
  /** Absent = perpetual. */
  exp: PositiveIntSchema.optional(),
  /** HMAC-SHA256 hardware fingerprint of the activated device. */
  fp: Sha256HexSchema,
  client_slug: z.string().min(1),
  business_type: BusinessTypeSchema,
  max_devices: PositiveIntSchema,
});
export type LicenseClaims = z.infer<typeof LicenseClaimsSchema>;

/** States in which the app must halt and show the lock screen. */
export const HALTING_LICENSE_STATES = [
  'missing',
  'invalid_signature',
  'fingerprint_mismatch',
  'expired',
  'revoked',
  'grace_exhausted',
] as const;

/** Result of the `verify_license` IPC command. */
export const LicenseStatusSchema = z.discriminatedUnion('state', [
  z.object({
    state: z.literal('valid'),
    license_id: UuidSchema,
    client_id: UuidSchema,
    expires_at: TimestampSchema.nullable(),
    last_seen_at: TimestampSchema.nullable(),
    /** Online check failed but the last success is within the 7-day window. */
    offline: z.boolean(),
    grace_days_remaining: NonNegativeIntSchema,
  }),
  z.object({
    state: z.enum(HALTING_LICENSE_STATES),
    /** Human-readable reason; never includes the fingerprint inputs. */
    reason: z.string(),
  }),
]);
export type LicenseStatus = z.infer<typeof LicenseStatusSchema>;

export function isLicenseUsable(status: LicenseStatus): boolean {
  return status.state === 'valid';
}
