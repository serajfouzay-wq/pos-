use std::sync::Arc;

use pos_core::config::ClientConfig;
use pos_core::time::SystemClock;
use pos_hwid::HardwareComponents;
use tauri::{AppHandle, Manager};

use crate::license::cloud::{CloudValidator, SupabaseValidator};
use crate::license::{LicenseEnv, LicenseService};

/// Validated at build time by `build.rs`; parsed again at startup.
const EMBEDDED_CLIENT_CONFIG: &str = include_str!(concat!(env!("OUT_DIR"), "/client_config.json"));
/// Verification-only public key, embedded at build time. There is no private
/// key anywhere in this binary.
const LICENSE_PUBLIC_KEY_PEM: &str =
    include_str!(concat!(env!("OUT_DIR"), "/license_public_key.pem"));
pub const LICENSE_KEY_ID: &str = env!("POS_LICENSE_KEY_ID");
pub const LICENSE_KEY_IS_DEV: bool = const_str_eq(env!("POS_LICENSE_KEY_IS_DEV"), "true");

const fn const_str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Process-wide state managed by Tauri. Later phases add the authenticated
/// session and hardware handles here.
pub struct AppState {
    pub client: Arc<ClientConfig>,
    pub license: Arc<LicenseService>,
}

impl AppState {
    pub fn load(app: &AppHandle) -> Result<Self, String> {
        let client = Arc::new(
            ClientConfig::parse(EMBEDDED_CLIENT_CONFIG)
                .map_err(|e| format!("embedded client config: {e}"))?,
        );
        let public_key = pos_license::keys::parse_public_key_pem(LICENSE_PUBLIC_KEY_PEM)
            .map_err(|e| format!("embedded license key: {e}"))?;
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("no app data directory: {e}"))?;
        let cloud = client.cloud.endpoint().map(|(url, key)| {
            Arc::new(SupabaseValidator::new(url, key)) as Arc<dyn CloudValidator>
        });

        let license = LicenseService::new(LicenseEnv {
            client: Arc::clone(&client),
            public_key,
            key_id: LICENSE_KEY_ID.to_owned(),
            data_dir,
            hardware: Box::new(HardwareComponents::collect),
            clock: Arc::new(SystemClock),
            cloud,
            app_version: app.package_info().version.to_string(),
            device_name: gethostname::gethostname().to_string_lossy().into_owned(),
        });
        Ok(Self {
            client,
            license: Arc::new(license),
        })
    }
}
