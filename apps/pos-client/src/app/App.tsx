import { useEffect, useRef } from 'react';
import { LicenseGate } from '../features/license/LicenseGate';
import { Workspace } from '../features/pos/Workspace';
import { SessionGate } from '../features/session/SessionGate';
import { useAppInfo, useSyncEvents } from '../ipc/queries';
import { useUiStore } from '../stores/ui';

export function App() {
  const appInfo = useAppInfo();
  const setLocale = useUiStore((s) => s.setLocale);
  const localeInitialised = useRef(false);
  useSyncEvents();

  // Adopt the client's configured default language once the core reports it.
  useEffect(() => {
    if (appInfo.data && !localeInitialised.current) {
      localeInitialised.current = true;
      setLocale(appInfo.data.client.locale.default);
    }
  }, [appInfo.data, setLocale]);

  return (
    <LicenseGate>
      <SessionGate>
        {(session) => <Workspace key={session.user_id} session={session} />}
      </SessionGate>
    </LicenseGate>
  );
}
