import {
  newUuid,
  REFUND_METHODS,
  type RefundInput,
  type SaleReceipt,
  type TransactionDetail,
  type Uuid,
} from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import { useRefund, useRefundQuote, useVoid } from '../../ipc/queries';
import { formatQuantity, parseQuantity, useMoney } from '../../lib/money';

type RefundMethod = (typeof REFUND_METHODS)[number];

const REASONS = ['damaged', 'wrongItem', 'changedMind', 'quality'] as const;

function ReasonPicker({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const { t } = useTranslation();
  return (
    <div className="field">
      <span>{t('history.reason')}</span>
      <div className="row row--wrap">
        {REASONS.map((r) => {
          const text = t(`history.reasons.${r}`);
          return (
            <button
              key={r}
              type="button"
              className="chip"
              aria-pressed={value === text}
              onClick={() => {
                onChange(text);
              }}
            >
              {text}
            </button>
          );
        })}
      </div>
      <input
        maxLength={200}
        value={value}
        placeholder={t('history.reasonPlaceholder')}
        onChange={(e) => {
          onChange(e.target.value);
        }}
      />
    </div>
  );
}

interface Props {
  detail: TransactionDetail;
  onClose: () => void;
  onDone: (receipt: SaleReceipt) => void;
}

/** Choose quantities to take back; Rust prices it from the stored sale. */
export function RefundDialog({ detail, onClose, onDone }: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const refund = useRefund();
  const lines = detail.lines.filter((l) => l.refundable_quantity_milli > 0);
  const [quantities, setQuantities] = useState<Record<string, number>>({});
  const [weights, setWeights] = useState<Record<string, string>>({});
  const [method, setMethod] = useState<RefundMethod>(
    detail.summary.payment_methods.includes('cash') || detail.summary.payment_methods.length === 0
      ? 'cash'
      : (REFUND_METHODS.find((m) => detail.summary.payment_methods.includes(m)) ?? 'cash'),
  );
  const [restock, setRestock] = useState(true);
  const [reason, setReason] = useState('');
  // One key per refund attempt: a retry after an error cannot pay twice.
  const [key] = useState(newUuid);

  const picked = lines
    .map((l) => ({ item_id: l.item_id, quantity_milli: quantities[l.item_id] ?? 0 }))
    .filter((l) => l.quantity_milli > 0);
  const input: RefundInput = {
    transaction_id: detail.summary.id,
    idempotency_key: key,
    lines: picked,
    method,
    restock,
    // The reason does not change the price: typing it must not re-quote.
    reason: '—',
  };
  // Query keys are compared by value, so a fresh object each render is fine.
  const request = picked.length ? input : null;
  const quote = useRefundQuote(request);

  const set = (itemId: Uuid, milli: number) => {
    setQuantities((q) => ({ ...q, [itemId]: milli }));
  };

  return (
    <Modal
      open
      wide
      title={t('history.refundTitle', { number: detail.summary.receipt_number })}
      onClose={onClose}
    >
      <div className="stack">
        <div className="row row--end">
          <button
            type="button"
            className="link-button"
            onClick={() => {
              setQuantities(
                Object.fromEntries(lines.map((l) => [l.item_id, l.refundable_quantity_milli])),
              );
              setWeights(
                Object.fromEntries(
                  lines.map((l) => [l.item_id, formatQuantity(l.refundable_quantity_milli)]),
                ),
              );
            }}
          >
            {t('history.everything')}
          </button>
        </div>
        <ul className="split-list">
          {lines.map((l) => {
            const qty = quantities[l.item_id] ?? 0;
            return (
              <li key={l.item_id} className="split-list__row">
                <span>
                  {l.name}
                  <span className="muted small">
                    {' '}
                    · {t('history.soldQty', { qty: formatQuantity(l.quantity_milli) })}
                    {l.reversed_quantity_milli > 0 &&
                      ` · ${t('history.backQty', { qty: formatQuantity(l.reversed_quantity_milli) })}`}
                  </span>
                </span>
                {l.whole_units ? (
                  <div className="stepper" dir="ltr">
                    <button
                      type="button"
                      disabled={qty <= 0}
                      aria-label={t('sell.less')}
                      onClick={() => {
                        set(l.item_id, qty - 1000);
                      }}
                    >
                      −
                    </button>
                    <span>{formatQuantity(qty)}</span>
                    <button
                      type="button"
                      disabled={qty + 1000 > l.refundable_quantity_milli}
                      aria-label={t('sell.more')}
                      onClick={() => {
                        set(l.item_id, qty + 1000);
                      }}
                    >
                      +
                    </button>
                  </div>
                ) : (
                  <input
                    className="port"
                    dir="ltr"
                    inputMode="decimal"
                    placeholder="0"
                    value={weights[l.item_id] ?? ''}
                    aria-label={l.name}
                    onChange={(e) => {
                      setWeights((w) => ({ ...w, [l.item_id]: e.target.value }));
                      const milli = parseQuantity(e.target.value);
                      set(l.item_id, typeof milli === 'number' ? milli : 0);
                    }}
                  />
                )}
              </li>
            );
          })}
        </ul>
        <div className="field">
          <span>{t('history.refundTo')}</span>
          <div className="row">
            {REFUND_METHODS.map((m) => (
              <button
                key={m}
                type="button"
                className="chip"
                aria-pressed={method === m}
                onClick={() => {
                  setMethod(m);
                }}
              >
                {t(`pay.methods.${m}`)}
              </button>
            ))}
          </div>
        </div>
        <label className="check">
          <input
            type="checkbox"
            checked={restock}
            onChange={(e) => {
              setRestock(e.target.checked);
            }}
          />
          {t('history.restock')}
        </label>
        <ReasonPicker value={reason} onChange={setReason} />
        {(quote.error ?? refund.error) && (
          <p role="alert" className="error-text">
            {(quote.error ?? refund.error)?.message}
          </p>
        )}
        <div className="row row--end">
          <button type="button" className="button" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button
            type="button"
            className="button button--danger"
            disabled={
              !request || !quote.data || quote.isFetching || !reason.trim() || refund.isPending
            }
            onClick={() => {
              refund.mutate({ ...input, reason: reason.trim() }, { onSuccess: onDone });
            }}
          >
            {quote.data && request
              ? t('history.refundAmount', { amount: format(quote.data.total) })
              : t('history.refundPick')}
          </button>
        </div>
      </div>
    </Modal>
  );
}

/** Reverses the whole sale (this till's open shift only). */
export function VoidDialog({ detail, onClose, onDone }: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const voiding = useVoid();
  const [reason, setReason] = useState('');
  const [key] = useState(newUuid);
  return (
    <Modal
      open
      title={t('history.voidTitle', { number: detail.summary.receipt_number })}
      onClose={onClose}
    >
      <div className="stack">
        <p>{t('history.voidBody', { amount: format(detail.summary.total) })}</p>
        <ReasonPicker value={reason} onChange={setReason} />
        {voiding.error && (
          <p role="alert" className="error-text">
            {voiding.error.message}
          </p>
        )}
        <div className="row row--end">
          <button type="button" className="button" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button
            type="button"
            className="button button--danger"
            disabled={!reason.trim() || voiding.isPending}
            onClick={() => {
              voiding.mutate(
                {
                  transaction_id: detail.summary.id,
                  idempotency_key: key,
                  reason: reason.trim(),
                },
                { onSuccess: onDone },
              );
            }}
          >
            {t('history.voidConfirm')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
