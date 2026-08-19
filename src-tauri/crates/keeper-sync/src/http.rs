//! The one HTTP client every network path in this crate shares, and the two
//! timeouts that decide whether a stalled transfer becomes a failure or a hang.
//!
//! # The hang this exists to prevent
//!
//! `reqwest` applies no timeout of its own. A client built without one waits on
//! a silent socket forever, and nothing below notices: a TCP connection whose
//! peer has gone — a laptop that changed networks, a NAT table that dropped the
//! mapping, a Wi-Fi link that stopped passing packets mid-object — is
//! indistinguishable from a slow one until something asks.
//!
//! That is not a stalled download. It is a stalled *profile*. The engine runs
//! one operation per profile ([`crate::engine`]), and a unit is returned to the
//! queue only when its operation ends or the process restarts
//! ([`crate::db::requeue_running`]) — so a body that never delivers another byte
//! parks every other unit behind it until someone quits the app.
//!
//! Observed 2026-08-18 on a folder pulling 53 GB of LFS objects over a degraded
//! link: nine downloads sat in `running` for sixteen hours, their backoff long
//! expired, with 95 units queued behind them and not one byte written to the
//! partial files in that time. The transfers had died; only the app had not
//! noticed.
//!
//! # Why a read timeout, and emphatically not a total one
//!
//! [`ClientBuilder::timeout`] bounds a whole request, which is the wrong
//! question for this crate: on the link above a single 1 GB object legitimately
//! takes over two hours, and any total budget large enough to allow that is far
//! too large to catch a stall.
//!
//! [`ClientBuilder::read_timeout`] bounds **silence** instead — it applies to
//! each read and resets after every successful one. A transfer that keeps
//! delivering bytes may run as long as it likes; a transfer that delivers
//! nothing for [`READ_TIMEOUT`] fails with a timeout, which
//! [`crate::error::SyncError::Network`] classifies `Transient`, so the unit
//! goes back to the queue with backoff and the profile keeps moving.
//!
//! Note what this does not do: it cannot cancel a write to a wedged disk, and
//! it says nothing about work that stalls outside an HTTP read. It answers the
//! failure this crate actually spends its time in — the network.

use std::time::Duration;

use crate::error::{Result, SyncError};

/// How long to wait for a connection to be established.
///
/// Generous, because the first connection of a pass may be racing a VPN's own
/// setup, and a forge behind a cold reverse proxy is slow rather than absent.
/// Nothing is lost by waiting: a peer that has not answered in fifteen seconds
/// is reported as unreachable and retried with backoff.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long an established connection may deliver nothing before it is a
/// failure.
///
/// A ceiling on silence, not on duration — see the module docs. Sixty seconds
/// rather than something tighter because a server is allowed to think: an LFS
/// batch over a large request, or a forge reading a cold object off spinning
/// disk, can reasonably leave the socket quiet for a while, and turning that
/// into a failure would trade a rare hang for frequent spurious retries.
pub const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// The client this crate runs on.
pub fn client(user_agent: &'static str) -> Result<reqwest::Client> {
    client_with(user_agent, CONNECT_TIMEOUT, READ_TIMEOUT)
}

/// [`client`], with the timeouts named — the seam tests use to make a stall
/// observable in milliseconds instead of a minute.
pub fn client_with(
    user_agent: &'static str,
    connect_timeout: Duration,
    read_timeout: Duration,
) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(user_agent)
        .connect_timeout(connect_timeout)
        .read_timeout(read_timeout)
        .build()
        .map_err(|err| SyncError::Config(format!("could not build an HTTP client: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A server that promises a megabyte, sends a handful of bytes, and then
    /// goes quiet forever without closing the connection. This is what a dead
    /// peer looks like from the client side, and reproducing it needs a real
    /// socket: no error is ever delivered, so nothing short of a timeout can
    /// end the wait.
    ///
    /// `std::net` rather than `tokio::net` deliberately — the workspace does
    /// not enable tokio's `net` feature, and a blocking thread is all a
    /// one-connection stub needs.
    fn silent_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let mut scratch = [0u8; 1024];
            let _ = sock.read(&mut scratch);
            let _ = sock.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 1048576\r\n\r\nthe first bytes arrive",
            );
            let _ = sock.flush();
            // Hold the connection open and send nothing more. Long enough to
            // outlast the test, short enough not to leak past the run.
            std::thread::sleep(Duration::from_secs(30));
        });
        format!("http://{addr}/object")
    }

    #[tokio::test]
    async fn a_body_that_goes_silent_fails_instead_of_hanging_forever() {
        let url = silent_server();
        let http = client_with("test", Duration::from_secs(5), Duration::from_millis(200))
            .expect("client");

        let started = std::time::Instant::now();
        let mut response = http.get(&url).send().await.expect("headers arrive");
        let err = loop {
            match response.chunk().await {
                // The first frames are real; the silence comes after them.
                Ok(Some(_)) => continue,
                Ok(None) => panic!("the body ended cleanly, so nothing was stalled"),
                Err(err) => break err,
            }
        };

        assert!(
            err.is_timeout(),
            "a silent socket must surface as a timeout, got: {err}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the read timeout must end the wait, not the connect timeout"
        );
    }

    /// The property the whole module exists for, stated where a future edit
    /// will trip over it: a *total* request timeout would cap how long a
    /// legitimate transfer may run, and multi-gigabyte objects over a slow link
    /// routinely run for hours.
    #[test]
    fn the_defaults_bound_silence_and_never_duration() {
        assert!(READ_TIMEOUT >= Duration::from_secs(30), "room to think");
        assert!(
            READ_TIMEOUT <= Duration::from_secs(120),
            "a stall must be noticed while the queue behind it still matters"
        );
        assert!(CONNECT_TIMEOUT < READ_TIMEOUT);
    }
}
