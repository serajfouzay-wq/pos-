import type { BuildRecord } from '@pos/shared';
import { useTranslation } from 'react-i18next';
import { BuildStatusBadge } from '../../components/BuildStatusBadge';
import { ErrorText } from '../../components/ErrorText';
import { useDownloadBuild, useOpenBuildRun, useRevealBuild } from '../../ipc/queries';
import { formatBytes, formatDateTime } from '../../lib/format';

function BuildRow({ build, showClient }: { build: BuildRecord; showClient: boolean }) {
  const { t, i18n } = useTranslation();
  const open = useOpenBuildRun();
  const download = useDownloadBuild();
  const reveal = useRevealBuild();
  return (
    <tr>
      {showClient && (
        <td>
          <code>{build.client_slug}</code>
        </td>
      )}
      <td>
        <BuildStatusBadge status={build.status} />
        {build.message && <div className="muted small">{build.message}</div>}
        <ErrorText error={open.error ?? download.error ?? reveal.error} />
      </td>
      <td>
        {formatDateTime(build.requested_at, i18n.language)}
        <div className="muted small">
          v{build.app_version}
          {build.commit_sha && ` · ${build.commit_sha.slice(0, 7)}`}
        </div>
      </td>
      <td>
        {build.artifact_size !== null && (
          <span className="muted small">{formatBytes(build.artifact_size)}</span>
        )}
      </td>
      <td>
        <div className="row row--end">
          {build.run_url && (
            <button
              type="button"
              className="button"
              onClick={() => {
                open.mutate(build.build_id);
              }}
            >
              {t('builds.actions.openRun')}
            </button>
          )}
          {build.artifact_id !== null && (
            <button
              type="button"
              className="button"
              disabled={download.isPending}
              onClick={() => {
                download.mutate(build.build_id);
              }}
            >
              {download.isPending
                ? t('common.working')
                : build.download_path
                  ? t('builds.actions.downloadAgain')
                  : t('builds.actions.download')}
            </button>
          )}
          {build.download_path && (
            <button
              type="button"
              className="button"
              onClick={() => {
                reveal.mutate(build.build_id);
              }}
            >
              {t('builds.actions.reveal')}
            </button>
          )}
        </div>
      </td>
    </tr>
  );
}

export function BuildList({
  builds,
  showClient,
}: {
  builds: readonly BuildRecord[];
  showClient: boolean;
}) {
  const { t } = useTranslation();
  if (builds.length === 0) return <p className="muted">{t('builds.empty')}</p>;
  return (
    <table className="table">
      <thead>
        <tr>
          {showClient && <th>{t('builds.columns.client')}</th>}
          <th>{t('builds.columns.status')}</th>
          <th>{t('builds.columns.requested')}</th>
          <th>{t('builds.columns.size')}</th>
          <th />
        </tr>
      </thead>
      <tbody>
        {builds.map((build) => (
          <BuildRow key={build.build_id} build={build} showClient={showClient} />
        ))}
      </tbody>
    </table>
  );
}
