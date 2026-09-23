//! The complete client-side acceptance policy for a license token.

use chrono::Duration;
use pos_core::time::Timestamp;
use rsa::RsaPublicKey;
use uuid::Uuid;

use crate::claims::{LicenseClaims, AUDIENCE, ISSUER};
use crate::jwt::{self, JwtError};

/// Tolerated clock difference between generator and till for `iat`/`nbf`.
pub const CLOCK_SKEW: Duration = Duration::minutes(5);

pub struct Expected<'a> {
    pub client_id: Uuid,
    pub fingerprint: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Rejection {
    #[error("{0}")]
    InvalidToken(String),
    #[error("this license belongs to a different business")]
    WrongClient,
    #[error("this license was issued for a different computer")]
    FingerprintMismatch,
    #[error("this license is not valid until {0}")]
    NotYetValid(Timestamp),
    #[error("this license expired on {0}")]
    Expired(Timestamp),
}

impl From<JwtError> for Rejection {
    fn from(error: JwtError) -> Self {
        Rejection::InvalidToken(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedLicense {
    pub claims: LicenseClaims,
    pub issued_at: Timestamp,
    pub expires_at: Option<Timestamp>,
}

fn ts(seconds: i64, label: &str) -> Result<Timestamp, Rejection> {
    Timestamp::from_unix_seconds(seconds)
        .ok_or_else(|| Rejection::InvalidToken(format!("{label} is out of range")))
}

/// Signature → issuer/audience → client → hardware → validity window.
/// `now` should be the *effective* now (see [`crate::grace::effective_now`]).
pub fn verify_license(
    token: &str,
    key: &RsaPublicKey,
    key_id: &str,
    expected: &Expected<'_>,
    now: Timestamp,
) -> Result<VerifiedLicense, Rejection> {
    let verified = verify_identity(token, key, key_id, expected)?;
    check_validity_window(&verified, now)?;
    Ok(verified)
}

/// Everything except time: signature, issuer/audience, client and hardware.
/// Passing this proves the token was issued for *this* machine, which is what
/// gates opening the database; time checks can then use the database's
/// tamper-resistant clock (see [`check_validity_window`]).
pub fn verify_identity(
    token: &str,
    key: &RsaPublicKey,
    key_id: &str,
    expected: &Expected<'_>,
) -> Result<VerifiedLicense, Rejection> {
    let (_, claims) = jwt::decode_verified(token, key, key_id)?;

    if claims.iss != ISSUER || claims.aud != AUDIENCE {
        return Err(Rejection::InvalidToken(
            "token was not issued for the POS client".into(),
        ));
    }
    if claims.sub != expected.client_id {
        return Err(Rejection::WrongClient);
    }
    if claims.fp != expected.fingerprint {
        return Err(Rejection::FingerprintMismatch);
    }

    let issued_at = ts(claims.iat, "iat")?;
    let expires_at = claims.exp.map(|exp| ts(exp, "exp")).transpose()?;
    Ok(VerifiedLicense {
        claims,
        issued_at,
        expires_at,
    })
}

/// `nbf`/`iat` (with [`CLOCK_SKEW`]) and `exp` against `now`.
pub fn check_validity_window(verified: &VerifiedLicense, now: Timestamp) -> Result<(), Rejection> {
    let not_before = verified
        .claims
        .nbf
        .map(|nbf| ts(nbf, "nbf"))
        .transpose()?
        .unwrap_or(verified.issued_at);
    let skewed_now = now.checked_add(CLOCK_SKEW).unwrap_or(now);
    if not_before > skewed_now {
        return Err(Rejection::NotYetValid(not_before));
    }
    if let Some(exp) = verified.expires_at {
        if now >= exp {
            return Err(Rejection::Expired(exp));
        }
    }
    Ok(())
}
