/**
 * Plain helpers for the discount editor: percentages as basis points, clock
 * times as minutes after midnight, days as a bit mask (Monday = bit 0), and
 * date inputs as local-midnight timestamps. Integers only; no money here.
 */
import { TimestampSchema, type Timestamp } from '@pos/shared';

export const EVERY_DAY = 0b111_1111;
export const WEEKDAYS = 0b001_1111;
export const WEEKEND = 0b110_0000;

/** `"12.5"` → 1250 bps; null when it is not a percentage from 0.01 to 100. */
export function parsePercent(text: string): number | null {
  const match = /^\s*(\d{1,3})(?:[.,](\d{1,2}))?\s*%?\s*$/.exec(text);
  if (!match) return null;
  const whole = Number(match[1]);
  const frac = Number((match[2] ?? '').padEnd(2, '0'));
  const bps = whole * 100 + frac;
  return bps >= 1 && bps <= 10_000 ? bps : null;
}

/** 1250 → `"12.5"`. */
export function formatPercent(bps: number): string {
  const whole = Math.trunc(bps / 100);
  const frac = bps % 100;
  if (frac === 0) return String(whole);
  return `${String(whole)}.${String(frac).padStart(2, '0').replace(/0$/, '')}`;
}

/** 990 → `"16:30"`; 1440 → `"24:00"`. */
export function minutesToTime(minutes: number): string {
  const h = Math.trunc(minutes / 60);
  const m = minutes % 60;
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`;
}

/** `"16:30"` → 990. An end time of 00:00 means midnight at the end of the day (1440). */
export function timeToMinutes(text: string, end = false): number | null {
  const match = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  if (!match) return null;
  const minutes = Number(match[1]) * 60 + Number(match[2]);
  if (minutes >= 1440 || Number(match[2]) >= 60) return null;
  return end && minutes === 0 ? 1440 : minutes;
}

/** Short weekday names in `locale`, Monday first. */
export function dayNames(locale: string): string[] {
  const format = new Intl.DateTimeFormat(locale, { weekday: 'short' });
  // 2024-01-01 was a Monday.
  return Array.from({ length: 7 }, (_, i) => format.format(new Date(2024, 0, 1 + i)));
}

/** Local midnight of a `YYYY-MM-DD` date input (plus `addDays`). */
export function dateToTimestamp(value: string, addDays = 0): Timestamp | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]) + addDays);
  return TimestampSchema.parse(date.toISOString());
}

/** A timestamp as a `YYYY-MM-DD` date input, local time (minus `minusDays`). */
export function timestampToDate(ts: Timestamp | null, minusDays = 0): string {
  if (!ts) return '';
  const d = new Date(ts);
  d.setDate(d.getDate() - minusDays);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${String(d.getFullYear())}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** `"Mon–Fri"`, `"Sat, Sun"`, or null for every day. */
export function daysSummary(mask: number | null, names: readonly string[]): string | null {
  if (mask === null || mask === EVERY_DAY) return null;
  const on = names.map((_, i) => (mask & (1 << i)) !== 0);
  // A single run of consecutive days reads as a range.
  const first = on.indexOf(true);
  const last = on.lastIndexOf(true);
  const run = first >= 0 && on.slice(first, last + 1).every(Boolean);
  if (run && last - first >= 2) return `${names[first] ?? ''}–${names[last] ?? ''}`;
  return names.filter((_, i) => on[i]).join(', ');
}
