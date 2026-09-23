import { describe, expect, it } from 'vitest';
import exampleConfig from '../../contracts/client-config.example.json';
import { ClientConfigSchema } from '../client-config';
import { textDirection } from '../i18n';
import { SyncEventSchema, SYNC_ENTITY_STRATEGY, expectedEventType } from '../sync';
import { TimestampSchema } from '../primitives';
import { TransactionSchema } from '../entities/sales';

const uuid = (n: number) => `00000000-0000-4000-8000-${n.toString().padStart(12, '0')}`;
const ts = '2026-09-23T10:15:30.123Z';

describe('ClientConfigSchema', () => {
  it('accepts the example config shared with Rust', () => {
    expect(ClientConfigSchema.safeParse(exampleConfig).success).toBe(true);
  });

  it('rejects a default locale that is not supported', () => {
    const bad = { ...exampleConfig, locale: { default: 'ar', supported: ['en'] } };
    expect(ClientConfigSchema.safeParse(bad).success).toBe(false);
  });
});

describe('TimestampSchema', () => {
  it('requires UTC with millisecond precision (lexically sortable)', () => {
    expect(TimestampSchema.safeParse(ts).success).toBe(true);
    expect(TimestampSchema.safeParse('2026-09-23T10:15:30Z').success).toBe(false);
    expect(TimestampSchema.safeParse('2026-09-23T10:15:30.123+03:00').success).toBe(false);
  });
});

describe('TransactionSchema', () => {
  const base = {
    id: uuid(1),
    created_at: ts,
    updated_at: ts,
    deleted_at: null,
    kind: 'sale',
    original_transaction_id: null,
    receipt_number: 'D01-000001',
    device_id: uuid(2),
    shift_id: uuid(3),
    cashier_id: uuid(4),
    approved_by: null,
    customer_id: null,
    order_type: 'counter',
    table_label: null,
    currency: 'KWD',
    subtotal: 1500,
    discount_total: 0,
    tax_total: 0,
    total: 1500,
    loyalty_points_earned: 15,
    loyalty_points_redeemed: 0,
    notes: null,
    idempotency_key: uuid(5),
    occurred_at: ts,
  };

  it('accepts integer money', () => {
    expect(TransactionSchema.safeParse(base).success).toBe(true);
  });

  it('rejects floating-point money', () => {
    expect(TransactionSchema.safeParse({ ...base, total: 15.0001 }).success).toBe(false);
  });

  it('requires refunds to reference an original transaction', () => {
    expect(TransactionSchema.safeParse({ ...base, kind: 'refund' }).success).toBe(false);
    expect(
      TransactionSchema.safeParse({ ...base, kind: 'refund', original_transaction_id: uuid(9) })
        .success,
    ).toBe(true);
  });
});

describe('sync protocol', () => {
  const row = { id: uuid(7), created_at: ts, updated_at: ts, deleted_at: null, name: 'Latte' };
  const event = {
    event_id: uuid(6),
    device_id: uuid(2),
    event_type: 'upsert',
    entity_type: 'products',
    entity_id: uuid(7),
    payload: row,
    occurred_at: ts,
  };

  it('maps strategies to event types', () => {
    expect(expectedEventType('products')).toBe('upsert');
    expect(expectedEventType('transactions')).toBe('append');
    expect(expectedEventType('stock_movements')).toBe('append');
    expect(SYNC_ENTITY_STRATEGY.stock_movements).toBe('additive_delta');
  });

  it('accepts a well-formed LWW upsert and keeps extra row columns', () => {
    const parsed = SyncEventSchema.parse(event);
    expect(parsed.payload['name']).toBe('Latte');
  });

  it('rejects an upsert against an append-only table', () => {
    expect(SyncEventSchema.safeParse({ ...event, entity_type: 'transactions' }).success).toBe(
      false,
    );
  });

  it('rejects a payload whose id differs from entity_id', () => {
    expect(SyncEventSchema.safeParse({ ...event, entity_id: uuid(8) }).success).toBe(false);
  });
});

describe('textDirection', () => {
  it('detects RTL locales', () => {
    expect(textDirection('ar')).toBe('rtl');
    expect(textDirection('ar-KW')).toBe('rtl');
    expect(textDirection('en')).toBe('ltr');
  });
});
