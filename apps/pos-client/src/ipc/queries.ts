import type { LicenseStatus } from '@pos/shared';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';
import { subscribe } from './events';
import { inTauri, ipc } from './index';

export const queryKeys = {
  appInfo: ['app_info'] as const,
  license: ['license'] as const,
  activationRequest: ['activation_request'] as const,
};

/** Raised when the UI is loaded in a plain browser instead of the Tauri shell. */
export class NotInTauriError extends Error {
  override readonly name = 'NotInTauriError';
}

function requireTauri(): void {
  if (!inTauri) throw new NotInTauriError('Tauri runtime not detected');
}

export function useAppInfo() {
  return useQuery({
    queryKey: queryKeys.appInfo,
    queryFn: () => {
      requireTauri();
      return ipc.call('app_info');
    },
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

/**
 * Current license status. Rust pushes changes (`license://status`), so the
 * UI reacts to revocation or grace running out without polling.
 */
export function useLicenseStatus() {
  const queryClient = useQueryClient();
  useEffect(
    () =>
      subscribe('license_status', (status) => {
        queryClient.setQueryData<LicenseStatus>(queryKeys.license, status);
      }),
    [queryClient],
  );
  return useQuery({
    queryKey: queryKeys.license,
    queryFn: () => {
      requireTauri();
      return ipc.call('verify_license');
    },
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

export function useActivationRequest(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.activationRequest,
    queryFn: () => ipc.call('get_activation_request'),
    enabled: enabled && inTauri,
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

export function useActivateLicense() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (token: string) => ipc.call('activate_license', { token }),
    onSuccess: (status) => {
      // A rejected token does not change the installed license, so only a
      // successful activation replaces the cached status.
      if (status.state === 'valid') queryClient.setQueryData(queryKeys.license, status);
    },
  });
}
