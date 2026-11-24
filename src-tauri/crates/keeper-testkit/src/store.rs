//! Loopback stand-in server and object store for file transfer tests (story 115.3).

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_TOKEN: &str = "standin-token";
const URL_TTL_MS: u64 = 3_600_000;
const HOLD_TIMEOUT: Duration = Duration::from_secs(30);
/// The store's receive buffer: loopback otherwise buffers megabytes, so a paced
/// store would hide a slow upload inside the kernel (and the client's wait for
/// the answer would grow with it), which a real link does not.
const STORE_RECV_BUFFER: usize = 64 * 1024;

/// Two loopback HTTP/1.1 listeners: a grant server and a presigned object store.
pub struct StandIn {
    database: String,
    server_port: u16,
    store_port: u16,
    token: String,
    shared: Arc<Shared>,
}

/// Gate returned by [`StandIn::hold_next_part`]; pauses one part `PUT` after its body is read.
pub struct Hold {
    inner: Arc<(Mutex<HoldState>, Condvar)>,
}

struct HoldState {
    held: bool,
    released: bool,
}

impl Hold {
    /// Wait up to `timeout` for the held part `PUT` to finish reading its body and park.
    pub fn wait_held(&self, timeout: Duration) -> bool {
        let (lock, cvar) = &*self.inner;
        let guard = lock.lock().expect("hold lock");
        let (guard, _) = cvar
            .wait_timeout_while(guard, timeout, |state| !state.held)
            .expect("hold wait");
        guard.held
    }

    /// Unblock the parked part `PUT` so it stores the part and answers `200`.
    pub fn release(self) {
        self.signal_release();
    }

    fn signal_release(&self) {
        let (lock, cvar) = &*self.inner;
        let mut guard = lock.lock().expect("hold lock");
        guard.released = true;
        cvar.notify_all();
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.signal_release();
    }
}

struct Shared {
    database: String,
    token: String,
    store_port: u16,
    bytes_received: AtomicU64,
    bytes_served: AtomicU64,
    store_gets: AtomicU64,
    state: Mutex<State>,
}

struct State {
    objects: BTreeMap<String, Arc<Vec<u8>>>,
    plans: BTreeMap<String, Plan>,
    mint_seq: u64,
    expired_up_to: u64,
    plan_seq: u64,
    foreign: bool,
    grant_requests: Vec<String>,
    parts_accepted: Vec<u32>,
    cut_next: Option<u64>,
    silence_next: Option<(u64, Duration)>,
    pace: Option<(usize, Duration)>,
    unpaced_upload_tail: u64,
    pending_hold: Option<Arc<(Mutex<HoldState>, Condvar)>>,
}

struct Plan {
    content_id: String,
    size: u64,
    declared: BTreeMap<u32, DeclaredPart>,
    held: BTreeMap<u32, Vec<u8>>,
}

struct DeclaredPart {
    sha256_b64: String,
    size: u64,
}

impl State {
    fn announced_origin(&self, store_port: u16) -> String {
        if self.foreign {
            format!("http://localhost:{store_port}")
        } else {
            format!("http://127.0.0.1:{store_port}")
        }
    }

    fn mint_sig(&mut self) -> u64 {
        self.mint_seq += 1;
        self.mint_seq
    }

    fn next_plan_id(&mut self) -> String {
        self.plan_seq += 1;
        let time_part = (now_ms() & 0xffff_ffff) as u32;
        let seq_part = (self.plan_seq & 0xffff_ffff) as u32;
        format!("{time_part:08x}{seq_part:08x}")
    }
}

impl StandIn {
    /// Bind and start both loopback listeners for `database`.
    pub fn start(database: &str) -> StandIn {
        let server_listener = TcpListener::bind("127.0.0.1:0").expect("bind server");
        let store_listener = bind_store_listener();
        let server_port = server_listener.local_addr().expect("server addr").port();
        let store_port = store_listener.local_addr().expect("store addr").port();
        let token = DEFAULT_TOKEN.to_owned();
        let shared = Arc::new(Shared {
            database: database.to_owned(),
            token: token.clone(),
            store_port,
            bytes_received: AtomicU64::new(0),
            bytes_served: AtomicU64::new(0),
            store_gets: AtomicU64::new(0),
            state: Mutex::new(State {
                objects: BTreeMap::new(),
                plans: BTreeMap::new(),
                mint_seq: 0,
                expired_up_to: 0,
                plan_seq: 0,
                foreign: false,
                grant_requests: Vec::new(),
                parts_accepted: Vec::new(),
                cut_next: None,
                silence_next: None,
                pace: None,
                unpaced_upload_tail: 0,
                pending_hold: None,
            }),
        });

        let server_shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            for stream in server_listener.incoming() {
                let Ok(stream) = stream else { return };
                let shared = Arc::clone(&server_shared);
                std::thread::spawn(move || handle_server_conn(stream, &shared));
            }
        });

        let store_shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            for stream in store_listener.incoming() {
                let Ok(stream) = stream else { return };
                let shared = Arc::clone(&store_shared);
                std::thread::spawn(move || handle_store_conn(stream, &shared));
            }
        });

        StandIn {
            database: database.to_owned(),
            server_port,
            store_port,
            token,
            shared,
        }
    }

    /// Origin of the grant server, e.g. `http://127.0.0.1:<port>`.
    pub fn server_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.server_port)
    }

    /// Database mount URL on the grant server, e.g. `http://127.0.0.1:<port>/<db>`.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/{}", self.server_port, self.database)
    }

    /// Origin of the object store, e.g. `http://127.0.0.1:<port>`.
    pub fn store_origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.store_port)
    }

    /// Bearer token accepted by the grant server.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Place `bytes` in the store at its `sha256:<hex>` content id and return that id.
    pub fn seed(&self, bytes: &[u8]) -> String {
        let id = crate::blob_id(bytes);
        self.shared
            .state
            .lock()
            .expect("state lock")
            .objects
            .insert(id.clone(), Arc::new(bytes.to_vec()));
        id
    }

    /// Read back a stored object by content id.
    pub fn object(&self, content_id: &str) -> Option<Vec<u8>> {
        self.shared
            .state
            .lock()
            .expect("state lock")
            .objects
            .get(content_id)
            .map(|arc| (**arc).clone())
    }

    /// Total `PUT` body bytes read by the store (single and multipart, including cut transfers).
    pub fn store_bytes_received(&self) -> u64 {
        self.shared.bytes_received.load(Ordering::SeqCst)
    }

    /// Total `GET` body bytes written by the store (including cut transfers).
    pub fn store_bytes_served(&self) -> u64 {
        self.shared.bytes_served.load(Ordering::SeqCst)
    }

    /// How many store `GET`s of an object have started (each request counts once, before its body).
    pub fn store_gets(&self) -> u64 {
        self.shared.store_gets.load(Ordering::SeqCst)
    }

    /// Part numbers answered `200`, in order (repeats included).
    pub fn parts_accepted(&self) -> Vec<u32> {
        self.shared
            .state
            .lock()
            .expect("state lock")
            .parts_accepted
            .clone()
    }

    /// Drop the next store connection after reading/writing at most `after_bytes` body bytes.
    pub fn cut_next(&self, after_bytes: u64) {
        self.shared.state.lock().expect("state lock").cut_next = Some(after_bytes);
    }

    /// Pace every store body from now on to `chunk` bytes per `every` (PUT reads, GET
    /// writes), until called again with `None`.
    pub fn throttle(&self, pace: Option<(usize, Duration)>) {
        self.shared.state.lock().expect("state lock").pace = pace;
    }

    /// Read the last `bytes` of every throttled upload body (single or part PUT) at full speed.
    ///
    /// After a client's last write, the bytes its kernel still holds reach the store before the
    /// answer can, so the client's wait for the answer is that drain. How much the kernel holds
    /// is the OS's call (macOS grows a loopback send buffer past what the store's receive window
    /// suggests); a tail at least that large keeps the drain off the paced clock, so a slow
    /// upload's answer wait measures the store, not the kernel. GET bodies are unaffected.
    pub fn unpaced_upload_tail(&self, bytes: u64) {
        self.shared
            .state
            .lock()
            .expect("state lock")
            .unpaced_upload_tail = bytes;
    }

    /// The next store connection (single PUT, part PUT or GET, whichever comes first) moves at most
    /// `after_bytes` body bytes (counted in store_bytes_received/served as today), then neither reads,
    /// writes nor closes for `hold_for`, then shuts the connection down without storing anything.
    pub fn silence_next(&self, after_bytes: u64, hold_for: Duration) {
        self.shared.state.lock().expect("state lock").silence_next = Some((after_bytes, hold_for));
    }

    /// Pause the next part `PUT` after its body is read until [`Hold::release`] is called.
    pub fn hold_next_part(&self) -> Hold {
        let inner = Arc::new((
            Mutex::new(HoldState {
                held: false,
                released: false,
            }),
            Condvar::new(),
        ));
        self.shared.state.lock().expect("state lock").pending_hold = Some(Arc::clone(&inner));
        Hold { inner }
    }

    /// Expire every presigned URL minted so far so it answers `403 AccessDenied`.
    pub fn expire_urls(&self) {
        let mut state = self.shared.state.lock().expect("state lock");
        state.expired_up_to = state.mint_seq;
    }

    /// Toggle announcing `http://localhost:<store port>` instead of `127.0.0.1`.
    pub fn announce_foreign(&self, on: bool) {
        self.shared.state.lock().expect("state lock").foreign = on;
    }

    /// Recorded `"<METHOD> <path>"` of every grant request in order.
    pub fn grant_requests(&self) -> Vec<String> {
        self.shared
            .state
            .lock()
            .expect("state lock")
            .grant_requests
            .clone()
    }

    /// Assemble and place a plan's held parts directly on the store, if complete and valid.
    pub fn complete_plan_on_store(&self, plan_id: &str) {
        let mut state = self.shared.state.lock().expect("state lock");
        let assembled = match state.plans.get(plan_id) {
            Some(plan) => assemble_plan(plan).ok(),
            None => None,
        };
        if let Some((content_id, bytes)) = assembled {
            state.objects.insert(content_id, Arc::new(bytes));
            state.plans.remove(plan_id);
        }
    }
}

struct RequestHead {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    initial_body: Vec<u8>,
}

/// The store's loopback listener, with [`STORE_RECV_BUFFER`] set before `listen` so every
/// accepted connection inherits it and the client's in-flight bytes stay link-sized.
///
/// Linux grows a sender's buffer from its congestion window times the segment size, so
/// loopback's 64 KiB segments would still grow the client's send buffer to megabytes; an
/// Ethernet-sized segment, set here and inherited by accepted sockets, keeps it small.
/// macOS refuses `TCP_MAXSEG` on an unconnected socket (EINVAL), and its loopback send buffer
/// can still hold a large share of a part (CI run 45444300522); a paced test that needs the
/// client's answer wait bounded sets [`StandIn::unpaced_upload_tail`] instead of relying on it.
fn bind_store_listener() -> TcpListener {
    use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    let socket =
        Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).expect("store socket");
    socket
        .set_recv_buffer_size(STORE_RECV_BUFFER)
        .expect("store receive buffer");
    #[cfg(target_os = "linux")]
    socket.set_tcp_mss(1400).expect("store segment size");
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("loopback address");
    socket.bind(&SockAddr::from(addr)).expect("bind store");
    socket.listen(128).expect("listen store");
    socket.into()
}

fn read_head(stream: &mut TcpStream) -> Option<RequestHead> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    let head_end = loop {
        if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return None,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
        }
    };
    let head_str = String::from_utf8_lossy(&raw[..head_end]);
    let mut lines = head_str.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();
    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let initial_body = raw[head_end..].to_vec();
    Some(RequestHead {
        method,
        target,
        headers,
        initial_body,
    })
}

fn read_exact_body(stream: &mut TcpStream, initial: &[u8], len: usize) -> Option<Vec<u8>> {
    let mut body = Vec::with_capacity(len.min(1024 * 1024));
    let take = initial.len().min(len);
    body.extend_from_slice(&initial[..take]);
    let mut buf = [0u8; 8192];
    while body.len() < len {
        let want = (len - body.len()).min(buf.len());
        match stream.read(&mut buf[..want]) {
            Ok(0) | Err(_) => return None,
            Ok(n) => body.extend_from_slice(&buf[..n]),
        }
    }
    Some(body)
}

enum StoreRead {
    Complete(Vec<u8>),
    Cut,
    Silenced,
    Eof,
}

/// A throttled upload body's pace: `pace.0` bytes per `pace.1`, except its last `unpaced_tail`
/// bytes, which are read at full speed ([`StandIn::unpaced_upload_tail`]).
#[derive(Clone, Copy)]
struct UploadPace {
    pace: (usize, Duration),
    unpaced_tail: u64,
}

impl UploadPace {
    fn of(state: &State) -> Option<UploadPace> {
        state.pace.map(|pace| UploadPace {
            pace,
            unpaced_tail: state.unpaced_upload_tail,
        })
    }
}

fn read_store_body(
    stream: &mut TcpStream,
    initial: &[u8],
    expected_len: u64,
    cut_limit: Option<u64>,
    silence: Option<(u64, Duration)>,
    pace: Option<UploadPace>,
    counter: &AtomicU64,
) -> StoreRead {
    let max_read = silence
        .map(|(n, _)| n)
        .or(cut_limit)
        .map_or(expected_len, |n| expected_len.min(n));
    let mut body = Vec::with_capacity((max_read as usize).min(1024 * 1024));
    let take = initial.len().min(max_read as usize);
    body.extend_from_slice(&initial[..take]);
    if take > 0 {
        counter.fetch_add(take as u64, Ordering::SeqCst);
    }
    let paced_until = pace.map_or(expected_len, |p| {
        expected_len.saturating_sub(p.unpaced_tail)
    });
    let pace = pace.map(|p| p.pace);
    let mut buf = [0u8; 16_384];
    let started = std::time::Instant::now();
    while (body.len() as u64) < max_read {
        let remaining = (max_read - body.len() as u64) as usize;
        let want = remaining.min(buf.len());
        match stream.read(&mut buf[..want]) {
            Ok(0) | Err(_) => return StoreRead::Eof,
            Ok(n) => {
                body.extend_from_slice(&buf[..n]);
                counter.fetch_add(n as u64, Ordering::SeqCst);
                keep_pace(pace, started, (body.len() as u64).min(paced_until));
            }
        }
    }
    if let Some((_, hold_for)) = silence {
        std::thread::sleep(hold_for);
        StoreRead::Silenced
    } else if cut_limit.is_some() {
        StoreRead::Cut
    } else {
        StoreRead::Complete(body)
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) {
    let reason = status_reason(status);
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\n",
        body.len()
    );
    for (k, v) in extra_headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("connection: close\r\n\r\n");
    let _ = stream.write_all(head.as_bytes());
    if !body.is_empty() {
        let _ = stream.write_all(body);
    }
    let _ = stream.flush();
}

fn write_json(stream: &mut TcpStream, status: u16, value: &Value) {
    let body = serde_json::to_vec(value).expect("serialize json");
    write_response(stream, status, "application/json", &[], &body);
}

fn write_xml_error(stream: &mut TcpStream, status: u16, code: &str, message: &str) {
    let body = format!("<Error><Code>{code}</Code><Message>{message}</Message></Error>");
    write_response(stream, status, "application/xml", &[], body.as_bytes());
}

fn status_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        416 => "Range Not Satisfiable",
        _ => "Error",
    }
}

fn handle_server_conn(mut stream: TcpStream, shared: &Shared) {
    let Some(req) = read_head(&mut stream) else {
        return;
    };
    let content_len: usize = req
        .headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let Some(body) = read_exact_body(&mut stream, &req.initial_body, content_len) else {
        return;
    };
    let path = req.target.split('?').next().unwrap_or(&req.target);

    let db_prefix = format!("/{}", shared.database);
    let is_grant_path = path.starts_with(&format!("{db_prefix}/"));
    if is_grant_path {
        shared
            .state
            .lock()
            .expect("state lock")
            .grant_requests
            .push(format!("{} {path}", req.method));
    }

    let expected_auth = format!("Bearer {}", shared.token);
    let authed = req
        .headers
        .get("authorization")
        .is_some_and(|v| v == &expected_auth);
    if !authed {
        write_json(
            &mut stream,
            401,
            &json!({"code": "auth.required", "message": "unauthorized"}),
        );
        return;
    }

    if req.method == "GET" && path == "/databases" {
        let origin = shared
            .state
            .lock()
            .expect("state lock")
            .announced_origin(shared.store_port);
        let payload = json!({
            "actorId": "a",
            "clientId": "a.c",
            "databases": [{
                "name": shared.database,
                "mount": db_prefix,
                "kind": "general",
                "display_name": shared.database,
                "store": origin,
            }]
        });
        write_json(&mut stream, 200, &payload);
        return;
    }

    let Some(subpath) = path.strip_prefix(&db_prefix) else {
        write_json(
            &mut stream,
            404,
            &json!({"code": "not_found", "message": "unknown database"}),
        );
        return;
    };

    if req.method == "POST" && subpath == "/files/upload-grant" {
        handle_upload_grant(&mut stream, shared, &body);
        return;
    }
    if req.method == "POST" && subpath == "/files/download-grant" {
        handle_download_grant(&mut stream, shared, &body);
        return;
    }
    if let Some(rest) = subpath.strip_prefix("/files/plans/") {
        if let Some((plan_id, action)) = rest.split_once('/') {
            match (req.method.as_str(), action) {
                ("POST", "part-urls") => {
                    handle_part_urls(&mut stream, shared, plan_id, &body);
                    return;
                }
                ("GET", "parts") => {
                    handle_list_parts(&mut stream, shared, plan_id);
                    return;
                }
                ("POST", "complete") => {
                    handle_complete_plan(&mut stream, shared, plan_id);
                    return;
                }
                _ => {}
            }
        }
    }

    write_json(
        &mut stream,
        404,
        &json!({"code": "not_found", "message": "unknown route"}),
    );
}

fn handle_upload_grant(stream: &mut TcpStream, shared: &Shared, body: &[u8]) {
    let Ok(val) = serde_json::from_slice::<Value>(body) else {
        write_json(
            stream,
            400,
            &json!({"code": "files.invalid_request", "message": "invalid json"}),
        );
        return;
    };
    let Some(content_id) = val.get("contentId").and_then(Value::as_str) else {
        write_json(
            stream,
            400,
            &json!({"code": "files.invalid_request", "message": "missing contentId"}),
        );
        return;
    };
    let Some(hex_digest) = content_id.strip_prefix("sha256:") else {
        write_json(
            stream,
            400,
            &json!({"code": "files.invalid_request", "message": "invalid contentId"}),
        );
        return;
    };
    let size = val.get("size").and_then(Value::as_u64).unwrap_or(0);
    let part_bytes = val.get("partBytes").and_then(Value::as_u64).unwrap_or(0);
    let parts = val
        .get("parts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut state = shared.state.lock().expect("state lock");
    if state.objects.contains_key(content_id) {
        write_json(stream, 200, &json!({"present": true}));
        return;
    }
    let origin = state.announced_origin(shared.store_port);
    let expires_at_ms = now_ms() + URL_TTL_MS;

    if parts.is_empty() {
        let sig = state.mint_sig();
        let url = format!("{origin}/put/{hex_digest}?sig={sig}");
        write_json(
            stream,
            200,
            &json!({"url": url, "expiresAtMs": expires_at_ms}),
        );
        return;
    }

    let plan_id = state.next_plan_id();
    let mut declared = BTreeMap::new();
    let mut signed_parts = Vec::with_capacity(parts.len());
    for entry in &parts {
        let Some(number) = entry
            .get("number")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n >= 1)
        else {
            write_json(
                stream,
                400,
                &json!({"code": "files.invalid_request", "message": "invalid part number"}),
            );
            return;
        };
        let Some(sha256_b64) = entry.get("sha256").and_then(Value::as_str) else {
            write_json(
                stream,
                400,
                &json!({"code": "files.invalid_request", "message": "missing part sha256"}),
            );
            return;
        };
        let offset = u64::from(number - 1).saturating_mul(part_bytes);
        let part_size = if offset >= size {
            0
        } else {
            (size - offset).min(part_bytes)
        };
        declared.insert(
            number,
            DeclaredPart {
                sha256_b64: sha256_b64.to_owned(),
                size: part_size,
            },
        );
        let sig = state.mint_sig();
        let url = format!("{origin}/up/{plan_id}/{number}?sig={sig}");
        signed_parts.push(json!({
            "number": number,
            "url": url,
            "expiresAtMs": expires_at_ms,
        }));
    }

    state.plans.insert(
        plan_id.clone(),
        Plan {
            content_id: content_id.to_owned(),
            size,
            declared,
            held: BTreeMap::new(),
        },
    );

    write_json(
        stream,
        200,
        &json!({"planId": plan_id, "parts": signed_parts}),
    );
}

fn handle_part_urls(stream: &mut TcpStream, shared: &Shared, plan_id: &str, body: &[u8]) {
    let Ok(val) = serde_json::from_slice::<Value>(body) else {
        write_json(
            stream,
            400,
            &json!({"code": "files.invalid_request", "message": "invalid json"}),
        );
        return;
    };
    let numbers = val
        .get("numbers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut state = shared.state.lock().expect("state lock");
    if !state.plans.contains_key(plan_id) {
        write_json(
            stream,
            404,
            &json!({"code": "files.plan_unknown", "message": "unknown plan"}),
        );
        return;
    }
    let origin = state.announced_origin(shared.store_port);
    let expires_at_ms = now_ms() + URL_TTL_MS;
    let mut out = Vec::with_capacity(numbers.len());
    for entry in &numbers {
        let Some(number) = entry.as_u64().and_then(|n| u32::try_from(n).ok()) else {
            continue;
        };
        let sig = state.mint_sig();
        let url = format!("{origin}/up/{plan_id}/{number}?sig={sig}");
        out.push(json!({
            "number": number,
            "url": url,
            "expiresAtMs": expires_at_ms,
        }));
    }
    write_json(stream, 200, &json!({"parts": out}));
}

fn handle_list_parts(stream: &mut TcpStream, shared: &Shared, plan_id: &str) {
    let state = shared.state.lock().expect("state lock");
    let Some(plan) = state.plans.get(plan_id) else {
        write_json(
            stream,
            404,
            &json!({"code": "files.plan_unknown", "message": "unknown plan"}),
        );
        return;
    };
    let parts: Vec<Value> = plan
        .held
        .iter()
        .map(|(number, bytes)| json!({"number": number, "size": bytes.len()}))
        .collect();
    write_json(stream, 200, &json!({"parts": parts}));
}

enum AssembleError {
    PartsMissing,
    HashMismatch,
}

fn assemble_plan(plan: &Plan) -> Result<(String, Vec<u8>), AssembleError> {
    let mut out = Vec::with_capacity((plan.size as usize).min(16 * 1024 * 1024));
    for (number, declared) in &plan.declared {
        let Some(bytes) = plan.held.get(number) else {
            return Err(AssembleError::PartsMissing);
        };
        if bytes.len() as u64 != declared.size {
            return Err(AssembleError::PartsMissing);
        }
        out.extend_from_slice(bytes);
    }
    if out.len() as u64 != plan.size {
        return Err(AssembleError::HashMismatch);
    }
    if crate::blob_id(&out) != plan.content_id {
        return Err(AssembleError::HashMismatch);
    }
    Ok((plan.content_id.clone(), out))
}

fn handle_complete_plan(stream: &mut TcpStream, shared: &Shared, plan_id: &str) {
    let mut state = shared.state.lock().expect("state lock");
    let Some(plan) = state.plans.get(plan_id) else {
        write_json(
            stream,
            404,
            &json!({"code": "files.plan_unknown", "message": "unknown plan"}),
        );
        return;
    };
    match assemble_plan(plan) {
        Ok((content_id, bytes)) => {
            state.objects.insert(content_id, Arc::new(bytes));
            state.plans.remove(plan_id);
            write_json(stream, 200, &json!({"status": "placed"}));
        }
        Err(AssembleError::PartsMissing) => {
            write_json(
                stream,
                409,
                &json!({"code": "files.parts_missing", "message": "parts missing"}),
            );
        }
        Err(AssembleError::HashMismatch) => {
            write_json(
                stream,
                409,
                &json!({"code": "files.hash_mismatch", "message": "hash mismatch"}),
            );
        }
    }
}

fn handle_download_grant(stream: &mut TcpStream, shared: &Shared, body: &[u8]) {
    let Ok(val) = serde_json::from_slice::<Value>(body) else {
        write_json(
            stream,
            400,
            &json!({"code": "files.invalid_request", "message": "invalid json"}),
        );
        return;
    };
    let Some(content_id) = val.get("contentId").and_then(Value::as_str) else {
        write_json(
            stream,
            400,
            &json!({"code": "files.invalid_request", "message": "missing contentId"}),
        );
        return;
    };
    let mut state = shared.state.lock().expect("state lock");
    if !state.objects.contains_key(content_id) {
        write_json(
            stream,
            404,
            &json!({"code": "blob.not_found", "message": "object not found"}),
        );
        return;
    }
    let Some(hex_digest) = content_id.strip_prefix("sha256:") else {
        write_json(
            stream,
            404,
            &json!({"code": "blob.not_found", "message": "object not found"}),
        );
        return;
    };
    let origin = state.announced_origin(shared.store_port);
    let sig = state.mint_sig();
    let expires_at_ms = now_ms() + URL_TTL_MS;
    let url = format!("{origin}/get/{hex_digest}?sig={sig}");
    write_json(
        stream,
        200,
        &json!({"url": url, "expiresAtMs": expires_at_ms}),
    );
}

fn handle_store_conn(mut stream: TcpStream, shared: &Shared) {
    let Some(req) = read_head(&mut stream) else {
        return;
    };
    let (path, query) = match req.target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (req.target.as_str(), ""),
    };
    let sig = parse_sig(query);
    let sig_valid = {
        let state = shared.state.lock().expect("state lock");
        sig.is_some_and(|s| s > state.expired_up_to && s <= state.mint_seq)
    };
    if !sig_valid {
        write_xml_error(&mut stream, 403, "AccessDenied", "Request has expired");
        return;
    }

    if req.method == "PUT" {
        if let Some(rest) = path.strip_prefix("/up/") {
            if let Some((plan_id, part_str)) = rest.split_once('/') {
                if let Ok(part_num) = part_str.parse::<u32>() {
                    handle_store_put_part(&mut stream, shared, &req, plan_id, part_num);
                    return;
                }
            }
        }
        if let Some(hex_digest) = path.strip_prefix("/put/") {
            handle_store_put_single(&mut stream, shared, &req, hex_digest);
            return;
        }
    }
    if req.method == "GET" {
        if let Some(hex_digest) = path.strip_prefix("/get/") {
            handle_store_get(&mut stream, shared, &req, hex_digest);
            return;
        }
    }
    write_xml_error(&mut stream, 404, "NoSuchKey", "Not found");
}

fn handle_store_put_part(
    stream: &mut TcpStream,
    shared: &Shared,
    req: &RequestHead,
    plan_id: &str,
    part_num: u32,
) {
    let (expected_size, expected_sha256, cut_limit, silence, pace) = {
        let mut state = shared.state.lock().expect("state lock");
        let Some(plan) = state.plans.get(plan_id) else {
            write_xml_error(stream, 404, "NoSuchUpload", "Unknown plan");
            return;
        };
        let Some(declared) = plan.declared.get(&part_num) else {
            write_xml_error(stream, 404, "NoSuchUpload", "Unknown part");
            return;
        };
        let expected_size = declared.size;
        let expected_sha256 = declared.sha256_b64.clone();
        let header_len = req
            .headers
            .get("content-length")
            .and_then(|v| v.parse::<u64>().ok());
        let header_sha = req.headers.get("x-amz-checksum-sha256");
        if header_len != Some(expected_size)
            || header_sha.map(String::as_str) != Some(&expected_sha256)
        {
            write_xml_error(
                stream,
                403,
                "SignatureDoesNotMatch",
                "Content-Length or checksum header mismatch",
            );
            return;
        }
        let cut_limit = state.cut_next.take();
        let silence = state.silence_next.take();
        let pace = UploadPace::of(&state);
        (expected_size, expected_sha256, cut_limit, silence, pace)
    };

    let body = match read_store_body(
        stream,
        &req.initial_body,
        expected_size,
        cut_limit,
        silence,
        pace,
        &shared.bytes_received,
    ) {
        StoreRead::Complete(body) => body,
        StoreRead::Cut | StoreRead::Silenced => {
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
        StoreRead::Eof => return,
    };

    let actual_sha = BASE64.encode(Sha256::digest(&body));
    if actual_sha != expected_sha256 {
        write_xml_error(stream, 400, "BadDigest", "Part SHA-256 mismatch");
        return;
    }

    let hold = shared.state.lock().expect("state lock").pending_hold.take();
    if let Some(hold) = hold {
        let (lock, cvar) = &*hold;
        let mut guard = lock.lock().expect("hold lock");
        guard.held = true;
        cvar.notify_all();
        let _ = cvar
            .wait_timeout_while(guard, HOLD_TIMEOUT, |s| !s.released)
            .expect("hold release wait");
    }

    {
        let mut state = shared.state.lock().expect("state lock");
        if let Some(plan) = state.plans.get_mut(plan_id) {
            plan.held.insert(part_num, body);
        }
        state.parts_accepted.push(part_num);
    }
    write_response(stream, 200, "application/octet-stream", &[], &[]);
}

fn handle_store_put_single(
    stream: &mut TcpStream,
    shared: &Shared,
    req: &RequestHead,
    hex_digest: &str,
) {
    let Some(content_len) = req
        .headers
        .get("content-length")
        .and_then(|v| v.parse::<u64>().ok())
    else {
        write_xml_error(
            stream,
            403,
            "SignatureDoesNotMatch",
            "Missing Content-Length",
        );
        return;
    };
    let (cut_limit, silence, pace) = {
        let mut state = shared.state.lock().expect("state lock");
        let pace = UploadPace::of(&state);
        (state.cut_next.take(), state.silence_next.take(), pace)
    };
    let body = match read_store_body(
        stream,
        &req.initial_body,
        content_len,
        cut_limit,
        silence,
        pace,
        &shared.bytes_received,
    ) {
        StoreRead::Complete(body) => body,
        StoreRead::Cut | StoreRead::Silenced => {
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
        StoreRead::Eof => return,
    };

    let digest = Sha256::digest(&body);
    if hex::encode(digest) != hex_digest {
        write_xml_error(stream, 400, "BadDigest", "Object SHA-256 mismatch");
        return;
    }
    if let Some(header_sha) = req.headers.get("x-amz-checksum-sha256") {
        if BASE64.encode(digest) != *header_sha {
            write_xml_error(stream, 400, "BadDigest", "Checksum header mismatch");
            return;
        }
    }
    let content_id = format!("sha256:{hex_digest}");
    shared
        .state
        .lock()
        .expect("state lock")
        .objects
        .insert(content_id, Arc::new(body));
    write_response(stream, 200, "application/octet-stream", &[], &[]);
}

/// Sleep until `moved` body bytes are due at `pace` (`chunk` bytes every `every`) since
/// `started`, so a paced body moves at that rate however the kernel splits it.
fn keep_pace(pace: Option<(usize, Duration)>, started: std::time::Instant, moved: u64) {
    let Some((chunk, every)) = pace else { return };
    let due = every.mul_f64(moved as f64 / chunk.max(1) as f64);
    if let Some(ahead) = due.checked_sub(started.elapsed()) {
        std::thread::sleep(ahead);
    }
}

fn handle_store_get(stream: &mut TcpStream, shared: &Shared, req: &RequestHead, hex_digest: &str) {
    let content_id = format!("sha256:{hex_digest}");
    let (object, cut_limit, silence, pace) = {
        let mut state = shared.state.lock().expect("state lock");
        let Some(object) = state.objects.get(&content_id).cloned() else {
            write_xml_error(stream, 404, "NoSuchKey", "Object not found");
            return;
        };
        let cut_limit = state.cut_next.take();
        let silence = state.silence_next.take();
        let pace = state.pace;
        (object, cut_limit, silence, pace)
    };
    shared.store_gets.fetch_add(1, Ordering::SeqCst);

    let total = object.len() as u64;
    let (status, slice, content_range) = if let Some(range_hdr) = req.headers.get("range") {
        match parse_range(range_hdr, total) {
            Some((start, end)) => (
                206u16,
                &object[start as usize..=end as usize],
                Some(format!("bytes {start}-{end}/{total}")),
            ),
            None => {
                let range_val = format!("bytes */{total}");
                write_response(
                    stream,
                    416,
                    "application/octet-stream",
                    &[("content-range", &range_val)],
                    &[],
                );
                return;
            }
        }
    } else {
        (200u16, &object[..], None)
    };

    if silence.is_none() && cut_limit == Some(0) {
        let _ = stream.shutdown(Shutdown::Both);
        return;
    }

    let reason = status_reason(status);
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/octet-stream\r\ncontent-length: {}\r\n",
        slice.len()
    );
    if let Some(cr) = &content_range {
        head.push_str(&format!("content-range: {cr}\r\n"));
    }
    head.push_str("connection: close\r\n\r\n");
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }

    let limit = silence.map(|(n, _)| n).or(cut_limit);
    let to_send = limit.map_or(slice.len(), |n| slice.len().min(n as usize));
    let chunk_cap = pace.map_or(16_384, |(chunk, _)| chunk.max(1));
    let started = std::time::Instant::now();
    let mut offset = 0usize;
    while offset < to_send {
        let end = (offset + chunk_cap).min(to_send);
        match stream.write(&slice[offset..end]) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                shared.bytes_served.fetch_add(n as u64, Ordering::SeqCst);
                offset += n;
                keep_pace(pace, started, offset as u64);
            }
        }
    }
    let _ = stream.flush();
    if let Some((_, hold_for)) = silence {
        std::thread::sleep(hold_for);
        let _ = stream.shutdown(Shutdown::Both);
    } else if cut_limit.is_some() {
        let _ = stream.shutdown(Shutdown::Both);
    }
}

fn parse_sig(query: &str) -> Option<u64> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == "sig").then(|| v.parse::<u64>().ok())?
    })
}

fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    let (start_s, end_s) = spec.split_once('-')?;
    let start: u64 = start_s.trim().parse().ok()?;
    if start >= total {
        return None;
    }
    let end = if end_s.trim().is_empty() {
        total - 1
    } else {
        let parsed: u64 = end_s.trim().parse().ok()?;
        parsed.min(total - 1)
    };
    if end < start {
        return None;
    }
    Some((start, end))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_http(host_port: &str, req: &str) -> (u16, String, Vec<u8>) {
        let mut stream = TcpStream::connect(host_port).expect("connect");
        stream.write_all(req.as_bytes()).expect("write req");
        stream.flush().expect("flush");
        let mut resp = Vec::new();
        let _ = stream.read_to_end(&mut resp);
        let head_end = resp
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("http header end")
            + 4;
        let head = String::from_utf8_lossy(&resp[..head_end]).into_owned();
        let status: u16 = head
            .lines()
            .next()
            .expect("status line")
            .split_whitespace()
            .nth(1)
            .expect("status code")
            .parse()
            .expect("parse status");
        (status, head, resp[head_end..].to_vec())
    }

    fn request_download_url(standin: &StandIn, content_id: &str) -> String {
        let server_addr = standin
            .server_url()
            .strip_prefix("http://")
            .expect("http scheme")
            .to_owned();
        let body = json!({"contentId": content_id}).to_string();
        let req = format!(
            "POST /notes/files/download-grant HTTP/1.1\r\n\
             host: {server_addr}\r\n\
             authorization: Bearer {}\r\n\
             content-type: application/json\r\n\
             content-length: {}\r\n\
             connection: close\r\n\r\n{body}",
            standin.token(),
            body.len()
        );
        let (status, _, resp_body) = raw_http(&server_addr, &req);
        assert_eq!(status, 200);
        let val: Value = serde_json::from_slice(&resp_body).expect("json");
        val.get("url")
            .and_then(Value::as_str)
            .expect("url")
            .to_owned()
    }

    fn split_url(url: &str) -> (String, String) {
        let rest = url.strip_prefix("http://").expect("http");
        let (host, path) = rest.split_once('/').expect("slash");
        (host.to_owned(), format!("/{path}"))
    }

    #[test]
    fn grant_and_range_get_206() {
        let standin = StandIn::start("notes");
        let payload: Vec<u8> = (0u8..100).collect();
        let id = standin.seed(&payload);
        let url = request_download_url(&standin, &id);
        assert_eq!(
            standin.grant_requests(),
            vec!["POST /notes/files/download-grant".to_owned()]
        );

        let (host, path) = split_url(&url);
        let req = format!(
            "GET {path} HTTP/1.1\r\nhost: {host}\r\nrange: bytes=40-\r\nconnection: close\r\n\r\n"
        );
        let (status, head, body) = raw_http(&host, &req);
        assert_eq!(status, 206);
        assert!(head
            .to_ascii_lowercase()
            .contains("content-range: bytes 40-99/100"));
        assert_eq!(body, &payload[40..]);
        assert_eq!(standin.store_bytes_served(), 60);

        let req_416 = format!(
            "GET {path} HTTP/1.1\r\nhost: {host}\r\nrange: bytes=100-\r\nconnection: close\r\n\r\n"
        );
        let (status_416, _, _) = raw_http(&host, &req_416);
        assert_eq!(status_416, 416);
    }

    #[test]
    fn expired_url_returns_403() {
        let standin = StandIn::start("notes");
        let id = standin.seed(b"hello world");
        let url = request_download_url(&standin, &id);
        standin.expire_urls();

        let (host, path) = split_url(&url);
        let req = format!("GET {path} HTTP/1.1\r\nhost: {host}\r\nconnection: close\r\n\r\n");
        let (status, _, body) = raw_http(&host, &req);
        assert_eq!(status, 403);
        assert!(String::from_utf8_lossy(&body).contains("AccessDenied"));
    }

    #[test]
    fn cut_next_drops_connection_after_limit() {
        let standin = StandIn::start("notes");
        let payload = vec![0xABu8; 1024];
        let id = standin.seed(&payload);
        let url = request_download_url(&standin, &id);
        standin.cut_next(128);

        let (host, path) = split_url(&url);
        let req = format!("GET {path} HTTP/1.1\r\nhost: {host}\r\nconnection: close\r\n\r\n");
        let (status, head, body) = raw_http(&host, &req);
        assert_eq!(status, 200);
        assert!(head.to_ascii_lowercase().contains("content-length: 1024"));
        assert_eq!(body.len(), 128);
        assert_eq!(standin.store_bytes_served(), 128);
    }

    #[test]
    fn silence_next_holds_the_connection_open_then_drops_it() {
        let standin = StandIn::start("notes");
        let payload = vec![0xCDu8; 1024];
        let id = standin.seed(&payload);
        let url = request_download_url(&standin, &id);
        standin.silence_next(100, Duration::from_millis(600));

        let (host, path) = split_url(&url);
        let req = format!("GET {path} HTTP/1.1\r\nhost: {host}\r\nconnection: close\r\n\r\n");
        let mut stream = TcpStream::connect(&host).expect("connect");
        stream.write_all(req.as_bytes()).expect("write req");
        stream.flush().expect("flush");
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .expect("set timeout");

        let mut buf = Vec::new();
        let mut tmp = [0u8; 512];
        let timed_out = loop {
            match stream.read(&mut tmp) {
                Ok(0) => break false,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break true;
                }
                Err(e) => panic!("unexpected read error: {e}"),
            }
        };
        assert!(
            timed_out,
            "expected read timeout while connection is held silent"
        );

        let head_end = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("http header end")
            + 4;
        assert_eq!(buf.len() - head_end, 100);

        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set eof timeout");
        match stream.read(&mut tmp) {
            Ok(0) => {}
            Ok(n) => panic!("expected EOF after silence, got {n} extra bytes"),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                ) => {}
            Err(e) => panic!("expected EOF after silence, got {e}"),
        }
        assert_eq!(standin.store_bytes_served(), 100);
        assert_eq!(standin.store_gets(), 1);
    }

    #[test]
    fn throttle_paces_a_get() {
        let standin = StandIn::start("notes");
        let payload = vec![0x5Au8; 5000];
        let id = standin.seed(&payload);
        let url = request_download_url(&standin, &id);
        standin.throttle(Some((1000, Duration::from_millis(20))));

        let (host, path) = split_url(&url);
        let req = format!("GET {path} HTTP/1.1\r\nhost: {host}\r\nconnection: close\r\n\r\n");
        let start = std::time::Instant::now();
        let (status, _, body) = raw_http(&host, &req);
        let elapsed = start.elapsed();

        assert_eq!(status, 200);
        assert_eq!(body, payload);
        assert!(
            elapsed >= Duration::from_millis(80),
            "expected paced GET >= 80ms, got {elapsed:?}"
        );
    }
}
