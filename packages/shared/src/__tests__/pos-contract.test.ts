import { describe, expect, it } from 'vitest';
import fixture from '../../contracts/pos-examples.json';
import { CategorySchema, ProductSchema } from '../entities/catalog';
import { CustomerSchema, UserSchema } from '../entities/people';
import {
  KitchenBoardSchema,
  KitchenChangeSchema,
  KitchenDisplayStatusSchema,
} from '../ipc/kitchen-types';
import { CustomerDetailSchema, LoyaltyProgramSchema } from '../ipc/loyalty-types';
import { UpdateStatusSchema } from '../ipc/updater-types';
import { PaidOrderSchema, SaleReceiptSchema, TransactionDetailSchema } from '../ipc/pos-contract';
import {
  AuditPageSchema,
  DashboardDataSchema,
  PeriodReportSchema,
  ReportPrintSchema,
  ShiftHistoryItemSchema,
  TransactionSummarySchema,
  ZReportSummarySchema,
} from '../ipc/report-types';
import { FireOutcomeSchema, MenuSchema, OpenOrderViewSchema } from '../ipc/layout-types';
import {
  DiscoveredPrinterSchema,
  LoginUserSchema,
  PrinterSettingsSchema,
  PrinterStatusSchema,
  PrintOutcomeSchema,
  QuoteSchema,
  SessionSchema,
  SessionStatusSchema,
  ShiftSummarySchema,
} from '../ipc/pos-types';

/** Every example Rust emits must satisfy the schema the UI validates with. */
const cases = {
  session: SessionSchema,
  session_status: SessionStatusSchema,
  login_user: LoginUserSchema,
  user: UserSchema,
  product: ProductSchema,
  category: CategorySchema,
  shift_summary: ShiftSummarySchema,
  quote: QuoteSchema,
  sale_receipt: SaleReceiptSchema,
  print_outcome: PrintOutcomeSchema,
  printer_settings: PrinterSettingsSchema,
  printer_status: PrinterStatusSchema,
  discovered_printer: DiscoveredPrinterSchema,
  menu: MenuSchema,
  open_order_view: OpenOrderViewSchema,
  fire_outcome: FireOutcomeSchema,
  paid_order: PaidOrderSchema,
  transaction_detail: TransactionDetailSchema,
  transaction_summary: TransactionSummarySchema,
  period_report: PeriodReportSchema,
  report_print: ReportPrintSchema,
  z_report_summary: ZReportSummarySchema,
  dashboard_data: DashboardDataSchema,
  audit_page: AuditPageSchema,
  shift_history_item: ShiftHistoryItemSchema,
  customer: CustomerSchema,
  customer_detail: CustomerDetailSchema,
  loyalty_program: LoyaltyProgramSchema,
  loyalty_quote: QuoteSchema,
  loyalty_receipt: SaleReceiptSchema,
  kitchen_board: KitchenBoardSchema,
  kitchen_change: KitchenChangeSchema,
  kitchen_display_status: KitchenDisplayStatusSchema,
  update_status: UpdateStatusSchema,
} as const;

describe('POS response contract (Rust → Zod)', () => {
  it('covers every example in the fixture', () => {
    expect(Object.keys(fixture.examples).sort()).toEqual(Object.keys(cases).sort());
  });

  it.each(Object.entries(cases))('%s parses', (name, schema) => {
    const example = (fixture.examples as Record<string, unknown>)[name];
    const result = schema.safeParse(example);
    expect(result.error?.issues ?? []).toEqual([]);
  });

  it('pins the Phase 6 behaviour Rust produced', () => {
    const menu = MenuSchema.parse(fixture.examples.menu);
    expect(menu.combos[0]?.items).toHaveLength(2);
    const fired = FireOutcomeSchema.parse(fixture.examples.fire_outcome);
    expect(fired.order.unfired).toBe(0);
    expect(fired.ticket_text).toContain('Table T4');
    expect(fired.ticket_text).toContain('+ Oat');
    // Latte 1.250 + oat milk 0.200.
    const paid = PaidOrderSchema.parse(fixture.examples.paid_order);
    expect(paid.sale.total).toBe(1_450);
    expect(paid.sale.lines[0]?.modifiers[0]?.name).toBe('Oat');
  });

  it('pins the Phase 7 behaviour Rust produced', () => {
    // 2 lattes (2.500), one refunded in cash.
    const detail = TransactionDetailSchema.parse(fixture.examples.transaction_detail);
    expect(detail.lines[0]?.refundable_quantity_milli).toBe(1000);
    expect(detail.reversals[0]?.total).toBe(-1_250);
    expect(detail.summary.reversed_total).toBe(1_250);
    const z = PeriodReportSchema.parse(fixture.examples.period_report);
    expect(z.kind).toBe('z');
    expect(z.z_number).toBe(1);
    expect(z.totals.refund_total).toBe(1_250);
    expect(z.cash.counted).toBe(24_000);
    const printed = ReportPrintSchema.parse(fixture.examples.report_print);
    expect(printed.text).toContain('X REPORT');
    const dashboard = DashboardDataSchema.parse(fixture.examples.dashboard_data);
    expect(dashboard.by_hour).toHaveLength(24);
  });

  it('pins the Phase 8 behaviour Rust produced', () => {
    // 2 lattes (2.500) for Layla, 100 of her 500 points off (1.000).
    const quote = QuoteSchema.parse(fixture.examples.loyalty_quote);
    expect(quote.total).toBe(1_500);
    expect(quote.loyalty?.redeem_value).toBe(1_000);
    expect(quote.loyalty?.max_redeem_points).toBe(250);
    expect(quote.loyalty?.points_earned).toBe(1);
    const receipt = SaleReceiptSchema.parse(fixture.examples.loyalty_receipt);
    expect(receipt.customer_name).toBe('Layla');
    expect(receipt.loyalty).toEqual({ earned: 1, redeemed: 100, balance: 401 });
    const customer = CustomerSchema.parse(fixture.examples.customer);
    expect(customer.phone).toBe('+96555551234');
    expect(customer.loyalty_points).toBe(401);
    const detail = CustomerDetailSchema.parse(fixture.examples.customer_detail);
    expect(detail.ledger.map((l) => l.reason)).toEqual(['earn', 'redeem', 'adjust']);
    const board = KitchenBoardSchema.parse(fixture.examples.kitchen_board);
    expect(board.open[0]?.items[0]?.done_at).not.toBeNull();
    expect(board.ready[0]?.kind).toBe('void');
  });

  it('pins the sale arithmetic Rust produced', () => {
    const receipt = SaleReceiptSchema.parse(fixture.examples.sale_receipt);
    expect(receipt.total).toBe(2_500);
    expect(receipt.change_due).toBe(2_500);
    expect(receipt.payments[0]?.amount).toBe(2_500);
  });
});
