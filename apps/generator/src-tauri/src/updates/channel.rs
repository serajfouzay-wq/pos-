//! The online release channel (optional): a client with a cloud gets its
//! signed installers uploaded to its Supabase project's private `releases`
//! bucket and recorded with `publish_app_release`, which makes the
//! `app-update` edge function offer them to its activated tills.
//!
//! This needs the project's service-role key, kept per client in the
//! credential store on the generator's PC (never in GitHub).

use std::time::Duration;

use serde_json::json;
use ureq::tls::{RootCerts, TlsConfig};
use ureq::{Agent, Proxy};
use uuid::Uuid;

pub const MAX_NOTES: usize = 4000;

pub struct ChannelRelease<'a> {
    pub client_id: Uuid,
    pub version: &'a str,
    pub target: &'a str,
    pub notes: &'a str,
    pub file_name: &'a str,
    pub bytes: &'a [u8],
    pub signature: &'a str,
}

impl ChannelRelease<'_> {
    /// `<client_id>/<version>/<file>` in the `releases` bucket.
    pub fn path(&self) -> String {
        format!("{}/{}/{}", self.client_id, self.version, self.file_name)
    }
}

pub trait ReleaseChannel: Send + Sync {
    fn publish(
        &self,
        base_url: &str,
        service_key: &str,
        release: &ChannelRelease<'_>,
    ) -> Result<(), String>;
}

pub struct HttpChannel {
    agent: Agent,
}

impl Default for HttpChannel {
    fn default() -> Self {
        let agent: Agent = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30 * 60)))
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .proxy(Proxy::try_from_env())
            .http_status_as_error(false)
            .user_agent(concat!("pos-factory-generator/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self { agent }
    }
}

fn encode_segment(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn reply(
    what: &str,
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<(), String> {
    let mut response = result.map_err(|e| format!("{what}: the cloud is unreachable ({e})"))?;
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(());
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .unwrap_or_default();
    Err(match status {
        401 | 403 => format!("{what}: the cloud refused the service key (HTTP {status})"),
        _ => format!("{what}: HTTP {status} {}", body.trim()),
    })
}

impl ReleaseChannel for HttpChannel {
    fn publish(
        &self,
        base_url: &str,
        service_key: &str,
        release: &ChannelRelease<'_>,
    ) -> Result<(), String> {
        let base = base_url.trim_end_matches('/');
        let auth = format!("Bearer {service_key}");
        let path = release.path();
        let encoded: Vec<String> = path.split('/').map(encode_segment).collect();
        let upload = self
            .agent
            .post(format!(
                "{base}/storage/v1/object/releases/{}",
                encoded.join("/")
            ))
            .header("Authorization", &auth)
            .header("apikey", service_key)
            .header("content-type", "application/octet-stream")
            .header("x-upsert", "true")
            .send(release.bytes);
        reply("upload", upload)?;
        let notes: String = release.notes.trim().chars().take(MAX_NOTES).collect();
        let record = self
            .agent
            .post(format!("{base}/rest/v1/rpc/publish_app_release"))
            .header("Authorization", &auth)
            .header("apikey", service_key)
            .send_json(json!({
                "p_client": release.client_id,
                "p_version": release.version,
                "p_target": release.target,
                "p_notes": notes,
                "p_path": path,
                "p_signature": release.signature,
            }));
        reply("recording the release", record)
    }
}
