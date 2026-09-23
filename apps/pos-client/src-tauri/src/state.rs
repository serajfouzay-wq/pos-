use std::sync::Arc;

use pos_core::config::ClientConfig;
use pos_core::time::SystemClock;
use pos_hwid::HardwareComponents;
use tauri::{AppHandle, Manager};

use crate::license::cloud::{CloudValidator, SupabaseValidator};
use crate::license::{LicenseEnv, LicenseService};
use crate::printing::{template_for, PrintService, SystemPrinters};
use crate::session::SessionStore;
use crate::sync::{HttpTransport, SyncEngine, SyncTransport};

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

/// Process-wide state managed by Tauri.
pub struct AppState {
    pub client: Arc<ClientConfig>,
    pub license: Arc<LicenseService>,
    pub session: SessionStore,
    pub printer: Arc<PrintService>,
    pub sync: Arc<SyncEngine>,
}

/// Receipt logo from the client's bundled assets (`<resources>/client-assets/`).
fn load_logo(app: &AppHandle, client: &ClientConfig) -> Option<pos_hardware::image::MonoImage> {
    let file = client.receipt.logo_asset.as_ref()?;
    let path = app
        .path()
        .resource_dir()
        .ok()?
        .join("client-assets")
        .join(file);
    let bytes = std::fs::read(path).ok()?;
    let dots = pos_hardware::image::dots_for_paper(client.receipt.paper_width_mm);
    pos_hardware::image::logo_from_png(&bytes, dots).ok()
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
        let transport = client
            .cloud
            .endpoint()
            .map(|(url, key)| Arc::new(HttpTransport::new(url, key)) as Arc<dyn SyncTransport>);

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
        let printer = PrintService::new(
            Arc::new(SystemPrinters),
            template_for(&client, load_logo(app, &client)),
        );
        Ok(Self {
            client,
            license: Arc::new(license),
            session: SessionStore::default(),
            printer: Arc::new(printer),
            sync: Arc::new(SyncEngine::new(transport, Arc::new(SystemClock))),
        })
    }
}
