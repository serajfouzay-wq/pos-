import {
  allocate,
  CURRENCIES,
  newUuid,
  type CurrencyCode,
  type Customer,
  type PaymentMethod,
  type QuoteRequest,
  type SaleReceipt,
  type Uuid,
} from '@pos/shared';
import { useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AmountPad } from '../../components/AmountPad';
import { Modal } from '../../components/Modal';
import { useLoyaltyProgram, useQuote } from '../../ipc/queries';
import { useMoney } from '../../lib/money';
import { CustomerPicker } from '../customers/CustomerPicker';
import { LoyaltyPanel } from '../customers/LoyaltyPanel';

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

/** The customer on the bill and the points they spend. */
export interface BillCustomer {
  customer_id: Uuid;
  loyalty_points_to_redeem: number;
}

/** What the dialog asks the caller to do (create a sale / pay an order). */
export interface PaymentSubmit {
  run: (
    payments: PaymentInput[],
    idempotencyKey: Uuid,
    customer: BillCustomer | null,
    callbacks: { onSuccess: (receipt: SaleReceipt) => void },
  ) => void;
  pending: boolean;
  error: Error | null;
  reset: () => void;
}

interface Props {
  open: boolean;
  /** Authoritative total from Rust (`quote_transaction`) without a customer. */
  total: number;
  /** What is being paid, to re-price it when a customer spends points. */
  request: QuoteRequest;
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
export function PaymentDialog({
  open,
  total: baseTotal,
  request,
  guests = 0,
  submit,
  onClose,
  onComplete,
}: Props) {
  const { t } = useTranslation();
  const { format, currency } = useMoney();
  const create = submit;
  const program = useLoyaltyProgram();
  const [customer, setCustomer] = useState<Customer | null>(null);
  const [picking, setPicking] = useState(false);
  // What the cashier typed; only a redemption the rules allow is priced, so
  // half-typed numbers never break the quote.
  const [redeemDraft, setRedeemDraft] = useState(0);
  const [maxRedeem, setMaxRedeem] = useState(0);
  const minRedeem = Math.max(1, program.data?.settings.min_redeem_points ?? 1);
  const redeem = redeemDraft >= minRedeem && redeemDraft <= maxRedeem ? redeemDraft : 0;
  // With a customer, Rust prices the bill again (points off, points earned).
  const customerQuote = useQuote(
    customer ? { ...request, loyalty: { customer_id: customer.id, redeem_points: redeem } } : null,
  );
  const loyalty = customer ? customerQuote.data?.loyalty : null;
  if (loyalty && loyalty.max_redeem_points !== maxRedeem) {
    setMaxRedeem(loyalty.max_redeem_points);
  }
  const total = customer ? (customerQuote.data?.total ?? baseTotal) : baseTotal;
  const repricing = customer !== null && (customerQuote.isFetching || !customerQuote.data);
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
  // A new amount due (points spent) starts the tenders again.
  const [dueFor, setDueFor] = useState(total);
  if (dueFor !== total) {
    setDueFor(total);
    setTenders([]);
    setEntry(total);
  }
  // One key per sale attempt: retries after an error can never double-charge.
  const idempotencyKey = useRef(newUuid());

  const paid = tenders.reduce((sum, x) => sum + x.amount, 0);
  const remaining = Math.max(0, total - paid);
  const withEntry = entry > 0 ? [...tenders, { method, amount: entry }] : tenders;
  const allPaid = withEntry.reduce((sum, x) => sum + x.amount, 0);
  const nonCash = withEntry
    .filter((x) => x.method !== 'cash')
    .reduce((sum, x) => sum + x.amount, 0);
  // Points can pay a whole bill: nothing is tendered then.
  const paidByPoints = total === 0 && customer !== null;
  const canComplete =
    !repricing && (paidByPoints || (allPaid >= total && nonCash <= total && withEntry.length > 0));

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
    const tendered = paidByPoints ? [{ method: 'cash' as const, amount: 0 }] : withEntry;
    create.run(
      tendered.map((x) => ({
        method: x.method,
        tendered_currency: currency,
        tendered_amount: x.amount,
        reference: null,
      })),
      idempotencyKey.current,
      customer ? { customer_id: customer.id, loyalty_points_to_redeem: redeem } : null,
      {
        onSuccess: (receipt) => {
          idempotencyKey.current = newUuid();
          setTenders([]);
          setCustomer(null);
          setRedeemDraft(0);
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
          {program.data && (
            <LoyaltyPanel
              customer={customer}
              quote={loyalty}
              redeem={redeemDraft}
              minRedeem={minRedeem}
              onRedeem={setRedeemDraft}
              onPick={() => {
                setPicking(true);
              }}
              onClear={() => {
                setCustomer(null);
                setRedeemDraft(0);
              }}
            />
          )}
          {customerQuote.error && (
            <p role="alert" className="error-text">
              {customerQuote.error.message}
            </p>
          )}
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
      <CustomerPicker
        open={picking}
        onClose={() => {
          setPicking(false);
        }}
        onPick={(c) => {
          setPicking(false);
          setCustomer(c);
          setRedeemDraft(0);
        }}
      />
    </Modal>
  );
}
