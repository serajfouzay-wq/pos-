//! Client builds: publish the client's folder to the build repository,
//! dispatch the `build-client.yml` workflow, follow the run, fetch the
//! installers, and sign them into update files (see [`crate::updates`]).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Duration;
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::github::{GitHub, GitHubError, RepoCheck, RepoTarget, WorkflowRun};
use crate::secrets::{SecretFactory, SecretStore, GITHUB_TOKEN};
use crate::signing::KeyStore;
use crate::store::{
    sha256_hex, AssetKind, BuildRecord, BuildStatus, BuildUpdate, ReleaseOptions, Store,
};
use crate::updates::channel::{ChannelRelease, ReleaseChannel};
use crate::updates::{self, Manifest, Platform, SignedRelease, UpdateKey};

pub const SETTINGS_KEY: &str = "build.settings";
/// Every client lives in `clients/<slug>/` of the build repository.
pub const CLIENTS_DIR: &str = "clients";
pub const CONFIG_FILE: &str = "client.json";
pub const PUBLIC_KEY_FILE: &str = "license-public-key.pem";
/// The workflow input (and the tills' update notice) keeps notes short.
pub const MAX_NOTES: usize = 1000;
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

/// The generator's outside world: GitHub, the credential store, the
/// clients' release channels.
pub struct BuildDeps {
    pub github: Arc<dyn GitHub>,
    pub secrets: SecretFactory,
    pub channel: Arc<dyn ReleaseChannel>,
}

pub struct BuildService {
    store: Arc<Store>,
    github: Arc<dyn GitHub>,
    secrets: SecretFactory,
    token: Arc<dyn SecretStore>,
    keys: Arc<KeyStore>,
    update_key: Arc<UpdateKey>,
    channel: Arc<dyn ReleaseChannel>,
    app_version: String,
    downloads: PathBuf,
}

fn service_key_name(client_id: Uuid) -> String {
    format!("cloud-service-key-{client_id}")
}

impl BuildService {
    pub fn new(
        store: Arc<Store>,
        deps: BuildDeps,
        keys: Arc<KeyStore>,
        update_key: Arc<UpdateKey>,
        app_version: String,
        downloads: PathBuf,
    ) -> Self {
        Self {
            store,
            github: deps.github,
            token: (deps.secrets)(GITHUB_TOKEN),
            secrets: deps.secrets,
            keys,
            update_key,
            channel: deps.channel,
            app_version,
            downloads,
        }
    }

    /// Whether this client's cloud service key is on this PC (needed to
    /// publish updates online).
    pub fn has_service_key(&self, client_id: Uuid) -> IpcResult<bool> {
        Ok((self.secrets)(&service_key_name(client_id))
            .get()?
            .is_some())
    }

    /// `None` removes it.
    pub fn set_service_key(&self, client_id: Uuid, key: Option<&str>) -> IpcResult<bool> {
        self.store.client(client_id)?;
        let secret = (self.secrets)(&service_key_name(client_id));
        match key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(key) => {
                if key.len() > 2000 || key.chars().any(char::is_whitespace) {
                    return Err(IpcError::validation(
                        "That does not look like a Supabase service key.",
                    ));
                }
                secret.set(key)?;
            }
            None => secret.clear()?,
        }
        self.has_service_key(client_id)
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
        // Made with the first build: the tills are built to trust it.
        let updater_key = self
            .update_key
            .ensure()?
            .public_key
            .ok_or_else(|| IpcError::internal("the update key has no public key"))?;
        let mut files = BTreeMap::from([
            (CONFIG_FILE.to_owned(), config),
            (PUBLIC_KEY_FILE.to_owned(), public_key.into_bytes()),
            (
                updates::PUBLIC_KEY_FILE.to_owned(),
                format!("{updater_key}\n").into_bytes(),
            ),
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
    pub fn start(
        &self,
        client_id: Uuid,
        release: &ReleaseOptions,
        now: Timestamp,
    ) -> IpcResult<BuildRecord> {
        if release.release_notes.trim().chars().count() > MAX_NOTES {
            return Err(IpcError::validation(format!(
                "Release notes are limited to {MAX_NOTES} characters."
            )));
        }
        let (target, token) = self.ready()?;
        let (slug, files) = self.build_files(client_id)?;
        let config_sha = sha256_hex(&files[CONFIG_FILE]);
        let build =
            self.store
                .create_build(client_id, &config_sha, &self.app_version, release, now)?;
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
            ("version".to_owned(), build.app_version.clone()),
            ("notes".to_owned(), build.release_notes.clone()),
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
                .filter(|a| !a.expired && a.name.contains(&marker))
                // The merged artifact (every platform) over a platform's own.
                .min_by_key(|a| !a.name.ends_with(&marker));
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

    /// Saves the installers under `<Downloads>/POS Factory/<slug>/<version>/`
    /// with a signed `.posupdate` file per platform (for a USB stick), and
    /// publishes them online when the build asks for it and the client has a
    /// cloud. An online failure is reported on the build; the files stay.
    pub fn download(&self, build_id: Uuid, now: Timestamp) -> IpcResult<BuildRecord> {
        let build = self.store.build(build_id)?;
        let Some(artifact_id) = build.artifact_id else {
            return Err(IpcError::validation(
                "This build has no installer to download yet.",
            ));
        };
        let (target, token) = self.ready()?;
        let bytes = self
            .github
            .download_artifact(&token, &target, artifact_id)
            .map_err(github_error)?;
        let installers = updates::installers_in(&bytes)?;
        if installers.is_empty() {
            return Err(IpcError::internal("The build has no installer in it."));
        }
        let client = self.store.client(build.client_id)?;
        let version = build.app_version.clone();
        let dir = self
            .downloads
            .join("POS Factory")
            .join(&build.client_slug)
            .join(&version);
        std::fs::create_dir_all(&dir)
            .map_err(|e| IpcError::internal(format!("create {}: {e}", dir.display())))?;
        for (name, bytes) in &installers {
            updates::write_file(&dir.join(name), bytes)?;
        }

        let mut signed = Vec::new();
        for platform in Platform::ALL {
            let Some((name, bytes)) = installers
                .iter()
                .find(|(n, _)| platform.is_update_installer(n))
            else {
                continue;
            };
            let release = SignedRelease {
                client_id: build.client_id,
                version: version.clone(),
                platform,
                file: name.clone(),
            };
            let signature = self.update_key.sign(bytes, &release, now)?;
            let manifest = Manifest {
                format: updates::FORMAT,
                client_id: build.client_id,
                client_slug: build.client_slug.clone(),
                version: version.clone(),
                target: platform.target(),
                notes: build.release_notes.clone(),
                installer: name.clone(),
                signature: signature.clone(),
                created_at: now,
            };
            let file = format!(
                "{}-{version}-{}.{}",
                build.client_slug,
                platform.short(),
                updates::PACKAGE_EXTENSION
            );
            updates::write_file(&dir.join(file), &updates::package(&manifest, bytes)?)?;
            signed.push((platform, name, bytes, signature));
        }

        let mut update = BuildUpdate {
            download_path: Some(dir.display().to_string()),
            ..BuildUpdate::default()
        };
        if build.publish_update {
            let outcome = match client.config.cloud.endpoint() {
                None => Err("This client has no cloud: take the .posupdate file to its tills.".to_owned()),
                Some((base, _)) => match (self.secrets)(&service_key_name(build.client_id)).get()? {
                    None => Err("Add the client's cloud service key (Builds) to publish online; the update files are saved.".to_owned()),
                    Some(key) => signed.iter().try_for_each(|(platform, name, bytes, signature)| {
                        self.channel.publish(
                            base,
                            &key,
                            &ChannelRelease {
                                client_id: build.client_id,
                                version: &version,
                                target: platform.target(),
                                notes: &build.release_notes,
                                file_name: name,
                                bytes,
                                signature,
                            },
                        )
                    }),
                },
            };
            update.message = Some(match outcome {
                Ok(()) => "Published: the tills install it on their next check.".to_owned(),
                Err(e) => e,
            });
        }
        self.store.update_build(build_id, &update, now)
    }
}

#[cfg(test)]
mod tests;
