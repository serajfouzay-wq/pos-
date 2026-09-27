import type { PeriodReport } from '@pos/shared';
import { useTranslation } from 'react-i18next';
import { formatDateTime, formatTime } from '../../lib/dates';
import { useMoney } from '../../lib/money';
import { useUiStore } from '../../stores/ui';

function Row({
  label,
  value,
  strong = false,
  tone,
}: {
  label: string;
  value: string;
  strong?: boolean;
  tone?: 'good' | 'bad' | undefined;
}) {
  return (
    <>
      <dt className={strong ? 'summary__total' : undefined}>{label}</dt>
      <dd
        className={[strong ? 'summary__total' : '', tone ? `tone--${tone}` : ''].join(' ').trim()}
      >
        {value}
      </dd>
    </>
  );
}

const percent = (bps: number) => `${String(bps / 100)}%`;

/** An X or Z report on screen: the same figures the printout carries. */
export function ReportView({ report }: { report: PeriodReport }) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const locale = useUiStore((s) => s.locale);
  const r = report.totals;
  const cash = report.cash;
  const variance = cash.variance;

  return (
    <div className="report">
      <p className="muted report__period">
        {t('reports.till', { label: report.device_label })} ·{' '}
        {formatDateTime(report.period_start, locale)} → {formatDateTime(report.period_end, locale)}
      </p>
      <div className="report__grid">
        <section className="card">
          <h3>{t('reports.sales')}</h3>
          <dl className="summary">
            <Row
              label={t('reports.salesCount', { count: r.sale_count })}
              value={format(r.gross_sales)}
            />
            <Row label={t('reports.discounts')} value={format(-r.discount_total)} />
            <Row
              label={t('reports.refunds', { count: r.refund_count })}
              value={format(-r.refund_total)}
            />
            <Row
              label={t('reports.voids', { count: r.void_count })}
              value={format(-r.void_total)}
            />
            <Row label={t('reports.netSales')} value={format(r.net_sales)} strong />
          </dl>
        </section>
        <section className="card">
          <h3>{t('reports.payments')}</h3>
          <dl className="summary">
            {r.by_payment_method.length === 0 && <Row label={t('reports.noPayments')} value="—" />}
            {r.by_payment_method.map((m) => (
              <Row
                key={m.method}
                label={`${t(`pay.methods.${m.method}`)} (${String(m.count)})`}
                value={format(m.amount)}
              />
            ))}
          </dl>
          <h3>{t('reports.tax')}</h3>
          <dl className="summary">
            {r.by_tax_rate
              .filter((x) => x.rate_bps > 0)
              .map((x) => (
                <Row
                  key={x.rate_bps}
                  label={t('reports.taxOn', {
                    rate: percent(x.rate_bps),
                    amount: format(x.taxable_amount),
                  })}
                  value={format(x.tax_amount)}
                />
              ))}
            <Row label={t('reports.taxTotal')} value={format(r.tax_total)} strong />
          </dl>
        </section>
        <section className="card">
          <h3>{t('reports.drawer')}</h3>
          <dl className="summary">
            <Row label={t('reports.openingFloats')} value={format(cash.opening_floats)} />
            <Row label={t('reports.cashSales')} value={format(cash.cash_sales)} />
            <Row label={t('reports.cashRefunds')} value={format(-cash.cash_refunds)} />
            <Row label={t('reports.expected')} value={format(cash.expected)} strong />
            {cash.counted !== null && variance !== null ? (
              <>
                <Row label={t('reports.counted')} value={format(cash.counted)} />
                <Row
                  label={t('reports.variance')}
                  value={format(variance)}
                  strong
                  tone={variance === 0 ? 'good' : variance < 0 ? 'bad' : undefined}
                />
              </>
            ) : (
              <Row label={t('reports.counted')} value={t('reports.notCounted')} />
            )}
          </dl>
        </section>
        <section className="card">
          <h3>{t('reports.shifts', { count: report.shifts.length })}</h3>
          <ul className="report__shifts">
            {report.shifts.length === 0 && <li className="muted">{t('reports.noShifts')}</li>}
            {report.shifts.map((s) => (
              <li key={s.shift_id}>
                <span>
                  {formatTime(s.opened_at, locale)}–
                  {s.closed_at ? formatTime(s.closed_at, locale) : t('reports.open')} ·{' '}
                  {s.opened_by_name}
                </span>
                <span
                  className={
                    s.variance === null ? 'muted' : s.variance < 0 ? 'tone--bad' : 'tone--good'
                  }
                >
                  {s.variance === null ? '—' : format(s.variance)}
                </span>
              </li>
            ))}
          </ul>
          <dl className="summary">
            <Row
              label={t('reports.receipts')}
              value={`${r.first_receipt ?? '—'} → ${r.last_receipt ?? '—'}`}
            />
            <Row label={t('reports.grandTotal')} value={format(report.grand_total)} strong />
          </dl>
        </section>
      </div>
    </div>
  );
}
