import type { BuildStatus } from '@pos/shared';
import { useTranslation } from 'react-i18next';

const TONE: Record<BuildStatus, 'ok' | 'warn' | 'bad' | 'busy'> = {
  publishing: 'busy',
  queued: 'busy',
  in_progress: 'busy',
  succeeded: 'ok',
  failed: 'bad',
  cancelled: 'warn',
  error: 'bad',
};

export function BuildStatusBadge({ status }: { status: BuildStatus }) {
  const { t } = useTranslation();
  return <span className={`badge badge--${TONE[status]}`}>{t(`builds.status.${status}`)}</span>;
}
