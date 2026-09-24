import { STOCK_MODES, type Product, type Session, type StockMode } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import { useAdjustStock, usePrintLabels, useProducts } from '../../ipc/queries';
import { formatQuantity, parseQuantity, useMoney } from '../../lib/money';
import { can } from '../../lib/permissions';

interface Adjusting {
  product: Product;
  mode: StockMode;
  quantity: string;
  note: string;
}

const isLow = (p: Product) =>
  p.track_stock &&
  p.reorder_threshold_milli !== null &&
  p.stock_on_hand_milli <= p.reorder_threshold_milli;

/** Stock on hand, low-stock list, receiving/waste/counts and shelf labels. */
export function StockAdmin({ session }: { session: Session }) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const [lowOnly, setLowOnly] = useState(false);
  const [search, setSearch] = useState('');
  const products = useProducts({
    limit: 1000,
    ...(lowOnly ? { low_stock_only: true } : {}),
    ...(search.trim() ? { search: search.trim() } : {}),
  });
  const adjust = useAdjustStock();
  const labels = usePrintLabels();
  const [adjusting, setAdjusting] = useState<Adjusting | null>(null);
  const [copies, setCopies] = useState<Record<string, number>>({});
  const [formError, setFormError] = useState<string | null>(null);
  const mayAdjust = can(session, 'inventory.adjust');
  const list = products.data ?? [];

  const submit = () => {
    if (!adjusting) return;
    const quantity = parseQuantity(adjusting.quantity, adjusting.mode === 'adjust');
    const valid =
      typeof quantity === 'number' && (adjusting.mode === 'count' ? quantity >= 0 : quantity !== 0);
    if (!valid) {
      setFormError(t('admin.stock.badQuantity'));
      return;
    }
    setFormError(null);
    adjust.mutate(
      {
        product_id: adjusting.product.id,
        mode: adjusting.mode,
        quantity_milli: quantity,
        note: adjusting.note.trim() || null,
      },
      {
        onSuccess: () => {
          setAdjusting(null);
        },
      },
    );
  };

  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('admin.stock.title')}</h1>
        <input
          className="search"
          type="search"
          placeholder={t('sell.search')}
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
          }}
        />
        <label className="check">
          <input
            type="checkbox"
            checked={lowOnly}
            onChange={(e) => {
              setLowOnly(e.target.checked);
            }}
          />
          {t('admin.stock.lowOnly')}
        </label>
      </header>

      <Modal
        open={adjusting !== null}
        title={
          adjusting
            ? `${adjusting.product.name} · ${t('admin.stock.onHandNow', {
                quantity: formatQuantity(adjusting.product.stock_on_hand_milli),
              })}`
            : ''
        }
        wide
        onClose={() => {
          setAdjusting(null);
        }}
      >
        {adjusting && (
          <form
            className="form-grid"
            onSubmit={(e) => {
              e.preventDefault();
              submit();
            }}
          >
            <label className="field">
              {t('admin.stock.mode')}
              <select
                value={adjusting.mode}
                onChange={(e) => {
                  const mode = STOCK_MODES.find((m) => m === e.target.value);
                  if (mode) setAdjusting({ ...adjusting, mode });
                }}
              >
                {STOCK_MODES.map((m) => (
                  <option key={m} value={m}>
                    {t(`admin.stock.modes.${m}`)}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              {t(`admin.stock.quantityFor.${adjusting.mode}`)}
              <input
                autoFocus
                dir="ltr"
                inputMode="decimal"
                value={adjusting.quantity}
                onChange={(e) => {
                  setAdjusting({ ...adjusting, quantity: e.target.value });
                }}
              />
            </label>
            <label className="field span-all">
              {t('admin.stock.note')}
              <input
                maxLength={200}
                value={adjusting.note}
                onChange={(e) => {
                  setAdjusting({ ...adjusting, note: e.target.value });
                }}
              />
            </label>
            {(formError ?? adjust.error?.message) && (
              <p role="alert" className="error-text span-all">
                {formError ?? adjust.error?.message}
              </p>
            )}
            <div className="row span-all">
              <button type="submit" className="button button--primary" disabled={adjust.isPending}>
                {t('admin.stock.apply')}
              </button>
              <button
                type="button"
                className="button"
                onClick={() => {
                  setAdjusting(null);
                }}
              >
                {t('common.cancel')}
              </button>
            </div>
          </form>
        )}
      </Modal>

      {labels.isSuccess && <p className="ok-text">{t('admin.stock.labelsSent')}</p>}
      {labels.error && (
        <p role="alert" className="error-text">
          {labels.error.message}
        </p>
      )}

      <table className="table">
        <thead>
          <tr>
            <th>{t('admin.products.name')}</th>
            <th>{t('admin.products.barcode')}</th>
            <th className="num">{t('admin.products.priceShort')}</th>
            <th className="num">{t('admin.stock.onHand')}</th>
            <th className="num">{t('admin.stock.reorderAt')}</th>
            <th>{t('admin.stock.labels')}</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {list.length === 0 && (
            <tr>
              <td colSpan={7} className="muted">
                {lowOnly ? t('admin.stock.noneLow') : t('sell.noProducts')}
              </td>
            </tr>
          )}
          {list.map((p) => (
            <tr key={p.id} className={isLow(p) ? 'row--low' : ''}>
              <td>
                {p.name}
                {isLow(p) && <span className="badge badge--warn">{t('admin.stock.low')}</span>}
              </td>
              <td dir="ltr">{p.barcode ?? '—'}</td>
              <td className="num">{format(p.price)}</td>
              <td className="num">{p.track_stock ? formatQuantity(p.stock_on_hand_milli) : '—'}</td>
              <td className="num">
                {p.reorder_threshold_milli === null
                  ? '—'
                  : formatQuantity(p.reorder_threshold_milli)}
              </td>
              <td>
                <div className="row">
                  <input
                    className="port"
                    type="number"
                    min={1}
                    max={50}
                    aria-label={t('admin.stock.copies')}
                    value={copies[p.id] ?? 1}
                    onChange={(e) => {
                      const n = Math.min(50, Math.max(1, Math.trunc(Number(e.target.value)) || 1));
                      setCopies({ ...copies, [p.id]: n });
                    }}
                  />
                  <button
                    type="button"
                    className="link-button"
                    disabled={labels.isPending}
                    onClick={() => {
                      labels.mutate({ productId: p.id, copies: copies[p.id] ?? 1 });
                    }}
                  >
                    {t('admin.stock.print')}
                  </button>
                </div>
              </td>
              <td>
                {mayAdjust && p.track_stock && (
                  <button
                    type="button"
                    className="link-button"
                    onClick={() => {
                      adjust.reset();
                      setFormError(null);
                      setAdjusting({ product: p, mode: 'receive', quantity: '', note: '' });
                    }}
                  >
                    {t('admin.stock.adjust')}
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
