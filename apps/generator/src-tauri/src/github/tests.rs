//! `HttpGitHub` against a local fake of the GitHub REST API that keeps a real
//! (in-memory) git object graph, so publishing is checked end to end: blobs,
//! trees with deletions, commits, fast-forward-only ref updates.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use base64::Engine;
use serde_json::{json, Value};

use super::*;

pub const TOKEN: &str = "good-token";

#[derive(Default)]
pub struct FakeRepo {
    pub head: String,
    commits: BTreeMap<String, (String, Vec<String>, String)>, // sha → (tree, parents, message)
    trees: BTreeMap<String, BTreeMap<String, String>>,        // sha → path → blob
    blobs: BTreeMap<String, Vec<u8>>,
    pub dispatches: Vec<Value>,
    pub runs: Vec<Value>,
    pub artifacts: BTreeMap<u64, Vec<Value>>,
    pub artifact_zips: BTreeMap<u64, Vec<u8>>,
    /// Simulates someone else pushing between our commit and ref update.
    pub race_next_update: bool,
    pub auth_on_blob_download: Option<bool>,
    pub requests: Vec<String>,
}

fn hash(value: impl AsRef<[u8]>) -> String {
    crate::store::sha256_hex(value.as_ref())[..40].to_owned()
}

impl FakeRepo {
    fn new() -> Self {
        let mut repo = Self::default();
        let mut files = BTreeMap::new();
        let readme = repo.put_blob(b"# repo".to_vec());
        files.insert("README.md".to_owned(), readme);
        let tree = repo.put_tree(files);
        repo.head = repo.put_commit(tree, vec![], "initial");
        repo
    }

    fn put_blob(&mut self, bytes: Vec<u8>) -> String {
        let sha = hash(&bytes);
        self.blobs.insert(sha.clone(), bytes);
        sha
    }

    fn put_tree(&mut self, files: BTreeMap<String, String>) -> String {
        let sha = hash(serde_json::to_vec(&files).expect("json"));
        self.trees.insert(sha.clone(), files);
        sha
    }

    fn put_commit(&mut self, tree: String, parents: Vec<String>, message: &str) -> String {
        let sha = hash(format!("{tree}{parents:?}{message}{}", self.commits.len()));
        self.commits
            .insert(sha.clone(), (tree, parents, message.to_owned()));
        sha
    }

    fn head_tree(&self) -> &BTreeMap<String, String> {
        let tree = &self.commits[&self.head].0;
        &self.trees[tree]
    }

    /// Files at HEAD (path → bytes).
    pub fn files(&self) -> BTreeMap<String, Vec<u8>> {
        self.head_tree()
            .iter()
            .map(|(p, b)| (p.clone(), self.blobs[b].clone()))
            .collect()
    }

    pub fn commit_count(&self) -> usize {
        let mut n = 0;
        let mut at = Some(self.head.clone());
        while let Some(sha) = at {
            n += 1;
            at = self.commits[&sha].1.first().cloned();
        }
        n
    }

    pub fn head_message(&self) -> String {
        self.commits[&self.head].2.clone()
    }

    fn handle(
        &mut self,
        method: &str,
        path: &str,
        auth: Option<&str>,
        body: &Value,
        port: u16,
    ) -> (u16, Vec<(String, String)>, Value) {
        self.requests.push(format!("{method} {path}"));
        if let Some(id) = path.strip_prefix("/blob/") {
            self.auth_on_blob_download = Some(auth.is_some());
            let id: u64 = id.parse().expect("id");
            return (
                200,
                vec![],
                Value::String(
                    base64::engine::general_purpose::STANDARD.encode(&self.artifact_zips[&id]),
                ),
            );
        }
        if auth != Some(&format!("Bearer {TOKEN}")) {
            return (401, vec![], json!({ "message": "Bad credentials" }));
        }
        let Some(rest) = path
            .strip_prefix("/repos/acme/pos/")
            .or_else(|| (path == "/repos/acme/pos").then_some(""))
        else {
            return (404, vec![], json!({ "message": "Not Found" }));
        };
        let (route, query) = rest.split_once('?').unwrap_or((rest, ""));
        match (method, route) {
            ("GET", "") => (
                200,
                vec![],
                json!({ "default_branch": "main", "permissions": { "push": true } }),
            ),
            ("GET", "branches/main") | ("GET", "actions/workflows/build-client.yml") => {
                (200, vec![], json!({}))
            }
            ("GET", "git/ref/heads/main") => {
                (200, vec![], json!({ "object": { "sha": self.head } }))
            }
            ("GET", r) if r.starts_with("git/commits/") => {
                let sha = &r["git/commits/".len()..];
                match self.commits.get(sha) {
                    Some((tree, _, _)) => (200, vec![], json!({ "tree": { "sha": tree } })),
                    None => (404, vec![], json!({})),
                }
            }
            ("GET", r) if r.starts_with("contents/") => {
                assert_eq!(query, "ref=main");
                let dir = format!("{}/", &r["contents/".len()..]);
                let entries: Vec<Value> = self
                    .head_tree()
                    .keys()
                    .filter_map(|p| p.strip_prefix(&dir))
                    .filter(|name| !name.contains('/'))
                    .map(|name| json!({ "name": name, "type": "file" }))
                    .collect();
                if entries.is_empty() {
                    (404, vec![], json!({ "message": "Not Found" }))
                } else {
                    (200, vec![], Value::Array(entries))
                }
            }
            ("POST", "git/blobs") => {
                assert_eq!(body["encoding"], "base64");
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(body["content"].as_str().expect("content"))
                    .expect("base64");
                (201, vec![], json!({ "sha": self.put_blob(bytes) }))
            }
            ("POST", "git/trees") => {
                let base = body["base_tree"].as_str().expect("base_tree");
                let mut files = self.trees[base].clone();
                for entry in body["tree"].as_array().expect("tree") {
                    let path = entry["path"].as_str().expect("path").to_owned();
                    match entry["sha"].as_str() {
                        Some(sha) => {
                            files.insert(path, sha.to_owned());
                        }
                        None => {
                            assert!(
                                files.remove(&path).is_some(),
                                "deleting a missing path is a GitHub error"
                            );
                        }
                    }
                }
                (201, vec![], json!({ "sha": self.put_tree(files) }))
            }
            ("POST", "git/commits") => {
                let parents = body["parents"]
                    .as_array()
                    .expect("parents")
                    .iter()
                    .map(|p| p.as_str().expect("sha").to_owned())
                    .collect();
                let sha = self.put_commit(
                    body["tree"].as_str().expect("tree").to_owned(),
                    parents,
                    body["message"].as_str().expect("message"),
                );
                (201, vec![], json!({ "sha": sha }))
            }
            ("PATCH", "git/refs/heads/main") => {
                assert_eq!(body["force"], false);
                if self.race_next_update {
                    self.race_next_update = false;
                    let tree = self.commits[&self.head].0.clone();
                    let head = self.head.clone();
                    self.head = self.put_commit(tree, vec![head], "someone else");
                }
                let sha = body["sha"].as_str().expect("sha").to_owned();
                if self.commits[&sha].1.first() != Some(&self.head) {
                    return (
                        422,
                        vec![],
                        json!({ "message": "Update is not a fast forward" }),
                    );
                }
                self.head = sha;
                (200, vec![], json!({}))
            }
            ("POST", "actions/workflows/build-client.yml/dispatches") => {
                assert_eq!(body["ref"], "main");
                self.dispatches.push(body["inputs"].clone());
                let id = 1000 + self.runs.len() as u64;
                self.runs.insert(0, json!({
                    "id": id,
                    "status": "queued",
                    "conclusion": null,
                    "html_url": format!("https://github.com/acme/pos/actions/runs/{id}"),
                    "display_title": format!("Build {} · {}", body["inputs"]["client"].as_str().expect("client"), body["inputs"]["build_id"].as_str().expect("id")),
                }));
                (204, vec![], Value::Null)
            }
            ("GET", "actions/workflows/build-client.yml/runs") => {
                assert!(query.contains("event=workflow_dispatch"));
                (200, vec![], json!({ "workflow_runs": self.runs }))
            }
            ("GET", r) if r.starts_with("actions/runs/") && r.ends_with("/artifacts") => {
                let id: u64 = r["actions/runs/".len()..r.len() - "/artifacts".len()]
                    .parse()
                    .expect("id");
                (
                    200,
                    vec![],
                    json!({ "artifacts": self.artifacts.get(&id).cloned().unwrap_or_default() }),
                )
            }
            ("GET", r) if r.starts_with("actions/artifacts/") && r.ends_with("/zip") => {
                let id = &r["actions/artifacts/".len()..r.len() - "/zip".len()];
                (
                    302,
                    vec![(
                        "Location".into(),
                        format!("http://127.0.0.1:{port}/blob/{id}"),
                    )],
                    Value::Null,
                )
            }
            _ => (
                404,
                vec![],
                json!({ "message": format!("no route {method} {route}") }),
            ),
        }
    }
}

pub struct FakeGitHubServer {
    pub repo: Arc<Mutex<FakeRepo>>,
    pub target: RepoTarget,
}

fn serve(stream: TcpStream, repo: &Arc<Mutex<FakeRepo>>, port: u16) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.is_empty() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let (mut length, mut auth) = (0usize, None);
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).expect("header");
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            match name.to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().expect("length"),
                "authorization" => auth = Some(value.trim().to_owned()),
                _ => {}
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).expect("body");
    let body: Value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).expect("json body")
    };
    let (status, headers, reply) =
        repo.lock()
            .expect("repo")
            .handle(&method, &path, auth.as_deref(), &body, port);
    let bytes = match (&reply, path.starts_with("/blob/")) {
        (Value::String(b64), true) => base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("zip"),
        (Value::Null, _) => vec![],
        (other, _) => serde_json::to_vec(other).expect("json"),
    };
    let mut out = stream;
    let mut head = format!(
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
        bytes.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = out.write_all(head.as_bytes());
    let _ = out.write_all(&bytes);
}

impl FakeGitHubServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let repo = Arc::new(Mutex::new(FakeRepo::new()));
        let shared = Arc::clone(&repo);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                serve(stream, &shared, port);
            }
        });
        Self {
            repo,
            target: RepoTarget {
                api_base_url: format!("http://127.0.0.1:{port}"),
                owner: "acme".into(),
                repo: "pos".into(),
                branch: "main".into(),
                workflow_file: "build-client.yml".into(),
            },
        }
    }
}

fn files(pairs: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.to_vec()))
        .collect()
}

#[test]
fn check_reports_the_repository() {
    let server = FakeGitHubServer::start();
    let gh = HttpGitHub::new();
    let check = gh.check(TOKEN, &server.target).expect("check");
    assert_eq!(
        check,
        RepoCheck {
            default_branch: "main".into(),
            can_push: true,
            branch_found: true,
            workflow_found: true
        }
    );
    assert!(matches!(
        gh.check("wrong", &server.target),
        Err(GitHubError::Unauthorized(_))
    ));
}

#[test]
fn publishing_is_one_commit_that_replaces_the_folder() {
    let server = FakeGitHubServer::start();
    let gh = HttpGitHub::new();
    let first = gh
        .publish(
            TOKEN,
            &server.target,
            "clients/acme",
            &files(&[("client.json", b"{}"), ("receipt-logo.png", b"png")]),
            "Build acme [skip ci]",
        )
        .expect("publish");
    {
        let repo = server.repo.lock().expect("repo");
        assert_eq!(repo.head, first);
        assert_eq!(repo.commit_count(), 2, "one commit for all files");
        assert_eq!(repo.head_message(), "Build acme [skip ci]");
        assert_eq!(repo.files()["clients/acme/receipt-logo.png"], b"png");
        assert!(
            repo.files().contains_key("README.md"),
            "rest of the repo untouched"
        );
    }
    // Logo removed, config changed: the stale file is deleted in the same commit.
    gh.publish(
        TOKEN,
        &server.target,
        "clients/acme",
        &files(&[("client.json", b"{\"v\":2}")]),
        "Build acme 2",
    )
    .expect("republish");
    let repo = server.repo.lock().expect("repo");
    assert!(!repo.files().contains_key("clients/acme/receipt-logo.png"));
    assert_eq!(repo.files()["clients/acme/client.json"], b"{\"v\":2}");
    assert_eq!(repo.commit_count(), 3);
    drop(repo);
    // Same content again: nothing to commit.
    let head = gh
        .publish(
            TOKEN,
            &server.target,
            "clients/acme",
            &files(&[("client.json", b"{\"v\":2}")]),
            "Build acme 3",
        )
        .expect("no-op");
    assert_eq!(server.repo.lock().expect("repo").commit_count(), 3);
    assert_eq!(server.repo.lock().expect("repo").head, head);
}

#[test]
fn a_concurrent_push_is_retried_not_overwritten() {
    let server = FakeGitHubServer::start();
    server.repo.lock().expect("repo").race_next_update = true;
    HttpGitHub::new()
        .publish(
            TOKEN,
            &server.target,
            "clients/acme",
            &files(&[("client.json", b"{}")]),
            "Build",
        )
        .expect("publish after retry");
    let repo = server.repo.lock().expect("repo");
    assert_eq!(repo.commit_count(), 3, "initial, someone else, ours on top");
    assert!(repo.files().contains_key("clients/acme/client.json"));
}

#[test]
fn dispatch_find_run_and_download_the_installer() {
    let server = FakeGitHubServer::start();
    let gh = HttpGitHub::new();
    let inputs: BTreeMap<String, String> = [
        ("client".to_owned(), "acme".to_owned()),
        ("build_id".to_owned(), "b-123".to_owned()),
    ]
    .into();
    gh.dispatch(TOKEN, &server.target, &inputs)
        .expect("dispatch");
    assert!(gh
        .find_run(TOKEN, &server.target, "b-999")
        .expect("find")
        .is_none());
    let run = gh
        .find_run(TOKEN, &server.target, "b-123")
        .expect("find")
        .expect("run");
    assert_eq!(run.status, "queued");

    {
        let mut repo = server.repo.lock().expect("repo");
        repo.artifacts.insert(
            run.id,
            vec![
                json!({ "id": 55, "name": "pos-acme-b-123", "size_in_bytes": 3, "expired": false }),
            ],
        );
        repo.artifact_zips.insert(55, b"PK\x03".to_vec());
    }
    let artifacts = gh
        .artifacts(TOKEN, &server.target, run.id)
        .expect("artifacts");
    assert_eq!(artifacts[0].name, "pos-acme-b-123");
    let zip = gh
        .download_artifact(TOKEN, &server.target, 55)
        .expect("download");
    assert_eq!(zip, b"PK\x03");
    assert_eq!(
        server.repo.lock().expect("repo").auth_on_blob_download,
        Some(false),
        "the token never leaves GitHub's API host"
    );
}
