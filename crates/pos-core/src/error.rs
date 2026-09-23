//! The error type every Tauri command returns.
//!
//! Serialized as `{ "code": "...", "message": "..." }`, matching
//! `IpcErrorPayloadSchema` in `@pos/shared`. Messages must be safe to show
//! to the user: never include SQL, key material or fingerprint inputs.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcErrorCode {
    Unauthenticated,
    Forbidden,
    LicenseInvalid,
    Validation,
    NotFound,
    Conflict,
    Hardware,
    Offline,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct IpcError {
    pub code: IpcErrorCode,
    pub message: String,
}

impl IpcError {
    pub fn new(code: IpcErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::Forbidden, message)
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::Validation, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::Internal, message)
    }
}

pub type IpcResult<T> = Result<T, IpcError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_the_shared_wire_shape() {
        let json = serde_json::to_value(IpcError::forbidden("nope")).expect("serializable");
        assert_eq!(
            json,
            serde_json::json!({ "code": "forbidden", "message": "nope" })
        );
    }
}
