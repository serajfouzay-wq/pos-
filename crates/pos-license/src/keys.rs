//! Public-key handling shared by verifier and issuer.

use rsa::pkcs8::{DecodePublicKey, EncodePublicKey};
use rsa::traits::PublicKeyParts;
use rsa::RsaPublicKey;
use sha2::{Digest, Sha256};

/// Smallest modulus we accept. RS256 with < 2048 bits is not safe.
pub const MIN_KEY_BITS: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("not a valid RSA public key (expected SPKI PEM): {0}")]
    InvalidPublicKey(String),
    #[error("RSA key is {0} bits; at least {MIN_KEY_BITS} are required")]
    TooSmall(usize),
}

pub fn parse_public_key_pem(pem: &str) -> Result<RsaPublicKey, KeyError> {
    let key = RsaPublicKey::from_public_key_pem(pem.trim())
        .map_err(|e| KeyError::InvalidPublicKey(e.to_string()))?;
    let bits = key.n().bits();
    if bits < MIN_KEY_BITS {
        return Err(KeyError::TooSmall(bits));
    }
    Ok(key)
}

/// Short, stable key identifier: first 8 bytes of SHA-256(SPKI DER), hex.
/// Carried in the JWT `kid` header so a token signed by a different key is
/// reported as such instead of as a generic bad signature.
pub fn key_id(key: &RsaPublicKey) -> String {
    let der = key
        .to_public_key_der()
        .expect("an RSA public key always encodes to DER");
    hex::encode(&Sha256::digest(der.as_bytes())[..8])
}
