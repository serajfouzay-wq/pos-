use std::path::PathBuf;
use std::sync::Arc;

use crate::builds::BuildService;
use crate::signing::KeyStore;
use crate::store::Store;
use crate::updates::UpdateKey;

/// Process-wide state managed by Tauri.
pub struct AppState {
    pub store: Arc<Store>,
    pub keys: Arc<KeyStore>,
    pub builds: Arc<BuildService>,
    pub update_key: Arc<UpdateKey>,
    /// Where installers and key backups are saved.
    pub downloads: PathBuf,
}
