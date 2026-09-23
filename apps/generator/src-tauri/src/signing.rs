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

use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use pos_license::activation::ActivationRequest;
use pos_license::issuer::{issue_license, IssueOptions, IssuerError, SigningKey, DEFAULT_KEY_BITS};
use pos_license::keys::{key_id, parse_public_key_pem};
use pos_license::LicenseClaims;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::store::{IssuedLicenseRecord, Store};

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

/// Mirrors `IssueLicenseRequestSchema`. Slug and business type come from
/// the client record, never from the form.
#[derive(Debug, Clone, Deserialize)]
pub struct IssueLicenseRequest {
    pub activation_code: String,
    pub client_id: Uuid,
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

    /// Signs a license for one till of a known client and records it. The
    /// activation code must come from a build of THAT client (its embedded
    /// client id), so a code pasted into the wrong client is refused.
    pub fn issue(
        &self,
        store: &Store,
        request: &IssueLicenseRequest,
        now: Timestamp,
    ) -> IpcResult<IssuedLicense> {
        let activation = ActivationRequest::decode(&request.activation_code)
            .map_err(|e| IpcError::validation(e.to_string()))?;
        let client = store.client(request.client_id)?;
        if activation.client_id != client.client_id {
            return Err(IpcError::validation(format!(
                "This activation code comes from a till built for another client ({}), not {}.",
                activation.client_id, client.config.display_name
            )));
        }
        if request.expires_at.is_some_and(|e| e <= now) {
            return Err(IpcError::validation(
                "The expiry date must be in the future.",
            ));
        }
        let issued = {
            let slot = self.slot();
            let key = slot.as_ref().ok_or_else(|| {
                IpcError::new(
                    IpcErrorCode::Unauthenticated,
                    "Unlock the signing key first.",
                )
            })?;
            issue_license(
                key,
                &activation,
                &IssueOptions {
                    client_slug: client.config.client_slug.clone(),
                    business_type: client.config.business_type,
                    max_devices: request.max_devices,
                    expires_at: request.expires_at,
                },
                now,
            )
            .map_err(to_ipc)?
        };
        store.record_license(
            &IssuedLicenseRecord {
                license_id: issued.claims.jti,
                client_id: client.client_id,
                device_name: activation.device_name.clone(),
                fingerprint_hash: activation.fingerprint.clone(),
                max_devices: request.max_devices,
                issued_at: now,
                expires_at: request.expires_at,
                token: issued.token.clone(),
            },
            now,
        )?;
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

    use super::*;

    const PASSPHRASE: &str = "correct horse battery staple";

    fn activation_code(client_id: Uuid, fp: &str) -> String {
        ActivationRequest {
            client_id,
            fingerprint: fp.into(),
            device_key_hash: "5e".repeat(32),
            device_name: "TILL-01".into(),
            app_version: "0.1.0".into(),
        }
        .encode()
    }

    fn client(store: &Store, now: Timestamp) -> Uuid {
        let config = crate::clients::new_client_config(&crate::clients::NewClientInput {
            display_name: "Acme Retail".into(),
            client_slug: "acme-retail".into(),
            business_type: pos_core::config::BusinessType::Retail,
            base_currency: pos_core::currency::CurrencyCode::KWD,
        });
        store.create_client(&config, now).expect("client").client_id
    }

    #[test]
    fn key_lifecycle_and_issuing() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store = Store::open_in_memory().expect("store");
        let keys = KeyStore::new(dir.path().to_path_buf());
        assert_eq!(keys.status().expect("status").state, KeyState::Absent);

        assert!(keys.create("short").is_err(), "weak passphrase refused");
        let created = keys.create(PASSPHRASE).expect("create");
        assert_eq!(created.state, KeyState::Unlocked);
        assert!(keys.create(PASSPHRASE).is_err(), "never overwritten");

        let on_disk = std::fs::read_to_string(dir.path().join(PRIVATE_KEY_FILE)).expect("file");
        assert!(on_disk.starts_with("-----BEGIN ENCRYPTED PRIVATE KEY-----"));

        keys.lock().expect("lock");
        let now: Timestamp = "2026-09-23T10:00:00.000Z".parse().expect("ts");
        let client_id = client(&store, now);
        let fp = "ab".repeat(32);
        let request = IssueLicenseRequest {
            activation_code: activation_code(client_id, &fp),
            client_id,
            max_devices: 2,
            expires_at: None,
        };
        assert!(
            keys.issue(&store, &request, now).is_err(),
            "locked key cannot sign"
        );
        assert!(keys.unlock("wrong passphrase!!").is_err());
        keys.unlock(PASSPHRASE).expect("unlock");

        let issued = keys.issue(&store, &request, now).expect("issue");
        assert_eq!(
            issued.claims.client_slug, "acme-retail",
            "from the client record"
        );
        let public = parse_public_key_pem(created.public_key_pem.as_deref().expect("pem"))
            .expect("public key");
        verify_license(
            &issued.token,
            &public,
            created.key_id.as_deref().expect("kid"),
            &Expected {
                client_id,
                fingerprint: &fp,
                device_key_hash: &"5e".repeat(32),
            },
            now,
        )
        .expect("the POS would accept it");
        let history = store.licenses(client_id).expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].license_id, issued.claims.jti);
        assert_eq!(history[0].device_name, "TILL-01");
    }

    #[test]
    fn a_code_from_another_clients_till_is_refused() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store = Store::open_in_memory().expect("store");
        let keys = KeyStore::new(dir.path().to_path_buf());
        keys.create(PASSPHRASE).expect("create");
        let now: Timestamp = "2026-09-23T10:00:00.000Z".parse().expect("ts");
        let client_id = client(&store, now);
        let err = keys
            .issue(
                &store,
                &IssueLicenseRequest {
                    activation_code: activation_code(Uuid::from_u128(42), &"ab".repeat(32)),
                    client_id,
                    max_devices: 1,
                    expires_at: None,
                },
                now,
            )
            .expect_err("wrong client");
        assert!(err.message.contains("another client"));
        assert!(store.licenses(client_id).expect("history").is_empty());
    }
}
