//! License tokens for the POS Factory.
//!
//! * The generator signs an RS256 JWT ([`LicenseClaims`]) with its private key
//!   (feature `issuer`).
//! * The POS binary embeds the public key only and verifies signature, issuer,
//!   audience, client, validity window and hardware fingerprint
//!   ([`verify::verify_license`]).
//! * [`grace`] decides whether a device that cannot reach the cloud may keep
//!   trading (7-day window).
//! * [`activation`] encodes the request code a new till shows the operator.

pub mod activation;
pub mod claims;
pub mod grace;
pub mod jwt;
pub mod keys;
pub mod status;
pub mod verify;

#[cfg(feature = "issuer")]
pub mod issuer;

pub use claims::LicenseClaims;
pub use status::LicenseStatus;
