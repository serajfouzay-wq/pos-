import type { Product } from '@pos/shared';
import { motion } from 'framer-motion';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useCategories, useProducts } from '../../ipc/queries';
import { useMoney } from '../../lib/money';

interface Props {
  onPick: (product: Product) => void;
}

export function ProductGrid({ onPick }: Props) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const categories = useCategories();
  const [categoryId, setCategoryId] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const products = useProducts({
    ...(categoryId ? { category_id: categoryId } : {}),
    ...(search.trim() ? { search: search.trim() } : {}),
    limit: 300,
  });
  const colorOf = (id: string | null) =>
    categories.data?.find((c) => c.id === id)?.color ?? undefined;

  return (
    <section className="grid-panel">
      <div className="grid-panel__toolbar">
        <input
          className="search"
          type="search"
          placeholder={t('sell.search')}
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
          }}
        />
      </div>
      <nav className="category-tabs" aria-label={t('sell.categories')}>
        <button
          type="button"
          aria-pressed={categoryId === null}
          onClick={() => {
            setCategoryId(null);
          }}
        >
          {t('sell.all')}
        </button>
        {categories.data?.map((c) => (
          <button
            key={c.id}
            type="button"
            aria-pressed={categoryId === c.id}
            style={c.color ? { borderColor: c.color } : undefined}
            onClick={() => {
              setCategoryId(c.id);
            }}
          >
            {c.name}
          </button>
        ))}
      </nav>
      <div className="product-grid">
        {products.data?.length === 0 && <p className="muted">{t('sell.noProducts')}</p>}
        {products.data?.map((p) => (
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
      </div>
    </section>
  );
}
