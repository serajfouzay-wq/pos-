//! Domain core shared by the POS client and the generator.
//!
//! Every rule in here has a TypeScript twin in `@pos/shared`; both are pinned
//! to the JSON fixtures in `packages/shared/contracts` by their test suites.

pub mod config;
pub mod currency;
pub mod error;
pub mod money;
pub mod pricing;
pub mod rbac;
pub mod receipt;
pub mod sales;
pub mod tender;
pub mod time;

pub use error::{IpcError, IpcErrorCode, IpcResult};
