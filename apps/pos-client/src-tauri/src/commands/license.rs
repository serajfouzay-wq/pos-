//! License commands. All are pre-authentication: they run before any user
//! can log in, and none of them exposes business data.
//!
//! A till with no internet is activated with files on a USB stick: it saves
//! its code as `<till>.posactivate`, the generator reads that and saves the
//! license as `<till>.poslicense`, and the till finds that file on the stick.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pos_core::{IpcError, IpcErrorCode, IpcResult};
use pos_license::LicenseStatus;
use serde::Serialize;
use tauri::{AppHandle, State};
use uuid::Uuid;

use super::blocking;
use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn verify_license(state: State<'_, AppState>) -> IpcResult<LicenseStatus> {
    let license = Arc::clone(&state.license);
    blocking(move || Ok(license.evaluate())).await
}

/// Mirrors `ActivationRequestInfoSchema`.
#[derive(Debug, Serialize)]
pub struct ActivationRequestInfo {
    code: String,
    client_id: Uuid,
    device_name: String,
    fingerprint: String,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_activation_request(
    state: State<'_, AppState>,
) -> IpcResult<ActivationRequestInfo> {
    let license = Arc::clone(&state.license);
    blocking(move || {
        let request = license
            .activation_request()
            .map_err(|e| IpcError::new(IpcErrorCode::Hardware, e.to_string()))?;
        Ok(ActivationRequestInfo {
            code: request.encode(),
            client_id: request.client_id,
            device_name: request.device_name,
            fingerprint: request.fingerprint,
        })
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn activate_license(
    state: State<'_, AppState>,
    token: String,
) -> IpcResult<LicenseStatus> {
    activate(&state, token).await
}

const LICENSE_EXTENSION: &str = "poslicense";
const ACTIVATION_EXTENSION: &str = "posactivate";

/// Saves this till's activation code as a file (on a USB stick) for the
/// generator. `path` comes from the save dialog; `.posactivate` is enforced.
#[tauri::command(rename_all = "snake_case")]
pub async fn save_activation_file(state: State<'_, AppState>, path: String) -> IpcResult<String> {
    let license = Arc::clone(&state.license);
    blocking(move || {
        let path = with_extension(PathBuf::from(path), ACTIVATION_EXTENSION);
        let request = license
            .activation_request()
            .map_err(|e| IpcError::new(IpcErrorCode::Hardware, e.to_string()))?;
        std::fs::write(&path, format!("{}\n", request.encode()))
            .map_err(|e| IpcError::validation(format!("Cannot save {}: {e}", path.display())))?;
        Ok(path.display().to_string())
    })
    .await
}

/// `.poslicense` files on USB sticks and in Downloads, newest first.
#[tauri::command(rename_all = "snake_case")]
pub async fn find_license_files(app: AppHandle) -> IpcResult<Vec<String>> {
    use tauri::Manager;
    let downloads = app.path().download_dir().ok();
    blocking(move || {
        let roots = crate::updater::offline::search_roots(downloads);
        Ok(
            crate::updater::offline::find_files(&roots, LICENSE_EXTENSION)
                .into_iter()
                .map(|p| p.display().to_string())
                .collect(),
        )
    })
    .await
}

/// Activates with the license in a `.poslicense` file.
#[tauri::command(rename_all = "snake_case")]
pub async fn activate_license_file(
    state: State<'_, AppState>,
    path: String,
) -> IpcResult<LicenseStatus> {
    let token = blocking(move || read_license_file(Path::new(&path))).await?;
    activate(&state, token).await
}

fn with_extension(mut path: PathBuf, extension: &str) -> PathBuf {
    let has = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension));
    if !has {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".");
        name.push(extension);
        path.set_file_name(name);
    }
    path
}

/// The token in a license file. Only `.poslicense` files, and only as much
/// as a token can be: nothing else on the disk is read through here.
fn read_license_file(path: &Path) -> IpcResult<String> {
    let not_a_license = || IpcError::validation("This is not a license file (.poslicense).");
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(LICENSE_EXTENSION))
    {
        return Err(not_a_license());
    }
    let size = std::fs::metadata(path)
        .map_err(|e| IpcError::validation(format!("Cannot open {}: {e}", path.display())))?
        .len();
    if size > pos_license::jwt::MAX_TOKEN_LEN as u64 + 1024 {
        return Err(not_a_license());
    }
    let text = std::fs::read_to_string(path).map_err(|_| not_a_license())?;
    let token = text.trim();
    if token.is_empty() {
        return Err(not_a_license());
    }
    Ok(token.to_owned())
}

async fn activate(state: &State<'_, AppState>, token: String) -> IpcResult<LicenseStatus> {
    if token.len() > pos_license::jwt::MAX_TOKEN_LEN {
        return Err(IpcError::validation("license token is too long"));
    }
    let license = Arc::clone(&state.license);
    let status = blocking({
        let license = Arc::clone(&license);
        move || Ok(license.activate(&token))
    })
    .await?;
    if status.is_valid() {
        // Register with the cloud straight away; the result arrives as an event.
        tauri::async_runtime::spawn_blocking(move || license.cloud_check());
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn license_files_are_read_with_care() {
        let dir = tempfile::TempDir::new().expect("tmp");
        let good = dir.path().join("till-1.POSLICENSE");
        std::fs::write(&good, "  eyJ.token.sig \r\n").expect("write");
        assert_eq!(read_license_file(&good).expect("read"), "eyJ.token.sig");

        let other = dir.path().join("notes.txt");
        std::fs::write(&other, "secret").expect("write");
        assert!(read_license_file(&other).is_err(), "only .poslicense files");

        let empty = dir.path().join("empty.poslicense");
        std::fs::write(&empty, "\n").expect("write");
        assert!(read_license_file(&empty).is_err());

        let big = dir.path().join("big.poslicense");
        std::fs::write(&big, vec![b'a'; pos_license::jwt::MAX_TOKEN_LEN + 2048]).expect("write");
        assert!(read_license_file(&big).is_err(), "never more than a token");
    }

    #[test]
    fn the_activation_file_always_gets_its_extension() {
        assert_eq!(
            with_extension(PathBuf::from("/media/usb/Till 1"), ACTIVATION_EXTENSION),
            PathBuf::from("/media/usb/Till 1.posactivate")
        );
        assert_eq!(
            with_extension(
                PathBuf::from("/media/usb/till.posactivate"),
                ACTIVATION_EXTENSION
            ),
            PathBuf::from("/media/usb/till.posactivate")
        );
        assert_eq!(
            with_extension(PathBuf::from("/media/usb/till.txt"), ACTIVATION_EXTENSION),
            PathBuf::from("/media/usb/till.txt.posactivate")
        );
    }
}
