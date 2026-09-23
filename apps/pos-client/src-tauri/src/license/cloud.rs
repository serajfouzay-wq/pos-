//! Cloud license validation (Supabase Edge Function `license-validate`).
//!
//! Validation is advisory for liveness and authoritative for revocation:
//! success moves `last_seen_at` forward; failure to *reach* the cloud is not
//! an error for the till, it just spends offline grace.

use std::time::Duration;

use pos_core::time::Timestamp;
use serde::{Deserialize, Serialize};
use ureq::tls::{RootCerts, TlsConfig};
use ureq::{Agent, Proxy};

#[derive(Debug, Clone, Serialize)]
pub struct CloudRequest<'a> {
    pub token: &'a str,
    pub fingerprint: &'a str,
    pub device_name: &'a str,
    pub app_version: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudDecision {
    Active,
    Revoked,
    DeviceLimit,
    /// The cloud does not accept this token at all (bad signature, unknown key…).
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CloudVerdict {
    pub status: CloudDecision,
    pub server_time: Timestamp,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CloudError {
    #[error("license server unreachable: {0}")]
    Unreachable(String),
}

pub trait CloudValidator: Send + Sync {
    fn validate(&self, request: &CloudRequest<'_>) -> Result<CloudVerdict, CloudError>;
}

pub struct SupabaseValidator {
    endpoint: String,
    anon_key: String,
    agent: Agent,
}

impl SupabaseValidator {
    pub fn new(supabase_url: &str, anon_key: &str) -> Self {
        let agent: Agent = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            // Windows certificate store (corporate TLS inspection) + system proxy.
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .proxy(Proxy::try_from_env())
            .user_agent(concat!("pos-client/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            endpoint: format!("{supabase_url}/functions/v1/license-validate"),
            anon_key: anon_key.to_owned(),
            agent,
        }
    }
}

impl CloudValidator for SupabaseValidator {
    fn validate(&self, request: &CloudRequest<'_>) -> Result<CloudVerdict, CloudError> {
        let mut response = self
            .agent
            .post(&self.endpoint)
            .header("apikey", &self.anon_key)
            .header("Authorization", &format!("Bearer {}", self.anon_key))
            .send_json(request)
            .map_err(|e| CloudError::Unreachable(e.to_string()))?;
        response
            .body_mut()
            .read_json::<CloudVerdict>()
            .map_err(|e| CloudError::Unreachable(format!("unexpected response: {e}")))
    }
}
