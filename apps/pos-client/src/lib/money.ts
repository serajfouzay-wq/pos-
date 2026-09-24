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

export { formatQuantity, parseQuantity } from './quantity';
