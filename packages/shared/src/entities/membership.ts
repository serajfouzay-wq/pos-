import { z } from 'zod';
import { NonNegativeMinorUnitsSchema } from '../money';
import {
  BasisPointsSchema,
  EntityBaseSchema,
  HexColorSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';

/**
 * `membership_plans` — what a shop sells as a membership (synced, LWW).
 * Each plan has its own product row, so selling one is an ordinary sale:
 * paid, printed, reported and refunded like anything else. Members get
 * `discount_bps` off every bill and earn points × `points_multiplier_bps`.
 */
export const MembershipPlanSchema = EntityBaseSchema.extend({
  name: z.string().min(1).max(80),
  description: z.string().max(500).nullable(),
  product_id: UuidSchema,
  price: NonNegativeMinorUnitsSchema,
  duration_days: z.int().min(1).max(3660),
  discount_bps: BasisPointsSchema,
  points_multiplier_bps: z.int().min(0).max(100_000),
  color: HexColorSchema.nullable(),
  is_active: z.boolean(),
});
export type MembershipPlan = z.infer<typeof MembershipPlanSchema>;

export const MEMBERSHIP_STATUSES = ['active', 'cancelled'] as const;
export const MembershipStatusSchema = z.enum(MEMBERSHIP_STATUSES);

/**
 * `memberships` — one period of one customer's membership (synced, LWW).
 * Renewing adds a period that starts when the current one ends.
 */
export const MembershipSchema = EntityBaseSchema.extend({
  customer_id: UuidSchema,
  plan_id: UuidSchema,
  /** Printed on the card; also finds the customer at the till. */
  card_number: z.string().min(4).max(32),
  starts_at: TimestampSchema,
  ends_at: TimestampSchema,
  status: MembershipStatusSchema,
  /** The sale that paid for it; null when the owner granted it. */
  transaction_id: UuidSchema.nullable(),
  price_paid: NonNegativeMinorUnitsSchema,
  device_id: UuidSchema,
  notes: z.string().max(500).nullable(),
});
export type Membership = z.infer<typeof MembershipSchema>;
