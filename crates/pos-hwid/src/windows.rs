//! Windows readers: registry for CPU brand + MachineGuid, WMI for board and
//! volume serials. All safe APIs — no `unsafe` in this crate.

use serde::Deserialize;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
use winreg::RegKey;
use wmi::WMIConnection;

use crate::{HardwareComponents, HwidError};

fn read_registry(path: &str, value: &str) -> Result<String, HwidError> {
    // KEY_WOW64_64KEY: always read the native view, never the WOW6432Node copy.
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(path, KEY_READ | KEY_WOW64_64KEY)
        .and_then(|key| key.get_value::<String, _>(value))
        .map_err(|e| HwidError::Read(format!("registry {value}: {e}")))
}

#[derive(Deserialize)]
#[serde(rename = "Win32_BaseBoard", rename_all = "PascalCase")]
struct BaseBoard {
    serial_number: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_LogicalDisk", rename_all = "PascalCase")]
struct LogicalDisk {
    volume_serial_number: Option<String>,
}

pub(crate) fn collect() -> Result<HardwareComponents, HwidError> {
    let machine_guid = read_registry(r"SOFTWARE\Microsoft\Cryptography", "MachineGuid")?;
    let cpu_brand = read_registry(
        r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
        "ProcessorNameString",
    )?;

    let wmi = WMIConnection::new().map_err(|e| HwidError::Read(format!("WMI: {e}")))?;
    let board: Vec<BaseBoard> = wmi
        .raw_query("SELECT SerialNumber FROM Win32_BaseBoard")
        .map_err(|e| HwidError::Read(format!("Win32_BaseBoard: {e}")))?;
    let disks: Vec<LogicalDisk> = wmi
        .raw_query("SELECT VolumeSerialNumber FROM Win32_LogicalDisk WHERE DeviceID = 'C:'")
        .map_err(|e| HwidError::Read(format!("Win32_LogicalDisk: {e}")))?;

    let board_serial = board
        .into_iter()
        .find_map(|b| b.serial_number)
        .unwrap_or_default();
    let volume_serial = disks
        .into_iter()
        .find_map(|d| d.volume_serial_number)
        .unwrap_or_default();

    HardwareComponents::new(&cpu_brand, &machine_guid, &board_serial, &volume_serial)
}
