//! Linux readers — DEVELOPMENT AND CI ONLY. Shipped installers are Windows;
//! this exists so the POS runs on a developer's Linux box. `/etc/machine-id`
//! stands in for `MachineGuid`; DMI serials are usually root-only and fall
//! back to empty.

use crate::{HardwareComponents, HwidError};

fn read_trimmed(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn cpu_brand() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|info| {
            info.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split_once(':'))
                .map(|(_, value)| value.trim().to_owned())
        })
        .unwrap_or_default()
}

pub(crate) fn collect() -> Result<HardwareComponents, HwidError> {
    let machine_id = read_trimmed("/etc/machine-id")
        .or_else(|| read_trimmed("/var/lib/dbus/machine-id"))
        .ok_or(HwidError::Missing("machine-id"))?;
    let board = read_trimmed("/sys/class/dmi/id/board_serial").unwrap_or_default();
    HardwareComponents::new(&cpu_brand(), &machine_id, &board, "")
}
