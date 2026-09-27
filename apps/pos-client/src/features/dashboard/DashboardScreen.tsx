import type { Uuid } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useDashboard } from '../../ipc/queries';
import { dayCount, formatDay, presetRange, RANGE_PRESETS, type RangePreset } from '../../lib/dates';
import { formatQuantity, useMoney } from '../../lib/money';
import { useUiStore } from '../../stores/ui';
import { BarChart, change, Kpi, ShareList } from './charts';

/** Owner analytics: the whole shop (synced tills) or one till. */
export function DashboardScreen({ onLowStock }: { onLowStock: () => void }) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const locale = useUiStore((s) => s.locale);
  const [preset, setPreset] = useState<RangePreset>('today');
  const [deviceId, setDeviceId] = useState<Uuid | null>(null);
  // Anchored when the preset is picked, so the query key stays stable.
  const [now, setNow] = useState(() => new Date());
  const range = presetRange(preset, now);
  const dashboard = useDashboard({ range, device_id: deviceId });
  const d = dashboard.data;
  const days = dayCount(range);

  return (
    <div className="admin dashboard">
      <header className="admin__header">
        <h1>{t('dashboard.title')}</h1>
        <div className="row row--wrap">
          {RANGE_PRESETS.map((p) => (
            <button
              key={p}
              type="button"
              className="chip"
              aria-pressed={preset === p}
              onClick={() => {
                setPreset(p);
                setNow(new Date());
              }}
            >
              {t(`ranges.${p}`)}
            </button>
          ))}
          {d && d.devices.length > 1 && (
            <select
              aria-label={t('dashboard.till')}
              value={deviceId ?? ''}
              onChange={(e) => {
                setDeviceId(
                  d.devices.find((x) => x.device_id === e.target.value)?.device_id ?? null,
                );
              }}
            >
              <option value="">{t('dashboard.allTills')}</option>
              {d.devices.map((x) => (
                <option key={x.device_id} value={x.device_id}>
                  {x.device_id === d.this_device_id
                    ? t('dashboard.thisTill', { label: x.label })
                    : t('reports.till', { label: x.label })}
                </option>
              ))}
            </select>
          )}
        </div>
      </header>
      {dashboard.error && (
        <p role="alert" className="error-text">
          {dashboard.error.message}
        </p>
      )}
      {d && (
        <>
          <div className="kpi-grid">
            <Kpi
              label={t('dashboard.netSales')}
              value={format(d.totals.net_sales)}
              delta={change(d.totals.net_sales, d.previous.net_sales)}
              hint={t('dashboard.previous', { amount: format(d.previous.net_sales) })}
            />
            <Kpi
              label={t('dashboard.sales')}
              value={String(d.totals.sale_count)}
              delta={change(d.totals.sale_count, d.previous.sale_count)}
              hint={t('dashboard.previousCount', { count: d.previous.sale_count })}
            />
            <Kpi label={t('dashboard.averageTicket')} value={format(d.average_ticket)} />
            <Kpi
              label={t('dashboard.refunds', { count: d.totals.refund_count })}
              value={format(-d.totals.refund_total)}
              hint={t('dashboard.voids', {
                count: d.totals.void_count,
                amount: format(-d.totals.void_total),
              })}
            />
            <Kpi label={t('dashboard.discounts')} value={format(-d.totals.discount_total)} />
            <Kpi label={t('dashboard.tax')} value={format(d.totals.tax_total)} />
            <button type="button" className="kpi card kpi--link" onClick={onLowStock}>
              <span className="kpi__label">{t('dashboard.lowStock')}</span>
              <strong className={d.low_stock_count > 0 ? 'kpi__value tone--bad' : 'kpi__value'}>
                {d.low_stock_count}
              </strong>
              <span className="muted small">{t('dashboard.openStock')}</span>
            </button>
          </div>

          <div className="dashboard__charts">
            <section className="card">
              <h2>{t('dashboard.byHour')}</h2>
              <BarChart
                labelEvery={3}
                bars={d.by_hour.map((h) => ({
                  key: String(h.hour),
                  label: String(h.hour),
                  value: h.amount,
                  title: `${String(h.hour).padStart(2, '0')}:00 · ${format(h.amount)} · ${t('dashboard.salesCount', { count: h.count })}`,
                }))}
              />
            </section>
            {days > 1 && (
              <section className="card">
                <h2>{t('dashboard.byDay')}</h2>
                <BarChart
                  labelEvery={Math.max(1, Math.ceil(d.by_day.length / 10))}
                  bars={d.by_day.map((day) => ({
                    key: day.date,
                    label: formatDay(day.date, locale),
                    value: day.amount,
                    title: `${formatDay(day.date, locale)} · ${format(day.amount)} · ${t('dashboard.salesCount', { count: day.count })}`,
                  }))}
                />
              </section>
            )}
          </div>

          <div className="dashboard__grid">
            <section className="card">
              <h2>{t('dashboard.topProducts')}</h2>
              <ShareList
                empty={t('dashboard.nothing')}
                items={d.top_products.map((p) => ({
                  key: p.product_id,
                  label: p.name,
                  value: p.amount,
                  display: format(p.amount),
                  sub: `× ${formatQuantity(p.quantity_milli)}`,
                }))}
              />
            </section>
            <section className="card">
              <h2>{t('dashboard.categories')}</h2>
              <ShareList
                empty={t('dashboard.nothing')}
                items={d.by_category.map((c) => ({
                  key: c.category_id ?? 'none',
                  label: c.name ?? t('dashboard.uncategorised'),
                  value: c.amount,
                  display: format(c.amount),
                }))}
              />
            </section>
            <section className="card">
              <h2>{t('dashboard.payments')}</h2>
              <ShareList
                empty={t('dashboard.nothing')}
                items={d.totals.by_payment_method.map((m) => ({
                  key: m.method,
                  label: t(`pay.methods.${m.method}`),
                  value: m.amount,
                  display: format(m.amount),
                }))}
              />
            </section>
            <section className="card">
              <h2>{t('dashboard.cashiers')}</h2>
              <ShareList
                empty={t('dashboard.nothing')}
                items={d.by_cashier.map((c) => ({
                  key: c.user_id,
                  label: c.name,
                  value: c.amount,
                  display: format(c.amount),
                  sub: t('dashboard.salesCount', { count: c.count }),
                }))}
              />
            </section>
            {d.by_order_type.length > 1 && (
              <section className="card">
                <h2>{t('dashboard.orderTypes')}</h2>
                <ShareList
                  empty={t('dashboard.nothing')}
                  items={d.by_order_type.map((o) => ({
                    key: o.order_type,
                    label: t(`dashboard.orderType.${o.order_type}`),
                    value: o.amount,
                    display: format(o.amount),
                    sub: t('dashboard.salesCount', { count: o.count }),
                  }))}
                />
              </section>
            )}
          </div>
        </>
      )}
    </div>
  );
}
