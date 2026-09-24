import type { OpenOrderView, Uuid } from '@pos/shared';
import { useLayoutEffect, useMemo, useRef, useState } from 'react';
import { errorText } from '../../components/Toast';
import { useUpdateOrder } from '../../ipc/queries';
import { orderLines, toOrderLines, type CatalogIndex, type CartLine } from '../sell/lines';

export interface OrderMeta {
  table_id: Uuid | null;
  label: string | null;
  guests: number;
  notes: string | null;
}

interface Pending {
  lines: CartLine[];
  meta: OrderMeta;
}

const metaOf = (order: OpenOrderView): OrderMeta => ({
  table_id: order.table_id,
  label: order.label,
  guests: order.guests,
  notes: order.notes,
});

/**
 * Edits an open order. Changes show at once (a local draft) and are saved in
 * order, one at a time, each against the version the previous save returned;
 * a change made on another till in between is refused by Rust and the order
 * reloads.
 */
export function useOrderEditor(order: OpenOrderView, index: CatalogIndex) {
  const update = useUpdateOrder();
  const [draft, setDraft] = useState<Pending | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const base = useRef(order);
  const inFlight = useRef(false);
  const queued = useRef<Pending | null>(null);

  useLayoutEffect(() => {
    if (!inFlight.current) base.current = order;
  }, [order]);

  const flush = async (first: Pending) => {
    inFlight.current = true;
    setSaving(true);
    let next: Pending | null = first;
    try {
      while (next) {
        queued.current = null;
        const current: OpenOrderView = base.current;
        base.current = await update.mutateAsync({
          order_id: current.id,
          expected_updated_at: current.updated_at,
          items: toOrderLines(next.lines),
          ...next.meta,
        });
        next = queued.current;
      }
      setError(null);
    } catch (e) {
      queued.current = null;
      setError(errorText(e));
    } finally {
      inFlight.current = false;
      setSaving(false);
      setDraft(null);
    }
  };

  const saved = useMemo(
    () => orderLines(order, { products: index.products, menu: index.menu }),
    [order, index.products, index.menu],
  );
  const lines = draft?.lines ?? saved;
  const meta = draft?.meta ?? metaOf(order);

  const change = (nextLines: CartLine[], nextMeta: OrderMeta = meta) => {
    const pending = { lines: nextLines, meta: nextMeta };
    setDraft(pending);
    if (inFlight.current) queued.current = pending;
    else void flush(pending);
  };

  return {
    lines,
    meta,
    change,
    /** Unsaved or saving: firing, splitting and paying wait for it. */
    dirty: saving || draft !== null,
    error,
    clearError: () => {
      setError(null);
    },
  };
}
