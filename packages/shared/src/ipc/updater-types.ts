/**
 * Auto-update (Phase 8). The till checks its shop's release channel in the
 * background, downloads and verifies a signed update, and installs it when
 * the till is next restarted.
 */
import { z } from 'zod';
import { TimestampSchema } from '../primitives';

export const UPDATE_STATES = [
  /** No updater key or no cloud in this build: updates come as installers. */
  'unavailable',
  'idle',
  'checking',
  'downloading',
  /** Downloaded and verified; installs on the next restart. */
  'ready',
  'up_to_date',
  'error',
] as const;
export const UpdateStateSchema = z.enum(UPDATE_STATES);
export type UpdateState = z.infer<typeof UpdateStateSchema>;

export const UpdateStatusSchema = z.object({
  state: UpdateStateSchema,
  current_version: z.string().min(1),
  available_version: z.string().nullable(),
  notes: z.string().nullable(),
  /** Download progress in basis points while `downloading`. */
  progress_bps: z.int().min(0).max(10_000).nullable(),
  error: z.string().nullable(),
  last_checked_at: TimestampSchema.nullable(),
  /** Set after an update was installed, until the notice is dismissed. */
  updated_from: z.string().nullable(),
  /** Release notes of the version now running (shown with the notice). */
  updated_notes: z.string().nullable(),
});
export type UpdateStatus = z.infer<typeof UpdateStatusSchema>;
