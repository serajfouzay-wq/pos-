import { describe, expect, it } from 'vitest';
import {
  dateToTimestamp,
  daysSummary,
  EVERY_DAY,
  formatPercent,
  minutesToTime,
  parsePercent,
  timestampToDate,
  timeToMinutes,
  WEEKDAYS,
  WEEKEND,
} from './schedule';

describe('discount schedule helpers', () => {
  it('reads percentages as basis points', () => {
    expect(parsePercent('10')).toBe(1000);
    expect(parsePercent('12.5')).toBe(1250);
    expect(parsePercent('12,25 %')).toBe(1225);
    expect(parsePercent('100')).toBe(10_000);
    for (const bad of ['', '0', '100.01', '101', 'abc', '1.234', '-5']) {
      expect(parsePercent(bad)).toBeNull();
    }
    expect(formatPercent(1250)).toBe('12.5');
    expect(formatPercent(1225)).toBe('12.25');
    expect(formatPercent(1000)).toBe('10');
  });

  it('reads clock times as minutes, with midnight ending the day', () => {
    expect(timeToMinutes('16:30')).toBe(990);
    expect(timeToMinutes('00:00')).toBe(0);
    expect(timeToMinutes('00:00', true)).toBe(1440);
    expect(timeToMinutes('24:00')).toBeNull();
    expect(timeToMinutes('7:65')).toBeNull();
    expect(minutesToTime(990)).toBe('16:30');
    expect(minutesToTime(1440)).toBe('24:00');
  });

  it('round-trips date inputs through local midnight', () => {
    const start = dateToTimestamp('2026-10-01');
    expect(start).not.toBeNull();
    expect(timestampToDate(start)).toBe('2026-10-01');
    // An inclusive end date is stored as the next midnight.
    const end = dateToTimestamp('2026-10-31', 1);
    expect(timestampToDate(end, 1)).toBe('2026-10-31');
    expect(dateToTimestamp('31/10/2026')).toBeNull();
  });

  it('summarises days', () => {
    const names = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
    expect(daysSummary(null, names)).toBeNull();
    expect(daysSummary(EVERY_DAY, names)).toBeNull();
    expect(daysSummary(WEEKDAYS, names)).toBe('Mon–Fri');
    expect(daysSummary(WEEKEND, names)).toBe('Sat, Sun');
    expect(daysSummary(0b001_0101, names)).toBe('Mon, Wed, Fri');
  });
});
