import type { OpenOrderView, Uuid } from '@pos/shared';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import { useQuote, useSplitLine } from '../../ipc/queries';
import { formatQuantity, useMoney } from '../../lib/money';
import { toCartItems, type CartLine } from '../sell/lines';

interface Props {
  order: OpenOrderView;
  lines: readonly CartLine[];
  onClose: () => void;
  /** Pay the chosen lines; `total` is Rust's quote for them. */
  onPay: (lineIds: Uuid[], total: number) => void;
}

/**
 * Split bill: pick the lines one guest pays. A combo is paid as a whole, so
 * picking one of its lines picks them all; "one each" splits a line of N
 * into N lines so guests can pay for single units.
 */
export function SplitBillDialog({ order, lines, onClose, onPay }: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const split = useSplitLine();
  const [selected, setSelected] = useState<ReadonlySet<Uuid>>(new Set());

  const chosen = useMemo(() => lines.filter((l) => selected.has(l.line_id)), [lines, selected]);
  const request = useMemo(
    () => (chosen.length ? { items: toCartItems(chosen), discount_rule_ids: [] } : null),
    [chosen],
  );
  const quote = useQuote(request);

  const toggle = (line: CartLine) => {
    const group = line.combo
      ? lines.filter((l) => l.combo?.instance === line.combo?.instance)
      : [line];
    const on = !selected.has(line.line_id);
    const next = new Set(selected);
    for (const l of group) {
      if (on) next.add(l.line_id);
      else next.delete(l.line_id);
    }
    setSelected(next);
  };

  const total = chosen.length > 0 ? quote.data?.total : undefined;

  return (
    <Modal open wide title={t('orders.splitTitle')} onClose={onClose}>
      <div className="stack">
        <p className="muted">{t('orders.splitHelp')}</p>
        <ul className="split-list">
          {lines.map((line) => (
            <li key={line.line_id} className="split-list__row">
              <label className="split-list__pick">
                <input
                  type="checkbox"
                  checked={selected.has(line.line_id)}
                  onChange={() => {
                    toggle(line);
                  }}
                />
                <span>
                  {formatQuantity(line.quantity_milli)} × {line.name}
                  {line.modifier_names.length > 0 && (
                    <span className="muted small"> · {line.modifier_names.join(', ')}</span>
                  )}
                  {line.combo_name && <span className="badge">{line.combo_name}</span>}
                </span>
              </label>
              {!line.sold_by_weight && !line.combo && line.quantity_milli > 1000 && (
                <button
                  type="button"
                  className="chip"
                  disabled={split.isPending}
                  onClick={() => {
                    split.mutate({ order, lineId: line.line_id });
                  }}
                >
                  {t('orders.oneEach')}
                </button>
              )}
            </li>
          ))}
        </ul>
        {(split.error ?? quote.error) && (
          <p role="alert" className="error-text">
            {(split.error ?? quote.error)?.message}
          </p>
        )}
        <div className="row row--end">
          <button type="button" className="button" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button
            type="button"
            className="button button--primary"
            disabled={total === undefined || quote.isFetching}
            onClick={() => {
              if (total !== undefined)
                onPay(
                  chosen.map((l) => l.line_id),
                  total,
                );
            }}
          >
            {total === undefined
              ? t('orders.paySelectedEmpty')
              : t('orders.paySelected', { amount: format(total) })}
          </button>
        </div>
      </div>
    </Modal>
  );
}
