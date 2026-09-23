import type { SyncStatus } from '@pos/shared';
import type { TFunction } from 'i18next';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useSyncNow, useSyncStatus } from '../../ipc/queries';

function label(status: SyncStatus, t: TFunction): string {
  switch (status.state) {
    case 'disabled':
      return '';
    case 'syncing':
      return t('sync.syncing');
    case 'offline':
      return t('sync.offline');
    case 'error':
      return t('sync.error');
    case 'idle':
      return status.pending > 0 ? t('sync.waiting') : t('sync.synced');
  }
}

/**
 * Top-bar cloud status. Tapping it runs a round now; the till also syncs
 * every minute, after each change, and as soon as the network comes back.
 */
export function SyncIndicator() {
  const { t, i18n } = useTranslation();
  const status = useSyncStatus();
  const syncNow = useSyncNow();
  const { mutate } = syncNow;

  // The OS reports the network is back: don't wait for the next minute.
  useEffect(() => {
    const onOnline = () => {
      mutate();
    };
    window.addEventListener('online', onOnline);
    return () => {
      window.removeEventListener('online', onOnline);
    };
  }, [mutate]);

  const data = status.data;
  if (!data || data.state === 'disabled') return null;

  const healthy = data.state === 'idle' || data.state === 'syncing';
  const lastSynced = data.last_synced_at
    ? new Intl.DateTimeFormat(i18n.language, { timeStyle: 'short', dateStyle: 'short' }).format(
        new Date(data.last_synced_at),
      )
    : t('sync.never');
  const title = [
    t('sync.lastSynced', { when: lastSynced }),
    data.parked > 0 ? t('sync.parked', { count: data.parked }) : null,
    data.last_error,
  ]
    .filter(Boolean)
    .join('\n');

  return (
    <button
      type="button"
      className={`chip status-dot ${healthy ? '' : 'status-dot--warn'}`}
      title={title}
      aria-busy={data.state === 'syncing' || syncNow.isPending}
      disabled={syncNow.isPending}
      onClick={() => {
        mutate();
      }}
    >
      {label(data, t)}
      {data.pending > 0 && ` · ${t('status.pending', { count: data.pending })}`}
    </button>
  );
}
