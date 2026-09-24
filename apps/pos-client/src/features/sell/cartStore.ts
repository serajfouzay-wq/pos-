import type { Uuid } from '@pos/shared';
import { create } from 'zustand';
import { addLines, removeLine, setQuantity, type CartLine } from './lines';

/** The quick-sale cart (not an open order). */
interface CartState {
  lines: CartLine[];
  add: (lines: CartLine[]) => void;
  setQuantity: (lineId: Uuid, quantityMilli: number) => void;
  remove: (lineId: Uuid) => void;
  replace: (lines: CartLine[]) => void;
  clear: () => void;
}

export const useCart = create<CartState>()((set) => ({
  lines: [],
  add: (added) => {
    set((s) => ({ lines: addLines(s.lines, added) }));
  },
  setQuantity: (lineId, quantityMilli) => {
    set((s) => ({ lines: setQuantity(s.lines, lineId, quantityMilli) }));
  },
  remove: (lineId) => {
    set((s) => ({ lines: removeLine(s.lines, lineId) }));
  },
  replace: (lines) => {
    set({ lines });
  },
  clear: () => {
    set({ lines: [] });
  },
}));
