import { z } from 'zod';
import { BasisPointsSchema, EntityBaseSchema } from '../primitives';

/**
 * `shop_settings` — settings shared by every till of the shop (synced,
 * last-write-wins). Each key has a FIXED row id, so two tills that create
 * the same setting offline write the same row and converge instead of
 * clashing on the unique key. Device-local settings (printers…) live in
 * `settings` instead.
 */
export const ShopSettingRowSchema = EntityBaseSchema.extend({
  key: z.string().min(1).max(64),
  value: z.json(),
});
export type ShopSettingRow = z.infer<typeof ShopSettingRowSchema>;

/** Row ids of the known shop settings (see `pos_client::repo::shop`). */
export const SHOP_SETTING_IDS = {
  loyalty: '0199a000-0000-7000-8000-000000000001',
} as const;

/**
 * The loyalty programme. Points are integers; money stays in minor units.
 * - earn: `floor(total × points_per_unit / 10^exponent)` per sale (the
 *   amount actually paid, after discounts and redeemed points);
 * - redeem: each point takes `point_value` minor units off the bill as an
 *   order discount, from `min_redeem_points` up, and for at most
 *   `max_redeem_bps` of what is left to pay before tax.
 */
export const LoyaltySettingsSchema = z.object({
  enabled: z.boolean(),
  points_per_unit: z.int().min(0).max(1000),
  point_value: z.int().min(1).max(1_000_000),
  min_redeem_points: z.int().min(0).max(1_000_000),
  max_redeem_bps: BasisPointsSchema,
});
export type LoyaltySettings = z.infer<typeof LoyaltySettingsSchema>;
