import type { CartItem, Product } from '@pos/shared';
import { create } from 'zustand';

/** Display snapshot only — prices shown on tiles; totals always come from Rust. */
export interface CartLine {
  product_id: string;
  name: string;
  unit_price: number;
  sold_by_weight: boolean;
  unit: Product['unit'];
  quantity_milli: number;
}

interface CartState {
  lines: CartLine[];
  add: (product: Product, quantityMilli?: number) => void;
  setQuantity: (productId: string, quantityMilli: number) => void;
  remove: (productId: string) => void;
  clear: () => void;
}

export const useCart = create<CartState>()((set) => ({
  lines: [],
  add: (product, quantityMilli = 1000) => {
    set((state) => {
      const existing = state.lines.find((l) => l.product_id === product.id);
      if (existing && !product.sold_by_weight) {
        return {
          lines: state.lines.map((l) =>
            l.product_id === product.id
              ? { ...l, quantity_milli: l.quantity_milli + quantityMilli }
              : l,
          ),
        };
      }
      if (existing) {
        return {
          lines: state.lines.map((l) =>
            l.product_id === product.id ? { ...l, quantity_milli: quantityMilli } : l,
          ),
        };
      }
      return {
        lines: [
          ...state.lines,
          {
            product_id: product.id,
            name: product.name,
            unit_price: product.price,
            sold_by_weight: product.sold_by_weight,
            unit: product.unit,
            quantity_milli: quantityMilli,
          },
        ],
      };
    });
  },
  setQuantity: (productId, quantityMilli) => {
    set((state) => ({
      lines:
        quantityMilli <= 0
          ? state.lines.filter((l) => l.product_id !== productId)
          : state.lines.map((l) =>
              l.product_id === productId ? { ...l, quantity_milli: quantityMilli } : l,
            ),
    }));
  },
  remove: (productId) => {
    set((state) => ({ lines: state.lines.filter((l) => l.product_id !== productId) }));
  },
  clear: () => {
    set({ lines: [] });
  },
}));

export function toCartItems(lines: readonly CartLine[]): CartItem[] {
  return lines.map((l) => ({
    product_id: l.product_id,
    quantity_milli: l.quantity_milli,
    modifier_ids: [],
    course: null,
    note: null,
  }));
}
