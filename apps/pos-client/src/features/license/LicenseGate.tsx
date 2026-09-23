import { isLicenseUsable, REACTIVATABLE_LICENSE_STATES } from '@pos/shared';
import { AnimatePresence, motion } from 'framer-motion';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { NotInTauriError, useAppInfo, useLicenseStatus } from '../../ipc/queries';
import { ActivationPanel } from './ActivationPanel';

/** Warn this many days before offline grace runs out. */
const GRACE_WARNING_DAYS = 3;

/**
 * Nothing below this component renders unless Rust reports a valid license.
 * (Rust enforces the same rule independently: without a valid license it
 * never opens the database, so there is no data to render anyway.)
 */
export function LicenseGate({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const license = useLicenseStatus();
  const appInfo = useAppInfo();

  if (license.isPending) {
    return (
      <main className="shell">
        <p className="shell__status">{t('license.checking')}</p>
      </main>
    );
  }

  if (license.isError) {
    return (
      <main className="shell">
        <p role="alert" className="shell__status shell__status--error">
          {license.error instanceof NotInTauriError
            ? t('shell.notInTauri')
            : t('shell.error', { message: license.error.message })}
        </p>
      </main>
    );
  }

  const status = license.data;
  if (!isLicenseUsable(status)) {
    const canActivate = REACTIVATABLE_LICENSE_STATES.includes(status.state);
    return (
      <main className="shell">
        <motion.section
          className="shell__card license"
          initial={{ opacity: 0, scale: 0.98 }}
          animate={{ opacity: 1, scale: 1 }}
        >
          <span className="license__lock" aria-hidden>
            🔒
          </span>
          <h1>{t(`license.state.${status.state}.title`)}</h1>
          <p className="shell__muted">{t(`license.state.${status.state}.body`)}</p>
          <p className="license__reason">{status.reason}</p>
          {canActivate && <ActivationPanel />}
        </motion.section>
      </main>
    );
  }

  const showGraceWarning =
    status.offline &&
    status.grace_days_remaining !== null &&
    status.grace_days_remaining <= GRACE_WARNING_DAYS;

  return (
    <>
      <AnimatePresence>
        {appInfo.data?.dev_license_key && (
          <motion.div
            key="dev-key"
            className="banner banner--danger"
            role="status"
            initial={{ y: -40 }}
            animate={{ y: 0 }}
          >
            {t('license.devKeyBanner')}
          </motion.div>
        )}
        {showGraceWarning && (
          <motion.div
            key="grace"
            className="banner banner--warning"
            role="status"
            initial={{ y: -40 }}
            animate={{ y: 0 }}
          >
            {t('license.graceBanner', { count: status.grace_days_remaining ?? 0 })}
          </motion.div>
        )}
      </AnimatePresence>
      {children}
    </>
  );
}
