import type { Quote } from '@pos/shared';
import { AnimatePresence, motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { formatQuantity, useMoney } from '../../lib/money';
import { useCart } from './cartStore';

interface Props {
  quote: Quote | undefined;
  quoteError: string | undefined;
  quoting: boolean;
  onPay: () => void;
  onEditWeight: (productId: string) => void;
}

export function CartPanel({ quote, quoteError, quoting, onPay, onEditWeight }: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const { lines, setQuantity, remove, clear } = useCart();
  const lineTotal = (productId: string) =>
    quote?.lines.find((l) => l.product_id === productId)?.line_total;

  return (
    <aside className="cart">
      <header className="cart__header">
        <h2>{t('sell.cart')}</h2>
        {lines.length > 0 && (
          <button type="button" className="link-button" onClick={clear}>
            {t('sell.clear')}
          </button>
        )}
      </header>
      <ul className="cart__lines">
        {lines.length === 0 && <li className="muted cart__empty">{t('sell.emptyCart')}</li>}
        <AnimatePresence initial={false}>
          {lines.map((line) => {
            const total = lineTotal(line.product_id);
            return (
              <motion.li
                key={line.product_id}
                className="cart-line"
                layout
                initial={{ opacity: 0, x: 16 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0, x: -16 }}
              >
                <div className="cart-line__main">
                  <span className="cart-line__name">{line.name}</span>
                  <span className="cart-line__total">
                    {total === undefined ? '…' : format(total)}
                  </span>
                </div>
                <div className="cart-line__controls">
                  {line.sold_by_weight ? (
                    <button
                      type="button"
                      className="chip"
                      onClick={() => {
                        onEditWeight(line.product_id);
                      }}
                    >
                      {formatQuantity(line.quantity_milli)} · {t('sell.editWeight')}
                    </button>
                  ) : (
                    <div className="stepper" dir="ltr">
                      <button
                        type="button"
                        onClick={() => {
                          setQuantity(line.product_id, line.quantity_milli - 1000);
                        }}
                        aria-label={t('sell.less')}
                      >
                        −
                      </button>
                      <span>{formatQuantity(line.quantity_milli)}</span>
                      <button
                        type="button"
                        onClick={() => {
                          setQuantity(line.product_id, line.quantity_milli + 1000);
                        }}
                        aria-label={t('sell.more')}
                      >
                        +
                      </button>
                    </div>
                  )}
                  <span className="muted small">× {format(line.unit_price)}</span>
                  <button
                    type="button"
                    className="icon-button"
                    onClick={() => {
                      remove(line.product_id);
                    }}
                    aria-label={t('sell.remove')}
                  >
                    ✕
                  </button>
                </div>
              </motion.li>
            );
          })}
        </AnimatePresence>
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
        <button
          type="button"
          className="button button--primary button--block button--xl"
          disabled={lines.length === 0 || !quote || quoting || Boolean(quoteError)}
          onClick={onPay}
        >
          {quote && lines.length > 0
            ? t('sell.pay', { amount: format(quote.total) })
            : t('sell.payEmpty')}
        </button>
      </footer>
    </aside>
  );
}
