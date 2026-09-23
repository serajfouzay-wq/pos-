import { useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useMoney } from '../lib/money';
import { Numpad } from './Numpad';

interface AmountPadProps {
  /** Minor units. */
  value: number;
  onChange: (minor: number) => void;
  label?: string;
  onEnter?: () => void;
}

const MAX_DIGITS = 12;

/**
 * Cash-register style entry: digits shift in from the right in minor units
 * (1 · 2 · 5 · 0 → 1.250 KWD). Integer-only by construction.
 *
 * A value set from outside (e.g. pre-filled with the amount due) is replaced
 * by the first digit typed, not appended to — 3.750 then "5000" is 5.000.
 */
export function AmountPad({ value, onChange, label, onEnter }: AmountPadProps) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const digits = value === 0 ? '' : String(value);
  const lastEmitted = useRef<number | null>(null);
  const emit = (next: number) => {
    lastEmitted.current = next;
    onChange(next);
  };
  return (
    <div className="amountpad">
      {label && <span className="muted">{label}</span>}
      <output className="amountpad__display" dir="ltr">
        {format(value)}
      </output>
      <Numpad
        onDigit={(d) => {
          const base = lastEmitted.current === value ? digits : '';
          if (base.length < MAX_DIGITS) emit(Number(base + d));
        }}
        onBackspace={() => {
          emit(Number(digits.slice(0, -1) || '0'));
        }}
        onClear={() => {
          emit(0);
        }}
        {...(onEnter ? { onEnter } : {})}
      />
      <span className="sr-only">{t('common.amountHelp')}</span>
    </div>
  );
}
