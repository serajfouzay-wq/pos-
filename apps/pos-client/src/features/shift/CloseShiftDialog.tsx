import type { ShiftSummary } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AmountPad } from '../../components/AmountPad';
import { Modal } from '../../components/Modal';
import { useCloseShift } from '../../ipc/queries';
import { useMoney } from '../../lib/money';

interface Props {
  open: boolean;
  shift: ShiftSummary;
  onClose: () => void;
}

/**
 * Blind count: the manager counts the drawer before seeing what the system
 * expects, then sees the variance. Rust computes expected cash and variance.
 */
export function CloseShiftDialog({ open, shift, onClose }: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const close = useCloseShift();
  const [step, setStep] = useState<'count' | 'float'>('count');
  const [counted, setCounted] = useState(0);
  const [float, setFloat] = useState(shift.shift.opening_float);

  const result = close.data;
  const reset = () => {
    setStep('count');
    setCounted(0);
    close.reset();
    onClose();
  };

  return (
    <Modal open={open} title={t('shift.closeTitle')} onClose={result ? undefined : reset}>
      {result ? (
        <div className="stack">
          <dl className="summary">
            <dt>{t('shift.sales')}</dt>
            <dd>
              {format(result.totals.sales_total)} ·{' '}
              {t('shift.transactions', { count: result.totals.transaction_count })}
            </dd>
            <dt>{t('shift.expected')}</dt>
            <dd>{format(result.shift.expected_cash ?? 0)}</dd>
            <dt>{t('shift.counted')}</dt>
            <dd>{format(result.shift.actual_cash ?? 0)}</dd>
            <dt>{t('shift.variance')}</dt>
            <dd className={(result.shift.variance ?? 0) < 0 ? 'negative' : 'positive'}>
              {format(result.shift.variance ?? 0)}
            </dd>
          </dl>
          <button type="button" className="button button--primary button--block" onClick={reset}>
            {t('common.done')}
          </button>
        </div>
      ) : step === 'count' ? (
        <div className="stack">
          <p className="muted">{t('shift.countBody')}</p>
          <AmountPad
            label={t('shift.counted')}
            value={counted}
            onChange={setCounted}
            onEnter={() => {
              setStep('float');
            }}
          />
          <button
            type="button"
            className="button button--primary button--block"
            onClick={() => {
              setStep('float');
            }}
          >
            {t('common.next')}
          </button>
        </div>
      ) : (
        <div className="stack">
          <p className="muted">{t('shift.floatBody')}</p>
          <AmountPad label={t('shift.closingFloat')} value={float} onChange={setFloat} />
          {close.error && (
            <p role="alert" className="error-text">
              {close.error.message}
            </p>
          )}
          <button
            type="button"
            className="button button--danger button--block"
            disabled={close.isPending || float > counted}
            onClick={() => {
              close.mutate({ actual_cash: counted, closing_float: float, notes: null });
            }}
          >
            {t('shift.close')}
          </button>
        </div>
      )}
    </Modal>
  );
}
