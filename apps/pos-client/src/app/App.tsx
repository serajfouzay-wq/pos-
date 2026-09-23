import { useEffect, useRef } from 'react';
import { ShellScreen } from '../features/shell/ShellScreen';
import { useAppInfo } from '../ipc/queries';
import { useUiStore } from '../stores/ui';

export function App() {
  const appInfo = useAppInfo();
  const setLocale = useUiStore((s) => s.setLocale);
  const localeInitialised = useRef(false);

  // Adopt the client's configured default language once the core reports it.
  useEffect(() => {
    if (appInfo.data && !localeInitialised.current) {
      localeInitialised.current = true;
      setLocale(appInfo.data.client.locale.default);
    }
  }, [appInfo.data, setLocale]);

  return <ShellScreen appInfo={appInfo} />;
}
