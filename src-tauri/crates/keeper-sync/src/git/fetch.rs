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
    time::Instant,
};

use crate::{
    error::{Result, SyncError},
    git::cli,
};

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
        .map_err(|err| classify(&flatten(&err), &host, interrupt))?;

    if let Some(credential) = credential {
        // Owned clones because the callback must be `'static`: gix keeps it for
        // the life of the connection and follows redirects with it.
        let username = credential.username.clone();
        let secret = credential.secret.clone();
        // The closure's return type is gix's, and its 192-byte `Err` lives in
        // `gix_credentials::protocol::Error` — a foreign type we can neither
        // box nor shrink, and the callback signature is not ours to change.
        #[allow(clippy::result_large_err)]
        connection.set_credentials(move |action| static_credential(&username, &secret, action));
    }

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
        Err(err) => return Err(classify(&flatten(&err), &host, interrupt)),
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
        Err(err) => return Err(classify(&flatten(&err), &host, interrupt)),
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

/// Answer gitoxide's credential requests from a secret we already hold.
///
/// `Store` and `Erase` deliberately return `Ok(None)`: the OS keychain owns the
/// secret's lifecycle, and letting git "approve" it would write a copy into a
/// credential store the user never opted into.
// The return type is dictated by gix's credential-callback contract, and the
// 192-byte `Err` variant lives in `gix_credentials::protocol::Error` — a
// foreign type we cannot box or shrink. Boxing our side would not change it.
#[allow(clippy::result_large_err)]
fn static_credential(
    username: &str,
    secret: &str,
    action: gix::credentials::helper::Action,
) -> gix::credentials::protocol::Result {
    match action {
        gix::credentials::helper::Action::Get(context) => {
            Ok(Some(gix::credentials::protocol::Outcome {
                identity: gix::sec::identity::Account {
                    username: username.to_owned(),
                    password: secret.to_owned(),
                    oauth_refresh_token: None,
                },
                next: context.into(),
            }))
        }
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
fn flatten(err: &dyn std::error::Error) -> String {
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

/// Turn a gitoxide transport error into the engine's taxonomy.
///
/// An interruption is checked first: gitoxide reports a cancelled transfer as
/// an ordinary transport error, and a user-requested stop must never be
/// retried with backoff or shown as a warning.
fn classify(text: &str, host: &str, interrupt: &AtomicBool) -> SyncError {
    if interrupt.load(Ordering::Relaxed) {
        return SyncError::Cancelled;
    }
    // Shared with the `git` shim: the same wire-level failures produce the same
    // wording whether they came from gix or from the binary.
    let message = cli::truncate(&cli::scrub_userinfo(text), 1_024);
    cli::classify_message(&message, host, Some(host), &[])
        .unwrap_or_else(|| SyncError::Git(format!("fetch from {host} failed: {message}")))
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
        let err = classify("connection reset by peer", "git.example.com", &interrupt);
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
            "could not resolve host: git.example.com",
            "git.example.com",
            &interrupt,
        );
        assert_eq!(network.code(), "network");
        assert!(network.to_string().contains("git.example.com"), "{network}");

        let unknown = classify(
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
        let context = gix::credentials::protocol::Context {
            url: Some("https://git.example.com/x.git".into()),
            ..Default::default()
        };
        let got = static_credential(
            "keeper",
            "token",
            gix::credentials::helper::Action::Get(context),
        )
        .expect("no error")
        .expect("an identity");
        assert_eq!(got.identity.username, "keeper");
        assert_eq!(got.identity.password, "token");

        // Approving would copy the secret into a credential store the user
        // never asked for; the keychain is the only home it has.
        let stored = static_credential(
            "keeper",
            "token",
            gix::credentials::helper::Action::Store("whatever".into()),
        )
        .expect("no error");
        assert!(stored.is_none());
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
