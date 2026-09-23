import { useQuery } from '@tanstack/react-query';
import { inTauri, ipc } from './index';

export const queryKeys = {
  appInfo: ['app_info'] as const,
};

/** Raised when the UI is loaded in a plain browser instead of the Tauri shell. */
export class NotInTauriError extends Error {
  override readonly name = 'NotInTauriError';
}

export function useAppInfo() {
  return useQuery({
    queryKey: queryKeys.appInfo,
    queryFn: () => {
      if (!inTauri) throw new NotInTauriError('Tauri runtime not detected');
      return ipc.call('app_info');
    },
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}
