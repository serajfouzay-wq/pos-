//! The signed-in user. One till = one active session; switching cashier is a
//! logout + PIN login. Permissions are sent to the UI from the Rust matrix so
//! it can hide controls — enforcement still happens per command.

use std::sync::Mutex;

use pos_core::rbac::{permissions_for, Permission, Role};
use pos_core::time::Timestamp;
use serde::Serialize;
use uuid::Uuid;

/// Mirrors `SessionSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Session {
    pub user_id: Uuid,
    pub display_name: String,
    pub role: Role,
    pub permissions: Vec<Permission>,
    pub started_at: Timestamp,
}

impl Session {
    pub fn new(user_id: Uuid, display_name: String, role: Role, now: Timestamp) -> Self {
        Self {
            user_id,
            display_name,
            role,
            permissions: permissions_for(role),
            started_at: now,
        }
    }
}

#[derive(Default)]
pub struct SessionStore(Mutex<Option<Session>>);

impl SessionStore {
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn current(&self) -> Option<Session> {
        self.slot().clone()
    }

    pub fn set(&self, session: Session) {
        *self.slot() = Some(session);
    }

    pub fn clear(&self) {
        *self.slot() = None;
    }
}
