use pos_core::config::BusinessType;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const ISSUER: &str = "pos-factory";
pub const AUDIENCE: &str = "pos-client";

/// JWT payload. Mirrors `LicenseClaimsSchema` in `@pos/shared`.
/// Standard claims are seconds since the Unix epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseClaims {
    pub iss: String,
    pub aud: String,
    /// Client id.
    pub sub: Uuid,
    /// Token id — unique per issued device token.
    pub jti: Uuid,
    pub iat: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    /// `None` = perpetual.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,
    /// Hardware fingerprint of the device this token is bound to.
    pub fp: String,
    pub client_slug: String,
    pub business_type: BusinessType,
    pub max_devices: u32,
}
