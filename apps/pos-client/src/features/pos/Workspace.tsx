import { LOCALES, type Locale, type Session, type ShiftSummary } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useAppInfo,
  useCurrentShift,
  useKickDrawer,
  useLogout,
  usePrinterStatus,
  useProducts,
} from '../../ipc/queries';
import { can } from '../../lib/permissions';
import { useUiStore } from '../../stores/ui';
import { FloorAdmin } from '../admin/FloorAdmin';
import { MenuAdmin } from '../admin/MenuAdmin';
import { PrinterAdmin } from '../admin/PrinterAdmin';
import { ProductsAdmin } from '../admin/ProductsAdmin';
import { StockAdmin } from '../admin/StockAdmin';
import { UsersAdmin } from '../admin/UsersAdmin';
import { SellScreen } from '../sell/SellScreen';
import { SyncIndicator } from '../sync/SyncIndicator';
import { CloseShiftDialog } from '../shift/CloseShiftDialog';
import { ShiftGate } from '../shift/ShiftGate';

type View = 'sell' | 'products' | 'menu' | 'floor' | 'stock' | 'users' | 'printer';
const LOCALE_LABELS: Record<Locale, string> = { en: 'EN', ar: 'ع' };

export function Workspace({ session }: { session: Session }) {
  const { t } = useTranslation();
  const info = useAppInfo();
  const logout = useLogout();
  const drawer = useKickDrawer();
  const printer = usePrinterStatus(true);
  const shift = useCurrentShift();
  const { locale, setLocale } = useUiStore();
  const [view, setView] = useState<View>('sell');
  const business = info.data?.client.business_type;
  const mayViewStock = can(session, 'inventory.view');
  const lowStock = useProducts({ low_stock_only: true, limit: 1000 }, mayViewStock);
  const lowCount = lowStock.data?.length ?? 0;
  // Snapshot of the shift being closed: the dialog must outlive the shift
  // itself so the reconciliation result stays on screen after closing.
  const [closingShift, setClosingShift] = useState<ShiftSummary | null>(null);

  const views: { id: View; allowed: boolean }[] = [
    { id: 'sell', allowed: true },
    { id: 'products', allowed: can(session, 'catalog.manage') },
    { id: 'menu', allowed: business !== 'retail' && can(session, 'catalog.manage') },
    { id: 'floor', allowed: business !== 'retail' && can(session, 'catalog.manage') },
    { id: 'stock', allowed: mayViewStock },
    { id: 'users', allowed: can(session, 'user.manage') },
    { id: 'printer', allowed: can(session, 'settings.manage') },
  ];
  const supported = info.data?.client.locale.supported ?? LOCALES;

  const renderView = (shift: ShiftSummary | null) => {
    switch (view) {
      case 'products':
        return <ProductsAdmin />;
      case 'menu':
        return <MenuAdmin />;
      case 'floor':
        return <FloorAdmin />;
      case 'stock':
        return <StockAdmin session={session} />;
      case 'users':
        return <UsersAdmin />;
      case 'printer':
        return <PrinterAdmin />;
      case 'sell':
        return shift ? <SellScreen session={session} /> : null;
    }
  };

  const printerState = printer.data;
  const printerClass =
    !printerState?.configured || printerState.online === false
      ? 'status-dot status-dot--warn'
      : 'status-dot status-dot--ok';

  return (
    <div className="workspace">
      <header className="topbar">
        <strong className="topbar__brand">{info.data?.client.display_name}</strong>
        <nav className="topbar__nav">
          {views
            .filter((v) => v.allowed)
            .map((v) => (
              <button
                key={v.id}
                type="button"
                aria-current={view === v.id ? 'page' : undefined}
                onClick={() => {
                  setView(v.id);
                }}
              >
                {t(`nav.${v.id}`)}
              </button>
            ))}
        </nav>
        <div className="topbar__status">
          <SyncIndicator />
          {lowCount > 0 && (
            <button
              type="button"
              className="chip chip--warn"
              onClick={() => {
                setView('stock');
              }}
            >
              {t('status.lowStock', { count: lowCount })}
            </button>
          )}
          <span className={printerClass} title={printerState?.last_error ?? ''}>
            {!printerState?.configured
              ? t('status.noPrinter')
              : printerState.online === false
                ? t('status.printerOffline')
                : t('status.printer')}
            {printerState &&
              printerState.pending_jobs > 0 &&
              ` · ${t('status.pending', { count: printerState.pending_jobs })}`}
          </span>
          {can(session, 'drawer.kick') && (
            <button
              type="button"
              className="chip"
              disabled={drawer.isPending}
              onClick={() => {
                drawer.mutate();
              }}
            >
              {t('nav.noSale')}
            </button>
          )}
          {can(session, 'shift.close') && view === 'sell' && shift.data && (
            <button
              type="button"
              className="chip"
              onClick={() => {
                setClosingShift(shift.data ?? null);
              }}
            >
              {t('shift.close')}
            </button>
          )}
          {supported.map((l) => (
            <button
              key={l}
              type="button"
              className="chip"
              aria-pressed={l === locale}
              onClick={() => {
                setLocale(l);
              }}
            >
              {LOCALE_LABELS[l]}
            </button>
          ))}
          <span className="topbar__user">
            {session.display_name} · <span className="muted">{t(`roles.${session.role}`)}</span>
          </span>
          <button
            type="button"
            className="button"
            onClick={() => {
              logout.mutate();
            }}
          >
            {t('session.signOut')}
          </button>
        </div>
      </header>
      {drawer.error && (
        <div className="banner banner--warning" role="alert">
          {drawer.error.message}
        </div>
      )}
      <main className="workspace__body">
        {view === 'sell' ? (
          <ShiftGate session={session}>{(open) => renderView(open)}</ShiftGate>
        ) : (
          renderView(null)
        )}
      </main>
      {closingShift && (
        <CloseShiftDialog
          key={closingShift.shift.id}
          open
          shift={closingShift}
          onClose={() => {
            setClosingShift(null);
          }}
        />
      )}
    </div>
  );
}
