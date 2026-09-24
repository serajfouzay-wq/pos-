import { newUuid, TimestampSchema, UuidSchema, type Product } from '@pos/shared';
import { describe, expect, it } from 'vitest';
import { addLines, newLine, removeLine, setQuantity, toCartItems } from './lines';

const latte: Product = {
  id: UuidSchema.parse('00000000-0000-4000-8000-000000000001'),
  created_at: TimestampSchema.parse('2026-09-24T10:00:00.000Z'),
  updated_at: TimestampSchema.parse('2026-09-24T10:00:00.000Z'),
  deleted_at: null,
  name: 'Latte',
  name_localized: {},
  category_id: null,
  sku: null,
  barcode: null,
  price: 1500,
  cost: null,
  tax_rate_bps: 0,
  unit: 'each',
  sold_by_weight: false,
  track_stock: false,
  stock_on_hand_milli: 0,
  reorder_threshold_milli: null,
  reorder_quantity_milli: null,
  image_asset: null,
  quick_key_position: null,
  is_active: true,
};

describe('cart lines', () => {
  it('merges identical lines and keeps different options apart', () => {
    let lines = addLines([], [newLine(latte)]);
    lines = addLines(lines, [newLine(latte)]);
    expect(lines).toHaveLength(1);
    expect(lines[0]?.quantity_milli).toBe(2000);
    lines = addLines(lines, [newLine(latte, { modifier_ids: [newUuid()] })]);
    expect(lines).toHaveLength(2);
  });

  it('never merges combo lines and removes combos whole', () => {
    const combo = { combo_id: newUuid(), instance: newUuid() };
    const a = newLine(latte, { combo });
    const b = newLine({ ...latte, id: newUuid() }, { combo });
    let lines = addLines([newLine(latte)], [a, b]);
    expect(lines).toHaveLength(3);
    lines = removeLine(lines, b.line_id);
    expect(lines).toHaveLength(1);
    expect(lines[0]?.combo).toBeNull();
  });

  it('quantity 0 removes the line; items carry combo refs to Rust', () => {
    const combo = { combo_id: newUuid(), instance: newUuid() };
    const line = newLine(latte, { combo });
    expect(setQuantity([line], line.line_id, 0)).toEqual([]);
    expect(toCartItems([line])[0]?.combo).toEqual(combo);
  });
});
