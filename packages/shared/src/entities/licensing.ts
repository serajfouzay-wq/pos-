import { z } from 'zod';
import { EntityBaseSchema, TimestampSchema, UuidSchema } from '../primitives';

/** Lower-case hex SHA-256 / HMAC-SHA256 digest. */
export const Sha256HexSchema = z.string().regex(/^[0-9a-f]{64}$/, 'Expected 64 hex chars');

/**
 * `license` — single local row holding the device's signed license token.
 * Local-only: never synced (the cloud owns `device_activations`).
 */
export const LicenseRowSchema = EntityBaseSchema.extend({
  license_id: UuidSchema,
  client_id: UuidSchema,
  /** RS256 JWT issued by the generator; verified against the embedded public key. */
  token: z.string().min(1),
  fingerprint_hash: Sha256HexSchema,
  issued_at: TimestampSchema,
  /** `null` = perpetual license. */
  expires_at: TimestampSchema.nullable(),
  /** Last successful cloud validation; drives the 7-day offline grace window. */
  last_seen_at: TimestampSchema.nullable(),
});
export type LicenseRow = z.infer<typeof LicenseRowSchema>;

/** `device_activations` — one row per machine bound to a license. */
export const DeviceActivationSchema = EntityBaseSchema.extend({
  license_id: UuidSchema,
  fingerprint_hash: Sha256HexSchema,
  device_name: z.string().min(1).max(120),
  activated_at: TimestampSchema,
  last_seen_at: TimestampSchema.nullable(),
  revoked_at: TimestampSchema.nullable(),
});
export type DeviceActivation = z.infer<typeof DeviceActivationSchema>;
