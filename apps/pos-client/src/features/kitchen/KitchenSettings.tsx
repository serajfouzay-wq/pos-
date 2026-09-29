import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useCheckForUpdates,
  useInspectUpdateFile,
  useInstallUpdateFile,
  useKitchenDisplayStatus,
  useSetKitchenDisplay,
  useUpdateStatus,
} from '../../ipc/queries';
import { formatDateTime } from '../../lib/dates';
import { useUiStore } from '../../stores/ui';

/** Owner: this till's kitchen display window. */
export function KitchenSettings() {
  const { t } = useTranslation();
  const status = useKitchenDisplayStatus();
  const set = useSetKitchenDisplay();
  const s = status.data;
  if (!s) return null;
  return (
    <section className="card">
      <h2>{t('kitchen.settings.title')}</h2>
      {s.available ? (
        <div className="stack">
          <p className="muted">{t('kitchen.settings.help')}</p>
          <label className="check">
            <input
              type="checkbox"
              checked={s.enabled}
              disabled={set.isPending}
              onChange={(e) => {
                set.mutate(e.target.checked);
              }}
            />
            {t('kitchen.settings.show')}
          </label>
          {s.enabled && (
            <div className="row">
              <button
                type="button"
                className="button"
                disabled={set.isPending}
                onClick={() => {
                  set.mutate(true);
                }}
              >
                {t('kitchen.settings.openNow')}
              </button>
            </div>
          )}
          {set.error && (
            <p role="alert" className="error-text">
              {set.error.message}
            </p>
          )}
        </div>
      ) : (
        <p className="muted">{t('kitchen.settings.unavailable')}</p>
      )}
    </section>
  );
}

/** Installing a `.posupdate` file from a USB stick (no internet needed). */
function UpdateFromFile() {
  const { t } = useTranslation();
  const locale = useUiStore((s) => s.locale);
  const inspect = useInspectUpdateFile();
  const install = useInstallUpdateFile();
  const [path, setPath] = useState<string | null>(null);
  const info = inspect.data;
  const choose = async () => {
    const chosen = await openDialog({
      multiple: false,
      directory: false,
      title: t('updates.file.choose'),
      filters: [{ name: t('updates.file.kind'), extensions: ['posupdate'] }],
    });
    if (typeof chosen !== 'string') return;
    setPath(chosen);
    install.reset();
    inspect.mutate(chosen);
  };
  return (
    <div className="stack">
      <h3>{t('updates.file.title')}</h3>
      <p className="muted small">{t('updates.file.help')}</p>
      <div className="row">
        <button
          type="button"
          className="button"
          disabled={inspect.isPending || install.isPending}
          onClick={() => {
            void choose();
          }}
        >
          {inspect.isPending ? t('updates.file.checking') : t('updates.file.choose')}
        </button>
      </div>
      {inspect.error && <p className="error-text">{inspect.error.message}</p>}
      {info && path && (
        <div className="card stack">
          <strong>
            {t('updates.file.found', {
              version: info.version,
              date: formatDateTime(info.created_at, locale),
            })}
          </strong>
          {info.notes && <p className="small pre-line">{info.notes}</p>}
          {info.newer ? (
            <button
              type="button"
              className="button button--primary"
              disabled={install.isPending}
              onClick={() => {
                if (window.confirm(t('updates.file.confirm', { version: info.version }))) {
                  install.mutate(path);
                }
              }}
            >
              {install.isPending ? t('updates.file.installing') : t('updates.file.install')}
            </button>
          ) : (
            <p className="muted">{t('updates.file.notNewer', { current: info.current_version })}</p>
          )}
          {install.error && <p className="error-text">{install.error.message}</p>}
        </div>
      )}
    </div>
  );
}

/** The running version, a manual update check, and updates from a file. */
export function UpdateSettings() {
  const { t } = useTranslation();
  const status = useUpdateStatus();
  const check = useCheckForUpdates();
  const s = status.data;
  if (!s) return null;
  return (
    <section className="card">
      <h2>{t('updates.version', { version: s.current_version })}</h2>
      {s.state !== 'unavailable' && (
        <div className="row">
          <button
            type="button"
            className="button"
            disabled={check.isPending || s.state === 'checking' || s.state === 'downloading'}
            onClick={() => {
              check.mutate();
            }}
          >
            {t('updates.check')}
          </button>
          {s.state === 'up_to_date' && (
            <span className="ok-text">{t('updates.upToDate', { version: s.current_version })}</span>
          )}
          {s.state === 'error' && (
            <span className="error-text">{t('updates.failed', { error: s.error ?? '' })}</span>
          )}
        </div>
      )}
      {s.file_updates ? (
        <UpdateFromFile />
      ) : (
        <p className="muted small">{t('updates.file.noKey')}</p>
      )}
    </section>
  );
}
