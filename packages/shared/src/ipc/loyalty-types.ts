/**
 * Customers and loyalty (Phase 8). Points are earned on sales and redeemed
 * as an order discount; the ledger is append-only and syncs as additive
 * deltas, so tills that sell offline still converge on each balance.
 */
import { z } from 'zod';
import { MemberQuoteSchema } from './membership-types';
import { CustomerSchema, LOYALTY_REASONS } from '../entities/people';
import { LoyaltySettingsSchema } from '../entities/shop';
import { MinorUnitsSchema, NonNegativeMinorUnitsSchema } from '../money';
import { IntSchema, NonNegativeIntSchema, TimestampSchema, UuidSchema } from '../primitives';

export const CustomerInputSchema = z.object({
  /** null = register a new customer. */
  id: UuidSchema.nullable(),
  display_name: z.string().trim().min(1).max(120),
  /** Digits, spaces and + - ( ) only; Rust keeps the digits for lookups. */
  phone: z
    .string()
    .trim()
    .max(32)
    .regex(/^[0-9+\-() ]*$/, 'Digits only')
    .nullable(),
  email: z.email().max(200).nullable(),
  notes: z.string().max(1000).nullable(),
});
export type CustomerInput = z.input<typeof CustomerInputSchema>;

export const CustomerSearchSchema = z.object({
  /** Name, phone or email; empty = most recent customers. */
  query: z.string().max(120),
  limit: z.int().min(1).max(100),
});
export type CustomerSearch = z.infer<typeof CustomerSearchSchema>;

export const LoyaltyLedgerViewSchema = z.object({
  id: UuidSchema,
  occurred_at: TimestampSchema,
  points_delta: IntSchema,
  reason: z.enum(LOYALTY_REASONS),
  transaction_id: UuidSchema.nullable(),
  receipt_number: z.string().nullable(),
  user_name: z.string(),
});
export type LoyaltyLedgerView = z.infer<typeof LoyaltyLedgerViewSchema>;

export const CustomerDetailSchema = z.object({
  customer: CustomerSchema,
  /** Sales to this customer (voided sales and the reversals themselves not counted). */
  visits: NonNegativeIntSchema,
  /** Net of refunds and voids. */
  spent: MinorUnitsSchema,
  last_visit_at: TimestampSchema.nullable(),
  /** Newest first. */
  ledger: z.array(LoyaltyLedgerViewSchema),
});
export type CustomerDetail = z.infer<typeof CustomerDetailSchema>;

export const PointsAdjustmentSchema = z.object({
  customer_id: UuidSchema,
  points_delta: z
    .int()
    .min(-1_000_000)
    .max(1_000_000)
    .refine((n) => n !== 0, 'Enter the points to add or remove'),
  note: z.string().trim().min(1).max(200),
});
export type PointsAdjustment = z.input<typeof PointsAdjustmentSchema>;

/** `get_loyalty_settings`: the programme, and whether this build has it. */
export const LoyaltyProgramSchema = z.object({
  /** `features.loyalty` of the build. */
  available: z.boolean(),
  settings: LoyaltySettingsSchema,
});
export type LoyaltyProgram = z.infer<typeof LoyaltyProgramSchema>;

/** Part of a quote request: who is buying, and the points to spend. */
export const LoyaltyRequestSchema = z.object({
  customer_id: UuidSchema,
  redeem_points: NonNegativeIntSchema,
});
export type LoyaltyRequest = z.input<typeof LoyaltyRequestSchema>;

/** Part of a quote: the customer's points on this bill (Rust's figures). */
export const LoyaltyQuoteSchema = z.object({
  customer_id: UuidSchema,
  customer_name: z.string(),
  /** False when the programme is off: the customer is only named on the receipt. */
  enabled: z.boolean(),
  balance: IntSchema,
  redeem_points: NonNegativeIntSchema,
  /** Taken off the bill for the redeemed points. */
  redeem_value: NonNegativeMinorUnitsSchema,
  /** The most this bill can take (balance, minimum and share rules applied). */
  max_redeem_points: NonNegativeIntSchema,
  points_earned: NonNegativeIntSchema,
  /** The membership the customer holds now. */
  member: MemberQuoteSchema.nullable(),
});
export type LoyaltyQuote = z.infer<typeof LoyaltyQuoteSchema>;
