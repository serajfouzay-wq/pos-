import {
  LOCALES,
  type KitchenChange,
  type Locale,
  type Session,
  type ShiftSummary,
} from '@pos/shared';
import { AnimatePresence, motion } from 'framer-motion';
import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useToast } from '../../components/Toast';
import {
  useAppInfo,
  useCurrentShift,
  useKickDrawer,
  useKitchenReady,
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
import { AuditScreen } from '../audit/AuditScreen';
import { BackupsScreen } from '../backups/BackupsScreen';
import { CustomersScreen } from '../customers/CustomersScreen';
import { DashboardScreen } from '../dashboard/DashboardScreen';
import { DiscountsScreen } from '../discounts/DiscountsScreen';
import { MembershipsScreen } from '../memberships/MembershipsScreen';
import { HistoryScreen } from '../history/HistoryScreen';
import { ReportsScreen } from '../reports/ReportsScreen';
import { SellScreen } from '../sell/SellScreen';
import { SyncIndicator } from '../sync/SyncIndicator';
import { CloseShiftDialog } from '../shift/CloseShiftDialog';
import { ShiftGate } from '../shift/ShiftGate';
import { UpdateBanner } from '../updates/UpdateBanner';

type View =
  | 'sell'
  | 'history'
  | 'reports'
  | 'dashboard'
  | 'customers'
  | 'memberships'
  | 'discounts'
  | 'products'
  | 'menu'
  | 'floor'
  | 'stock'
  | 'users'
  | 'printer'
  | 'backups'
  | 'audit';
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
  // The kitchen display on this till tells the front when food is ready.
  const toast = useToast(6000);
  const showToast = toast.show;
  const onReady = useCallback(
    (change: KitchenChange) => {
      showToast(t('kitchen.readyToast', { number: change.ticket_number, title: change.title }));
    },
    [showToast, t],
  );
  useKitchenReady(onReady);

  // What each role sees follows the permission matrix Rust sent with the
  // session (Rust re-checks every command). Front of house on the bar;
  // back office in its menu.
  const views: { id: View; allowed: boolean }[] = [
    { id: 'sell', allowed: true },
    { id: 'history', allowed: can(session, 'receipt.reprint') },
    { id: 'reports', allowed: can(session, 'report.view') },
    { id: 'dashboard', allowed: can(session, 'analytics.view') },
  ];
  const backOffice: { id: View; allowed: boolean }[] = [
    { id: 'customers', allowed: can(session, 'customer.lookup') },
    { id: 'memberships', allowed: can(session, 'customer.lookup') },
    { id: 'products', allowed: can(session, 'catalog.manage') },
    { id: 'discounts', allowed: can(session, 'catalog.manage') },
    { id: 'menu', allowed: business !== 'retail' && can(session, 'catalog.manage') },
    { id: 'floor', allowed: business !== 'retail' && can(session, 'catalog.manage') },
    { id: 'stock', allowed: mayViewStock },
    { id: 'users', allowed: can(session, 'user.manage') },
    { id: 'printer', allowed: can(session, 'settings.manage') },
    { id: 'backups', allowed: can(session, 'settings.manage') },
    { id: 'audit', allowed: can(session, 'audit.view') },
  ];
  const office = backOffice.filter((v) => v.allowed);
  const [menuOpen, setMenuOpen] = useState(false);
  const supported = info.data?.client.locale.supported ?? LOCALES;

  const renderView = (shift: ShiftSummary | null) => {
    switch (view) {
      case 'history':
        return <HistoryScreen session={session} />;
      case 'reports':
        return <ReportsScreen session={session} />;
      case 'dashboard':
        return (
          <DashboardScreen
            onLowStock={() => {
              setView('stock');
            }}
          />
        );
      case 'audit':
        return <AuditScreen />;
      case 'customers':
        return <CustomersScreen session={session} />;
      case 'products':
        return <ProductsAdmin />;
      case 'discounts':
        return <DiscountsScreen />;
      case 'memberships':
        return <MembershipsScreen session={session} />;
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
      case 'backups':
        return <BackupsScreen />;
      case 'sell':
        return shift ? <SellScreen session={session} /> : null;
    }
  };

  const printerState = printer.data;
  const printerLabel = [
    !printerState?.configured
      ? t('status.noPrinter')
      : printerState.online === false
        ? t('status.printerOffline')
        : t('status.printer'),
    printerState && printerState.pending_jobs > 0
      ? t('status.pending', { count: printerState.pending_jobs })
      : null,
    printerState && printerState.kitchen_pending > 0
      ? t('admin.printer.kitchenWaiting', { count: printerState.kitchen_pending })
      : null,
  ]
    .filter(Boolean)
    .join(' · ');
  const printerClass =
    !printerState?.configured || printerState.online === false || printerState.kitchen_pending > 0
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
        {office.length > 0 && (
          <div className="nav-menu">
            <button
              type="button"
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              aria-current={office.some((v) => v.id === view) ? 'page' : undefined}
              onClick={() => {
                setMenuOpen(!menuOpen);
              }}
            >
              {office.find((v) => v.id === view) ? t(`nav.${view}`) : t('nav.backOffice')} ▾
            </button>
            {menuOpen && (
              <>
                <div
                  className="nav-menu__backdrop"
                  onClick={() => {
                    setMenuOpen(false);
                  }}
                />
                <ul className="nav-menu__list" role="menu">
                  {office.map((v) => (
                    <li key={v.id} role="none">
                      <button
                        type="button"
                        role="menuitem"
                        aria-current={view === v.id ? 'page' : undefined}
                        onClick={() => {
                          setView(v.id);
                          setMenuOpen(false);
                        }}
                      >
                        {t(`nav.${v.id}`)}
                      </button>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </div>
        )}
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
          <span
            className={printerClass}
            title={[printerLabel, printerState?.last_error].filter(Boolean).join(' · ')}
          >
            <span className="status-dot__label">{printerLabel}</span>
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
      <UpdateBanner session={session} />
      <main className="workspace__body">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={view}
            className="workspace__view"
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.16, ease: 'easeOut' }}
          >
            {view === 'sell' ? (
              <ShiftGate session={session}>{(open) => renderView(open)}</ShiftGate>
            ) : (
              renderView(null)
            )}
          </motion.div>
        </AnimatePresence>
      </main>
      {toast.node}
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
