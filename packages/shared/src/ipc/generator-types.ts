/**
 * Shapes returned by the generator's Rust core (`apps/generator/src-tauri`).
 * Examples in `contracts/generator-examples.json` are produced by Rust and
 * parsed here in tests.
 */
import { z } from 'zod';
import { BusinessTypeSchema } from '../business';
import { ClientConfigSchema } from '../client-config';
import { CurrencyCodeSchema } from '../currency';
import { Sha256HexSchema } from '../entities/licensing';
import {
  NonNegativeIntSchema,
  PositiveIntSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';

export const ClientSlugSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/, 'kebab-case only')
  .max(40);

export const NewClientInputSchema = z.object({
  display_name: z.string().trim().min(1).max(80),
  client_slug: ClientSlugSchema,
  business_type: BusinessTypeSchema,
  base_currency: CurrencyCodeSchema,
});
export type NewClientInput = z.infer<typeof NewClientInputSchema>;

/** `receipt_logo`: printed on receipts. `app_icon`: installer and window icon. */
export const ASSET_KINDS = ['receipt_logo', 'app_icon'] as const;
export const AssetKindSchema = z.enum(ASSET_KINDS);
export type AssetKind = z.infer<typeof AssetKindSchema>;

export const AssetInfoSchema = z.object({
  kind: AssetKindSchema,
  sha256: Sha256HexSchema,
  width: PositiveIntSchema,
  height: PositiveIntSchema,
  byte_length: NonNegativeIntSchema,
  updated_at: TimestampSchema,
});
export type AssetInfo = z.infer<typeof AssetInfoSchema>;

export const ClientDetailSchema = z.object({
  client_id: UuidSchema,
  created_at: TimestampSchema,
  updated_at: TimestampSchema,
  config: ClientConfigSchema,
  notes: z.string().max(2000),
  assets: z.array(AssetInfoSchema),
});
export type ClientDetail = z.infer<typeof ClientDetailSchema>;

/**
 * - `publishing`: committing the client's files to the build repository.
 * - `queued` / `in_progress`: the GitHub Actions run.
 * - `succeeded` / `failed` / `cancelled`: the run's outcome.
 * - `error`: the generator could not publish or dispatch (see `message`).
 */
export const BUILD_STATUSES = [
  'publishing',
  'queued',
  'in_progress',
  'succeeded',
  'failed',
  'cancelled',
  'error',
] as const;
export const BuildStatusSchema = z.enum(BUILD_STATUSES);
export type BuildStatus = z.infer<typeof BuildStatusSchema>;

export function isActiveBuild(status: BuildStatus): boolean {
  return status === 'publishing' || status === 'queued' || status === 'in_progress';
}

export const BuildRecordSchema = z.object({
  build_id: UuidSchema,
  client_id: UuidSchema,
  client_slug: ClientSlugSchema,
  status: BuildStatusSchema,
  config_sha256: Sha256HexSchema,
  /** The client version this build produces (MAJOR.MINOR of the app, then a per-client number). */
  app_version: z.string(),
  release_notes: z.string(),
  /**
   * Also publish the signed installers to the client's cloud (needs its
   * service key on this PC). Update files for a USB stick are made either way.
   */
  publish_update: z.boolean(),
  commit_sha: z.string().nullable(),
  run_id: NonNegativeIntSchema.nullable(),
  run_url: z.string().nullable(),
  artifact_id: NonNegativeIntSchema.nullable(),
  artifact_name: z.string().nullable(),
  artifact_size: NonNegativeIntSchema.nullable(),
  /** Folder with the installers and the signed `.posupdate` files. */
  download_path: z.string().nullable(),
  message: z.string().nullable(),
  requested_at: TimestampSchema,
  updated_at: TimestampSchema,
  completed_at: TimestampSchema.nullable(),
});
export type BuildRecord = z.infer<typeof BuildRecordSchema>;

/** `start_build` options: what the tills show after updating, and whether to ship it. */
export const ReleaseOptionsSchema = z.object({
  release_notes: z.string().trim().max(1000),
  publish_update: z.boolean(),
});
export type ReleaseOptions = z.input<typeof ReleaseOptionsSchema>;

export const ClientSummarySchema = z.object({
  client_id: UuidSchema,
  client_slug: ClientSlugSchema,
  display_name: z.string(),
  business_type: BusinessTypeSchema,
  updated_at: TimestampSchema,
  has_receipt_logo: z.boolean(),
  has_app_icon: z.boolean(),
  license_count: NonNegativeIntSchema,
  last_build: BuildRecordSchema.nullable(),
});
export type ClientSummary = z.infer<typeof ClientSummarySchema>;

export const IssuedLicenseRecordSchema = z.object({
  license_id: UuidSchema,
  client_id: UuidSchema,
  device_name: z.string(),
  fingerprint_hash: Sha256HexSchema,
  max_devices: PositiveIntSchema,
  issued_at: TimestampSchema,
  expires_at: TimestampSchema.nullable(),
  token: z.string(),
});
export type IssuedLicenseRecord = z.infer<typeof IssuedLicenseRecordSchema>;

/** The build repository. The GitHub token is kept in the OS credential store. */
export const BuildSettingsInputSchema = z.object({
  repo_owner: z.string().min(1).max(100),
  repo_name: z.string().min(1).max(100),
  branch: z.string().min(1).max(100),
  workflow_file: z.string().min(1).max(100),
  api_base_url: z.string().min(1).max(200),
});
export type BuildSettingsInput = z.infer<typeof BuildSettingsInputSchema>;

/** Current settings; owner and name are empty until configured. */
export const BuildSettingsSchema = z.object({
  repo_owner: z.string().max(100),
  repo_name: z.string().max(100),
  branch: z.string().max(100),
  workflow_file: z.string().max(100),
  api_base_url: z.string().max(200),
  token_configured: z.boolean(),
});
export type BuildSettings = z.infer<typeof BuildSettingsSchema>;

export const RepoCheckSchema = z.object({
  default_branch: z.string(),
  can_push: z.boolean(),
  branch_found: z.boolean(),
  workflow_found: z.boolean(),
});
export type RepoCheck = z.infer<typeof RepoCheckSchema>;

export const ReceiptPreviewSchema = z.object({
  columns: PositiveIntSchema,
  /** Monospace text, exactly the characters the printer receives. */
  text: z.string(),
  /** The logo as printed (1-bit dithered PNG), base64. */
  logo_png_base64: z.string().nullable(),
  logo_width: PositiveIntSchema.nullable(),
  logo_height: PositiveIntSchema.nullable(),
  /**
   * The receipt in the client's default language as the till prints it when
   * that needs an image (Arabic), as a PNG; null when it prints as text.
   */
  image_png_base64: z.string().nullable(),
});
export type ReceiptPreview = z.infer<typeof ReceiptPreviewSchema>;

/**
 * The key that signs every client's updates (minisign). It lives in the
 * OS credential store; the tills are built with its public half.
 */
export const UpdateKeyStatusSchema = z.object({
  configured: z.boolean(),
  key_id: z.string().nullable(),
  /** Base64 of the minisign public key (what `POS_UPDATER_PUBLIC_KEY` holds). */
  public_key: z.string().nullable(),
  /** When a backup file was last saved; `null` = never (the Start checklist warns). */
  backed_up_at: TimestampSchema.nullable(),
});
export type UpdateKeyStatus = z.infer<typeof UpdateKeyStatusSchema>;

/** File extension of a signed offline update (a zip: manifest + installer). */
export const UPDATE_FILE_EXTENSION = 'posupdate';
