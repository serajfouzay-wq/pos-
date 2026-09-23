/**
 * Request/response shapes of the Phase 3 POS commands (sessions, catalogue,
 * shifts, sales, printers). Rust structs mirror these; the JSON fixtures in
 * `contracts/pos-examples.json` are produced by Rust and parsed here in tests.
 */
import { z } from 'zod';
import { CurrencyCodeSchema } from '../currency';
import { ProductUnitSchema } from '../entities/catalog';
import { ShiftSchema } from '../entities/sales';
import { MinorUnitsSchema, NonNegativeMinorUnitsSchema } from '../money';
import {
  BasisPointsSchema,
  NonNegativeIntSchema,
  QuantityMilliSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';
import { PermissionSchema, PinSchema, RoleSchema } from '../rbac';

// ── Sessions ───────────────────────────────────────────────────────────────

export const SessionSchema = z.object({
  user_id: UuidSchema,
  display_name: z.string(),
  role: RoleSchema,
  /** From the Rust matrix — for hiding controls only. */
  permissions: z.array(PermissionSchema),
  started_at: TimestampSchema,
});
export type Session = z.infer<typeof SessionSchema>;

export const SessionStatusSchema = z.object({
  needs_setup: z.boolean(),
  session: SessionSchema.nullable(),
});
export type SessionStatus = z.infer<typeof SessionStatusSchema>;

export const LoginUserSchema = z.object({
  id: UuidSchema,
  display_name: z.string(),
  role: RoleSchema,
  locked_until: TimestampSchema.nullable(),
});
export type LoginUser = z.infer<typeof LoginUserSchema>;

export const DisplayNameSchema = z.string().trim().min(1).max(80);

export const NewUserSchema = z.object({
  display_name: DisplayNameSchema,
  role: RoleSchema,
  pin: PinSchema,
});

// ── Catalogue ──────────────────────────────────────────────────────────────

export const ProductInputSchema = z.object({
  /** Omit to create. */
  id: UuidSchema.nullable(),
  name: z.string().trim().min(1).max(120),
  category_id: UuidSchema.nullable(),
  sku: z.string().max(64).nullable(),
  barcode: z.string().max(64).nullable(),
  price: NonNegativeMinorUnitsSchema,
  tax_rate_bps: BasisPointsSchema,
  unit: ProductUnitSchema,
  track_stock: z.boolean(),
  reorder_threshold_milli: NonNegativeIntSchema.nullable(),
  quick_key_position: NonNegativeIntSchema.nullable(),
  is_active: z.boolean(),
});
export type ProductInput = z.input<typeof ProductInputSchema>;

// ── Shifts ─────────────────────────────────────────────────────────────────

export const ShiftTotalsSchema = z.object({
  transaction_count: NonNegativeIntSchema,
  sales_total: MinorUnitsSchema,
  cash_total: MinorUnitsSchema,
  card_total: MinorUnitsSchema,
  wallet_total: MinorUnitsSchema,
});

export const ShiftSummarySchema = z.object({
  shift: ShiftSchema,
  totals: ShiftTotalsSchema,
  /** opening float + net cash taken. */
  expected_cash: MinorUnitsSchema,
});
export type ShiftSummary = z.infer<typeof ShiftSummarySchema>;

// ── Quotes ─────────────────────────────────────────────────────────────────

export const CartItemSchema = z.object({
  product_id: UuidSchema,
  quantity_milli: QuantityMilliSchema,
  modifier_ids: z.array(UuidSchema),
  course: z.int().positive().nullable(),
  note: z.string().max(200).nullable(),
});
export type CartItem = z.input<typeof CartItemSchema>;

export const QuoteRequestSchema = z.object({
  items: z.array(CartItemSchema).min(1).max(500),
  discount_rule_ids: z.array(UuidSchema),
});
export type QuoteRequest = z.input<typeof QuoteRequestSchema>;

export const QuoteSchema = z.object({
  currency: CurrencyCodeSchema,
  lines: z.array(
    z.object({
      product_id: UuidSchema,
      name: z.string(),
      quantity_milli: QuantityMilliSchema,
      unit_price: NonNegativeMinorUnitsSchema,
      tax_rate_bps: BasisPointsSchema,
      gross: NonNegativeMinorUnitsSchema,
      discount_amount: NonNegativeMinorUnitsSchema,
      tax_amount: NonNegativeMinorUnitsSchema,
      line_total: NonNegativeMinorUnitsSchema,
    }),
  ),
  subtotal: MinorUnitsSchema,
  discount_total: MinorUnitsSchema,
  tax_total: MinorUnitsSchema,
  total: MinorUnitsSchema,
  tax_lines: z.array(
    z.object({
      rate_bps: BasisPointsSchema,
      taxable_amount: MinorUnitsSchema,
      tax_amount: MinorUnitsSchema,
    }),
  ),
});
export type Quote = z.infer<typeof QuoteSchema>;

export const PrintOutcomeSchema = z.object({
  printed: z.boolean(),
  /** Still waiting in the offline print queue. */
  queued: z.boolean(),
});
export type PrintOutcome = z.infer<typeof PrintOutcomeSchema>;

// ── Printers ───────────────────────────────────────────────────────────────

export const PrinterTargetSchema = z.discriminatedUnion('kind', [
  z.object({
    kind: z.literal('tcp'),
    host: z.string().trim().min(1).max(253),
    port: z.int().min(1).max(65_535),
  }),
  z.object({
    kind: z.literal('serial'),
    port: z.string().min(1).max(64),
    baud_rate: z.int().positive(),
  }),
  z.object({ kind: z.literal('windows_printer'), name: z.string().min(1).max(256) }),
]);
export type PrinterTarget = z.infer<typeof PrinterTargetSchema>;

export const DiscoveredPrinterSchema = z.object({
  target: PrinterTargetSchema,
  connection: z.enum(['usb', 'network', 'bluetooth', 'serial']),
  label: z.string(),
});
export type DiscoveredPrinter = z.infer<typeof DiscoveredPrinterSchema>;

export const PrinterSettingsSchema = z.object({
  /** Tried in order: primary, then fallbacks. */
  chain: z.array(PrinterTargetSchema).max(3),
  open_drawer_on_cash: z.boolean(),
});
export type PrinterSettings = z.infer<typeof PrinterSettingsSchema>;

export const PrinterStatusSchema = z.object({
  configured: z.boolean(),
  /** `null` until the first print attempt. */
  online: z.boolean().nullable(),
  pending_jobs: NonNegativeIntSchema,
  last_error: z.string().nullable(),
});
export type PrinterStatus = z.infer<typeof PrinterStatusSchema>;
