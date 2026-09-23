//! Activation request code.
//!
//! A till without a license shows this code; the operator pastes it into the
//! generator, which signs a token bound to the fingerprint inside. The code
//! carries no secrets — only the public fingerprint hash.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PREFIX: &str = "POSACT1.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationRequest {
    pub client_id: Uuid,
    pub fingerprint: String,
    pub device_name: String,
    pub app_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActivationError {
    #[error("this is not a POS activation code")]
    NotAnActivationCode,
    #[error("the activation code is damaged — copy it again")]
    Corrupt,
    #[error("the activation code contains an invalid {0}")]
    InvalidField(&'static str),
}

pub fn is_fingerprint_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl ActivationRequest {
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).expect("activation request serializes");
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
    }

    /// Tolerates whitespace and line breaks introduced by email/chat apps.
    pub fn decode(code: &str) -> Result<Self, ActivationError> {
        let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        let body = compact
            .strip_prefix(PREFIX)
            .ok_or(ActivationError::NotAnActivationCode)?;
        if body.len() > 2048 {
            return Err(ActivationError::Corrupt);
        }
        let json = URL_SAFE_NO_PAD
            .decode(body)
            .map_err(|_| ActivationError::Corrupt)?;
        let request: Self = serde_json::from_slice(&json).map_err(|_| ActivationError::Corrupt)?;
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), ActivationError> {
        if !is_fingerprint_hash(&self.fingerprint) {
            return Err(ActivationError::InvalidField("fingerprint"));
        }
        let name_len = self.device_name.chars().count();
        if name_len == 0 || name_len > 120 {
            return Err(ActivationError::InvalidField("device name"));
        }
        if self.app_version.is_empty() || self.app_version.len() > 32 {
            return Err(ActivationError::InvalidField("app version"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ActivationRequest {
        ActivationRequest {
            client_id: Uuid::from_u128(0x8f14e45f_ceea_467a_9a4e_3b2f1c9d0a11),
            fingerprint: "ab".repeat(32),
            device_name: "TILL-01".into(),
            app_version: "0.1.0".into(),
        }
    }

    #[test]
    fn round_trips_even_when_mangled_by_chat_apps() {
        let code = sample().encode();
        let mangled = format!("  {}\n{}  ", &code[..20], &code[20..]);
        assert_eq!(ActivationRequest::decode(&mangled), Ok(sample()));
    }

    #[test]
    fn rejects_foreign_or_damaged_codes() {
        assert_eq!(
            ActivationRequest::decode("hello"),
            Err(ActivationError::NotAnActivationCode)
        );
        let mut code = sample().encode();
        code.truncate(code.len() - 5);
        assert!(ActivationRequest::decode(&code).is_err());
        let bad = ActivationRequest {
            fingerprint: "XYZ".into(),
            ..sample()
        };
        assert_eq!(
            ActivationRequest::decode(&bad.encode()),
            Err(ActivationError::InvalidField("fingerprint"))
        );
    }
}
