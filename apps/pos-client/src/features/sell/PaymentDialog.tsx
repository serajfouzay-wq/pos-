import { CURRENCIES, type PaymentMethod, type SaleReceipt } from '@pos/shared';
import { useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AmountPad } from '../../components/AmountPad';
import { Modal } from '../../components/Modal';
import { useCreateTransaction } from '../../ipc/queries';
import { useMoney } from '../../lib/money';
import { toCartItems, useCart } from './cartStore';

type TenderMethod = Extract<PaymentMethod, 'cash' | 'card' | 'wallet'>;
const METHODS: readonly TenderMethod[] = ['cash', 'card', 'wallet'];

interface Tender {
  method: TenderMethod;
  amount: number;
}

interface Props {
  open: boolean;
  /** Authoritative total from `quote_transaction`. */
  total: number;
  onClose: () => void;
  onComplete: (receipt: SaleReceipt) => void;
}

/**
 * Tender entry. The sums shown here are for guidance; Rust re-prices the cart
 * from the catalogue and re-validates every tender before recording the sale.
 */
export function PaymentDialog({ open, total, onClose, onComplete }: Props) {
  const { t } = useTranslation();
  const { format, currency } = useMoney();
  const lines = useCart((s) => s.lines);
  const create = useCreateTransaction();
  const [tenders, setTenders] = useState<Tender[]>([]);
  const [method, setMethod] = useState<TenderMethod>('cash');
  const [entry, setEntry] = useState(total);
  // One key per sale attempt: retries after an error can never double-charge.
  const idempotencyKey = useRef(crypto.randomUUID());

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
    create.mutate(
      {
        idempotency_key: idempotencyKey.current,
        customer_id: null,
        order_type: 'counter',
        table_label: null,
        items: toCartItems(lines),
        discount_rule_ids: [],
        loyalty_points_to_redeem: 0,
        payments: withEntry.map((x) => ({
          method: x.method,
          tendered_currency: currency,
          tendered_amount: x.amount,
          reference: null,
        })),
        notes: null,
      },
      {
        onSuccess: (receipt) => {
          idempotencyKey.current = crypto.randomUUID();
          setTenders([]);
          onComplete(receipt);
        },
      },
    );
  };

  const close = () => {
    if (create.isPending) return;
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
            disabled={!canComplete || create.isPending}
            onClick={complete}
          >
            {create.isPending ? t('common.working') : t('pay.complete')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
