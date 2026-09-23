import type { Session } from '@pos/shared';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useSessionStatus } from '../../ipc/queries';
import { LoginScreen } from './LoginScreen';
import { SetupOwner } from './SetupOwner';

/** Below this, a signed-in session is guaranteed. */
export function SessionGate({ children }: { children: (session: Session) => ReactNode }) {
  const { t } = useTranslation();
  const status = useSessionStatus();

  if (status.isPending) {
    return (
      <main className="shell">
        <p className="shell__status">{t('app.loading')}</p>
      </main>
    );
  }
  if (status.isError) {
    return (
      <main className="shell">
        <p role="alert" className="shell__status shell__status--error">
          {status.error.message}
        </p>
      </main>
    );
  }
  if (status.data.needs_setup) return <SetupOwner />;
  if (!status.data.session) return <LoginScreen />;
  return <>{children(status.data.session)}</>;
}
