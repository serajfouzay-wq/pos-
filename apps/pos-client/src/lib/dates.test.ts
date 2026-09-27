import { describe, expect, it } from 'vitest';
import { dayCount, presetRange } from './dates';

describe('local ranges', () => {
  const now = new Date(2026, 8, 24, 15, 30); // 24 Sep 2026, 15:30 local

  it('today and yesterday are whole local days', () => {
    const today = presetRange('today', now);
    expect(new Date(today.from).getHours()).toBe(0);
    expect(new Date(today.from).getDate()).toBe(24);
    expect(new Date(today.to).getDate()).toBe(25);
    const yesterday = presetRange('yesterday', now);
    expect(yesterday.to).toBe(today.from);
    expect(dayCount(yesterday)).toBe(1);
  });

  it('last 7 days include today; the month starts on the 1st', () => {
    const week = presetRange('last7', now);
    expect(dayCount(week)).toBe(7);
    expect(new Date(week.from).getDate()).toBe(18);
    const month = presetRange('month', now);
    expect(new Date(month.from).getDate()).toBe(1);
    expect(dayCount(month)).toBe(24);
  });

  it('produces the timestamp format Rust accepts', () => {
    expect(presetRange('today', now).from).toMatch(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/);
  });
});
