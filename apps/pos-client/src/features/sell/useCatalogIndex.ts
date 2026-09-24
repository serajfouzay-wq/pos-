import type { Product } from '@pos/shared';
import { useMemo } from 'react';
import { useMenu, useProducts } from '../../ipc/queries';
import type { CatalogIndex } from './lines';

/** Products by id plus the menu, for naming order lines and pickers. */
export function useCatalogIndex(): CatalogIndex {
  const products = useProducts({ limit: 1000 });
  const menu = useMenu();
  const map = useMemo(
    () => new Map<string, Product>((products.data ?? []).map((p) => [p.id, p])),
    [products.data],
  );
  return { products: map, menu: menu.data };
}
