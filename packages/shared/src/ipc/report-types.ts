/**
 * Phase 7 shapes: sales history, refunds and voids, X/Z reports, the
 * analytics dashboard, the audit trail and shift history. Everything is
 * computed in Rust from the immutable transaction rows.
 *
 * Signs: refund and void transactions store negative header totals and
 * payments (money going back), and positive item rows. Report figures called
 * `*_total` for refunds/voids are magnitudes (≥ 0); `net_*` figures are signed.
 */
import { z } from 'zod';
import { AuditLogEntrySchema } from '../entities/audit';
import {
  OrderTypeSchema,
  PaymentMethodSchema,
  ShiftSchema,
  TransactionKindSchema,
} from '../entities/sales';
import { CurrencyCodeSchema } from '../currency';
import { MinorUnitsSchema, NonNegativeMinorUnitsSchema } from '../money';
import {
  BasisPointsSchema,
  NonNegativeIntSchema,
  PositiveIntSchema,
  QuantityMilliSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';
import { ShiftTotalsSchema } from './pos-types';

export const DateRangeSchema = z
  .object({ from: TimestampSchema, to: TimestampSchema })
  .refine((range) => range.from < range.to, { message: '`from` must be before `to`' });
export type DateRange = z.infer<typeof DateRangeSchema>;

const PageSchema = {
  limit: z.int().min(1).max(500).default(50),
  offset: z.int().nonnegative().default(0),
};

// ── sales history ──────────────────────────────────────────────────────────

export const TransactionFilterSchema = z.object({
  from: TimestampSchema.nullable().default(null),
  to: TimestampSchema.nullable().default(null),
  /** Receipt number (part of it) or product name. */
  search: z.string().max(80).nullable().default(null),
  kind: TransactionKindSchema.nullable().default(null),
  device_id: UuidSchema.nullable().default(null),
  ...PageSchema,
});
export type TransactionFilter = z.input<typeof TransactionFilterSchema>;

export const TransactionSummarySchema = z.object({
  id: UuidSchema,
  kind: TransactionKindSchema,
  receipt_number: z.string(),
  /** Refunds and voids: the sale they reverse. */
  original_id: UuidSchema.nullable(),
  original_receipt_number: z.string().nullable(),
  occurred_at: TimestampSchema,
  device_id: UuidSchema,
  shift_id: UuidSchema,
  cashier_name: z.string(),
  approved_by_name: z.string().nullable(),
  order_type: OrderTypeSchema,
  table_label: z.string().nullable(),
  currency: CurrencyCodeSchema,
  /** Signed: negative for refunds and voids. */
  total: MinorUnitsSchema,
  line_count: NonNegativeIntSchema,
  payment_methods: z.array(PaymentMethodSchema),
  /** Sales: how much has been refunded or voided against it (magnitude). */
  reversed_total: NonNegativeMinorUnitsSchema,
  notes: z.string().nullable(),
});
export type TransactionSummary = z.infer<typeof TransactionSummarySchema>;

export const TransactionLineSchema = z.object({
  item_id: UuidSchema,
  line_number: PositiveIntSchema,
  product_id: UuidSchema,
  name: z.string(),
  quantity_milli: QuantityMilliSchema,
  unit_price: NonNegativeMinorUnitsSchema,
  line_total: NonNegativeMinorUnitsSchema,
  /** Already refunded or voided. */
  reversed_quantity_milli: NonNegativeIntSchema,
  /** What a refund can still take back. */
  refundable_quantity_milli: NonNegativeIntSchema,
  /** Weighed goods are refunded whole or by weight, not by the unit. */
  whole_units: z.boolean(),
});
export type TransactionLine = z.infer<typeof TransactionLineSchema>;

export const REFUND_METHODS = ['cash', 'card', 'wallet'] as const;
export const RefundMethodSchema = z.enum(REFUND_METHODS);

export const RefundInputSchema = z.object({
  transaction_id: UuidSchema,
  idempotency_key: UuidSchema,
  lines: z
    .array(z.object({ item_id: UuidSchema, quantity_milli: QuantityMilliSchema }))
    .min(1)
    .max(500),
  /** How the money goes back. */
  method: RefundMethodSchema,
  /** Put the returned goods back on the shelf (stock-tracked products). */
  restock: z.boolean(),
  reason: z.string().trim().min(1).max(200),
});
export type RefundInput = z.infer<typeof RefundInputSchema>;

export const RefundQuoteSchema = z.object({
  /** What the refund would pay back (magnitude), decided by the stored sale. */
  total: NonNegativeMinorUnitsSchema,
});
export type RefundQuote = z.infer<typeof RefundQuoteSchema>;

/** A void reverses a whole sale of the open shift, tender by tender. */
export const VoidInputSchema = z.object({
  transaction_id: UuidSchema,
  idempotency_key: UuidSchema,
  reason: z.string().trim().min(1).max(200),
});
export type VoidInput = z.infer<typeof VoidInputSchema>;

// ── period totals (X/Z reports and the dashboard) ──────────────────────────

const AmountCountSchema = z.object({ amount: MinorUnitsSchema, count: NonNegativeIntSchema });

export const PeriodTotalsSchema = z.object({
  sale_count: NonNegativeIntSchema,
  /** Σ sale subtotals, before discounts. */
  gross_sales: NonNegativeMinorUnitsSchema,
  discount_total: NonNegativeMinorUnitsSchema,
  refund_count: NonNegativeIntSchema,
  refund_total: NonNegativeMinorUnitsSchema,
  void_count: NonNegativeIntSchema,
  void_total: NonNegativeMinorUnitsSchema,
  /** Σ signed totals: what the shop actually kept (tax included). */
  net_sales: MinorUnitsSchema,
  tax_total: MinorUnitsSchema,
  by_tax_rate: z.array(
    z.object({
      rate_bps: BasisPointsSchema,
      taxable_amount: MinorUnitsSchema,
      tax_amount: MinorUnitsSchema,
    }),
  ),
  /** Net per tender (refunds subtract). */
  by_payment_method: z.array(AmountCountSchema.extend({ method: PaymentMethodSchema })),
  first_receipt: z.string().nullable(),
  last_receipt: z.string().nullable(),
});
export type PeriodTotals = z.infer<typeof PeriodTotalsSchema>;

export const CashSummarySchema = z.object({
  shift_count: NonNegativeIntSchema,
  opening_floats: NonNegativeMinorUnitsSchema,
  cash_sales: NonNegativeMinorUnitsSchema,
  cash_refunds: NonNegativeMinorUnitsSchema,
  expected: MinorUnitsSchema,
  /** Null while a shift of the period is still open. */
  counted: MinorUnitsSchema.nullable(),
  variance: MinorUnitsSchema.nullable(),
});
export type CashSummary = z.infer<typeof CashSummarySchema>;

export const ReportShiftSchema = z.object({
  shift_id: UuidSchema,
  opened_at: TimestampSchema,
  closed_at: TimestampSchema.nullable(),
  opened_by_name: z.string(),
  closed_by_name: z.string().nullable(),
  expected_cash: MinorUnitsSchema,
  actual_cash: MinorUnitsSchema.nullable(),
  variance: MinorUnitsSchema.nullable(),
});

export const REPORT_KINDS = ['x', 'z'] as const;

/** An X report (a look, nothing closed) or a Z report (the closing). */
export const PeriodReportSchema = z.object({
  kind: z.enum(REPORT_KINDS),
  /** Z only. */
  z_report_id: UuidSchema.nullable(),
  z_number: PositiveIntSchema.nullable(),
  device_id: UuidSchema,
  device_label: z.string(),
  period_start: TimestampSchema,
  period_end: TimestampSchema,
  generated_at: TimestampSchema,
  generated_by_name: z.string(),
  currency: CurrencyCodeSchema,
  totals: PeriodTotalsSchema,
  cash: CashSummarySchema,
  shifts: z.array(ReportShiftSchema),
  /** Running net total of this till's Z reports, this one included (X: as if run now). */
  grand_total: MinorUnitsSchema,
});
export type PeriodReport = z.infer<typeof PeriodReportSchema>;

export const ZReportSummarySchema = z.object({
  id: UuidSchema,
  device_id: UuidSchema,
  device_label: z.string(),
  z_number: PositiveIntSchema,
  period_start: TimestampSchema,
  period_end: TimestampSchema,
  run_by_name: z.string(),
  sale_count: NonNegativeIntSchema,
  net_sales: MinorUnitsSchema,
  grand_total: MinorUnitsSchema,
});
export type ZReportSummary = z.infer<typeof ZReportSummarySchema>;

/** A report sent to the receipt printer; the text is always returned. */
export const ReportPrintSchema = z.object({
  report: PeriodReportSchema,
  printed: z.boolean(),
  print_error: z.string().nullable(),
  text: z.string(),
});
export type ReportPrint = z.infer<typeof ReportPrintSchema>;

// ── analytics dashboard ────────────────────────────────────────────────────

export const DashboardRequestSchema = z.object({
  range: DateRangeSchema,
  /** One till, or the whole shop (every till's synced sales). */
  device_id: UuidSchema.nullable(),
});
export type DashboardRequest = z.infer<typeof DashboardRequestSchema>;

export const DashboardDataSchema = z.object({
  range: DateRangeSchema,
  currency: CurrencyCodeSchema,
  totals: PeriodTotalsSchema,
  /** Σ sale totals ÷ number of sales, rounded (0 without sales). */
  average_ticket: MinorUnitsSchema,
  /** 24 entries, local time of the till. */
  by_hour: z.array(AmountCountSchema.extend({ hour: z.int().min(0).max(23) })),
  /** Every local day of the range, oldest first. */
  by_day: z.array(AmountCountSchema.extend({ date: z.iso.date() })),
  top_products: z.array(
    z.object({
      product_id: UuidSchema,
      name: z.string(),
      quantity_milli: z.int(),
      amount: MinorUnitsSchema,
    }),
  ),
  by_category: z.array(
    z.object({
      category_id: UuidSchema.nullable(),
      name: z.string().nullable(),
      quantity_milli: z.int(),
      amount: MinorUnitsSchema,
    }),
  ),
  by_cashier: z.array(AmountCountSchema.extend({ user_id: UuidSchema, name: z.string() })),
  by_order_type: z.array(AmountCountSchema.extend({ order_type: OrderTypeSchema })),
  /** The same length of time just before `range`, for comparison. */
  previous: z.object({ net_sales: MinorUnitsSchema, sale_count: NonNegativeIntSchema }),
  low_stock_count: NonNegativeIntSchema,
  /** The till asking, and every till that has sales in the shop (for the filter). */
  this_device_id: UuidSchema,
  devices: z.array(z.object({ device_id: UuidSchema, label: z.string() })),
});
export type DashboardData = z.infer<typeof DashboardDataSchema>;

// ── audit trail ────────────────────────────────────────────────────────────

export const AuditFilterSchema = z.object({
  from: TimestampSchema.nullable().default(null),
  to: TimestampSchema.nullable().default(null),
  /** Exact action (`sale.refund`) or a prefix ending in a dot (`sale.`). */
  action: z.string().max(64).nullable().default(null),
  user_id: UuidSchema.nullable().default(null),
  ...PageSchema,
});
export type AuditFilter = z.input<typeof AuditFilterSchema>;

export const AuditEntryViewSchema = AuditLogEntrySchema.extend({
  user_name: z.string().nullable(),
});
export type AuditEntryView = z.infer<typeof AuditEntryViewSchema>;

export const AuditPageSchema = z.object({
  entries: z.array(AuditEntryViewSchema),
  total: NonNegativeIntSchema,
  /** Every action that occurs in the log, for the filter. */
  actions: z.array(z.string()),
});
export type AuditPage = z.infer<typeof AuditPageSchema>;

// ── shift history ──────────────────────────────────────────────────────────

export const ShiftFilterSchema = z.object({
  from: TimestampSchema.nullable().default(null),
  to: TimestampSchema.nullable().default(null),
  device_id: UuidSchema.nullable().default(null),
  ...PageSchema,
});
export type ShiftFilter = z.input<typeof ShiftFilterSchema>;

export const ShiftHistoryItemSchema = z.object({
  shift: ShiftSchema,
  device_label: z.string(),
  opened_by_name: z.string(),
  closed_by_name: z.string().nullable(),
  totals: ShiftTotalsSchema,
  /** Stored at close; while open, the float plus net cash so far. */
  expected_cash: MinorUnitsSchema,
});
export type ShiftHistoryItem = z.infer<typeof ShiftHistoryItemSchema>;
