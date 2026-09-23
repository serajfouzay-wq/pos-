/**
 * Integer money arithmetic.
 *
 * Every monetary value in the system is an integer count of the currency's
 * minor unit (cents, fils, sen, …). No function here ever produces or accepts
 * a fractional amount; intermediate products are computed in BigInt so large
 * totals cannot silently lose precision.
 *
 * The Rust implementation (`pos_core::money`) is authoritative for anything
 * persisted. This module exists so the UI can preview identical numbers
 * instantly; both are pinned to `contracts/money-vectors.json`.
 */
import { z } from 'zod';
import { CURRENCIES, type CurrencyCode, type ExchangeRate } from './currency';
import { BPS_SCALE, QUANTITY_SCALE } from './primitives';

/** An integer amount in the minor unit of some currency. */
export type MinorUnits = number;

export const MinorUnitsSchema = z.int();
export const NonNegativeMinorUnitsSchema = z.int().nonnegative();

export const ROUNDING_MODES = ['half_up', 'half_even', 'toward_zero'] as const;
/**
 * - `half_up`: ties round away from zero (common for tax).
 * - `half_even`: ties round to the even neighbour (banker's rounding).
 * - `toward_zero`: truncate.
 */
export type RoundingMode = (typeof ROUNDING_MODES)[number];
export const RoundingModeSchema = z.enum(ROUNDING_MODES);

export class MoneyError extends Error {
  override readonly name = 'MoneyError';
}

export function assertMinorUnits(value: number, label = 'amount'): MinorUnits {
  if (!Number.isSafeInteger(value)) {
    throw new MoneyError(`${label} must be a safe integer in minor units, got ${String(value)}`);
  }
  return value;
}

function toSafeNumber(value: bigint, label: string): MinorUnits {
  if (value > BigInt(Number.MAX_SAFE_INTEGER) || value < BigInt(Number.MIN_SAFE_INTEGER)) {
    throw new MoneyError(`${label} overflowed the safe integer range`);
  }
  return Number(value);
}

function abs(value: bigint): bigint {
  return value < 0n ? -value : value;
}

/** Integer division `numerator / denominator` with an explicit rounding mode. */
export function divRound(numerator: bigint, denominator: bigint, mode: RoundingMode): bigint {
  if (denominator === 0n) throw new MoneyError('division by zero');
  let n = numerator;
  let d = denominator;
  if (d < 0n) {
    n = -n;
    d = -d;
  }
  const quotient = n / d; // BigInt division truncates toward zero
  const remainder = n % d;
  if (remainder === 0n || mode === 'toward_zero') return quotient;

  const step = n < 0n ? -1n : 1n;
  const twiceRemainder = 2n * abs(remainder);
  if (twiceRemainder > d) return quotient + step;
  if (twiceRemainder < d) return quotient;
  // exact tie
  if (mode === 'half_up') return quotient + step;
  return quotient % 2n === 0n ? quotient : quotient + step;
}

export function addMinor(...amounts: readonly MinorUnits[]): MinorUnits {
  let total = 0n;
  for (const amount of amounts) total += BigInt(assertMinorUnits(amount));
  return toSafeNumber(total, 'sum');
}

export function subtractMinor(a: MinorUnits, b: MinorUnits): MinorUnits {
  return toSafeNumber(BigInt(assertMinorUnits(a)) - BigInt(assertMinorUnits(b)), 'difference');
}

/** `unitPrice × quantity`, where quantity is in thousandths (1000 = 1 unit). */
export function multiplyByQuantity(
  unitPrice: MinorUnits,
  quantityMilli: number,
  mode: RoundingMode = 'half_up',
): MinorUnits {
  const product =
    BigInt(assertMinorUnits(unitPrice)) * BigInt(assertMinorUnits(quantityMilli, 'quantity'));
  return toSafeNumber(divRound(product, BigInt(QUANTITY_SCALE), mode), 'line total');
}

/** `amount × bps / 10 000` — percentage discounts, exclusive tax, loyalty earn. */
export function applyBasisPoints(
  amount: MinorUnits,
  bps: number,
  mode: RoundingMode = 'half_up',
): MinorUnits {
  const product = BigInt(assertMinorUnits(amount)) * BigInt(assertMinorUnits(bps, 'basis points'));
  return toSafeNumber(divRound(product, BigInt(BPS_SCALE), mode), 'percentage');
}

/**
 * Splits a tax-inclusive gross amount into net + tax.
 * `net = gross × 10 000 / (10 000 + rate)`, `tax = gross − net`, so
 * `net + tax === gross` always holds.
 */
export function extractInclusiveTax(
  gross: MinorUnits,
  rateBps: number,
  mode: RoundingMode = 'half_up',
): { net: MinorUnits; tax: MinorUnits } {
  const g = BigInt(assertMinorUnits(gross));
  const rate = BigInt(assertMinorUnits(rateBps, 'tax rate'));
  if (rate < 0n) throw new MoneyError('tax rate must be non-negative');
  const scale = BigInt(BPS_SCALE);
  const net = divRound(g * scale, scale + rate, mode);
  return { net: toSafeNumber(net, 'net'), tax: toSafeNumber(g - net, 'tax') };
}

/**
 * Splits `amount` across `weights` so the parts sum exactly to `amount`
 * (largest-remainder method; ties go to the earliest index). Used for split
 * bills and pro-rating order-level discounts across lines.
 */
export function allocate(amount: MinorUnits, weights: readonly number[]): MinorUnits[] {
  const total = BigInt(assertMinorUnits(amount));
  if (weights.length === 0) throw new MoneyError('allocate needs at least one weight');
  const w = weights.map((weight) => {
    const value = BigInt(assertMinorUnits(weight, 'weight'));
    if (value < 0n) throw new MoneyError('weights must be non-negative');
    return value;
  });
  const weightSum = w.reduce((acc, value) => acc + value, 0n);
  if (weightSum === 0n) throw new MoneyError('weights must not all be zero');

  const sign = total < 0n ? -1n : 1n;
  const magnitude = abs(total);
  const parts = w.map((value) => (magnitude * value) / weightSum);
  const remainders = w.map((value, index) => ({ index, rem: (magnitude * value) % weightSum }));
  let leftover = magnitude - parts.reduce((acc, value) => acc + value, 0n);
  remainders.sort((a, b) => (a.rem === b.rem ? a.index - b.index : a.rem > b.rem ? -1 : 1));
  for (const { index } of remainders) {
    if (leftover === 0n) break;
    parts[index] = (parts[index] ?? 0n) + 1n;
    leftover -= 1n;
  }
  return parts.map((part) => toSafeNumber(part * sign, 'allocation'));
}

/**
 * Converts `amount` (minor units of `rate.base`) into minor units of `rate.quote`,
 * accounting for differing exponents (e.g. KWD has 3 decimals, USD has 2).
 */
export function convertCurrency(
  amount: MinorUnits,
  rate: ExchangeRate,
  mode: RoundingMode = 'half_up',
): MinorUnits {
  const fromExp = CURRENCIES[rate.base].exponent;
  const toExp = CURRENCIES[rate.quote].exponent;
  const numerator =
    BigInt(assertMinorUnits(amount)) * BigInt(rate.numerator) * 10n ** BigInt(toExp);
  const denominator = BigInt(rate.denominator) * 10n ** BigInt(fromExp);
  return toSafeNumber(divRound(numerator, denominator, mode), 'converted amount');
}

/** `1500, 'KWD'` → `"1.500"`; `-5, 'USD'` → `"-0.05"`. Exact, no floats. */
export function toDecimalString(amount: MinorUnits, currency: CurrencyCode): string {
  const exponent = CURRENCIES[currency].exponent;
  const negative = assertMinorUnits(amount) < 0;
  const digits = Math.abs(amount).toString();
  if (exponent === 0) return (negative ? '-' : '') + digits;
  const padded = digits.padStart(exponent + 1, '0');
  const major = padded.slice(0, -exponent);
  const minor = padded.slice(-exponent);
  return `${negative ? '-' : ''}${major}.${minor}`;
}

/**
 * Parses user input such as `"1.5"` into minor units (`1500` for KWD).
 * Rejects more fractional digits than the currency allows rather than rounding.
 */
export function parseDecimalString(input: string, currency: CurrencyCode): MinorUnits {
  const exponent = CURRENCIES[currency].exponent;
  const match = /^(-)?(\d+)(?:\.(\d*))?$/.exec(input.trim());
  if (!match) throw new MoneyError(`not a decimal amount: "${input}"`);
  const [, sign, major = '0', fraction = ''] = match;
  if (fraction.length > exponent) {
    throw new MoneyError(`${currency} allows at most ${String(exponent)} decimal places`);
  }
  const minor = BigInt(major + fraction.padEnd(exponent, '0'));
  return toSafeNumber(sign ? -minor : minor, 'parsed amount');
}

/** Locale-aware display, e.g. `formatMoney(1500, 'KWD', 'ar-KW')`. Display only. */
export function formatMoney(amount: MinorUnits, currency: CurrencyCode, locale: string): string {
  const exponent = CURRENCIES[currency].exponent;
  const formatter = new Intl.NumberFormat(locale, {
    style: 'currency',
    currency,
    minimumFractionDigits: exponent,
    maximumFractionDigits: exponent,
  });
  // Intl accepts exact decimal strings, so no float conversion happens here.
  return formatter.format(toDecimalString(amount, currency) as Intl.StringNumericLiteral);
}
