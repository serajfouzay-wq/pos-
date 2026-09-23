use pos_core::config::ClientConfig;

/// Validated at build time by `build.rs`; parsed again at startup.
const EMBEDDED_CLIENT_CONFIG: &str = include_str!(concat!(env!("OUT_DIR"), "/client_config.json"));

/// Process-wide state managed by Tauri. Later phases add the database pool,
/// license status, authenticated session and hardware handles here.
pub struct AppState {
    pub client: ClientConfig,
}

impl AppState {
    pub fn load() -> Self {
        let client = ClientConfig::parse(EMBEDDED_CLIENT_CONFIG)
            .expect("embedded client config was validated at build time");
        Self { client }
    }
}
