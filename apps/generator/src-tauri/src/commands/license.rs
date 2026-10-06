//! License-issuing commands. Key generation and scrypt run on the blocking pool.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use pos_license::activation::ActivationRequest;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

use super::blocking;
use crate::signing::{IssueLicenseRequest, IssuedLicense, SigningKeyStatus};
use crate::state::AppState;
use crate::store::IssuedLicenseRecord;

#[tauri::command(rename_all = "snake_case")]
pub fn license_key_status(state: State<'_, AppState>) -> IpcResult<SigningKeyStatus> {
    state.keys.status()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_license_key(
    state: State<'_, AppState>,
    passphrase: String,
) -> IpcResult<SigningKeyStatus> {
    let keys = Arc::clone(&state.keys);
    blocking(move || keys.create(&passphrase)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn unlock_license_key(
    state: State<'_, AppState>,
    passphrase: String,
) -> IpcResult<SigningKeyStatus> {
    let keys = Arc::clone(&state.keys);
    blocking(move || keys.unlock(&passphrase)).await
}

#[tauri::command(rename_all = "snake_case")]
pub fn lock_license_key(state: State<'_, AppState>) -> IpcResult<SigningKeyStatus> {
    state.keys.lock()
}

#[tauri::command(rename_all = "snake_case")]
pub fn decode_activation_request(code: String) -> IpcResult<ActivationRequest> {
    ActivationRequest::decode(&code).map_err(|e| IpcError::validation(e.to_string()))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn issue_license(
    state: State<'_, AppState>,
    request: IssueLicenseRequest,
) -> IpcResult<IssuedLicense> {
    let (keys, store) = (Arc::clone(&state.keys), Arc::clone(&state.store));
    blocking(move || keys.issue(&store, &request, SystemClock.now())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_issued_licenses(
    state: State<'_, AppState>,
    client_id: Uuid,
) -> IpcResult<Vec<IssuedLicenseRecord>> {
    let store = Arc::clone(&state.store);
    blocking(move || store.licenses(client_id)).await
}

/// Saves an issued license as `<till>.poslicense` next to the client's
/// installers (`<Downloads>/POS Factory/<slug>/licenses/`) and shows it in
/// the file manager, ready to copy to the USB stick. The till finds it there.
#[tauri::command(rename_all = "snake_case")]
pub async fn save_license_file(
    app: AppHandle,
    state: State<'_, AppState>,
    client_id: Uuid,
    device_name: String,
    token: String,
) -> IpcResult<String> {
    let (store, downloads) = (Arc::clone(&state.store), state.downloads.clone());
    let path = blocking(move || {
        let slug = store.client(client_id)?.config.client_slug;
        write_license_file(&downloads, &slug, &device_name, &token)
    })
    .await?;
    // Showing the folder is a convenience: the file is saved either way.
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(path.display().to_string())
}

fn write_license_file(
    downloads: &Path,
    slug: &str,
    device_name: &str,
    token: &str,
) -> IpcResult<PathBuf> {
    let token = token.trim();
    let looks_like_token = token.len() <= pos_license::jwt::MAX_TOKEN_LEN
        && token.split('.').count() == 3
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    if !looks_like_token {
        return Err(IpcError::validation("This is not a license token."));
    }
    let dir = downloads.join("POS Factory").join(slug).join("licenses");
    std::fs::create_dir_all(&dir)
        .map_err(|e| IpcError::internal(format!("create {}: {e}", dir.display())))?;
    let path = dir.join(format!("{}.poslicense", file_stem(device_name)));
    std::fs::write(&path, format!("{token}\n"))
        .map_err(|e| IpcError::internal(format!("write {}: {e}", path.display())))?;
    Ok(path)
}

/// A till name as a file name on Windows and Linux alike.
fn file_stem(name: &str) -> String {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .take(60)
        .collect();
    let stem = stem.trim_matches(['.', ' ']).to_owned();
    if stem.is_empty() {
        "till".to_owned()
    } else {
        stem
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_license_file_lands_next_to_the_installers() {
        let dir = tempfile::TempDir::new().expect("tmp");
        let path = write_license_file(dir.path(), "al-noor", "CASHIER/1: front", " aaa.bbb.ccc\n")
            .expect("saved");
        assert_eq!(
            path,
            dir.path()
                .join("POS Factory")
                .join("al-noor")
                .join("licenses")
                .join("CASHIER-1- front.poslicense")
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "aaa.bbb.ccc\n"
        );
    }

    #[test]
    fn only_tokens_are_written_and_names_stay_safe() {
        let dir = tempfile::TempDir::new().expect("tmp");
        assert!(write_license_file(dir.path(), "a", "till", "not a token").is_err());
        assert!(write_license_file(dir.path(), "a", "till", "a.b").is_err());
        assert_eq!(file_stem("  ..  "), "till");
        assert_eq!(file_stem("..\\..\\evil"), "-..-evil");
        assert_eq!(file_stem("مقهى النور"), "مقهى النور");
    }
}
