/**
 * Open orders: a cafe tab or a restaurant table, kept until it is paid. One
 * order can be settled by several transactions (split bill); each paid line
 * leaves the order. Last-write-wins between tills: two tills editing the
 * same open order at the same moment keep the later edit.
 */
import { z } from 'zod';
import {
  EntityBaseSchema,
  NonNegativeIntSchema,
  PositiveIntSchema,
  QuantityMilliSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';
import { OrderTypeSchema } from './sales';

/** Lines added together from one combo share an `instance`. */
export const ComboRefSchema = z.object({
  combo_id: UuidSchema,
  instance: UuidSchema,
});
export type ComboRef = z.infer<typeof ComboRefSchema>;

export const OpenOrderItemSchema = z.object({
  line_id: UuidSchema,
  product_id: UuidSchema,
  quantity_milli: QuantityMilliSchema,
  modifier_ids: z.array(UuidSchema).max(20),
  course: PositiveIntSchema.max(9).nullable(),
  note: z.string().max(200).nullable(),
  combo: ComboRefSchema.nullable(),
  /** Sent to the kitchen; editing or removing it then needs `sale.void`. */
  fired_at: TimestampSchema.nullable(),
  added_by: UuidSchema,
  added_at: TimestampSchema,
});
export type OpenOrderItem = z.infer<typeof OpenOrderItemSchema>;

export const OPEN_ORDER_STATUSES = ['open', 'settled', 'cancelled'] as const;
export const OpenOrderStatusSchema = z.enum(OPEN_ORDER_STATUSES);
export type OpenOrderStatus = z.infer<typeof OpenOrderStatusSchema>;

export const OpenOrderSchema = EntityBaseSchema.extend({
  device_id: UuidSchema,
  order_type: OrderTypeSchema,
  table_id: UuidSchema.nullable(),
  /** Tab name ("Sara", "Window seat") or null for a table order. */
  label: z.string().max(40).nullable(),
  guests: NonNegativeIntSchema.max(99),
  status: OpenOrderStatusSchema,
  items: z.array(OpenOrderItemSchema).max(500),
  /** Sales that paid parts of this order (split bill). */
  transaction_ids: z.array(UuidSchema),
  opened_by: UuidSchema,
  opened_at: TimestampSchema,
  closed_at: TimestampSchema.nullable(),
  notes: z.string().max(500).nullable(),
});
export type OpenOrder = z.infer<typeof OpenOrderSchema>;
