import { describe, expect, it } from 'vitest';
import fixture from '../../contracts/pos-examples.json';
import { CategorySchema, ProductSchema } from '../entities/catalog';
import { UserSchema } from '../entities/people';
import { SaleReceiptSchema } from '../ipc/pos-contract';
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

  it('pins the sale arithmetic Rust produced', () => {
    const receipt = SaleReceiptSchema.parse(fixture.examples.sale_receipt);
    expect(receipt.total).toBe(2_500);
    expect(receipt.change_due).toBe(2_500);
    expect(receipt.payments[0]?.amount).toBe(2_500);
  });
});
