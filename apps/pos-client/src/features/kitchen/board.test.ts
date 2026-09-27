import { KitchenTicketSchema, type KitchenTicket } from '@pos/shared';
import { describe, expect, it } from 'vitest';
import { allDay, arrived, minutesOpen, urgency } from './board';

function ticket(
  id: string,
  kind: 'order' | 'void',
  items: [string, number, boolean][],
): KitchenTicket {
  return KitchenTicketSchema.parse({
    id,
    created_at: '2026-09-27T18:00:00.000Z',
    updated_at: '2026-09-27T18:00:00.000Z',
    deleted_at: null,
    device_id: '0199a000-0000-7000-8000-00000000d001',
    ticket_number: 1,
    kind,
    order_id: null,
    transaction_id: null,
    title: 'Table T4',
    order_type: 'dine_in',
    course: null,
    server_name: 'Sara',
    guests: 2,
    items: items.map(([name, qty, done], i) => ({
      line_id: `0199a000-0000-7000-8000-0000000000${String(10 + i)}`,
      quantity_milli: qty,
      name,
      modifiers: [],
      note: null,
      course: null,
      done_at: done ? '2026-09-27T18:05:00.000Z' : null,
    })),
    status: 'open',
    fired_at: '2026-09-27T18:00:00.000Z',
    ready_at: null,
  });
}

const A = '0199a000-0000-7000-8000-0000000000a1';
const B = '0199a000-0000-7000-8000-0000000000b1';

describe('kitchen board', () => {
  it('times tickets and flags late ones', () => {
    const t = ticket(A, 'order', [['Soup', 1000, false]]);
    const now = Date.parse('2026-09-27T18:12:30.000Z');
    expect(minutesOpen(t, now)).toBe(12);
    expect([urgency(3), urgency(12), urgency(25)]).toEqual(['ok', 'warn', 'late']);
  });

  it('sums what is left to make, skipping struck items and voids', () => {
    const lines = allDay([
      ticket(A, 'order', [
        ['Soup', 2000, false],
        ['Burger', 1000, true],
      ]),
      ticket(B, 'order', [['Soup', 1000, false]]),
      ticket('0199a000-0000-7000-8000-0000000000c1', 'void', [['Soup', 1000, false]]),
    ]);
    expect(lines.map((l) => [l.name, l.quantity_milli])).toEqual([['Soup', 3000]]);
  });

  it('spots new tickets but not on the first load', () => {
    const open = [
      ticket(A, 'order', [['Soup', 1000, false]]),
      ticket(B, 'order', [['Tea', 1000, false]]),
    ];
    expect(arrived(null, open)).toEqual([]);
    expect(arrived(new Set([A]), open).map((t) => t.id)).toEqual([B]);
  });
});
