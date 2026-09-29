//! The shop network: tills sync with one of them (the hub) over the local
//! network, with no internet at all.
//!
//! - The hub runs a small HTTP server (`/pos-hub/v1/push|pull|hello`) that
//!   answers the cloud's protocol from [`super::hub`], and answers discovery
//!   broadcasts (UDP) so the other tills find it by themselves.
//! - Every request carries the client id and the hub's pairing code (shown
//!   on the hub, typed once on each till); anything else is refused.
//! - The other tills use [`LanTransport`] in place of the cloud transport;
//!   the hub itself uses [`LocalHubTransport`], so its own changes go
//!   through the same rules as everyone's.
//!
//! Windows asks once whether the app may accept connections on the network
//! (firewall): the owner answers "Allow" on the hub.

use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::hub;
use super::protocol::{PullRequest, PullResponse, PushRequest, PushResponse};
use super::transport::{SyncError, SyncTransport};
use crate::db::Database;
use crate::license::{LicenseService, SyncCredentials};

pub const SETTINGS_KEY: &str = "lan";
/// The hub's own pairing code (on the hub only).
pub const HUB_KEY_SETTING: &str = "lan.hub_key";
pub const DEFAULT_PORT: u16 = 47800;
const DISCOVERY_ASK: &str = "POS-HUB?";
const DISCOVERY_ANSWER: &str = "POS-HUB!";
const MAX_BODY: u64 = 32 * 1024 * 1024;

/// This till's part in the shop network. Mirrors `LanRoleSchema`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanRole {
    #[default]
    Off,
    Hub,
    Client,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

/// Mirrors `LanSettingsSchema` (device-local).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanSettings {
    #[serde(default)]
    pub role: LanRole,
    #[serde(default = "default_port")]
    pub port: u16,
    /// The hub's `host:port` (tills joining a hub).
    #[serde(default)]
    pub hub_address: Option<String>,
    /// The hub's pairing code (tills joining a hub).
    #[serde(default)]
    pub hub_code: Option<String>,
}

impl Default for LanSettings {
    fn default() -> Self {
        Self {
            role: LanRole::Off,
            port: DEFAULT_PORT,
            hub_address: None,
            hub_code: None,
        }
    }
}

/// `ABCD-EFGH`: 8 characters without look-alikes (no 0/O, 1/I).
pub fn new_pairing_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    let chars: String = (0..8)
        .map(|_| char::from(ALPHABET[rng.gen_range(0..ALPHABET.len())]))
        .collect();
    format!("{}-{}", &chars[..4], &chars[4..])
}

/// Codes are compared without case, spaces or dashes.
pub fn normalize_code(code: &str) -> String {
    code.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Constant-time comparison, so the code cannot be guessed by timing.
fn same_code(a: &str, b: &str) -> bool {
    let (a, b) = (normalize_code(a), normalize_code(b));
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// `192.168.1.10` → `192.168.1.10:47800`; `host:port` kept.
pub fn normalize_address(address: &str, port: u16) -> IpcResult<String> {
    let address = address.trim();
    if address.is_empty() || address.len() > 100 || address.contains('/') {
        return Err(IpcError::validation(
            "Enter the hub's address, e.g. 192.168.1.10.",
        ));
    }
    Ok(if address.contains(':') {
        address.to_owned()
    } else {
        format!("{address}:{port}")
    })
}

// ── Tills joining a hub ─────────────────────────────────────────────────────

pub struct LanTransport {
    base: String,
    client_id: Uuid,
    code: String,
    agent: ureq::Agent,
}

impl LanTransport {
    pub fn new(address: &str, client_id: Uuid, code: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            // On a shop network a hub that is there answers at once.
            .timeout_connect(Some(Duration::from_secs(3)))
            .http_status_as_error(false)
            .user_agent(concat!("pos-client/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            base: format!("http://{address}/pos-hub/v1"),
            client_id,
            code: code.to_owned(),
            agent,
        }
    }

    fn post<Req: Serialize, Res: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        request: &Req,
    ) -> Result<Res, SyncError> {
        let mut response = self
            .agent
            .post(&format!("{}/{path}", self.base))
            .header("x-pos-client", &self.client_id.to_string())
            .header("x-pos-hub-code", &self.code)
            .send_json(request)
            .map_err(|e| SyncError::Offline(format!("hub unreachable: {e}")))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_string()
            .map_err(|e| SyncError::Offline(e.to_string()))?;
        match status {
            200 => {
                serde_json::from_str(&body).map_err(|e| SyncError::Protocol(format!("{e}: {body}")))
            }
            401 | 403 => Err(SyncError::Unauthorized(body)),
            400..=499 => Err(SyncError::Protocol(format!("HTTP {status}: {body}"))),
            _ => Err(SyncError::Offline(format!("hub HTTP {status}: {body}"))),
        }
    }

    /// Asks the hub who it is (the "Test connection" button).
    pub fn hello(&self) -> Result<HubHello, SyncError> {
        self.post("hello", &serde_json::json!({}))
    }
}

impl SyncTransport for LanTransport {
    fn push(&self, _: &SyncCredentials, request: &PushRequest) -> Result<PushResponse, SyncError> {
        self.post("push", request)
    }

    fn pull(&self, _: &SyncCredentials, request: &PullRequest) -> Result<PullResponse, SyncError> {
        self.post("pull", request)
    }
}

// ── The hub ─────────────────────────────────────────────────────────────────

/// The hub's database (the license gate's, in the app).
pub type HubDb = Arc<dyn Fn() -> Result<Arc<Database>, SyncError> + Send + Sync>;

/// The license gate's database, as the hub reaches it.
pub fn license_db(license: Arc<LicenseService>) -> HubDb {
    Arc::new(move || {
        license
            .database()
            .map_err(|e| SyncError::Unauthorized(e.message))
    })
}

fn hub_push(db: &HubDb, request: &PushRequest) -> Result<PushResponse, SyncError> {
    let db = db()?;
    let mut conn = db.conn();
    let tx = conn
        .transaction()
        .map_err(|e| SyncError::Local(e.to_string()))?;
    let response =
        hub::push(&tx, request, SystemClock.now()).map_err(|e| SyncError::Local(e.to_string()))?;
    tx.commit().map_err(|e| SyncError::Local(e.to_string()))?;
    Ok(response)
}

fn hub_pull(db: &HubDb, request: &PullRequest) -> Result<PullResponse, SyncError> {
    let db = db()?;
    let conn = db.conn();
    hub::pull(&conn, request).map_err(|e| SyncError::Local(e.to_string()))
}

/// The hub's own sync: straight into its hub tables, no network.
pub struct LocalHubTransport {
    pub db: HubDb,
}

impl SyncTransport for LocalHubTransport {
    fn push(&self, _: &SyncCredentials, request: &PushRequest) -> Result<PushResponse, SyncError> {
        hub_push(&self.db, request)
    }

    fn pull(&self, _: &SyncCredentials, request: &PullRequest) -> Result<PullResponse, SyncError> {
        hub_pull(&self.db, request)
    }
}

/// Mirrors `HubHelloSchema`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubHello {
    pub client_id: Uuid,
    pub hub_name: String,
    pub rows: i64,
    pub tills: i64,
}

/// A running hub server. Dropping it stops the server (within 250 ms).
pub struct HubServer {
    stopped: Arc<AtomicBool>,
    pub port: u16,
}

impl Drop for HubServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

fn respond(request: tiny_http::Request, status: u16, body: &str) {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static header");
    let response = tiny_http::Response::from_string(body)
        .with_status_code(status)
        .with_header(header);
    let _ = request.respond(response);
}

fn header<'a>(request: &'a tiny_http::Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}

fn handle(
    mut request: tiny_http::Request,
    db: &HubDb,
    client_id: Uuid,
    code: &str,
    hub_name: &str,
) {
    let authorized = header(&request, "x-pos-client") == Some(client_id.to_string().as_str())
        && header(&request, "x-pos-hub-code").is_some_and(|c| same_code(c, code));
    if !authorized {
        return respond(request, 403, r#"{"error":"wrong shop or pairing code"}"#);
    }
    if *request.method() != tiny_http::Method::Post {
        return respond(request, 405, r#"{"error":"POST only"}"#);
    }
    let path = request.url().to_owned();
    let mut body = String::new();
    if request
        .as_reader()
        .take(MAX_BODY)
        .read_to_string(&mut body)
        .is_err()
    {
        return respond(request, 400, r#"{"error":"unreadable body"}"#);
    }
    let result: Result<String, (u16, String)> = match path.as_str() {
        "/pos-hub/v1/push" => serde_json::from_str::<PushRequest>(&body)
            .map_err(|e| (400, e.to_string()))
            .and_then(|r| hub_push(db, &r).map_err(|e| (500, e.to_string())))
            .and_then(|r| serde_json::to_string(&r).map_err(|e| (500, e.to_string()))),
        "/pos-hub/v1/pull" => serde_json::from_str::<PullRequest>(&body)
            .map_err(|e| (400, e.to_string()))
            .and_then(|r| hub_pull(db, &r).map_err(|e| (500, e.to_string())))
            .and_then(|r| serde_json::to_string(&r).map_err(|e| (500, e.to_string()))),
        "/pos-hub/v1/hello" => db()
            .map_err(|e| (503, e.to_string()))
            .and_then(|db| hub::stats(&db.conn()).map_err(|e| (500, e.to_string())))
            .and_then(|(rows, tills)| {
                serde_json::to_string(&HubHello {
                    client_id,
                    hub_name: hub_name.to_owned(),
                    rows,
                    tills,
                })
                .map_err(|e| (500, e.to_string()))
            }),
        _ => Err((404, "unknown path".into())),
    };
    match result {
        Ok(json) => respond(request, 200, &json),
        Err((status, message)) => respond(
            request,
            status,
            &serde_json::json!({ "error": message }).to_string(),
        ),
    }
}

impl HubServer {
    /// Starts serving on `port` (all interfaces; 0 = any free port) and
    /// answering discovery.
    pub fn start(
        db: HubDb,
        client_id: Uuid,
        code: String,
        hub_name: String,
        port: u16,
    ) -> IpcResult<Self> {
        let server = Arc::new(tiny_http::Server::http(("0.0.0.0", port)).map_err(|e| {
            IpcError::validation(format!(
                "Port {port} is not free on this PC ({e}). Choose another port."
            ))
        })?);
        let port = server
            .server_addr()
            .to_ip()
            .map_or(port, |addr| addr.port());
        let stopped = Arc::new(AtomicBool::new(false));
        for _ in 0..4 {
            let (server, db, code, name, stop) = (
                Arc::clone(&server),
                Arc::clone(&db),
                code.clone(),
                hub_name.clone(),
                Arc::clone(&stopped),
            );
            // Polls so every worker notices a stop (and the socket closes).
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match server.recv_timeout(Duration::from_millis(250)) {
                        Ok(Some(request)) => handle(request, &db, client_id, &code, &name),
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
            });
        }
        // Bound before returning, so a till asking right away is answered.
        if let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port.saturating_add(1))) {
            let (flag, name) = (Arc::clone(&stopped), hub_name);
            std::thread::spawn(move || answer_discovery(&socket, client_id, port, &name, &flag));
        }
        Ok(Self { stopped, port })
    }
}

/// Answers `POS-HUB?<client id>` broadcasts on `port + 1` (UDP).
fn answer_discovery(
    socket: &UdpSocket,
    client_id: Uuid,
    port: u16,
    name: &str,
    stopped: &AtomicBool,
) {
    let _ = socket.set_read_timeout(Some(Duration::from_millis(500)));
    let ask = format!("{DISCOVERY_ASK}{client_id}");
    let answer = format!(
        "{DISCOVERY_ANSWER}{}",
        serde_json::json!({ "client_id": client_id, "port": port, "name": name })
    );
    let mut buf = [0u8; 256];
    while !stopped.load(Ordering::SeqCst) {
        if let Ok((n, from)) = socket.recv_from(&mut buf) {
            if buf[..n] == *ask.as_bytes() {
                let _ = socket.send_to(answer.as_bytes(), from);
            }
        }
    }
}

/// Mirrors `FoundHubSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoundHub {
    pub address: String,
    pub name: String,
}

/// Broadcasts on the shop network for a hub of this shop (1.5 s).
pub fn discover(client_id: Uuid, port: u16) -> Vec<FoundHub> {
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else {
        return Vec::new();
    };
    let _ = socket.set_broadcast(true);
    let _ = socket.set_read_timeout(Some(Duration::from_millis(200)));
    let ask = format!("{DISCOVERY_ASK}{client_id}");
    let targets = [
        SocketAddr::from((Ipv4Addr::BROADCAST, port.saturating_add(1))),
        // This PC itself (a hub on the same machine).
        SocketAddr::from((Ipv4Addr::LOCALHOST, port.saturating_add(1))),
    ];
    let mut found: Vec<FoundHub> = Vec::new();
    let start = Instant::now();
    let deadline = start + Duration::from_millis(1500);
    let mut next_ask = start;
    let mut buf = [0u8; 512];
    while Instant::now() < deadline {
        // Ask a few times: a broadcast can be lost on a busy network.
        if Instant::now() >= next_ask {
            for target in targets {
                let _ = socket.send_to(ask.as_bytes(), target);
            }
            next_ask += Duration::from_millis(400);
        }
        let Ok((n, from)) = socket.recv_from(&mut buf) else {
            continue;
        };
        let Some(json) = std::str::from_utf8(&buf[..n])
            .ok()
            .and_then(|t| t.strip_prefix(DISCOVERY_ANSWER))
        else {
            continue;
        };
        let Ok(reply) = serde_json::from_str::<serde_json::Value>(json) else {
            continue;
        };
        if reply["client_id"].as_str() != Some(client_id.to_string().as_str()) {
            continue;
        }
        let port = reply["port"].as_u64().unwrap_or(u64::from(DEFAULT_PORT));
        let hub = FoundHub {
            address: format!("{}:{port}", from.ip()),
            name: reply["name"].as_str().unwrap_or_default().to_owned(),
        };
        if !found.contains(&hub) {
            found.push(hub);
        }
    }
    // The same hub answering on loopback and the LAN: keep the LAN address.
    if found.len() > 1 {
        found.retain(|h| !h.address.starts_with("127."));
    }
    found
}

/// This PC's IPv4 addresses on the shop network (shown on the hub).
pub fn local_addresses() -> Vec<String> {
    if_addrs::get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .filter(|i| !i.is_loopback())
                .filter_map(|i| match i.ip() {
                    std::net::IpAddr::V4(ip) => Some(ip.to_string()),
                    std::net::IpAddr::V6(_) => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

// ── This till's shop-network role ───────────────────────────────────────────

/// Mirrors `HubInfoSchema`: what the hub shows so tills can join.
#[derive(Debug, Clone, Serialize)]
pub struct HubInfo {
    pub code: String,
    pub addresses: Vec<String>,
    pub port: u16,
    pub running: bool,
    pub rows: i64,
    pub tills: i64,
}

/// Mirrors `LanStatusSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct LanStatus {
    pub settings: LanSettings,
    pub hub: Option<HubInfo>,
    pub last_error: Option<String>,
}

/// Applies the device's shop-network settings: runs the hub server, or
/// points sync at a hub, or back at the cloud (or nothing).
pub struct LanService {
    client_id: Uuid,
    /// The cloud transport of builds that have one.
    cloud: Option<Arc<dyn SyncTransport>>,
    server: std::sync::Mutex<Option<HubServer>>,
    applied: AtomicBool,
    last_error: std::sync::Mutex<Option<String>>,
}

impl LanService {
    pub fn new(client_id: Uuid, cloud: Option<Arc<dyn SyncTransport>>) -> Self {
        Self {
            client_id,
            cloud,
            server: std::sync::Mutex::new(None),
            applied: AtomicBool::new(false),
            last_error: std::sync::Mutex::new(None),
        }
    }

    pub fn applied(&self) -> bool {
        self.applied.load(Ordering::SeqCst)
    }

    pub fn settings(db: &Database) -> IpcResult<LanSettings> {
        use crate::repo::SqlResultExt;
        Ok(crate::repo::settings::get(&db.conn(), SETTINGS_KEY)
            .ipc()?
            .unwrap_or_default())
    }

    fn hub_code(db: &Database) -> IpcResult<String> {
        use crate::repo::SqlResultExt;
        let conn = db.conn();
        if let Some(code) = crate::repo::settings::get::<String>(&conn, HUB_KEY_SETTING).ipc()? {
            return Ok(code);
        }
        let code = new_pairing_code();
        crate::repo::settings::put(&conn, HUB_KEY_SETTING, &code, SystemClock.now()).ipc()?;
        Ok(code)
    }

    /// A new pairing code (the tills must enter it again).
    pub fn new_code(&self, db: &Database) -> IpcResult<()> {
        use crate::repo::SqlResultExt;
        crate::repo::settings::put(
            &db.conn(),
            HUB_KEY_SETTING,
            &new_pairing_code(),
            SystemClock.now(),
        )
        .ipc()?;
        Ok(())
    }

    /// Makes the till do what its settings say. Safe to call again.
    pub fn apply(
        &self,
        db: &Arc<Database>,
        license: &Arc<LicenseService>,
        sync: &super::SyncEngine,
    ) -> IpcResult<()> {
        use super::engine::SyncMode;
        let settings = Self::settings(db)?;
        let result = match settings.role {
            LanRole::Off => {
                *self.server.lock().unwrap_or_else(|p| p.into_inner()) = None;
                let mode = if self.cloud.is_some() {
                    SyncMode::Cloud
                } else {
                    SyncMode::Off
                };
                sync.set_transport(mode, self.cloud.clone(), "");
                Ok(())
            }
            LanRole::Hub => self.start_hub(db, license, sync, &settings),
            LanRole::Client => match (&settings.hub_address, &settings.hub_code) {
                (Some(address), Some(code)) => {
                    *self.server.lock().unwrap_or_else(|p| p.into_inner()) = None;
                    let transport = LanTransport::new(address, self.client_id, code);
                    sync.set_transport(SyncMode::Lan, Some(Arc::new(transport)), address);
                    Ok(())
                }
                _ => Err(IpcError::validation(
                    "Enter the hub's address and pairing code.",
                )),
            },
        };
        *self.last_error.lock().unwrap_or_else(|p| p.into_inner()) =
            result.as_ref().err().map(|e| e.message.clone());
        self.applied.store(true, Ordering::SeqCst);
        result
    }

    fn start_hub(
        &self,
        db: &Arc<Database>,
        license: &Arc<LicenseService>,
        sync: &super::SyncEngine,
        settings: &LanSettings,
    ) -> IpcResult<()> {
        use super::engine::SyncMode;
        use crate::repo::SqlResultExt;
        let code = Self::hub_code(db)?;
        {
            let mut conn = db.conn();
            let tx = conn.transaction().ipc()?;
            let device_id = crate::repo::device::id(&tx).ipc()?;
            hub::seed(&tx, device_id, SystemClock.now())
                .map_err(|e| IpcError::internal(e.to_string()))?;
            tx.commit().ipc()?;
        }
        let name: String = db
            .conn()
            .query_row("SELECT name FROM device LIMIT 1", [], |r| r.get(0))
            .ipc()?;
        let mut server = self.server.lock().unwrap_or_else(|p| p.into_inner());
        // (Re)start when the port or the code changed.
        *server = None;
        *server = Some(HubServer::start(
            license_db(Arc::clone(license)),
            self.client_id,
            code,
            name,
            settings.port,
        )?);
        drop(server);
        sync.set_transport(
            SyncMode::Hub,
            Some(Arc::new(LocalHubTransport {
                db: license_db(Arc::clone(license)),
            })),
            "",
        );
        Ok(())
    }

    pub fn status(&self, db: &Database) -> IpcResult<LanStatus> {
        use crate::repo::SqlResultExt;
        let settings = Self::settings(db)?;
        let hub = if settings.role == LanRole::Hub {
            let (rows, tills) = hub::stats(&db.conn()).ipc()?;
            let server = self.server.lock().unwrap_or_else(|p| p.into_inner());
            Some(HubInfo {
                code: Self::hub_code(db)?,
                addresses: local_addresses(),
                port: server.as_ref().map_or(settings.port, |s| s.port),
                running: server.is_some(),
                rows,
                tills,
            })
        } else {
            None
        };
        Ok(LanStatus {
            settings,
            hub,
            last_error: self
                .last_error
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone(),
        })
    }
}

#[cfg(test)]
mod tests;
