import { useTranslation } from 'react-i18next';
import {
  useCheckForUpdates,
  useKitchenDisplayStatus,
  useSetKitchenDisplay,
  useUpdateStatus,
} from '../../ipc/queries';

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

/** The running version and a manual update check. */
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
    </section>
  );
}
