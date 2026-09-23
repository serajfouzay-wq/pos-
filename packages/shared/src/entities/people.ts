import { z } from 'zod';
import {
  EntityBaseSchema,
  IntSchema,
  NonNegativeIntSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';
import { RoleSchema } from '../rbac';

/**
 * `users` row as stored and synced. `pin_hash` (Argon2id) never crosses the
 * IPC boundary — the frontend only ever sees {@link UserSchema}.
 */
export const UserRowSchema = EntityBaseSchema.extend({
  display_name: z.string().min(1).max(80),
  role: RoleSchema,
  pin_hash: z.string().min(1),
  is_active: z.boolean(),
  failed_pin_attempts: NonNegativeIntSchema,
  locked_until: TimestampSchema.nullable(),
});
export type UserRow = z.infer<typeof UserRowSchema>;

export const UserSchema = UserRowSchema.omit({ pin_hash: true, failed_pin_attempts: true });
export type User = z.infer<typeof UserSchema>;

/** `customers` — `loyalty_points` is a cached balance; the ledger is the truth. */
export const CustomerSchema = EntityBaseSchema.extend({
  display_name: z.string().min(1).max(120),
  phone: z.string().max(32).nullable(),
  email: z.email().nullable(),
  loyalty_points: IntSchema,
  notes: z.string().max(1000).nullable(),
});
export type Customer = z.infer<typeof CustomerSchema>;

export const LOYALTY_REASONS = ['earn', 'redeem', 'adjust', 'expire', 'refund_reversal'] as const;

/** `loyalty_ledger` — append-only additive deltas; balance = Σ points_delta. */
export const LoyaltyLedgerEntrySchema = EntityBaseSchema.extend({
  customer_id: UuidSchema,
  transaction_id: UuidSchema.nullable(),
  points_delta: IntSchema.refine((n) => n !== 0, 'delta must be non-zero'),
  reason: z.enum(LOYALTY_REASONS),
  device_id: UuidSchema,
  user_id: UuidSchema,
});
export type LoyaltyLedgerEntry = z.infer<typeof LoyaltyLedgerEntrySchema>;
