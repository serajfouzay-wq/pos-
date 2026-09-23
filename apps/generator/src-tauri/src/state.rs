use std::sync::Arc;

use crate::builds::BuildService;
use crate::signing::KeyStore;
use crate::store::Store;

/// Process-wide state managed by Tauri.
pub struct AppState {
    pub store: Arc<Store>,
    pub keys: Arc<KeyStore>,
    pub builds: Arc<BuildService>,
}
