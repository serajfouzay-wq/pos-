/**
 * IPC contract of the POS client (`apps/pos-client/src-tauri`).
 *
 * Every command except `app_info` requires an authenticated session and a
 * valid license, and checks the caller's role in Rust before doing anything.
 */
import { z } from 'zod';
import { CurrencyCodeSchema } from '../currency';
import { CategorySchema, ProductSchema } from '../entities/catalog';
import { UserSchema } from '../entities/people';
import {
  ModifierSnapshotSchema,
  OrderTypeSchema,
  PaymentMethodSchema,
  TransactionKindSchema,
} from '../entities/sales';
import { ActivationRequestInfoSchema, LicenseStatusSchema } from '../license';
import { MinorUnitsSchema, NonNegativeMinorUnitsSchema } from '../money';
import {
  BasisPointsSchema,
  NonNegativeIntSchema,
  PositiveIntSchema,
  QuantityMilliSchema,
  TimestampSchema,
  UuidSchema,
} from '../primitives';
import { SyncReportSchema, SyncStatusSchema } from '../sync';
import { PosAppInfoSchema } from './app-info';
import { command } from './contract';
import {
  DiscoveredPrinterSchema,
  DisplayNameSchema,
  LoginUserSchema,
  PrinterSettingsSchema,
  PrinterStatusSchema,
  PrinterTargetSchema,
  PrintOutcomeSchema,
  ProductInputSchema,
  QuoteRequestSchema,
  QuoteSchema,
  SessionSchema,
  SessionStatusSchema,
  ShiftSummarySchema,
} from './pos-types';
import { PinSchema, RoleSchema } from '../rbac';

// ── create_transaction ─────────────────────────────────────────────────────

/**
 * Intentionally carries NO prices, names or totals: Rust looks every product up,
 * snapshots its current price and computes all money. The UI cannot influence
 * what an item costs.
 */
export const TransactionPayloadSchema = z.object({
  idempotency_key: UuidSchema,
  customer_id: UuidSchema.nullable(),
  order_type: OrderTypeSchema,
  table_label: z.string().max(32).nullable(),
  items: z
    .array(
      z.object({
        product_id: UuidSchema,
        quantity_milli: QuantityMilliSchema,
        modifier_ids: z.array(UuidSchema),
        course: PositiveIntSchema.nullable(),
        note: z.string().max(200).nullable(),
      }),
    )
    .min(1),
  /** Discount rules to apply; Rust re-checks eligibility and the caller's permission. */
  discount_rule_ids: z.array(UuidSchema),
  loyalty_points_to_redeem: NonNegativeIntSchema,
  payments: z
    .array(
      z.object({
        method: PaymentMethodSchema,
        tendered_currency: CurrencyCodeSchema,
        /** Minor units of `tendered_currency`. Rust converts using the stored rate. */
        tendered_amount: NonNegativeMinorUnitsSchema,
        reference: z.string().max(64).nullable(),
      }),
    )
    .min(1),
  notes: z.string().max(500).nullable(),
});
export type TransactionPayload = z.infer<typeof TransactionPayloadSchema>;
/** What the UI sends (plain strings; Zod brands ids on parse). */
export type TransactionPayloadInput = z.input<typeof TransactionPayloadSchema>;

export const ReceiptSchema = z.object({
  transaction_id: UuidSchema,
  kind: TransactionKindSchema,
  receipt_number: z.string(),
  issued_at: TimestampSchema,
  cashier_name: z.string(),
  customer_name: z.string().nullable(),
  currency: CurrencyCodeSchema,
  lines: z.array(
    z.object({
      name: z.string(),
      quantity_milli: QuantityMilliSchema,
      unit_price: NonNegativeMinorUnitsSchema,
      modifiers: z.array(ModifierSnapshotSchema),
      discount_amount: NonNegativeMinorUnitsSchema,
      line_total: NonNegativeMinorUnitsSchema,
    }),
  ),
  subtotal: MinorUnitsSchema,
  discount_total: MinorUnitsSchema,
  tax_lines: z.array(
    z.object({
      rate_bps: BasisPointsSchema,
      taxable_amount: MinorUnitsSchema,
      tax_amount: MinorUnitsSchema,
    }),
  ),
  total: MinorUnitsSchema,
  payments: z.array(
    z.object({
      method: PaymentMethodSchema,
      amount: MinorUnitsSchema,
      tendered_currency: CurrencyCodeSchema,
      tendered_amount: MinorUnitsSchema,
    }),
  ),
  change_due: NonNegativeMinorUnitsSchema,
  loyalty: z
    .object({
      earned: NonNegativeIntSchema,
      redeemed: NonNegativeIntSchema,
      balance: z.int(),
    })
    .nullable(),
  /** `false` when the printer was unreachable and the job sits in the offline print queue. */
  printed: z.boolean(),
});
export type Receipt = z.infer<typeof ReceiptSchema>;

/** `create_transaction` result: the receipt plus what the hardware did. */
export const SaleReceiptSchema = ReceiptSchema.extend({
  /** Cash sale and the drawer kick reached the printer. */
  drawer_opened: z.boolean(),
});
export type SaleReceipt = z.infer<typeof SaleReceiptSchema>;

// ── get_products ───────────────────────────────────────────────────────────

export const ProductFilterSchema = z.object({
  search: z.string().max(120).optional(),
  category_id: UuidSchema.optional(),
  barcode: z.string().max(64).optional(),
  low_stock_only: z.boolean().optional(),
  include_inactive: z.boolean().optional(),
  limit: z.int().min(1).max(1000).default(200),
  offset: z.int().nonnegative().default(0),
});
export type ProductFilter = z.input<typeof ProductFilterSchema>;

// ── get_dashboard_metrics ──────────────────────────────────────────────────

export const DateRangeSchema = z
  .object({ from: TimestampSchema, to: TimestampSchema })
  .refine((range) => range.from < range.to, { message: '`from` must be before `to`' });
export type DateRange = z.infer<typeof DateRangeSchema>;

const AmountCountSchema = z.object({ amount: MinorUnitsSchema, count: NonNegativeIntSchema });

export const DashboardDataSchema = z.object({
  range: DateRangeSchema,
  currency: CurrencyCodeSchema,
  gross_sales: MinorUnitsSchema,
  net_sales: MinorUnitsSchema,
  tax_total: MinorUnitsSchema,
  discount_total: MinorUnitsSchema,
  refund_total: MinorUnitsSchema,
  transaction_count: NonNegativeIntSchema,
  average_ticket: MinorUnitsSchema,
  by_payment_method: z.array(AmountCountSchema.extend({ method: PaymentMethodSchema })),
  by_hour: z.array(AmountCountSchema.extend({ hour: z.int().min(0).max(23) })),
  top_products: z.array(
    z.object({
      product_id: UuidSchema,
      name: z.string(),
      quantity_milli: z.int(),
      amount: MinorUnitsSchema,
    }),
  ),
  low_stock_count: NonNegativeIntSchema,
});
export type DashboardData = z.infer<typeof DashboardDataSchema>;

// ── contract ───────────────────────────────────────────────────────────────

const NoArgs = z.object({});

export const POS_IPC = {
  app_info: command(NoArgs, PosAppInfoSchema, 1),

  // Licensing (pre-authentication).
  /** Re-evaluates the license (signature, hardware, expiry, grace). */
  verify_license: command(NoArgs, LicenseStatusSchema, 2),
  /** The code an unlicensed till shows the operator. */
  get_activation_request: command(NoArgs, ActivationRequestInfoSchema, 2),
  /** Installs a token from the generator if it verifies for this machine. */
  activate_license: command(
    z.object({ token: z.string().min(1).max(8192) }),
    LicenseStatusSchema,
    2,
  ),

  // Sessions (pre-authentication, behind the license gate).
  session_status: command(NoArgs, SessionStatusSchema, 3),
  list_login_users: command(NoArgs, z.array(LoginUserSchema), 3),
  /** Creates the first owner; refused once any user exists. */
  bootstrap_owner: command(
    z.object({ display_name: DisplayNameSchema, pin: PinSchema }),
    SessionSchema,
    3,
  ),
  login: command(z.object({ user_id: UuidSchema, pin: PinSchema }), SessionSchema, 3),
  logout: command(NoArgs, z.null(), 3),

  // Users — user.manage.
  list_users: command(NoArgs, z.array(UserSchema), 3),
  create_user: command(
    z.object({ display_name: DisplayNameSchema, role: RoleSchema, pin: PinSchema }),
    UserSchema,
    3,
  ),

  // Catalogue — catalog.view / catalog.manage.
  get_products: command(z.object({ filter: ProductFilterSchema }), z.array(ProductSchema), 3),
  get_categories: command(NoArgs, z.array(CategorySchema), 3),
  save_product: command(z.object({ product: ProductInputSchema }), ProductSchema, 3),
  /** Starter catalogue for the business type; empty catalogue only. Returns products created. */
  load_sample_catalog: command(NoArgs, NonNegativeIntSchema, 3),

  // Shifts — sale.create (view) / shift.open / shift.close.
  current_shift: command(NoArgs, ShiftSummarySchema.nullable(), 3),
  open_shift: command(
    z.object({ opening_float: NonNegativeMinorUnitsSchema }),
    ShiftSummarySchema,
    3,
  ),
  close_shift: command(
    z.object({
      actual_cash: NonNegativeMinorUnitsSchema,
      closing_float: NonNegativeMinorUnitsSchema,
      notes: z.string().max(500).nullable(),
    }),
    ShiftSummarySchema,
    3,
  ),

  // Sales — sale.create (+ discount.apply when discounts are requested).
  /** Prices the cart exactly as `create_transaction` will; the UI never totals. */
  quote_transaction: command(z.object({ request: QuoteRequestSchema }), QuoteSchema, 3),
  create_transaction: command(
    z.object({ payload: TransactionPayloadSchema }),
    SaleReceiptSchema,
    3,
  ),
  /**
   * Takes a transaction id, not a receipt body: Rust re-renders from the stored,
   * immutable transaction. A second print is a COPY and needs `receipt.reprint`.
   */
  print_receipt: command(z.object({ transaction_id: UuidSchema }), PrintOutcomeSchema, 3),
  /** "No sale" drawer open — `drawer.kick`, audited. */
  kick_cash_drawer: command(NoArgs, z.null(), 3),

  // Printers — status: any signed-in user; configuration: settings.manage.
  printer_status: command(NoArgs, PrinterStatusSchema, 3),
  list_printers: command(NoArgs, z.array(DiscoveredPrinterSchema), 3),
  get_printer_settings: command(NoArgs, PrinterSettingsSchema, 3),
  save_printer_settings: command(
    z.object({ settings: PrinterSettingsSchema }),
    PrinterSettingsSchema,
    3,
  ),
  test_printer: command(z.object({ target: PrinterTargetSchema }), z.null(), 3),

  // Sync — any signed-in user may trigger a round or read the status.
  /** Runs a push + pull round now (the worker also runs every 60 s). */
  sync_to_cloud: command(NoArgs, SyncReportSchema, 4),
  sync_status: command(NoArgs, SyncStatusSchema, 4),
  get_dashboard_metrics: command(z.object({ range: DateRangeSchema }), DashboardDataSchema, 7),
} as const;

export type PosIpcContract = typeof POS_IPC;
