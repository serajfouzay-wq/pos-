/**
 * Licensing contract.
 *
 * The generator signs an RS256 JWT with the claims below. The POS binary
 * embeds only the public key and verifies signature, issuer/audience, client,
 * expiry and hardware fingerprint in Rust *before* the database is opened.
 * Any failure halts the till: no data access until it is resolved.
 */
import { z } from 'zod';
import { BusinessTypeSchema } from './business';
import { Sha256HexSchema } from './entities/licensing';
import { NonNegativeIntSchema, PositiveIntSchema, TimestampSchema, UuidSchema } from './primitives';

export const LICENSE_ISSUER = 'pos-factory';
export const LICENSE_AUDIENCE = 'pos-client';
export const ACTIVATION_CODE_PREFIX = 'POSACT1.';

/** JWT payload. Standard claims use JWT naming (seconds since epoch). */
export const LicenseClaimsSchema = z.object({
  iss: z.literal(LICENSE_ISSUER),
  aud: z.literal(LICENSE_AUDIENCE),
  /** Client id. */
  sub: UuidSchema,
  /** Token id, unique per issued device token. */
  jti: UuidSchema,
  iat: PositiveIntSchema,
  nbf: PositiveIntSchema.optional(),
  /** Absent = perpetual. */
  exp: PositiveIntSchema.optional(),
  /** HMAC-SHA256 hardware fingerprint of the activated device. */
  fp: Sha256HexSchema,
  /** SHA-256 of the device's sync key: the sync API requires the matching key. */
  dkh: Sha256HexSchema,
  client_slug: z.string().min(1),
  business_type: BusinessTypeSchema,
  /** Enforced by the cloud across all of the client's activations. */
  max_devices: PositiveIntSchema,
});
export type LicenseClaims = z.infer<typeof LicenseClaimsSchema>;

/** States in which the till halts and shows the lock / activation screen. */
export const HALTING_LICENSE_STATES = [
  'missing',
  'invalid_token',
  'fingerprint_mismatch',
  'expired',
  'revoked',
  'grace_exhausted',
  'hardware_error',
  'storage_error',
] as const;
export const HaltingLicenseStateSchema = z.enum(HALTING_LICENSE_STATES);
export type HaltingLicenseState = z.infer<typeof HaltingLicenseStateSchema>;

/** States from which entering a new token can recover the till. */
export const REACTIVATABLE_LICENSE_STATES: readonly HaltingLicenseState[] = [
  'missing',
  'invalid_token',
  'fingerprint_mismatch',
  'expired',
  'revoked',
  'grace_exhausted',
];

/** Result of `verify_license` / `activate_license`; payload of the `license://status` event. */
export const LicenseStatusSchema = z.discriminatedUnion('state', [
  z.object({
    state: z.literal('valid'),
    /** Token id (`jti`). */
    license_id: UuidSchema,
    client_id: UuidSchema,
    expires_at: TimestampSchema.nullable(),
    last_seen_at: TimestampSchema.nullable(),
    /** The latest cloud validation attempt failed, or none has succeeded yet. */
    offline: z.boolean(),
    /** Whole days of offline trading left; `null` = not enforced (no cloud configured). */
    grace_days_remaining: NonNegativeIntSchema.nullable(),
  }),
  z.object({
    state: HaltingLicenseStateSchema,
    /** Human-readable reason; never includes fingerprint inputs or key material. */
    reason: z.string(),
  }),
]);
export type LicenseStatus = z.infer<typeof LicenseStatusSchema>;

export function isLicenseUsable(
  status: LicenseStatus,
): status is Extract<LicenseStatus, { state: 'valid' }> {
  return status.state === 'valid';
}

/** Result of `get_activation_request`: what a new till shows the operator. */
export const ActivationRequestInfoSchema = z.object({
  /** Paste into the generator. Contains only public data. */
  code: z.string().startsWith(ACTIVATION_CODE_PREFIX),
  client_id: UuidSchema,
  device_name: z.string(),
  fingerprint: Sha256HexSchema,
});
export type ActivationRequestInfo = z.infer<typeof ActivationRequestInfoSchema>;
