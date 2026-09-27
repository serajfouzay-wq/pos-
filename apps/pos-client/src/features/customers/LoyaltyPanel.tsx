import type { Customer, LoyaltyQuote } from '@pos/shared';
import { useTranslation } from 'react-i18next';
import { useMoney } from '../../lib/money';

interface Props {
  customer: Customer | null;
  /** Rust's figures for this bill (balance, the most it can take, points earned). */
  quote: LoyaltyQuote | null | undefined;
  /** What was typed (it only counts once it meets the rules). */
  redeem: number;
  minRedeem: number;
  onRedeem: (points: number) => void;
  onPick: () => void;
  onClear: () => void;
}

/** The customer on a bill and the points they spend and earn on it. */
export function LoyaltyPanel({
  customer,
  quote,
  redeem,
  minRedeem,
  onRedeem,
  onPick,
  onClear,
}: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();

  if (!customer) {
    return (
      <button type="button" className="button button--block loyalty-add" onClick={onPick}>
        + {t('customers.add')}
      </button>
    );
  }
  const max = quote?.max_redeem_points ?? 0;
  return (
    <section className="loyalty">
      <div className="row">
        <span className="grow">
          <strong>{customer.display_name}</strong>
          {customer.phone && (
            <span className="muted small" dir="ltr">
              {' '}
              · {customer.phone}
            </span>
          )}
        </span>
        <button
          type="button"
          className="icon-button"
          aria-label={t('customers.remove')}
          onClick={onClear}
        >
          ✕
        </button>
      </div>
      {quote?.enabled && (
        <>
          <p className="muted small">
            {t('customers.balance', { count: quote.balance })} ·{' '}
            {t('customers.earns', { count: quote.points_earned })}
          </p>
          {max > 0 ? (
            <div className="loyalty__redeem">
              <label className="field">
                <span>{t('customers.redeem')}</span>
                <input
                  dir="ltr"
                  inputMode="numeric"
                  value={redeem === 0 ? '' : String(redeem)}
                  placeholder="0"
                  onChange={(e) => {
                    const digits = e.target.value.replace(/\D/g, '');
                    onRedeem(digits ? Math.min(Number(digits), max) : 0);
                  }}
                />
              </label>
              <button
                type="button"
                className="chip"
                aria-pressed={redeem === max}
                onClick={() => {
                  onRedeem(redeem === max ? 0 : max);
                }}
              >
                {t('customers.useMax', { count: max })}
              </button>
              {quote.redeem_value > 0 && (
                <strong className="tone--good">−{format(quote.redeem_value)}</strong>
              )}
              {redeem > 0 && redeem < minRedeem && (
                <span className="muted small">{t('customers.minimum', { count: minRedeem })}</span>
              )}
            </div>
          ) : (
            <p className="muted small">{t('customers.cannotRedeem')}</p>
          )}
        </>
      )}
    </section>
  );
}
