import { z } from 'zod';

/**
 * Supported currencies and their minor-unit exponent (ISO 4217).
 * Amounts are always stored as integers in the minor unit:
 * 1.500 KWD → 1500 fils, 12.34 USD → 1234 cents, 500 JPY → 500.
 *
 * Mirrored in Rust (`pos_core::currency`) and pinned by `contracts/currencies.json`.
 */
export const CURRENCIES = {
  AED: { exponent: 2, symbol: 'د.إ' },
  BHD: { exponent: 3, symbol: '.د.ب' },
  EGP: { exponent: 2, symbol: 'E£' },
  EUR: { exponent: 2, symbol: '€' },
  GBP: { exponent: 2, symbol: '£' },
  IQD: { exponent: 3, symbol: 'ع.د' },
  JOD: { exponent: 3, symbol: 'د.ا' },
  JPY: { exponent: 0, symbol: '¥' },
  KWD: { exponent: 3, symbol: 'د.ك' },
  MYR: { exponent: 2, symbol: 'RM' },
  OMR: { exponent: 3, symbol: 'ر.ع.' },
  QAR: { exponent: 2, symbol: 'ر.ق' },
  SAR: { exponent: 2, symbol: 'ر.س' },
  TND: { exponent: 3, symbol: 'د.ت' },
  USD: { exponent: 2, symbol: '$' },
} as const satisfies Record<string, { exponent: 0 | 2 | 3; symbol: string }>;

export type CurrencyCode = keyof typeof CURRENCIES;

export const CURRENCY_CODES = Object.keys(CURRENCIES) as [CurrencyCode, ...CurrencyCode[]];

export const CurrencyCodeSchema = z.enum(CURRENCY_CODES);

export function currencyExponent(code: CurrencyCode): number {
  return CURRENCIES[code].exponent;
}

/**
 * Exact exchange rate as a rational number: 1 major unit of `base` =
 * `numerator / denominator` major units of `quote`. Rationals avoid float drift.
 */
export const ExchangeRateSchema = z.object({
  base: CurrencyCodeSchema,
  quote: CurrencyCodeSchema,
  numerator: z.int().positive(),
  denominator: z.int().positive(),
});
export type ExchangeRate = z.infer<typeof ExchangeRateSchema>;
