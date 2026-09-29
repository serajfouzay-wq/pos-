//! Update signing, held by the generator (no secrets in GitHub).
//!
//! One minisign key signs every client's releases. Its secret half lives in
//! the OS credential store (see [`crate::secrets`]); its public half is
//! committed with each client build (`updater-public-key.txt`) and compiled
//! into the till, which accepts an update only when it verifies.
//!
//! After a build, the generator signs each installer and writes a
//! `.posupdate` file: a zip of `manifest.json` and the installer. A till
//! installs it from a USB stick with no internet; the signature's trusted
//! comment (signed with the installer) names the client, the version and
//! the platform, so a file for another shop, an older version or another
//! platform is refused. The same signature serves the online channel.
//!
//! Losing the key means the tills must be reinstalled by hand to trust a new
//! one: back it up (Settings → Updates).

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use minisign::{KeyPair, PublicKey, SecretKey, SecretKeyBox};
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcResult};
use serde::Serialize;
use uuid::Uuid;

use crate::secrets::SecretStore;

pub mod channel;

pub const SECRET_NAME: &str = "update-signing-key";
/// In `clients/<slug>/` of the build repository.
pub const PUBLIC_KEY_FILE: &str = "updater-public-key.txt";
pub const PACKAGE_EXTENSION: &str = "posupdate";
pub const MANIFEST: &str = "manifest.json";
pub const FORMAT: u32 = 1;
const KEY_COMMENT: &str = "POS Factory update signing key";

/// The platforms a client build produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Platform {
    #[serde(rename = "windows-x86_64")]
    Windows,
    #[serde(rename = "linux-x86_64")]
    Linux,
}

impl Platform {
    pub const ALL: [Self; 2] = [Self::Windows, Self::Linux];

    pub fn target(self) -> &'static str {
        match self {
            Self::Windows => "windows-x86_64",
            Self::Linux => "linux-x86_64",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }

    /// The installer the updater runs (the NSIS setup; the AppImage).
    pub fn is_update_installer(self, name: &str) -> bool {
        match self {
            Self::Windows => name.ends_with("-setup.exe"),
            Self::Linux => name.ends_with(".AppImage"),
        }
    }
}

/// Mirrors `UpdateKeyStatusSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateKeyStatus {
    pub configured: bool,
    pub key_id: Option<String>,
    /// What the tills are built with (base64 of the minisign public key).
    pub public_key: Option<String>,
}

/// What a signature vouches for (its trusted comment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedRelease {
    pub client_id: Uuid,
    pub version: String,
    pub platform: Platform,
    pub file: String,
}

impl SignedRelease {
    pub fn trusted_comment(&self, now: Timestamp) -> String {
        format!(
            "timestamp:{}\tfile:{}\tversion:{}\tclient:{}\ttarget:{}",
            now.unix_seconds(),
            self.file,
            self.version,
            self.client_id,
            self.platform.target()
        )
    }
}

/// Mirrors the till's `UpdateManifest`.
#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub format: u32,
    pub client_id: Uuid,
    pub client_slug: String,
    pub version: String,
    pub target: &'static str,
    pub notes: String,
    pub installer: String,
    pub signature: String,
    pub created_at: Timestamp,
}

fn key_error(e: impl std::fmt::Display) -> IpcError {
    IpcError::internal(format!("update key: {e}"))
}

fn key_id(pk: &PublicKey) -> String {
    pk.keynum()
        .iter()
        .rev()
        .map(|b| format!("{b:02X}"))
        .collect()
}

/// The Tauri updater's form of a public key: base64 of the key file text.
pub fn tauri_public_key(pk: &PublicKey) -> IpcResult<String> {
    let text = pk.to_box().map_err(key_error)?.into_string();
    Ok(base64::engine::general_purpose::STANDARD.encode(text))
}

/// The key is stored with an empty password (the credential store protects
/// it); `Some("")` also keeps minisign from prompting on a terminal.
fn parse_secret(text: &str) -> IpcResult<SecretKey> {
    let text = format!("{}\n", text.trim());
    SecretKeyBox::from_string(&text)
        .and_then(|b| b.into_secret_key(Some(String::new())))
        .map_err(|_| IpcError::validation("That is not a POS Factory update key backup."))
}

pub struct UpdateKey {
    secret: Arc<dyn SecretStore>,
}

impl UpdateKey {
    pub fn new(secret: Arc<dyn SecretStore>) -> Self {
        Self { secret }
    }

    fn load(&self) -> IpcResult<Option<SecretKey>> {
        self.secret
            .get()?
            .map(|text| parse_secret(&text))
            .transpose()
    }

    pub fn status(&self) -> IpcResult<UpdateKeyStatus> {
        let Some(sk) = self.load()? else {
            return Ok(UpdateKeyStatus {
                configured: false,
                key_id: None,
                public_key: None,
            });
        };
        let pk = PublicKey::from_secret_key(&sk).map_err(key_error)?;
        Ok(UpdateKeyStatus {
            configured: true,
            key_id: Some(key_id(&pk)),
            public_key: Some(tauri_public_key(&pk)?),
        })
    }

    /// The key, created on first use (the first client build).
    pub fn ensure(&self) -> IpcResult<UpdateKeyStatus> {
        if self.load()?.is_none() {
            let pair =
                KeyPair::generate_encrypted_keypair(Some(String::new())).map_err(key_error)?;
            let text = pair
                .sk
                .to_box(Some(KEY_COMMENT))
                .map_err(key_error)?
                .into_string();
            self.secret.set(&text)?;
        }
        self.status()
    }

    /// The text to keep somewhere safe (it signs updates for every client).
    pub fn backup(&self) -> IpcResult<String> {
        self.secret
            .get()?
            .ok_or_else(|| IpcError::validation("There is no update key yet."))
    }

    /// Puts a backed-up key on this PC (a new or reinstalled PC). A
    /// different key is never overwritten: the tills trust only one.
    pub fn restore(&self, text: &str) -> IpcResult<UpdateKeyStatus> {
        let sk = parse_secret(text)?;
        let pk = PublicKey::from_secret_key(&sk).map_err(key_error)?;
        if let Some(current) = self.load()? {
            let current = PublicKey::from_secret_key(&current).map_err(key_error)?;
            if current.keynum() != pk.keynum() {
                return Err(IpcError::validation(
                    "This PC already has a different update key; the tills built with it would refuse updates signed by another.",
                ));
            }
        }
        let text = sk
            .to_box(Some(KEY_COMMENT))
            .map_err(key_error)?
            .into_string();
        self.secret.set(&text)?;
        self.status()
    }

    /// Base64 of the minisign signature (what the Tauri updater expects).
    pub fn sign(&self, bytes: &[u8], release: &SignedRelease, now: Timestamp) -> IpcResult<String> {
        let sk = self.load()?.ok_or_else(|| {
            IpcError::validation("There is no update key yet: build a client first.")
        })?;
        let pk = PublicKey::from_secret_key(&sk).map_err(key_error)?;
        let comment = release.trusted_comment(now);
        let signature = minisign::sign(
            Some(&pk),
            &sk,
            Cursor::new(bytes),
            Some(&comment),
            Some("signature from POS Factory"),
        )
        .map_err(key_error)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(signature.into_string()))
    }
}

/// The installers inside a build artifact (any folder), by file name.
pub fn installers_in(zip_bytes: &[u8]) -> IpcResult<Vec<(String, Vec<u8>)>> {
    let bad = |e: zip::result::ZipError| IpcError::internal(format!("the build artifact: {e}"));
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).map_err(bad)?;
    let mut found = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(bad)?;
        if !entry.is_file() {
            continue;
        }
        let Some(name) = entry
            .enclosed_name()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        else {
            continue;
        };
        let wanted = [".exe", ".msi", ".AppImage", ".deb", ".rpm"]
            .iter()
            .any(|ext| name.ends_with(ext));
        if !wanted || name.starts_with('.') {
            continue;
        }
        let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| IpcError::internal(format!("the build artifact: {e}")))?;
        found.push((name, bytes));
    }
    Ok(found)
}

/// The `.posupdate` file: the manifest, then the installer (stored as is:
/// installers are compressed already).
pub fn package(manifest: &Manifest, installer: &[u8]) -> IpcResult<Vec<u8>> {
    let io = |e: std::io::Error| IpcError::internal(format!("update file: {e}"));
    let zip_err = |e: zip::result::ZipError| IpcError::internal(format!("update file: {e}"));
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let json =
        serde_json::to_vec_pretty(manifest).map_err(|e| IpcError::internal(e.to_string()))?;
    out.start_file(MANIFEST, deflated).map_err(zip_err)?;
    out.write_all(&json).map_err(io)?;
    out.start_file(
        manifest.installer.as_str(),
        stored.large_file(installer.len() > u32::MAX as usize),
    )
    .map_err(zip_err)?;
    out.write_all(installer).map_err(io)?;
    Ok(out.finish().map_err(zip_err)?.into_inner())
}

/// Writes through a temporary file so a half-written file never looks done.
pub fn write_file(path: &Path, bytes: &[u8]) -> IpcResult<()> {
    let mut partial = PathBuf::from(path);
    partial.as_mut_os_string().push(".part");
    std::fs::write(&partial, bytes)
        .and_then(|()| std::fs::rename(&partial, path))
        .map_err(|e| IpcError::internal(format!("save {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecret;

    fn now() -> Timestamp {
        "2026-09-29T10:00:00.000Z".parse().expect("ts")
    }

    #[test]
    fn the_key_is_made_once_signs_and_restores_from_its_backup() {
        let key = UpdateKey::new(Arc::new(MemorySecret::default()));
        assert!(!key.status().expect("status").configured);
        assert!(key.backup().is_err());
        let status = key.ensure().expect("create");
        assert!(status.configured);
        assert_eq!(key.ensure().expect("again"), status, "created once");

        // The public key is what `tauri signer generate` prints.
        let public = status.public_key.clone().expect("pk");
        let text = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(&public)
                .expect("b64"),
        )
        .expect("utf8");
        assert!(text.starts_with("untrusted comment: minisign public key:"));

        // A signature the till (minisign-verify) accepts, naming the release.
        let release = SignedRelease {
            client_id: Uuid::from_u128(7),
            version: "0.1.4".into(),
            platform: Platform::Windows,
            file: "Acme_0.1.4_x64-setup.exe".into(),
        };
        let signature = key.sign(b"installer", &release, now()).expect("sign");
        let sig_text = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(&signature)
                .expect("b64"),
        )
        .expect("utf8");
        let pk = minisign_verify::PublicKey::decode(&text).expect("pk");
        let sig = minisign_verify::Signature::decode(&sig_text).expect("sig");
        pk.verify(b"installer", &sig, false).expect("verifies");
        assert!(pk.verify(b"tampered", &sig, false).is_err());
        assert!(sig.trusted_comment().contains("\tversion:0.1.4\t"));
        assert!(sig
            .trusted_comment()
            .contains(&format!("client:{}", Uuid::from_u128(7))));

        // A new PC: the backup brings back the same key; another key is refused.
        let backup = key.backup().expect("backup");
        let fresh = UpdateKey::new(Arc::new(MemorySecret::default()));
        assert!(fresh.restore("not a key").is_err());
        assert_eq!(fresh.restore(&backup).expect("restore"), status);
        let other = UpdateKey::new(Arc::new(MemorySecret::default()));
        other.ensure().expect("other");
        assert!(other.restore(&backup).is_err(), "never replaced silently");
    }

    #[test]
    fn packages_hold_the_manifest_and_the_installer() {
        let manifest = Manifest {
            format: FORMAT,
            client_id: Uuid::from_u128(7),
            client_slug: "acme".into(),
            version: "0.1.4".into(),
            target: Platform::Windows.target(),
            notes: "Faster receipts".into(),
            installer: "Acme_0.1.4_x64-setup.exe".into(),
            signature: "c2ln".into(),
            created_at: now(),
        };
        let bytes = package(&manifest, b"MZ installer").expect("package");
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
        let mut json = String::new();
        zip.by_name(MANIFEST)
            .expect("manifest")
            .read_to_string(&mut json)
            .expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(parsed["target"], "windows-x86_64");
        assert_eq!(parsed["format"], 1);
        let mut installer = Vec::new();
        zip.by_name("Acme_0.1.4_x64-setup.exe")
            .expect("installer")
            .read_to_end(&mut installer)
            .expect("read");
        assert_eq!(installer, b"MZ installer");

        // Installers are found in an artifact's platform folders.
        let mut artifact = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in [
            ("windows/Acme_0.1.4_x64-setup.exe", &b"exe"[..]),
            ("linux/Acme_0.1.4_amd64.AppImage", b"appimage"),
            ("linux/Acme_0.1.4_amd64.deb", b"deb"),
            ("linux/notes.txt", b"ignored"),
        ] {
            artifact.start_file(name, options).expect("file");
            artifact.write_all(body).expect("write");
        }
        let artifact = artifact.finish().expect("zip").into_inner();
        let found = installers_in(&artifact).expect("installers");
        let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "Acme_0.1.4_x64-setup.exe",
                "Acme_0.1.4_amd64.AppImage",
                "Acme_0.1.4_amd64.deb"
            ]
        );
        assert!(Platform::Linux.is_update_installer(names[1]));
        assert!(!Platform::Windows.is_update_installer(names[2]));
        assert!(installers_in(b"not a zip").is_err());
    }
}
