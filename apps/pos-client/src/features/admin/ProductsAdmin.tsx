import {
  parseDecimalString,
  PRODUCT_UNITS,
  toDecimalString,
  type Menu,
  type Product,
  type ProductInput,
  type Uuid,
} from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useAppInfo,
  useCategories,
  useLoadSampleCatalog,
  useMenu,
  useProducts,
  useSaveProduct,
  useSetProductGroups,
} from '../../ipc/queries';
import { parseQuantity, useMoney } from '../../lib/money';

type Unit = (typeof PRODUCT_UNITS)[number];

interface Draft {
  id: string | null;
  name: string;
  price: string;
  barcode: string;
  category_id: string;
  unit: Unit;
  tax_percent: string;
  track_stock: boolean;
  /** Units, e.g. "5" or "2.5"; empty = no alert. */
  reorder_at: string;
  /** Slot on the retail quick-keys grid; empty = none. */
  quick_key: string;
  group_ids: Uuid[];
  is_active: boolean;
}

function draftFrom(
  product: Product | null,
  defaultTaxBps: number,
  format: (p: number) => string,
  menu: Menu | undefined,
): Draft {
  const links = menu?.product_modifier_groups as
    Readonly<Record<string, readonly Uuid[]>> | undefined;
  return {
    id: product?.id ?? null,
    name: product?.name ?? '',
    price: product ? format(product.price) : '',
    barcode: product?.barcode ?? '',
    category_id: product?.category_id ?? '',
    unit: product?.unit ?? 'each',
    tax_percent: String((product?.tax_rate_bps ?? defaultTaxBps) / 100),
    track_stock: product?.track_stock ?? false,
    reorder_at:
      product?.reorder_threshold_milli == null
        ? ''
        : String(product.reorder_threshold_milli / 1000),
    quick_key: product?.quick_key_position == null ? '' : String(product.quick_key_position),
    group_ids: product ? [...(links?.[product.id] ?? [])] : [],
    is_active: product?.is_active ?? true,
  };
}

/** `"5"` / `"5.5"` → basis points, integer-only. */
function percentToBps(text: string): number | null {
  const match = /^(\d{1,3})(?:\.(\d{1,2}))?$/.exec(text.trim());
  if (!match) return null;
  const bps = Number(match[1]) * 100 + Number((match[2] ?? '').padEnd(2, '0'));
  return bps <= 10_000 ? bps : null;
}

export function ProductsAdmin() {
  const { t } = useTranslation();
  const { currency, format } = useMoney();
  const info = useAppInfo();
  const products = useProducts({ include_inactive: true, limit: 1000 });
  const categories = useCategories();
  const save = useSaveProduct();
  const setGroups = useSetProductGroups();
  const menu = useMenu(true);
  const groups = menu.data?.modifier_groups ?? [];
  const sample = useLoadSampleCatalog();
  const defaultTax = info.data?.client.tax.default_rate_bps ?? 0;
  const plain = (p: number) => toDecimalString(p, currency);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);

  const submit = () => {
    if (!draft) return;
    let price: number;
    try {
      price = parseDecimalString(draft.price || '0', currency);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return;
    }
    const tax = percentToBps(draft.tax_percent);
    if (tax === null) {
      setError(t('admin.products.badTax'));
      return;
    }
    const reorder = parseQuantity(draft.reorder_at);
    const quickKey = draft.quick_key.trim() ? Number(draft.quick_key) : null;
    if (
      reorder === undefined ||
      (quickKey !== null && !(Number.isInteger(quickKey) && quickKey >= 0))
    ) {
      setError(t('admin.products.badNumber'));
      return;
    }
    const input: ProductInput = {
      id: draft.id,
      name: draft.name,
      category_id: draft.category_id || null,
      sku: null,
      barcode: draft.barcode.trim() || null,
      price,
      tax_rate_bps: tax,
      unit: draft.unit,
      track_stock: draft.track_stock,
      reorder_threshold_milli: reorder,
      quick_key_position: quickKey,
      is_active: draft.is_active,
    };
    setError(null);
    const groupIds = groups.map((g) => g.id).filter((id) => draft.group_ids.includes(id));
    const before = draftFrom(
      products.data?.find((p) => p.id === draft.id) ?? null,
      defaultTax,
      plain,
      menu.data,
    ).group_ids;
    save.mutate(input, {
      onSuccess: (product) => {
        if (groupIds.join() === before.join()) {
          setDraft(null);
          return;
        }
        setGroups.mutate(
          { productId: product.id, groupIds },
          {
            onSuccess: () => {
              setDraft(null);
            },
          },
        );
      },
    });
  };

  const set = <K extends keyof Draft>(key: K, value: Draft[K]) => {
    setDraft((d) => (d ? { ...d, [key]: value } : d));
  };

  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('admin.products.title')}</h1>
        <button
          type="button"
          className="button button--primary"
          onClick={() => {
            setDraft(draftFrom(null, defaultTax, plain, menu.data));
          }}
        >
          {t('admin.products.add')}
        </button>
      </header>

      {products.data?.length === 0 && (
        <div className="empty-state">
          <p>{t('admin.products.empty')}</p>
          <button
            type="button"
            className="button button--primary"
            disabled={sample.isPending}
            onClick={() => {
              sample.mutate();
            }}
          >
            {t('admin.products.loadSample', {
              type: t(`shell.businessType.${info.data?.client.business_type ?? 'retail'}`),
            })}
          </button>
          {sample.error && <p className="error-text">{sample.error.message}</p>}
        </div>
      )}

      {draft && (
        <form
          className="card form-grid"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
          <label className="field">
            {t('admin.products.name')}
            <input
              value={draft.name}
              required
              maxLength={120}
              onChange={(e) => {
                set('name', e.target.value);
              }}
            />
          </label>
          <label className="field">
            {t('admin.products.price', { currency })}
            <input
              value={draft.price}
              inputMode="decimal"
              dir="ltr"
              placeholder="0.000"
              onChange={(e) => {
                set('price', e.target.value);
              }}
            />
          </label>
          <label className="field">
            {t('admin.products.barcode')}
            <input
              value={draft.barcode}
              dir="ltr"
              onKeyDown={(e) => {
                // Scanners end with Enter; don't submit the form on a scan.
                if (e.key === 'Enter') e.preventDefault();
              }}
              onChange={(e) => {
                set('barcode', e.target.value);
              }}
            />
          </label>
          <label className="field">
            {t('admin.products.category')}
            <select
              value={draft.category_id}
              onChange={(e) => {
                set('category_id', e.target.value);
              }}
            >
              <option value="">—</option>
              {categories.data?.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            {t('admin.products.unit')}
            <select
              value={draft.unit}
              onChange={(e) => {
                set('unit', e.target.value as Unit);
              }}
            >
              {PRODUCT_UNITS.map((u) => (
                <option key={u} value={u}>
                  {t(`admin.products.units.${u}`)}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            {t('admin.products.tax')}
            <input
              value={draft.tax_percent}
              inputMode="decimal"
              dir="ltr"
              onChange={(e) => {
                set('tax_percent', e.target.value);
              }}
            />
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.track_stock}
              onChange={(e) => {
                set('track_stock', e.target.checked);
              }}
            />
            {t('admin.products.trackStock')}
          </label>
          {draft.track_stock && (
            <label className="field">
              {t('admin.products.reorderAt')}
              <input
                value={draft.reorder_at}
                inputMode="decimal"
                dir="ltr"
                placeholder="—"
                onChange={(e) => {
                  set('reorder_at', e.target.value);
                }}
              />
            </label>
          )}
          {info.data?.client.business_type === 'retail' && (
            <label className="field">
              {t('admin.products.quickKey')}
              <input
                value={draft.quick_key}
                inputMode="numeric"
                dir="ltr"
                placeholder="—"
                onChange={(e) => {
                  set('quick_key', e.target.value);
                }}
              />
            </label>
          )}
          {groups.length > 0 && (
            <fieldset className="field span-all">
              <legend>{t('admin.products.options')}</legend>
              <div className="row row--wrap">
                {groups.map((g) => (
                  <label key={g.id} className="check">
                    <input
                      type="checkbox"
                      checked={draft.group_ids.includes(g.id)}
                      onChange={(e) => {
                        set(
                          'group_ids',
                          e.target.checked
                            ? [...draft.group_ids, g.id]
                            : draft.group_ids.filter((id) => id !== g.id),
                        );
                      }}
                    />
                    {g.name}
                  </label>
                ))}
              </div>
            </fieldset>
          )}
          <label className="check">
            <input
              type="checkbox"
              checked={draft.is_active}
              onChange={(e) => {
                set('is_active', e.target.checked);
              }}
            />
            {t('admin.products.active')}
          </label>
          {(error ?? save.error?.message ?? setGroups.error?.message) && (
            <p role="alert" className="error-text span-all">
              {error ?? save.error?.message ?? setGroups.error?.message}
            </p>
          )}
          <div className="row span-all">
            <button
              type="submit"
              className="button button--primary"
              disabled={save.isPending || setGroups.isPending}
            >
              {t('common.save')}
            </button>
            <button
              type="button"
              className="button"
              onClick={() => {
                setDraft(null);
              }}
            >
              {t('common.cancel')}
            </button>
          </div>
        </form>
      )}

      <table className="table">
        <thead>
          <tr>
            <th>{t('admin.products.name')}</th>
            <th>{t('admin.products.category')}</th>
            <th>{t('admin.products.barcode')}</th>
            <th className="num">{t('admin.products.priceShort')}</th>
            <th className="num">{t('admin.products.stock')}</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {products.data?.map((p) => (
            <tr key={p.id} className={p.is_active ? '' : 'inactive'}>
              <td>{p.name}</td>
              <td>{categories.data?.find((c) => c.id === p.category_id)?.name ?? '—'}</td>
              <td dir="ltr">{p.barcode ?? '—'}</td>
              <td className="num">{format(p.price)}</td>
              <td className="num">
                {p.track_stock ? Math.trunc(p.stock_on_hand_milli / 1000) : '—'}
              </td>
              <td>
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    setDraft(draftFrom(p, defaultTax, plain, menu.data));
                  }}
                >
                  {t('common.edit')}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
