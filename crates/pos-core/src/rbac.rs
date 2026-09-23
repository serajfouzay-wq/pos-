//! Role-based access control — the enforcement point.
//!
//! Every IPC command that reads or mutates business data calls [`authorize`]
//! with the session's role *before* doing anything else. The TypeScript copy
//! of this matrix only drives what the UI shows.

use serde::{Deserialize, Serialize};

use crate::error::{IpcError, IpcResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Manager,
    Cashier,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Owner, Role::Manager, Role::Cashier];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Manager => "manager",
            Role::Cashier => "cashier",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Permission {
    // Sales floor
    #[serde(rename = "catalog.view")]
    CatalogView,
    #[serde(rename = "customer.lookup")]
    CustomerLookup,
    #[serde(rename = "sale.create")]
    SaleCreate,
    #[serde(rename = "receipt.print")]
    ReceiptPrint,
    #[serde(rename = "drawer.kick")]
    DrawerKick,
    #[serde(rename = "loyalty.redeem")]
    LoyaltyRedeem,
    // Supervisor
    #[serde(rename = "sale.refund")]
    SaleRefund,
    #[serde(rename = "sale.void")]
    SaleVoid,
    #[serde(rename = "discount.apply")]
    DiscountApply,
    #[serde(rename = "shift.open")]
    ShiftOpen,
    #[serde(rename = "shift.close")]
    ShiftClose,
    #[serde(rename = "receipt.reprint")]
    ReceiptReprint,
    #[serde(rename = "report.view")]
    ReportView,
    #[serde(rename = "report.z_run")]
    ReportZRun,
    #[serde(rename = "inventory.view")]
    InventoryView,
    // Back office
    #[serde(rename = "catalog.manage")]
    CatalogManage,
    #[serde(rename = "inventory.adjust")]
    InventoryAdjust,
    #[serde(rename = "customer.manage")]
    CustomerManage,
    #[serde(rename = "supplier.manage")]
    SupplierManage,
    #[serde(rename = "purchase_order.manage")]
    PurchaseOrderManage,
    #[serde(rename = "analytics.view")]
    AnalyticsView,
    #[serde(rename = "audit.view")]
    AuditView,
    #[serde(rename = "user.manage")]
    UserManage,
    #[serde(rename = "settings.manage")]
    SettingsManage,
}

impl Permission {
    pub const ALL: [Permission; 24] = [
        Permission::CatalogView,
        Permission::CustomerLookup,
        Permission::SaleCreate,
        Permission::ReceiptPrint,
        Permission::DrawerKick,
        Permission::LoyaltyRedeem,
        Permission::SaleRefund,
        Permission::SaleVoid,
        Permission::DiscountApply,
        Permission::ShiftOpen,
        Permission::ShiftClose,
        Permission::ReceiptReprint,
        Permission::ReportView,
        Permission::ReportZRun,
        Permission::InventoryView,
        Permission::CatalogManage,
        Permission::InventoryAdjust,
        Permission::CustomerManage,
        Permission::SupplierManage,
        Permission::PurchaseOrderManage,
        Permission::AnalyticsView,
        Permission::AuditView,
        Permission::UserManage,
        Permission::SettingsManage,
    ];
}

/// The matrix. An exhaustive `match` — adding a permission without deciding
/// who gets it is a compile error.
pub const fn is_allowed(role: Role, permission: Permission) -> bool {
    use Permission::*;
    match permission {
        CatalogView | CustomerLookup | SaleCreate | ReceiptPrint | DrawerKick | LoyaltyRedeem => {
            true
        }
        SaleRefund | SaleVoid | DiscountApply | ShiftOpen | ShiftClose | ReceiptReprint
        | ReportView | ReportZRun | InventoryView => matches!(role, Role::Owner | Role::Manager),
        CatalogManage | InventoryAdjust | CustomerManage | SupplierManage | PurchaseOrderManage
        | AnalyticsView | AuditView | UserManage | SettingsManage => matches!(role, Role::Owner),
    }
}

/// Guard used at the top of every privileged IPC command.
pub fn authorize(role: Role, permission: Permission) -> IpcResult<()> {
    if is_allowed(role, permission) {
        Ok(())
    } else {
        Err(IpcError::forbidden(format!(
            "The {} role is not allowed to perform this action.",
            role.as_str()
        )))
    }
}

pub fn permissions_for(role: Role) -> Vec<Permission> {
    Permission::ALL
        .into_iter()
        .filter(|p| is_allowed(role, *p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_holds() {
        for p in Permission::ALL {
            if is_allowed(Role::Cashier, p) {
                assert!(is_allowed(Role::Manager, p), "{p:?}");
            }
            if is_allowed(Role::Manager, p) {
                assert!(is_allowed(Role::Owner, p), "{p:?}");
            }
            assert!(is_allowed(Role::Owner, p));
        }
    }

    #[test]
    fn authorize_rejects_with_forbidden() {
        let err = authorize(Role::Cashier, Permission::SaleRefund).expect_err("must be denied");
        assert_eq!(err.code, crate::IpcErrorCode::Forbidden);
        assert!(authorize(Role::Manager, Permission::SaleRefund).is_ok());
    }
}
