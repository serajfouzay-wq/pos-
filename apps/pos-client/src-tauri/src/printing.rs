//! Receipt printing, the offline print queues and the cash drawer.
//!
//! Every receipt is a queued job (`print_jobs`) rendered from the stored
//! transaction, and every kitchen ticket a queued job (`kitchen_print_jobs`)
//! for this till's kitchen printer. The queues are drained right after a
//! sale or a course is sent and every 30 s by a background worker, so what
//! was sent while a printer was off comes out, in order, once it is back.
//! The database lock is never held during printer I/O.
//!
//! Language and mode: receipts, tickets and reports print in the till's
//! printer language (the client's default until set) and, in `auto` mode,
//! as text when the printer's own font can show everything, otherwise as an
//! image with the bundled Arabic/Latin font (`pos_hardware::raster`).
//!
//! Drawer kicks are deliberately NOT queued: a drawer popping open minutes
//! later, unattended, is worse than reporting that it could not open.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use chrono::Duration;
use pos_core::config::{ClientConfig, Locale};
use pos_core::time::{Clock, SystemClock, Zone};
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use pos_hardware::doc::{Doc, Line, PrintMode};
use pos_hardware::escpos::{Align, DRAWER_KICK};
use pos_hardware::image::{dots_for_paper, logo_from_png, MonoImage};
use pos_hardware::kitchen::KitchenTicket;
use pos_hardware::label::ProductLabel;
use pos_hardware::receipt::ReceiptTemplate;
use pos_hardware::report::ReportDoc;
use pos_hardware::transport::{self, DiscoveredPrinter, PrinterTarget, TransportError};
use pos_hardware::words::{is_rtl, words};
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
    /// Kitchen tickets whenever food is sent (restaurants/cafes). `None` =
    /// no kitchen printer.
    #[serde(default)]
    pub kitchen: Option<PrinterTarget>,
    /// The language printouts use; `None` = the client's default.
    #[serde(default)]
    pub language: Option<Locale>,
    #[serde(default)]
    pub mode: PrintMode,
    /// The receipt printer's paper when it differs from the build's (58/80).
    #[serde(default)]
    pub paper_width_mm: Option<u16>,
    #[serde(default)]
    pub kitchen_paper_width_mm: Option<u16>,
    /// Print a receipt for every sale; off = only when asked for.
    #[serde(default = "yes")]
    pub auto_print_receipt: bool,
}

impl Default for PrinterSettings {
    fn default() -> Self {
        Self {
            chain: Vec::new(),
            open_drawer_on_cash: true,
            kitchen: None,
            language: None,
            mode: PrintMode::Auto,
            paper_width_mm: None,
            kitchen_paper_width_mm: None,
            auto_print_receipt: true,
        }
    }
}

impl PrinterSettings {
    pub fn validate(&self) -> IpcResult<()> {
        if self.chain.len() > 3 {
            return Err(IpcError::validation(
                "Configure at most three printers (primary + two fallbacks).",
            ));
        }
        for target in self.chain.iter().chain(&self.kitchen) {
            match target {
                PrinterTarget::Tcp { host, port } if host.trim().is_empty() || *port == 0 => {
                    return Err(IpcError::validation(
                        "Network printers need a host and port.",
                    ));
                }
                PrinterTarget::Device { path } if !transport::is_printer_device(path) => {
                    return Err(IpcError::validation(
                        "A USB printer on Linux is a device such as /dev/usb/lp0.",
                    ));
                }
                PrinterTarget::Cups { name } if !transport::is_queue_name(name) => {
                    return Err(IpcError::validation("That is not a printer name."));
                }
                _ => {}
            }
        }
        for width in [self.paper_width_mm, self.kitchen_paper_width_mm]
            .into_iter()
            .flatten()
        {
            if width != 58 && width != 80 {
                return Err(IpcError::validation("Paper is 58 mm or 80 mm wide."));
            }
        }
        Ok(())
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
    /// Kitchen tickets waiting for the kitchen printer.
    pub kitchen_pending: i64,
    pub kitchen_error: Option<String>,
}

type Listener = Box<dyn Fn(&PrinterStatus) + Send + Sync>;

pub struct PrintService {
    io: Arc<dyn PrinterIo>,
    template: ReceiptTemplate,
    default_language: Locale,
    /// The receipt logo as supplied (PNG), rasterised per paper width.
    logo_png: Option<Vec<u8>>,
    logos: Mutex<HashMap<u16, Option<MonoImage>>>,
    health: Mutex<(Option<bool>, Option<String>)>,
    kitchen_error: Mutex<Option<String>>,
    last_published: Mutex<Option<PrinterStatus>>,
    listener: OnceLock<Listener>,
}

/// The till prints its own local time.
pub fn template_for(client: &ClientConfig) -> ReceiptTemplate {
    ReceiptTemplate {
        zone: Zone::System,
        ..ReceiptTemplate::for_client(client, None)
    }
}

/// Kitchen tickets not printed within a day are not worth printing.
const KITCHEN_STALE_HOURS: i64 = 24;

impl PrintService {
    pub fn new(io: Arc<dyn PrinterIo>, client: &ClientConfig, logo_png: Option<Vec<u8>>) -> Self {
        Self {
            io,
            template: template_for(client),
            default_language: client.locale.default,
            logo_png,
            logos: Mutex::new(HashMap::new()),
            health: Mutex::new((None, None)),
            kitchen_error: Mutex::new(None),
            last_published: Mutex::new(None),
            listener: OnceLock::new(),
        }
    }

    fn logo(&self, paper_width_mm: u16) -> Option<MonoImage> {
        let png = self.logo_png.as_ref()?;
        let mut logos = self.logos.lock().unwrap_or_else(|p| p.into_inner());
        logos
            .entry(paper_width_mm)
            .or_insert_with(|| logo_from_png(png, dots_for_paper(paper_width_mm)).ok())
            .clone()
    }

    pub fn language(&self, settings: &PrinterSettings) -> Locale {
        settings.language.unwrap_or(self.default_language)
    }

    /// The receipt layout for this till's printer (its paper and logo).
    pub fn receipt_template(&self, settings: &PrinterSettings) -> ReceiptTemplate {
        let paper = settings
            .paper_width_mm
            .unwrap_or(self.template.paper_width_mm);
        ReceiptTemplate {
            paper_width_mm: paper,
            logo: self.logo(paper),
            ..self.template.clone()
        }
    }

    fn kitchen_paper(&self, settings: &PrinterSettings) -> u16 {
        settings
            .kitchen_paper_width_mm
            .or(settings.paper_width_mm)
            .unwrap_or(self.template.paper_width_mm)
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
        let kitchen_pending = print_jobs::kitchen_pending_count(&conn).ipc()?;
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
            kitchen_pending,
            kitchen_error: self
                .kitchen_error
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone(),
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
                let settings = Self::settings(&conn)?;
                let template = self.receipt_template(&settings);
                let language = self.language(&settings);
                let jobs = print_jobs::pending(&conn, QUEUE_BATCH).ipc()?;
                let mut rendered = Vec::with_capacity(jobs.len());
                for job in jobs {
                    let receipt = sales::load_receipt(&conn, job.transaction_id, true)?;
                    let doc =
                        pos_hardware::receipt::document(&receipt, &template, job.copy, language);
                    rendered.push((job.clone(), doc.to_escpos(settings.mode)));
                }
                (settings.chain, rendered)
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

    /// Product labels on the receipt printer chain.
    pub fn print_labels(&self, db: &Database, label: &ProductLabel, copies: u8) -> IpcResult<()> {
        let settings = Self::settings(&db.conn())?;
        let paper = self.receipt_template(&settings).paper_width_mm;
        let bytes = pos_hardware::label::render_escpos(label, paper, copies, settings.mode);
        let result = self.send_chain(&settings.chain, &bytes);
        self.publish(db);
        result.map_err(hardware_error)
    }

    /// An X/Z report on the receipt printer chain (not queued: it can be
    /// printed again from the report history). `doc` is already worded in
    /// [`Self::language`].
    pub fn print_report(&self, db: &Database, doc: &ReportDoc) -> IpcResult<()> {
        let settings = Self::settings(&db.conn())?;
        if settings.chain.is_empty() {
            return Err(IpcError::new(
                IpcErrorCode::Hardware,
                "No printer is set up.",
            ));
        }
        let paper = self.receipt_template(&settings).paper_width_mm;
        let rtl = is_rtl(self.language(&settings));
        let bytes = pos_hardware::report::document(doc, paper, rtl).to_escpos(settings.mode);
        let result = self.send_chain(&settings.chain, &bytes);
        self.publish(db);
        result.map_err(hardware_error)
    }

    pub fn paper_width_mm(&self, db: &Database) -> u16 {
        Self::settings(&db.conn())
            .map(|s| self.receipt_template(&s).paper_width_mm)
            .unwrap_or(self.template.paper_width_mm)
    }

    /// The printer language (for callers that word documents themselves).
    pub fn current_language(&self, db: &Database) -> Locale {
        Self::settings(&db.conn())
            .map(|s| self.language(&s))
            .unwrap_or(self.default_language)
    }

    /// Queues a kitchen ticket when this till has a kitchen printer. Called
    /// inside the transaction that sends the food, so a sent course and its
    /// ticket are one change. Returns whether a job was queued.
    pub fn queue_kitchen(
        conn: &Connection,
        ticket: &KitchenTicket,
        now: pos_core::time::Timestamp,
    ) -> IpcResult<bool> {
        if Self::settings(conn)?.kitchen.is_none() || ticket.lines.is_empty() {
            return Ok(false);
        }
        print_jobs::enqueue_kitchen(conn, ticket, now).ipc()?;
        Ok(true)
    }

    /// Prints queued kitchen tickets oldest-first on the kitchen printer,
    /// stopping at the first failure. Returns how many are still waiting.
    pub fn drain_kitchen(&self, db: &Database) -> IpcResult<i64> {
        self.drain_kitchen_at(db, SystemClock.now())
    }

    /// [`Self::drain_kitchen`] as of `now` (tickets older than a day are
    /// dropped rather than printed).
    pub fn drain_kitchen_at(
        &self,
        db: &Database,
        now: pos_core::time::Timestamp,
    ) -> IpcResult<i64> {
        loop {
            let (target, batch) = {
                let conn = db.conn();
                let settings = Self::settings(&conn)?;
                let stale = now
                    .checked_add(Duration::hours(-KITCHEN_STALE_HOURS))
                    .unwrap_or(now);
                print_jobs::expire_kitchen(&conn, stale, now).ipc()?;
                let Some(target) = settings.kitchen.clone() else {
                    return print_jobs::kitchen_pending_count(&conn).ipc();
                };
                let paper = self.kitchen_paper(&settings);
                let language = self.language(&settings);
                let jobs = print_jobs::pending_kitchen(&conn, QUEUE_BATCH).ipc()?;
                let rendered: Vec<(uuid::Uuid, Vec<u8>)> = jobs
                    .into_iter()
                    .map(|job| {
                        let ticket = KitchenTicket {
                            zone: Zone::System,
                            ..job.ticket
                        };
                        let doc = pos_hardware::kitchen::document(&ticket, paper, language);
                        (job.id, doc.to_escpos(settings.mode))
                    })
                    .collect();
                (target, rendered)
            };
            if batch.is_empty() {
                break;
            }
            let full = batch.len() >= usize::try_from(QUEUE_BATCH).unwrap_or(usize::MAX);
            for (id, bytes) in &batch {
                let result = self.io.send(&target, bytes);
                let error = result.as_ref().err().map(ToString::to_string);
                print_jobs::mark_kitchen(&db.conn(), *id, error.as_deref(), now).ipc()?;
                *self.kitchen_error.lock().unwrap_or_else(|p| p.into_inner()) = error.clone();
                if error.is_some() {
                    self.publish(db);
                    return print_jobs::kitchen_pending_count(&db.conn()).ipc();
                }
            }
            if !full {
                break;
            }
        }
        self.publish(db);
        print_jobs::kitchen_pending_count(&db.conn()).ipc()
    }

    /// The kitchen printer's last error (`None` once it printed again).
    pub fn kitchen_error(&self) -> Option<String> {
        self.kitchen_error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// A test page in the given (or saved) language and mode: the shop name,
    /// a line in each language and the printer's address.
    pub fn test_print(
        &self,
        db: &Database,
        target: &PrinterTarget,
        language: Option<Locale>,
        mode: Option<PrintMode>,
        paper_width_mm: Option<u16>,
    ) -> IpcResult<()> {
        let settings = Self::settings(&db.conn())?;
        let language = language.unwrap_or_else(|| self.language(&settings));
        let mode = mode.unwrap_or(settings.mode);
        let paper =
            paper_width_mm.unwrap_or_else(|| self.receipt_template(&settings).paper_width_mm);
        let mut doc = Doc::new(paper, is_rtl(language));
        doc.push(Line::large(
            self.template.business_name.as_str(),
            Align::Center,
        ))
        .push(Line::bold(words(language).printer_test, Align::Center))
        .push(Line::Rule)
        .push(Line::row("English", "1.250"))
        .push(Line::row("العربية", "1.250"))
        .push(Line::Rule)
        .push(Line::text(target.label(), Align::Center));
        self.io
            .send(target, &doc.to_escpos(mode))
            .map_err(hardware_error)
    }
}

pub fn hardware_error(e: TransportError) -> IpcError {
    IpcError::new(IpcErrorCode::Hardware, e.to_string())
}
