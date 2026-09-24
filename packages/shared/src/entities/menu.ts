/**
 * Menu structure for cafes and restaurants, plus the dining-room layout.
 * All last-write-wins, synced between tills.
 */
import { z } from 'zod';
import { MinorUnitsSchema, NonNegativeMinorUnitsSchema } from '../money';
import {
  EntityBaseSchema,
  HexColorSchema,
  IntSchema,
  QuantityMilliSchema,
  UuidSchema,
} from '../primitives';
import { LocalizedNamesSchema } from './catalog';

/**
 * A choice asked when the product is added: "Size" (pick exactly 1),
 * "Milk" (0–1), "Extras" (0–3)…
 */
export const ModifierGroupSchema = EntityBaseSchema.extend({
  name: z.string().min(1).max(80),
  name_localized: LocalizedNamesSchema,
  min_select: z.int().min(0).max(20),
  max_select: z.int().min(1).max(20),
  sort_order: IntSchema,
  is_active: z.boolean(),
}).refine((g) => g.min_select <= g.max_select, {
  message: 'min_select cannot exceed max_select',
  path: ['min_select'],
});
export type ModifierGroup = z.infer<typeof ModifierGroupSchema>;

/** One option of a group. `price_delta` is added to the unit price (may be negative). */
export const ModifierSchema = EntityBaseSchema.extend({
  group_id: UuidSchema,
  name: z.string().min(1).max(80),
  name_localized: LocalizedNamesSchema,
  price_delta: MinorUnitsSchema,
  /** Pre-selected when the picker opens. */
  is_default: z.boolean(),
  sort_order: IntSchema,
  is_active: z.boolean(),
});
export type Modifier = z.infer<typeof ModifierSchema>;

/** Which groups a product asks, in order. */
export const ProductModifierGroupSchema = EntityBaseSchema.extend({
  product_id: UuidSchema,
  group_id: UuidSchema,
  sort_order: IntSchema,
});
export type ProductModifierGroup = z.infer<typeof ProductModifierGroupSchema>;

/**
 * A quick combo ("Breakfast set"): fixed components sold together for
 * `price`. Components are sold as normal lines (stock, kitchen, receipt) and
 * the saving is a discount allocated across them.
 */
export const ComboSchema = EntityBaseSchema.extend({
  name: z.string().min(1).max(80),
  name_localized: LocalizedNamesSchema,
  price: NonNegativeMinorUnitsSchema,
  color: HexColorSchema.nullable(),
  sort_order: IntSchema,
  is_active: z.boolean(),
});
export type Combo = z.infer<typeof ComboSchema>;

export const ComboItemSchema = EntityBaseSchema.extend({
  combo_id: UuidSchema,
  product_id: UuidSchema,
  quantity_milli: QuantityMilliSchema,
  sort_order: IntSchema,
});
export type ComboItem = z.infer<typeof ComboItemSchema>;

export const TABLE_SHAPES = ['square', 'round', 'bar'] as const;
export const TableShapeSchema = z.enum(TABLE_SHAPES);

/** Floor plan grid: 24 columns × 16 rows. */
export const FLOOR_COLUMNS = 24;
export const FLOOR_ROWS = 16;

export const DiningTableSchema = EntityBaseSchema.extend({
  label: z.string().min(1).max(16),
  area: z.string().max(40),
  seats: z.int().min(1).max(50),
  shape: TableShapeSchema,
  grid_x: z
    .int()
    .min(0)
    .max(FLOOR_COLUMNS - 1),
  grid_y: z
    .int()
    .min(0)
    .max(FLOOR_ROWS - 1),
  sort_order: IntSchema,
  is_active: z.boolean(),
});
export type DiningTable = z.infer<typeof DiningTableSchema>;
