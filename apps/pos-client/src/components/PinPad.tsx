import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Numpad } from './Numpad';

interface PinPadProps {
  onSubmit: (pin: string) => void;
  busy?: boolean | undefined;
  error?: string | undefined;
  submitLabel?: string | undefined;
}

/** 4–6 digit PIN entry on the numpad. */
export function PinPad({ onSubmit, busy, error, submitLabel }: PinPadProps) {
  const { t } = useTranslation();
  const [pin, setPin] = useState('');
  const submit = () => {
    if (pin.length >= 4 && !busy) {
      onSubmit(pin);
      setPin('');
    }
  };
  return (
    <div className="pinpad">
      <div className="pinpad__dots" aria-label={t('session.pinEntered', { count: pin.length })}>
        {Array.from({ length: 6 }, (_, i) => (
          <span key={i} className={i < pin.length ? 'is-filled' : i < 4 ? '' : 'is-optional'} />
        ))}
      </div>
      {error && (
        <p role="alert" className="error-text">
          {error}
        </p>
      )}
      <Numpad
        disabled={busy}
        onDigit={(d) => {
          setPin((p) => (p.length < 6 ? p + d : p));
        }}
        onBackspace={() => {
          setPin((p) => p.slice(0, -1));
        }}
        onClear={() => {
          setPin('');
        }}
        onEnter={submit}
      />
      <button
        type="button"
        className="button button--primary button--block"
        disabled={pin.length < 4 || busy}
        onClick={submit}
      >
        {busy ? t('common.working') : (submitLabel ?? t('session.signIn'))}
      </button>
    </div>
  );
}
