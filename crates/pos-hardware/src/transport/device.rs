//! Linux: a USB printer the kernel's `usblp` driver exposes as a device file
//! (`/dev/usb/lp0`), written to directly, and CUPS print queues (`lp -o raw`),
//! for printers installed through the system's printer settings.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use super::{
    is_printer_device, is_queue_name, Connection, DiscoveredPrinter, PrinterTarget, TransportError,
};

pub(super) fn send_device(
    target: &PrinterTarget,
    path: &str,
    bytes: &[u8],
) -> Result<(), TransportError> {
    if !is_printer_device(path) {
        return Err(TransportError::unreachable(target, "not a printer device"));
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.flush()))
        .map_err(|e| TransportError::unreachable(target, e))
}

pub(super) fn send_cups(
    target: &PrinterTarget,
    name: &str,
    bytes: &[u8],
) -> Result<(), TransportError> {
    if !is_queue_name(name) {
        return Err(TransportError::unreachable(target, "not a printer name"));
    }
    let mut child = Command::new("lp")
        .args(["-s", "-o", "raw", "-d", name])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| TransportError::unreachable(target, format!("lp: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(bytes)
            .map_err(|e| TransportError::unreachable(target, e))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| TransportError::unreachable(target, e))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(TransportError::unreachable(
            target,
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

pub(super) fn discover() -> Vec<DiscoveredPrinter> {
    let mut found = Vec::new();
    for dir in ["/dev/usb", "/dev"] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut paths: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|e| Path::new(dir).join(e.file_name()).display().to_string())
            .filter(|p| is_printer_device(p))
            .collect();
        paths.sort();
        found.extend(paths.into_iter().map(|path| DiscoveredPrinter {
            label: format!("USB printer ({path})"),
            target: PrinterTarget::Device { path },
            connection: Connection::Usb,
        }));
    }
    // `lpstat -e` lists the CUPS destinations (none, or no CUPS: nothing).
    if let Ok(output) = Command::new("lpstat").arg("-e").output() {
        let queues = String::from_utf8_lossy(&output.stdout);
        found.extend(
            queues
                .lines()
                .map(str::trim)
                .filter(|n| is_queue_name(n))
                .map(|name| DiscoveredPrinter {
                    label: name.to_owned(),
                    target: PrinterTarget::Cups {
                        name: name.to_owned(),
                    },
                    connection: Connection::Usb,
                }),
        );
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_write_anywhere_else() {
        let target = PrinterTarget::Device {
            path: "/tmp/not-a-printer".into(),
        };
        assert!(send_device(&target, "/tmp/not-a-printer", b"x").is_err());
    }
}
