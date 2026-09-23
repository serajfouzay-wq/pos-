//! The slice of the GitHub REST API the build pipeline needs.
//!
//! A build is: commit the client's folder (`clients/<slug>/`) to the build
//! repository in ONE commit (Git Data API: blobs → tree → commit → ref), then
//! `workflow_dispatch` the `build-client.yml` workflow with the build id, find
//! the run by that id (the workflow's `run-name` carries it), and fetch the
//! installer artifact once it succeeds.

mod http;

use std::collections::BTreeMap;

pub use http::HttpGitHub;
use serde::{Deserialize, Serialize};

/// Where builds happen. Mirrors the repository part of `BuildSettingsSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoTarget {
    pub api_base_url: String,
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub workflow_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoCheck {
    pub default_branch: String,
    pub can_push: bool,
    pub branch_found: bool,
    pub workflow_found: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRun {
    pub id: u64,
    /// `queued`, `in_progress`, `completed`, … as reported by GitHub.
    pub status: String,
    pub conclusion: Option<String>,
    pub html_url: String,
    pub display_title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub id: u64,
    pub name: String,
    pub size_in_bytes: u64,
    pub expired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitHubError {
    /// 401, or 403 without rate limiting: the token is missing, wrong or
    /// lacks a permission.
    #[error("GitHub refused the token: {0}")]
    Unauthorized(String),
    #[error("not found on GitHub: {0}")]
    NotFound(String),
    /// The branch moved while publishing (another build or a push).
    #[error("the branch changed while publishing")]
    Conflict,
    #[error("GitHub is unreachable: {0}")]
    Network(String),
    #[error("GitHub API error {status}: {message}")]
    Api { status: u16, message: String },
}

pub trait GitHub: Send + Sync {
    fn check(&self, token: &str, target: &RepoTarget) -> Result<RepoCheck, GitHubError>;

    /// Makes `dir` contain exactly `files` (paths relative to `dir`) in a
    /// single commit on `target.branch`; returns the resulting head commit
    /// (unchanged when there was nothing to commit).
    fn publish(
        &self,
        token: &str,
        target: &RepoTarget,
        dir: &str,
        files: &BTreeMap<String, Vec<u8>>,
        message: &str,
    ) -> Result<String, GitHubError>;

    fn dispatch(
        &self,
        token: &str,
        target: &RepoTarget,
        inputs: &BTreeMap<String, String>,
    ) -> Result<(), GitHubError>;

    /// The most recent dispatched run whose title contains `marker`.
    fn find_run(
        &self,
        token: &str,
        target: &RepoTarget,
        marker: &str,
    ) -> Result<Option<WorkflowRun>, GitHubError>;

    fn artifacts(
        &self,
        token: &str,
        target: &RepoTarget,
        run_id: u64,
    ) -> Result<Vec<Artifact>, GitHubError>;

    fn download_artifact(
        &self,
        token: &str,
        target: &RepoTarget,
        artifact_id: u64,
    ) -> Result<Vec<u8>, GitHubError>;
}

#[cfg(test)]
pub(crate) mod tests;
