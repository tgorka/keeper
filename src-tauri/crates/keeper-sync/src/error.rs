//! The sync error taxonomy (Story 23.2, AD-21 posture, AD-40).
//!
//! `keeper-sync` cannot use `keeper_core::CoreError` — it does not depend on
//! `keeper-core` (AD-40) — so it owns a self-contained `SyncError`. The shell
//! maps it into the existing `IpcError` envelope through the single
//! `to_ipc_error` funnel, and `keeper-syncd` renders it directly.
//!
//! Invariant, inherited verbatim from `keeper_core::error`: **a message never
//! carries a credential, a token, or file content.** Ids, hosts, paths, byte
//! counts and status codes are permitted; anything a user typed as a secret is
//! not. `Story 23.6`'s log-scan test enforces this mechanically.
//!
//! Every variant answers one question the UI actually has to distinguish. A
//! variant exists because a surface reacts differently to it, never because an
//! underlying library happened to have that error shape.

use std::path::PathBuf;

/// Whether a failed unit of work is worth attempting again unchanged.
///
/// This is a *policy* answer, not a transport detail: the scheduler consults it
/// to decide between re-queueing with backoff (Story 26.6) and parking the unit
/// for a human. Keeping it on the error means the classification lives next to
/// the thing being classified instead of in a match at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retriability {
    /// Try again after backoff — the world may change (network, quota, lock).
    Transient,
    /// Never retry unchanged; something must be reconfigured or fixed first.
    Permanent,
    /// Not an error the scheduler should retry: the work is *deliberately*
    /// deferred until an external condition holds (a volume is re-attached).
    Deferred,
}

/// Errors raised by the sync engine.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// No usable `git` binary. AD-41 makes it a hard runtime prerequisite:
    /// push, worktree mutation, sparse-checkout patterns and gc have no
    /// in-process implementation in gitoxide.
    #[error("git is unavailable: {reason}")]
    GitMissing { reason: String },

    /// A `git` subprocess exited non-zero. `stderr` is captured verbatim
    /// because git's own diagnostics are the most useful thing we can show;
    /// the shim truncates it and never logs a URL carrying userinfo.
    #[error("git {subcommand} failed (exit {code}): {stderr}")]
    GitCommand {
        subcommand: &'static str,
        code: i32,
        stderr: String,
    },

    /// The remote was unreachable, or the transfer was cut. Distinguished from
    /// `Auth` so the scheduler backs off instead of parking the profile.
    #[error("network failure talking to {host}: {reason}")]
    Network { host: String, reason: String },

    /// The credential was **rejected** (HTTP 401, or git's own "authentication
    /// failed"). Never retried unchanged — a retry storm against a git host is
    /// how an account gets rate-limited or locked.
    ///
    /// The host is the only fact available at every site that raises this: a
    /// 401 says the credential was not accepted and nothing about why, so the
    /// message names the one action that can change the outcome rather than
    /// guessing between expired, revoked and mistyped.
    #[error("{host} rejected the access token — replace it with a current one")]
    Auth { host: String },

    /// The credential was **accepted and then refused** (HTTP 403).
    ///
    /// A separate variant because the remedy is the opposite of [`Self::Auth`]'s:
    /// the token is valid, so pasting the same one again — or a freshly minted
    /// one with the same scopes — changes nothing. What is missing is
    /// permission, and only a human can grant it.
    ///
    /// Deliberately does not name a scope. A 403 from a forge is also how an
    /// archived repository, a lapsed membership and a fine-grained token
    /// missing repository access all read, and the wire carries nothing that
    /// tells them apart.
    #[error(
        "{host} accepted the access token but refused the request — give that token access to \
         this repository, or use one that already has it"
    )]
    Forbidden { host: String },

    /// A path cannot be represented on the remote or on a peer's filesystem
    /// (reserved name, illegal character, too long). This is one of the few
    /// conditions that genuinely requires a human: the user must rename.
    /// The message names the path because the surfaces that show this one only
    /// ever render `to_string()`: `record_failure` copies it into the profile's
    /// warning verbatim, and a "path cannot be synchronized" with no path is a
    /// sentence a user can do nothing with. The path is theirs already — they
    /// named the file — so it carries no secret the taxonomy forbids.
    #[error("{} cannot be synchronized: {reason}", .path.display())]
    InvalidPathForRemote { path: PathBuf, reason: String },

    /// The profile's volume is not mounted. **Not a failure** — AD-48's whole
    /// point is that absence is never deletion. Carried as an error only
    /// because it aborts the current operation.
    #[error("removable volume for this profile is not attached")]
    MediaAbsent,

    /// A push is holding because objects the commit's pointers name are not on
    /// the remote yet (Story 34.15).
    ///
    /// **Not a failure**, and the sibling of [`Self::MediaAbsent`]: both are
    /// waits on a condition rather than on a clock, which is why both are
    /// [`Retriability::Deferred`]. Publishing a pointer whose object the server
    /// does not have is the one outcome nobody sees go wrong — git accepts the
    /// push, and the next peer to clone gets a working tree full of pointer
    /// text with no error anywhere — so the push waits for its own uploads
    /// instead. The waiting unit is re-queued by whichever upload lands last.
    ///
    /// The count is outstanding upload *units*, not bytes, and it is a count
    /// rather than a list because the journal rows are the list.
    #[error(
        "publishing is on hold until this folder's large files reach the remote \
         ({objects} outstanding)"
    )]
    LfsUploadPending { objects: u32 },

    /// Content did not match its expected digest or length. Always hard: we
    /// discard the staged bytes rather than resume from a poisoned prefix.
    #[error("integrity check failed for {subject}: expected {expected}, got {actual}")]
    Integrity {
        subject: String,
        expected: String,
        actual: String,
    },

    /// The remote refused the write for capacity reasons (HTTP 413/507/509, or
    /// a Forgejo per-owner quota). Transient in principle, but only a human can
    /// usually clear it, so it is surfaced as an actionable notice.
    #[error("remote storage quota exceeded for {host}")]
    Quota { host: String },

    /// A divergence was detected that policy could not resolve automatically.
    /// Bidirectional profiles never produce this (they make conflict copies,
    /// AD-43); a one-way lane whose remote branch moved does (AD-50), because
    /// there a human decision is the point.
    #[error("{profile}: {reason}")]
    Diverged { profile: String, reason: String },

    /// The remote moved between this profile's last reconcile and its push, so
    /// git refused the update (DW-207).
    ///
    /// The same git output as [`Self::Diverged`] and a different condition. On
    /// a bidirectional profile sharing a branch with another machine, being
    /// overtaken between merge and push is *routine* — the other machine
    /// pushed, which is the entire point of sharing the branch. The answer is
    /// the reconcile loop keeper already owns: fetch, merge, push again. So
    /// this is transient and needs nobody, where `Diverged` is permanent and
    /// needs a human, and telling them apart is what stops a shared folder from
    /// parking itself every time two machines are awake at once.
    #[error("{profile}: the remote moved while pushing ({reason}) — reconciling")]
    RemoteMoved { profile: String, reason: String },

    /// The durable journal or profile store could not be read or written.
    /// Treated as fatal for the profile: without the journal we cannot promise
    /// NFR-24, and continuing would risk losing work silently.
    #[error("sync journal failure: {0}")]
    Journal(String),

    /// A gitoxide operation failed. Boxed because gix's error types are large
    /// and would otherwise inflate every `Result` in the crate.
    #[error("git object store failure: {0}")]
    Git(String),

    /// Filesystem failure, with the path it happened to. `std::io::Error` alone
    /// loses the path, which makes these reports useless in the field.
    #[error("{operation} failed for {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Configuration the engine cannot act on (unparseable remote URL, an
    /// invalid subpath, a threshold out of range).
    #[error("invalid sync configuration: {0}")]
    Config(String),

    /// keeper will not change the bytes at one path, and the refusal says why
    /// (Story 56.3, FR-338).
    ///
    /// The one variant that carries a *typed* refusal rather than a string,
    /// because two exhaustive matches have to classify it —
    /// `keeper-syncd::sync_exit_code` and `keeper::sync_ipc::sync_ipc_error` —
    /// and 56.4 adds four more refusals to the same vocabulary. Growing
    /// [`crate::lfs::hydrate::ContentRefusal`] therefore costs no churn in
    /// either match, where five more `SyncError` variants would have cost it
    /// twice each.
    ///
    /// The message is the refusal's own sentence, unchanged: it is written for
    /// the person who asked, and a prefix worded here would be a second voice
    /// in front of it.
    #[error("{0}")]
    Refused(crate::lfs::hydrate::ContentRefusal),

    /// A sync for this folder is already running.
    ///
    /// Not a misconfiguration and not a failure: a scheduled run and a "Sync
    /// now" click overlapping is the ordinary case, and the work is already
    /// being done by the run that got there first. It had been reported as
    /// `Config`, which rendered as "invalid sync configuration: … is already
    /// syncing" — a sentence that sends somebody to look for a broken setting
    /// that does not exist.
    #[error("{0} is already syncing; the run already in progress will finish it")]
    Busy(String),

    /// This folder's working copy was never finished (Story 56.15).
    ///
    /// `HEAD` holds a tree and the index holds nothing, which git itself never
    /// leaves behind: a checkout writes both together, and even `git rm -r .`
    /// writes an index full of removals rather than an index with no entries.
    /// The two ways in are a clone or checkout killed between the fetch and
    /// the index write, and a human deleting `.git/index`.
    ///
    /// It is a variant of its own — rather than a `Git(String)` — because the
    /// two things that must happen are opposite to everything else in this
    /// taxonomy: the folder must be **refused** (a status walk from that state
    /// reports every tracked path as deleted, so a commit made from it deletes
    /// the entire tree) and simultaneously **retried**, because the repair is
    /// mechanical and keeper owns it. `Transient` is what buys the retry;
    /// [`crate::engine::Engine::do_checkout`] is what makes the folder say so
    /// in the meantime, since a network cause would otherwise resolve to
    /// `Offline` with no error recorded at all — which is precisely the state
    /// the owner's machine was found in.
    ///
    /// `detail` carries the specifics, because the two shapes differ: a clone
    /// that never produced a repository at all, and a repository whose
    /// checkout stopped between the fetch and the index write. The head of the
    /// sentence is fixed because it is what is true of both, and it is what a
    /// person needs first: **this folder's first copy never finished.**
    #[error("{}: this folder's first copy never finished. {detail}", .path.display())]
    CheckoutUnfinished { path: PathBuf, detail: String },

    /// The operation was cancelled — by the user, by shutdown, or by a volume
    /// disappearing mid-flight. Never surfaced as a failure.
    #[error("operation cancelled")]
    Cancelled,
}

impl SyncError {
    /// Build an `Io` variant without repeating the struct literal at ~40 call
    /// sites. `operation` is a `&'static str` so it cannot accidentally carry
    /// interpolated user data.
    pub fn io(operation: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }

    /// How the scheduler should treat this failure (Story 26.6).
    pub fn retriability(&self) -> Retriability {
        match self {
            // The world may change on its own.
            Self::Network { .. } | Self::Quota { .. } | Self::GitCommand { .. } => {
                Retriability::Transient
            }
            // Integrity failures are retried because the *source* may be fine
            // and the transfer was the problem — but the staged prefix is
            // always discarded first, so this is a retry from zero, not a
            // resume from poisoned bytes.
            Self::Integrity { .. } => Retriability::Transient,
            // Waiting on an external condition, not on time: a volume that has
            // to come back, or an upload that has to land first.
            Self::MediaAbsent | Self::LfsUploadPending { .. } => Retriability::Deferred,
            // Someone must change something.
            Self::GitMissing { .. }
            | Self::Auth { .. }
            | Self::Forbidden { .. }
            | Self::InvalidPathForRemote { .. }
            | Self::Diverged { .. }
            | Self::Journal(_)
            | Self::Config(_) => Retriability::Permanent,
            // Ambiguous by nature; a bounded retry is cheaper than parking a
            // profile on a transient EINTR or a momentarily locked file.
            //
            // `RemoteMoved` joins them deliberately: another machine pushing
            // first is the ordinary weather of a shared branch, and the retry
            // lands after the reconcile that `do_push` queues alongside it.
            Self::Git(_) | Self::Io { .. } | Self::RemoteMoved { .. } => Retriability::Transient,
            // Nothing to retry: the run that holds the folder is doing this work.
            Self::Busy(_) | Self::Cancelled => Retriability::Permanent,
            // A refusal is an ANSWER, not a fault: the request named a path
            // whose bytes keeper will not change, and asking again unchanged
            // gets the same answer. A retry could only succeed by overwriting
            // the very thing the refusal protects.
            Self::Refused(_) => Retriability::Permanent,
            // Transient, and the classification IS the retry: the repair —
            // writing the `HEAD` paths the interrupted checkout never wrote —
            // is mechanical, owned by keeper, and needs nobody. Parking it
            // would leave a folder that downloads nothing until a human finds
            // a button, which is the failure this variant exists to end.
            Self::CheckoutUnfinished { .. } => Retriability::Transient,
        }
    }

    /// Whether this condition needs a human before the profile can progress.
    ///
    /// Drives AD-51's split between a passive amber warning and a notice with
    /// an inline action. Deliberately narrow: the product promise (FR-89) is
    /// that convergence never waits on a prompt, so only conditions no policy
    /// can decide are allowed to return `true`.
    ///
    /// [`Self::Refused`] is deliberately absent. It answers ONE request about
    /// ONE path and says nothing about the folder: a user who asked for a file
    /// they had edited has a working folder that needs nothing done to it, and
    /// returning `true` would raise the folder-needs-attention surface — an
    /// amber warning and a notice with an inline action — over a sentence the
    /// caller has already been shown.
    pub fn needs_user_action(&self) -> bool {
        matches!(
            self,
            Self::GitMissing { .. }
                | Self::Auth { .. }
                | Self::Forbidden { .. }
                | Self::InvalidPathForRemote { .. }
                | Self::Quota { .. }
                | Self::Diverged { .. }
        )
    }

    /// Stable machine-readable discriminant for IPC, CLI `--json` output and
    /// log fields. Kept separate from `Display` so prose can be reworded
    /// without breaking a consumer.
    pub fn code(&self) -> &'static str {
        match self {
            Self::GitMissing { .. } => "gitMissing",
            Self::GitCommand { .. } => "gitCommand",
            Self::Network { .. } => "network",
            Self::Auth { .. } => "auth",
            Self::Forbidden { .. } => "forbidden",
            Self::InvalidPathForRemote { .. } => "invalidPath",
            Self::MediaAbsent => "mediaAbsent",
            Self::LfsUploadPending { .. } => "lfsUploadPending",
            Self::Integrity { .. } => "integrity",
            Self::Quota { .. } => "quota",
            Self::Diverged { .. } => "diverged",
            Self::RemoteMoved { .. } => "remote-moved",
            Self::Journal(_) => "journal",
            Self::Git(_) => "git",
            Self::Io { .. } => "io",
            Self::Config(_) => "config",
            Self::Busy(_) => "busy",
            Self::Refused(_) => "refused",
            Self::CheckoutUnfinished { .. } => "checkoutUnfinished",
            Self::Cancelled => "cancelled",
        }
    }
}

impl From<rusqlite::Error> for SyncError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Journal(err.to_string())
    }
}

pub type Result<T, E = SyncError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_is_never_retried_unchanged() {
        // A retry storm against a git host gets an account rate-limited.
        let err = SyncError::Auth {
            host: "git.example".into(),
        };
        assert_eq!(err.retriability(), Retriability::Permanent);
        assert!(err.needs_user_action());
    }

    #[test]
    fn absent_media_is_deferred_not_failed() {
        // AD-48: an unplugged pendrive must never look like a failure, and must
        // never be retried on a timer — it waits for the volume.
        assert_eq!(
            SyncError::MediaAbsent.retriability(),
            Retriability::Deferred
        );
        assert!(!SyncError::MediaAbsent.needs_user_action());
    }

    #[test]
    fn network_backs_off_without_bothering_the_user() {
        let err = SyncError::Network {
            host: "git.example".into(),
            reason: "connection reset".into(),
        };
        assert_eq!(err.retriability(), Retriability::Transient);
        assert!(!err.needs_user_action());
    }

    #[test]
    fn codes_are_unique_across_variants() {
        // The code is a wire contract; two variants sharing one would make a
        // consumer's match silently wrong.
        let codes = [
            SyncError::GitMissing {
                reason: String::new(),
            }
            .code(),
            SyncError::GitCommand {
                subcommand: "push",
                code: 1,
                stderr: String::new(),
            }
            .code(),
            SyncError::Network {
                host: String::new(),
                reason: String::new(),
            }
            .code(),
            SyncError::Auth {
                host: String::new(),
            }
            .code(),
            SyncError::InvalidPathForRemote {
                path: PathBuf::new(),
                reason: String::new(),
            }
            .code(),
            SyncError::MediaAbsent.code(),
            SyncError::Integrity {
                subject: String::new(),
                expected: String::new(),
                actual: String::new(),
            }
            .code(),
            SyncError::Quota {
                host: String::new(),
            }
            .code(),
            SyncError::Diverged {
                profile: String::new(),
                reason: String::new(),
            }
            .code(),
            SyncError::Journal(String::new()).code(),
            SyncError::Git(String::new()).code(),
            SyncError::io("read", "/x", std::io::Error::other("x")).code(),
            SyncError::Config(String::new()).code(),
            SyncError::Refused(crate::lfs::hydrate::ContentRefusal::Missing {
                path: String::new(),
            })
            .code(),
            SyncError::Cancelled.code(),
        ];
        let mut seen = codes.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), codes.len(), "duplicate error code");
    }

    #[test]
    fn an_unsynchronizable_path_is_named_by_its_message() {
        // `record_failure` puts `to_string()` into the profile's warning and
        // nothing downstream destructures the variant, so a path missing from
        // the message is a path the user never learns.
        let err = SyncError::InvalidPathForRemote {
            path: PathBuf::from("20-records/pipe.fifo"),
            reason: "only regular files and symlinks can be synchronized".to_owned(),
        };
        assert!(
            err.to_string().contains("20-records/pipe.fifo"),
            "got: {err}"
        );
        assert!(err.needs_user_action());
    }

    #[test]
    fn io_errors_keep_the_path_that_failed() {
        // std::io::Error alone loses the path, which makes field reports useless.
        let err = SyncError::io(
            "open",
            "/tmp/keeper/thing.bin",
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        assert!(err.to_string().contains("/tmp/keeper/thing.bin"));
        assert!(err.to_string().contains("open"));
    }
}
