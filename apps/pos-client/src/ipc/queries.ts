import type {
  AuditFilter,
  CustomerInput,
  CustomerSearch,
  KitchenChange,
  LoyaltySettings,
  PointsAdjustment,
  UpdateStatus,
  CommandArgs,
  DashboardRequest,
  RefundInput,
  ShiftFilter,
  TransactionFilter,
  Uuid,
  VoidInput,
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
  Locale,
  PrintMode,
  DiscountRuleInput,
  GrantMembership,
  MemberFilter,
  MembershipPlanInput,
  BackupSettings,
  RestoreRequest,
  LanSettings,
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
  transactions: (filter: TransactionFilter) => ['transactions', filter] as const,
  transaction: (id: string) => ['transaction', id] as const,
  xReport: ['x_report'] as const,
  zReports: ['z_reports'] as const,
  zReport: (id: string) => ['z_report', id] as const,
  shifts: (filter: ShiftFilter) => ['shifts', filter] as const,
  dashboard: (request: DashboardRequest) => ['dashboard', request] as const,
  audit: (filter: AuditFilter) => ['audit', filter] as const,
  customers: (search: CustomerSearch) => ['customers', search] as const,
  customer: (id: string) => ['customer', id] as const,
  loyalty: ['loyalty_settings'] as const,
  kitchenStatus: ['kitchen_display'] as const,
  kitchenBoard: ['kitchen_board'] as const,
  updates: ['update_status'] as const,
  discountRules: ['discount_rules'] as const,
  membershipPlans: ['membership_plans'] as const,
  members: (filter: MemberFilter) => ['members', filter] as const,
  customerMemberships: (id: string) => ['customer_memberships', id] as const,
  backups: ['backup_status'] as const,
  lan: ['lan_status'] as const,
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

/** Saves the activation code to `path` (picked in the save dialog). */
export function useSaveActivationFile() {
  return useMutation({
    mutationFn: (path: string) => ipc.call('save_activation_file', { path }),
  });
}

/** License files the generator saved, found on USB sticks and in Downloads. */
export function useFoundLicenseFiles() {
  return useQuery({
    queryKey: ['license_files'],
    queryFn: () => ipc.call('find_license_files'),
    enabled: inTauri,
    staleTime: 0,
    // A stick plugged in while the screen is open shows up by itself.
    refetchInterval: 5_000,
  });
}

export function useActivateLicenseFile() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (path: string) => ipc.call('activate_license_file', { path }),
    onSuccess: (status) => {
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
    mutationFn: (args: {
      target: PrinterTarget;
      language?: Locale | null;
      mode?: PrintMode | null;
      paper_width_mm?: 58 | 80 | null;
    }) => ipc.call('test_printer', args),
  });
}

// ── Cloud sync ─────────────────────────────────────────────────────────────

/** Local data another till may have changed; refetched after a sync round. */
const SYNCED_QUERIES = [
  ['products'],
  ['discount_rules'],
  ['membership_plans'],
  ['members'],
  ['customer_memberships'],
  ['transactions'],
  ['z_reports'],
  ['dashboard'],
  ['menu'],
  queryKeys.openOrders,
  ['customers'],
  ['customer'],
  queryKeys.loyalty,
  queryKeys.kitchenBoard,
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

// ── History, refunds, voids (Phase 7) ──────────────────────────────────────

export function useTransactions(filter: TransactionFilter, enabled = true) {
  return useQuery({
    queryKey: queryKeys.transactions(filter),
    queryFn: () => ipc.call('list_transactions', { filter }),
    placeholderData: keepPreviousData,
    enabled,
  });
}

export function useTransaction(id: Uuid | null) {
  return useQuery({
    queryKey: queryKeys.transaction(id ?? ''),
    queryFn: () => {
      if (id === null) throw new Error('no transaction');
      return ipc.call('get_transaction', { transaction_id: id });
    },
    enabled: id !== null,
  });
}

/** Rust prices the refund from the stored sale; the dialog shows this. */
export function useRefundQuote(input: RefundInput | null) {
  return useQuery({
    queryKey: ['refund_quote', input],
    queryFn: () => {
      if (input === null) throw new Error('nothing to quote');
      return ipc.call('quote_refund', { input });
    },
    enabled: input !== null && input.lines.length > 0,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

function useReversed() {
  const queryClient = useQueryClient();
  return () => {
    for (const key of ['transactions', 'transaction', 'x_report', 'dashboard', 'products']) {
      void queryClient.invalidateQueries({ queryKey: [key] });
    }
    void queryClient.invalidateQueries({ queryKey: queryKeys.shift });
  };
}

export function useRefund() {
  const reversed = useReversed();
  return useMutation({
    mutationFn: (input: RefundInput) => ipc.call('refund_transaction', { input }),
    onSuccess: reversed,
  });
}

export function useVoid() {
  const reversed = useReversed();
  return useMutation({
    mutationFn: (input: VoidInput) => ipc.call('void_transaction', { input }),
    onSuccess: reversed,
  });
}

// ── Reports ────────────────────────────────────────────────────────────────

export function useXReport(enabled = true) {
  return useQuery({
    queryKey: queryKeys.xReport,
    queryFn: () => ipc.call('get_x_report'),
    enabled,
    refetchInterval: 30_000,
  });
}

export function useRunZ() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.call('run_z_report'),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.xReport });
      void queryClient.invalidateQueries({ queryKey: queryKeys.zReports });
      void queryClient.invalidateQueries({ queryKey: ['shifts'] });
    },
  });
}

export function useZReports(deviceId: Uuid | null = null) {
  return useQuery({
    queryKey: [...queryKeys.zReports, deviceId],
    queryFn: () => ipc.call('list_z_reports', { device_id: deviceId, limit: 200, offset: 0 }),
  });
}

export function useZReport(id: Uuid | null) {
  return useQuery({
    queryKey: queryKeys.zReport(id ?? ''),
    queryFn: () => {
      if (id === null) throw new Error('no report');
      return ipc.call('get_z_report', { z_report_id: id });
    },
    enabled: id !== null,
    staleTime: Number.POSITIVE_INFINITY,
  });
}

export function usePrintReport() {
  return useMutation({
    mutationFn: (zReportId: Uuid | null) => ipc.call('print_report', { z_report_id: zReportId }),
  });
}

export function useShifts(filter: ShiftFilter) {
  return useQuery({
    queryKey: queryKeys.shifts(filter),
    queryFn: () => ipc.call('list_shifts', { filter }),
    placeholderData: keepPreviousData,
  });
}

export function useDashboard(request: DashboardRequest) {
  return useQuery({
    queryKey: queryKeys.dashboard(request),
    queryFn: () => ipc.call('get_dashboard_metrics', { request }),
    placeholderData: keepPreviousData,
    refetchInterval: 60_000,
  });
}

export function useAuditLog(filter: AuditFilter) {
  return useQuery({
    queryKey: queryKeys.audit(filter),
    queryFn: () => ipc.call('list_audit_log', { filter }),
    placeholderData: keepPreviousData,
  });
}

// ── Customers & loyalty (Phase 8) ──────────────────────────────────────────

export function useCustomers(search: CustomerSearch, enabled = true) {
  return useQuery({
    queryKey: queryKeys.customers(search),
    queryFn: () => ipc.call('search_customers', { search }),
    placeholderData: keepPreviousData,
    enabled,
  });
}

export function useCustomer(id: Uuid | null) {
  return useQuery({
    queryKey: queryKeys.customer(id ?? ''),
    queryFn: () => {
      if (id === null) throw new Error('no customer');
      return ipc.call('get_customer', { customer_id: id });
    },
    enabled: id !== null,
  });
}

function useCustomersChanged() {
  const queryClient = useQueryClient();
  return () => {
    void queryClient.invalidateQueries({ queryKey: ['customers'] });
    void queryClient.invalidateQueries({ queryKey: ['customer'] });
    void queryClient.invalidateQueries({ queryKey: ['quote'] });
  };
}

export function useSaveCustomer() {
  const changed = useCustomersChanged();
  return useMutation({
    mutationFn: (customer: CustomerInput) => ipc.call('save_customer', { customer }),
    onSuccess: changed,
  });
}

export function useDeleteCustomer() {
  const changed = useCustomersChanged();
  return useMutation({
    mutationFn: (customerId: Uuid) => ipc.call('delete_customer', { customer_id: customerId }),
    onSuccess: changed,
  });
}

export function useAdjustPoints() {
  const changed = useCustomersChanged();
  return useMutation({
    mutationFn: (adjustment: PointsAdjustment) => ipc.call('adjust_loyalty_points', { adjustment }),
    onSuccess: changed,
  });
}

export function useLoyaltyProgram() {
  return useQuery({
    queryKey: queryKeys.loyalty,
    queryFn: () => ipc.call('get_loyalty_settings'),
    staleTime: 60_000,
  });
}

export function useSaveLoyaltyProgram() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: LoyaltySettings) => ipc.call('save_loyalty_settings', { settings }),
    onSuccess: (program) => {
      queryClient.setQueryData(queryKeys.loyalty, program);
      void queryClient.invalidateQueries({ queryKey: ['quote'] });
    },
  });
}

// ── Kitchen display (Phase 8) ──────────────────────────────────────────────

export function useKitchenDisplayStatus(enabled = true) {
  return useQuery({
    queryKey: queryKeys.kitchenStatus,
    queryFn: () => ipc.call('kitchen_display_status'),
    enabled,
  });
}

export function useSetKitchenDisplay() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (enabled: boolean) => ipc.call('set_kitchen_display', { enabled }),
    onSuccess: (status) => {
      queryClient.setQueryData(queryKeys.kitchenStatus, status);
    },
  });
}

/**
 * The ticket board. Refetched on every local change (`kitchen://changed`),
 * after sync rounds, and every few seconds so timers and other tills'
 * tickets stay current.
 */
export function useKitchenBoard(recentMinutes = 30) {
  const queryClient = useQueryClient();
  useEffect(
    () =>
      subscribe('kitchen_changed', () => {
        void queryClient.invalidateQueries({ queryKey: queryKeys.kitchenBoard });
      }),
    [queryClient],
  );
  return useQuery({
    queryKey: [...queryKeys.kitchenBoard, recentMinutes],
    queryFn: () => ipc.call('list_kitchen_tickets', { recent_minutes: recentMinutes }),
    refetchInterval: 5_000,
    placeholderData: keepPreviousData,
  });
}

export function useBumpTicket() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { ticketId: Uuid; ready: boolean }) =>
      ipc.call('bump_kitchen_ticket', { ticket_id: args.ticketId, ready: args.ready }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.kitchenBoard });
    },
  });
}

export function useStrikeItem() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { ticketId: Uuid; lineId: Uuid; done: boolean }) =>
      ipc.call('set_kitchen_item_done', {
        ticket_id: args.ticketId,
        line_id: args.lineId,
        done: args.done,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.kitchenBoard });
    },
  });
}

/** Calls `handler` when the kitchen marks a ticket ready (this till's window). */
export function useKitchenReady(handler: (change: KitchenChange) => void) {
  useEffect(
    () =>
      subscribe('kitchen_changed', (change) => {
        if (change.status === 'ready' && change.kind === 'order') handler(change);
      }),
    [handler],
  );
}

// ── Updates (Phase 8) ──────────────────────────────────────────────────────

export function useUpdateStatus() {
  const queryClient = useQueryClient();
  useEffect(
    () =>
      subscribe('update_status', (status) => {
        queryClient.setQueryData<UpdateStatus>(queryKeys.updates, status);
      }),
    [queryClient],
  );
  return useQuery({
    queryKey: queryKeys.updates,
    queryFn: () => ipc.call('update_status'),
    staleTime: Number.POSITIVE_INFINITY,
  });
}

export function useCheckForUpdates() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.call('check_for_updates'),
    onSuccess: (status) => {
      queryClient.setQueryData(queryKeys.updates, status);
    },
  });
}

export function useInstallUpdate() {
  return useMutation({ mutationFn: () => ipc.call('install_update') });
}

/** Update files on USB sticks and in Downloads (asked when the screen opens). */
export function useFoundUpdateFiles(enabled: boolean) {
  return useQuery({
    queryKey: ['update_files'],
    queryFn: () => ipc.call('find_update_files'),
    enabled: inTauri && enabled,
    staleTime: 0,
  });
}

export function useInspectUpdateFile() {
  return useMutation({
    mutationFn: (path: string) => ipc.call('inspect_update_file', { path }),
  });
}

export function useInstallUpdateFile() {
  return useMutation({
    mutationFn: (path: string) => ipc.call('install_update_file', { path }),
  });
}

export function useDismissUpdateNotice() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.call('dismiss_update_notice'),
    onSuccess: (status) => {
      queryClient.setQueryData(queryKeys.updates, status);
    },
  });
}

// ── Discount rules (Phase 9) ───────────────────────────────────────────────

/** Every rule; `live` flags change with the clock, so this refreshes. */
export function useDiscountRules(enabled = true) {
  return useQuery({
    queryKey: queryKeys.discountRules,
    queryFn: () => ipc.call('list_discount_rules'),
    enabled,
    staleTime: 30_000,
    refetchInterval: 60_000,
  });
}

export function useSaveDiscountRule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (rule: DiscountRuleInput) => ipc.call('save_discount_rule', { rule }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.discountRules });
      void queryClient.invalidateQueries({ queryKey: ['quote'] });
    },
  });
}

export function useDeleteDiscountRule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (ruleId: Uuid) => ipc.call('delete_discount_rule', { rule_id: ruleId }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.discountRules });
      void queryClient.invalidateQueries({ queryKey: ['quote'] });
    },
  });
}

// ── Memberships (Phase 9) ──────────────────────────────────────────────────

export function useMembershipPlans(enabled = true) {
  return useQuery({
    queryKey: queryKeys.membershipPlans,
    queryFn: () => ipc.call('list_membership_plans'),
    enabled,
    staleTime: 30_000,
  });
}

export function useMembers(filter: MemberFilter, enabled = true) {
  return useQuery({
    queryKey: queryKeys.members(filter),
    queryFn: () => ipc.call('list_members', { filter }),
    enabled,
    placeholderData: keepPreviousData,
  });
}

export function useCustomerMemberships(customerId: Uuid | null) {
  return useQuery({
    queryKey: queryKeys.customerMemberships(customerId ?? ''),
    queryFn: () => {
      if (customerId === null) throw new Error('no customer');
      return ipc.call('customer_memberships', { customer_id: customerId });
    },
    enabled: customerId !== null,
  });
}

function useMembershipMutation<A, R>(fn: (args: A) => Promise<R>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: () => {
      for (const key of [
        queryKeys.membershipPlans,
        ['members'],
        ['customer_memberships'],
        ['products'],
        ['categories'],
        ['quote'],
      ]) {
        void queryClient.invalidateQueries({ queryKey: key });
      }
    },
  });
}

export function useSaveMembershipPlan() {
  return useMembershipMutation((plan: MembershipPlanInput) =>
    ipc.call('save_membership_plan', { plan }),
  );
}

export function useDeleteMembershipPlan() {
  return useMembershipMutation((planId: Uuid) =>
    ipc.call('delete_membership_plan', { plan_id: planId }),
  );
}

export function useGrantMembership() {
  return useMembershipMutation((grant: GrantMembership) => ipc.call('grant_membership', { grant }));
}

export function useCancelMembership() {
  return useMembershipMutation((membershipId: Uuid) =>
    ipc.call('cancel_membership', { membership_id: membershipId }),
  );
}

// ── Backups (Phase 9) ──────────────────────────────────────────────────────

export function useBackupStatus(enabled = true) {
  return useQuery({
    queryKey: queryKeys.backups,
    queryFn: () => ipc.call('backup_status'),
    enabled,
    refetchInterval: 60_000,
  });
}

function useBackupMutation<A, R>(fn: (args: A) => Promise<R>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.backups });
    },
  });
}

export function useBackupNow() {
  return useBackupMutation(() => ipc.call('backup_now'));
}

export function useSaveBackupSettings() {
  return useBackupMutation((settings: BackupSettings) =>
    ipc.call('save_backup_settings', { settings }),
  );
}

export function useSetBackupPassword() {
  return useBackupMutation((password: string) => ipc.call('set_backup_password', { password }));
}

export function useBackupsIn() {
  return useMutation({
    mutationFn: (dir: string) => ipc.call('list_backups_in', { dir }),
  });
}

export function useRestoreBackup() {
  return useBackupMutation((request: RestoreRequest) => ipc.call('restore_backup', { request }));
}

export function useRestartApp() {
  return useMutation({ mutationFn: () => ipc.call('restart_app') });
}

// ── Shop network (Phase 9) ─────────────────────────────────────────────────

export function useLanStatus() {
  return useQuery({
    queryKey: queryKeys.lan,
    queryFn: () => ipc.call('lan_status'),
    refetchInterval: 10_000,
  });
}

export function useSaveLanSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: LanSettings) => ipc.call('save_lan_settings', { settings }),
    onSuccess: (status) => {
      queryClient.setQueryData(queryKeys.lan, status);
      void queryClient.invalidateQueries({ queryKey: queryKeys.syncStatus });
    },
  });
}

export function useDiscoverHubs() {
  return useMutation({ mutationFn: (port: number) => ipc.call('discover_hubs', { port }) });
}

export function useTestHub() {
  return useMutation({
    mutationFn: (args: { address: string; code: string; port: number }) =>
      ipc.call('test_hub', args),
  });
}

export function useNewHubCode() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.call('new_hub_code'),
    onSuccess: (status) => {
      queryClient.setQueryData(queryKeys.lan, status);
    },
  });
}
