//! Minimal compact-JWS implementation, RS256 only.
//!
//! Deliberately not a general JWT library: the algorithm is fixed, so
//! `alg: none`, HS256-with-public-key and similar confusion attacks are
//! impossible by construction.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::signature::Verifier;
use rsa::RsaPublicKey;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::claims::LicenseClaims;

/// Tokens are a few hundred bytes; anything huge is hostile.
pub const MAX_TOKEN_LEN: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub alg: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typ: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JwtError {
    #[error("token is not a well-formed JWT")]
    Malformed,
    #[error("token uses unsupported algorithm {0:?}; only RS256 is accepted")]
    UnsupportedAlgorithm(String),
    #[error("token was signed with a different key (kid {0})")]
    UnknownKey(String),
    #[error("token signature is invalid")]
    BadSignature,
    #[error("token claims are invalid: {0}")]
    InvalidClaims(String),
}

#[cfg(feature = "issuer")]
pub(crate) fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(part: &str) -> Result<Vec<u8>, JwtError> {
    URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| JwtError::Malformed)
}

/// Verifies signature and decodes claims. Does NOT check time, audience or
/// fingerprint — see [`crate::verify::verify_license`] for the full policy.
pub fn decode_verified(
    token: &str,
    key: &RsaPublicKey,
    expected_kid: &str,
) -> Result<(Header, LicenseClaims), JwtError> {
    let token = token.trim();
    if token.is_empty() || token.len() > MAX_TOKEN_LEN {
        return Err(JwtError::Malformed);
    }
    let mut parts = token.split('.');
    let (Some(header_b64), Some(payload_b64), Some(signature_b64), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(JwtError::Malformed);
    };

    let header: Header =
        serde_json::from_slice(&unb64(header_b64)?).map_err(|_| JwtError::Malformed)?;
    if header.alg != "RS256" {
        return Err(JwtError::UnsupportedAlgorithm(header.alg));
    }
    if let Some(kid) = &header.kid {
        if kid != expected_kid {
            return Err(JwtError::UnknownKey(kid.clone()));
        }
    }

    let signature = Signature::try_from(unb64(signature_b64)?.as_slice())
        .map_err(|_| JwtError::BadSignature)?;
    let signing_input = &token[..header_b64.len() + 1 + payload_b64.len()];
    VerifyingKey::<Sha256>::new(key.clone())
        .verify(signing_input.as_bytes(), &signature)
        .map_err(|_| JwtError::BadSignature)?;

    // Only parse the payload once it is known to be ours.
    let claims: LicenseClaims = serde_json::from_slice(&unb64(payload_b64)?)
        .map_err(|e| JwtError::InvalidClaims(e.to_string()))?;
    Ok((header, claims))
}
