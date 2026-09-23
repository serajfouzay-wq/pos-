import { z } from 'zod';
import { LocaleSchema } from '../i18n';
import { NonNegativeMinorUnitsSchema } from '../money';
import {
  BasisPointsSchema,
  EntityBaseSchema,
  HexColorSchema,
  IntSchema,
  NonNegativeIntSchema,
  QuantityMilliSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';

/** Optional per-locale overrides of a display name, e.g. `{ ar: 'قهوة' }`. */
export const LocalizedNamesSchema = z.partialRecord(LocaleSchema, z.string().min(1).max(120));

export const CategorySchema = EntityBaseSchema.extend({
  name: z.string().min(1).max(80),
  name_localized: LocalizedNamesSchema,
  parent_id: UuidSchema.nullable(),
  sort_order: IntSchema,
  color: HexColorSchema.nullable(),
});
export type Category = z.infer<typeof CategorySchema>;

export const PRODUCT_UNITS = ['each', 'kg', 'g', 'l', 'ml'] as const;
export const ProductUnitSchema = z.enum(PRODUCT_UNITS);

export const ProductSchema = EntityBaseSchema.extend({
  name: z.string().min(1).max(120),
  name_localized: LocalizedNamesSchema,
  category_id: UuidSchema.nullable(),
  sku: z.string().max(64).nullable(),
  barcode: z.string().max(64).nullable(),
  /** Selling price in base-currency minor units. */
  price: NonNegativeMinorUnitsSchema,
  cost: NonNegativeMinorUnitsSchema.nullable(),
  tax_rate_bps: BasisPointsSchema,
  unit: ProductUnitSchema,
  sold_by_weight: z.boolean(),
  track_stock: z.boolean(),
  /** Cached Σ of `stock_movements.quantity_delta_milli`; recomputed on sync. */
  stock_on_hand_milli: IntSchema,
  reorder_threshold_milli: NonNegativeIntSchema.nullable(),
  reorder_quantity_milli: QuantityMilliSchema.nullable(),
  image_asset: z.string().nullable(),
  /** Slot on the retail quick-keys grid / cafe menu grid. */
  quick_key_position: NonNegativeIntSchema.nullable(),
  is_active: z.boolean(),
});
export type Product = z.infer<typeof ProductSchema>;

export const STOCK_MOVEMENT_REASONS = [
  'sale',
  'refund',
  'purchase_receipt',
  'adjustment',
  'waste',
  'stock_count',
] as const;

/**
 * `stock_movements` — inventory as additive delta events. Two offline devices
 * selling the same item both append −1; merging is plain summation, so no
 * update is ever lost.
 */
export const StockMovementSchema = EntityBaseSchema.extend({
  product_id: UuidSchema,
  quantity_delta_milli: IntSchema.refine((n) => n !== 0, 'delta must be non-zero'),
  reason: z.enum(STOCK_MOVEMENT_REASONS),
  /** transaction / purchase order / count sheet that caused the movement. */
  reference_id: UuidSchema.nullable(),
  device_id: UuidSchema,
  user_id: UuidSchema,
  occurred_at: TimestampSchema,
});
export type StockMovement = z.infer<typeof StockMovementSchema>;

export const DISCOUNT_KINDS = ['percentage', 'fixed_amount'] as const;
export const DISCOUNT_SCOPES = ['order', 'product', 'category'] as const;

export const DiscountRuleSchema = EntityBaseSchema.extend({
  name: z.string().min(1).max(80),
  kind: z.enum(DISCOUNT_KINDS),
  /** Basis points for `percentage`, minor units for `fixed_amount`. */
  value: NonNegativeIntSchema,
  scope: z.enum(DISCOUNT_SCOPES),
  /** Product or category id when scope ≠ `order`. */
  target_id: UuidSchema.nullable(),
  min_subtotal: NonNegativeMinorUnitsSchema.nullable(),
  starts_at: TimestampSchema.nullable(),
  ends_at: TimestampSchema.nullable(),
  is_active: z.boolean(),
})
  .refine((rule) => rule.kind !== 'percentage' || rule.value <= 10_000, {
    message: 'percentage discounts are capped at 10 000 bps',
    path: ['value'],
  })
  .refine((rule) => (rule.scope === 'order') === (rule.target_id === null), {
    message: 'target_id is required for product/category scope and forbidden for order scope',
    path: ['target_id'],
  });
export type DiscountRule = z.infer<typeof DiscountRuleSchema>;
