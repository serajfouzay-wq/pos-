import type { Session } from '@pos/shared';
import { AnimatePresence, motion } from 'framer-motion';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/Modal';
import { useDismissUpdateNotice, useInstallUpdate, useUpdateStatus } from '../../ipc/queries';
import { can } from '../../lib/permissions';

/**
 * Update state under the top bar: the download, then "ready — installs at
 * the next restart" (managers may restart now). After an update, the first
 * sign-in shows what changed until someone dismisses it.
 */
export function UpdateBanner({ session }: { session: Session }) {
  const { t } = useTranslation();
  const status = useUpdateStatus();
  const install = useInstallUpdate();
  const dismiss = useDismissUpdateNotice();
  const [confirm, setConfirm] = useState(false);
  const s = status.data;
  if (!s) return null;
  const version = s.available_version ?? '';

  return (
    <>
      <AnimatePresence>
        {(s.state === 'ready' || s.state === 'downloading') && (
          <motion.div
            className="banner banner--info update-banner"
            role="status"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
          >
            <span className="grow">
              {s.state === 'ready'
                ? t('updates.ready', { version })
                : t('updates.downloading', {
                    version,
                    percent: Math.floor((s.progress_bps ?? 0) / 100),
                  })}
            </span>
            {s.state === 'downloading' && (
              <span className="update-banner__bar" aria-hidden="true">
                <motion.span
                  animate={{ width: `${String((s.progress_bps ?? 0) / 100)}%` }}
                  transition={{ ease: 'easeOut' }}
                />
              </span>
            )}
            {s.state === 'ready' && can(session, 'shift.close') && (
              <button
                type="button"
                className="button"
                onClick={() => {
                  install.reset();
                  setConfirm(true);
                }}
              >
                {t('updates.restartNow')}
              </button>
            )}
          </motion.div>
        )}
      </AnimatePresence>
      <Modal
        open={confirm}
        title={t('updates.restartNow')}
        onClose={() => {
          setConfirm(false);
        }}
      >
        <div className="stack">
          <p>{t('updates.restartConfirm', { version })}</p>
          {s.notes && <p className="update-notes">{s.notes}</p>}
          {install.error && (
            <p role="alert" className="error-text">
              {install.error.message}
            </p>
          )}
          <div className="row row--end">
            <button
              type="button"
              className="button"
              onClick={() => {
                setConfirm(false);
              }}
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              className="button button--primary"
              disabled={install.isPending}
              onClick={() => {
                install.mutate();
              }}
            >
              {t('updates.restartNow')}
            </button>
          </div>
        </div>
      </Modal>
      <Modal
        open={s.updated_from !== null}
        title={t('updates.updated', { version: s.current_version })}
      >
        <div className="stack">
          <motion.div
            className="update-check"
            initial={{ scale: 0.4, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            transition={{ type: 'spring', stiffness: 260, damping: 16 }}
            aria-hidden="true"
          >
            ✓
          </motion.div>
          <p>
            {t('updates.updatedFrom', {
              from: s.updated_from ?? '',
              version: s.current_version,
            })}
          </p>
          {s.updated_notes && (
            <>
              <h3>{t('updates.whatsNew')}</h3>
              <p className="update-notes">{s.updated_notes}</p>
            </>
          )}
          <button
            type="button"
            className="button button--primary"
            disabled={dismiss.isPending}
            onClick={() => {
              dismiss.mutate();
            }}
          >
            {t('updates.gotIt')}
          </button>
        </div>
      </Modal>
    </>
  );
}
