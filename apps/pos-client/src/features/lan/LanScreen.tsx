import type { LanRole, LanSettings } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useDiscoverHubs,
  useLanStatus,
  useNewHubCode,
  useSaveLanSettings,
  useTestHub,
} from '../../ipc/queries';

const ROLES: readonly LanRole[] = ['off', 'hub', 'client'];

/** Owner: this till's part in the shop network (tills syncing without internet). */
export function LanScreen() {
  const { t } = useTranslation();
  const status = useLanStatus();
  const save = useSaveLanSettings();
  const discover = useDiscoverHubs();
  const test = useTestHub();
  const newCode = useNewHubCode();
  const [edited, setEdited] = useState<LanSettings | null>(null);
  const data = status.data;
  if (!data) return <p className="muted center">{status.error?.message ?? '…'}</p>;
  const draft = edited ?? data.settings;
  const set = (patch: Partial<LanSettings>) => {
    setEdited({ ...draft, ...patch });
    test.reset();
  };
  const hub = data.hub;

  return (
    <div className="admin">
      <header className="admin__header">
        <h1>{t('lan.title')}</h1>
      </header>
      <p className="muted">{t('lan.help')}</p>

      <section className="lan-roles" role="radiogroup" aria-label={t('lan.role')}>
        {ROLES.map((role) => (
          <button
            key={role}
            type="button"
            role="radio"
            aria-checked={draft.role === role}
            className="lan-role"
            onClick={() => {
              set({ role });
            }}
          >
            <strong>{t(`lan.roles.${role}`)}</strong>
            <span className="muted small">{t(`lan.roles.${role}Help`)}</span>
          </button>
        ))}
      </section>

      {draft.role === 'client' && (
        <section className="card">
          <h2>{t('lan.joinTitle')}</h2>
          <div className="row row--wrap">
            <button
              type="button"
              className="button"
              disabled={discover.isPending}
              onClick={() => {
                discover.mutate(draft.port);
              }}
            >
              {discover.isPending ? t('lan.searching') : t('lan.find')}
            </button>
            {discover.data?.length === 0 && (
              <span className="muted small">{t('lan.noneFound')}</span>
            )}
            {discover.data?.map((h) => (
              <button
                key={h.address}
                type="button"
                className="chip"
                aria-pressed={draft.hub_address === h.address}
                onClick={() => {
                  set({ hub_address: h.address });
                }}
              >
                {h.name} · <span dir="ltr">{h.address}</span>
              </button>
            ))}
          </div>
          <div className="form-grid">
            <label className="field">
              <span>{t('lan.address')}</span>
              <input
                dir="ltr"
                placeholder="192.168.1.10"
                value={draft.hub_address ?? ''}
                onChange={(e) => {
                  set({ hub_address: e.target.value || null });
                }}
              />
            </label>
            <label className="field">
              <span>{t('lan.code')}</span>
              <input
                dir="ltr"
                className="mono"
                placeholder="ABCD-EFGH"
                value={draft.hub_code ?? ''}
                onChange={(e) => {
                  set({ hub_code: e.target.value.toUpperCase() || null });
                }}
              />
            </label>
          </div>
          <div className="row row--wrap">
            <button
              type="button"
              className="button"
              disabled={!draft.hub_address || !draft.hub_code || test.isPending}
              onClick={() => {
                test.mutate({
                  address: draft.hub_address ?? '',
                  code: draft.hub_code ?? '',
                  port: draft.port,
                });
              }}
            >
              {t('lan.test')}
            </button>
            {test.data && (
              <span className="ok-text">
                {t('lan.testOk', { name: test.data.hub_name, tills: test.data.tills })}
              </span>
            )}
            {test.error && <span className="error-text">{test.error.message}</span>}
          </div>
        </section>
      )}

      {draft.role === 'hub' && hub && data.settings.role === 'hub' && (
        <section className="card lan-hub">
          <h2>{t('lan.hubTitle')}</h2>
          <p className="muted">{t('lan.hubHelp')}</p>
          <div className="lan-hub__code">
            <span className="muted small">{t('lan.code')}</span>
            <strong className="mono" dir="ltr">
              {hub.code}
            </strong>
          </div>
          <div>
            <span className="muted small">{t('lan.addresses')}</span>
            <ul className="lan-hub__addresses" dir="ltr">
              {hub.addresses.length === 0 && <li className="muted">—</li>}
              {hub.addresses.map((a) => (
                <li key={a} className="mono">
                  {a}:{hub.port}
                </li>
              ))}
            </ul>
          </div>
          <p className="small">
            <span className={hub.running ? 'tone--good' : 'tone--bad'}>
              {hub.running ? t('lan.running') : t('lan.stopped')}
            </span>{' '}
            · {t('lan.stats', { rows: hub.rows, tills: hub.tills })}
          </p>
          <p className="muted small">{t('lan.firewall')}</p>
          <button
            type="button"
            className="link-button"
            disabled={newCode.isPending}
            onClick={() => {
              if (window.confirm(t('lan.newCodeConfirm'))) newCode.mutate();
            }}
          >
            {t('lan.newCode')}
          </button>
        </section>
      )}

      <section className="card">
        <details>
          <summary>{t('lan.advanced')}</summary>
          <label className="field">
            <span>{t('lan.port')}</span>
            <input
              dir="ltr"
              inputMode="numeric"
              className="port"
              value={draft.port}
              onChange={(e) => {
                const port = Number(e.target.value.replace(/\D/g, '') || '0');
                set({ port });
              }}
            />
          </label>
        </details>
        {(save.error ?? data.last_error) && (
          <p role="alert" className="error-text">
            {save.error?.message ?? data.last_error}
          </p>
        )}
        <button
          type="button"
          className="button button--primary"
          disabled={save.isPending || !edited}
          onClick={() => {
            save.mutate(draft, {
              onSuccess: () => {
                setEdited(null);
              },
            });
          }}
        >
          {save.isPending ? t('common.working') : t('common.save')}
        </button>
      </section>
    </div>
  );
}
