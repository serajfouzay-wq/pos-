import type { ReportPrint, Session, Uuid } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import {
  useCurrentShift,
  usePrintReport,
  useRunZ,
  useShifts,
  useXReport,
  useZReport,
  useZReports,
} from '../../ipc/queries';
import { formatDateTime } from '../../lib/dates';
import { useMoney } from '../../lib/money';
import { can } from '../../lib/permissions';
import { useUiStore } from '../../stores/ui';
import { ReportView } from './ReportView';

type Tab = 'x' | 'z' | 'shifts';

/** A printout result: printed or not, and the text on screen either way. */
function PrintedDialog({ result, onClose }: { result: ReportPrint | null; onClose: () => void }) {
  const { t } = useTranslation();
  return (
    <Modal
      open={result !== null}
      wide
      title={
        result?.report.kind === 'z'
          ? t('reports.zTitle', { n: result.report.z_number ?? 0 })
          : t('reports.xTitle')
      }
      onClose={onClose}
    >
      {result && (
        <div className="stack">
          <p className={result.printed ? 'ok-text' : 'muted'}>
            {result.printed
              ? t('reports.printed')
              : t('reports.notPrinted', { error: result.print_error ?? '' })}
          </p>
          <pre className="ticket-text" dir="ltr">
            {result.text}
          </pre>
          <button type="button" className="button button--primary" onClick={onClose}>
            {t('common.done')}
          </button>
        </div>
      )}
    </Modal>
  );
}

function XTab({ session }: { session: Session }) {
  const { t } = useTranslation();
  const x = useXReport();
  const shift = useCurrentShift();
  const runZ = useRunZ();
  const print = usePrintReport();
  const [confirm, setConfirm] = useState(false);
  const [printed, setPrinted] = useState<ReportPrint | null>(null);
  const shiftOpen = Boolean(shift.data);

  return (
    <div className="stack">
      <div className="row">
        <button
          type="button"
          className="button"
          disabled={print.isPending}
          onClick={() => {
            print.mutate(null, { onSuccess: setPrinted });
          }}
        >
          {t('reports.printX')}
        </button>
        {can(session, 'report.z_run') && (
          <button
            type="button"
            className="button button--primary"
            disabled={shiftOpen || runZ.isPending}
            onClick={() => {
              runZ.reset();
              setConfirm(true);
            }}
          >
            {t('reports.runZ')}
          </button>
        )}
        {shiftOpen && <span className="muted small">{t('reports.closeShiftFirst')}</span>}
      </div>
      {(x.error ?? print.error) && (
        <p role="alert" className="error-text">
          {(x.error ?? print.error)?.message}
        </p>
      )}
      {x.data && <ReportView report={x.data} />}
      <Modal
        open={confirm}
        title={t('reports.runZ')}
        onClose={() => {
          setConfirm(false);
        }}
      >
        <div className="stack">
          <p>{t('reports.runZBody')}</p>
          {runZ.error && (
            <p role="alert" className="error-text">
              {runZ.error.message}
            </p>
          )}
          <div className="row row--end">
            <button
              type="button"
              className="button"
              onClick={() => {
                setConfirm(false);
              }}
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              className="button button--primary"
              disabled={runZ.isPending}
              onClick={() => {
                runZ.mutate(undefined, {
                  onSuccess: (result) => {
                    setConfirm(false);
                    setPrinted(result);
                  },
                });
              }}
            >
              {t('reports.runZConfirm')}
            </button>
          </div>
        </div>
      </Modal>
      <PrintedDialog
        result={printed}
        onClose={() => {
          setPrinted(null);
        }}
      />
    </div>
  );
}

function ZTab() {
  const { t } = useTranslation();
  const { format } = useMoney();
  const locale = useUiStore((s) => s.locale);
  const list = useZReports();
  const [openId, setOpenId] = useState<Uuid | null>(null);
  const report = useZReport(openId);
  const print = usePrintReport();
  const [printed, setPrinted] = useState<ReportPrint | null>(null);

  return (
    <div className="stack">
      <table className="table">
        <thead>
          <tr>
            <th>{t('reports.zNumber')}</th>
            <th>{t('reports.tillColumn')}</th>
            <th>{t('reports.period')}</th>
            <th>{t('reports.runBy')}</th>
            <th className="num">{t('reports.salesColumn')}</th>
            <th className="num">{t('reports.netSales')}</th>
            <th className="num">{t('reports.grandTotal')}</th>
          </tr>
        </thead>
        <tbody>
          {list.data?.length === 0 && (
            <tr>
              <td colSpan={7} className="muted">
                {t('reports.noZ')}
              </td>
            </tr>
          )}
          {list.data?.map((z) => (
            <tr
              key={z.id}
              className="table__link"
              tabIndex={0}
              onClick={() => {
                setOpenId(z.id);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') setOpenId(z.id);
              }}
            >
              <td>#{z.z_number}</td>
              <td>{z.device_label}</td>
              <td>
                {formatDateTime(z.period_start, locale)} → {formatDateTime(z.period_end, locale)}
              </td>
              <td>{z.run_by_name}</td>
              <td className="num">{z.sale_count}</td>
              <td className="num">{format(z.net_sales)}</td>
              <td className="num">{format(z.grand_total)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <Modal
        open={openId !== null}
        wide
        title={t('reports.zTitle', { n: report.data?.z_number ?? 0 })}
        onClose={() => {
          setOpenId(null);
        }}
      >
        {report.data && (
          <div className="stack">
            <ReportView report={report.data} />
            {print.error && (
              <p role="alert" className="error-text">
                {print.error.message}
              </p>
            )}
            <div className="row row--end">
              <button
                type="button"
                className="button"
                disabled={print.isPending}
                onClick={() => {
                  print.mutate(openId, {
                    onSuccess: (result) => {
                      setOpenId(null);
                      setPrinted(result);
                    },
                  });
                }}
              >
                {t('reports.printAgain')}
              </button>
            </div>
          </div>
        )}
      </Modal>
      <PrintedDialog
        result={printed}
        onClose={() => {
          setPrinted(null);
        }}
      />
    </div>
  );
}

function ShiftsTab() {
  const { t } = useTranslation();
  const { format } = useMoney();
  const locale = useUiStore((s) => s.locale);
  const [limit, setLimit] = useState(50);
  const shifts = useShifts({ limit });
  const rows = shifts.data ?? [];

  return (
    <div className="stack">
      <table className="table">
        <thead>
          <tr>
            <th>{t('reports.tillColumn')}</th>
            <th>{t('reports.openedAt')}</th>
            <th>{t('reports.closedAt')}</th>
            <th>{t('reports.people')}</th>
            <th className="num">{t('reports.salesColumn')}</th>
            <th className="num">{t('reports.netSales')}</th>
            <th className="num">{t('reports.expected')}</th>
            <th className="num">{t('reports.counted')}</th>
            <th className="num">{t('reports.variance')}</th>
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 && (
            <tr>
              <td colSpan={9} className="muted">
                {t('reports.noShifts')}
              </td>
            </tr>
          )}
          {rows.map(
            ({ shift, device_label, opened_by_name, closed_by_name, totals, expected_cash }) => (
              <tr key={shift.id}>
                <td>{device_label}</td>
                <td>{formatDateTime(shift.opened_at, locale)}</td>
                <td>
                  {shift.closed_at ? formatDateTime(shift.closed_at, locale) : t('reports.open')}
                </td>
                <td>
                  {opened_by_name}
                  {closed_by_name && closed_by_name !== opened_by_name
                    ? ` → ${closed_by_name}`
                    : ''}
                </td>
                <td className="num">{totals.transaction_count}</td>
                <td className="num">{format(totals.sales_total)}</td>
                <td className="num">{format(expected_cash)}</td>
                <td className="num">
                  {shift.actual_cash === null ? '—' : format(shift.actual_cash)}
                </td>
                <td
                  className={`num ${
                    shift.variance === null ? '' : shift.variance < 0 ? 'tone--bad' : 'tone--good'
                  }`}
                >
                  {shift.variance === null ? '—' : format(shift.variance)}
                </td>
              </tr>
            ),
          )}
        </tbody>
      </table>
      {rows.length >= limit && (
        <button
          type="button"
          className="button"
          onClick={() => {
            setLimit(limit + 50);
          }}
        >
          {t('common.more')}
        </button>
      )}
    </div>
  );
}

/** Managers and owners: the till's X report and Z closing, and history. */
export function ReportsScreen({ session }: { session: Session }) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<Tab>('x');
  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('reports.title')}</h1>
        <nav className="category-tabs" aria-label={t('reports.title')}>
          {(['x', 'z', 'shifts'] as const).map((id) => (
            <button
              key={id}
              type="button"
              aria-pressed={tab === id}
              onClick={() => {
                setTab(id);
              }}
            >
              {t(`reports.tabs.${id}`)}
            </button>
          ))}
        </nav>
      </header>
      {tab === 'x' && <XTab session={session} />}
      {tab === 'z' && <ZTab />}
      {tab === 'shifts' && <ShiftsTab />}
    </div>
  );
}
