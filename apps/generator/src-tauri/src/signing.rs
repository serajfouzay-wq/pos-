//! License signing key store.
//!
//! The RSA-3072 private key lives on disk only as an encrypted PKCS#8 PEM
//! (scrypt + AES-256-CBC) in the generator's app-data directory. The operator
//! unlocks it with a passphrase for the session; `lock` (or quitting) drops
//! the decrypted key, which `rsa` zeroizes on drop.
//!
//! Back up BOTH files. Losing the private key means every client binary must
//! be rebuilt with a new public key before new tills can be activated.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pos_core::config::BusinessType;
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use pos_license::activation::ActivationRequest;
use pos_license::issuer::{issue_license, IssueOptions, IssuerError, SigningKey, DEFAULT_KEY_BITS};
use pos_license::keys::{key_id, parse_public_key_pem};
use pos_license::LicenseClaims;
use serde::{Deserialize, Serialize};

pub const PRIVATE_KEY_FILE: &str = "license-signing-key.pem";
pub const PUBLIC_KEY_FILE: &str = "license-public-key.pem";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    Absent,
    Locked,
    Unlocked,
}

/// Mirrors `SigningKeyStatusSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SigningKeyStatus {
    pub state: KeyState,
    pub key_id: Option<String>,
    pub public_key_pem: Option<String>,
    pub key_path: Option<String>,
}

/// Mirrors `IssueLicenseRequestSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct IssueLicenseRequest {
    pub activation_code: String,
    pub client_slug: String,
    pub business_type: BusinessType,
    pub max_devices: u32,
    pub expires_at: Option<Timestamp>,
}

/// Mirrors `IssuedLicenseSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct IssuedLicense {
    pub token: String,
    pub claims: LicenseClaims,
}

fn to_ipc(error: IssuerError) -> IpcError {
    match error {
        IssuerError::Unlock => IpcError::new(IpcErrorCode::Unauthenticated, error.to_string()),
        IssuerError::WeakPassphrase | IssuerError::InvalidRequest(_) => {
            IpcError::validation(error.to_string())
        }
        IssuerError::Key(_) => IpcError::internal(error.to_string()),
    }
}

fn io_error(context: &str, error: std::io::Error) -> IpcError {
    IpcError::internal(format!("{context}: {error}"))
}

pub struct KeyStore {
    dir: PathBuf,
    unlocked: Mutex<Option<SigningKey>>,
}

impl KeyStore {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            unlocked: Mutex::new(None),
        }
    }

    fn private_path(&self) -> PathBuf {
        self.dir.join(PRIVATE_KEY_FILE)
    }

    fn public_path(&self) -> PathBuf {
        self.dir.join(PUBLIC_KEY_FILE)
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<SigningKey>> {
        self.unlocked.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn status(&self) -> IpcResult<SigningKeyStatus> {
        if !self.private_path().exists() {
            return Ok(SigningKeyStatus {
                state: KeyState::Absent,
                key_id: None,
                public_key_pem: None,
                key_path: None,
            });
        }
        let public_key_pem = std::fs::read_to_string(self.public_path()).ok();
        let key_id = public_key_pem
            .as_deref()
            .and_then(|pem| parse_public_key_pem(pem).ok())
            .map(|key| key_id(&key));
        let state = if self.slot().is_some() {
            KeyState::Unlocked
        } else {
            KeyState::Locked
        };
        Ok(SigningKeyStatus {
            state,
            key_id,
            public_key_pem,
            key_path: Some(self.private_path().display().to_string()),
        })
    }

    /// Generates the signing key. Refuses to overwrite an existing one.
    pub fn create(&self, passphrase: &str) -> IpcResult<SigningKeyStatus> {
        if self.private_path().exists() {
            return Err(IpcError::new(
                IpcErrorCode::Conflict,
                "A signing key already exists. It is never overwritten.",
            ));
        }
        let key = SigningKey::generate(DEFAULT_KEY_BITS).map_err(to_ipc)?;
        let encrypted = key.to_encrypted_pem(passphrase).map_err(to_ipc)?;
        std::fs::create_dir_all(&self.dir).map_err(|e| io_error("create key directory", e))?;
        write_new(&self.private_path(), encrypted.as_bytes())?;
        write_new(&self.public_path(), key.public_key_pem().as_bytes())?;
        *self.slot() = Some(key);
        self.status()
    }

    pub fn unlock(&self, passphrase: &str) -> IpcResult<SigningKeyStatus> {
        let pem = std::fs::read_to_string(self.private_path())
            .map_err(|e| io_error("read signing key", e))?;
        let key = SigningKey::from_encrypted_pem(&pem, passphrase).map_err(to_ipc)?;
        if let Some(expected) = self.status()?.key_id {
            if expected != key.key_id() {
                return Err(IpcError::internal(
                    "the public key file does not belong to this signing key",
                ));
            }
        }
        *self.slot() = Some(key);
        self.status()
    }

    pub fn lock(&self) -> IpcResult<SigningKeyStatus> {
        *self.slot() = None;
        self.status()
    }

    pub fn issue(&self, request: &IssueLicenseRequest, now: Timestamp) -> IpcResult<IssuedLicense> {
        let activation = ActivationRequest::decode(&request.activation_code)
            .map_err(|e| IpcError::validation(e.to_string()))?;
        let slot = self.slot();
        let key = slot.as_ref().ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::Unauthenticated,
                "Unlock the signing key first.",
            )
        })?;
        let issued = issue_license(
            key,
            &activation,
            &IssueOptions {
                client_slug: request.client_slug.clone(),
                business_type: request.business_type,
                max_devices: request.max_devices,
                expires_at: request.expires_at,
            },
            now,
        )
        .map_err(to_ipc)?;
        Ok(IssuedLicense {
            token: issued.token,
            claims: issued.claims,
        })
    }
}

/// Creates `path`, failing if it exists (no silent overwrite of key material).
fn write_new(path: &Path, contents: &[u8]) -> IpcResult<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| io_error(&format!("create {}", path.display()), e))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|e| io_error(&format!("write {}", path.display()), e))
}

#[cfg(test)]
mod tests {
    use pos_license::verify::{verify_license, Expected};
    use uuid::Uuid;

    use super::*;

    const PASSPHRASE: &str = "correct horse battery staple";

    fn activation_code(fp: &str) -> String {
        ActivationRequest {
            client_id: Uuid::from_u128(42),
            fingerprint: fp.into(),
            device_key_hash: "5e".repeat(32),
            device_name: "TILL-01".into(),
            app_version: "0.1.0".into(),
        }
        .encode()
    }

    #[test]
    fn key_lifecycle_and_issuing() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store = KeyStore::new(dir.path().to_path_buf());
        assert_eq!(store.status().expect("status").state, KeyState::Absent);

        assert!(store.create("short").is_err(), "weak passphrase refused");
        let created = store.create(PASSPHRASE).expect("create");
        assert_eq!(created.state, KeyState::Unlocked);
        assert!(store.create(PASSPHRASE).is_err(), "never overwritten");

        let on_disk = std::fs::read_to_string(dir.path().join(PRIVATE_KEY_FILE)).expect("file");
        assert!(on_disk.starts_with("-----BEGIN ENCRYPTED PRIVATE KEY-----"));

        store.lock().expect("lock");
        let now: Timestamp = "2026-09-23T10:00:00.000Z".parse().expect("ts");
        let fp = "ab".repeat(32);
        let request = IssueLicenseRequest {
            activation_code: activation_code(&fp),
            client_slug: "acme-retail".into(),
            business_type: BusinessType::Retail,
            max_devices: 2,
            expires_at: None,
        };
        assert!(
            store.issue(&request, now).is_err(),
            "locked key cannot sign"
        );
        assert!(store.unlock("wrong passphrase!!").is_err());
        store.unlock(PASSPHRASE).expect("unlock");

        let issued = store.issue(&request, now).expect("issue");
        let public = parse_public_key_pem(created.public_key_pem.as_deref().expect("pem"))
            .expect("public key");
        verify_license(
            &issued.token,
            &public,
            created.key_id.as_deref().expect("kid"),
            &Expected {
                client_id: Uuid::from_u128(42),
                fingerprint: &fp,
                device_key_hash: &"5e".repeat(32),
            },
            now,
        )
        .expect("the POS would accept it");
    }
}
