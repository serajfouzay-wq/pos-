import { LOCALES, type GeneratorAppInfo, type Locale } from '@pos/shared';
import type { UseQueryResult } from '@tanstack/react-query';
import { AnimatePresence, motion } from 'framer-motion';
import type { JSX } from 'react';
import { useTranslation } from 'react-i18next';
import { NotInTauriError } from '../../ipc/queries';
import { SECTIONS, useNavigationStore, type Section } from '../../stores/navigation';
import { BuildsScreen } from '../builds/BuildsScreen';
import { ClientsScreen } from '../clients/ClientsScreen';
import { LicensesScreen } from '../licenses/LicensesScreen';
import { SettingsScreen } from '../settings/SettingsScreen';

import { useUiStore } from '../../stores/ui';

const SCREENS: Record<Section, () => JSX.Element | null> = {
  clients: ClientsScreen,
  builds: BuildsScreen,
  licenses: LicensesScreen,
  settings: SettingsScreen,
};

const LOCALE_LABELS: Record<Locale, string> = { en: 'English', ar: 'العربية' };

interface Props {
  appInfo: UseQueryResult<GeneratorAppInfo>;
}

export function ShellScreen({ appInfo }: Props) {
  const { t } = useTranslation();
  const { locale, setLocale } = useUiStore();
  const { section, navigate, closeClient } = useNavigationStore();
  const Screen = SCREENS[section];

  return (
    <div className="layout">
      <nav className="sidebar" aria-label={t('app.title')}>
        <div className="sidebar__brand">{t('app.title')}</div>
        {SECTIONS.map((candidate) => (
          <button
            key={candidate}
            type="button"
            className="sidebar__item"
            aria-current={candidate === section ? 'page' : undefined}
            onClick={() => {
              if (candidate === 'clients' && section === 'clients') closeClient();
              navigate(candidate);
            }}
          >
            {candidate === section && (
              <motion.span layoutId="sidebar-active" className="sidebar__indicator" />
            )}
            <span>{t(`nav.${candidate}`)}</span>
          </button>
        ))}

        <div className="sidebar__footer">
          <span className="muted">{t('shell.language')}</span>
          <div className="sidebar__locales">
            {LOCALES.map((candidate) => (
              <button
                key={candidate}
                type="button"
                className="chip"
                aria-pressed={candidate === locale}
                onClick={() => {
                  setLocale(candidate);
                }}
              >
                {LOCALE_LABELS[candidate]}
              </button>
            ))}
          </div>
          {appInfo.data && (
            <span className="muted">
              {t('shell.version', {
                version: appInfo.data.version,
                profile: appInfo.data.build_profile,
              })}
            </span>
          )}
        </div>
      </nav>

      <main className="content">
        {appInfo.isError && (
          <p role="alert" className="error">
            {appInfo.error instanceof NotInTauriError
              ? t('shell.notInTauri')
              : t('shell.error', { message: appInfo.error.message })}
          </p>
        )}
        <AnimatePresence mode="wait">
          <motion.section
            key={section}
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -12 }}
            transition={{ duration: 0.18 }}
          >
            <h1>{t(`nav.${section}`)}</h1>
            <Screen />
          </motion.section>
        </AnimatePresence>
      </main>
    </div>
  );
}
