//! What the live agentd tests share: the test homeserver and its admin
//! calls, a recording client, the local model stub, and a bare drive.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keeper_core::agents::matrix::AgentClient;
use matrix_sdk::ruma::events::AnySyncTimelineEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::OwnedUserId;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// The homeserver
// ---------------------------------------------------------------------------

pub struct Smoke {
    pub homeserver: String,
    pub server_name: String,
    pub secrets: HashMap<String, String>,
    pub http: reqwest::Client,
}

impl Smoke {
    pub fn from_env() -> Smoke {
        let homeserver = std::env::var("KEEPER_AGENTS_SMOKE_HOMESERVER")
            .expect("KEEPER_AGENTS_SMOKE_HOMESERVER names the test homeserver");
        let path = std::env::var("KEEPER_AGENTS_SMOKE_SECRETS")
            .expect("KEEPER_AGENTS_SMOKE_SECRETS names the secrets file");
        let text = std::fs::read_to_string(path).expect("the secrets file is readable");
        let secrets: HashMap<String, String> = text
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
            .collect();
        Smoke {
            homeserver: homeserver.trim_end_matches('/').to_owned(),
            server_name: secrets.get("SERVER_NAME").cloned().expect("SERVER_NAME"),
            secrets,
            http: reqwest::Client::new(),
        }
    }

    pub fn user(&self, localpart: &str) -> OwnedUserId {
        OwnedUserId::try_from(format!("@{localpart}:{}", self.server_name)).expect("user")
    }

    pub fn secret(&self, key: &str) -> &str {
        self.secrets
            .get(key)
            .map(String::as_str)
            .unwrap_or_else(|| panic!("{key}"))
    }

    pub async fn admin(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Value {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.homeserver))
            .bearer_auth(self.secret("ADMIN_TOKEN"));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.expect("admin call");
        let status = response.status();
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        assert!(status.is_success(), "admin {path}: {status} {value}");
        value
    }

    pub async fn clear_devices(&self, user: &OwnedUserId) {
        let listed = self
            .admin(
                reqwest::Method::GET,
                &format!("/_synapse/admin/v2/users/{user}/devices"),
                None,
            )
            .await;
        let devices: Vec<Value> = listed["devices"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|d| d["device_id"].clone())
            .collect();
        if !devices.is_empty() {
            self.admin(
                reqwest::Method::POST,
                &format!("/_synapse/admin/v2/users/{user}/delete_devices"),
                Some(json!({ "devices": devices })),
            )
            .await;
        }
    }

    pub async fn set_ratelimit(&self, user: &OwnedUserId, per_second: u32, burst: u32) {
        self.admin(
            reqwest::Method::POST,
            &format!("/_synapse/admin/v1/users/{user}/override_ratelimit"),
            Some(json!({ "messages_per_second": per_second, "burst_count": burst })),
        )
        .await;
    }

    pub async fn client(&self, dir: &Path, localpart: &str, password: &str) -> AgentClient {
        let client = AgentClient::open(&self.homeserver, dir, "live-passphrase")
            .await
            .expect("client");
        client
            .login(localpart, password, None, "live harness")
            .await
            .expect("login");
        client.sync_once().await.expect("first sync");
        client
    }
}

/// Every decrypted timeline event a client is handed, with the moment it
/// arrived.
pub fn record(client: &AgentClient) -> Arc<Mutex<Vec<(Instant, Value)>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    client
        .client()
        .add_event_handler(move |event: Raw<AnySyncTimelineEvent>| {
            let sink = Arc::clone(&sink);
            async move {
                if let Ok(value) = event.deserialize_as::<Value>() {
                    sink.lock().expect("lock").push((Instant::now(), value));
                }
            }
        });
    seen
}

pub fn syncing(client: &AgentClient) -> tokio::task::JoinHandle<()> {
    let client = client.client().clone();
    tokio::spawn(async move {
        let _ = client
            .sync(keeper_core::agents::matrix::sync_settings())
            .await;
    })
}

/// A fresh device makes the person's cross-signing identity and signs
/// itself with it, replacing the one a previous run left. The account keeps
/// an identity between runs, so the server asks for the password.
pub async fn bootstrap(client: &AgentClient, user: &OwnedUserId, password: &str) {
    use matrix_sdk::ruma::api::client::uiaa;
    let encryption = client.client().encryption();
    if let Err(error) = encryption.bootstrap_cross_signing(None).await {
        let Some(challenge) = error.as_uiaa_response() else {
            panic!("cross-signing: {error}");
        };
        let mut auth = uiaa::Password::new(
            uiaa::UserIdentifier::Matrix(uiaa::MatrixUserIdentifier::new(user.to_string())),
            password.to_owned(),
        );
        auth.session = challenge.session.clone();
        encryption
            .bootstrap_cross_signing(Some(uiaa::AuthData::Password(auth)))
            .await
            .expect("cross-signing with the password");
    }
}

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// The crate's local provider: every chat request gets `answer`, streamed in
/// `pieces` over `over`.
pub struct Stub {
    pub url: String,
    pub requests: Arc<Mutex<usize>>,
}

pub fn stub(answer: &'static str, pieces: usize, over: Duration) -> Stub {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let requests = Arc::new(Mutex::new(0));
    let counted = Arc::clone(&requests);
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else { continue };
            let counted = Arc::clone(&counted);
            std::thread::spawn(move || {
                let mut reader = BufReader::new(socket.try_clone().expect("clone"));
                let mut first = String::new();
                let _ = reader.read_line(&mut first);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                if !first.contains("/chat/completions") {
                    let _ = write!(
                        socket,
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    return;
                }
                *counted.lock().expect("lock") += 1;
                let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n");
                let size = answer.len().div_ceil(pieces);
                let mut start = 0;
                while start < answer.len() {
                    let mut end = (start + size).min(answer.len());
                    while !answer.is_char_boundary(end) {
                        end += 1;
                    }
                    let delta = json!({"model":"stub","choices":[{"index":0,"delta":{"content":&answer[start..end]},"finish_reason":null}]});
                    let _ = write!(socket, "data: {delta}\n\n");
                    let _ = socket.flush();
                    std::thread::sleep(over / pieces as u32);
                    start = end;
                }
                let done = json!({"model":"stub","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
                let _ = write!(socket, "data: {done}\n\ndata: [DONE]\n\n");
            });
        }
    });
    Stub { url, requests }
}

/// A model that answers its first chat request with one call of the tool
/// `name` with `args`, and every later one with `answer`.
pub fn tool_then(name: &'static str, args: Value, answer: &'static str) -> Stub {
    calls_at(vec![(1, name, args)], answer)
}

/// A model that answers its `n`th chat request (from 1) with one call of
/// the tool each `(n, name, args)` of `calls` names, and every other one
/// with `answer`. `@ASK@` in a call's arguments is the id of the last
/// question the request carries ("Question <id>: …").
pub fn calls_at(calls: Vec<(usize, &'static str, Value)>, answer: &'static str) -> Stub {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let requests = Arc::new(Mutex::new(0));
    let counted = Arc::clone(&requests);
    let calls = Arc::new(calls);
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else { continue };
            let counted = Arc::clone(&counted);
            let calls = Arc::clone(&calls);
            std::thread::spawn(move || {
                let mut reader = BufReader::new(socket.try_clone().expect("clone"));
                let mut first = String::new();
                let _ = reader.read_line(&mut first);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                if !first.contains("/chat/completions") {
                    let _ = write!(
                        socket,
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    return;
                }
                let n = {
                    let mut count = counted.lock().expect("lock");
                    *count += 1;
                    *count
                };
                let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n");
                let frame = match calls.iter().find(|(at, ..)| *at == n) {
                    Some((_, name, args)) => {
                        let request = String::from_utf8_lossy(&body);
                        let asked = request.rfind("Question ").map(|at| {
                            let from = at + "Question ".len();
                            request[from..(from + 26).min(request.len())].to_owned()
                        });
                        let args = args
                            .to_string()
                            .replace("@ASK@", asked.as_deref().unwrap_or(""));
                        json!({"model":"stub","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":format!("call_{n}"),"type":"function","function":{"name":name,"arguments":args}}]},"finish_reason":"tool_calls"}]})
                    }
                    None => {
                        json!({"model":"stub","choices":[{"index":0,"delta":{"content":answer},"finish_reason":"stop"}]})
                    }
                };
                let _ = write!(socket, "data: {frame}\n\ndata: [DONE]\n\n");
            });
        }
    });
    Stub { url, requests }
}

// ---------------------------------------------------------------------------
// The drive and the host
// ---------------------------------------------------------------------------

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "seed")
        .env("GIT_AUTHOR_EMAIL", "seed@example.invalid")
        .env("GIT_COMMITTER_NAME", "seed")
        .env("GIT_COMMITTER_EMAIL", "seed@example.invalid")
        .args(args)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn bare_drive(root: &Path, files: &[(String, String)]) -> PathBuf {
    let bare = root.join("smoke.git");
    std::fs::create_dir_all(&bare).expect("bare");
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    let seed = root.join("seed");
    std::fs::create_dir_all(&seed).expect("seed");
    git(&seed, &["init", "-q", "-b", "main"]);
    for (rel, text) in files {
        let path = seed.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "-q", "-m", "seed"]);
    git(&seed, &["push", "-q", &bare.to_string_lossy(), "main"]);
    bare
}
