//! Fetching, credentials and fast-forward analysis (Stories 24.2, 24.3, AD-53).
//!
//! **Blocking.** gitoxide's HTTP transport has no async implementation — the
//! `async-network-client` feature does not cover `https://` — and the pack
//! resolution that follows is CPU-bound anyway. Every caller on a tokio runtime
//! must wrap [`fetch`] in `spawn_blocking`, or it will stall the executor and
//! with it the UI (NFR-25).
//!
//! **Credentials never become a subprocess.** AD-53 requires the secret to be
//! injected through gitoxide's programmatic
//! [`set_credentials`](gix::remote::Connection::set_credentials) callback, so
//! it never reaches a `git credential` helper's cache, a process argument list
//! or `~/.git-credentials`. `Store` and `Erase` requests are answered with "no
//! opinion": the keychain is the only place a keeper secret lives.

use std::{
    num::NonZeroU32,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use crate::{
    error::{Result, SyncError},
    git::cli,
};

/// Longest a fetch may run before the engine stops waiting for it (AD-228,
/// F-PULLPUSH-3, F-Deps-2).
///
/// gitoxide's reqwest transport sets one timeout — 20 s to connect
/// (`gix-transport/src/client/blocking_io/http/reqwest/remote.rs:64`) — and
/// nothing after it: no read timeout, no total. A peer that completes the TCP
/// handshake and then goes silent therefore parked the fetch, the
/// `spawn_blocking` thread under it and the profile's one-operation
/// reservation for ever. Ten minutes is the whole-fetch bound the engine
/// applies from outside (`Engine::do_pull`), generous enough for a first fetch
/// of the reference folder's history over a slow link and finite, which is the
/// property the transport lacked. The LFS legs bound their silence with a
/// read timeout; a fetch has no seam for one, so the bound is on the leg.
pub const FETCH_DEADLINE: Duration = Duration::from_secs(600);

/// Byte- or object-level transfer progress as `(done, total)`; a `total` of `0`
/// means the remote did not say.
///
/// An `Arc` rather than a borrowed `&dyn Fn` because gitoxide requires
/// `P::SubProgress: 'static` on `receive`, so the adapter that carries this
/// callback into gix cannot borrow from the caller's frame.
pub type TransferProgress = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// How much history to ask for, and which refs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FetchOptions {
    /// Truncate history to this many commits at the remote. `None` fetches all
    /// of it; an existing shallow boundary is left alone.
    pub shallow: Option<NonZeroU32>,
    /// Refspecs to fetch. Empty uses whatever `remote.<name>.fetch` configures.
    pub refspecs: Vec<String>,
}

/// A username/secret pair read out of [`SyncPlatform::secret_get`].
///
/// [`SyncPlatform::secret_get`]: crate::platform::SyncPlatform::secret_get
#[derive(Clone)]
pub struct Credential {
    /// Account name. Token auth on Forgejo and GitHub often puts the token
    /// here and something inert in `secret`, so this is not necessarily safe
    /// to log either.
    pub username: String,
    /// Password, token or app password. Never logged, never persisted by this
    /// crate, and redacted by the `Debug` implementation below.
    pub secret: String,
}

impl std::fmt::Debug for Credential {
    /// Hand-written because `#[derive(Debug)]` on a secret is how tokens end
    /// up in log files (NFR-26). *Both* fields are withheld: with token auth
    /// the username routinely carries the secret and the password is a
    /// placeholder, so redacting only one of them protects nothing.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("username", &"<redacted>")
            .field("secret", &"<redacted>")
            .finish()
    }
}

/// What a fetch found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FetchOutcome {
    /// Full name of the remote ref matching the local branch, if the remote
    /// advertised one (`refs/heads/main`).
    pub remote_ref: Option<String>,
    /// The commit that ref points at on the remote.
    pub remote_id: Option<gix::hash::ObjectId>,
    /// The commit the local branch points at.
    pub local_id: Option<gix::hash::ObjectId>,
    /// Whether the local branch can be advanced to `remote_id` without a merge.
    ///
    /// `false` means the two sides diverged, which for a bidirectional profile
    /// is where conflict copies come from (AD-43) and for a one-way lane is a
    /// hard error (AD-50).
    pub fast_forward: bool,
    /// Whether the remote actually sent objects.
    pub received_pack: bool,
}

/// Fetch from `remote_name` and report what it means for the local branch.
///
/// See the module docs: blocking, and credentials go through a callback rather
/// than a helper process.
pub fn fetch(
    repo: &gix::Repository,
    remote_name: &str,
    options: &FetchOptions,
    credential: Option<&Credential>,
    progress: &TransferProgress,
    interrupt: &AtomicBool,
) -> Result<FetchOutcome> {
    let mut remote = repo
        .find_remote(remote_name)
        .map_err(|err| SyncError::Config(format!("remote {remote_name:?} is not usable: {err}")))?;
    if !options.refspecs.is_empty() {
        remote = remote
            .with_refspecs(
                options
                    .refspecs
                    .iter()
                    .map(|spec| gix::bstr::BStr::new(spec.as_str())),
                gix::remote::Direction::Fetch,
            )
            .map_err(|err| SyncError::Config(format!("invalid refspec: {err}")))?;
    }
    let host = remote
        .url(gix::remote::Direction::Fetch)
        .and_then(|url| url.host().map(str::to_owned))
        // A filesystem remote (a pendrive, AD-48) genuinely has no host.
        .unwrap_or_else(|| "local".to_owned());

    let mut connection = remote
        .connect(gix::remote::Direction::Fetch)
        .map_err(|err| classify_error("fetch", &err, &host, interrupt))?;

    // Installed whether or not there is a credential. With no callback gitoxide
    // reads `credential.helper` from the configuration and asks whatever it
    // finds — on a Mac that has run `git config --global credential.helper
    // osxkeychain` that is *somebody's* account for this host, not this
    // profile's, and the fetch quietly authenticates as them (F-PULLPUSH-5).
    // A callback that answers "no credential" makes the failure `Auth`, which
    // is the truth. Owned clone because the callback must be `'static`: gix
    // keeps it for the life of the connection and follows redirects with it.
    let credential = credential.cloned();
    // The closure's return type is gix's, and its 192-byte `Err` lives in
    // `gix_credentials::protocol::Error` — a foreign type we can neither box
    // nor shrink, and the callback signature is not ours to change.
    #[allow(clippy::result_large_err)]
    connection.set_credentials(move |action| static_credential(credential.as_ref(), action));

    // A repository created in the forge and not yet pushed to advertises zero
    // refs. gitoxide surfaces that as a refspec-match failure, and it can come
    // from either stage depending on the transport — so both are folded into
    // one empty outcome rather than guessing which. There is genuinely nothing
    // to pull; the push that follows is what creates the branch.
    //
    // `local_id` is still read: an adopted folder has commits of its own, and
    // the caller decides what to do with them.
    let nothing_to_pull = || -> Result<FetchOutcome> {
        Ok(FetchOutcome {
            remote_ref: None,
            remote_id: None,
            local_id: super::repo::head_commit_id(repo)?,
            fast_forward: false,
            received_pack: false,
        })
    };

    let prepared = match connection.prepare_fetch(
        FlatProgress::root(Arc::clone(progress)),
        gix::remote::ref_map::Options::default(),
    ) {
        Ok(prepared) => prepared,
        Err(err) if mentions_an_empty_advertisement(&flatten(&err)) => {
            return nothing_to_pull();
        }
        Err(err) => return Err(classify_error("fetch", &err, &host, interrupt)),
    };
    let prepared = match options.shallow {
        Some(depth) => prepared.with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(depth)),
        None => prepared,
    };

    let outcome = match prepared.receive(FlatProgress::root(Arc::clone(progress)), interrupt) {
        Ok(outcome) => outcome,
        Err(err) if mentions_an_empty_advertisement(&flatten(&err)) => {
            return nothing_to_pull();
        }
        Err(err) => return Err(classify_error("fetch", &err, &host, interrupt)),
    };

    summarize(repo, &outcome)
}

/// Did this failure mean the remote advertised no refs at all?
///
/// A refspec matching nothing has two very different causes: the branch does
/// not exist on an otherwise-populated remote (a real problem worth surfacing),
/// or the remote is brand new and holds nothing yet (routine). Only the second
/// is tolerated, and the count in gitoxide's message is what separates them.
fn mentions_an_empty_advertisement(text: &str) -> bool {
    text.contains("matched any of the 0 refs")
}

/// Answer gitoxide's credential requests from a secret we already hold — or
/// refuse, when we hold none.
///
/// `Store` and `Erase` deliberately return `Ok(None)`: the OS keychain owns the
/// secret's lifecycle, and letting git "approve" it would write a copy into a
/// credential store the user never opted into.
///
/// `Get` with no credential answers `Quit` rather than `Ok(None)`. Both end
/// the handshake, but `Ok(None)` is what gix reports as *"No credentials were
/// returned at all as if the credential helper isn't functioning unknowingly"*
/// — a sentence about a helper this callback exists to keep out of the loop.
/// `Quit` is *"Failed to obtain credentials"*, which [`cli::classify_message`]
/// already reads as [`SyncError::Auth`]: the remote asked for an identity and
/// this profile has none to give, and only a person can change that.
// The return type is dictated by gix's credential-callback contract, and the
// 192-byte `Err` variant lives in `gix_credentials::protocol::Error` — a
// foreign type we cannot box or shrink. Boxing our side would not change it.
#[allow(clippy::result_large_err)]
pub(crate) fn static_credential(
    credential: Option<&Credential>,
    action: gix::credentials::helper::Action,
) -> gix::credentials::protocol::Result {
    match action {
        gix::credentials::helper::Action::Get(context) => match credential {
            Some(credential) => Ok(Some(gix::credentials::protocol::Outcome {
                identity: gix::sec::identity::Account {
                    username: credential.username.clone(),
                    password: credential.secret.clone(),
                    oauth_refresh_token: None,
                },
                next: context.into(),
            })),
            None => Err(gix::credentials::protocol::Error::Quit),
        },
        gix::credentials::helper::Action::Store(_) | gix::credentials::helper::Action::Erase(_) => {
            Ok(None)
        }
    }
}

/// Flatten an error and everything that caused it into one line.
///
/// gitoxide's outer messages are frequently the least informative part of the
/// failure — "Failed to update references to their new position" says nothing
/// about *which* ref or *why*, and the actual reason lives two or three
/// `source()` hops down. Reporting only the top frame turns a diagnosable
/// problem into a guess, so the whole chain is kept.
pub(crate) fn flatten(err: &dyn std::error::Error) -> String {
    let mut message = err.to_string();
    let mut cause = err.source();
    while let Some(current) = cause {
        let text = current.to_string();
        // gix repeats the parent's wording in some variants; adding it twice
        // makes the line longer without making it clearer.
        if !message.contains(&text) {
            message.push_str(": ");
            message.push_str(&text);
        }
        cause = current.source();
    }
    message
}

/// Turn a gitoxide transport error into the engine's taxonomy, by what it
/// **is** before by what it **says** (AD-228).
///
/// The text list in [`cli::classify_message`] is shared with the `git` shim
/// and is the only thing a shim has; a gitoxide error is a typed chain, and
/// the chain knows things the text does not. The order:
///
/// 1. an interruption is `Cancelled` — gitoxide reports a cancelled transfer
///    as an ordinary transport error, and a user-requested stop must never be
///    retried with backoff or shown as a warning;
/// 2. the text, so `Auth` and `Diverged` keep the precedence they have for
///    the shim (a 403 arrives inside wording that also reads as a network
///    failure);
/// 3. the chain: a `reqwest::Error` that is a connect or timeout failure, or
///    an `io::Error` of a connection-shaped kind, anywhere under `source()`;
/// 4. `Git`, for what is left.
///
/// Step 3 is what hesperia lacked. `tcp connect error: deadline has elapsed`
/// matched no needle, so a remote that had been unreachable for six days was
/// `Git` — Transient, but not `Network` — and the profile never once read
/// `Offline` (F-engine-1). The needle is in the list now too, because the
/// shim can produce the same words; the type is what stops the next wording
/// nobody has seen yet from repeating the six days.
pub(crate) fn classify_error(
    operation: &str,
    err: &(dyn std::error::Error + 'static),
    host: &str,
    interrupt: &AtomicBool,
) -> SyncError {
    if interrupt.load(Ordering::Relaxed) {
        return SyncError::Cancelled;
    }
    let text = flatten(err);
    let message = cli::truncate(&cli::scrub_userinfo(&text), 1_024);
    if let Some(classified) = cli::classify_message(&message, host, Some(host), &[]) {
        return classified;
    }
    if let Some(kind) = network_cause(err) {
        return SyncError::Network {
            host: host.to_owned(),
            reason: format!("{kind}: {}", cli::one_line(&message)),
        };
    }
    SyncError::Git(format!("{operation} from {host} failed: {message}"))
}

/// The connection-shaped cause in an error chain, named, if there is one.
///
/// Walks `source()` from the top and inspects each frame for the two types a
/// gitoxide HTTP failure can carry: the `reqwest::Error` gix preserves inside
/// its `io::Error` (`reqwest/remote.rs:203`, `io::Error::other(err)`), and the
/// `io::Error` itself. The kinds are the ones that mean *the other end is not
/// there or stopped talking*. Two gix mappings are deliberately absent: an
/// HTTP 5xx becomes `ConnectionAborted` and a 401 becomes `PermissionDenied`
/// (`reqwest/remote.rs:191-198`), and neither is the network's fault.
///
/// The `get_ref` step is not optional. `io::Error::source()` does not return
/// the payload `other()` wrapped — it returns the payload's *own* source, so a
/// plain `source()` walk steps over the `reqwest::Error` without ever seeing
/// it. For a refused connection that still ends at an `io::Error` two frames
/// down; for the connect timeout hesperia hit it ends at
/// `tokio::time::error::Elapsed`, and only the `reqwest::Error` frame knows
/// that was a timeout.
fn network_cause(err: &(dyn std::error::Error + 'static)) -> Option<&'static str> {
    let mut frame = Some(err);
    while let Some(current) = frame {
        if let Some(kind) = connection_failure(current) {
            return Some(kind);
        }
        if let Some(inner) = current
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::get_ref)
        {
            if let Some(kind) = connection_failure(inner) {
                return Some(kind);
            }
        }
        frame = current.source();
    }
    None
}

/// One frame of [`network_cause`]'s walk.
fn connection_failure(frame: &(dyn std::error::Error + 'static)) -> Option<&'static str> {
    if let Some(err) = frame.downcast_ref::<reqwest::Error>() {
        if err.is_timeout() {
            return Some("timed out");
        }
        if err.is_connect() {
            return Some("could not connect");
        }
    }
    let err = frame.downcast_ref::<std::io::Error>()?;
    use std::io::ErrorKind as K;
    match err.kind() {
        K::TimedOut => Some("timed out"),
        K::ConnectionRefused => Some("connection refused"),
        K::ConnectionReset => Some("connection reset"),
        K::NotConnected => Some("not connected"),
        K::HostUnreachable => Some("host unreachable"),
        K::NetworkUnreachable => Some("network unreachable"),
        K::BrokenPipe => Some("connection broken"),
        _ => None,
    }
}

/// [`classify_error`] for a caller that holds only the text — the shim, and
/// the tests that feed it a line from a log.
///
/// Same order minus the chain, which text does not have.
#[cfg(test)]
pub(crate) fn classify(
    operation: &str,
    text: &str,
    host: &str,
    interrupt: &AtomicBool,
) -> SyncError {
    if interrupt.load(Ordering::Relaxed) {
        return SyncError::Cancelled;
    }
    // Shared with the `git` shim: the same wire-level failures produce the same
    // wording whether they came from gix or from the binary.
    let message = cli::truncate(&cli::scrub_userinfo(text), 1_024);
    cli::classify_message(&message, host, Some(host), &[])
        .unwrap_or_else(|| SyncError::Git(format!("{operation} from {host} failed: {message}")))
}

/// Derive the outcome from the ref advertisement and the local branch.
fn summarize(
    repo: &gix::Repository,
    outcome: &gix::remote::fetch::Outcome,
) -> Result<FetchOutcome> {
    let local_id = super::repo::head_commit_id(repo)?;
    let branch = repo
        .head_name()
        .map_err(|err| SyncError::Git(format!("could not read HEAD: {err}")))?;

    let mut remote_ref = None;
    let mut remote_id = None;
    if let Some(branch) = &branch {
        for candidate in &outcome.ref_map.remote_refs {
            let (name, target, _peeled) = candidate.unpack();
            if name == branch.as_bstr() {
                remote_ref = Some(name.to_string());
                remote_id = target.map(|id| id.to_owned());
                break;
            }
        }
    }

    let fast_forward = match (local_id, remote_id) {
        // An unborn local branch can adopt anything.
        (None, Some(_)) => true,
        (Some(local), Some(remote)) if local == remote => true,
        (Some(local), Some(remote)) => repo
            .merge_base(local, remote)
            // No merge base means unrelated histories, which is a divergence
            // rather than an error worth failing the whole fetch over.
            .map(|base| base.detach() == local)
            .unwrap_or(false),
        _ => false,
    };

    Ok(FetchOutcome {
        remote_ref,
        remote_id,
        local_id,
        fast_forward,
        received_pack: matches!(&outcome.status, gix::remote::fetch::Status::Change { .. }),
    })
}

/// Shortest gap between two progress callbacks, in milliseconds.
///
/// gitoxide ticks its counters per packet; forwarding every one of them would
/// hammer the host's sink hundreds of thousands of times during a large pack
/// for a tray line that repaints at ~1 Hz.
const REPORT_INTERVAL_MS: u64 = 100;

/// Throttling state shared by every node of one progress tree.
struct ProgressSink {
    report: TransferProgress,
    started: Instant,
    last_report_ms: AtomicU64,
}

/// Bridges gitoxide's `prodash` progress *tree* onto one flat `(done, total)`
/// callback.
///
/// gix reports progress as a hierarchy — a node per negotiation round, per pack
/// phase, per checkout chunk — while the surface AD-51 specifies is a single
/// status line. Each node therefore keeps its **own** counter and they all
/// report through one shared, throttled sink, so the line tracks whichever
/// phase is currently active. Summing unrelated counters into one bar would
/// produce a number that is not any real quantity.
///
/// Caveat worth knowing: `Count::counter()` hands out the raw atomic, and code
/// that increments it directly bypasses the callback. Those phases still show
/// up, just at the next `set`/`inc_by`/`init` boundary.
struct FlatProgress {
    step: Arc<AtomicUsize>,
    /// `0` means "unbounded", matching prodash's `init(None, …)`.
    max: Arc<AtomicUsize>,
    sink: Arc<ProgressSink>,
    name: String,
    id: gix::progress::Id,
}

impl FlatProgress {
    /// The root node of a fresh progress tree.
    fn root(report: TransferProgress) -> Self {
        Self {
            step: Arc::new(AtomicUsize::new(0)),
            max: Arc::new(AtomicUsize::new(0)),
            sink: Arc::new(ProgressSink {
                report,
                started: Instant::now(),
                last_report_ms: AtomicU64::new(0),
            }),
            name: String::new(),
            id: gix::progress::UNKNOWN,
        }
    }

    /// A sibling counter sharing this tree's sink.
    fn child(&self, name: String, id: gix::progress::Id) -> Self {
        Self {
            step: Arc::new(AtomicUsize::new(0)),
            max: Arc::new(AtomicUsize::new(0)),
            sink: Arc::clone(&self.sink),
            name,
            id,
        }
    }

    /// Forward the current numbers, unless it is too soon.
    fn emit(&self, force: bool) {
        let step = self.step.load(Ordering::Relaxed) as u64;
        let max = self.max.load(Ordering::Relaxed) as u64;
        let elapsed = u64::try_from(self.sink.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let last = self.sink.last_report_ms.load(Ordering::Relaxed);
        // The completion tick is always forwarded: a bar left at 99% is worse
        // than one that updates a little less often.
        let complete = max > 0 && step >= max;
        if !force && !complete && elapsed.saturating_sub(last) < REPORT_INTERVAL_MS {
            return;
        }
        self.sink.last_report_ms.store(elapsed, Ordering::Relaxed);
        (self.sink.report)(step, max);
    }
}

impl gix::progress::Count for FlatProgress {
    fn set(&self, step: gix::progress::Step) {
        self.step.store(step, Ordering::Relaxed);
        self.emit(false);
    }

    fn step(&self) -> gix::progress::Step {
        self.step.load(Ordering::Relaxed)
    }

    fn inc_by(&self, step: gix::progress::Step) {
        self.step.fetch_add(step, Ordering::Relaxed);
        self.emit(false);
    }

    fn counter(&self) -> gix::progress::StepShared {
        Arc::clone(&self.step)
    }
}

impl gix::progress::Progress for FlatProgress {
    fn init(&mut self, max: Option<gix::progress::Step>, _unit: Option<gix::progress::Unit>) {
        self.max.store(max.unwrap_or(0), Ordering::Relaxed);
        self.step.store(0, Ordering::Relaxed);
        // A phase starting is exactly when the status line should change.
        self.emit(true);
    }

    fn max(&self) -> Option<gix::progress::Step> {
        let max = self.max.load(Ordering::Relaxed);
        (max > 0).then_some(max)
    }

    fn set_max(&mut self, max: Option<gix::progress::Step>) -> Option<gix::progress::Step> {
        let previous = self.max.swap(max.unwrap_or(0), Ordering::Relaxed);
        (previous > 0).then_some(previous)
    }

    fn set_name(&mut self, name: String) {
        self.name = name;
    }

    fn name(&self) -> Option<String> {
        Some(self.name.clone())
    }

    fn id(&self) -> gix::progress::Id {
        self.id
    }

    fn message(&self, level: gix::progress::MessageLevel, message: String) {
        // Progress messages are diagnostics, not user-facing copy; they carry
        // ref names and counts, never content or credentials.
        tracing::debug!(?level, name = %self.name, %message, "git progress");
    }
}

impl gix::progress::NestedProgress for FlatProgress {
    type SubProgress = FlatProgress;

    fn add_child(&mut self, name: impl Into<String>) -> Self::SubProgress {
        self.child(name.into(), gix::progress::UNKNOWN)
    }

    fn add_child_with_id(
        &mut self,
        name: impl Into<String>,
        id: gix::progress::Id,
    ) -> Self::SubProgress {
        self.child(name.into(), id)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_empty_remote_is_told_apart_from_a_missing_branch() {
        // The ref COUNT is the whole distinction: a populated remote that has
        // no such branch is a real problem and must keep surfacing as one.
        assert!(mentions_an_empty_advertisement(
            "None of the refspec(s) +refs/heads/main:refs/remotes/origin/main \
             matched any of the 0 refs on the remote"
        ));
        assert!(!mentions_an_empty_advertisement(
            "None of the refspec(s) +refs/heads/nope:refs/remotes/origin/nope \
             matched any of the 7 refs on the remote"
        ));
        assert!(!mentions_an_empty_advertisement("connection reset by peer"));
    }

    use super::*;
    use gix::progress::{Count as _, NestedProgress as _, Progress as _};
    use std::path::Path;

    fn signature() -> gix::actor::Signature {
        gix::actor::Signature {
            name: "Keeper".into(),
            email: "sync@01abc.keeper.invalid".into(),
            time: gix::date::Time::new(1_700_000_000, 0),
        }
    }

    /// Commit `content` on `HEAD`, deterministically.
    ///
    /// Fixed name, email and time mean the same content produces the same
    /// commit id in two independent repositories, which is how the fixtures
    /// below get a shared ancestor without any transport.
    fn commit(
        repo: &gix::Repository,
        parent: Option<gix::hash::ObjectId>,
        content: &str,
    ) -> gix::hash::ObjectId {
        let blob = repo.write_blob(content.as_bytes()).expect("blob").detach();
        let tree = gix::objs::Tree {
            entries: vec![gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: "a.txt".into(),
                oid: blob,
            }],
        };
        let tree = repo.write_object(&tree).expect("tree").detach();
        let mut buf = gix::date::parse::TimeBuf::default();
        let author = signature();
        let author = author.to_ref(&mut buf);
        repo.commit_as(
            author,
            author,
            "HEAD",
            content,
            tree,
            parent.into_iter().collect::<Vec<_>>(),
        )
        .expect("commit")
        .detach()
    }

    fn point_at_remote(local_dir: &Path, remote_dir: &Path) {
        let config_path = local_dir.join(".git/config");
        let mut config = gix::config::File::from_path_no_includes(
            config_path.clone(),
            gix::config::Source::Local,
        )
        .expect("read config");
        config
            .set_raw_value("remote.origin.url", remote_dir.to_string_lossy().as_ref())
            .expect("set url");
        config
            .set_raw_value("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*")
            .expect("set fetch");
        let mut out = Vec::new();
        config.write_to(&mut out).expect("serialize");
        std::fs::write(&config_path, out).expect("write config");
    }

    struct Fixture {
        _remote_dir: tempfile::TempDir,
        _local_dir: tempfile::TempDir,
        local: gix::Repository,
    }

    /// A bare remote and a local repository, both seeded with the same root
    /// commit, then advanced independently by `remote_extra` / `local_extra`.
    fn fixture(remote_extra: &[&str], local_extra: &[&str]) -> Fixture {
        let remote_dir = tempfile::tempdir().expect("tempdir");
        let local_dir = tempfile::tempdir().expect("tempdir");
        let remote = gix::init_bare(remote_dir.path()).expect("init bare");
        let local = gix::init(local_dir.path()).expect("init");

        let root_remote = commit(&remote, None, "root");
        let root_local = commit(&local, None, "root");
        assert_eq!(
            root_remote, root_local,
            "the fixtures must share an ancestor for merge-base to be meaningful"
        );

        let mut tip = Some(root_remote);
        for content in remote_extra {
            tip = Some(commit(&remote, tip, content));
        }
        let mut tip = Some(root_local);
        for content in local_extra {
            tip = Some(commit(&local, tip, content));
        }

        point_at_remote(local_dir.path(), remote_dir.path());
        // Configure the local repository the way a managed one actually is. The
        // fetch below moves a remote-tracking ref, which writes a reflog entry,
        // and gitoxide refuses to write one without a committer. Going through
        // the production helper means these tests no longer silently depend on
        // the host having a global git identity — a CI runner has none, so the
        // fetch failed there while passing on every developer machine — and it
        // puts the identity fallback itself under test.
        let configured = gix::open(local_dir.path()).expect("reopen");
        crate::git::repo::enforce_local_config(&configured).expect("managed config");
        // Re-open so both the configured remote and the identity are visible.
        let local = gix::open(local_dir.path()).expect("reopen");
        Fixture {
            _remote_dir: remote_dir,
            _local_dir: local_dir,
            local,
        }
    }

    fn fetch_once(repo: &gix::Repository) -> FetchOutcome {
        let progress: TransferProgress = Arc::new(|_, _| {});
        let interrupt = AtomicBool::new(false);
        fetch(
            repo,
            "origin",
            &FetchOptions::default(),
            None,
            &progress,
            &interrupt,
        )
        .expect("fetch from a local bare repository")
    }

    #[test]
    fn a_local_branch_behind_the_remote_can_fast_forward() {
        let fixture = fixture(&["second"], &[]);
        let outcome = fetch_once(&fixture.local);

        assert!(outcome.fast_forward, "{outcome:?}");
        assert!(outcome.remote_ref.is_some());
        assert_ne!(outcome.remote_id, outcome.local_id);
        assert!(outcome.received_pack, "the remote had a commit we lacked");
    }

    #[test]
    fn a_local_branch_ahead_of_the_remote_cannot_fast_forward() {
        // Advancing the local branch to an *older* remote tip would throw work
        // away, so this must never be reported as a fast-forward.
        let fixture = fixture(&[], &["second"]);
        let outcome = fetch_once(&fixture.local);

        assert!(!outcome.fast_forward, "{outcome:?}");
    }

    #[test]
    fn diverged_branches_cannot_fast_forward() {
        let fixture = fixture(&["theirs"], &["ours"]);
        let outcome = fetch_once(&fixture.local);

        assert!(!outcome.fast_forward, "{outcome:?}");
        assert_ne!(outcome.remote_id, outcome.local_id);
        assert!(
            outcome.remote_id.is_some() && outcome.local_id.is_some(),
            "both tips must be reported so the caller can make conflict copies"
        );
    }

    #[test]
    fn identical_branches_are_trivially_fast_forwardable() {
        let fixture = fixture(&[], &[]);
        let outcome = fetch_once(&fixture.local);

        assert!(outcome.fast_forward);
        assert_eq!(outcome.remote_id, outcome.local_id);
    }

    #[test]
    fn an_unknown_remote_is_a_configuration_error_not_a_network_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        let progress: TransferProgress = Arc::new(|_, _| {});
        let interrupt = AtomicBool::new(false);

        let err = fetch(
            &repo,
            "nope",
            &FetchOptions::default(),
            None,
            &progress,
            &interrupt,
        )
        .expect_err("there is no such remote");
        assert_eq!(err.code(), "config");
    }

    #[test]
    fn an_interrupted_fetch_is_cancelled_not_failed() {
        let interrupt = AtomicBool::new(true);
        let err = classify(
            "fetch",
            "connection reset by peer",
            "git.example.com",
            &interrupt,
        );
        assert_eq!(err.code(), "cancelled");
        assert_eq!(
            err.retriability(),
            crate::error::Retriability::Permanent,
            "a cancelled transfer must not be re-driven with backoff"
        );
    }

    #[test]
    fn transport_failures_are_classified_and_scrubbed() {
        let interrupt = AtomicBool::new(false);

        let auth = classify(
            "fetch",
            "Authentication failed for 'https://tok:en@git.example.com/x.git'",
            "git.example.com",
            &interrupt,
        );
        assert_eq!(auth.code(), "auth");
        assert!(
            auth.to_string().contains("git.example.com"),
            "the configured host must reach the message: {auth}"
        );
        assert!(
            !auth.to_string().contains("tok:en"),
            "userinfo leaked: {auth}"
        );

        let network = classify(
            "fetch",
            "could not resolve host: git.example.com",
            "git.example.com",
            &interrupt,
        );
        assert_eq!(network.code(), "network");
        assert!(network.to_string().contains("git.example.com"), "{network}");

        let unknown = classify(
            "fetch",
            "pack index checksum mismatch for 'https://u:p@git.example.com/x.git'",
            "git.example.com",
            &interrupt,
        );
        assert_eq!(unknown.code(), "git");
        assert!(
            !unknown.to_string().contains(":p@"),
            "userinfo leaked: {unknown}"
        );
    }

    #[test]
    fn a_credential_never_prints_either_of_its_fields() {
        // A token pair is `(token, "x-oauth-basic")` as often as it is
        // `(user, password)`, so neither field may reach a log line.
        let credential = Credential {
            username: "ghp_supersecret".to_owned(),
            secret: "x-oauth-basic".to_owned(),
        };
        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("ghp_supersecret"), "{rendered}");
        assert!(!rendered.contains("x-oauth-basic"), "{rendered}");
        assert!(rendered.contains("Credential"), "{rendered}");
    }

    #[test]
    fn the_credential_callback_answers_get_and_declines_to_store() {
        let credential = Credential {
            username: "keeper".to_owned(),
            secret: "token".to_owned(),
        };
        let context = gix::credentials::protocol::Context {
            url: Some("https://git.example.com/x.git".into()),
            ..Default::default()
        };
        let got = static_credential(
            Some(&credential),
            gix::credentials::helper::Action::Get(context),
        )
        .expect("no error")
        .expect("an identity");
        assert_eq!(got.identity.username, "keeper");
        assert_eq!(got.identity.password, "token");

        // Approving would copy the secret into a credential store the user
        // never asked for; the keychain is the only home it has.
        let stored = static_credential(
            Some(&credential),
            gix::credentials::helper::Action::Store("whatever".into()),
        )
        .expect("no error");
        assert!(stored.is_none());
    }

    /// With no credential the callback still answers — with a refusal, so the
    /// handshake ends in `Failed to obtain credentials` and the folder reads
    /// `Auth`, never in a helper the machine happens to have (F-PULLPUSH-5).
    #[test]
    fn the_credential_callback_refuses_when_there_is_no_credential() {
        let context = gix::credentials::protocol::Context {
            url: Some("https://git.example.com/x.git".into()),
            ..Default::default()
        };
        let refused = static_credential(None, gix::credentials::helper::Action::Get(context))
            .expect_err("no credential is an answer, not an absence of one");
        assert!(
            matches!(refused, gix::credentials::protocol::Error::Quit),
            "{refused}"
        );
        // And the sentence gix wraps that in is one the classifier reads as
        // the credential problem it is.
        let interrupt = AtomicBool::new(false);
        let err = classify(
            "fetch",
            &format!("Failed to obtain credentials: {refused}"),
            "git.example.com",
            &interrupt,
        );
        assert_eq!(err.code(), "auth", "{err}");

        // Store and erase with nothing held are still "no opinion".
        let stored = static_credential(
            None,
            gix::credentials::helper::Action::Store("whatever".into()),
        )
        .expect("no error");
        assert!(stored.is_none());
    }

    /// The exact line hesperia's log carried 53 times over six days, verbatim
    /// from `review-sync-2026-09-08/evidence-hesperia.md`. It was `Git`.
    #[test]
    fn hesperias_connect_timeout_wording_is_network() {
        let interrupt = AtomicBool::new(false);
        let text = "fetch from electra failed: An IO error occurred when talking to the server: \
                    error sending request for url (https://electra/tgorka/tgdrive.git/info/refs?\
                    service=git-upload-pack): client error (Connect): tcp connect error: deadline \
                    has elapsed";
        let err = classify("fetch", text, "electra", &interrupt);
        assert_eq!(err.code(), "network", "{err}");
        assert_eq!(
            err.retriability(),
            crate::error::Retriability::Transient,
            "offline is retried, never parked"
        );
    }

    /// An error chain whose *words* say nothing and whose *type* says the
    /// connection is gone. The wording of the next transport nobody has seen
    /// yet cannot be in a needle list; its `io::ErrorKind` can.
    #[test]
    fn a_connection_shaped_io_error_is_network_whatever_it_says() {
        #[derive(Debug)]
        struct Outer(std::io::Error);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("boom")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let interrupt = AtomicBool::new(false);

        for (kind, word) in [
            (std::io::ErrorKind::TimedOut, "timed out"),
            (std::io::ErrorKind::ConnectionRefused, "connection refused"),
            (std::io::ErrorKind::ConnectionReset, "connection reset"),
            (std::io::ErrorKind::NotConnected, "not connected"),
            (std::io::ErrorKind::HostUnreachable, "host unreachable"),
            (
                std::io::ErrorKind::NetworkUnreachable,
                "network unreachable",
            ),
            (std::io::ErrorKind::BrokenPipe, "connection broken"),
        ] {
            let err = Outer(std::io::Error::new(kind, "zzz"));
            assert_eq!(
                classify("fetch", &flatten(&err), "h", &interrupt).code(),
                "git",
                "the text alone must NOT classify, or this test proves nothing: {kind:?}"
            );
            let classified = classify_error("fetch", &err, "h", &interrupt);
            assert_eq!(classified.code(), "network", "{kind:?}: {classified}");
            assert!(
                classified.to_string().contains(word),
                "the reason must name the kind: {classified}"
            );
        }

        // And the kinds gix uses for HTTP statuses stay out of it: a 5xx is
        // `ConnectionAborted`, a 401 is `PermissionDenied`, and neither is the
        // network's fault.
        for kind in [
            std::io::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::Other,
        ] {
            let err = Outer(std::io::Error::new(kind, "zzz"));
            assert_eq!(
                classify_error("fetch", &err, "h", &interrupt).code(),
                "git",
                "{kind:?}"
            );
        }
    }

    /// A real `reqwest::Error` from a real refused connection and a real
    /// timeout, each found by type through the `io::Error::other` gix wraps it
    /// in. The timeout is the one that matters: its chain ends in tokio's
    /// `Elapsed`, not in an `io::Error`, so only the `reqwest::Error` frame —
    /// reachable through `get_ref`, invisible to `source()` — can name it.
    #[test]
    fn a_reqwest_failure_is_found_through_gix_s_io_wrapper() {
        // Bind and drop: the port was ours a moment ago, so nothing answers.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port();
        let refused = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("client")
            .get(format!("http://127.0.0.1:{port}/x.git/info/refs"))
            .send()
            .expect_err("nothing listens there");
        assert!(refused.is_connect(), "{refused}");
        // `reqwest/remote.rs:203`: `std::io::Error::other(err)`.
        let wrapped = std::io::Error::other(refused);
        assert_eq!(network_cause(&wrapped), Some("could not connect"));

        // A listener that accepts and says nothing, against a client that
        // gives up: hesperia's failure shape, one layer in.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let quiet = std::thread::spawn(move || listener.accept().map(|(stream, _)| stream));
        let timed_out = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(200))
            .build()
            .expect("client")
            .get(format!("http://127.0.0.1:{port}/x.git/info/refs"))
            .send()
            .expect_err("the peer never answers");
        assert!(timed_out.is_timeout(), "{timed_out}");
        let wrapped = std::io::Error::other(timed_out);
        assert_eq!(network_cause(&wrapped), Some("timed out"));
        // And through `source()` alone the frame is gone — which is why the
        // walk looks inside the wrapper.
        let below = std::error::Error::source(&wrapped);
        assert!(
            below.is_none_or(|deeper| deeper.downcast_ref::<reqwest::Error>().is_none()),
            "io::Error::source() must not hand back the payload, or this test is moot"
        );
        drop(quiet.join());
    }

    /// A rejected credential arrives inside wording that also reads as a
    /// network failure, and the chain may carry a reset too; `Auth` must win,
    /// or the profile retries a token that will never start working.
    #[test]
    fn auth_keeps_precedence_over_a_network_shaped_chain() {
        #[derive(Debug)]
        struct Outer(std::io::Error);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("Authentication failed for 'https://git.example.com/x.git'")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let interrupt = AtomicBool::new(false);
        let err = Outer(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset",
        ));
        assert_eq!(
            classify_error("fetch", &err, "git.example.com", &interrupt).code(),
            "auth"
        );
    }

    #[test]
    fn progress_is_forwarded_on_init_and_on_completion() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let report: TransferProgress = Arc::new(move |done, total| {
            if let Ok(mut guard) = sink.lock() {
                guard.push((done, total));
            }
        });

        let mut root = FlatProgress::root(report);
        root.init(Some(10), None);
        root.inc_by(10);

        let observed = seen.lock().expect("lock").clone();
        assert_eq!(
            observed.first().copied(),
            Some((0, 10)),
            "a phase starting must reach the sink immediately"
        );
        assert_eq!(
            observed.last().copied(),
            Some((10, 10)),
            "completion must never be throttled away"
        );
    }

    #[test]
    fn a_child_counter_reports_independently_through_the_same_sink() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let report: TransferProgress = Arc::new(move |done, total| {
            if let Ok(mut guard) = sink.lock() {
                guard.push((done, total));
            }
        });

        let mut root = FlatProgress::root(report);
        root.init(Some(100), None);
        let mut child = root.add_child("receiving objects");
        child.init(Some(4), None);
        child.inc_by(4);

        assert_eq!(child.max(), Some(4));
        // The parent's own counter is untouched by the child's work.
        assert_eq!(root.step(), 0);
        assert_eq!(root.max(), Some(100));
        let observed = seen.lock().expect("lock").clone();
        assert!(
            observed.contains(&(4, 4)),
            "the child's completion never arrived: {observed:?}"
        );
    }

    #[test]
    fn intermediate_ticks_are_throttled() {
        let count = Arc::new(AtomicUsize::new(0));
        let sink = Arc::clone(&count);
        let report: TransferProgress = Arc::new(move |_, _| {
            sink.fetch_add(1, Ordering::Relaxed);
        });

        let mut root = FlatProgress::root(report);
        root.init(Some(1_000_000), None);
        for _ in 0..10_000 {
            root.inc_by(1);
        }

        assert!(
            count.load(Ordering::Relaxed) < 100,
            "10 000 ticks produced {} callbacks; the sink would be hammered",
            count.load(Ordering::Relaxed)
        );
    }
}
