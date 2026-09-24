import type { OpenOrderInput, SaleReceipt, Session, Uuid } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { errorText, useToast } from '../../components/Toast';
import { useMenu, useOpenOrder, useOpenOrders, useUpdateOrder } from '../../ipc/queries';
import { useMoney } from '../../lib/money';
import { NewOrderDialog } from '../orders/NewOrderDialog';
import { OrderEditor } from '../orders/OrderEditor';
import { useCart } from './cartStore';
import { toOrderLines, type CartLine } from './lines';
import { QuickSale } from './QuickSale';
import { ReceiptDialog } from './ReceiptDialog';

/**
 * Cafe: pay-now at the counter by default; tabs (a name, maybe a table) for
 * customers who pay later. Tabs are shared with the other tills.
 */
export function CafeSell({ session }: { session: Session }) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const orders = useOpenOrders();
  const menu = useMenu();
  const openOrder = useOpenOrder();
  const update = useUpdateOrder();
  const clearCart = useCart((s) => s.clear);
  const toast = useToast();
  const [activeId, setActiveId] = useState<Uuid | null>(null);
  const [creating, setCreating] = useState<CartLine[] | null>(null);
  const [receipt, setReceipt] = useState<SaleReceipt | null>(null);

  const tabs = [...(orders.data ?? [])].sort((a, b) => a.opened_at.localeCompare(b.opened_at));
  const active = tabs.find((o) => o.id === activeId);
  const taken = new Set(tabs.map((o) => o.table_id));
  const freeTables = (menu.data?.dining_tables ?? []).filter(
    (d) => d.is_active && !taken.has(d.id),
  );

  const createTab = async (input: OpenOrderInput, lines: CartLine[]) => {
    const order = await openOrder.mutateAsync(input);
    setCreating(null);
    setActiveId(order.id);
    if (lines.length === 0) return;
    try {
      await update.mutateAsync({
        order_id: order.id,
        expected_updated_at: order.updated_at,
        items: toOrderLines(lines),
        table_id: order.table_id,
        label: order.label,
        guests: order.guests,
        notes: order.notes,
      });
      clearCart();
    } catch (e) {
      toast.show(errorText(e));
    }
  };

  return (
    <div className="layout-with-tabs">
      <nav className="tab-bar" aria-label={t('orders.tabs')}>
        <button
          type="button"
          aria-pressed={!active}
          onClick={() => {
            setActiveId(null);
          }}
        >
          {t('orders.quickSale')}
        </button>
        {tabs.map((o) => (
          <button
            key={o.id}
            type="button"
            aria-pressed={o.id === active?.id}
            onClick={() => {
              setActiveId(o.id);
            }}
          >
            <span>{o.label ?? o.table_label}</span>
            {o.total !== null && <span className="muted small"> · {format(o.total)}</span>}
          </button>
        ))}
        <button
          type="button"
          className="tab-bar__new"
          onClick={() => {
            openOrder.reset();
            setCreating([]);
          }}
        >
          + {t('orders.newTab')}
        </button>
      </nav>
      {active ? (
        <OrderEditor
          key={active.id}
          session={session}
          order={active}
          courses={false}
          backLabel={t('orders.backToCounter')}
          onBack={() => {
            setActiveId(null);
          }}
          onPaid={setReceipt}
        />
      ) : (
        <QuickSale
          session={session}
          orderType="counter"
          onHold={(lines) => {
            openOrder.reset();
            setCreating(lines);
          }}
        />
      )}
      {creating && (
        <NewOrderDialog
          table={null}
          freeTables={freeTables}
          title={creating.length ? t('orders.holdAsTab') : t('orders.newTab')}
          confirmLabel={t('orders.openTab')}
          pending={openOrder.isPending || update.isPending}
          error={openOrder.error?.message ?? null}
          onClose={() => {
            setCreating(null);
          }}
          onOpen={(input) => {
            void createTab(input, creating).catch(() => undefined);
          }}
        />
      )}
      <ReceiptDialog
        receipt={receipt}
        session={session}
        onNewSale={() => {
          setReceipt(null);
          if (!tabs.some((o) => o.id === activeId)) setActiveId(null);
        }}
      />
      {toast.node}
    </div>
  );
}
