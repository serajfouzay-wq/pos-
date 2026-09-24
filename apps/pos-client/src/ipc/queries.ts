import type {
  CommandArgs,
  ComboInput,
  DiningTableInput,
  LicenseStatus,
  ModifierGroupInput,
  OpenOrderInput,
  OpenOrderUpdate,
  OpenOrderView,
  PosIpcContract,
  StockAdjustment,
  PrinterSettings,
  PrinterStatus,
  PrinterTarget,
  ProductFilter,
  ProductInput,
  QuoteRequest,
  Role,
  SessionStatus,
  SyncStatus,
  TransactionPayloadInput,
} from '@pos/shared';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';
import { subscribe } from './events';
import { inTauri, ipc } from './index';

export const queryKeys = {
  appInfo: ['app_info'] as const,
  license: ['license'] as const,
  activationRequest: ['activation_request'] as const,
  session: ['session'] as const,
  loginUsers: ['login_users'] as const,
  users: ['users'] as const,
  shift: ['shift'] as const,
  products: (filter: ProductFilter) => ['products', filter] as const,
  categories: ['categories'] as const,
  quote: (request: QuoteRequest) => ['quote', request] as const,
  printerStatus: ['printer_status'] as const,
  printerSettings: ['printer_settings'] as const,
  printers: ['printers'] as const,
  syncStatus: ['sync_status'] as const,
  menu: (includeInactive: boolean) => ['menu', includeInactive] as const,
  openOrders: ['open_orders'] as const,
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

// ── License ────────────────────────────────────────────────────────────────

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
      // A rejected token does not change the installed license.
      if (status.state === 'valid') queryClient.setQueryData(queryKeys.license, status);
    },
  });
}

// ── Session ────────────────────────────────────────────────────────────────

export function useSessionStatus() {
  return useQuery({
    queryKey: queryKeys.session,
    queryFn: () => ipc.call('session_status'),
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

export function useLoginUsers(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.loginUsers,
    queryFn: () => ipc.call('list_login_users'),
    enabled,
  });
}

function useSetSession() {
  const queryClient = useQueryClient();
  return (status: SessionStatus) => {
    // Nothing cached for the previous user may survive a user switch.
    queryClient.removeQueries({
      predicate: (q) => !['app_info', 'license', 'session'].includes(String(q.queryKey[0])),
    });
    queryClient.setQueryData(queryKeys.session, status);
  };
}

export function useBootstrapOwner() {
  const setSession = useSetSession();
  return useMutation({
    mutationFn: (input: { display_name: string; pin: string }) =>
      ipc.call('bootstrap_owner', input),
    onSuccess: (session) => {
      setSession({ needs_setup: false, session });
    },
  });
}

export function useLogin() {
  const setSession = useSetSession();
  return useMutation({
    mutationFn: (input: { user_id: string; pin: string }) => ipc.call('login', input),
    onSuccess: (session) => {
      setSession({ needs_setup: false, session });
    },
  });
}

export function useLogout() {
  const setSession = useSetSession();
  return useMutation({
    mutationFn: () => ipc.call('logout'),
    onSettled: () => {
      setSession({ needs_setup: false, session: null });
    },
  });
}

// ── Users ──────────────────────────────────────────────────────────────────

export function useUsers() {
  return useQuery({ queryKey: queryKeys.users, queryFn: () => ipc.call('list_users') });
}

export function useCreateUser() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { display_name: string; role: Role; pin: string }) =>
      ipc.call('create_user', input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.users });
      void queryClient.invalidateQueries({ queryKey: queryKeys.loginUsers });
    },
  });
}

// ── Shifts ─────────────────────────────────────────────────────────────────

export function useCurrentShift() {
  return useQuery({ queryKey: queryKeys.shift, queryFn: () => ipc.call('current_shift') });
}

export function useOpenShift() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (opening_float: number) => ipc.call('open_shift', { opening_float }),
    onSuccess: (summary) => {
      queryClient.setQueryData(queryKeys.shift, summary);
    },
  });
}

export function useCloseShift() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { actual_cash: number; closing_float: number; notes: string | null }) =>
      ipc.call('close_shift', input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.shift });
    },
  });
}

// ── Catalogue ──────────────────────────────────────────────────────────────

export function useProducts(filter: ProductFilter, enabled = true) {
  return useQuery({
    queryKey: queryKeys.products(filter),
    queryFn: () => ipc.call('get_products', { filter }),
    placeholderData: keepPreviousData,
    enabled,
  });
}

export function useCategories() {
  return useQuery({ queryKey: queryKeys.categories, queryFn: () => ipc.call('get_categories') });
}

export function useSaveProduct() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (product: ProductInput) => ipc.call('save_product', { product }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['products'] });
    },
  });
}

export function useLoadSampleCatalog() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.call('load_sample_catalog'),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['products'] });
      void queryClient.invalidateQueries({ queryKey: queryKeys.categories });
      void queryClient.invalidateQueries({ queryKey: ['menu'] });
    },
  });
}

// ── Sales ──────────────────────────────────────────────────────────────────

/** Totals always come from Rust; the UI never adds up money itself. */
export function useQuote(request: QuoteRequest | null) {
  return useQuery({
    queryKey: queryKeys.quote(request ?? { items: [], discount_rule_ids: [] }),
    queryFn: () =>
      ipc.call('quote_transaction', { request: request ?? { items: [], discount_rule_ids: [] } }),
    enabled: request !== null && request.items.length > 0,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

export function useCreateTransaction() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (payload: TransactionPayloadInput) => ipc.call('create_transaction', { payload }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.shift });
      void queryClient.invalidateQueries({ queryKey: ['products'] });
    },
  });
}

export function usePrintReceipt() {
  return useMutation({
    mutationFn: (transaction_id: string) => ipc.call('print_receipt', { transaction_id }),
  });
}

export function useKickDrawer() {
  return useMutation({ mutationFn: () => ipc.call('kick_cash_drawer') });
}

// ── Printers ───────────────────────────────────────────────────────────────

export function usePrinterStatus(enabled: boolean) {
  const queryClient = useQueryClient();
  useEffect(
    () =>
      subscribe('printer_status', (status) => {
        queryClient.setQueryData<PrinterStatus>(queryKeys.printerStatus, status);
      }),
    [queryClient],
  );
  return useQuery({
    queryKey: queryKeys.printerStatus,
    queryFn: () => ipc.call('printer_status'),
    enabled,
    refetchInterval: 60_000,
  });
}

export function usePrinterSettings() {
  return useQuery({
    queryKey: queryKeys.printerSettings,
    queryFn: () => ipc.call('get_printer_settings'),
  });
}

export function useDiscoveredPrinters() {
  return useQuery({ queryKey: queryKeys.printers, queryFn: () => ipc.call('list_printers') });
}

export function useSavePrinterSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: PrinterSettings) => ipc.call('save_printer_settings', { settings }),
    onSuccess: (settings) => {
      queryClient.setQueryData(queryKeys.printerSettings, settings);
      void queryClient.invalidateQueries({ queryKey: queryKeys.printerStatus });
    },
  });
}

export function useTestPrinter() {
  return useMutation({
    mutationFn: (target: PrinterTarget) => ipc.call('test_printer', { target }),
  });
}

// ── Cloud sync ─────────────────────────────────────────────────────────────

/** Local data another till may have changed; refetched after a sync round. */
const SYNCED_QUERIES = [
  ['products'],
  ['menu'],
  queryKeys.openOrders,
  queryKeys.categories,
  queryKeys.users,
  queryKeys.loginUsers,
  queryKeys.session,
] as const;

/**
 * Keeps the UI in step with the background sync worker: every completed
 * round refetches synced data (a new till gets the shop's users and
 * catalogue without a restart). Mounted once, above the session gate.
 */
export function useSyncEvents() {
  const queryClient = useQueryClient();
  useEffect(() => {
    let previous: SyncStatus['state'] | undefined;
    return subscribe('sync_status', (status) => {
      queryClient.setQueryData<SyncStatus>(queryKeys.syncStatus, status);
      if (status.state === 'idle' && previous === 'syncing') {
        for (const queryKey of SYNCED_QUERIES) void queryClient.invalidateQueries({ queryKey });
      }
      previous = status.state;
    });
  }, [queryClient]);
}

export function useSyncStatus() {
  return useQuery({
    queryKey: queryKeys.syncStatus,
    queryFn: () => ipc.call('sync_status'),
    refetchInterval: 60_000,
  });
}

export function useSyncNow() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.call('sync_to_cloud'),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.syncStatus });
    },
  });
}

// ── Menu & floor (Phase 6) ─────────────────────────────────────────────────

export function useMenu(includeInactive = false) {
  return useQuery({
    queryKey: queryKeys.menu(includeInactive),
    queryFn: () => ipc.call('get_menu', { include_inactive: includeInactive }),
    staleTime: 30_000,
  });
}

function useMenuChanged() {
  const queryClient = useQueryClient();
  return () => {
    void queryClient.invalidateQueries({ queryKey: ['menu'] });
    void queryClient.invalidateQueries({ queryKey: queryKeys.openOrders });
  };
}

export function useSaveModifierGroup() {
  const changed = useMenuChanged();
  return useMutation({
    mutationFn: (group: ModifierGroupInput) => ipc.call('save_modifier_group', { group }),
    onSuccess: changed,
  });
}

export function useDeleteModifierGroup() {
  const changed = useMenuChanged();
  return useMutation({
    mutationFn: (groupId: string) => ipc.call('delete_modifier_group', { group_id: groupId }),
    onSuccess: changed,
  });
}

export function useSetProductGroups() {
  const changed = useMenuChanged();
  return useMutation({
    mutationFn: (args: { productId: string; groupIds: string[] }) =>
      ipc.call('set_product_modifier_groups', {
        product_id: args.productId,
        group_ids: args.groupIds,
      }),
    onSuccess: changed,
  });
}

export function useSaveCombo() {
  const changed = useMenuChanged();
  return useMutation({
    mutationFn: (combo: ComboInput) => ipc.call('save_combo', { combo }),
    onSuccess: changed,
  });
}

export function useDeleteCombo() {
  const changed = useMenuChanged();
  return useMutation({
    mutationFn: (comboId: string) => ipc.call('delete_combo', { combo_id: comboId }),
    onSuccess: changed,
  });
}

export function useSaveTable() {
  const changed = useMenuChanged();
  return useMutation({
    mutationFn: (table: DiningTableInput) => ipc.call('save_dining_table', { table }),
    onSuccess: changed,
  });
}

export function useDeleteTable() {
  const changed = useMenuChanged();
  return useMutation({
    mutationFn: (tableId: string) => ipc.call('delete_dining_table', { table_id: tableId }),
    onSuccess: changed,
  });
}

// ── Open orders (tabs & tables) ────────────────────────────────────────────

/** Open orders of the whole shop (other tills' arrive through sync). */
export function useOpenOrders(enabled = true) {
  return useQuery({
    queryKey: queryKeys.openOrders,
    queryFn: () => ipc.call('list_open_orders'),
    enabled,
    refetchInterval: 15_000,
  });
}

/** Writes a fresh order version into the list cache. */
function useOrderStored() {
  const queryClient = useQueryClient();
  return (order: OpenOrderView | null, removedId?: string) => {
    queryClient.setQueryData<OpenOrderView[]>(queryKeys.openOrders, (list = []) => {
      const others = list.filter((o) => o.id !== (order?.id ?? removedId));
      return order ? [...others, order] : others;
    });
  };
}

export function useOpenOrder() {
  const stored = useOrderStored();
  return useMutation({
    mutationFn: (input: OpenOrderInput) => ipc.call('open_order', { input }),
    onSuccess: (order) => {
      stored(order);
    },
  });
}

export function useUpdateOrder() {
  const stored = useOrderStored();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: OpenOrderUpdate) => ipc.call('update_open_order', { input }),
    onSuccess: (order) => {
      stored(order);
    },
    onError: () => {
      // Probably changed on another till: take the current version.
      void queryClient.invalidateQueries({ queryKey: queryKeys.openOrders });
    },
  });
}

export function useSplitLine() {
  const stored = useOrderStored();
  return useMutation({
    mutationFn: (args: { order: OpenOrderView; lineId: string }) =>
      ipc.call('split_order_line', {
        order_id: args.order.id,
        line_id: args.lineId,
        expected_updated_at: args.order.updated_at,
      }),
    onSuccess: (order) => {
      stored(order);
    },
  });
}

export function useFireCourse() {
  const stored = useOrderStored();
  return useMutation({
    mutationFn: (args: { order: OpenOrderView; course: number | null }) =>
      ipc.call('fire_course', {
        order_id: args.order.id,
        course: args.course,
        expected_updated_at: args.order.updated_at,
      }),
    onSuccess: (outcome) => {
      stored(outcome.order);
    },
  });
}

export function useCancelOrder() {
  const stored = useOrderStored();
  return useMutation({
    mutationFn: (order: OpenOrderView) =>
      ipc.call('cancel_open_order', { order_id: order.id, expected_updated_at: order.updated_at }),
    onSuccess: (_done, order) => {
      stored(null, order.id);
    },
  });
}

export function usePayOrder() {
  const stored = useOrderStored();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CommandArgs<PosIpcContract, 'pay_open_order'>['input']) =>
      ipc.call('pay_open_order', { input }),
    onSuccess: (paid, input) => {
      stored(paid.order, input.order_id);
      void queryClient.invalidateQueries({ queryKey: queryKeys.shift });
      void queryClient.invalidateQueries({ queryKey: ['products'] });
    },
  });
}

// ── Stock & labels ─────────────────────────────────────────────────────────

export function useAdjustStock() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (adjustment: StockAdjustment) => ipc.call('adjust_stock', { adjustment }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['products'] });
    },
  });
}

export function usePrintLabels() {
  return useMutation({
    mutationFn: (args: { productId: string; copies: number }) =>
      ipc.call('print_product_labels', { product_id: args.productId, copies: args.copies }),
  });
}
