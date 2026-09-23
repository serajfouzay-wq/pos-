//! Receipt printing, the offline print queue and the cash drawer.
//!
//! Every receipt is a queued job (`print_jobs`) rendered from the stored
//! transaction. The queue is drained right after a sale and every 30 s by a
//! background worker, so receipts taken while the printer was off come out, in
//! order, once it is back. The database lock is never held during printer I/O.
//!
//! Drawer kicks are deliberately NOT queued: a drawer popping open minutes
//! later, unattended, is worse than reporting that it could not open.

use std::sync::{Arc, Mutex, OnceLock};

use pos_core::config::ClientConfig;
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use pos_hardware::escpos::{Align, EscPos, DRAWER_KICK};
use pos_hardware::image::MonoImage;
use pos_hardware::receipt::{render_escpos, ReceiptTemplate};
use pos_hardware::transport::{self, DiscoveredPrinter, PrinterTarget, TransportError};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::repo::{print_jobs, sales, settings, SqlResultExt};

pub const SETTINGS_KEY: &str = "printer";
const QUEUE_BATCH: i64 = 25;

pub trait PrinterIo: Send + Sync {
    fn send(&self, target: &PrinterTarget, bytes: &[u8]) -> Result<(), TransportError>;
    fn discover(&self) -> Vec<DiscoveredPrinter>;
}

pub struct SystemPrinters;

impl PrinterIo for SystemPrinters {
    fn send(&self, target: &PrinterTarget, bytes: &[u8]) -> Result<(), TransportError> {
        transport::send(target, bytes)
    }
    fn discover(&self) -> Vec<DiscoveredPrinter> {
        transport::discover()
    }
}

fn yes() -> bool {
    true
}

/// Mirrors `PrinterSettingsSchema`. `chain` is tried in order (fallback).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrinterSettings {
    pub chain: Vec<PrinterTarget>,
    #[serde(default = "yes")]
    pub open_drawer_on_cash: bool,
}

impl Default for PrinterSettings {
    fn default() -> Self {
        Self {
            chain: Vec::new(),
            open_drawer_on_cash: true,
        }
    }
}

/// Mirrors `PrinterStatusSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrinterStatus {
    pub configured: bool,
    /// `None` until the first print attempt.
    pub online: Option<bool>,
    pub pending_jobs: i64,
    pub last_error: Option<String>,
}

type Listener = Box<dyn Fn(&PrinterStatus) + Send + Sync>;

pub struct PrintService {
    io: Arc<dyn PrinterIo>,
    template: ReceiptTemplate,
    health: Mutex<(Option<bool>, Option<String>)>,
    last_published: Mutex<Option<PrinterStatus>>,
    listener: OnceLock<Listener>,
}

pub fn template_for(client: &ClientConfig, logo: Option<MonoImage>) -> ReceiptTemplate {
    let receipt = &client.receipt;
    ReceiptTemplate {
        business_name: client.display_name.clone(),
        header_lines: receipt.header_lines.clone(),
        footer_text: receipt.footer_text.clone(),
        tax_number: receipt
            .show_tax_number
            .then(|| client.tax.registration_number.clone())
            .flatten(),
        paper_width_mm: receipt.paper_width_mm,
        logo,
    }
}

impl PrintService {
    pub fn new(io: Arc<dyn PrinterIo>, template: ReceiptTemplate) -> Self {
        Self {
            io,
            template,
            health: Mutex::new((None, None)),
            last_published: Mutex::new(None),
            listener: OnceLock::new(),
        }
    }

    pub fn set_listener(&self, listener: impl Fn(&PrinterStatus) + Send + Sync + 'static) {
        let _ = self.listener.set(Box::new(listener));
    }

    pub fn settings(conn: &Connection) -> IpcResult<PrinterSettings> {
        Ok(settings::get(conn, SETTINGS_KEY).ipc()?.unwrap_or_default())
    }

    pub fn discover(&self) -> Vec<DiscoveredPrinter> {
        self.io.discover()
    }

    fn record(&self, result: &Result<(), TransportError>) {
        let mut health = self.health.lock().unwrap_or_else(|p| p.into_inner());
        *health = match result {
            Ok(()) => (Some(true), None),
            Err(TransportError::NotConfigured) => {
                (None, Some(TransportError::NotConfigured.to_string()))
            }
            Err(e) => (Some(false), Some(e.to_string())),
        };
    }

    fn send_chain(&self, chain: &[PrinterTarget], bytes: &[u8]) -> Result<(), TransportError> {
        let result =
            transport::send_with_fallback(chain, bytes, |t, b| self.io.send(t, b)).map(|_| ());
        self.record(&result);
        result
    }

    pub fn status(&self, db: &Database) -> IpcResult<PrinterStatus> {
        let conn = db.conn();
        let configured = !Self::settings(&conn)?.chain.is_empty();
        let pending_jobs = print_jobs::pending_count(&conn).ipc()?;
        let (online, last_error) = self
            .health
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        Ok(PrinterStatus {
            configured,
            online,
            pending_jobs,
            last_error,
        })
    }

    fn publish(&self, db: &Database) {
        let Ok(status) = self.status(db) else { return };
        let mut last = self
            .last_published
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if last.as_ref() != Some(&status) {
            if let Some(listener) = self.listener.get() {
                listener(&status);
            }
            *last = Some(status);
        }
    }

    /// Prints queued receipts oldest-first, stopping at the first failure so
    /// order is preserved. Returns whether `watch` (if given) got printed.
    pub fn drain(&self, db: &Database, watch: Option<uuid::Uuid>) -> IpcResult<bool> {
        let mut watched_printed = false;
        loop {
            // Render under the lock…
            let (chain, batch) = {
                let conn = db.conn();
                let chain = Self::settings(&conn)?.chain;
                let jobs = print_jobs::pending(&conn, QUEUE_BATCH).ipc()?;
                let mut rendered = Vec::with_capacity(jobs.len());
                for job in jobs {
                    let receipt = sales::load_receipt(&conn, job.transaction_id, true)?;
                    rendered.push((
                        job.clone(),
                        render_escpos(&receipt, &self.template, job.copy),
                    ));
                }
                (chain, rendered)
            };
            if batch.is_empty() {
                break;
            }
            if chain.is_empty() {
                self.record(&Err(TransportError::NotConfigured));
                break;
            }
            // …print without it.
            let mut failed = false;
            for (job, bytes) in &batch {
                let result = self.send_chain(&chain, bytes);
                let now = SystemClock.now();
                let conn = db.conn();
                match result {
                    Ok(()) => {
                        print_jobs::mark_printed(&conn, job.id, now).ipc()?;
                        watched_printed |= Some(job.transaction_id) == watch;
                    }
                    Err(e) => {
                        print_jobs::mark_failed(&conn, job.id, &e.to_string(), now).ipc()?;
                        failed = true;
                        break;
                    }
                }
            }
            if failed || batch.len() < usize::try_from(QUEUE_BATCH).unwrap_or(usize::MAX) {
                break;
            }
        }
        self.publish(db);
        Ok(watched_printed)
    }

    pub fn kick_drawer(&self, db: &Database) -> IpcResult<()> {
        let chain = Self::settings(&db.conn())?.chain;
        let result = self.send_chain(&chain, &DRAWER_KICK);
        self.publish(db);
        result.map_err(hardware_error)
    }

    pub fn test_print(&self, target: &PrinterTarget) -> IpcResult<()> {
        let mut page = EscPos::new();
        page.align(Align::Center)
            .bold(true)
            .line(&self.template.business_name)
            .bold(false)
            .line("Printer test OK")
            .line(&target.label())
            .feed(3)
            .cut();
        self.io
            .send(target, &page.into_bytes())
            .map_err(hardware_error)
    }
}

pub fn hardware_error(e: TransportError) -> IpcError {
    IpcError::new(IpcErrorCode::Hardware, e.to_string())
}
