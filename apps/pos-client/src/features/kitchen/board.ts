import type { KitchenTicket } from '@pos/shared';

/** Minutes since the ticket was sent, on the till's clock. */
export function minutesOpen(ticket: KitchenTicket, nowMs: number): number {
  return Math.max(0, Math.floor((nowMs - Date.parse(ticket.fired_at)) / 60_000));
}

/** Late tickets change colour: under 10 minutes fine, then warn, then late. */
export function urgency(minutes: number): 'ok' | 'warn' | 'late' {
  if (minutes >= 20) return 'late';
  if (minutes >= 10) return 'warn';
  return 'ok';
}

export interface AllDayLine {
  key: string;
  name: string;
  modifiers: string[];
  quantity_milli: number;
}

/** What is still to make across the open tickets (voids excluded). */
export function allDay(tickets: readonly KitchenTicket[]): AllDayLine[] {
  const lines = new Map<string, AllDayLine>();
  for (const ticket of tickets) {
    if (ticket.kind !== 'order') continue;
    for (const item of ticket.items) {
      if (item.done_at !== null) continue;
      const key = [item.name, ...item.modifiers].join('\u0000');
      const line = lines.get(key);
      if (line) line.quantity_milli += item.quantity_milli;
      else
        lines.set(key, {
          key,
          name: item.name,
          modifiers: item.modifiers,
          quantity_milli: item.quantity_milli,
        });
    }
  }
  return [...lines.values()].sort(
    (a, b) => b.quantity_milli - a.quantity_milli || a.name.localeCompare(b.name),
  );
}

/** Ticket ids that were not on the previous board (for the chime). */
export function arrived(previous: ReadonlySet<string> | null, open: readonly KitchenTicket[]) {
  if (previous === null) return [];
  return open.filter((t) => !previous.has(t.id));
}
