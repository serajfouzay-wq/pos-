import {
  isActiveBuild,
  type AssetKind,
  type BuildSettingsInput,
  type ClientConfig,
  type ClientDetail,
  type IssueLicenseRequest,
  type NewClientInput,
} from '@pos/shared';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { inTauri, ipc } from './index';

export const queryKeys = {
  appInfo: ['app_info'] as const,
  signingKey: ['signing_key'] as const,
  clients: ['clients'] as const,
  client: (id: string) => ['client', id] as const,
  asset: (id: string, kind: AssetKind, sha: string) => ['asset', id, kind, sha] as const,
  preview: (id: string, config: ClientConfig) => ['receipt_preview', id, config] as const,
  licenses: (id: string) => ['licenses', id] as const,
  builds: (id: string | null) => ['builds', id] as const,
  buildSettings: ['build_settings'] as const,
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
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: IssueLicenseRequest) => ipc.call('issue_license', { request }),
    onSuccess: (_issued, request) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.licenses(request.client_id) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.clients });
    },
  });
}

export function useIssuedLicenses(clientId: string) {
  return useQuery({
    queryKey: queryKeys.licenses(clientId),
    queryFn: () => ipc.call('list_issued_licenses', { client_id: clientId }),
  });
}

// ── Clients ────────────────────────────────────────────────────────────────

export function useClients() {
  return useQuery({
    queryKey: queryKeys.clients,
    queryFn: () => ipc.call('list_clients'),
    enabled: inTauri,
  });
}

export function useClient(clientId: string) {
  return useQuery({
    queryKey: queryKeys.client(clientId),
    queryFn: () => ipc.call('get_client', { client_id: clientId }),
  });
}

function useClientUpdated() {
  const queryClient = useQueryClient();
  return (detail: ClientDetail) => {
    queryClient.setQueryData(queryKeys.client(detail.client_id), detail);
    void queryClient.invalidateQueries({ queryKey: queryKeys.clients });
  };
}

export function useCreateClient() {
  const updated = useClientUpdated();
  return useMutation({
    mutationFn: (input: NewClientInput) => ipc.call('create_client', { input }),
    onSuccess: updated,
  });
}

export function useSaveClient() {
  const updated = useClientUpdated();
  return useMutation({
    mutationFn: (args: { clientId: string; config: ClientConfig; notes: string }) =>
      ipc.call('save_client', { client_id: args.clientId, config: args.config, notes: args.notes }),
    onSuccess: updated,
  });
}

export function useArchiveClient() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (clientId: string) => ipc.call('archive_client', { client_id: clientId }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.clients });
    },
  });
}

export function useUploadAsset() {
  const updated = useClientUpdated();
  return useMutation({
    mutationFn: (args: { clientId: string; kind: AssetKind; dataBase64: string }) =>
      ipc.call('upload_client_asset', {
        client_id: args.clientId,
        kind: args.kind,
        data_base64: args.dataBase64,
      }),
    onSuccess: updated,
  });
}

export function useRemoveAsset() {
  const updated = useClientUpdated();
  return useMutation({
    mutationFn: (args: { clientId: string; kind: AssetKind }) =>
      ipc.call('remove_client_asset', { client_id: args.clientId, kind: args.kind }),
    onSuccess: updated,
  });
}

/** The uploaded image as base64 (cached per content hash). */
export function useAssetData(clientId: string, kind: AssetKind, sha256: string | null) {
  return useQuery({
    queryKey: queryKeys.asset(clientId, kind, sha256 ?? ''),
    queryFn: () => ipc.call('get_client_asset', { client_id: clientId, kind }),
    enabled: sha256 !== null,
    staleTime: Number.POSITIVE_INFINITY,
  });
}

export function useReceiptPreview(clientId: string, config: ClientConfig, logoSha: string | null) {
  return useQuery({
    queryKey: [...queryKeys.preview(clientId, config), logoSha],
    queryFn: () => ipc.call('preview_receipt', { client_id: clientId, config }),
    placeholderData: keepPreviousData,
    retry: false,
  });
}

// ── Builds ─────────────────────────────────────────────────────────────────

const BUILD_POLL_MS = 15_000;

/**
 * Builds (all clients, or one). While any is in flight the list polls
 * `refresh_builds`, which follows the runs on GitHub from Rust.
 */
export function useBuilds(clientId: string | null) {
  const queryClient = useQueryClient();
  return useQuery({
    queryKey: queryKeys.builds(clientId),
    queryFn: async () => {
      const list = await ipc.call('list_builds', { client_id: clientId });
      if (!list.some((b) => isActiveBuild(b.status))) return list;
      const refreshed = await ipc.call('refresh_builds');
      if (refreshed.some((b) => !isActiveBuild(b.status))) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.clients });
      }
      return ipc.call('list_builds', { client_id: clientId });
    },
    enabled: inTauri,
    refetchInterval: (query) =>
      query.state.data?.some((b) => isActiveBuild(b.status)) ? BUILD_POLL_MS : false,
  });
}

function useBuildsChanged() {
  const queryClient = useQueryClient();
  return () => {
    void queryClient.invalidateQueries({ queryKey: ['builds'] });
    void queryClient.invalidateQueries({ queryKey: queryKeys.clients });
  };
}

export function useStartBuild() {
  const changed = useBuildsChanged();
  return useMutation({
    mutationFn: (clientId: string) => ipc.call('start_build', { client_id: clientId }),
    onSettled: changed,
  });
}

export function useDownloadBuild() {
  const changed = useBuildsChanged();
  return useMutation({
    mutationFn: (buildId: string) => ipc.call('download_build', { build_id: buildId }),
    onSuccess: changed,
  });
}

export function useOpenBuildRun() {
  return useMutation({
    mutationFn: (buildId: string) => ipc.call('open_build_run', { build_id: buildId }),
  });
}

export function useRevealBuild() {
  return useMutation({
    mutationFn: (buildId: string) => ipc.call('reveal_build_download', { build_id: buildId }),
  });
}

export function useBuildSettings() {
  return useQuery({
    queryKey: queryKeys.buildSettings,
    queryFn: () => ipc.call('get_build_settings'),
    enabled: inTauri,
  });
}

export function useSaveBuildSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { settings: BuildSettingsInput; githubToken: string | null }) =>
      ipc.call('save_build_settings', { settings: args.settings, github_token: args.githubToken }),
    onSuccess: (settings) => {
      queryClient.setQueryData(queryKeys.buildSettings, settings);
    },
  });
}

export function useClearGithubToken() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.call('clear_github_token'),
    onSuccess: (settings) => {
      queryClient.setQueryData(queryKeys.buildSettings, settings);
    },
  });
}

export function useCheckBuildSettings() {
  return useMutation({ mutationFn: () => ipc.call('check_build_settings') });
}
