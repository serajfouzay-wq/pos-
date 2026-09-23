import { describe, expect, it } from 'vitest';
import { formatBps, parseBps, slugify } from './format';

describe('basis points', () => {
  it('parses percentages without floats', () => {
    expect(parseBps('5')).toBe(500);
    expect(parseBps('5.25')).toBe(525);
    expect(parseBps('5.5')).toBe(550);
    expect(parseBps('0.01')).toBe(1);
    expect(parseBps('')).toBe(0);
    expect(parseBps('100')).toBe(10_000);
    expect(parseBps('100.01')).toBeNull();
    expect(parseBps('5.255')).toBeNull();
    expect(parseBps('-1')).toBeNull();
    expect(parseBps('abc')).toBeNull();
  });

  it('formats and round-trips', () => {
    for (const bps of [0, 1, 50, 500, 525, 550, 1_500, 10_000]) {
      expect(parseBps(formatBps(bps))).toBe(bps);
    }
    expect(formatBps(550)).toBe('5.5');
    expect(formatBps(1_500)).toBe('15');
  });
});

describe('slugify', () => {
  it('makes kebab-case slugs', () => {
    expect(slugify('Al-Noor Café & Bakery')).toBe('al-noor-cafe-bakery');
    expect(slugify('  --Acme--  ')).toBe('acme');
    expect(slugify('مقهى')).toBe('');
    expect(slugify('a'.repeat(50)).length).toBe(40);
  });
});
