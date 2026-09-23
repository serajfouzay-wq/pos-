/**
 * Role-based access control matrix.
 *
 * ENFORCEMENT LIVES IN RUST. Every Tauri IPC command calls
 * `pos_core::rbac::authorize` before touching data or hardware. This TypeScript
 * copy exists only so the UI can hide controls a role cannot use — hiding a
 * button is never a security boundary.
 *
 * Both copies are pinned to `contracts/rbac.json`; a drift fails CI.
 */
import { z } from 'zod';

export const ROLES = ['owner', 'manager', 'cashier'] as const;
export const RoleSchema = z.enum(ROLES);
export type Role = z.infer<typeof RoleSchema>;

export const PERMISSIONS = [
  // Sales floor
  'catalog.view',
  'customer.lookup',
  'sale.create',
  'receipt.print',
  'drawer.kick',
  'loyalty.redeem',
  // Supervisor
  'sale.refund',
  'sale.void',
  'discount.apply',
  'shift.open',
  'shift.close',
  'receipt.reprint',
  'report.view',
  'report.z_run',
  'inventory.view',
  // Back office
  'catalog.manage',
  'inventory.adjust',
  'customer.manage',
  'supplier.manage',
  'purchase_order.manage',
  'analytics.view',
  'audit.view',
  'user.manage',
  'settings.manage',
] as const;
export const PermissionSchema = z.enum(PERMISSIONS);
export type Permission = z.infer<typeof PermissionSchema>;

const CASHIER: readonly Permission[] = [
  'catalog.view',
  'customer.lookup',
  'sale.create',
  'receipt.print',
  'drawer.kick',
  'loyalty.redeem',
];

const MANAGER: readonly Permission[] = [
  ...CASHIER,
  'sale.refund',
  'sale.void',
  'discount.apply',
  'shift.open',
  'shift.close',
  'receipt.reprint',
  'report.view',
  'report.z_run',
  'inventory.view',
];

export const ROLE_PERMISSIONS: Readonly<Record<Role, ReadonlySet<Permission>>> = {
  owner: new Set(PERMISSIONS),
  manager: new Set(MANAGER),
  cashier: new Set(CASHIER),
};

export function hasPermission(role: Role, permission: Permission): boolean {
  return ROLE_PERMISSIONS[role].has(permission);
}

/** Login PIN: 4–6 digits, entered on the on-screen numpad. */
export const PinSchema = z.string().regex(/^\d{4,6}$/, 'PIN must be 4–6 digits');
