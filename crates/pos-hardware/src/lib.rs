//! Receipt printer and cash drawer support.
//!
//! * [`escpos`] — command encoder (text, alignment, emphasis, raster, cut).
//! * [`receipt`] — lays a [`pos_core::receipt::Receipt`] out for 58/80 mm paper.
//! * [`label`] — product labels with EAN-13/EAN-8/UPC-A or Code 128 barcodes.
//! * [`kitchen`] — kitchen tickets for fired courses.
//! * [`image`] — PNG logo → 1-bit raster (Floyd–Steinberg dithered).
//! * [`transport`] — TCP/9100, serial COM ports (USB-serial and Bluetooth SPP,
//!   which Windows exposes as COM ports) and the Windows spooler (USB printers
//!   installed with a driver), plus discovery.
//!
//! Cash drawers hang off the printer's RJ11 port, so a drawer kick is just
//! [`escpos::DRAWER_KICK`] sent to the printer.

pub mod escpos;
pub mod image;
pub mod kitchen;
pub mod label;
pub mod receipt;
pub mod transport;
