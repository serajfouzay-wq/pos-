import { ShellScreen } from '../features/shell/ShellScreen';
import { useAppInfo } from '../ipc/queries';

export function App() {
  return <ShellScreen appInfo={useAppInfo()} />;
}
