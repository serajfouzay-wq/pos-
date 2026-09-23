import { z } from 'zod';
import { CurrencyCodeSchema } from '../currency';
import { MinorUnitsSchema, NonNegativeMinorUnitsSchema } from '../money';
import {
  BasisPointsSchema,
  EntityBaseSchema,
  NonNegativeIntSchema,
  PositiveIntSchema,
  QuantityMilliSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';

/**
 * Transactions are IMMUTABLE and APPEND-ONLY. A refund or void is a new
 * transaction of kind `refund`/`void` pointing at `original_transaction_id`;
 * the original row is never modified. Consequently `updated_at === created_at`
 * and `deleted_at === null` for every row.
 */
export const TRANSACTION_KINDS = ['sale', 'refund', 'void'] as const;
export const TransactionKindSchema = z.enum(TRANSACTION_KINDS);

export const ORDER_TYPES = ['counter', 'dine_in', 'takeaway', 'delivery'] as const;
export const OrderTypeSchema = z.enum(ORDER_TYPES);

export const TransactionSchema = EntityBaseSchema.extend({
  kind: TransactionKindSchema,
  original_transaction_id: UuidSchema.nullable(),
  /** Human-facing, unique per device: e.g. `D01-000123`. */
  receipt_number: z.string().min(1).max(32),
  device_id: UuidSchema,
  shift_id: UuidSchema,
  cashier_id: UuidSchema,
  /** Manager/owner who authorised a refund, void or discount override. */
  approved_by: UuidSchema.nullable(),
  customer_id: UuidSchema.nullable(),
  order_type: OrderTypeSchema,
  table_label: z.string().max(32).nullable(),
  currency: CurrencyCodeSchema,
  subtotal: MinorUnitsSchema,
  discount_total: MinorUnitsSchema,
  tax_total: MinorUnitsSchema,
  total: MinorUnitsSchema,
  loyalty_points_earned: NonNegativeIntSchema,
  loyalty_points_redeemed: NonNegativeIntSchema,
  notes: z.string().max(500).nullable(),
  /** Client-generated key; a retried `create_transaction` with the same key is a no-op. */
  idempotency_key: UuidSchema,
  occurred_at: TimestampSchema,
}).refine((tx) => (tx.kind === 'sale') === (tx.original_transaction_id === null), {
  message: 'refunds and voids must reference the original transaction; sales must not',
  path: ['original_transaction_id'],
});
export type Transaction = z.infer<typeof TransactionSchema>;

/** A modifier as it was sold (snapshot), e.g. `{ name: 'Oat milk', price_delta: 250 }`. */
export const ModifierSnapshotSchema = z.object({
  modifier_id: UuidSchema.nullable(),
  name: z.string().min(1).max(80),
  price_delta: MinorUnitsSchema,
});
export type ModifierSnapshot = z.infer<typeof ModifierSnapshotSchema>;

/**
 * `transaction_items` — `product_name`, `sku` and `unit_price` are snapshots
 * taken at sale time so later catalogue edits never rewrite history.
 */
export const TransactionItemSchema = EntityBaseSchema.extend({
  transaction_id: UuidSchema,
  line_number: PositiveIntSchema,
  product_id: UuidSchema,
  product_name: z.string().min(1).max(120),
  sku: z.string().max(64).nullable(),
  unit_price: NonNegativeMinorUnitsSchema,
  quantity_milli: QuantityMilliSchema,
  modifiers: z.array(ModifierSnapshotSchema),
  discount_amount: NonNegativeMinorUnitsSchema,
  tax_rate_bps: BasisPointsSchema,
  tax_amount: NonNegativeMinorUnitsSchema,
  line_total: NonNegativeMinorUnitsSchema,
  /** Restaurant course sequencing (1 = starters …); null elsewhere. */
  course: PositiveIntSchema.nullable(),
  note: z.string().max(200).nullable(),
});
export type TransactionItem = z.infer<typeof TransactionItemSchema>;

export const PAYMENT_METHODS = ['cash', 'card', 'wallet', 'loyalty', 'voucher'] as const;
export const PaymentMethodSchema = z.enum(PAYMENT_METHODS);
export type PaymentMethod = z.infer<typeof PaymentMethodSchema>;

/**
 * `transaction_payments` — one row per tender. `amount` is in the
 * transaction's base currency; foreign tenders keep the exact rate used.
 */
export const TransactionPaymentSchema = EntityBaseSchema.extend({
  transaction_id: UuidSchema,
  method: PaymentMethodSchema,
  amount: MinorUnitsSchema,
  tendered_currency: CurrencyCodeSchema,
  tendered_amount: MinorUnitsSchema,
  rate_numerator: PositiveIntSchema.nullable(),
  rate_denominator: PositiveIntSchema.nullable(),
  change_given: NonNegativeMinorUnitsSchema,
  reference: z.string().max(64).nullable(),
});
export type TransactionPayment = z.infer<typeof TransactionPaymentSchema>;

/**
 * `shifts` — cash float reconciliation.
 * - `opening_float`: cash in the drawer at open.
 * - `expected_cash`: opening_float + cash sales − cash refunds − payouts.
 * - `actual_cash`: counted at close.
 * - `variance`: actual_cash − expected_cash (negative = shortage).
 * - `closing_float`: cash left in the drawer for the next shift.
 */
export const ShiftSchema = EntityBaseSchema.extend({
  device_id: UuidSchema,
  opened_by: UuidSchema,
  closed_by: UuidSchema.nullable(),
  opened_at: TimestampSchema,
  closed_at: TimestampSchema.nullable(),
  opening_float: NonNegativeMinorUnitsSchema,
  closing_float: NonNegativeMinorUnitsSchema.nullable(),
  expected_cash: MinorUnitsSchema.nullable(),
  actual_cash: NonNegativeMinorUnitsSchema.nullable(),
  variance: MinorUnitsSchema.nullable(),
  notes: z.string().max(500).nullable(),
});
export type Shift = z.infer<typeof ShiftSchema>;
