import { z } from 'zod';
import { EntityBaseSchema, TimestampSchema, UuidSchema } from '../primitives';
import { RoleSchema } from '../rbac';

/**
 * `audit_log` — append-only. Every privileged action records who did it, in
 * what role, and the entity before/after the change.
 */
export const AuditLogEntrySchema = EntityBaseSchema.extend({
  user_id: UuidSchema,
  role: RoleSchema,
  /** Dotted action name, usually the permission exercised: `sale.refund`. */
  action: z.string().min(1).max(64),
  entity_type: z.string().min(1).max(64),
  entity_id: UuidSchema.nullable(),
  before: z.json().nullable(),
  after: z.json().nullable(),
  device_id: UuidSchema,
  occurred_at: TimestampSchema,
});
export type AuditLogEntry = z.infer<typeof AuditLogEntrySchema>;
