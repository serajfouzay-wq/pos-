use std::io::Write;
use std::time::Duration;

use serialport::SerialPortType;

use super::{Connection, DiscoveredPrinter, PrinterTarget, TransportError};

pub const DEFAULT_BAUD_RATE: u32 = 9600;

pub(super) fn send(
    target: &PrinterTarget,
    port: &str,
    baud_rate: u32,
    bytes: &[u8],
) -> Result<(), TransportError> {
    let mut serial = serialport::new(port, baud_rate)
        .timeout(Duration::from_secs(4))
        .open()
        .map_err(|e| TransportError::unreachable(target, e))?;
    serial
        .write_all(bytes)
        .and_then(|()| serial.flush())
        .map_err(|e| TransportError::unreachable(target, e))
}

pub(super) fn discover() -> Vec<DiscoveredPrinter> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|info| {
            let (connection, detail) = match &info.port_type {
                SerialPortType::BluetoothPort => (Connection::Bluetooth, "Bluetooth".to_owned()),
                SerialPortType::UsbPort(usb) => (
                    Connection::Usb,
                    usb.product
                        .clone()
                        .unwrap_or_else(|| "USB serial".to_owned()),
                ),
                SerialPortType::PciPort | SerialPortType::Unknown => {
                    (Connection::Serial, "Serial".to_owned())
                }
            };
            DiscoveredPrinter {
                label: format!("{} ({detail})", info.port_name),
                target: PrinterTarget::Serial {
                    port: info.port_name,
                    baud_rate: DEFAULT_BAUD_RATE,
                },
                connection,
            }
        })
        .collect()
}
