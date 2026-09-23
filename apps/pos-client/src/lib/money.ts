import { formatMoney, type CurrencyCode } from '@pos/shared';
import { useCallback } from 'react';
import { useAppInfo } from '../ipc/queries';
import { useUiStore } from '../stores/ui';

/** Formats minor units in the client's base currency and the UI locale. */
export function useMoney(): { currency: CurrencyCode; format: (minor: number) => string } {
  const info = useAppInfo();
  const locale = useUiStore((s) => s.locale);
  const currency = info.data?.client.currency.base ?? 'USD';
  const format = useCallback(
    (minor: number) => formatMoney(minor, currency, locale),
    [currency, locale],
  );
  return { currency, format };
}

/** `2000` → `"2"`, `250` → `"0.25"` (display only). */
export function formatQuantity(quantityMilli: number): string {
  const whole = Math.trunc(quantityMilli / 1000);
  const frac = Math.abs(quantityMilli % 1000);
  if (frac === 0) return String(whole);
  return `${String(whole)}.${String(frac).padStart(3, '0').replace(/0+$/, '')}`;
}
