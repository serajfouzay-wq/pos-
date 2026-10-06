//! Updates from a file: the `.posupdate` the generator writes after a build
//! (a zip of `manifest.json` and the installer), brought on a USB stick to
//! a till with no internet.
//!
//! Only the signature is trusted, never the manifest: the installer must
//! verify against the update key compiled into this till, and the signed
//! trusted comment must name this client, this platform, the installer's
//! file name and a version newer than the running one. A file for another
//! shop, an older version or another system is refused before anything is
//! written.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use base64::Engine;
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::is_newer;

pub const FORMAT: u32 = 1;
const MANIFEST: &str = "manifest.json";
const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// What the generator writes (see its `updates::Manifest`).
#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    format: u32,
    version: String,
    target: String,
    #[serde(default)]
    notes: String,
    installer: String,
    signature: String,
    created_at: Timestamp,
}

/// Mirrors `UpdateFileInfoSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateFileInfo {
    pub version: String,
    pub current_version: String,
    pub notes: String,
    pub created_at: Timestamp,
    /// Newer than the running version: it can be installed.
    pub newer: bool,
}

/// A verified update, ready to install.
pub struct VerifiedUpdate {
    pub info: UpdateFileInfo,
    pub installer_name: String,
    pub installer: Vec<u8>,
}

/// This till's platform, as the generator names it.
pub fn this_target() -> String {
    let os = if cfg!(windows) {
        "windows"
    } else {
        std::env::consts::OS
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

fn refused(message: &str) -> IpcError {
    IpcError::validation(message)
}

fn not_an_update() -> IpcError {
    refused("This is not a POS update file.")
}

/// The fields of a signature's trusted comment (`key:value` tab-separated).
fn comment_field<'a>(comment: &'a str, key: &str) -> Option<&'a str> {
    comment.split('\t').find_map(|part| {
        part.split_once(':')
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v)
    })
}

fn decode_text(b64: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    String::from_utf8(bytes).ok()
}

/// Opens and verifies an update file for this till.
pub fn verify(
    bytes: &[u8],
    public_key: Option<&str>,
    client_id: Uuid,
    current_version: &str,
    target: &str,
) -> IpcResult<VerifiedUpdate> {
    let key = public_key.filter(|k| !k.trim().is_empty()).ok_or_else(|| {
        refused("This till was built without an update key: install the new version with its installer.")
    })?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| not_an_update())?;
    let manifest: Manifest = {
        let entry = archive.by_name(MANIFEST).map_err(|_| not_an_update())?;
        let mut json = Vec::new();
        entry
            .take(MAX_MANIFEST_BYTES)
            .read_to_end(&mut json)
            .map_err(|_| not_an_update())?;
        serde_json::from_slice(&json).map_err(|_| not_an_update())?
    };
    if manifest.format != FORMAT {
        return Err(refused(
            "This update file is from a newer generator: update this till with its installer.",
        ));
    }
    if manifest.target != target {
        return Err(refused(&format!(
            "This update is for {}, not this till ({target}).",
            manifest.target
        )));
    }
    let name = manifest.installer.clone();
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(not_an_update());
    }
    let mut installer = Vec::new();
    archive
        .by_name(&name)
        .map_err(|_| not_an_update())?
        .take(MAX_FILE_BYTES)
        .read_to_end(&mut installer)
        .map_err(|_| not_an_update())?;

    let key_text = decode_text(key).ok_or_else(|| IpcError::internal("bad update key"))?;
    let key = minisign_verify::PublicKey::decode(&key_text)
        .map_err(|_| IpcError::internal("bad update key"))?;
    let signature = decode_text(&manifest.signature)
        .and_then(|s| minisign_verify::Signature::decode(&s).ok())
        .ok_or_else(not_an_update)?;
    key.verify(&installer, &signature, false).map_err(|_| {
        refused("The update file is damaged or was not made for this shop's tills (signature check failed).")
    })?;

    // The signed facts, not the manifest's.
    let comment = signature.trusted_comment();
    let signed_client = comment_field(comment, "client");
    let signed_version = comment_field(comment, "version");
    if signed_client != Some(client_id.to_string().as_str()) {
        return Err(refused("This update is for another shop's tills."));
    }
    if comment_field(comment, "target") != Some(target)
        || comment_field(comment, "file") != Some(name.as_str())
        || signed_version != Some(manifest.version.as_str())
    {
        return Err(refused("The update file was altered after it was signed."));
    }
    Ok(VerifiedUpdate {
        info: UpdateFileInfo {
            newer: is_newer(&manifest.version, current_version),
            version: manifest.version,
            current_version: current_version.to_owned(),
            notes: manifest.notes.trim().to_owned(),
            created_at: manifest.created_at,
        },
        installer_name: name,
        installer,
    })
}

/// How deep the search looks under each place (a USB stick's root, its
/// folders, the generator's `POS Factory/<shop>/<version>/` layout).
const SEARCH_DEPTH: usize = 4;
/// Entries looked at per place, so a full disk never stalls the till.
const SEARCH_BUDGET: usize = 5_000;
const MAX_FOUND: usize = 20;

/// Where update files usually are: removable drives (USB sticks) and the
/// Downloads folder. Windows: every drive letter but A:, B: and C:; Linux:
/// the desktop's mount folders.
pub fn search_roots(downloads: Option<PathBuf>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        roots.extend(
            ('D'..='Z')
                .map(|d| PathBuf::from(format!("{d}:\\")))
                .filter(|p| p.exists()),
        );
    } else {
        for base in ["/media", "/run/media", "/mnt"] {
            if let Ok(entries) = std::fs::read_dir(base) {
                roots.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
            }
        }
    }
    roots.extend(downloads);
    roots
}

/// Files ending in `.<extension>` under `roots`, newest first: `.posupdate`
/// here, `.poslicense` for activation.
pub fn find_files(roots: &[PathBuf], extension: &str) -> Vec<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for root in roots {
        let mut budget = SEARCH_BUDGET;
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                if budget == 0 {
                    break;
                }
                budget -= 1;
                let path = entry.path();
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_dir() && depth < SEARCH_DEPTH {
                    let hidden = path
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with(['.', '$']));
                    if !hidden {
                        stack.push((path, depth + 1));
                    }
                } else if kind.is_file()
                    && path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
                {
                    let modified = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    found.push((modified, path));
                }
            }
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.dedup_by(|a, b| a.1 == b.1);
    found.into_iter().take(MAX_FOUND).map(|(_, p)| p).collect()
}

pub fn read(path: &Path) -> IpcResult<Vec<u8>> {
    let size = std::fs::metadata(path)
        .map_err(|e| IpcError::validation(format!("Cannot open {}: {e}", path.display())))?
        .len();
    if size > MAX_FILE_BYTES {
        return Err(not_an_update());
    }
    std::fs::read(path)
        .map_err(|e| IpcError::validation(format!("Cannot read {}: {e}", path.display())))
}

/// Starts the installer; the caller exits the till right after so its files
/// can be replaced. Windows: the NSIS setup, passive, restarting the till
/// (what the online updater does). Linux: the running AppImage is replaced
/// and the new one started.
pub fn launch(update: &VerifiedUpdate) -> IpcResult<()> {
    let io = |e: std::io::Error| IpcError::internal(format!("the update did not start: {e}"));
    if cfg!(windows) {
        let dir = std::env::temp_dir().join(format!("pos-update-{}", update.info.version));
        std::fs::create_dir_all(&dir).map_err(io)?;
        let path = dir.join(&update.installer_name);
        std::fs::write(&path, &update.installer).map_err(io)?;
        std::process::Command::new(&path)
            .args(["/P", "/UPDATE", "/R"])
            .spawn()
            .map_err(io)?;
        return Ok(());
    }
    let Some(current) = std::env::var_os("APPIMAGE").map(PathBuf::from) else {
        return Err(refused(
            "This till was installed from a .deb package: install the new .deb from the build folder instead.",
        ));
    };
    replace_appimage(&current, &update.installer).map_err(io)?;
    std::process::Command::new(&current).spawn().map_err(io)?;
    Ok(())
}

/// Writes the new AppImage next to the running one, then swaps it in (one
/// rename: the old file stays whole until the new one is complete).
fn replace_appimage(current: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut staged = current.as_os_str().to_owned();
    staged.push(".new");
    let staged = PathBuf::from(staged);
    std::fs::write(&staged, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&staged, current)
}

#[cfg(test)]
mod tests;
