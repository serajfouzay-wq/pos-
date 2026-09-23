//! Where the GitHub token lives: the OS credential store (Windows Credential
//! Manager, the shipped platform; macOS Keychain), never the workspace
//! database. Linux is a development platform only, and its kernel keyring is
//! not dependable in desktop sessions or containers, so there the token is a
//! file readable by the current user only (0600) in the app-data folder.

use std::path::PathBuf;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

use pos_core::{IpcError, IpcResult};

pub trait SecretStore: Send + Sync {
    fn get(&self) -> IpcResult<Option<String>>;
    fn set(&self, value: &str) -> IpcResult<()>;
    fn clear(&self) -> IpcResult<()>;
}

/// The platform's store for the GitHub token.
pub fn github_token_store(data_dir: PathBuf) -> Arc<dyn SecretStore> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let _ = data_dir;
        Arc::new(KeyringSecret {
            service: "com.posfactory.generator",
            user: "github-token",
        })
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Arc::new(FileSecret {
            path: data_dir.join("secrets").join("github-token"),
        })
    }
}

#[cfg(any(windows, target_os = "macos"))]
struct KeyringSecret {
    service: &'static str,
    user: &'static str,
}

#[cfg(any(windows, target_os = "macos"))]
impl KeyringSecret {
    fn entry(&self) -> IpcResult<keyring::Entry> {
        keyring::Entry::new(self.service, self.user)
            .map_err(|e| IpcError::internal(format!("credential store unavailable: {e}")))
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl SecretStore for KeyringSecret {
    fn get(&self) -> IpcResult<Option<String>> {
        match self.entry()?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(IpcError::internal(format!("read credential: {e}"))),
        }
    }

    fn set(&self, value: &str) -> IpcResult<()> {
        self.entry()?
            .set_password(value)
            .map_err(|e| IpcError::internal(format!("save credential: {e}")))
    }

    fn clear(&self) -> IpcResult<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(IpcError::internal(format!("remove credential: {e}"))),
        }
    }
}

/// Development fallback (Linux): a 0600 file.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
struct FileSecret {
    path: PathBuf,
}

impl SecretStore for FileSecret {
    fn get(&self) -> IpcResult<Option<String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(value) => Ok(Some(value.trim().to_owned()).filter(|v| !v.is_empty())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(IpcError::internal(format!("read token: {e}"))),
        }
    }

    fn set(&self, value: &str) -> IpcResult<()> {
        use std::io::Write;
        let io = |e: std::io::Error| IpcError::internal(format!("save token: {e}"));
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.path).map_err(io)?;
        file.write_all(value.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(io)
    }

    fn clear(&self) -> IpcResult<()> {
        // The token file is not a business record: removing it is the point.
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(IpcError::internal(format!("remove token: {e}"))),
        }
    }
}

/// For tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemorySecret(Mutex<Option<String>>);

#[cfg(test)]
impl SecretStore for MemorySecret {
    fn get(&self) -> IpcResult<Option<String>> {
        Ok(self.0.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }

    fn set(&self, value: &str) -> IpcResult<()> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(value.to_owned());
        Ok(())
    }

    fn clear(&self) -> IpcResult<()> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_fallback_round_trips_and_is_private() {
        let dir = tempfile::TempDir::new().expect("tmp");
        let store = FileSecret {
            path: dir.path().join("secrets").join("github-token"),
        };
        assert_eq!(store.get().expect("get"), None);
        store.set("ghp_x").expect("set");
        store.set("ghp_y").expect("overwrite");
        assert_eq!(store.get().expect("get").as_deref(), Some("ghp_y"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&store.path)
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        store.clear().expect("clear");
        store.clear().expect("idempotent");
        assert_eq!(store.get().expect("get"), None);
    }
}
