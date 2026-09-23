import { useEffect, useLayoutEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';

interface NumpadProps {
  onDigit: (digit: string) => void;
  onBackspace: () => void;
  onClear: () => void;
  onEnter?: () => void;
  disabled?: boolean | undefined;
  /** Also accept the physical keyboard's digits, Backspace, Delete (clear) and Enter. Escape stays with dialogs. */
  keyboard?: boolean;
}

const KEYS = ['1', '2', '3', '4', '5', '6', '7', '8', '9'] as const;

/** Touch numpad: no keyboard needed. Digits are always laid out LTR. */
export function Numpad({
  onDigit,
  onBackspace,
  onClear,
  onEnter,
  disabled,
  keyboard = true,
}: NumpadProps) {
  const { t } = useTranslation();
  const handlers = useRef({ onDigit, onBackspace, onClear, onEnter });
  useLayoutEffect(() => {
    handlers.current = { onDigit, onBackspace, onClear, onEnter };
  });

  useEffect(() => {
    if (!keyboard || disabled) return undefined;
    const listener = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')) return;
      const h = handlers.current;
      if (/^[0-9]$/.test(e.key)) h.onDigit(e.key);
      else if (e.key === 'Backspace') h.onBackspace();
      else if (e.key === 'Delete') h.onClear();
      else if (e.key === 'Enter' && h.onEnter) h.onEnter();
      else return;
      e.preventDefault();
    };
    window.addEventListener('keydown', listener);
    return () => {
      window.removeEventListener('keydown', listener);
    };
  }, [keyboard, disabled]);

  return (
    <div className="numpad" dir="ltr">
      {KEYS.map((k) => (
        <button
          key={k}
          type="button"
          disabled={disabled}
          onClick={() => {
            onDigit(k);
          }}
        >
          {k}
        </button>
      ))}
      <button
        type="button"
        className="numpad__aux"
        disabled={disabled}
        onClick={onClear}
        aria-label={t('common.clear')}
      >
        C
      </button>
      <button
        type="button"
        disabled={disabled}
        onClick={() => {
          onDigit('0');
        }}
      >
        0
      </button>
      <button
        type="button"
        className="numpad__aux"
        disabled={disabled}
        onClick={onBackspace}
        aria-label={t('common.backspace')}
      >
        ⌫
      </button>
    </div>
  );
}
