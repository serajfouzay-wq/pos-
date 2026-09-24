/**
 * Phase 6 shapes: the menu (options, combos, floor), open orders, stock and
 * labels. Rust examples in `contracts/pos-examples.json` are parsed with
 * these in tests.
 */
import { z } from 'zod';
import { STOCK_MOVEMENT_REASONS } from '../entities/catalog';
import {
  ComboItemSchema,
  ComboSchema,
  DiningTableSchema,
  ModifierGroupSchema,
  ModifierSchema,
  TableShapeSchema,
} from '../entities/menu';
import { ComboRefSchema, OpenOrderSchema } from '../entities/orders';
import { OrderTypeSchema } from '../entities/sales';
import { MinorUnitsSchema, NonNegativeMinorUnitsSchema } from '../money';
import {
  IntSchema,
  NonNegativeIntSchema,
  PositiveIntSchema,
  QuantityMilliSchema,
  UuidSchema,
} from '../primitives';

export const ModifierGroupWithOptionsSchema = ModifierGroupSchema.and(
  z.object({ modifiers: z.array(ModifierSchema) }),
);
export type ModifierGroupWithOptions = z.infer<typeof ModifierGroupWithOptionsSchema>;

export const ComboWithItemsSchema = ComboSchema.extend({ items: z.array(ComboItemSchema) });
export type ComboWithItems = z.infer<typeof ComboWithItemsSchema>;

/** Everything the sell screens need besides products (`get_menu`). */
export const MenuSchema = z.object({
  modifier_groups: z.array(ModifierGroupWithOptionsSchema),
  /** product id → the groups it asks, in order. */
  product_modifier_groups: z.record(UuidSchema, z.array(UuidSchema)),
  combos: z.array(ComboWithItemsSchema),
  dining_tables: z.array(DiningTableSchema),
});
export type Menu = z.infer<typeof MenuSchema>;

// ── back office inputs (catalog.manage) ────────────────────────────────────

export const ModifierGroupInputSchema = z.object({
  id: UuidSchema.nullable(),
  name: z.string().trim().min(1).max(80),
  min_select: z.int().min(0).max(20),
  max_select: z.int().min(1).max(20),
  sort_order: IntSchema,
  is_active: z.boolean(),
  modifiers: z
    .array(
      z.object({
        id: UuidSchema.nullable(),
        name: z.string().trim().min(1).max(80),
        price_delta: MinorUnitsSchema,
        is_default: z.boolean(),
        is_active: z.boolean(),
      }),
    )
    .min(1)
    .max(40),
});
export type ModifierGroupInput = z.infer<typeof ModifierGroupInputSchema>;

export const ComboInputSchema = z.object({
  id: UuidSchema.nullable(),
  name: z.string().trim().min(1).max(80),
  price: NonNegativeMinorUnitsSchema,
  color: z.string().nullable(),
  sort_order: IntSchema,
  is_active: z.boolean(),
  items: z
    .array(z.object({ product_id: UuidSchema, quantity_milli: QuantityMilliSchema }))
    .min(2)
    .max(12),
});
export type ComboInput = z.infer<typeof ComboInputSchema>;

export const DiningTableInputSchema = z.object({
  id: UuidSchema.nullable(),
  label: z.string().trim().min(1).max(16),
  area: z.string().max(40),
  seats: z.int().min(1).max(50),
  shape: TableShapeSchema,
  grid_x: z.int().min(0).max(23),
  grid_y: z.int().min(0).max(15),
  is_active: z.boolean(),
});
export type DiningTableInput = z.infer<typeof DiningTableInputSchema>;

// ── open orders ────────────────────────────────────────────────────────────

/** An open order plus what the floor plan and tab list show. */
export const OpenOrderViewSchema = OpenOrderSchema.extend({
  table_label: z.string().nullable(),
  /** Priced like the sale would be; null when empty. */
  total: MinorUnitsSchema.nullable(),
  /** Lines not yet sent to the kitchen. */
  unfired: NonNegativeIntSchema,
});
export type OpenOrderView = z.infer<typeof OpenOrderViewSchema>;

export const OpenOrderInputSchema = z.object({
  table_id: UuidSchema.nullable(),
  label: z.string().max(40).nullable(),
  guests: z.int().min(0).max(99),
  order_type: OrderTypeSchema,
});
export type OpenOrderInput = z.infer<typeof OpenOrderInputSchema>;

/** A line as the till sends it; who/when/fired are stamped by Rust. */
export const OrderLineInputSchema = z.object({
  line_id: UuidSchema,
  product_id: UuidSchema,
  quantity_milli: QuantityMilliSchema,
  modifier_ids: z.array(UuidSchema).max(20),
  course: PositiveIntSchema.max(9).nullable(),
  note: z.string().max(200).nullable(),
  combo: ComboRefSchema.nullable(),
});
export type OrderLineInput = z.infer<typeof OrderLineInputSchema>;

export const OpenOrderUpdateSchema = z.object({
  order_id: UuidSchema,
  /** The version this till last saw; another till's edit in between is refused. */
  expected_updated_at: z.string(),
  items: z.array(OrderLineInputSchema).max(500),
  table_id: UuidSchema.nullable(),
  label: z.string().max(40).nullable(),
  guests: z.int().min(0).max(99),
  notes: z.string().max(500).nullable(),
});
export type OpenOrderUpdate = z.infer<typeof OpenOrderUpdateSchema>;

export const FireOutcomeSchema = z.object({
  order: OpenOrderViewSchema,
  /** A kitchen printer took the ticket. */
  printed: z.boolean(),
  print_error: z.string().nullable(),
  ticket_text: z.string(),
});
export type FireOutcome = z.infer<typeof FireOutcomeSchema>;

// ── stock ──────────────────────────────────────────────────────────────────

export const STOCK_MODES = ['receive', 'adjust', 'waste', 'count'] as const;
export const StockModeSchema = z.enum(STOCK_MODES);
export type StockMode = z.infer<typeof StockModeSchema>;

export const StockAdjustmentSchema = z.object({
  product_id: UuidSchema,
  mode: StockModeSchema,
  /** receive/waste: > 0; adjust: ≠ 0 (±); count: the counted on-hand (≥ 0). */
  quantity_milli: z.int(),
  note: z.string().max(200).nullable(),
});
export type StockAdjustment = z.infer<typeof StockAdjustmentSchema>;

/** Reasons a movement can have, for history screens. */
export const StockReasonSchema = z.enum(STOCK_MOVEMENT_REASONS);

export { ComboRefSchema };
