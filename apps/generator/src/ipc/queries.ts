import type { IssueLicenseRequest } from '@pos/shared';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { inTauri, ipc } from './index';

export const queryKeys = {
  appInfo: ['app_info'] as const,
  signingKey: ['signing_key'] as const,
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

export function useSigningKey() {
  return useQuery({
    queryKey: queryKeys.signingKey,
    queryFn: () => ipc.call('license_key_status'),
    enabled: inTauri,
    retry: false,
  });
}

type KeyAction = 'create_license_key' | 'unlock_license_key' | 'lock_license_key';

export function useSigningKeyAction(action: KeyAction) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (passphrase: string) =>
      action === 'lock_license_key'
        ? ipc.call('lock_license_key')
        : ipc.call(action, { passphrase }),
    onSuccess: (status) => {
      queryClient.setQueryData(queryKeys.signingKey, status);
    },
  });
}

export function useDecodeActivation() {
  return useMutation({
    mutationFn: (code: string) => ipc.call('decode_activation_request', { code }),
  });
}

export function useIssueLicense() {
  return useMutation({
    mutationFn: (request: IssueLicenseRequest) => ipc.call('issue_license', { request }),
  });
}
