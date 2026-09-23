import { formatMoney, type Locale, type PosAppInfo } from '@pos/shared';
import type { UseQueryResult } from '@tanstack/react-query';
import { AnimatePresence, motion } from 'framer-motion';
import type { CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { NotInTauriError } from '../../ipc/queries';
import { useUiStore } from '../../stores/ui';

const LOCALE_LABELS: Record<Locale, string> = { en: 'English', ar: 'العربية' };

interface Props {
  appInfo: UseQueryResult<PosAppInfo>;
}

export function ShellScreen({ appInfo }: Props) {
  const { t } = useTranslation();
  const { locale, setLocale } = useUiStore();
  const info = appInfo.data;

  const brandStyle = info
    ? ({
        '--brand-primary': info.client.branding.primary_color,
        '--brand-accent': info.client.branding.accent_color,
      } as CSSProperties)
    : undefined;

  return (
    <main className="shell" style={brandStyle}>
      <AnimatePresence mode="wait">
        {appInfo.isPending && (
          <motion.p key="loading" className="shell__status" exit={{ opacity: 0 }}>
            {t('app.loading')}
          </motion.p>
        )}

        {appInfo.isError && (
          <motion.p
            key="error"
            role="alert"
            className="shell__status shell__status--error"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
          >
            {appInfo.error instanceof NotInTauriError
              ? t('shell.notInTauri')
              : t('shell.error', { message: appInfo.error.message })}
          </motion.p>
        )}

        {info && (
          <motion.section
            key="ready"
            className="shell__card"
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ type: 'spring', stiffness: 260, damping: 26 }}
          >
            <header className="shell__header">
              <span className="shell__badge">
                {t(`shell.businessType.${info.client.business_type}`)}
              </span>
              <h1>{info.client.display_name}</h1>
              <p className="shell__muted">{t('shell.phase')}</p>
            </header>

            <dl className="shell__facts">
              <div>
                <dt>{t('shell.baseCurrency')}</dt>
                <dd>
                  {info.client.currency.base} · {formatMoney(0, info.client.currency.base, locale)}
                </dd>
              </div>
              <div>
                <dt>{t('shell.language')}</dt>
                <dd className="shell__locales">
                  {info.client.locale.supported.map((candidate) => (
                    <button
                      key={candidate}
                      type="button"
                      className="shell__locale"
                      aria-pressed={candidate === locale}
                      onClick={() => {
                        setLocale(candidate);
                      }}
                    >
                      {LOCALE_LABELS[candidate]}
                    </button>
                  ))}
                </dd>
              </div>
            </dl>

            <footer className="shell__muted">
              ✓ {t('shell.ready')} ·{' '}
              {t('shell.version', { version: info.version, profile: info.build_profile })}
            </footer>
          </motion.section>
        )}
      </AnimatePresence>
    </main>
  );
}
