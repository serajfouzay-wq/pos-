import { describe, expect, it } from 'vitest';
import { formatQuantity, parseQuantity } from './quantity';

describe('quantities', () => {
  it('formats thousandths without floats', () => {
    expect(formatQuantity(2000)).toBe('2');
    expect(formatQuantity(1250)).toBe('1.25');
    expect(formatQuantity(-250)).toBe('-0.25');
    expect(formatQuantity(-3000)).toBe('-3');
  });

  it('parses what people type', () => {
    expect(parseQuantity('2.5')).toBe(2500);
    expect(parseQuantity(' 12 ')).toBe(12_000);
    expect(parseQuantity('0.125')).toBe(125);
    expect(parseQuantity('')).toBeNull();
    expect(parseQuantity('1.2345')).toBeUndefined();
    expect(parseQuantity('abc')).toBeUndefined();
    expect(parseQuantity('-3')).toBeUndefined();
    expect(parseQuantity('-3', true)).toBe(-3000);
  });
});
