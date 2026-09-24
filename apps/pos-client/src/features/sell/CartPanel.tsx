import type { Quote, Uuid } from '@pos/shared';
import { AnimatePresence, motion } from 'framer-motion';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { formatQuantity, useMoney } from '../../lib/money';
import type { CartLine } from './lines';

interface Props {
  title: string;
  lines: readonly CartLine[];
  quote: Quote | undefined;
  quoteError: string | undefined;
  quoting: boolean;
  /** Group lines under "Course 1", "Course 2"… (restaurant orders). */
  byCourse?: boolean;
  onSetQuantity: (lineId: Uuid, quantityMilli: number) => void;
  onRemove: (lineId: Uuid) => void;
  onEditWeight: (line: CartLine) => void;
  /** Cafe/restaurant: a kitchen note per line. */
  onEditNote?: ((line: CartLine) => void) | undefined;
  onClear?: (() => void) | undefined;
  /** Actions above the pay button (hold as tab, send to kitchen…). */
  actions?: ReactNode;
  payLabel?: string;
  /** E.g. while an order edit is still being saved. */
  payDisabled?: boolean;
  onPay: () => void;
}

function LineRow({
  line,
  total,
  onSetQuantity,
  onRemove,
  onEditWeight,
  onEditNote,
}: {
  line: CartLine;
  total: number | undefined;
  onSetQuantity: Props['onSetQuantity'];
  onRemove: Props['onRemove'];
  onEditWeight: Props['onEditWeight'];
  onEditNote: Props['onEditNote'];
}) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const locked = line.combo !== null;
  return (
    <motion.li
      className={line.fired_at ? 'cart-line cart-line--sent' : 'cart-line'}
      layout
      initial={{ opacity: 0, x: 16 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: -16 }}
    >
      <div className="cart-line__main">
        <span className="cart-line__name">
          {line.name}
          {line.fired_at && <span className="badge badge--sent">{t('orders.sent')}</span>}
        </span>
        <span className="cart-line__total">{total === undefined ? '…' : format(total)}</span>
      </div>
      {(line.modifier_names.length > 0 || line.note !== null || line.combo_name !== null) && (
        <div className="cart-line__details">
          {line.combo_name && <span className="badge">{line.combo_name}</span>}
          {line.modifier_names.map((name) => (
            <span key={name} className="muted small">
              + {name}
            </span>
          ))}
          {line.note && <span className="small cart-line__note">“{line.note}”</span>}
        </div>
      )}
      <div className="cart-line__controls">
        {line.sold_by_weight ? (
          <button
            type="button"
            className="chip"
            onClick={() => {
              onEditWeight(line);
            }}
          >
            {formatQuantity(line.quantity_milli)} · {t('sell.editWeight')}
          </button>
        ) : (
          <div className="stepper" dir="ltr">
            <button
              type="button"
              disabled={locked}
              onClick={() => {
                onSetQuantity(line.line_id, line.quantity_milli - 1000);
              }}
              aria-label={t('sell.less')}
            >
              −
            </button>
            <span>{formatQuantity(line.quantity_milli)}</span>
            <button
              type="button"
              disabled={locked}
              onClick={() => {
                onSetQuantity(line.line_id, line.quantity_milli + 1000);
              }}
              aria-label={t('sell.more')}
            >
              +
            </button>
          </div>
        )}
        <span className="muted small">× {format(line.unit_price)}</span>
        {onEditNote && line.fired_at === null && (
          <button
            type="button"
            className="icon-button"
            onClick={() => {
              onEditNote(line);
            }}
            aria-label={t('options.note')}
            title={t('options.note')}
          >
            ✎
          </button>
        )}
        <button
          type="button"
          className="icon-button"
          onClick={() => {
            onRemove(line.line_id);
          }}
          aria-label={locked ? t('sell.removeCombo') : t('sell.remove')}
        >
          ✕
        </button>
      </div>
    </motion.li>
  );
}

export function CartPanel({
  title,
  lines,
  quote,
  quoteError,
  quoting,
  byCourse = false,
  onSetQuantity,
  onRemove,
  onEditWeight,
  onEditNote,
  onClear,
  actions,
  payLabel,
  payDisabled = false,
  onPay,
}: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  // Quote lines come back in item order.
  const totalOf = (line: CartLine) => quote?.lines[lines.indexOf(line)]?.line_total;
  const groups: { course: number | null; lines: CartLine[] }[] = byCourse
    ? [...new Set(lines.map((l) => l.course))]
        .sort((a, b) => (a ?? 0) - (b ?? 0))
        .map((course) => ({ course, lines: lines.filter((l) => l.course === course) }))
    : [{ course: null, lines: [...lines] }];

  return (
    <aside className="cart">
      <header className="cart__header">
        <h2>{title}</h2>
        {onClear && lines.length > 0 && (
          <button type="button" className="link-button" onClick={onClear}>
            {t('sell.clear')}
          </button>
        )}
      </header>
      <ul className="cart__lines">
        {lines.length === 0 && <li className="muted cart__empty">{t('sell.emptyCart')}</li>}
        {groups.map((group) => (
          <li key={String(group.course)} className="cart__group">
            {byCourse && (
              <h3 className="cart__course">
                {group.course ? t('orders.course', { n: group.course }) : t('orders.noCourse')}
              </h3>
            )}
            <ul>
              <AnimatePresence initial={false}>
                {group.lines.map((line) => (
                  <LineRow
                    key={line.line_id}
                    line={line}
                    total={totalOf(line)}
                    onSetQuantity={onSetQuantity}
                    onRemove={onRemove}
                    onEditWeight={onEditWeight}
                    onEditNote={onEditNote}
                  />
                ))}
              </AnimatePresence>
            </ul>
          </li>
        ))}
      </ul>
      <footer className="cart__totals">
        {quoteError && (
          <p role="alert" className="error-text">
            {quoteError}
          </p>
        )}
        {quote && lines.length > 0 && (
          <dl className="summary">
            <dt>{t('sell.subtotal')}</dt>
            <dd>{format(quote.subtotal)}</dd>
            {quote.discount_total > 0 && (
              <>
                <dt>{t('sell.discount')}</dt>
                <dd>−{format(quote.discount_total)}</dd>
              </>
            )}
            {quote.tax_total > 0 && (
              <>
                <dt>{t('sell.tax')}</dt>
                <dd>{format(quote.tax_total)}</dd>
              </>
            )}
            <dt className="summary__total">{t('sell.total')}</dt>
            <dd className="summary__total">{format(quote.total)}</dd>
          </dl>
        )}
        {actions && <div className="cart__actions">{actions}</div>}
        <button
          type="button"
          className="button button--primary button--block button--xl"
          disabled={payDisabled || lines.length === 0 || !quote || quoting || Boolean(quoteError)}
          onClick={onPay}
        >
          {payLabel ??
            (quote && lines.length > 0
              ? t('sell.pay', { amount: format(quote.total) })
              : t('sell.payEmpty'))}
        </button>
      </footer>
    </aside>
  );
}
