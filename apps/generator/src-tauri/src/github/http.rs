//! `GitHub` over HTTPS (ureq). Works against github.com or GitHub Enterprise
//! Server (`api_base_url`).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use base64::Engine;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use ureq::tls::{RootCerts, TlsConfig};
use ureq::{Agent, Proxy};

use super::{Artifact, GitHub, GitHubError, RepoCheck, RepoTarget, WorkflowRun};

const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
const PUBLISH_ATTEMPTS: usize = 3;

pub struct HttpGitHub {
    agent: Agent,
}

impl Default for HttpGitHub {
    fn default() -> Self {
        Self::new()
    }
}

struct Reply {
    status: u16,
    body: Vec<u8>,
    rate_limited: bool,
}

impl HttpGitHub {
    pub fn new() -> Self {
        let agent: Agent = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .proxy(Proxy::try_from_env())
            .http_status_as_error(false)
            .user_agent(concat!("pos-factory-generator/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self { agent }
    }

    fn url(target: &RepoTarget, path: &str) -> String {
        format!(
            "{}/repos/{}/{}{path}",
            target.api_base_url.trim_end_matches('/'),
            target.owner,
            target.repo
        )
    }

    fn send(
        &self,
        method: &str,
        url: &str,
        token: &str,
        body: Option<&Value>,
        limit: u64,
    ) -> Result<Reply, GitHubError> {
        let auth = format!("Bearer {token}");
        let result = match (method, body) {
            ("GET", _) => self
                .agent
                .get(url)
                .header("Authorization", &auth)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .call(),
            (_, body) => {
                let request = match method {
                    "PATCH" => self.agent.patch(url),
                    _ => self.agent.post(url),
                };
                request
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", "2022-11-28")
                    .send_json(body.unwrap_or(&Value::Null))
            }
        };
        let mut response = result.map_err(|e| GitHubError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        let rate_limited = response
            .headers()
            .get("x-ratelimit-remaining")
            .is_some_and(|v| v.as_bytes() == b"0");
        let body = response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|e| GitHubError::Network(e.to_string()))?;
        Ok(Reply {
            status,
            body,
            rate_limited,
        })
    }

    fn json<T: DeserializeOwned>(
        &self,
        method: &str,
        url: &str,
        token: &str,
        body: Option<&Value>,
    ) -> Result<T, GitHubError> {
        let reply = self.send(method, url, token, body, MAX_JSON_BYTES)?;
        check(&reply, url)?;
        serde_json::from_slice(&reply.body).map_err(|e| GitHubError::Api {
            status: reply.status,
            message: format!("unexpected response from {url}: {e}"),
        })
    }

    fn head_commit(&self, token: &str, target: &RepoTarget) -> Result<String, GitHubError> {
        #[derive(Deserialize)]
        struct Ref {
            object: Sha,
        }
        let r: Ref = self.json(
            "GET",
            &Self::url(target, &format!("/git/ref/heads/{}", target.branch)),
            token,
            None,
        )?;
        Ok(r.object.sha)
    }

    fn existing_files(
        &self,
        token: &str,
        target: &RepoTarget,
        dir: &str,
    ) -> Result<BTreeSet<String>, GitHubError> {
        #[derive(Deserialize)]
        struct Entry {
            name: String,
            #[serde(rename = "type")]
            kind: String,
        }
        let url = Self::url(target, &format!("/contents/{dir}?ref={}", target.branch));
        match self.json::<Vec<Entry>>("GET", &url, token, None) {
            Ok(entries) => Ok(entries
                .into_iter()
                .filter(|e| e.kind == "file")
                .map(|e| e.name)
                .collect()),
            Err(GitHubError::NotFound(_)) => Ok(BTreeSet::new()),
            Err(e) => Err(e),
        }
    }

    fn publish_once(
        &self,
        token: &str,
        target: &RepoTarget,
        dir: &str,
        files: &BTreeMap<String, Vec<u8>>,
        message: &str,
    ) -> Result<String, GitHubError> {
        #[derive(Deserialize)]
        struct Commit {
            tree: Sha,
        }
        let head = self.head_commit(token, target)?;
        let base: Commit = self.json(
            "GET",
            &Self::url(target, &format!("/git/commits/{head}")),
            token,
            None,
        )?;
        let existing = self.existing_files(token, target, dir)?;

        let mut entries = Vec::new();
        for (name, bytes) in files {
            let blob: Sha = self.json(
                "POST",
                &Self::url(target, "/git/blobs"),
                token,
                Some(&json!({
                    "content": base64::engine::general_purpose::STANDARD.encode(bytes),
                    "encoding": "base64",
                })),
            )?;
            entries.push(json!({
                "path": format!("{dir}/{name}"), "mode": "100644", "type": "blob", "sha": blob.sha,
            }));
        }
        for stale in existing.iter().filter(|name| !files.contains_key(*name)) {
            entries.push(json!({
                "path": format!("{dir}/{stale}"), "mode": "100644", "type": "blob", "sha": Value::Null,
            }));
        }
        let tree: Sha = self.json(
            "POST",
            &Self::url(target, "/git/trees"),
            token,
            Some(&json!({ "base_tree": base.tree.sha, "tree": entries })),
        )?;
        if tree.sha == base.tree.sha {
            return Ok(head); // already published
        }
        let commit: Sha = self.json(
            "POST",
            &Self::url(target, "/git/commits"),
            token,
            Some(&json!({ "message": message, "tree": tree.sha, "parents": [head] })),
        )?;
        let url = Self::url(target, &format!("/git/refs/heads/{}", target.branch));
        let reply = self.send(
            "PATCH",
            &url,
            token,
            Some(&json!({ "sha": commit.sha, "force": false })),
            MAX_JSON_BYTES,
        )?;
        if reply.status == 422 {
            return Err(GitHubError::Conflict);
        }
        check(&reply, &url)?;
        Ok(commit.sha)
    }
}

#[derive(Deserialize)]
struct Sha {
    sha: String,
}

fn api_message(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(body).chars().take(200).collect())
}

fn check(reply: &Reply, url: &str) -> Result<(), GitHubError> {
    match reply.status {
        200..=299 => Ok(()),
        401 => Err(GitHubError::Unauthorized(api_message(&reply.body))),
        403 if !reply.rate_limited => Err(GitHubError::Unauthorized(api_message(&reply.body))),
        404 => Err(GitHubError::NotFound(url.to_owned())),
        status => Err(GitHubError::Api {
            status,
            message: api_message(&reply.body),
        }),
    }
}

impl GitHub for HttpGitHub {
    fn check(&self, token: &str, target: &RepoTarget) -> Result<RepoCheck, GitHubError> {
        #[derive(Deserialize)]
        struct Repo {
            default_branch: String,
            #[serde(default)]
            permissions: Option<Permissions>,
        }
        #[derive(Deserialize)]
        struct Permissions {
            #[serde(default)]
            push: bool,
        }
        let repo: Repo = self.json("GET", &Self::url(target, ""), token, None)?;
        let exists =
            |path: &str| match self.json::<Value>("GET", &Self::url(target, path), token, None) {
                Ok(_) => Ok(true),
                Err(GitHubError::NotFound(_)) => Ok(false),
                Err(e) => Err(e),
            };
        Ok(RepoCheck {
            can_push: repo.permissions.is_some_and(|p| p.push),
            branch_found: exists(&format!("/branches/{}", target.branch))?,
            workflow_found: exists(&format!("/actions/workflows/{}", target.workflow_file))?,
            default_branch: repo.default_branch,
        })
    }

    fn publish(
        &self,
        token: &str,
        target: &RepoTarget,
        dir: &str,
        files: &BTreeMap<String, Vec<u8>>,
        message: &str,
    ) -> Result<String, GitHubError> {
        let mut last = GitHubError::Conflict;
        for _ in 0..PUBLISH_ATTEMPTS {
            match self.publish_once(token, target, dir, files, message) {
                Err(GitHubError::Conflict) => last = GitHubError::Conflict,
                other => return other,
            }
        }
        Err(last)
    }

    fn dispatch(
        &self,
        token: &str,
        target: &RepoTarget,
        inputs: &BTreeMap<String, String>,
    ) -> Result<(), GitHubError> {
        let url = Self::url(
            target,
            &format!("/actions/workflows/{}/dispatches", target.workflow_file),
        );
        let reply = self.send(
            "POST",
            &url,
            token,
            Some(&json!({ "ref": target.branch, "inputs": inputs })),
            MAX_JSON_BYTES,
        )?;
        check(&reply, &url)
    }

    fn find_run(
        &self,
        token: &str,
        target: &RepoTarget,
        marker: &str,
    ) -> Result<Option<WorkflowRun>, GitHubError> {
        #[derive(Deserialize)]
        struct Runs {
            workflow_runs: Vec<Run>,
        }
        #[derive(Deserialize)]
        struct Run {
            id: u64,
            status: String,
            conclusion: Option<String>,
            html_url: String,
            #[serde(default)]
            display_title: String,
        }
        let runs: Runs = self.json(
            "GET",
            &Self::url(
                target,
                &format!(
                    "/actions/workflows/{}/runs?event=workflow_dispatch&per_page=50",
                    target.workflow_file
                ),
            ),
            token,
            None,
        )?;
        Ok(runs
            .workflow_runs
            .into_iter()
            .find(|r| r.display_title.contains(marker))
            .map(|r| WorkflowRun {
                id: r.id,
                status: r.status,
                conclusion: r.conclusion,
                html_url: r.html_url,
                display_title: r.display_title,
            }))
    }

    fn artifacts(
        &self,
        token: &str,
        target: &RepoTarget,
        run_id: u64,
    ) -> Result<Vec<Artifact>, GitHubError> {
        #[derive(Deserialize)]
        struct List {
            artifacts: Vec<Item>,
        }
        #[derive(Deserialize)]
        struct Item {
            id: u64,
            name: String,
            size_in_bytes: u64,
            expired: bool,
        }
        let list: List = self.json(
            "GET",
            &Self::url(target, &format!("/actions/runs/{run_id}/artifacts")),
            token,
            None,
        )?;
        Ok(list
            .artifacts
            .into_iter()
            .map(|a| Artifact {
                id: a.id,
                name: a.name,
                size_in_bytes: a.size_in_bytes,
                expired: a.expired,
            })
            .collect())
    }

    fn download_artifact(
        &self,
        token: &str,
        target: &RepoTarget,
        artifact_id: u64,
    ) -> Result<Vec<u8>, GitHubError> {
        // GitHub answers with a redirect to short-lived blob storage; ureq
        // follows it and does not forward the Authorization header.
        let url = Self::url(target, &format!("/actions/artifacts/{artifact_id}/zip"));
        let reply = self.send("GET", &url, token, None, MAX_ARTIFACT_BYTES)?;
        check(&reply, &url)?;
        Ok(reply.body)
    }
}
