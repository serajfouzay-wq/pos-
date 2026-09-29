import { z } from 'zod';
import { MembershipPlanSchema, MembershipSchema } from '../entities/membership';
import { NonNegativeMinorUnitsSchema } from '../money';
import { BasisPointsSchema, HexColorSchema, TimestampSchema, UuidSchema } from '../primitives';

/** Creating or changing a plan (`customer.manage`); its product follows. */
export const MembershipPlanInputSchema = z.object({
  id: UuidSchema.nullable(),
  name: z.string().trim().min(1).max(80),
  description: z.string().max(500).nullable(),
  price: NonNegativeMinorUnitsSchema,
  duration_days: z.int().min(1).max(3660),
  /** Off every bill of a member (0 = none). */
  discount_bps: BasisPointsSchema,
  /** Points earned × this / 10 000 (10 000 = normal, 20 000 = double). */
  points_multiplier_bps: z.int().min(0).max(100_000),
  color: HexColorSchema.nullable(),
  is_active: z.boolean(),
});
export type MembershipPlanInput = z.infer<typeof MembershipPlanInputSchema>;

export const MembershipPlanViewSchema = z.object({
  plan: MembershipPlanSchema,
  /** Customers holding a period of it right now. */
  active_members: z.int().nonnegative(),
});
export type MembershipPlanView = z.infer<typeof MembershipPlanViewSchema>;

export const MEMBER_STATES = ['active', 'upcoming', 'expired', 'cancelled'] as const;
export const MemberStateSchema = z.enum(MEMBER_STATES);
export type MemberState = z.infer<typeof MemberStateSchema>;

export const MemberRowSchema = z.object({
  membership: MembershipSchema,
  customer_name: z.string(),
  customer_phone: z.string().nullable(),
  plan_name: z.string(),
  state: MemberStateSchema,
});
export type MemberRow = z.infer<typeof MemberRowSchema>;

export const MemberFilterSchema = z.object({
  /** Name, phone or card number. */
  query: z.string().max(80).default(''),
  /** null = every state. */
  state: MemberStateSchema.nullable(),
  customer_id: UuidSchema.nullable().default(null),
  limit: z.int().min(1).max(500),
});
export type MemberFilter = z.input<typeof MemberFilterSchema>;

/** A period given without a sale (a gift, staff, a correction). */
export const GrantMembershipSchema = z.object({
  customer_id: UuidSchema,
  plan_id: UuidSchema,
  notes: z.string().max(500).nullable(),
});
export type GrantMembership = z.infer<typeof GrantMembershipSchema>;

/** The membership a customer holds on a bill; its discount is in the quote. */
export const MemberQuoteSchema = z.object({
  membership_id: UuidSchema,
  plan_name: z.string(),
  card_number: z.string(),
  ends_at: TimestampSchema,
  discount_bps: BasisPointsSchema,
  points_multiplier_bps: z.int().min(0).max(100_000),
  /** What the member discount takes off this bill. */
  discount: NonNegativeMinorUnitsSchema,
});
export type MemberQuote = z.infer<typeof MemberQuoteSchema>;
