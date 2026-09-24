import type { DiningTable, SaleReceipt, Session, Uuid } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useMenu, useOpenOrder, useOpenOrders } from '../../ipc/queries';
import { useMoney } from '../../lib/money';
import { NewOrderDialog } from '../orders/NewOrderDialog';
import { OrderEditor } from '../orders/OrderEditor';
import { TableMap } from '../orders/TableMap';
import { QuickSale } from './QuickSale';
import { ReceiptDialog } from './ReceiptDialog';

type Mode = { kind: 'floor' } | { kind: 'order'; id: Uuid } | { kind: 'takeaway' };

/** Restaurant: the floor plan, one order per table, courses, split bills. */
export function RestaurantSell({ session }: { session: Session }) {
  const { t } = useTranslation();
  const { format } = useMoney();
  const orders = useOpenOrders();
  const menu = useMenu();
  const openOrder = useOpenOrder();
  const [mode, setMode] = useState<Mode>({ kind: 'floor' });
  const [seating, setSeating] = useState<DiningTable | null>(null);
  const [receipt, setReceipt] = useState<SaleReceipt | null>(null);

  const all = orders.data ?? [];
  const active = mode.kind === 'order' ? all.find((o) => o.id === mode.id) : undefined;
  const tables = (menu.data?.dining_tables ?? []).filter((d) => d.is_active);
  const untabled = all.filter((o) => o.table_id === null);
  const back = () => {
    setMode({ kind: 'floor' });
  };

  const body = () => {
    if (active) {
      return (
        <OrderEditor
          key={active.id}
          session={session}
          order={active}
          courses
          backLabel={t('orders.backToFloor')}
          onBack={back}
          onPaid={setReceipt}
        />
      );
    }
    if (mode.kind === 'takeaway') {
      return (
        <QuickSale
          session={session}
          orderType="takeaway"
          header={
            <div className="order-bar">
              <button type="button" className="button" onClick={back}>
                {t('orders.backToFloor')}
              </button>
              <strong className="order-bar__title">{t('orders.takeaway')}</strong>
            </div>
          }
        />
      );
    }
    return (
      <section className="floor-screen">
        <header className="floor-screen__bar">
          <h2>{t('orders.floor')}</h2>
          <span className="muted">
            {t('orders.occupied', {
              count: all.filter((o) => o.table_id !== null).length,
              total: tables.length,
            })}
          </span>
          {untabled.map((o) => (
            <button
              key={o.id}
              type="button"
              className="chip"
              onClick={() => {
                setMode({ kind: 'order', id: o.id });
              }}
            >
              {o.label}
              {o.total !== null && ` · ${format(o.total)}`}
            </button>
          ))}
          <button
            type="button"
            className="button button--primary floor-screen__end"
            onClick={() => {
              setMode({ kind: 'takeaway' });
            }}
          >
            {t('orders.takeaway')}
          </button>
        </header>
        <TableMap
          tables={tables}
          orders={all}
          onTable={(table, order) => {
            if (order) setMode({ kind: 'order', id: order.id });
            else {
              openOrder.reset();
              setSeating(table);
            }
          }}
        />
      </section>
    );
  };

  return (
    <>
      {body()}
      {seating && (
        <NewOrderDialog
          key={seating.id}
          table={seating}
          title={t('orders.seatTable', { label: seating.label })}
          confirmLabel={t('orders.seat')}
          pending={openOrder.isPending}
          error={openOrder.error?.message ?? null}
          onClose={() => {
            setSeating(null);
          }}
          onOpen={(input) => {
            openOrder.mutate(input, {
              onSuccess: (order) => {
                setSeating(null);
                setMode({ kind: 'order', id: order.id });
              },
            });
          }}
        />
      )}
      <ReceiptDialog
        receipt={receipt}
        session={session}
        onNewSale={() => {
          setReceipt(null);
          if (!active) back();
        }}
      />
    </>
  );
}
