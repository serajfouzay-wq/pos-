/**
 * IPC contract of the generator app (`apps/generator/src-tauri`): the client
 * registry, license signing and client builds on GitHub Actions.
 */
import { z } from 'zod';
import { ClientConfigSchema } from '../client-config';
import { Sha256HexSchema } from '../entities/licensing';
import { LicenseClaimsSchema } from '../license';
import { PositiveIntSchema, TimestampSchema, UuidSchema } from '../primitives';
import { GeneratorAppInfoSchema } from './app-info';
import { command } from './contract';
import {
  AssetKindSchema,
  BuildRecordSchema,
  BuildSettingsInputSchema,
  BuildSettingsSchema,
  ClientDetailSchema,
  ClientSummarySchema,
  IssuedLicenseRecordSchema,
  NewClientInputSchema,
  ReceiptPreviewSchema,
  RepoCheckSchema,
} from './generator-types';

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

const ClientRef = z.object({ client_id: UuidSchema });
const BuildRef = z.object({ build_id: UuidSchema });

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

/**
 * Slug and business type are taken from the client record; the activation
 * code must come from a till built for that client.
 */
export const IssueLicenseRequestSchema = z.object({
  activation_code: z.string().min(1).max(4096),
  client_id: UuidSchema,
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
  list_issued_licenses: command(ClientRef, z.array(IssuedLicenseRecordSchema), 5),

  // Clients
  list_clients: command(z.object({}), z.array(ClientSummarySchema), 5),
  get_client: command(ClientRef, ClientDetailSchema, 5),
  create_client: command(z.object({ input: NewClientInputSchema }), ClientDetailSchema, 5),
  /** The slug and client id are fixed; `receipt.logo_asset` follows the uploaded logo. */
  save_client: command(
    ClientRef.extend({ config: ClientConfigSchema, notes: z.string().max(2000) }),
    ClientDetailSchema,
    5,
  ),
  /** Soft delete: hidden from the dashboard, history kept, slug freed. */
  archive_client: command(ClientRef, z.null(), 5),
  /** PNG only. Logo ≤ 1 MB; icon square ≥ 512 px, ≤ 4 MB. */
  upload_client_asset: command(
    ClientRef.extend({ kind: AssetKindSchema, data_base64: z.string().min(1) }),
    ClientDetailSchema,
    5,
  ),
  remove_client_asset: command(ClientRef.extend({ kind: AssetKindSchema }), ClientDetailSchema, 5),
  get_client_asset: command(ClientRef.extend({ kind: AssetKindSchema }), z.string().nullable(), 5),
  /** Sample receipt for a draft (unsaved) config, rendered by the till's own layout code. */
  preview_receipt: command(
    ClientRef.extend({ config: ClientConfigSchema }),
    ReceiptPreviewSchema,
    5,
  ),

  // Builds
  get_build_settings: command(z.object({}), BuildSettingsSchema, 5),
  /** `github_token: null` keeps the stored token. */
  save_build_settings: command(
    z.object({
      settings: BuildSettingsInputSchema,
      github_token: z.string().max(255).nullable(),
    }),
    BuildSettingsSchema,
    5,
  ),
  clear_github_token: command(z.object({}), BuildSettingsSchema, 5),
  check_build_settings: command(z.object({}), RepoCheckSchema, 5),
  start_build: command(ClientRef, BuildRecordSchema, 5),
  list_builds: command(
    z.object({ client_id: UuidSchema.nullable() }),
    z.array(BuildRecordSchema),
    5,
  ),
  /** Follows every build still in flight on GitHub; returns the refreshed ones. */
  refresh_builds: command(z.object({}), z.array(BuildRecordSchema), 5),
  download_build: command(BuildRef, BuildRecordSchema, 5),
  open_build_run: command(BuildRef, z.null(), 5),
  reveal_build_download: command(BuildRef, z.null(), 5),
} as const;

export type GeneratorIpcContract = typeof GENERATOR_IPC;
