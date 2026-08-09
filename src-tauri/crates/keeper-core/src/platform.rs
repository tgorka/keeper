//! Platform port trait (AD-24).
//!
//! `keeper-core` reaches the OS only through this port. The concrete
//! implementation lives in the `keeper` shell crate, keeping the hexagon free
//! of any platform- or tauri-specific dependency. Story 1.1 wires the data-dir
//! port end-to-end; the remaining ports (keychain, notifier, sidecar) are
//! declared here and return [`CoreError::Unsupported`] from the shell impl for
//! now — honest and non-panicking — until later stories fill them.
//!
//! [`SecretCache`] lives here too, beside the keychain port it memoizes: it is
//! pure in-memory policy (no OS call of its own), so it belongs in the hexagon
//! and is testable without a keychain, while the shell owns the single instance
//! that sits in front of the real one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::error::CoreError;
use crate::vm::NotifyTarget;

/// Injected platform capabilities the core depends on.
///
/// Implementations must be `Send + Sync` so they can be shared across the
/// account-supervision tasks introduced in later stories.
pub trait Platform: Send + Sync {
    /// Root directory for keeper's own persistent data (databases, account
    /// stores). Fully wired in Story 1.1.
    fn data_dir(&self) -> Result<PathBuf, CoreError>;

    /// Store a secret (e.g. an access token) in the OS keychain under `key`.
    /// Wired in Story 1.3 via the shell's `DesktopPlatform` (macOS Keychain,
    /// service `dev.tgorka.keeper`, backed by `keyring`).
    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError>;

    /// Retrieve a secret previously stored under `key`, if present.
    /// Wired in Story 1.3 via the shell's `DesktopPlatform` (macOS Keychain).
    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError>;

    /// Delete a secret stored under `key`.
    /// Wired in Story 1.3 via the shell's `DesktopPlatform` (macOS Keychain).
    fn keychain_delete(&self, key: &str) -> Result<(), CoreError>;

    /// Open `url` in the user's default system browser (Story 2.2, OIDC login).
    ///
    /// Used by the OIDC `AuthProvider` to present the OAuth authorization URL for
    /// browser consent. Kept as a port so `keeper-core` stays tauri-free — the
    /// concrete browser-open lives in the `keeper` shell.
    fn open_url(&self, url: &str) -> Result<(), CoreError>;

    /// Post a desktop notification (title + body) carrying a typed click-through
    /// [`NotifyTarget`] (Story 10.4, FR-51). The kept desktop backend has no
    /// per-notification click callback, so the shell records `target` as the "last
    /// notification target" at dispatch and drives a coarse view landing on app
    /// activation (Message → Inbox, Bridge → Bridges); it must NEVER be presented as
    /// exact-message routing. Not wired in Story 1.1 — returns [`CoreError::Unsupported`].
    fn notify(&self, title: &str, body: &str, target: &NotifyTarget) -> Result<(), CoreError>;

    /// Resolve the path to a bundled sidecar binary by logical `name`.
    /// Not wired in Story 1.1 — returns [`CoreError::Unsupported`].
    fn sidecar_path(&self, name: &str) -> Result<PathBuf, CoreError>;

    /// Exclude `path` (a file or directory under [`Platform::data_dir`]) from OS
    /// device backups (Story 14.7, FR-65).
    ///
    /// On iOS this sets `NSURLIsExcludedFromBackupKey` on the file/directory URL
    /// so the path never reaches iCloud/iTunes device backups; **directory-level
    /// exclusion covers the whole subtree**, which is how the SQLite `-wal`/`-shm`
    /// sidecars next to each `.db` are kept out of backup (callers flag the
    /// containing directory, never a bare `.db` file). On desktop there is no
    /// equivalent per-path backup-exclusion concept, so the port is an honest
    /// no-op returning `Ok(())`. Callers pass absolute, already-created paths
    /// rooted under `data_dir`, and must treat a failure as best-effort hardening
    /// to log and swallow — never a reason to abort startup, login, or restore.
    fn exclude_from_backup(&self, path: &Path) -> Result<(), CoreError>;

    /// Set (or clear) the OS dock badge (Story 10.3, FR-53). `Some(n)` shows the
    /// count `n`; `None` clears the badge. Driven from the Rust-computed
    /// cross-account unread/mention aggregate so it stays correct while the window
    /// is hidden — the count is never derived in the webview. The desktop shell
    /// wires this to the main window's badge; when no app handle is available
    /// (headless / tests) it is an honest no-op, never a panic.
    fn set_badge_count(&self, count: Option<u32>) -> Result<(), CoreError>;
}

/// Best-effort [`Platform::exclude_from_backup`]: log-and-continue on failure
/// (Story 14.7, FR-65).
///
/// Backup exclusion is privacy hardening, not a correctness precondition — a
/// failure must never panic the app, abort login or session-restore, or abort
/// the archive path (which is invariant-bound to never abort startup). Every
/// store-creation site calls through this funnel so no exclusion error can be
/// `?`-propagated into a fatal path.
pub(crate) fn exclude_from_backup_best_effort(platform: &dyn Platform, path: &Path) {
    if let Err(error) = platform.exclude_from_backup(path) {
        tracing::warn!(
            path = %path.display(),
            %error,
            "could not exclude path from device backup; continuing (best-effort hardening)"
        );
    }
}

/// Secrets this process has already read out of the OS keychain, held for the
/// lifetime of the process.
///
/// # The failure this prevents
///
/// macOS evaluates a keychain item's ACL on every read that *returns data*, so
/// each read of an item the calling binary is not yet trusted for raises the
/// "keeper wants to use your confidential information stored in
/// dev.tgorka.keeper in your keychain" dialog — once per item **per read**, not
/// once per item. Reading one item twice in a launch was therefore two dialogs,
/// and `sync/<profile>/credential` was read on every push *and* every fetch, so
/// on a machine that syncs continuously the dialog kept coming back all session
/// long even after the user clicked "Always Allow". Memoizing the first read
/// makes it one dialog per distinct item per process, which is the floor
/// reachable without changing which items exist.
///
/// # Absence is held too
///
/// A `None` is remembered like a value: an optional secret — a sync profile on
/// a public remote has no credential — re-queried on every call is the same
/// defect one layer down, a syscall per call instead of a dialog per call. The
/// cost is that an item added by **another** process (Keychain Access, a second
/// keeper, `keeper-syncd`) stays invisible until relaunch. That is the right
/// trade: every writer inside this process goes through the same seam and so
/// through [`SecretCache::invalidate`], which means the only loser is a case
/// nobody has.
///
/// # Invalidation is not optional
///
/// Every `set` and `delete` on the seam in front of this cache MUST call
/// [`SecretCache::invalidate`]. A corrected password that keeps failing
/// authentication until the app is relaunched would be a worse bug than the
/// dialog this removes.
///
/// Held values never leave through `Debug` (hand-written below, NFR-26/AD-53),
/// are never logged, and never appear in an error.
pub struct SecretCache {
    /// `key` → the answer the OS gave, where `None` is a remembered absence.
    /// Behind the same lock as `generation` so an invalidation that races an
    /// in-flight read cannot be undone by it.
    state: Mutex<CacheState>,
}

/// [`SecretCache`]'s contents. A struct rather than two fields so the memo and
/// the counter guarding it are always taken under one lock, never two.
struct CacheState {
    /// The memo. Keys are non-secret (`session/<id>`, `sync/<id>/credential`);
    /// the values are the secrets, which is why nothing here is printable.
    held: HashMap<String, Option<String>>,
    /// Bumped by every [`SecretCache::invalidate`]. A read that began before
    /// the bump discards its answer instead of re-seating it — without this,
    /// the lost update would pin the pre-correction credential for the rest of
    /// the process, which is precisely the "keeps failing auth until relaunch"
    /// bug invalidation exists to prevent.
    generation: u64,
}

impl SecretCache {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(CacheState {
                held: HashMap::new(),
                generation: 0,
            }),
        }
    }

    /// Return the held answer for `key`, else call `read` — the real keychain
    /// lookup — and hold whatever it says.
    ///
    /// `read` runs with the lock **released**. A keychain read can block on a
    /// modal authorization dialog, and holding a process-wide lock across a
    /// window the user has not answered yet would stall every unrelated secret
    /// read behind it. The cost is that two callers racing the same cold key
    /// may each read once — no worse than the unmemoized behaviour they
    /// replace, and the steady state is still one.
    ///
    /// A failed read is not held: an error describes this attempt, not the
    /// item, and remembering it would turn one transient keychain failure into
    /// a dead secret for the rest of the process.
    pub fn read_through(
        &self,
        key: &str,
        read: impl FnOnce() -> Result<Option<String>, CoreError>,
    ) -> Result<Option<String>, CoreError> {
        let generation = {
            let state = self.lock();
            if let Some(held) = state.held.get(key) {
                return Ok(held.clone());
            }
            state.generation
        };
        let fresh = read()?;
        let mut state = self.lock();
        if state.generation == generation {
            state.held.insert(key.to_owned(), fresh.clone());
        }
        Ok(fresh)
    }

    /// Drop the held answer for `key` so the next [`SecretCache::read_through`]
    /// asks the OS again.
    ///
    /// Call this *after* every write and delete of `key`, and regardless of
    /// whether the write reported success: a write that failed part-way may
    /// still have replaced the stored item, and continuing to hand out the
    /// pre-write value would spend a credential the user has already changed.
    pub fn invalidate(&self, key: &str) {
        let mut state = self.lock();
        state.held.remove(key);
        state.generation = state.generation.wrapping_add(1);
    }

    /// Lock helper that recovers a poisoned mutex, mirroring
    /// [`crate::oauth::OAuthFlowRegistry`]'s: a panic under this lock would only
    /// have left a fully-formed map behind, and failing every later secret read
    /// over it would take login, restore and sync down for nothing.
    fn lock(&self) -> MutexGuard<'_, CacheState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for SecretCache {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SecretCache {
    /// Hand-written for the reason `keeper_sync::credential::AccessToken`'s is:
    /// a derived `Debug` over held secrets is how tokens reach log files
    /// (NFR-26, AD-53). Only the number of held entries is printable — never a
    /// key, never a value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let held = self.state.lock().map(|s| s.held.len()).unwrap_or(0);
        f.debug_struct("SecretCache").field("held", &held).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A stand-in for the OS keychain that counts how many times it was asked.
    /// The dialog count the user sees is exactly this count, so it is the only
    /// thing worth asserting on.
    struct FakeKeychain {
        answer: Option<String>,
        reads: AtomicUsize,
    }

    impl FakeKeychain {
        fn holding(secret: &str) -> Self {
            Self {
                answer: Some(secret.to_owned()),
                reads: AtomicUsize::new(0),
            }
        }

        fn empty() -> Self {
            Self {
                answer: None,
                reads: AtomicUsize::new(0),
            }
        }

        fn read(&self) -> Result<Option<String>, CoreError> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            Ok(self.answer.clone())
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::Relaxed)
        }
    }

    /// The contract: however many times a key is asked for, the OS is asked
    /// once. Two reads of one item are two authorization dialogs, so this
    /// assertion *is* the prompt count.
    #[test]
    fn one_key_reaches_the_os_exactly_once() {
        let cache = SecretCache::new();
        let os = FakeKeychain::holding("session-blob");

        for _ in 0..5 {
            assert_eq!(
                cache
                    .read_through("session/acct", || os.read())
                    .expect("read succeeds")
                    .as_deref(),
                Some("session-blob"),
                "every call must answer with the same secret"
            );
        }
        assert_eq!(os.reads(), 1, "five asks, one dialog");
    }

    /// Absence is memoized like a value — an optional secret (a public remote
    /// has no credential) must not re-query on every call.
    #[test]
    fn absence_is_held_like_a_value() {
        let cache = SecretCache::new();
        let os = FakeKeychain::empty();

        assert_eq!(
            cache
                .read_through("sync/p1/credential", || os.read())
                .expect("read succeeds"),
            None
        );
        assert_eq!(
            cache
                .read_through("sync/p1/credential", || os.read())
                .expect("read succeeds"),
            None
        );
        assert_eq!(os.reads(), 1, "a remembered absence is not re-queried");
    }

    /// The invalidation contract: a corrected password takes effect without a
    /// relaunch. Without this the cache would pin the old credential and keep
    /// failing authentication — worse than the dialog it removes.
    #[test]
    fn invalidate_lets_a_corrected_secret_take_effect() {
        let cache = SecretCache::new();
        let stale = FakeKeychain::holding("wrong-password");
        let corrected = FakeKeychain::holding("right-password");

        assert_eq!(
            cache
                .read_through("sync/p1/credential", || stale.read())
                .expect("read succeeds")
                .as_deref(),
            Some("wrong-password")
        );
        cache.invalidate("sync/p1/credential");
        assert_eq!(
            cache
                .read_through("sync/p1/credential", || corrected.read())
                .expect("read succeeds")
                .as_deref(),
            Some("right-password"),
            "the write invalidated the entry, so the next read sees the new secret"
        );
    }

    /// An invalidation that lands *while* a read is in flight must win. `read`
    /// runs with the lock released, so calling `invalidate` from inside it is
    /// exactly the interleaving a concurrent `sync_set_credential` produces —
    /// and re-seating the in-flight answer would pin the pre-correction
    /// credential for the rest of the process.
    #[test]
    fn an_invalidation_racing_a_read_discards_the_stale_answer() {
        let cache = SecretCache::new();

        let raced = cache
            .read_through("sync/p1/credential", || {
                cache.invalidate("sync/p1/credential");
                Ok(Some("pre-correction".to_owned()))
            })
            .expect("read succeeds");
        assert_eq!(
            raced.as_deref(),
            Some("pre-correction"),
            "this attempt still returns what the OS handed it"
        );

        let corrected = FakeKeychain::holding("post-correction");
        assert_eq!(
            cache
                .read_through("sync/p1/credential", || corrected.read())
                .expect("read succeeds")
                .as_deref(),
            Some("post-correction"),
            "the raced answer must not have been held"
        );
        assert_eq!(corrected.reads(), 1);
    }

    /// A failed read describes the attempt, not the item: holding it would turn
    /// one transient keychain failure into a secret that stays dead until
    /// relaunch.
    #[test]
    fn a_failed_read_is_not_held() {
        let cache = SecretCache::new();

        let failed = cache.read_through("session/acct", || {
            Err(CoreError::Internal("keychain unavailable".to_owned()))
        });
        assert!(failed.is_err(), "the failure is reported, not swallowed");

        let os = FakeKeychain::holding("session-blob");
        assert_eq!(
            cache
                .read_through("session/acct", || os.read())
                .expect("the retry succeeds")
                .as_deref(),
            Some("session-blob"),
            "the next call must reach the OS again"
        );
    }

    /// Entries are per key: one account's session must never answer another's,
    /// and each still costs exactly one dialog.
    #[test]
    fn keys_are_independent() {
        let cache = SecretCache::new();
        let a = FakeKeychain::holding("session-a");
        let b = FakeKeychain::holding("session-b");

        assert_eq!(
            cache
                .read_through("session/a", || a.read())
                .expect("read a")
                .as_deref(),
            Some("session-a")
        );
        assert_eq!(
            cache
                .read_through("session/b", || b.read())
                .expect("read b")
                .as_deref(),
            Some("session-b")
        );
        assert_eq!(
            cache
                .read_through("session/a", || a.read())
                .expect("re-read a")
                .as_deref(),
            Some("session-a")
        );
        assert_eq!(a.reads(), 1);
        assert_eq!(b.reads(), 1);
        // And invalidating one leaves the other held.
        cache.invalidate("session/a");
        let _ = cache.read_through("session/a", || a.read()).expect("a again");
        let _ = cache.read_through("session/b", || b.read()).expect("b again");
        assert_eq!(a.reads(), 2, "the invalidated key is re-read");
        assert_eq!(b.reads(), 1, "its neighbour is untouched");
    }

    /// NFR-26/AD-53: the hand-written `Debug` must be incapable of printing a
    /// held secret or even a key. A derived one would print both.
    #[test]
    fn debug_withholds_every_held_secret() {
        let cache = SecretCache::new();
        let os = FakeKeychain::holding("super-secret-token");
        let _ = cache
            .read_through("sync/p1/credential", || os.read())
            .expect("read succeeds");

        let rendered = format!("{cache:?}");
        assert!(
            !rendered.contains("super-secret-token"),
            "leaked a secret: {rendered}"
        );
        assert!(
            !rendered.contains("sync/p1/credential"),
            "leaked a key: {rendered}"
        );
        assert!(
            rendered.contains("held: 1"),
            "the count is the only printable fact: {rendered}"
        );
    }
}
