import type { PrinterSettings, PrinterTarget } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useDiscoveredPrinters,
  useKickDrawer,
  usePrinterSettings,
  usePrinterStatus,
  useSavePrinterSettings,
  useTestPrinter,
} from '../../ipc/queries';

function label(target: PrinterTarget): string {
  switch (target.kind) {
    case 'tcp':
      return `${target.host}:${String(target.port)}`;
    case 'serial':
      return `${target.port} @ ${String(target.baud_rate)}`;
    case 'windows_printer':
      return target.name;
  }
}

function same(a: PrinterTarget, b: PrinterTarget): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function PrinterAdmin() {
  const { t } = useTranslation();
  const settings = usePrinterSettings();
  const discovered = useDiscoveredPrinters();
  const status = usePrinterStatus(true);
  const save = useSavePrinterSettings();
  const test = useTestPrinter();
  const drawer = useKickDrawer();
  // Unsaved edits; until the first edit the saved settings are shown.
  const [edited, setDraft] = useState<PrinterSettings | null>(null);
  const [host, setHost] = useState('');
  const [port, setPort] = useState('9100');

  const draft = edited ?? settings.data ?? null;

  if (!draft) return <p className="muted center">…</p>;
  const chain = draft.chain;
  const update = (next: PrinterTarget[]) => {
    setDraft({ ...draft, chain: next });
  };
  const addTarget = (target: PrinterTarget) => {
    if (chain.length < 3 && !chain.some((c) => same(c, target))) update([...chain, target]);
  };
  const move = (i: number, delta: number) => {
    const j = i + delta;
    if (j < 0 || j >= chain.length) return;
    const next = [...chain];
    const a = next[i];
    const b = next[j];
    if (a && b) {
      next[i] = b;
      next[j] = a;
      update(next);
    }
  };

  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('admin.printer.title')}</h1>
        {status.data && (
          <span className={status.data.online === false ? 'badge badge--warn' : 'badge'}>
            {status.data.online === null
              ? t('status.printerUnknown')
              : status.data.online
                ? t('status.printerOnline')
                : t('status.printerOffline')}
            {status.data.pending_jobs > 0 &&
              ` · ${t('status.pending', { count: status.data.pending_jobs })}`}
          </span>
        )}
      </header>

      <section className="card">
        <h2>{t('admin.printer.chain')}</h2>
        <p className="muted">{t('admin.printer.chainHelp')}</p>
        <ol className="chain">
          {chain.length === 0 && <li className="muted">{t('admin.printer.none')}</li>}
          {chain.map((target, i) => (
            <li key={label(target)}>
              <span className="badge">
                {i === 0 ? t('admin.printer.primary') : t('admin.printer.fallback')}
              </span>
              <span className="grow" dir="ltr">
                {label(target)}
              </span>
              <button
                type="button"
                className="icon-button"
                onClick={() => {
                  move(i, -1);
                }}
                aria-label="Up"
              >
                ↑
              </button>
              <button
                type="button"
                className="icon-button"
                onClick={() => {
                  move(i, 1);
                }}
                aria-label="Down"
              >
                ↓
              </button>
              <button
                type="button"
                className="link-button"
                disabled={test.isPending}
                onClick={() => {
                  test.mutate(target);
                }}
              >
                {t('admin.printer.test')}
              </button>
              <button
                type="button"
                className="icon-button"
                onClick={() => {
                  update(chain.filter((_, j) => j !== i));
                }}
                aria-label={t('sell.remove')}
              >
                ✕
              </button>
            </li>
          ))}
        </ol>
        {test.isSuccess && <p className="ok-text">{t('admin.printer.testOk')}</p>}
        {test.error && (
          <p role="alert" className="error-text">
            {test.error.message}
          </p>
        )}
        <label className="check">
          <input
            type="checkbox"
            checked={draft.open_drawer_on_cash}
            onChange={(e) => {
              setDraft({ ...draft, open_drawer_on_cash: e.target.checked });
            }}
          />
          {t('admin.printer.openDrawer')}
        </label>
        {save.error && (
          <p role="alert" className="error-text">
            {save.error.message}
          </p>
        )}
        <div className="row">
          <button
            type="button"
            className="button button--primary"
            disabled={save.isPending}
            onClick={() => {
              save.mutate(draft);
            }}
          >
            {t('common.save')}
          </button>
          <button
            type="button"
            className="button"
            disabled={drawer.isPending}
            onClick={() => {
              drawer.mutate();
            }}
          >
            {t('admin.printer.testDrawer')}
          </button>
        </div>
        {drawer.error && (
          <p role="alert" className="error-text">
            {drawer.error.message}
          </p>
        )}
      </section>

      <section className="card">
        <h2>{t('admin.printer.detected')}</h2>
        <ul className="chain">
          {discovered.data?.length === 0 && (
            <li className="muted">{t('admin.printer.noneDetected')}</li>
          )}
          {discovered.data?.map((p) => (
            <li key={p.label}>
              <span className="badge">{t(`admin.printer.connection.${p.connection}`)}</span>
              <span className="grow">{p.label}</span>
              <button
                type="button"
                className="link-button"
                onClick={() => {
                  addTarget(p.target);
                }}
              >
                {t('admin.printer.add')}
              </button>
            </li>
          ))}
        </ul>
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            const p = Number(port);
            if (host.trim() && Number.isInteger(p) && p > 0 && p < 65_536) {
              addTarget({ kind: 'tcp', host: host.trim(), port: p });
              setHost('');
            }
          }}
        >
          <input
            className="grow"
            dir="ltr"
            placeholder={t('admin.printer.hostPlaceholder')}
            value={host}
            onChange={(e) => {
              setHost(e.target.value);
            }}
          />
          <input
            className="port"
            dir="ltr"
            inputMode="numeric"
            value={port}
            onChange={(e) => {
              setPort(e.target.value.replace(/\D/g, ''));
            }}
          />
          <button type="submit" className="button">
            {t('admin.printer.addNetwork')}
          </button>
        </form>
      </section>
    </div>
  );
}
