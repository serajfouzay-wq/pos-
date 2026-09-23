import { z } from 'zod';
import { EntityBaseSchema, TimestampSchema, UuidSchema } from '../primitives';

/** Lower-case hex SHA-256 / HMAC-SHA256 digest. */
export const Sha256HexSchema = z
  .string()
  .regex(/^[0-9a-f]{64}$/, 'Expected 64 lowercase hex chars');

/**
 * `license` — local history of license tokens installed on this till. The
 * active token also lives in `license.jwt` next to the database, because it
 * must be verified *before* the (fingerprint-keyed) database can be opened.
 * Local-only: never synced (the cloud owns `device_activations`).
 */
export const LicenseRowSchema = EntityBaseSchema.extend({
  /** Token id (`jti`). */
  license_id: UuidSchema,
  client_id: UuidSchema,
  /** RS256 JWT issued by the generator; verified against the embedded public key. */
  token: z.string().min(1),
  fingerprint_hash: Sha256HexSchema,
  issued_at: TimestampSchema,
  /** `null` = perpetual license. */
  expires_at: TimestampSchema.nullable(),
  /** Last successful cloud validation (server time); drives the offline grace window. */
  last_seen_at: TimestampSchema.nullable(),
  /** Set when the cloud reports revocation; persists so going offline cannot undo it. */
  revoked_at: TimestampSchema.nullable(),
  /** Highest clock value ever observed; winding the clock back cannot extend grace. */
  clock_high_water_at: TimestampSchema.nullable(),
});
export type LicenseRow = z.infer<typeof LicenseRowSchema>;

/** `device` — single local row identifying this till. Local-only. */
export const DeviceRowSchema = EntityBaseSchema.extend({
  name: z.string().min(1).max(120),
});
export type DeviceRow = z.infer<typeof DeviceRowSchema>;

/**
 * `device_activations` (cloud) — one row per machine activated for a client.
 * `max_devices` is enforced across a client's non-revoked activations.
 */
export const DeviceActivationSchema = EntityBaseSchema.extend({
  client_id: UuidSchema,
  /** `jti` of the most recent token presented by this device. */
  token_id: UuidSchema,
  fingerprint_hash: Sha256HexSchema,
  device_name: z.string().min(1).max(120),
  activated_at: TimestampSchema,
  last_seen_at: TimestampSchema.nullable(),
  revoked_at: TimestampSchema.nullable(),
});
export type DeviceActivation = z.infer<typeof DeviceActivationSchema>;
