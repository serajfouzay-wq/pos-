import { describe, expect, it } from 'vitest';
import contract from '../../contracts/rbac.json';
import { hasPermission, PERMISSIONS, PinSchema, ROLE_PERMISSIONS, ROLES } from '../rbac';

describe('RBAC matrix', () => {
  it('matches contracts/rbac.json exactly', () => {
    for (const role of ROLES) {
      expect([...ROLE_PERMISSIONS[role]].sort()).toEqual([...contract.roles[role]].sort());
    }
    expect(Object.keys(contract.roles).sort()).toEqual([...ROLES].sort());
  });

  it('is strictly hierarchical: cashier ⊂ manager ⊂ owner', () => {
    for (const permission of ROLE_PERMISSIONS.cashier) {
      expect(hasPermission('manager', permission)).toBe(true);
    }
    for (const permission of ROLE_PERMISSIONS.manager) {
      expect(hasPermission('owner', permission)).toBe(true);
    }
    expect(ROLE_PERMISSIONS.owner.size).toBe(PERMISSIONS.length);
  });

  it('keeps cashiers to the sales floor', () => {
    expect(hasPermission('cashier', 'sale.create')).toBe(true);
    expect(hasPermission('cashier', 'sale.refund')).toBe(false);
    expect(hasPermission('cashier', 'discount.apply')).toBe(false);
    expect(hasPermission('cashier', 'shift.open')).toBe(false);
    expect(hasPermission('manager', 'user.manage')).toBe(false);
  });

  it('accepts 4–6 digit PINs only', () => {
    expect(PinSchema.safeParse('1234').success).toBe(true);
    expect(PinSchema.safeParse('123456').success).toBe(true);
    expect(PinSchema.safeParse('123').success).toBe(false);
    expect(PinSchema.safeParse('1234567').success).toBe(false);
    expect(PinSchema.safeParse('12a4').success).toBe(false);
  });
});
