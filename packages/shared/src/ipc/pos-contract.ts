/**
 * IPC contract of the POS client (`apps/pos-client/src-tauri`).
 *
 * Every command except `app_info` requires an authenticated session and a
 * valid license, and checks the caller's role in Rust before doing anything.
 */
import { z } from 'zod';
import { CurrencyCodeSchema } from '../currency';
import { CategorySchema, DiscountRuleSchema, ProductSchema } from '../entities/catalog';
import { DiscountRuleInputSchema, DiscountRuleViewSchema } from './discount-types';
import { MembershipPlanSchema, MembershipSchema } from '../entities/membership';
import {
  GrantMembershipSchema,
  MemberFilterSchema,
  MemberRowSchema,
  MembershipPlanInputSchema,
  MembershipPlanViewSchema,
} from './membership-types';
import {
  BackupInfoSchema,
  BackupSettingsSchema,
  BackupStatusSchema,
  RestoredBackupSchema,
  RestoreRequestSchema,
} from './backup-types';
import { FoundHubSchema, HubHelloSchema, LanSettingsSchema, LanStatusSchema } from './lan-types';
import { KitchenTicketSchema } from '../entities/kitchen';
import { LoyaltySettingsSchema } from '../entities/shop';
import { CustomerSchema, UserSchema } from '../entities/people';
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
import { LocaleSchema } from '../i18n';
import { DiningTableSchema } from '../entities/menu';
import { PosAppInfoSchema } from './app-info';
import {
  ComboInputSchema,
  DiningTableInputSchema,
  FireOutcomeSchema,
  MenuSchema,
  ModifierGroupInputSchema,
  OpenOrderInputSchema,
  OpenOrderUpdateSchema,
  OpenOrderViewSchema,
  StockAdjustmentSchema,
} from './layout-types';
import { command } from './contract';
import {
  AuditFilterSchema,
  AuditPageSchema,
  DashboardDataSchema,
  DashboardRequestSchema,
  PeriodReportSchema,
  RefundInputSchema,
  RefundQuoteSchema,
  ReportPrintSchema,
  ShiftFilterSchema,
  ShiftHistoryItemSchema,
  TransactionFilterSchema,
  TransactionLineSchema,
  TransactionSummarySchema,
  VoidInputSchema,
  ZReportSummarySchema,
} from './report-types';
import {
  DiscoveredPrinterSchema,
  DisplayNameSchema,
  LoginUserSchema,
  PaperWidthSchema,
  PrinterSettingsSchema,
  PrinterStatusSchema,
  PrinterTargetSchema,
  PrintModeSchema,
  PrintOutcomeSchema,
  ProductInputSchema,
  QuoteRequestSchema,
  QuoteSchema,
  SessionSchema,
  SessionStatusSchema,
  ShiftSummarySchema,
} from './pos-types';
import { PinSchema, RoleSchema } from '../rbac';
import { KitchenBoardSchema, KitchenDisplayStatusSchema } from './kitchen-types';
import {
  CustomerDetailSchema,
  CustomerInputSchema,
  CustomerSearchSchema,
  LoyaltyProgramSchema,
  PointsAdjustmentSchema,
} from './loyalty-types';
import { UpdateFileInfoSchema, UpdateStatusSchema } from './updater-types';

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
  /** The membership the customer held (or just bought) when a sale was made. */
  member: z
    .object({
      plan_name: z.string(),
      card_number: z.string(),
      ends_at: TimestampSchema,
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
  /** A receipt waits in the print queue (false when receipts print on request). */
  print_queued: z.boolean(),
});
export type SaleReceipt = z.infer<typeof SaleReceiptSchema>;

/** `pay_open_order`: the sale, and what is left of the order (null once settled). */
export const PaidOrderSchema = z.object({
  sale: SaleReceiptSchema,
  order: OpenOrderViewSchema.nullable(),
});
export type PaidOrder = z.infer<typeof PaidOrderSchema>;

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

/** `get_transaction`: a stored sale, refund or void with what can still be reversed. */
export const TransactionDetailSchema = z.object({
  summary: TransactionSummarySchema,
  receipt: ReceiptSchema,
  lines: z.array(TransactionLineSchema),
  /** Refunds and voids of this sale, oldest first. */
  reversals: z.array(TransactionSummarySchema),
  can_refund: z.boolean(),
  /** Why a void is not possible (null = it is). */
  void_blocker: z.string().nullable(),
});
export type TransactionDetail = z.infer<typeof TransactionDetailSchema>;

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
  /** Saves the activation code as a `.posactivate` file (USB stick); returns its path. */
  save_activation_file: command(z.object({ path: z.string().min(1).max(4096) }), z.string(), 10),
  /** `.poslicense` files on USB sticks and in Downloads, newest first. */
  find_license_files: command(NoArgs, z.array(z.string()), 10),
  /** Activates with the license in a `.poslicense` file. */
  activate_license_file: command(
    z.object({ path: z.string().min(1).max(4096) }),
    LicenseStatusSchema,
    10,
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
  /** A test page; the optional settings are tried before they are saved. */
  test_printer: command(
    z.object({
      target: PrinterTargetSchema,
      language: LocaleSchema.nullable().optional(),
      mode: PrintModeSchema.nullable().optional(),
      paper_width_mm: PaperWidthSchema.nullable().optional(),
    }),
    z.null(),
    3,
  ),

  // Sync — any signed-in user may trigger a round or read the status.
  /** Runs a push + pull round now (the worker also runs every 60 s). */
  sync_to_cloud: command(NoArgs, SyncReportSchema, 4),
  sync_status: command(NoArgs, SyncStatusSchema, 4),

  // Menu & floor — read: catalog.view (inactive rows: catalog.manage); edit: catalog.manage.
  get_menu: command(z.object({ include_inactive: z.boolean() }), MenuSchema, 6),
  save_modifier_group: command(z.object({ group: ModifierGroupInputSchema }), UuidSchema, 6),
  delete_modifier_group: command(z.object({ group_id: UuidSchema }), z.null(), 6),
  set_product_modifier_groups: command(
    z.object({ product_id: UuidSchema, group_ids: z.array(UuidSchema).max(10) }),
    z.null(),
    6,
  ),
  save_combo: command(z.object({ combo: ComboInputSchema }), UuidSchema, 6),
  delete_combo: command(z.object({ combo_id: UuidSchema }), z.null(), 6),
  save_dining_table: command(z.object({ table: DiningTableInputSchema }), DiningTableSchema, 6),
  delete_dining_table: command(z.object({ table_id: UuidSchema }), z.null(), 6),

  // Open orders (tabs, tables) — sale.create; changing sent items: sale.void.
  list_open_orders: command(NoArgs, z.array(OpenOrderViewSchema), 6),
  open_order: command(z.object({ input: OpenOrderInputSchema }), OpenOrderViewSchema, 6),
  update_open_order: command(z.object({ input: OpenOrderUpdateSchema }), OpenOrderViewSchema, 6),
  /** A line of N whole items → N lines of one (to pay them separately). */
  split_order_line: command(
    z.object({ order_id: UuidSchema, line_id: UuidSchema, expected_updated_at: z.string() }),
    OpenOrderViewSchema,
    6,
  ),
  /** Sends unsent lines of `course` (null = all unsent) to the kitchen. */
  fire_course: command(
    z.object({
      order_id: UuidSchema,
      course: z.int().min(1).max(9).nullable(),
      expected_updated_at: z.string(),
    }),
    FireOutcomeSchema,
    6,
  ),
  cancel_open_order: command(
    z.object({ order_id: UuidSchema, expected_updated_at: z.string() }),
    z.null(),
    6,
  ),
  /** Pays the chosen lines (split bill) or everything left (`line_ids: null`). */
  pay_open_order: command(
    z.object({
      input: z.object({
        order_id: UuidSchema,
        idempotency_key: UuidSchema,
        line_ids: z.array(UuidSchema).min(1).nullable(),
        discount_rule_ids: z.array(UuidSchema),
        customer_id: UuidSchema.nullable().default(null),
        loyalty_points_to_redeem: NonNegativeIntSchema.default(0),
        payments: TransactionPayloadSchema.shape.payments,
      }),
    }),
    PaidOrderSchema,
    6,
  ),

  // Stock & labels — inventory.adjust / inventory.view.
  adjust_stock: command(z.object({ adjustment: StockAdjustmentSchema }), ProductSchema, 6),
  print_product_labels: command(
    z.object({ product_id: UuidSchema, copies: z.int().min(1).max(50) }),
    z.null(),
    6,
  ),

  // Sales history — receipt.reprint; refunds: sale.refund; voids: sale.void.
  list_transactions: command(
    z.object({ filter: TransactionFilterSchema }),
    z.array(TransactionSummarySchema),
    7,
  ),
  get_transaction: command(z.object({ transaction_id: UuidSchema }), TransactionDetailSchema, 7),
  /** What `refund_transaction` would pay back, without doing it. */
  quote_refund: command(z.object({ input: RefundInputSchema }), RefundQuoteSchema, 7),
  /** Takes back chosen quantities of a sale; prints a refund receipt. */
  refund_transaction: command(z.object({ input: RefundInputSchema }), SaleReceiptSchema, 7),
  /** Reverses a whole sale of the open shift, tender by tender. */
  void_transaction: command(z.object({ input: VoidInputSchema }), SaleReceiptSchema, 7),

  // Reports — X / history: report.view; running a Z: report.z_run.
  /** This till since its last Z, closing nothing. */
  get_x_report: command(NoArgs, PeriodReportSchema, 7),
  /** Closes the period (every shift of this till must be closed) and prints it. */
  run_z_report: command(NoArgs, ReportPrintSchema, 7),
  list_z_reports: command(
    z.object({
      device_id: UuidSchema.nullable(),
      limit: z.int().min(1).max(500),
      offset: z.int().nonnegative(),
    }),
    z.array(ZReportSummarySchema),
    7,
  ),
  get_z_report: command(z.object({ z_report_id: UuidSchema }), PeriodReportSchema, 7),
  /** Prints a stored Z again (identical), or the X report now (`null`). */
  print_report: command(z.object({ z_report_id: UuidSchema.nullable() }), ReportPrintSchema, 7),
  list_shifts: command(z.object({ filter: ShiftFilterSchema }), z.array(ShiftHistoryItemSchema), 7),

  // Analytics — analytics.view. Audit trail — audit.view.
  get_dashboard_metrics: command(
    z.object({ request: DashboardRequestSchema }),
    DashboardDataSchema,
    7,
  ),
  list_audit_log: command(z.object({ filter: AuditFilterSchema }), AuditPageSchema, 7),

  // Customers — lookup and registering: customer.lookup; editing, removing and
  // adjusting points: customer.manage. Loyalty settings: settings.manage.
  search_customers: command(z.object({ search: CustomerSearchSchema }), z.array(CustomerSchema), 8),
  get_customer: command(z.object({ customer_id: UuidSchema }), CustomerDetailSchema, 8),
  save_customer: command(z.object({ customer: CustomerInputSchema }), CustomerSchema, 8),
  delete_customer: command(z.object({ customer_id: UuidSchema }), z.null(), 8),
  adjust_loyalty_points: command(
    z.object({ adjustment: PointsAdjustmentSchema }),
    CustomerSchema,
    8,
  ),
  get_loyalty_settings: command(NoArgs, LoyaltyProgramSchema, 8),
  save_loyalty_settings: command(
    z.object({ settings: LoyaltySettingsSchema }),
    LoyaltyProgramSchema,
    8,
  ),

  // Kitchen display — the window: settings.manage; the board: the kitchen
  // window itself (no sign-in there) or sale.create.
  kitchen_display_status: command(NoArgs, KitchenDisplayStatusSchema, 8),
  /** Shows (and reopens at every start) or closes this till's kitchen window. */
  set_kitchen_display: command(z.object({ enabled: z.boolean() }), KitchenDisplayStatusSchema, 8),
  list_kitchen_tickets: command(
    z.object({ recent_minutes: z.int().min(0).max(240) }),
    KitchenBoardSchema,
    8,
  ),
  /** `ready: true` bumps the ticket off the board; `false` recalls it. */
  bump_kitchen_ticket: command(
    z.object({ ticket_id: UuidSchema, ready: z.boolean() }),
    KitchenTicketSchema,
    8,
  ),
  set_kitchen_item_done: command(
    z.object({ ticket_id: UuidSchema, line_id: UuidSchema, done: z.boolean() }),
    KitchenTicketSchema,
    8,
  ),

  // Updates — status/check: any signed-in user; installing now restarts the
  // till: shift.close. Otherwise a downloaded update installs at the next start.
  update_status: command(NoArgs, UpdateStatusSchema, 8),
  check_for_updates: command(NoArgs, UpdateStatusSchema, 8),
  install_update: command(NoArgs, z.null(), 8),
  dismiss_update_notice: command(NoArgs, UpdateStatusSchema, 8),
  /** `.posupdate` files on USB sticks and in Downloads, newest first: shift.close. */
  find_update_files: command(NoArgs, z.array(z.string()), 9),
  /** Checks a `.posupdate` file (signature, shop, system, version): shift.close. */
  inspect_update_file: command(
    z.object({ path: z.string().min(1).max(4096) }),
    UpdateFileInfoSchema,
    9,
  ),
  /** Backs up, starts the file's installer and closes the till: shift.close. */
  install_update_file: command(z.object({ path: z.string().min(1).max(4096) }), z.null(), 9),

  // Discount rules — the list: anyone selling (manual rules are offered to
  // managers at the till); changes: catalog.manage, audited.
  list_discount_rules: command(NoArgs, z.array(DiscountRuleViewSchema), 9),
  save_discount_rule: command(z.object({ rule: DiscountRuleInputSchema }), DiscountRuleSchema, 9),
  delete_discount_rule: command(z.object({ rule_id: UuidSchema }), z.null(), 9),

  // Memberships — plans and members: customer.lookup (the till shows a
  // customer's membership); plans, grants and cancellations: customer.manage.
  // Selling a plan is an ordinary sale of its product.
  list_membership_plans: command(NoArgs, z.array(MembershipPlanViewSchema), 9),
  save_membership_plan: command(
    z.object({ plan: MembershipPlanInputSchema }),
    MembershipPlanSchema,
    9,
  ),
  delete_membership_plan: command(z.object({ plan_id: UuidSchema }), z.null(), 9),
  list_members: command(z.object({ filter: MemberFilterSchema }), z.array(MemberRowSchema), 9),
  customer_memberships: command(z.object({ customer_id: UuidSchema }), z.array(MemberRowSchema), 9),
  grant_membership: command(z.object({ grant: GrantMembershipSchema }), MembershipSchema, 9),
  cancel_membership: command(z.object({ membership_id: UuidSchema }), MembershipSchema, 9),

  // Backups — status, settings, password, restore: settings.manage; a backup
  // now: shift.close. Restoring works without sign-in only when the
  // database cannot be opened at all (storage error).
  backup_status: command(NoArgs, BackupStatusSchema, 9),
  backup_now: command(NoArgs, BackupInfoSchema, 9),
  save_backup_settings: command(
    z.object({ settings: BackupSettingsSchema }),
    BackupStatusSchema,
    9,
  ),
  set_backup_password: command(
    z.object({ password: z.string().min(6).max(200) }),
    BackupStatusSchema,
    9,
  ),
  list_backups_in: command(z.object({ dir: z.string().min(1) }), z.array(BackupInfoSchema), 9),
  /** Stages the backup; `restart_app` puts it in place. */
  restore_backup: command(z.object({ request: RestoreRequestSchema }), RestoredBackupSchema, 9),
  restart_app: command(NoArgs, z.null(), 9),

  // Shop network (LAN hub) — per till, settings.manage.
  lan_status: command(NoArgs, LanStatusSchema, 9),
  /** Saves and applies at once (starts/stops the hub, retargets sync). */
  save_lan_settings: command(z.object({ settings: LanSettingsSchema }), LanStatusSchema, 9),
  discover_hubs: command(z.object({ port: z.int() }), z.array(FoundHubSchema), 9),
  test_hub: command(
    z.object({ address: z.string().min(1), code: z.string().min(1), port: z.int() }),
    HubHelloSchema,
    9,
  ),
  new_hub_code: command(NoArgs, LanStatusSchema, 9),
} as const;

export type PosIpcContract = typeof POS_IPC;
