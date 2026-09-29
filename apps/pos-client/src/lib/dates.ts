/**
 * Local-day ranges for reports and history, and display formatting. Rust
 * stores UTC; the till's local midnight is computed here (the same machine,
 * the same time zone) and sent as UTC timestamps.
 */
import { TimestampSchema, type Timestamp } from '@pos/shared';

export const RANGE_PRESETS = ['today', 'yesterday', 'last7', 'last30', 'month'] as const;
export type RangePreset = (typeof RANGE_PRESETS)[number];

export interface LocalRange {
  from: Timestamp;
  to: Timestamp;
}

const stamp = (date: Date): Timestamp => TimestampSchema.parse(date.toISOString());

function midnight(date: Date, addDays = 0): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + addDays);
}

/** `[from, to)` of a preset, in local days, as of `now`. */
export function presetRange(preset: RangePreset, now: Date = new Date()): LocalRange {
  const today = midnight(now);
  const tomorrow = midnight(now, 1);
  switch (preset) {
    case 'today':
      return { from: stamp(today), to: stamp(tomorrow) };
    case 'yesterday':
      return { from: stamp(midnight(now, -1)), to: stamp(today) };
    case 'last7':
      return { from: stamp(midnight(now, -6)), to: stamp(tomorrow) };
    case 'last30':
      return { from: stamp(midnight(now, -29)), to: stamp(tomorrow) };
    case 'month':
      return { from: stamp(new Date(now.getFullYear(), now.getMonth(), 1)), to: stamp(tomorrow) };
  }
}

/** Whole local days between two ends of a range (at least 1). */
export function dayCount(range: LocalRange): number {
  const from = new Date(range.from);
  const to = new Date(range.to);
  return Math.max(1, Math.round((midnight(to).getTime() - midnight(from).getTime()) / 86_400_000));
}

export function formatDateTime(at: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(at));
}

export function formatDate(at: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium' }).format(new Date(at));
}

export function formatTime(at: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, { timeStyle: 'short' }).format(new Date(at));
}

/** `2026-09-24` → a short local date label. */
export function formatDay(isoDate: string, locale: string): string {
  const [y, m, d] = isoDate.split('-').map(Number);
  return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short' }).format(
    new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1),
  );
}
