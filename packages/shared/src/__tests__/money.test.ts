import { describe, expect, it } from 'vitest';
import vectors from '../../contracts/money-vectors.json';
import currencies from '../../contracts/currencies.json';
import { CURRENCIES, CurrencyCodeSchema, type CurrencyCode } from '../currency';
import {
  allocate,
  applyBasisPoints,
  convertCurrency,
  divRound,
  extractInclusiveTax,
  formatMoney,
  MoneyError,
  multiplyByQuantity,
  parseDecimalString,
  RoundingModeSchema,
  toDecimalString,
} from '../money';

const mode = (value: string) => RoundingModeSchema.parse(value);
const currency = (value: string): CurrencyCode => CurrencyCodeSchema.parse(value);

describe('money contract vectors', () => {
  it.each(vectors.div_round)('divRound($n, $d, $mode) = $expected', (v) => {
    expect(divRound(BigInt(v.n), BigInt(v.d), mode(v.mode))).toBe(BigInt(v.expected));
  });

  it.each(vectors.apply_bps)('applyBasisPoints($amount, $bps, $mode) = $expected', (v) => {
    expect(applyBasisPoints(v.amount, v.bps, mode(v.mode))).toBe(v.expected);
  });

  it.each(vectors.multiply_by_quantity)(
    'multiplyByQuantity($unit_price, $quantity_milli, $mode) = $expected',
    (v) => {
      expect(multiplyByQuantity(v.unit_price, v.quantity_milli, mode(v.mode))).toBe(v.expected);
    },
  );

  it.each(vectors.extract_inclusive_tax)('extractInclusiveTax($gross, $rate_bps)', (v) => {
    const { net, tax } = extractInclusiveTax(v.gross, v.rate_bps, mode(v.mode));
    expect({ net, tax }).toEqual({ net: v.net, tax: v.tax });
    expect(net + tax).toBe(v.gross);
  });

  it.each(vectors.convert_currency)('convert $amount $base→$quote', (v) => {
    const rate = {
      base: currency(v.base),
      quote: currency(v.quote),
      numerator: v.numerator,
      denominator: v.denominator,
    };
    expect(convertCurrency(v.amount, rate, mode(v.mode))).toBe(v.expected);
  });

  it.each(vectors.allocate)('allocate($amount, $weights)', (v) => {
    const parts = allocate(v.amount, v.weights);
    expect(parts).toEqual(v.expected);
    expect(parts.reduce((a, b) => a + b, 0)).toBe(v.amount);
  });

  it.each(vectors.to_decimal_string)('toDecimalString($amount, $currency)', (v) => {
    expect(toDecimalString(v.amount, currency(v.currency))).toBe(v.expected);
  });

  it.each(vectors.parse_decimal_string)('parseDecimalString("$input", $currency)', (v) => {
    const parse = () => parseDecimalString(v.input, currency(v.currency));
    if (v.expected === null) expect(parse).toThrow(MoneyError);
    else expect(parse()).toBe(v.expected);
  });
});

describe('money guards', () => {
  it('rejects fractional input anywhere', () => {
    expect(() => applyBasisPoints(10.5, 100)).toThrow(MoneyError);
    expect(() => multiplyByQuantity(100, 0.5)).toThrow(MoneyError);
  });

  it('detects overflow beyond the safe integer range', () => {
    expect(() => multiplyByQuantity(Number.MAX_SAFE_INTEGER, 2000)).toThrow(/overflow/);
  });

  it('formats via Intl without floats', () => {
    expect(formatMoney(1500, 'KWD', 'en-US')).toContain('1.500');
    expect(formatMoney(123456, 'USD', 'en-US')).toBe('$1,234.56');
  });
});

describe('currency table', () => {
  it('matches contracts/currencies.json', () => {
    const native = Object.fromEntries(
      Object.entries(CURRENCIES).map(([code, def]) => [code, { exponent: def.exponent }]),
    );
    expect(native).toEqual(currencies);
  });
});
