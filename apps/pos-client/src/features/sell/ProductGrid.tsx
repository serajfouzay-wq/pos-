import type { ComboWithItems, Product } from '@pos/shared';
import { motion } from 'framer-motion';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useCategories, useProducts } from '../../ipc/queries';
import { useMoney } from '../../lib/money';

interface Props {
  onPick: (product: Product) => void;
  /** Cafe/restaurant: combo tiles in their own tab. */
  combos?: readonly ComboWithItems[];
  onPickCombo?: (combo: ComboWithItems) => void;
  /** Retail: start on the quick-keys grid (products with a quick-key slot). */
  quickKeys?: boolean;
  /** Retail: Enter in the search box looks the text up as a barcode first. */
  onSubmitSearch?: (text: string) => Promise<boolean>;
}

type Tab =
  { kind: 'all' } | { kind: 'quick' } | { kind: 'combos' } | { kind: 'category'; id: string };

export function ProductGrid({
  onPick,
  combos = [],
  onPickCombo,
  quickKeys = false,
  onSubmitSearch,
}: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const categories = useCategories();
  const [tab, setTab] = useState<Tab>(quickKeys ? { kind: 'quick' } : { kind: 'all' });
  const [search, setSearch] = useState('');
  const searching = search.trim().length > 0;
  const products = useProducts({
    ...(tab.kind === 'category' && !searching ? { category_id: tab.id } : {}),
    ...(searching ? { search: search.trim() } : {}),
    limit: 300,
  });
  const colorOf = (id: string | null) =>
    categories.data?.find((c) => c.id === id)?.color ?? undefined;
  const shown =
    tab.kind === 'quick' && !searching
      ? (products.data ?? [])
          .filter((p) => p.quick_key_position !== null)
          .sort((a, b) => (a.quick_key_position ?? 0) - (b.quick_key_position ?? 0))
      : (products.data ?? []);
  const pressed = (kind: Tab['kind'], id?: string) =>
    !searching && tab.kind === kind && (tab.kind !== 'category' || tab.id === id);

  return (
    <section className="grid-panel">
      <div className="grid-panel__toolbar">
        <input
          className="search"
          type="search"
          placeholder={onSubmitSearch ? t('sell.scanOrSearch') : t('sell.search')}
          value={search}
          autoFocus={Boolean(onSubmitSearch)}
          onChange={(e) => {
            setSearch(e.target.value);
          }}
          onKeyDown={(e) => {
            if (e.key !== 'Enter' || !onSubmitSearch || !search.trim()) return;
            e.preventDefault();
            void onSubmitSearch(search.trim()).then((found) => {
              if (found) setSearch('');
            });
          }}
        />
      </div>
      <nav className="category-tabs" aria-label={t('sell.categories')}>
        {quickKeys && (
          <button
            type="button"
            aria-pressed={pressed('quick')}
            onClick={() => {
              setSearch('');
              setTab({ kind: 'quick' });
            }}
          >
            {t('sell.quickKeys')}
          </button>
        )}
        {combos.length > 0 && (
          <button
            type="button"
            aria-pressed={pressed('combos')}
            onClick={() => {
              setSearch('');
              setTab({ kind: 'combos' });
            }}
          >
            {t('sell.combos')}
          </button>
        )}
        <button
          type="button"
          aria-pressed={pressed('all')}
          onClick={() => {
            setSearch('');
            setTab({ kind: 'all' });
          }}
        >
          {t('sell.all')}
        </button>
        {categories.data?.map((c) => (
          <button
            key={c.id}
            type="button"
            aria-pressed={pressed('category', c.id)}
            style={c.color ? { borderColor: c.color } : undefined}
            onClick={() => {
              setSearch('');
              setTab({ kind: 'category', id: c.id });
            }}
          >
            {c.name}
          </button>
        ))}
      </nav>
      <div className="product-grid">
        {tab.kind === 'combos' && !searching ? (
          combos.map((combo) => (
            <motion.button
              key={combo.id}
              type="button"
              className="product-tile product-tile--combo"
              whileTap={{ scale: 0.95 }}
              style={{ borderInlineStartColor: combo.color ?? undefined }}
              onClick={() => onPickCombo?.(combo)}
            >
              <span className="product-tile__name">{combo.name}</span>
              <span className="product-tile__price">{format(combo.price)}</span>
              <span className="muted small">
                {t('sell.comboItems', { count: combo.items.length })}
              </span>
            </motion.button>
          ))
        ) : (
          <>
            {shown.length === 0 && (
              <p className="muted product-grid__empty">
                {tab.kind === 'quick' && !searching ? t('sell.noQuickKeys') : t('sell.noProducts')}
              </p>
            )}
            {shown.map((p) => (
              <motion.button
                key={p.id}
                type="button"
                className="product-tile"
                whileTap={{ scale: 0.95 }}
                style={{ borderInlineStartColor: colorOf(p.category_id) }}
                onClick={() => {
                  onPick(p);
                }}
              >
                <span className="product-tile__name">{p.name}</span>
                <span className="product-tile__price">
                  {format(p.price)}
                  {p.sold_by_weight && <span className="muted small"> / {p.unit}</span>}
                </span>
                {p.track_stock && (
                  <span
                    className={
                      p.stock_on_hand_milli <= (p.reorder_threshold_milli ?? 0)
                        ? 'stock stock--low'
                        : 'stock'
                    }
                  >
                    {t('sell.inStock', { count: Math.trunc(p.stock_on_hand_milli / 1000) })}
                  </span>
                )}
              </motion.button>
            ))}
          </>
        )}
      </div>
    </section>
  );
}
