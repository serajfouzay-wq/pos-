import { describe, expect, it } from 'vitest';
import fixture from '../../contracts/pos-examples.json';
import { CategorySchema, ProductSchema } from '../entities/catalog';
import { UserSchema } from '../entities/people';
import { PaidOrderSchema, SaleReceiptSchema } from '../ipc/pos-contract';
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

  it('pins the sale arithmetic Rust produced', () => {
    const receipt = SaleReceiptSchema.parse(fixture.examples.sale_receipt);
    expect(receipt.total).toBe(2_500);
    expect(receipt.change_due).toBe(2_500);
    expect(receipt.payments[0]?.amount).toBe(2_500);
  });
});
