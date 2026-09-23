//! Getting bytes to a printer.
//!
//! | Connection            | Transport                               |
//! | --------------------- | --------------------------------------- |
//! | USB (driver installed)| Windows spooler, RAW datatype           |
//! | USB-serial / RS-232   | COM port                                |
//! | Bluetooth SPP         | COM port (Windows pairs SPP as COMn)    |
//! | Ethernet / Wi-Fi      | TCP, port 9100                          |
//!
//! `hidapi` was considered for USB: thermal printers enumerate as the USB
//! *printer* class, not HID, so it cannot reach them without replacing the
//! driver (Zadig) — unacceptable for a zero-terminal install. The spooler
//! works with the vendor or "Generic / Text Only" driver Windows installs.

mod serial;
#[cfg(windows)]
mod spooler;
mod tcp;

use serde::{Deserialize, Serialize};

pub const DEFAULT_TCP_PORT: u16 = 9100;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrinterTarget {
    Tcp { host: String, port: u16 },
    Serial { port: String, baud_rate: u32 },
    WindowsPrinter { name: String },
}

impl PrinterTarget {
    pub fn label(&self) -> String {
        match self {
            PrinterTarget::Tcp { host, port } => format!("{host}:{port}"),
            PrinterTarget::Serial { port, baud_rate } => format!("{port} @ {baud_rate}"),
            PrinterTarget::WindowsPrinter { name } => name.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Usb,
    Network,
    Bluetooth,
    Serial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredPrinter {
    pub target: PrinterTarget,
    pub connection: Connection,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("printer {target} is unreachable: {reason}")]
    Unreachable { target: String, reason: String },
    #[error("no printer is configured")]
    NotConfigured,
    #[error("{0} printers are not supported on this platform")]
    Unsupported(&'static str),
}

impl TransportError {
    pub(crate) fn unreachable(target: &PrinterTarget, reason: impl std::fmt::Display) -> Self {
        TransportError::Unreachable {
            target: target.label(),
            reason: reason.to_string(),
        }
    }
}

/// Sends raw ESC/POS bytes to one printer.
pub fn send(target: &PrinterTarget, bytes: &[u8]) -> Result<(), TransportError> {
    match target {
        PrinterTarget::Tcp { host, port } => tcp::send(target, host, *port, bytes),
        PrinterTarget::Serial { port, baud_rate } => serial::send(target, port, *baud_rate, bytes),
        #[cfg(windows)]
        PrinterTarget::WindowsPrinter { name } => spooler::send(target, name, bytes),
        #[cfg(not(windows))]
        PrinterTarget::WindowsPrinter { .. } => Err(TransportError::Unsupported("Windows spooler")),
    }
}

/// Tries each target in order (the fallback chain) and returns the one that
/// accepted the job.
pub fn send_with_fallback<'a>(
    chain: &'a [PrinterTarget],
    bytes: &[u8],
    send_one: impl Fn(&PrinterTarget, &[u8]) -> Result<(), TransportError>,
) -> Result<&'a PrinterTarget, TransportError> {
    let mut last = TransportError::NotConfigured;
    for target in chain {
        match send_one(target, bytes) {
            Ok(()) => return Ok(target),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Printers this machine can see, ordered as the default fallback chain:
/// USB (spooler, USB-serial) → Bluetooth. Network printers cannot be
/// discovered reliably and are added by address.
pub fn discover() -> Vec<DiscoveredPrinter> {
    let mut found = Vec::new();
    #[cfg(windows)]
    found.extend(spooler::discover());
    found.extend(serial::discover());
    found.sort_by_key(|p| match p.connection {
        Connection::Usb => 0,
        Connection::Network => 1,
        Connection::Serial => 2,
        Connection::Bluetooth => 3,
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_uses_the_first_working_target() {
        let chain = vec![
            PrinterTarget::Tcp {
                host: "a".into(),
                port: 1,
            },
            PrinterTarget::Tcp {
                host: "b".into(),
                port: 2,
            },
        ];
        let used = send_with_fallback(&chain, b"x", |t, _| match t {
            PrinterTarget::Tcp { host, .. } if host == "b" => Ok(()),
            other => Err(TransportError::unreachable(other, "down")),
        })
        .expect("second works");
        assert_eq!(used, &chain[1]);
        assert!(send_with_fallback(&[], b"x", |_, _| Ok(())).is_err());
    }

    #[test]
    fn targets_serialize_with_a_kind_tag() {
        let json = serde_json::to_value(PrinterTarget::Tcp {
            host: "10.0.0.5".into(),
            port: 9100,
        })
        .expect("json");
        assert_eq!(
            json,
            serde_json::json!({ "kind": "tcp", "host": "10.0.0.5", "port": 9100 })
        );
    }
}
