/**
 * Registry of local SQLite tables → the Zod schema describing one row.
 *
 * `contracts/db-schema.json` lists every table's columns; this registry and
 * the Rust migrations are both tested against it, so a column added on one
 * side only fails CI.
 */
import type { z } from 'zod';
import { AuditLogEntrySchema } from './entities/audit';
import {
  CategorySchema,
  DiscountRuleSchema,
  ProductSchema,
  StockMovementSchema,
} from './entities/catalog';
import { DeviceRowSchema, LicenseRowSchema } from './entities/licensing';
import { CustomerSchema, LoyaltyLedgerEntrySchema, UserRowSchema } from './entities/people';
import {
  PurchaseOrderItemSchema,
  PurchaseOrderSchema,
  SupplierSchema,
} from './entities/purchasing';
import {
  ShiftSchema,
  TransactionItemSchema,
  TransactionPaymentSchema,
  TransactionSchema,
} from './entities/sales';
import { SyncQueueRowSchema } from './sync';

export const LOCAL_TABLES = {
  license: LicenseRowSchema,
  device: DeviceRowSchema,
  users: UserRowSchema,
  categories: CategorySchema,
  products: ProductSchema,
  stock_movements: StockMovementSchema,
  discount_rules: DiscountRuleSchema,
  customers: CustomerSchema,
  loyalty_ledger: LoyaltyLedgerEntrySchema,
  shifts: ShiftSchema,
  transactions: TransactionSchema,
  transaction_items: TransactionItemSchema,
  transaction_payments: TransactionPaymentSchema,
  suppliers: SupplierSchema,
  purchase_orders: PurchaseOrderSchema,
  purchase_order_items: PurchaseOrderItemSchema,
  audit_log: AuditLogEntrySchema,
  sync_queue: SyncQueueRowSchema,
} as const satisfies Record<string, z.ZodObject>;

export type LocalTable = keyof typeof LOCAL_TABLES;

/** Immutable tables: the database rejects UPDATE on these (triggers). */
export const APPEND_ONLY_TABLES = [
  'transactions',
  'transaction_items',
  'transaction_payments',
  'stock_movements',
  'loyalty_ledger',
  'audit_log',
] as const satisfies readonly LocalTable[];
