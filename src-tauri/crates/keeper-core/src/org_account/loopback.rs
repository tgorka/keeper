//! The RFC 8252 loopback redirect for desktops (AD-311).
//!
//! A descriptor whose `redirect_uri` is `http://127.0.0.1[:port]/path` gets a
//! listener on that address (port `0` when none is pinned) instead of the
//! platform's auth sheet. It is `std::net` on one `std::thread`, deliberately:
//! it serves a single callback, and tokio's net feature is not something the
//! rest of keeper-core needs.
//!
//! The thread answers every request on the redirect path that carries a
//! `state` or `error` with a small "you can close this tab" page and hands the
//! full callback URL to the sign-in over a channel — which routes it through
//! [`crate::oauth::OAuthFlowRegistry::resolve`] like any deep link. It keeps
//! serving after that: any local process can reach the port, and a request
//! with the wrong `state` must not stop the listener before the browser's
//! real callback arrives. Anything else (a favicon) gets a `404`. Dropping the
//! [`Listener`] — once the flow resolved or timed out — stops the thread.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use url::Url;

use super::AccountError;

/// How long one connection may take to send its whole request head; the
/// listener serves one connection at a time, so a slow sender is cut off.
const REQUEST_DEADLINE: Duration = Duration::from_secs(5);

/// The largest request head the listener reads.
const MAX_REQUEST: usize = 16 * 1024;

const DONE_PAGE: &str = "<!doctype html><meta charset=\"utf-8\"><title>keeper</title>\
<body style=\"font-family:system-ui;margin:3rem\"><p>keeper has the answer from your sign-in. You can \
close this tab and return to keeper.</p></body>";

/// Whether `redirect` is a loopback redirect keeper serves itself: plain
/// `http` to the literal `127.0.0.1` the listener binds, never `localhost`
/// (which may resolve to `::1`, where another process can listen).
pub fn is_loopback(redirect: &Url) -> bool {
    redirect.scheme() == "http" && redirect.host() == Some(url::Host::Ipv4(Ipv4Addr::LOCALHOST))
}

/// A bound loopback listener waiting for the authorization callback.
pub struct Listener {
    redirect: Url,
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    callbacks: mpsc::UnboundedReceiver<String>,
}

impl Listener {
    /// Bind the address `redirect` names (an ephemeral port when it names
    /// none) and start the accept thread.
    pub fn bind(redirect: &Url) -> Result<Self, AccountError> {
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let listener = TcpListener::bind(SocketAddr::new(ip, redirect.port().unwrap_or(0)))
            .map_err(|e| {
                AccountError::Internal(format!(
                    "keeper could not listen on {ip} for the sign-in to return: {e}"
                ))
            })?;
        let addr = listener
            .local_addr()
            .map_err(|e| AccountError::Internal(format!("loopback address unavailable: {e}")))?;
        let mut actual = redirect.clone();
        actual
            .set_port(Some(addr.port()))
            .map_err(|()| AccountError::Internal("loopback redirect has no port".to_owned()))?;

        let stop = Arc::new(AtomicBool::new(false));
        let (tx, callbacks) = mpsc::unbounded_channel();
        let thread_stop = Arc::clone(&stop);
        let base = actual.clone();
        std::thread::Builder::new()
            .name("keeper-oidc-loopback".to_owned())
            .spawn(move || serve(listener, &base, &thread_stop, &tx))
            .map_err(|e| AccountError::Internal(format!("loopback thread: {e}")))?;

        Ok(Listener {
            redirect: actual,
            addr,
            stop,
            callbacks,
        })
    }

    /// The redirect URI with the port actually bound — the one the
    /// authorization request and the token exchange must both carry.
    pub fn redirect_uri(&self) -> &Url {
        &self.redirect
    }

    /// The next callback URL, or `None` once the thread has stopped.
    pub async fn next(&mut self) -> Option<String> {
        self.callbacks.recv().await
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake a blocked `accept` so the thread sees the flag and exits.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
    }
}

fn serve(listener: TcpListener, base: &Url, stop: &AtomicBool, tx: &mpsc::UnboundedSender<String>) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(mut stream) = stream else { continue };
        let Some(target) = request_target(&mut stream) else {
            respond(&mut stream, "400 Bad Request", "");
            continue;
        };
        match callback_url(base, &target) {
            Some(url) => {
                respond(&mut stream, "200 OK", DONE_PAGE);
                if tx.send(url).is_err() {
                    return;
                }
            }
            None => respond(&mut stream, "404 Not Found", ""),
        }
    }
}

/// The request-target of an HTTP/1.x `GET`, e.g. `/callback?code=…&state=…`.
/// A network-path reference (`//host/…`) is refused: joined onto the base it
/// would name another host.
fn request_target(stream: &mut TcpStream) -> Option<String> {
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let mut head = Vec::new();
    let mut chunk = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let left = deadline.checked_duration_since(Instant::now())?;
        stream.set_read_timeout(Some(left)).ok()?;
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 || head.len() + n > MAX_REQUEST {
            return None;
        }
        head.extend_from_slice(&chunk[..n]);
    }
    let line = std::str::from_utf8(&head).ok()?.lines().next()?;
    let mut parts = line.split(' ');
    match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) if target.starts_with('/') && !target.starts_with("//") => {
            Some(target.to_owned())
        }
        _ => None,
    }
}

/// The full callback URL when `target` is on the redirect path and carries
/// an authorization response.
fn callback_url(base: &Url, target: &str) -> Option<String> {
    let url = base.join(target).ok()?;
    let is_response = url.query_pairs().any(|(k, _)| k == "state" || k == "error");
    (url.path() == base.path() && is_response).then(|| url.into())
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(addr: SocketAddr, target: &str) -> String {
        let mut stream = TcpStream::connect(addr).expect("connect");
        write!(stream, "GET {target} HTTP/1.1\r\nHost: {addr}\r\n\r\n").expect("write");
        let mut reply = String::new();
        let _ = stream.read_to_string(&mut reply);
        reply
    }

    #[test]
    fn only_http_to_the_literal_127_0_0_1_is_served_here() {
        for yes in ["http://127.0.0.1/cb", "http://127.0.0.1:8123/cb"] {
            assert!(is_loopback(&Url::parse(yes).expect("url")), "{yes}");
        }
        for no in [
            "https://127.0.0.1/cb",
            "keeper://oauth/acme/callback",
            "http://10.0.0.1/cb",
            "http://localhost/cb",
            "http://127.0.0.2/cb",
            "http://[::1]/cb",
        ] {
            assert!(!is_loopback(&Url::parse(no).expect("url")), "{no}");
        }
    }

    #[tokio::test]
    async fn a_callback_with_the_wrong_state_does_not_stop_the_listener() {
        let mut listener =
            Listener::bind(&Url::parse("http://127.0.0.1/callback").expect("url")).expect("bind");
        let addr = listener.addr;
        assert_ne!(listener.redirect_uri().port(), Some(0));

        let favicon = tokio::task::spawn_blocking(move || get(addr, "/favicon.ico"))
            .await
            .expect("join");
        assert!(favicon.starts_with("HTTP/1.1 404"), "{favicon}");
        let elsewhere = tokio::task::spawn_blocking(move || {
            get(addr, "//evil.example/callback?state=s&code=c")
        })
        .await
        .expect("join");
        assert!(elsewhere.starts_with("HTTP/1.1 400"), "{elsewhere}");

        // A local process guessing the port gets in first…
        let bogus = tokio::task::spawn_blocking(move || get(addr, "/callback?state=guess"))
            .await
            .expect("join");
        assert!(bogus.starts_with("HTTP/1.1 200"), "{bogus}");
        // …and the browser's real callback is still served.
        let page = tokio::task::spawn_blocking(move || get(addr, "/callback?code=c&state=s"))
            .await
            .expect("join");
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
        assert!(page.contains("close this tab"));

        let port = addr.port();
        assert_eq!(
            listener.next().await.expect("the guess"),
            format!("http://127.0.0.1:{port}/callback?state=guess")
        );
        assert_eq!(
            listener.next().await.expect("the real callback"),
            format!("http://127.0.0.1:{port}/callback?code=c&state=s")
        );
    }

    #[test]
    fn a_connection_that_trickles_its_request_is_cut_off_at_the_deadline() {
        let listener =
            Listener::bind(&Url::parse("http://127.0.0.1/callback").expect("url")).expect("bind");
        let mut slow = TcpStream::connect(listener.addr).expect("connect");
        let started = Instant::now();
        let trickle = std::thread::spawn(move || {
            for _ in 0..40 {
                if slow.write_all(b"G").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
        // The listener gives up on the trickler and serves the next caller.
        let page = get(listener.addr, "/callback?code=c&state=s");
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
        assert!(started.elapsed() < REQUEST_DEADLINE + Duration::from_secs(3));
        let _ = trickle.join();
    }
}
