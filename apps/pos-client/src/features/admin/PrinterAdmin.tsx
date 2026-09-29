import type { PrinterSettings, PrinterTarget } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { KitchenSettings, UpdateSettings } from '../kitchen/KitchenSettings';
import {
  useAppInfo,
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
    case 'cups':
      return target.name;
    case 'device':
      return target.path;
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
  const info = useAppInfo();
  const kitchenEnabled = info.data?.client.business_type !== 'retail';
  const buildPaper = info.data?.client.receipt.paper_width_mm ?? 80;
  // Unsaved edits; until the first edit the saved settings are shown.
  const [edited, setDraft] = useState<PrinterSettings | null>(null);
  const [host, setHost] = useState('');
  const [port, setPort] = useState('9100');

  const draft = edited ?? settings.data ?? null;

  if (!draft) return <p className="muted center">…</p>;
  // A test page tries the settings on screen, saved or not.
  const testPage = (target: PrinterTarget, paper: 58 | 80 | null) => {
    test.mutate({
      target,
      language: draft.language,
      mode: draft.mode,
      paper_width_mm: paper,
    });
  };
  const paperValue = (w: 58 | 80 | null) => (w === null ? '' : String(w));
  const parsePaper = (v: string): 58 | 80 | null => (v === '58' ? 58 : v === '80' ? 80 : null);
  const chain = draft.chain;
  const update = (next: PrinterTarget[]) => {
    setDraft({ ...draft, chain: next });
  };
  const setKitchen = (target: PrinterTarget | null) => {
    setDraft({ ...draft, kitchen: target });
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
                  testPage(target, draft.paper_width_mm);
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

      {kitchenEnabled && (
        <section className="card">
          <h2>{t('admin.printer.kitchen')}</h2>
          <p className="muted">{t('admin.printer.kitchenHelp')}</p>
          {status.data && status.data.kitchen_pending > 0 && (
            <p role="status" className="error-text">
              {t('admin.printer.kitchenWaiting', { count: status.data.kitchen_pending })}
              {status.data.kitchen_error && ` · ${status.data.kitchen_error}`}
            </p>
          )}
          <ol className="chain">
            {draft.kitchen ? (
              <li>
                <span className="badge">{t('admin.printer.kitchenBadge')}</span>
                <span className="grow" dir="ltr">
                  {label(draft.kitchen)}
                </span>
                <button
                  type="button"
                  className="link-button"
                  disabled={test.isPending}
                  onClick={() => {
                    if (draft.kitchen)
                      testPage(draft.kitchen, draft.kitchen_paper_width_mm ?? draft.paper_width_mm);
                  }}
                >
                  {t('admin.printer.test')}
                </button>
                <button
                  type="button"
                  className="icon-button"
                  onClick={() => {
                    setKitchen(null);
                  }}
                  aria-label={t('sell.remove')}
                >
                  ✕
                </button>
              </li>
            ) : (
              <li className="muted">{t('admin.printer.kitchenNone')}</li>
            )}
          </ol>
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
        </section>
      )}

      <section className="card">
        <h2>{t('admin.printer.printouts')}</h2>
        <p className="muted">{t('admin.printer.printoutsHelp')}</p>
        <div className="form-grid">
          <label className="field">
            <span>{t('admin.printer.language')}</span>
            <select
              value={draft.language ?? ''}
              onChange={(e) => {
                const v = e.target.value;
                setDraft({ ...draft, language: v === 'en' || v === 'ar' ? v : null });
              }}
            >
              <option value="">{t('admin.printer.languageDefault')}</option>
              <option value="en">English</option>
              <option value="ar">العربية</option>
            </select>
          </label>
          <label className="field">
            <span>{t('admin.printer.mode')}</span>
            <select
              value={draft.mode}
              onChange={(e) => {
                const v = e.target.value;
                setDraft({ ...draft, mode: v === 'text' || v === 'image' ? v : 'auto' });
              }}
            >
              <option value="auto">{t('admin.printer.modes.auto')}</option>
              <option value="image">{t('admin.printer.modes.image')}</option>
              <option value="text">{t('admin.printer.modes.text')}</option>
            </select>
          </label>
          <label className="field">
            <span>{t('admin.printer.paper')}</span>
            <select
              value={paperValue(draft.paper_width_mm)}
              onChange={(e) => {
                setDraft({ ...draft, paper_width_mm: parsePaper(e.target.value) });
              }}
            >
              <option value="">{t('admin.printer.paperDefault', { mm: buildPaper })}</option>
              <option value="58">58 mm</option>
              <option value="80">80 mm</option>
            </select>
          </label>
          {kitchenEnabled && (
            <label className="field">
              <span>{t('admin.printer.kitchenPaper')}</span>
              <select
                value={paperValue(draft.kitchen_paper_width_mm)}
                onChange={(e) => {
                  setDraft({ ...draft, kitchen_paper_width_mm: parsePaper(e.target.value) });
                }}
              >
                <option value="">{t('admin.printer.kitchenPaperSame')}</option>
                <option value="58">58 mm</option>
                <option value="80">80 mm</option>
              </select>
            </label>
          )}
        </div>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.auto_print_receipt}
            onChange={(e) => {
              setDraft({ ...draft, auto_print_receipt: e.target.checked });
            }}
          />
          {t('admin.printer.autoPrint')}
        </label>
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
              {kitchenEnabled && (
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    setKitchen(p.target);
                  }}
                >
                  {t('admin.printer.useForKitchen')}
                </button>
              )}
            </li>
          ))}
        </ul>
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            const p = Number(port);
            if (host.trim() && Number.isInteger(p) && p > 0 && p < 65_536) {
              const target: PrinterTarget = { kind: 'tcp', host: host.trim(), port: p };
              const submitter = (e.nativeEvent as SubmitEvent).submitter;
              if (submitter?.getAttribute('name') === 'kitchen') setKitchen(target);
              else addTarget(target);
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
          {kitchenEnabled && (
            <button type="submit" name="kitchen" className="button">
              {t('admin.printer.useForKitchen')}
            </button>
          )}
        </form>
      </section>
      <KitchenSettings />
      <UpdateSettings />
    </div>
  );
}
