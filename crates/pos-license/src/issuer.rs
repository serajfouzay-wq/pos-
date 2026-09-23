//! Signing side — compiled into the generator only (feature `issuer`).
//!
//! The private key is stored as an encrypted PKCS#8 PEM (scrypt +
//! AES-256-CBC, via the `pkcs8` crate). It is decrypted into memory only while
//! the operator has it unlocked.
//!
//! Note: the `rsa` crate has a known timing side channel (RUSTSEC-2023-0071,
//! "Marvin"). It is only exploitable by an attacker who can time many private-
//! key operations; signing here is local and operator-initiated, and blinding
//! is enabled via `RandomizedSigner`.

use chrono::Duration;
use pos_core::config::BusinessType;
use pos_core::time::Timestamp;
use rand::rngs::OsRng;
use rsa::pkcs1v15::SigningKey as Pkcs1SigningKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::signature::{RandomizedSigner, SignatureEncoding};
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::activation::ActivationRequest;
use crate::claims::{LicenseClaims, AUDIENCE, ISSUER};
use crate::jwt::{b64, Header};
use crate::keys::{key_id, MIN_KEY_BITS};

pub const DEFAULT_KEY_BITS: usize = 3072;
pub const MIN_PASSPHRASE_CHARS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IssuerError {
    #[error("passphrase must be at least {MIN_PASSPHRASE_CHARS} characters")]
    WeakPassphrase,
    #[error("wrong passphrase, or the key file is damaged")]
    Unlock,
    #[error("key error: {0}")]
    Key(String),
    #[error("{0}")]
    InvalidRequest(String),
}

pub struct SigningKey {
    private: RsaPrivateKey,
    public: RsaPublicKey,
    kid: String,
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SigningKey {{ kid: {} }}", self.kid)
    }
}

impl SigningKey {
    fn from_private(private: RsaPrivateKey) -> Result<Self, IssuerError> {
        let bits = private.n().bits();
        if bits < MIN_KEY_BITS {
            return Err(IssuerError::Key(format!("{bits}-bit key is too small")));
        }
        let public = private.to_public_key();
        let kid = key_id(&public);
        Ok(Self {
            private,
            public,
            kid,
        })
    }

    pub fn generate(bits: usize) -> Result<Self, IssuerError> {
        let private =
            RsaPrivateKey::new(&mut OsRng, bits).map_err(|e| IssuerError::Key(e.to_string()))?;
        Self::from_private(private)
    }

    pub fn from_encrypted_pem(pem: &str, passphrase: &str) -> Result<Self, IssuerError> {
        let private = RsaPrivateKey::from_pkcs8_encrypted_pem(pem, passphrase.as_bytes())
            .map_err(|_| IssuerError::Unlock)?;
        Self::from_private(private)
    }

    /// Unencrypted PKCS#8 — development keys and tests only.
    pub fn from_unencrypted_pem(pem: &str) -> Result<Self, IssuerError> {
        let private =
            RsaPrivateKey::from_pkcs8_pem(pem).map_err(|e| IssuerError::Key(e.to_string()))?;
        Self::from_private(private)
    }

    pub fn to_encrypted_pem(&self, passphrase: &str) -> Result<Zeroizing<String>, IssuerError> {
        if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
            return Err(IssuerError::WeakPassphrase);
        }
        self.private
            .to_pkcs8_encrypted_pem(&mut OsRng, passphrase.as_bytes(), LineEnding::LF)
            .map_err(|e| IssuerError::Key(e.to_string()))
    }

    pub fn public_key_pem(&self) -> String {
        self.public
            .to_public_key_pem(LineEnding::LF)
            .expect("an RSA public key always encodes to PEM")
    }

    pub fn key_id(&self) -> &str {
        &self.kid
    }

    /// Signs arbitrary claims. Prefer [`issue_license`], which fills and
    /// validates them.
    pub fn sign(&self, claims: &LicenseClaims) -> String {
        let header = Header {
            alg: "RS256".into(),
            typ: Some("JWT".into()),
            kid: Some(self.kid.clone()),
        };
        let header_json = serde_json::to_vec(&header).expect("header serializes");
        let claims_json = serde_json::to_vec(claims).expect("claims serialize");
        let signing_input = format!("{}.{}", b64(&header_json), b64(&claims_json));
        let signer = Pkcs1SigningKey::<Sha256>::new(self.private.clone());
        let signature = signer.sign_with_rng(&mut OsRng, signing_input.as_bytes());
        format!("{signing_input}.{}", b64(&signature.to_bytes()))
    }
}

/// What the operator chooses when issuing a device token.
#[derive(Debug, Clone)]
pub struct IssueOptions {
    pub client_slug: String,
    pub business_type: BusinessType,
    pub max_devices: u32,
    pub expires_at: Option<Timestamp>,
}

pub struct IssuedLicense {
    pub token: String,
    pub claims: LicenseClaims,
}

pub fn issue_license(
    key: &SigningKey,
    request: &ActivationRequest,
    options: &IssueOptions,
    now: Timestamp,
) -> Result<IssuedLicense, IssuerError> {
    request
        .validate()
        .map_err(|e| IssuerError::InvalidRequest(e.to_string()))?;
    if options.max_devices == 0 || options.max_devices > 1000 {
        return Err(IssuerError::InvalidRequest(
            "max devices must be between 1 and 1000".into(),
        ));
    }
    if options.client_slug.is_empty() || options.client_slug.len() > 40 {
        return Err(IssuerError::InvalidRequest(
            "client slug is required".into(),
        ));
    }
    if let Some(exp) = options.expires_at {
        if exp <= now.checked_add(Duration::days(1)).unwrap_or(now) {
            return Err(IssuerError::InvalidRequest(
                "expiry must be at least one day in the future".into(),
            ));
        }
    }
    let claims = LicenseClaims {
        iss: ISSUER.into(),
        aud: AUDIENCE.into(),
        sub: request.client_id,
        jti: Uuid::new_v4(),
        iat: now.unix_seconds(),
        nbf: None,
        exp: options.expires_at.map(Timestamp::unix_seconds),
        fp: request.fingerprint.clone(),
        client_slug: options.client_slug.clone(),
        business_type: options.business_type,
        max_devices: options.max_devices,
    };
    Ok(IssuedLicense {
        token: key.sign(&claims),
        claims,
    })
}
