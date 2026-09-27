/** The few window controls the UI needs (the kitchen display's full screen). */
import { getCurrentWindow } from '@tauri-apps/api/window';
import { inTauri } from './index';

/** Which window this bundle is running in (`?window=kds` for the kitchen). */
export function windowRole(): 'main' | 'kitchen' {
  return new URLSearchParams(window.location.search).get('window') === 'kds' ? 'kitchen' : 'main';
}

export async function toggleFullscreen(): Promise<boolean> {
  if (!inTauri) return false;
  const current = getCurrentWindow();
  const next = !(await current.isFullscreen());
  await current.setFullscreen(next);
  return next;
}
