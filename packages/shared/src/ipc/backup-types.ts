import { z } from 'zod';
import { TimestampSchema, UuidSchema } from '../primitives';

/** This till's backup settings (device-local). */
export const BackupSettingsSchema = z.object({
  automatic: z.boolean(),
  interval_hours: z.int().min(1).max(168),
  /** Newest backups kept (older ones are removed). */
  keep: z.int().min(3).max(365),
  /** A second copy of every backup: a USB stick or another disk. */
  extra_dir: z.string().max(500).nullable(),
});
export type BackupSettings = z.infer<typeof BackupSettingsSchema>;

export const BACKUP_REASONS = [
  'manual',
  'scheduled',
  'start',
  'shift_close',
  'z_report',
  'before_restore',
  'before_update',
] as const;

export const BackupInfoSchema = z.object({
  path: z.string(),
  file_name: z.string(),
  format: z.int(),
  created_at: TimestampSchema,
  reason: z.enum(BACKUP_REASONS),
  app_version: z.string(),
  schema_version: z.int(),
  client_id: UuidSchema,
  device_id: UuidSchema,
  device_name: z.string(),
  /** Protected by the backup password: restores on any PC. */
  portable: z.boolean(),
  salt: z.string().nullable(),
  size_bytes: z.int().nonnegative(),
});
export type BackupInfo = z.infer<typeof BackupInfoSchema>;

export const BackupStatusSchema = z.object({
  settings: BackupSettingsSchema,
  dir: z.string(),
  backups: z.array(BackupInfoSchema),
  last_backup_at: TimestampSchema.nullable(),
  password_set: z.boolean(),
  /** null = no second folder set. */
  extra_dir_ok: z.boolean().nullable(),
  integrity: z.enum(['unknown', 'ok', 'damaged']),
  integrity_detail: z.string().nullable(),
  last_error: z.string().nullable(),
  /** A restore is staged and takes effect when the app restarts. */
  restore_pending: z.boolean(),
});
export type BackupStatus = z.infer<typeof BackupStatusSchema>;

export const RestoreRequestSchema = z.object({
  path: z.string().min(1).max(1000),
  password: z.string().max(200).nullable(),
});
export type RestoreRequest = z.infer<typeof RestoreRequestSchema>;

/** What a staged restore contains (the backup's description). */
export const RestoredBackupSchema = BackupInfoSchema.omit({ path: true, file_name: true });
export type RestoredBackup = z.infer<typeof RestoredBackupSchema>;
