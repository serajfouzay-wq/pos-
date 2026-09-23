import { z } from 'zod';
import { CurrencyCodeSchema } from '../currency';
import { NonNegativeMinorUnitsSchema } from '../money';
import {
  EntityBaseSchema,
  NonNegativeIntSchema,
  QuantityMilliSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';

export const SupplierSchema = EntityBaseSchema.extend({
  name: z.string().min(1).max(120),
  contact_name: z.string().max(120).nullable(),
  phone: z.string().max(32).nullable(),
  email: z.email().nullable(),
  tax_number: z.string().max(40).nullable(),
  notes: z.string().max(1000).nullable(),
});
export type Supplier = z.infer<typeof SupplierSchema>;

export const PURCHASE_ORDER_STATUSES = [
  'draft',
  'sent',
  'partially_received',
  'received',
  'cancelled',
] as const;

export const PurchaseOrderSchema = EntityBaseSchema.extend({
  supplier_id: UuidSchema,
  reference: z.string().min(1).max(32),
  status: z.enum(PURCHASE_ORDER_STATUSES),
  currency: CurrencyCodeSchema,
  total: NonNegativeMinorUnitsSchema,
  ordered_at: TimestampSchema.nullable(),
  expected_at: TimestampSchema.nullable(),
  created_by: UuidSchema,
  notes: z.string().max(1000).nullable(),
});
export type PurchaseOrder = z.infer<typeof PurchaseOrderSchema>;

export const PurchaseOrderItemSchema = EntityBaseSchema.extend({
  purchase_order_id: UuidSchema,
  product_id: UuidSchema,
  product_name: z.string().min(1).max(120),
  quantity_ordered_milli: QuantityMilliSchema,
  quantity_received_milli: NonNegativeIntSchema,
  unit_cost: NonNegativeMinorUnitsSchema,
});
export type PurchaseOrderItem = z.infer<typeof PurchaseOrderItemSchema>;
