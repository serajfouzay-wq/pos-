import { isActiveBuild, type ClientDetail } from '@pos/shared';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ErrorText } from '../../components/ErrorText';
import { useBuilds, useBuildSettings, useSigningKey, useStartBuild } from '../../ipc/queries';
import { useNavigationStore } from '../../stores/navigation';
import { BuildList } from './BuildList';

export function ClientBuildsTab({ detail, unsaved }: { detail: ClientDetail; unsaved: boolean }) {
  const { t } = useTranslation();
  const builds = useBuilds(detail.client_id);
  const settings = useBuildSettings();
  const key = useSigningKey();
  const start = useStartBuild();
  const navigate = useNavigationStore((s) => s.navigate);
  const [notes, setNotes] = useState('');
  const [publish, setPublish] = useState(true);

  const configured = Boolean(settings.data?.repo_owner && settings.data.token_configured);
  const hasKey = key.data !== undefined && key.data.state !== 'absent';
  const running = builds.data?.some((b) => isActiveBuild(b.status)) ?? false;
  const blocker = unsaved
    ? t('builds.start.saveFirst')
    : !configured
      ? t('builds.start.configureFirst')
      : !hasKey
        ? t('builds.start.keyFirst')
        : null;

  return (
    <div className="stack">
      <section className="card form">
        <h3>{t('builds.start.title')}</h3>
        <p className="muted">
          {t('builds.start.help', {
            repo: configured
              ? `${settings.data?.repo_owner ?? ''}/${settings.data?.repo_name ?? ''}`
              : '—',
            slug: detail.config.client_slug,
          })}
        </p>
        {blocker && (
          <p className="warning">
            {blocker}{' '}
            {!configured && !unsaved && (
              <button
                type="button"
                className="link"
                onClick={() => {
                  navigate('settings');
                }}
              >
                {t('nav.settings')}
              </button>
            )}
          </p>
        )}
        <label>
          <span>{t('builds.start.notes')}</span>
          <textarea
            rows={3}
            maxLength={1000}
            value={notes}
            onChange={(e) => {
              setNotes(e.target.value);
            }}
          />
          <span className="muted small">{t('builds.start.notesHelp')}</span>
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={publish}
            onChange={(e) => {
              setPublish(e.target.checked);
            }}
          />
          <span>
            {t('builds.start.publish')}
            <span className="muted small"> — {t('builds.start.publishHelp')}</span>
          </span>
        </label>
        <ErrorText error={start.error} />
        <button
          type="button"
          className="button button--primary"
          disabled={blocker !== null || start.isPending || running}
          onClick={() => {
            start.mutate(
              {
                clientId: detail.client_id,
                release: { release_notes: notes, publish_update: publish },
              },
              {
                onSuccess: () => {
                  setNotes('');
                },
              },
            );
          }}
        >
          {start.isPending
            ? t('builds.start.publishing')
            : running
              ? t('builds.start.running')
              : t('builds.start.submit')}
        </button>
      </section>
      <section className="card">
        <h3>{t('builds.history')}</h3>
        <ErrorText error={builds.error} />
        {builds.data && <BuildList builds={builds.data} showClient={false} />}
      </section>
    </div>
  );
}
