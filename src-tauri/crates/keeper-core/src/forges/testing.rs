//! A loopback HTTP fake and an in-memory platform for the forge tests, in the
//! shape of `oidc`'s: one thread per connection, `Connection: close`, and
//! every request recorded for the assertions.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::error::CoreError;
use crate::platform::Platform;
use crate::vm::NotifyTarget;

/// One request as the fake saw it. Header names are lowercase.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    /// The path without the query.
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Seen {
    pub fn form(&self) -> HashMap<String, String> {
        url::form_urlencoded::parse(self.body.as_bytes())
            .into_owned()
            .collect()
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Reply {
    pub fn json(status: u16, body: &str) -> Reply {
        Reply {
            status,
            headers: Vec::new(),
            body: body.to_owned(),
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Reply {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

pub struct Fake {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Fake {
    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().expect("seen").clone()
    }
}

/// Serve `handler` on a loopback port until the test binary exits.
pub fn serve(handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake forge");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(handler);
    let fake = Fake {
        base,
        seen: Arc::clone(&seen),
    };
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (seen, handler) = (Arc::clone(&seen), Arc::clone(&handler));
            std::thread::spawn(move || {
                let Some(request) = read_request(&mut stream) else {
                    return;
                };
                seen.lock().expect("seen").push(request.clone());
                let reply = handler(&request);
                write_reply(&mut stream, &reply);
            });
        }
    });
    fake
}

fn read_request(stream: &mut TcpStream) -> Option<Seen> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let headers: HashMap<String, String> = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect();
    let length = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < head_end + length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_owned(), query),
        None => (target.clone(), ""),
    };
    Some(Seen {
        method,
        path,
        query: url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect(),
        headers,
        body: String::from_utf8_lossy(&buf[head_end..]).into_owned(),
    })
}

fn write_reply(stream: &mut TcpStream, reply: &Reply) {
    let mut head = format!(
        "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.body.len()
    );
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(reply.body.as_bytes());
}

/// The client the shell passes: no redirects followed.
pub fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client")
}

/// A keychain in memory and a scratch data directory.
pub struct FakePlatform {
    pub keychain: Mutex<HashMap<String, String>>,
    pub data_dir: PathBuf,
}

impl Default for FakePlatform {
    fn default() -> Self {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let data_dir = std::env::temp_dir().join(format!(
            "keeper-forges-{}-{n}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&data_dir).expect("scratch dir");
        FakePlatform {
            keychain: Mutex::new(HashMap::new()),
            data_dir,
        }
    }
}

impl Drop for FakePlatform {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

impl Platform for FakePlatform {
    fn data_dir(&self) -> Result<PathBuf, CoreError> {
        Ok(self.data_dir.clone())
    }
    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        self.keychain
            .lock()
            .expect("keychain")
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }
    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
        Ok(self.keychain.lock().expect("keychain").get(key).cloned())
    }
    fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
        self.keychain.lock().expect("keychain").remove(key);
        Ok(())
    }
    fn open_url(&self, _: &str) -> Result<(), CoreError> {
        Ok(())
    }
    fn notify(&self, _: &str, _: &str, _: &NotifyTarget) -> Result<(), CoreError> {
        Ok(())
    }
    fn sidecar_path(&self, _: &str) -> Result<PathBuf, CoreError> {
        Err(CoreError::Unsupported("no sidecars in tests".to_owned()))
    }
    fn exclude_from_backup(&self, _: &Path) -> Result<(), CoreError> {
        Ok(())
    }
    fn set_badge_count(&self, _: Option<u32>) -> Result<(), CoreError> {
        Ok(())
    }
}

/// An account whose access token is fresh in `p`'s keychain, so
/// `oidc::access_token` answers `account-token` without the network.
pub fn signed_in_account(
    p: &FakePlatform,
    extra: &str,
) -> crate::org_account::descriptor::AccountDescriptor {
    use crate::org_account::session::{store_session, Binding, StoredSession};
    let d = crate::org_account::descriptor::parse_json(&format!(
        r#"{{ "version": 1, "id": "acme", "name": "Acme",
             "auth": {{ "issuer": "https://id.acme.dev", "client_id": "keeper" }},
             "config": {{ "url": "https://git.acme.dev/people/keeper-config.git" }} {extra} }}"#
    ))
    .expect("descriptor");
    let session = StoredSession {
        binding: Binding::session(&d),
        iss: d.auth.issuer.clone(),
        sub: "sub-1".to_owned(),
        refresh_token: None,
        access_token: "account-token".to_owned(),
        access_expires_ms: Some(chrono::Utc::now().timestamp_millis() + 3_600_000),
        id_token: String::new(),
        login: "tgorka".to_owned(),
        display_name: "tgorka".to_owned(),
        email: None,
        roles: Vec::new(),
    };
    store_session(p, &d.id, &session).expect("seed session");
    d
}
