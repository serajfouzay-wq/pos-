//! Windows spooler, RAW datatype: the ESC/POS bytes go to the printer
//! untouched by the driver. The only `unsafe` code in the workspace; every
//! block is a direct, documented Win32 call with owned, live buffers.
#![allow(unsafe_code)]

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Graphics::Printing::{
    ClosePrinter, EndDocPrinter, EndPagePrinter, EnumPrintersW, OpenPrinterW, StartDocPrinterW,
    StartPagePrinter, WritePrinter, DOC_INFO_1W, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL,
    PRINTER_HANDLE, PRINTER_INFO_4W,
};

use super::{Connection, DiscoveredPrinter, PrinterTarget, TransportError};

/// Closes the printer handle on every exit path.
struct Handle(PRINTER_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful OpenPrinterW and is closed once.
        unsafe {
            let _ = ClosePrinter(self.0);
        }
    }
}

pub(super) fn send(target: &PrinterTarget, name: &str, bytes: &[u8]) -> Result<(), TransportError> {
    let fail = |what: &str| TransportError::unreachable(target, format!("{what} failed"));
    let len = u32::try_from(bytes.len()).map_err(|_| fail("job size"))?;
    let printer_name = HSTRING::from(name);
    let mut raw = PRINTER_HANDLE::default();
    // SAFETY: `printer_name` outlives the call; `raw` is a valid out-pointer.
    unsafe { OpenPrinterW(&printer_name, &mut raw, None) }
        .map_err(|e| TransportError::unreachable(target, e))?;
    let handle = Handle(raw);

    let mut doc_name: Vec<u16> = "POS receipt\0".encode_utf16().collect();
    let mut datatype: Vec<u16> = "RAW\0".encode_utf16().collect();
    let doc = DOC_INFO_1W {
        pDocName: PWSTR(doc_name.as_mut_ptr()),
        pOutputFile: PWSTR::null(),
        pDatatype: PWSTR(datatype.as_mut_ptr()),
    };
    // SAFETY: `doc` and the strings it points to live until the job is ended below.
    if unsafe { StartDocPrinterW(handle.0, 1, &doc) } == 0 {
        return Err(fail("StartDocPrinter"));
    }
    let mut written = 0u32;
    // SAFETY: `bytes` is valid for `len` bytes; `written` is a valid out-pointer.
    let ok = unsafe {
        StartPagePrinter(handle.0).as_bool()
            && WritePrinter(handle.0, bytes.as_ptr().cast(), len, &mut written).as_bool()
            && EndPagePrinter(handle.0).as_bool()
    };
    // SAFETY: ends the document started above (also after a failed write).
    let ended = unsafe { EndDocPrinter(handle.0) }.as_bool();
    if !ok || !ended || written != len {
        return Err(fail("WritePrinter"));
    }
    Ok(())
}

pub(super) fn discover() -> Vec<DiscoveredPrinter> {
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let (mut needed, mut returned) = (0u32, 0u32);
    // SAFETY: size query — no buffer, valid out-pointers. Expected to "fail"
    // with ERROR_INSUFFICIENT_BUFFER while reporting the size.
    let _ = unsafe { EnumPrintersW(flags, PCWSTR::null(), 4, None, &mut needed, &mut returned) };
    if needed == 0 {
        return Vec::new();
    }
    // u64-aligned storage so the PRINTER_INFO_4W records inside are aligned.
    let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: the byte view covers exactly `storage`, which outlives its use.
    let buffer = unsafe {
        std::slice::from_raw_parts_mut(storage.as_mut_ptr().cast::<u8>(), storage.len() * 8)
    };
    // SAFETY: `buffer` is at least `needed` bytes.
    if unsafe {
        EnumPrintersW(
            flags,
            PCWSTR::null(),
            4,
            Some(buffer),
            &mut needed,
            &mut returned,
        )
    }
    .is_err()
    {
        return Vec::new();
    }
    // SAFETY: on success the buffer starts with `returned` PRINTER_INFO_4W
    // records whose string pointers point into the same buffer.
    let infos = unsafe {
        std::slice::from_raw_parts(
            storage.as_ptr().cast::<PRINTER_INFO_4W>(),
            returned as usize,
        )
    };
    infos
        .iter()
        .filter_map(|info| {
            // SAFETY: pPrinterName is a NUL-terminated string inside the buffer.
            let name = unsafe { info.pPrinterName.to_string() }.ok()?;
            Some(DiscoveredPrinter {
                label: format!("{name} (Windows printer)"),
                target: PrinterTarget::WindowsPrinter { name },
                connection: Connection::Usb,
            })
        })
        .collect()
}
