//! HTTP transport to the `sync-push` / `sync-pull` edge functions.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use ureq::tls::{RootCerts, TlsConfig};
use ureq::{Agent, Proxy};

use super::protocol::{PullRequest, PullResponse, PushRequest, PushResponse};
use crate::license::SyncCredentials;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
    /// Network down, server unavailable (5xx), timeouts: try again later.
    #[error("cloud unreachable: {0}")]
    Offline(String),
    /// 401/403: the cloud refuses this device (not activated yet, revoked…).
    #[error("cloud refused this till: {0}")]
    Unauthorized(String),
    /// 400 or an unparseable response: a bug on one side; retrying won't help.
    #[error("sync protocol error: {0}")]
    Protocol(String),
    #[error("{0}")]
    Local(String),
}

pub trait SyncTransport: Send + Sync {
    fn push(
        &self,
        credentials: &SyncCredentials,
        request: &PushRequest,
    ) -> Result<PushResponse, SyncError>;
    fn pull(
        &self,
        credentials: &SyncCredentials,
        request: &PullRequest,
    ) -> Result<PullResponse, SyncError>;
}

pub struct HttpTransport {
    base: String,
    anon_key: String,
    agent: Agent,
}

impl HttpTransport {
    pub fn new(supabase_url: &str, anon_key: &str) -> Self {
        let agent: Agent = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .proxy(Proxy::try_from_env())
            .http_status_as_error(false)
            .user_agent(concat!("pos-client/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            base: format!("{supabase_url}/functions/v1"),
            anon_key: anon_key.to_owned(),
            agent,
        }
    }

    fn post<Req: Serialize, Res: DeserializeOwned>(
        &self,
        function: &str,
        credentials: &SyncCredentials,
        request: &Req,
    ) -> Result<Res, SyncError> {
        let mut response = self
            .agent
            .post(&format!("{}/{function}", self.base))
            .header("apikey", &self.anon_key)
            .header("Authorization", &format!("Bearer {}", self.anon_key))
            .header("x-pos-license", &credentials.token)
            .header("x-pos-device-key", credentials.device_key.as_str())
            .send_json(request)
            .map_err(|e| SyncError::Offline(e.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| SyncError::Offline(e.to_string()))?;
        match status {
            200 => {
                serde_json::from_str(&body).map_err(|e| SyncError::Protocol(format!("{e}: {body}")))
            }
            401 | 403 => Err(SyncError::Unauthorized(body)),
            400..=499 => Err(SyncError::Protocol(format!("HTTP {status}: {body}"))),
            _ => Err(SyncError::Offline(format!("HTTP {status}"))),
        }
    }
}

impl SyncTransport for HttpTransport {
    fn push(
        &self,
        credentials: &SyncCredentials,
        request: &PushRequest,
    ) -> Result<PushResponse, SyncError> {
        self.post("sync-push", credentials, request)
    }

    fn pull(
        &self,
        credentials: &SyncCredentials,
        request: &PullRequest,
    ) -> Result<PullResponse, SyncError> {
        self.post("sync-pull", credentials, request)
    }
}
