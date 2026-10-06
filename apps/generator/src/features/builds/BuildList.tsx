import { isActiveBuild, type BuildRecord } from '@pos/shared';
import { useTranslation } from 'react-i18next';
import { BuildStatusBadge } from '../../components/BuildStatusBadge';
import { ErrorText } from '../../components/ErrorText';
import { useDownloadBuild, useOpenBuildRun, useRevealBuild } from '../../ipc/queries';
import { formatBytes, formatDateTime } from '../../lib/format';
import { useUiStore } from '../../stores/ui';

function BuildRow({
  build,
  showClient,
  latest,
}: {
  build: BuildRecord;
  showClient: boolean;
  /** The newest downloaded build: says what to do with it. */
  latest: boolean;
}) {
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
        {build.download_path && (
          <div className="small">
            {t('builds.savedIn')} <code dir="ltr">{build.download_path}</code>
            {latest && <div className="muted">{t('builds.next')}</div>}
          </div>
        )}
        <ErrorText error={open.error ?? download.error ?? reveal.error} />
      </td>
      <td>
        {formatDateTime(build.requested_at, i18n.language)}
        <div className="muted small">
          v{build.app_version}
          {build.commit_sha && ` · ${build.commit_sha.slice(0, 7)}`}
          {build.publish_update && <span className="badge">{t('builds.published')}</span>}
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
  const offline = useUiStore((s) => s.githubOffline);
  if (builds.length === 0) return <p className="muted">{t('builds.empty')}</p>;
  const latestDownload = builds.find((b) => b.download_path !== null)?.build_id;
  const active = builds.some((b) => isActiveBuild(b.status));
  return (
    <>
      {offline && active && (
        <p className="warning" role="status">
          {t('builds.offline')}
        </p>
      )}
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
            <BuildRow
              key={build.build_id}
              build={build}
              showClient={showClient}
              latest={build.build_id === latestDownload}
            />
          ))}
        </tbody>
      </table>
    </>
  );
}
