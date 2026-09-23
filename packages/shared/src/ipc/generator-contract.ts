/**
 * IPC contract of the generator app (`apps/generator/src-tauri`).
 * Client management, asset upload and build triggering arrive in Phase 5.
 */
import { z } from 'zod';
import { BusinessTypeSchema } from '../business';
import { Sha256HexSchema } from '../entities/licensing';
import { LicenseClaimsSchema } from '../license';
import { PositiveIntSchema, TimestampSchema, UuidSchema } from '../primitives';
import { GeneratorAppInfoSchema } from './app-info';
import { command } from './contract';

export const MIN_SIGNING_PASSPHRASE_LENGTH = 12;

/**
 * - `absent`: no signing key yet.
 * - `locked`: encrypted key on disk; passphrase needed to issue.
 * - `unlocked`: decrypted in memory (until `lock_license_key` or app exit).
 */
export const SigningKeyStatusSchema = z.object({
  state: z.enum(['absent', 'locked', 'unlocked']),
  key_id: z.string().nullable(),
  /** SPKI PEM to embed in client builds (`POS_LICENSE_PUBLIC_KEY`) and the edge function. */
  public_key_pem: z.string().nullable(),
  key_path: z.string().nullable(),
});
export type SigningKeyStatus = z.infer<typeof SigningKeyStatusSchema>;

const PassphraseArgs = z.object({
  passphrase: z.string().min(MIN_SIGNING_PASSPHRASE_LENGTH).max(1024),
});

export const DecodedActivationRequestSchema = z.object({
  client_id: UuidSchema,
  fingerprint: Sha256HexSchema,
  device_key_hash: Sha256HexSchema,
  device_name: z.string(),
  app_version: z.string(),
});
export type DecodedActivationRequest = z.infer<typeof DecodedActivationRequestSchema>;

export const IssueLicenseRequestSchema = z.object({
  activation_code: z.string().min(1).max(4096),
  client_slug: z
    .string()
    .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/, 'kebab-case only')
    .max(40),
  business_type: BusinessTypeSchema,
  max_devices: PositiveIntSchema.max(1000),
  expires_at: TimestampSchema.nullable(),
});
export type IssueLicenseRequest = z.input<typeof IssueLicenseRequestSchema>;

export const IssuedLicenseSchema = z.object({
  token: z.string(),
  claims: LicenseClaimsSchema,
});
export type IssuedLicense = z.infer<typeof IssuedLicenseSchema>;

export const GENERATOR_IPC = {
  app_info: command(z.object({}), GeneratorAppInfoSchema, 1),
  license_key_status: command(z.object({}), SigningKeyStatusSchema, 2),
  create_license_key: command(PassphraseArgs, SigningKeyStatusSchema, 2),
  unlock_license_key: command(PassphraseArgs, SigningKeyStatusSchema, 2),
  lock_license_key: command(z.object({}), SigningKeyStatusSchema, 2),
  decode_activation_request: command(
    z.object({ code: z.string().min(1).max(4096) }),
    DecodedActivationRequestSchema,
    2,
  ),
  issue_license: command(z.object({ request: IssueLicenseRequestSchema }), IssuedLicenseSchema, 2),
} as const;

export type GeneratorIpcContract = typeof GENERATOR_IPC;
