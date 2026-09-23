//! Client builds: publish the client's folder to the build repository,
//! dispatch the `build-client.yml` workflow, follow the run, fetch the
//! installer.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Duration;
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::github::{GitHub, GitHubError, RepoCheck, RepoTarget, WorkflowRun};
use crate::secrets::SecretStore;
use crate::signing::KeyStore;
use crate::store::{sha256_hex, AssetKind, BuildRecord, BuildStatus, BuildUpdate, Store};

pub const SETTINGS_KEY: &str = "build.settings";
/// Every client lives in `clients/<slug>/` of the build repository.
pub const CLIENTS_DIR: &str = "clients";
pub const CONFIG_FILE: &str = "client.json";
pub const PUBLIC_KEY_FILE: &str = "license-public-key.pem";
/// A dispatched build whose run never shows up is reported after this long.
const RUN_APPEAR_TIMEOUT_MINUTES: i64 = 30;

/// Stored repository settings (the token is in the credential store).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildSettings {
    pub repo_owner: String,
    pub repo_name: String,
    pub branch: String,
    pub workflow_file: String,
    pub api_base_url: String,
}

impl Default for BuildSettings {
    fn default() -> Self {
        Self {
            repo_owner: String::new(),
            repo_name: String::new(),
            branch: "main".into(),
            workflow_file: "build-client.yml".into(),
            api_base_url: "https://api.github.com".into(),
        }
    }
}

/// Mirrors `BuildSettingsSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildSettingsView {
    #[serde(flatten)]
    pub settings: BuildSettings,
    pub token_configured: bool,
}

fn is_name(s: &str, extra: &[u8]) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && !s.starts_with(['.', '-', '/'])
        && !s.contains("..")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b) || extra.contains(&b))
}

impl BuildSettings {
    pub fn validate(&self) -> IpcResult<()> {
        if !is_name(&self.repo_owner, b"") || !is_name(&self.repo_name, b"") {
            return Err(IpcError::validation(
                "Enter the repository as owner and name (letters, digits, - _ .).",
            ));
        }
        if !is_name(&self.branch, b"/") {
            return Err(IpcError::validation("That is not a valid branch name."));
        }
        if !is_name(&self.workflow_file, b"")
            || !(self.workflow_file.ends_with(".yml") || self.workflow_file.ends_with(".yaml"))
        {
            return Err(IpcError::validation(
                "The workflow is a file name such as build-client.yml.",
            ));
        }
        let url = self.api_base_url.as_str();
        let loopback = url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost");
        if !(url.starts_with("https://") || loopback) || url.len() > 200 {
            return Err(IpcError::validation(
                "The API URL must start with https:// (https://api.github.com for github.com).",
            ));
        }
        Ok(())
    }

    fn target(&self) -> RepoTarget {
        RepoTarget {
            api_base_url: self.api_base_url.trim_end_matches('/').to_owned(),
            owner: self.repo_owner.clone(),
            repo: self.repo_name.clone(),
            branch: self.branch.clone(),
            workflow_file: self.workflow_file.clone(),
        }
    }
}

pub fn github_error(error: GitHubError) -> IpcError {
    match error {
        GitHubError::Unauthorized(m) => IpcError::new(
            IpcErrorCode::Unauthenticated,
            format!("GitHub refused the token ({m}). Check it has Contents and Actions read/write access to the repository."),
        ),
        GitHubError::Network(m) => IpcError::new(IpcErrorCode::Offline, format!("GitHub is unreachable: {m}")),
        other => IpcError::internal(other.to_string()),
    }
}

/// GitHub's run state → ours.
pub fn run_status(run: &WorkflowRun) -> BuildStatus {
    match (run.status.as_str(), run.conclusion.as_deref()) {
        ("completed", Some("success")) => BuildStatus::Succeeded,
        ("completed", Some("cancelled" | "skipped")) => BuildStatus::Cancelled,
        ("completed", _) => BuildStatus::Failed,
        ("in_progress", _) => BuildStatus::InProgress,
        _ => BuildStatus::Queued,
    }
}

pub struct BuildService {
    store: Arc<Store>,
    github: Arc<dyn GitHub>,
    token: Arc<dyn SecretStore>,
    keys: Arc<KeyStore>,
    app_version: String,
    downloads: PathBuf,
}

impl BuildService {
    pub fn new(
        store: Arc<Store>,
        github: Arc<dyn GitHub>,
        token: Arc<dyn SecretStore>,
        keys: Arc<KeyStore>,
        app_version: String,
        downloads: PathBuf,
    ) -> Self {
        Self {
            store,
            github,
            token,
            keys,
            app_version,
            downloads,
        }
    }

    pub fn settings(&self) -> IpcResult<BuildSettingsView> {
        Ok(BuildSettingsView {
            settings: self.store.setting(SETTINGS_KEY)?.unwrap_or_default(),
            token_configured: self.token.get()?.is_some(),
        })
    }

    /// `token: Some` replaces the stored token; `None` keeps it.
    pub fn save_settings(
        &self,
        settings: &BuildSettings,
        token: Option<&str>,
        now: Timestamp,
    ) -> IpcResult<BuildSettingsView> {
        settings.validate()?;
        if let Some(token) = token.map(str::trim).filter(|t| !t.is_empty()) {
            if token.len() > 255 || token.chars().any(char::is_whitespace) {
                return Err(IpcError::validation(
                    "That does not look like a GitHub token.",
                ));
            }
            self.token.set(token)?;
        }
        self.store.put_setting(SETTINGS_KEY, settings, now)?;
        self.settings()
    }

    pub fn clear_token(&self) -> IpcResult<BuildSettingsView> {
        self.token.clear()?;
        self.settings()
    }

    fn ready(&self) -> IpcResult<(RepoTarget, String)> {
        let settings: BuildSettings = self.store.setting(SETTINGS_KEY)?.ok_or_else(|| {
            IpcError::validation("Set up the build repository in Settings first.")
        })?;
        settings.validate()?;
        let token = self
            .token
            .get()?
            .ok_or_else(|| IpcError::validation("Add a GitHub token in Settings first."))?;
        Ok((settings.target(), token))
    }

    pub fn check(&self) -> IpcResult<RepoCheck> {
        let (target, token) = self.ready()?;
        self.github.check(&token, &target).map_err(github_error)
    }

    /// The exact files a build of this client commits (paths inside
    /// `clients/<slug>/`).
    pub fn build_files(&self, client_id: Uuid) -> IpcResult<(String, BTreeMap<String, Vec<u8>>)> {
        let client = self.store.client(client_id)?;
        let public_key = self.keys.status()?.public_key_pem.ok_or_else(|| {
            IpcError::validation("Create the license signing key first (Licenses).")
        })?;
        let mut config = serde_json::to_vec_pretty(&client.config)
            .map_err(|e| IpcError::internal(e.to_string()))?;
        config.push(b'\n');
        let mut files = BTreeMap::from([
            (CONFIG_FILE.to_owned(), config),
            (PUBLIC_KEY_FILE.to_owned(), public_key.into_bytes()),
        ]);
        for kind in [AssetKind::ReceiptLogo, AssetKind::AppIcon] {
            if let Some(bytes) = self.store.asset(client_id, kind)? {
                files.insert(kind.file_name().to_owned(), bytes);
            }
        }
        Ok((client.config.client_slug, files))
    }

    /// Publishes and dispatches. Failures are recorded on the build (status
    /// `error`) so the history shows them; the record is returned either way.
    pub fn start(&self, client_id: Uuid, now: Timestamp) -> IpcResult<BuildRecord> {
        let (target, token) = self.ready()?;
        let (slug, files) = self.build_files(client_id)?;
        let config_sha = sha256_hex(&files[CONFIG_FILE]);
        let build = self
            .store
            .create_build(client_id, &config_sha, &self.app_version, now)?;
        let fail = |error: IpcError| {
            self.store.update_build(
                build.build_id,
                &BuildUpdate {
                    status: Some(BuildStatus::Error),
                    message: Some(error.message),
                    ..BuildUpdate::default()
                },
                now,
            )
        };

        let dir = format!("{CLIENTS_DIR}/{slug}");
        // `[skip ci]` keeps the regular CI from running for config commits.
        let message = format!("Build {slug} ({}) [skip ci]", build.build_id);
        let commit = match self.github.publish(&token, &target, &dir, &files, &message) {
            Ok(commit) => commit,
            Err(e) => return fail(github_error(e)),
        };
        self.store.update_build(
            build.build_id,
            &BuildUpdate {
                commit_sha: Some(commit),
                ..BuildUpdate::default()
            },
            now,
        )?;
        let inputs = BTreeMap::from([
            ("client".to_owned(), slug),
            ("build_id".to_owned(), build.build_id.to_string()),
        ]);
        if let Err(e) = self.github.dispatch(&token, &target, &inputs) {
            return fail(github_error(e));
        }
        self.store.update_build(
            build.build_id,
            &BuildUpdate {
                status: Some(BuildStatus::Queued),
                ..BuildUpdate::default()
            },
            now,
        )
    }

    /// Follows the build's workflow run; once it succeeded, records the
    /// installer artifact.
    pub fn refresh(&self, build_id: Uuid, now: Timestamp) -> IpcResult<BuildRecord> {
        let build = self.store.build(build_id)?;
        if build.status == BuildStatus::Publishing {
            // Publishing happens inside `start`; one still marked so after a
            // while was interrupted (the app closed mid-way).
            if now.signed_duration_since(build.requested_at) > Duration::minutes(10) {
                return self.store.update_build(
                    build_id,
                    &BuildUpdate {
                        status: Some(BuildStatus::Error),
                        message: Some("Publishing was interrupted. Start the build again.".into()),
                        ..BuildUpdate::default()
                    },
                    now,
                );
            }
            return Ok(build);
        }
        let needs_artifact = build.status == BuildStatus::Succeeded && build.artifact_id.is_none();
        if !build.status.is_active() && !needs_artifact {
            return Ok(build);
        }
        let (target, token) = self.ready()?;
        let marker = build_id.to_string();
        let Some(run) = self
            .github
            .find_run(&token, &target, &marker)
            .map_err(github_error)?
        else {
            let waited = now.signed_duration_since(build.requested_at);
            if waited > Duration::minutes(RUN_APPEAR_TIMEOUT_MINUTES) {
                return self.store.update_build(
                    build_id,
                    &BuildUpdate {
                        status: Some(BuildStatus::Error),
                        message: Some(format!(
                            "No workflow run appeared. Is {} on the repository's default branch?",
                            target.workflow_file
                        )),
                        ..BuildUpdate::default()
                    },
                    now,
                );
            }
            return Ok(build);
        };
        let status = run_status(&run);
        let mut update = BuildUpdate {
            status: Some(status),
            run_id: Some(run.id),
            run_url: Some(run.html_url.clone()),
            ..BuildUpdate::default()
        };
        if status == BuildStatus::Failed {
            update.message = Some("The build failed. Open the run on GitHub for the log.".into());
        }
        if status == BuildStatus::Succeeded {
            let artifact = self
                .github
                .artifacts(&token, &target, run.id)
                .map_err(github_error)?
                .into_iter()
                .find(|a| !a.expired && a.name.contains(&marker));
            match artifact {
                Some(a) => {
                    update.artifact_id = Some(a.id);
                    update.artifact_name = Some(a.name);
                    update.artifact_size = Some(a.size_in_bytes);
                }
                None => {
                    update.message = Some("The run succeeded but has no installer artifact.".into())
                }
            }
        }
        self.store.update_build(build_id, &update, now)
    }

    /// Refreshes every build still in flight (the UI polls this).
    pub fn refresh_active(&self, now: Timestamp) -> IpcResult<Vec<BuildRecord>> {
        self.store
            .active_builds()?
            .into_iter()
            .map(|b| self.refresh(b.build_id, now))
            .collect()
    }

    /// Saves the installer zip under `<Downloads>/POS Factory/<slug>/`.
    pub fn download(&self, build_id: Uuid, now: Timestamp) -> IpcResult<BuildRecord> {
        let build = self.store.build(build_id)?;
        let (Some(artifact_id), Some(name)) = (build.artifact_id, build.artifact_name.clone())
        else {
            return Err(IpcError::validation(
                "This build has no installer to download yet.",
            ));
        };
        let (target, token) = self.ready()?;
        let bytes = self
            .github
            .download_artifact(&token, &target, artifact_id)
            .map_err(github_error)?;
        let dir = self.downloads.join("POS Factory").join(&build.client_slug);
        std::fs::create_dir_all(&dir)
            .map_err(|e| IpcError::internal(format!("create {}: {e}", dir.display())))?;
        let path = dir.join(format!("{name}.zip"));
        write_atomically(&path, &bytes)?;
        self.store.update_build(
            build_id,
            &BuildUpdate {
                download_path: Some(path.display().to_string()),
                ..BuildUpdate::default()
            },
            now,
        )
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> IpcResult<()> {
    let partial = path.with_extension("zip.part");
    std::fs::write(&partial, bytes)
        .and_then(|()| std::fs::rename(&partial, path))
        .map_err(|e| IpcError::internal(format!("save {}: {e}", path.display())))
}

#[cfg(test)]
mod tests;
