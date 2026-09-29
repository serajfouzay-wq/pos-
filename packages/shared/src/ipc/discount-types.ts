import { z } from 'zod';
import {
  DISCOUNT_KINDS,
  DISCOUNT_SCOPES,
  DiscountApplyModeSchema,
  DiscountRuleSchema,
} from '../entities/catalog';
import { NonNegativeMinorUnitsSchema } from '../money';
import { TimestampSchema, UuidSchema } from '../primitives';

/**
 * Creating or changing a discount rule (`catalog.manage`). `value` is basis
 * points for a percentage (1–10 000) and minor units for a fixed amount.
 * Days: Monday = bit 0 … Sunday = bit 6 (127 = every day). Times are
 * minutes after local midnight; an end before the start crosses midnight.
 */
export const DiscountRuleInputSchema = z
  .object({
    id: UuidSchema.nullable(),
    name: z.string().trim().min(1).max(80),
    kind: z.enum(DISCOUNT_KINDS),
    value: z.int().positive(),
    scope: z.enum(DISCOUNT_SCOPES),
    target_id: UuidSchema.nullable(),
    min_subtotal: NonNegativeMinorUnitsSchema.nullable(),
    starts_at: TimestampSchema.nullable(),
    ends_at: TimestampSchema.nullable(),
    is_active: z.boolean(),
    apply_mode: DiscountApplyModeSchema,
    days_mask: z.int().min(1).max(127).nullable(),
    time_from: z.int().min(0).max(1439).nullable(),
    time_to: z.int().min(1).max(1440).nullable(),
  })
  .refine((r) => r.kind !== 'percentage' || r.value <= 10_000, {
    message: 'percentage discounts are capped at 100%',
    path: ['value'],
  })
  .refine((r) => r.scope === 'order' || r.target_id !== null, {
    message: 'choose what the discount applies to',
    path: ['target_id'],
  });
export type DiscountRuleInput = z.infer<typeof DiscountRuleInputSchema>;

export const DiscountRuleViewSchema = z.object({
  rule: DiscountRuleSchema,
  /** The product or category it applies to. */
  target_name: z.string().nullable(),
  /** Running right now: active, within its dates, days and hours. */
  live: z.boolean(),
});
export type DiscountRuleView = z.infer<typeof DiscountRuleViewSchema>;
