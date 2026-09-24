import {
  allocate,
  CURRENCIES,
  newUuid,
  type CurrencyCode,
  type PaymentMethod,
  type SaleReceipt,
  type Uuid,
} from '@pos/shared';
import { useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AmountPad } from '../../components/AmountPad';
import { Modal } from '../../components/Modal';
import { useMoney } from '../../lib/money';

export type TenderMethod = Extract<PaymentMethod, 'cash' | 'card' | 'wallet'>;
const METHODS: readonly TenderMethod[] = ['cash', 'card', 'wallet'];

interface Tender {
  method: TenderMethod;
  amount: number;
}

export interface PaymentInput {
  method: TenderMethod;
  tendered_currency: CurrencyCode;
  tendered_amount: number;
  reference: null;
}

/** What the dialog asks the caller to do (create a sale / pay an order). */
export interface PaymentSubmit {
  run: (
    payments: PaymentInput[],
    idempotencyKey: Uuid,
    callbacks: { onSuccess: (receipt: SaleReceipt) => void },
  ) => void;
  pending: boolean;
  error: Error | null;
  reset: () => void;
}

interface Props {
  open: boolean;
  /** Authoritative total from Rust (`quote_transaction`). */
  total: number;
  /** Offer "split equally" between this many guests (0/1 = hidden). */
  guests?: number;
  submit: PaymentSubmit;
  onClose: () => void;
  onComplete: (receipt: SaleReceipt) => void;
}

/**
 * Tender entry. The sums shown here are for guidance; Rust re-prices the cart
 * from the catalogue and re-validates every tender before recording the sale.
 */
export function PaymentDialog({ open, total, guests = 0, submit, onClose, onComplete }: Props) {
  const { t } = useTranslation();
  const { format, currency } = useMoney();
  const create = submit;
  const [splitWays, setSplitWays] = useState(Math.max(guests, 2));
  // Equal shares in integer minor units (largest remainder), from Rust's rules.
  const shares = useMemo(
    () =>
      allocate(
        total,
        Array.from({ length: splitWays }, () => 1),
      ),
    [total, splitWays],
  );
  const [tenders, setTenders] = useState<Tender[]>([]);
  const [method, setMethod] = useState<TenderMethod>('cash');
  const [entry, setEntry] = useState(total);
  // One key per sale attempt: retries after an error can never double-charge.
  const idempotencyKey = useRef(newUuid());

  const paid = tenders.reduce((sum, x) => sum + x.amount, 0);
  const remaining = Math.max(0, total - paid);
  const withEntry = entry > 0 ? [...tenders, { method, amount: entry }] : tenders;
  const allPaid = withEntry.reduce((sum, x) => sum + x.amount, 0);
  const nonCash = withEntry
    .filter((x) => x.method !== 'cash')
    .reduce((sum, x) => sum + x.amount, 0);
  const canComplete = allPaid >= total && nonCash <= total && withEntry.length > 0;

  const quickCash = useMemo(() => {
    const unit = 10 ** CURRENCIES[currency].exponent;
    const notes = [1, 5, 10, 20, 50, 100].map((n) => n * unit).filter((n) => n > remaining);
    return notes.slice(0, 3);
  }, [currency, remaining]);

  const choose = (next: TenderMethod) => {
    setMethod(next);
    setEntry(remaining);
  };

  const addTender = () => {
    if (entry <= 0) return;
    setTenders((ts) => [...ts, { method, amount: entry }]);
    setEntry(Math.max(0, remaining - entry));
  };

  const complete = () => {
    if (!canComplete) return;
    create.run(
      withEntry.map((x) => ({
        method: x.method,
        tendered_currency: currency,
        tendered_amount: x.amount,
        reference: null,
      })),
      idempotencyKey.current,
      {
        onSuccess: (receipt) => {
          idempotencyKey.current = newUuid();
          setTenders([]);
          onComplete(receipt);
        },
      },
    );
  };

  const close = () => {
    if (create.pending) return;
    setTenders([]);
    setEntry(total);
    create.reset();
    onClose();
  };

  return (
    <Modal open={open} title={t('pay.title', { amount: format(total) })} onClose={close} wide>
      <div className="payment">
        <div className="payment__entry">
          <div className="segmented" role="radiogroup" aria-label={t('pay.method')}>
            {METHODS.map((m) => (
              <button
                key={m}
                type="button"
                role="radio"
                aria-checked={method === m}
                onClick={() => {
                  choose(m);
                }}
              >
                {t(`pay.methods.${m}`)}
              </button>
            ))}
          </div>
          <AmountPad value={entry} onChange={setEntry} onEnter={complete} />
          <div className="quick-row">
            <button
              type="button"
              className="chip"
              onClick={() => {
                setEntry(remaining);
              }}
            >
              {t('pay.exact')}
            </button>
            {method === 'cash' &&
              quickCash.map((n) => (
                <button
                  key={n}
                  type="button"
                  className="chip"
                  onClick={() => {
                    setEntry(n);
                  }}
                >
                  {format(n)}
                </button>
              ))}
            <button
              type="button"
              className="chip"
              disabled={entry <= 0 || entry >= remaining}
              onClick={addTender}
            >
              {t('pay.split')}
            </button>
          </div>
          {guests > 1 && (
            <div className="quick-row">
              <span className="muted small">{t('pay.splitEqually')}</span>
              <div className="stepper" dir="ltr">
                <button
                  type="button"
                  aria-label={t('sell.less')}
                  onClick={() => {
                    setSplitWays((n) => Math.max(2, n - 1));
                  }}
                >
                  −
                </button>
                <span>÷{splitWays}</span>
                <button
                  type="button"
                  aria-label={t('sell.more')}
                  onClick={() => {
                    setSplitWays((n) => Math.min(20, n + 1));
                  }}
                >
                  +
                </button>
              </div>
              <button
                type="button"
                className="chip"
                disabled={tenders.length >= splitWays - 1 || remaining <= 0}
                onClick={() => {
                  setEntry(Math.min(remaining, shares[tenders.length] ?? remaining));
                }}
              >
                {t('pay.share', {
                  n: tenders.length + 1,
                  of: splitWays,
                  amount: format(shares[tenders.length] ?? 0),
                })}
              </button>
            </div>
          )}
        </div>
        <div className="payment__summary">
          <ul className="tenders">
            {tenders.map((x, i) => (
              <li key={`${x.method}-${String(i)}`}>
                <span>{t(`pay.methods.${x.method}`)}</span>
                <span>{format(x.amount)}</span>
                <button
                  type="button"
                  className="icon-button"
                  aria-label={t('sell.remove')}
                  onClick={() => {
                    setTenders((ts) => ts.filter((_, j) => j !== i));
                  }}
                >
                  ✕
                </button>
              </li>
            ))}
          </ul>
          <dl className="summary">
            <dt>{t('pay.due')}</dt>
            <dd>{format(total)}</dd>
            <dt>{t('pay.tendered')}</dt>
            <dd>{format(allPaid)}</dd>
            {allPaid >= total ? (
              <>
                <dt className="summary__total">{t('pay.change')}</dt>
                <dd className="summary__total">{format(allPaid - total)}</dd>
              </>
            ) : (
              <>
                <dt className="summary__total">{t('pay.remaining')}</dt>
                <dd className="summary__total">{format(total - allPaid)}</dd>
              </>
            )}
          </dl>
          {nonCash > total && (
            <p role="alert" className="error-text">
              {t('pay.noChangeOnCard')}
            </p>
          )}
          {create.error && (
            <p role="alert" className="error-text">
              {create.error.message}
            </p>
          )}
          <button
            type="button"
            className="button button--primary button--block button--xl"
            disabled={!canComplete || create.pending}
            onClick={complete}
          >
            {create.pending ? t('common.working') : t('pay.complete')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
