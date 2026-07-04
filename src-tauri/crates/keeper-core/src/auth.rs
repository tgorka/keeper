//! Password login with Simplified Sliding Sync verification (FR-1, FR-5, AD-3).
//!
//! The full ordered flow: a **store-less** SSS capability probe runs *first*
//! (nothing is persisted); only if the homeserver supports Simplified Sliding
//! Sync (MSC4186) does keeper generate a ULID account id, open a persistent
//! SQLite store at `accounts/<ulid>/sdk/`, log in, store the session in the
//! macOS Keychain, and write one non-secret row into `keeper.db`.
//!
//! Any failure *after* the persistent store directory is created rolls back:
//! the store dir is removed, any Keychain entry is deleted, and no `keeper.db`
//! row is written — so a non-SSS/unreachable/rejected server leaves **zero**
//! persistent state and there is never a half-configured account.

use matrix_sdk::ruma::api::FeatureFlag;
use matrix_sdk::Client;
use ulid::Ulid;

use crate::error::{AuthError, CoreError};
use crate::platform::Platform;
use crate::registry;
use crate::vm::AccountVm;

/// The mechanism-specific credential→session step of an account add (AD-17).
///
/// Every login mechanism (password here; OIDC and Beeper in Stories 2.2/2.3)
/// implements exactly this narrow seam: given a freshly-built persistent
/// `Client`, authenticate it so that `client.matrix_auth().session()` is
/// populated afterwards. Everything else — the SSS capability gate, store-dir
/// creation, Keychain persistence, the registry row, hue assignment, and
/// rollback on failure — lives once in [`add_account`] and is shared across all
/// impls, so a new mechanism never re-implements the orchestration.
///
/// Kept intentionally free of `#[async_trait]` (no new dependency): native
/// `async fn` in traits suffices because [`add_account`] dispatches statically
/// over a concrete provider.
///
/// The error type is [`CoreError`] (not the narrower [`AuthError`]) so a
/// mechanism can surface a platform / internal failure faithfully; the password
/// impl only ever returns [`AuthError`] cases, which [`CoreError`] absorbs via
/// its `From` impl.
pub trait AuthProvider {
    /// Authenticate `client`. On success the client must carry a live session
    /// (`client.matrix_auth().session().is_some()`).
    fn authenticate(
        &self,
        client: &Client,
    ) -> impl std::future::Future<Output = Result<(), CoreError>> + Send;
}

/// The password `AuthProvider` — the first and, in Story 2.1, only impl.
///
/// Holds the transient username/password for exactly one login. The password is
/// never persisted, never returned, and never logged (only borrowed to drive the
/// SDK login), so a `PasswordAuthProvider` value is dropped as soon as the add
/// completes.
pub struct PasswordAuthProvider<'a> {
    /// The Matrix username (localpart or full user id).
    pub username: &'a str,
    /// The transient password — borrowed for the single login only.
    pub password: &'a str,
}

impl AuthProvider for PasswordAuthProvider<'_> {
    async fn authenticate(&self, client: &Client) -> Result<(), CoreError> {
        client
            .matrix_auth()
            .login_username(self.username, self.password)
            .initial_device_display_name("keeper")
            .send()
            .await
            .map_err(|e| map_login_error(&e))?;
        Ok(())
    }
}

/// Keychain key under which an account's serialized `MatrixSession` is stored.
///
/// Namespaced by account id so logout can delete exactly one account's secret.
pub fn session_keychain_key(account_id: &str) -> String {
    format!("session/{account_id}")
}

/// Map a matrix-sdk login error to the secret-free [`AuthError`] taxonomy.
///
/// An authentication rejection (`M_FORBIDDEN` / `M_UNAUTHORIZED`) means bad
/// credentials; an unknown/unsupported login type (`M_UNRECOGNIZED` or an
/// invalid-param rejection of the password flow) means password login is not
/// offered; anything without a client-API error kind (transport/DNS/connection)
/// is treated as unreachable.
fn map_login_error(err: &matrix_sdk::Error) -> CoreError {
    use matrix_sdk::ruma::api::error::ErrorKind;

    match err.client_api_error_kind() {
        Some(ErrorKind::Forbidden) | Some(ErrorKind::Unauthorized) => {
            AuthError::InvalidCredentials.into()
        }
        Some(ErrorKind::Unrecognized)
        | Some(ErrorKind::InvalidParam)
        | Some(ErrorKind::MissingParam) => AuthError::UnsupportedLoginType(
            "homeserver rejected the password login flow".to_owned(),
        )
        .into(),
        // A different server-reported errcode (rate limit, deactivated account,
        // …) is neither bad credentials nor a transport failure. Surface it as a
        // non-retriable internal error rather than the misleading, retriable
        // "couldn't reach that homeserver" copy.
        Some(_) => CoreError::Internal("homeserver returned an unexpected error".to_owned()),
        // No client-API error kind → transport/DNS/connection failure (retriable).
        None => AuthError::ServerUnreachable("could not complete login request".to_owned()).into(),
    }
}

/// Best-effort rollback of persistent state created during Phase B.
///
/// Removes the SDK store directory and deletes any Keychain entry that may have
/// been written. Both steps are best-effort: a missing Keychain entry is not an
/// error, and cleanup failures are logged but do not mask the original error.
fn rollback(platform: &dyn Platform, sdk_dir: &std::path::Path, keychain_key: &str) {
    if let Err(e) = std::fs::remove_dir_all(sdk_dir) {
        // ENOENT is fine (dir may not have been created yet); log others.
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(error = %e, "rollback: could not remove SDK store dir");
        }
    }
    if let Err(e) = platform.keychain_delete(keychain_key) {
        tracing::warn!(error = %e, "rollback: could not delete keychain entry");
    }
}

/// Perform a password login against `homeserver` for `username`/`password`.
///
/// On success returns a non-secret [`AccountVm`]; the session (access/refresh
/// tokens) is written only to the OS Keychain and never crosses back to the
/// caller. Adding the 2nd or the Nth account runs this identical path — nothing
/// here assumes a single account. A thin wrapper over the shared
/// [`add_account`] orchestration with the [`PasswordAuthProvider`] mechanism.
pub async fn login_password(
    platform: &dyn Platform,
    homeserver: &str,
    username: &str,
    password: &str,
) -> Result<AccountVm, CoreError> {
    let provider = PasswordAuthProvider { username, password };
    add_account(platform, homeserver, &provider).await
}

/// Shared add-account orchestration used by every [`AuthProvider`] mechanism
/// (AD-17). The strict ordering: store-less SSS capability gate (Phase A) →
/// persistent store dir → `provider.authenticate` → Keychain session → registry
/// row with a freshly assigned hue index (Phase B), rolling back the store dir
/// and Keychain entry on any Phase-B failure so a rejected/unreachable server
/// leaves zero residue. Never enforces an account-count limit, so the Nth add
/// is identical to the 2nd.
pub async fn add_account<P: AuthProvider>(
    platform: &dyn Platform,
    homeserver: &str,
    provider: &P,
) -> Result<AccountVm, CoreError> {
    // --- Phase A: store-less SSS probe (NOTHING persisted) --------------------
    // Default in-memory store (no `.sqlite_store`), so a non-SSS or unreachable
    // server leaves zero state on disk.
    let probe = Client::builder()
        .server_name_or_homeserver_url(homeserver)
        .build()
        .await
        .map_err(|e| AuthError::ServerUnreachable(e.to_string()))?;

    // Query `/versions` directly. Do NOT use `available_sliding_sync_versions()`:
    // it swallows a transport error into an empty result (see its docs: "If
    // `.well-known` or `/versions` is unreachable, it will simply move potential
    // sliding sync versions aside. No error will be reported."), which would
    // mislabel an unreachable/flaky server as permanently non-SSS. Instead: a
    // transport failure here → ServerUnreachable (retriable); a reachable server
    // that genuinely lacks MSC4186 → SlidingSyncUnsupported.
    let supported = probe
        .supported_versions()
        .await
        .map_err(|e| AuthError::ServerUnreachable(e.to_string()))?;
    if !supported.features.contains(&FeatureFlag::Msc4186) {
        tracing::info!(sss_supported = false, "SSS probe: homeserver lacks MSC4186");
        return Err(AuthError::SlidingSyncUnsupported.into());
    }
    tracing::info!(
        sss_supported = true,
        "SSS probe: homeserver supports MSC4186"
    );

    // Reuse the discovered homeserver URL so discovery runs exactly once.
    let resolved = probe.homeserver();
    drop(probe);

    // --- Phase B: persistent account (rollback on any failure below) ---------
    let account_id = Ulid::new().to_string();
    let data_dir = platform.data_dir()?;
    let sdk_dir = data_dir.join("accounts").join(&account_id).join("sdk");
    let keychain_key = session_keychain_key(&account_id);

    // From this point on, persistent state may exist; wrap failures in rollback.
    let result = async {
        let client = Client::builder()
            .homeserver_url(resolved.clone())
            .sqlite_store(&sdk_dir, None)
            .build()
            .await
            .map_err(|e| CoreError::Auth(AuthError::ServerUnreachable(e.to_string())))?;

        // Mechanism-specific credential→session step (AD-17).
        provider.authenticate(&client).await?;

        let session = client
            .matrix_auth()
            .session()
            .ok_or_else(|| CoreError::Internal("no session after successful login".to_owned()))?;

        let user_id = session.meta.user_id.to_string();
        let device_id = session.meta.device_id.to_string();

        // Persist the session only to the Keychain (never to keeper.db / IPC).
        let session_json = serde_json::to_string(&session)
            .map_err(|e| CoreError::Internal(format!("could not serialize session: {e}")))?;
        platform.keychain_set(&keychain_key, &session_json)?;

        // Assign the lowest unused hue on the 8-hue wheel (else count % 8) and
        // persist it with the registry row so it is stable across restarts.
        let hue_index = registry::next_hue_index(&data_dir)?;
        registry::insert_account(
            &data_dir,
            &account_id,
            &user_id,
            resolved.as_str(),
            &device_id,
            now_ms(),
            hue_index,
        )?;

        Ok::<AccountVm, CoreError>(AccountVm {
            account_id: account_id.clone(),
            user_id,
            homeserver_url: resolved.to_string(),
            hue_index,
        })
    }
    .await;

    match result {
        Ok(vm) => {
            tracing::info!(account_id = %account_id, "login succeeded; account persisted");
            Ok(vm)
        }
        Err(err) => {
            tracing::warn!(account_id = %account_id, "login failed; rolling back persistent state");
            rollback(platform, &sdk_dir, &keychain_key);
            Err(err)
        }
    }
}

/// Find every persisted account that can be restored on launch (FR-8, AD-20).
///
/// Lists the non-secret registry rows and returns each whose Keychain session
/// (`session/<id>`) is still present, built as a non-secret [`AccountVm`]
/// (opaque account id, Matrix user id, homeserver URL, hue index) from its row.
/// A registry row **without** a Keychain session is *not* restorable — it is
/// skipped, not fatal, so a half-torn-down account never blocks the others.
/// Identity only: this does not activate any account or touch the SDK store (the
/// lazy inbox/room-list subscribe restores each session). A legacy row whose
/// `hue_index` is still `NULL` is backfilled here so every returned VM carries a
/// stable hue. Returns accounts in registry (creation) order; the merged inbox
/// re-orders their rooms by recency.
pub fn find_restorable_accounts(platform: &dyn Platform) -> Result<Vec<AccountVm>, CoreError> {
    let data_dir = platform.data_dir()?;
    let mut restorable = Vec::new();
    for row in registry::list_accounts(&data_dir)? {
        if platform
            .keychain_get(&session_keychain_key(&row.account_id))?
            .is_none()
        {
            tracing::info!(
                account_id = %row.account_id,
                "registry row has no keychain session; skipping as not restorable"
            );
            continue;
        }
        // Backfill a legacy NULL hue in place so the VM always carries one.
        let hue_index = match row.hue_index {
            Some(hue) => hue,
            None => registry::backfill_hue_index(&data_dir, &row.account_id)?,
        };
        restorable.push(AccountVm {
            account_id: row.account_id,
            user_id: row.user_id,
            homeserver_url: row.homeserver_url,
            hue_index,
        });
    }
    Ok(restorable)
}

/// Delete exactly one account's persisted state — its SDK store dir, its Keychain
/// session entry, and its `keeper.db` registry row — for local sign-out (AD-10).
///
/// Mirrors the private [`rollback`] cleanup, adding the registry-row delete
/// (sign-out runs *after* the row was written, so removing it is what makes "no
/// residual session on relaunch" true). Each step is idempotent / best-effort and
/// tolerates already-absent state: a missing dir (`NotFound`), a missing Keychain
/// entry, and a missing row are all non-errors, so a partial prior sign-out or an
/// account that was never activated both converge cleanly. Touches *only* this
/// account's state — nothing belonging to another account.
pub fn sign_out_cleanup(platform: &dyn Platform, account_id: &str) -> Result<(), CoreError> {
    let data_dir = platform.data_dir()?;

    // Delete the two keys `find_restorable_account` relies on FIRST — the registry
    // row, then the Keychain session — propagating their (rare) errors. Removing
    // either one already makes the account non-restorable, so even if the store-dir
    // removal below fails, the user is never left with a "restorable" ghost (row +
    // session present, store gone) that lands them on a broken shell on relaunch.
    registry::delete_account(&data_dir, account_id)?;
    platform.keychain_delete(&session_keychain_key(account_id))?;

    // Store-dir removal is best-effort and LAST: a transient failure here (e.g. a
    // file lock) must not resurrect a restorable account, so — like `rollback` — we
    // log and swallow it rather than propagate. A missing dir is expected (never
    // activated, or a partial prior sign-out).
    let sdk_dir = data_dir.join("accounts").join(account_id).join("sdk");
    if let Err(e) = std::fs::remove_dir_all(&sdk_dir) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                account_id = %account_id,
                error = %e,
                "sign-out: could not remove SDK store dir (orphaned; account already non-restorable)"
            );
        }
    }

    tracing::info!(account_id = %account_id, "signed out: persisted account state deleted");
    Ok(())
}

/// Current wall-clock time in milliseconds since the Unix epoch (UTC).
fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    #[test]
    fn keychain_key_is_namespaced_by_account() {
        assert_eq!(
            session_keychain_key("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            "session/01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
    }

    /// Fake platform that records the keys passed to `keychain_delete`, so the
    /// rollback tests can assert the session secret is cleaned up.
    #[derive(Default)]
    struct RecordingPlatform {
        deleted: Mutex<Vec<String>>,
    }

    impl Platform for RecordingPlatform {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Ok(std::env::temp_dir())
        }
        fn keychain_set(&self, _key: &str, _value: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn keychain_get(&self, _key: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
        fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
            self.deleted
                .lock()
                .expect("lock poisoned")
                .push(key.to_owned());
            Ok(())
        }
        fn notify(&self, _title: &str, _body: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn sidecar_path(&self, _name: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("sidecar unused in tests".to_owned()))
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-auth-test-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        dir
    }

    #[test]
    fn rollback_removes_store_dir_and_deletes_keychain_entry() {
        let platform = RecordingPlatform::default();
        let sdk_dir = temp_dir("rollback");
        std::fs::create_dir_all(sdk_dir.join("sub")).expect("create store dir");
        std::fs::write(sdk_dir.join("sub").join("f"), b"x").expect("write file");
        assert!(sdk_dir.exists());

        rollback(&platform, &sdk_dir, "session/01ARZ3NDEKTSV4RRFFQ69G5FAV");

        assert!(!sdk_dir.exists(), "store dir should be removed by rollback");
        assert_eq!(
            platform.deleted.lock().expect("lock poisoned").as_slice(),
            ["session/01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned()],
            "rollback must delete exactly the account's keychain entry"
        );
    }

    #[test]
    fn rollback_of_missing_store_dir_is_silent_and_still_clears_keychain() {
        let platform = RecordingPlatform::default();
        let sdk_dir = temp_dir("rollback-missing");
        // Directory never created: rollback must not panic and must still attempt
        // the keychain cleanup (a missing dir is not an error).
        rollback(&platform, &sdk_dir, "session/x");
        assert_eq!(platform.deleted.lock().expect("lock poisoned").len(), 1);
    }

    /// Fake platform with a fixed data dir and an in-memory keychain map, so the
    /// restore/cleanup tests can drive registry rows + keychain entries together.
    struct FakePlatform {
        data_dir: PathBuf,
        keychain: Mutex<std::collections::HashMap<String, String>>,
    }

    impl FakePlatform {
        fn new(data_dir: PathBuf) -> Self {
            Self {
                data_dir,
                keychain: Mutex::new(std::collections::HashMap::new()),
            }
        }
    }

    impl Platform for FakePlatform {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Ok(self.data_dir.clone())
        }
        fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
            self.keychain
                .lock()
                .expect("lock poisoned")
                .insert(key.to_owned(), value.to_owned());
            Ok(())
        }
        fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
            Ok(self
                .keychain
                .lock()
                .expect("lock poisoned")
                .get(key)
                .cloned())
        }
        fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
            self.keychain.lock().expect("lock poisoned").remove(key);
            Ok(())
        }
        fn notify(&self, _title: &str, _body: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn sidecar_path(&self, _name: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("sidecar unused in tests".to_owned()))
        }
    }

    #[test]
    fn find_restorable_accounts_empty_on_empty_registry() {
        let platform = FakePlatform::new(temp_dir("find-empty"));
        let found = find_restorable_accounts(&platform).expect("find should succeed");
        assert!(found.is_empty(), "empty registry has nothing to restore");
        let _ = std::fs::remove_dir_all(&platform.data_dir);
    }

    #[test]
    fn find_restorable_accounts_returns_row_with_present_session() {
        let platform = FakePlatform::new(temp_dir("find-present"));
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        registry::insert_account(
            &platform.data_dir,
            id,
            "@alice:example.org",
            "https://matrix.example.org/",
            "DEVID",
            1,
            3,
        )
        .expect("insert row");
        platform
            .keychain_set(&session_keychain_key(id), "opaque-session-json")
            .expect("set session");

        let vms = find_restorable_accounts(&platform).expect("find should succeed");
        assert_eq!(vms.len(), 1);
        assert_eq!(vms[0].account_id, id);
        assert_eq!(vms[0].user_id, "@alice:example.org");
        assert_eq!(vms[0].homeserver_url, "https://matrix.example.org/");
        assert_eq!(vms[0].hue_index, 3);
        let _ = std::fs::remove_dir_all(&platform.data_dir);
    }

    #[test]
    fn find_restorable_accounts_returns_all_and_skips_row_without_session() {
        let platform = FakePlatform::new(temp_dir("find-multi"));
        // Two accounts with sessions, one without — the sessionless one is
        // skipped (not fatal), the other two are both returned.
        for (id, hue) in [
            ("01ARZ3NDEKTSV4RRFFQ69G5FAV", 0u8),
            ("01BX5ZZKBKACTAV9WEVGEMMVRZ", 1u8),
        ] {
            registry::insert_account(
                &platform.data_dir,
                id,
                "@u:example.org",
                "https://matrix.example.org/",
                "DEV",
                1,
                hue,
            )
            .expect("insert row");
            platform
                .keychain_set(&session_keychain_key(id), "session-json")
                .expect("set session");
        }
        // A third row with no keychain session.
        registry::insert_account(
            &platform.data_dir,
            "01CX5ZZKBKACTAV9WEVGEMMVRZ",
            "@c:example.org",
            "https://matrix.example.org/",
            "DEV",
            2,
            2,
        )
        .expect("insert sessionless row");

        let vms = find_restorable_accounts(&platform).expect("find should succeed");
        assert_eq!(vms.len(), 2, "both accounts with sessions are restorable");
        let ids: Vec<&str> = vms.iter().map(|v| v.account_id.as_str()).collect();
        assert!(ids.contains(&"01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert!(ids.contains(&"01BX5ZZKBKACTAV9WEVGEMMVRZ"));
        assert!(!ids.contains(&"01CX5ZZKBKACTAV9WEVGEMMVRZ"));
        let _ = std::fs::remove_dir_all(&platform.data_dir);
    }

    #[test]
    fn find_restorable_accounts_backfills_legacy_null_hue() {
        let platform = FakePlatform::new(temp_dir("find-legacy-hue"));
        let dir = &platform.data_dir;
        std::fs::create_dir_all(dir).expect("create dir");
        // Simulate a legacy row (NULL hue) by inserting via a pre-hue schema path:
        // insert normally then null the column out.
        registry::insert_account(dir, "legacy", "@l:e.org", "https://e.org/", "DEV", 1, 0)
            .expect("insert row");
        {
            let conn = rusqlite::Connection::open(dir.join("keeper.db")).expect("open db");
            conn.execute(
                "UPDATE accounts SET hue_index = NULL WHERE account_id = 'legacy'",
                [],
            )
            .expect("null hue");
        }
        platform
            .keychain_set(&session_keychain_key("legacy"), "session-json")
            .expect("set session");

        let vms = find_restorable_accounts(&platform).expect("find should succeed");
        assert_eq!(vms.len(), 1);
        assert_eq!(vms[0].hue_index, 0, "legacy NULL hue is backfilled");
        // Persisted afterwards.
        let row = registry::get_account(dir, "legacy")
            .expect("get")
            .expect("row");
        assert_eq!(row.hue_index, Some(0));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sign_out_cleanup_deletes_exactly_the_three_targets() {
        let platform = FakePlatform::new(temp_dir("cleanup-exact"));
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let key = session_keychain_key(id);

        // Seed all three persisted targets plus an unrelated sibling account that
        // must remain untouched (AD-10: nothing else).
        registry::insert_account(
            &platform.data_dir,
            id,
            "@alice:example.org",
            "https://matrix.example.org/",
            "DEVID",
            1,
            0,
        )
        .expect("insert row");
        registry::insert_account(
            &platform.data_dir,
            "01BX5ZZKBKACTAV9WEVGEMMVRZ",
            "@bob:example.org",
            "https://matrix.example.org/",
            "DEVID2",
            2,
            1,
        )
        .expect("insert sibling row");
        platform
            .keychain_set(&key, "session-json")
            .expect("set session");
        platform
            .keychain_set("session/01BX5ZZKBKACTAV9WEVGEMMVRZ", "sibling-session")
            .expect("set sibling session");
        let sdk_dir = platform.data_dir.join("accounts").join(id).join("sdk");
        std::fs::create_dir_all(sdk_dir.join("sub")).expect("create sdk dir");
        std::fs::write(sdk_dir.join("sub").join("f"), b"x").expect("write file");
        let sibling_sdk = platform
            .data_dir
            .join("accounts")
            .join("01BX5ZZKBKACTAV9WEVGEMMVRZ")
            .join("sdk");
        std::fs::create_dir_all(&sibling_sdk).expect("create sibling sdk dir");

        sign_out_cleanup(&platform, id).expect("cleanup should succeed");

        // This account's three targets are gone.
        assert!(!sdk_dir.exists(), "sdk dir should be removed");
        assert!(
            platform.keychain_get(&key).expect("get").is_none(),
            "keychain session should be deleted"
        );
        assert!(
            registry::get_account(&platform.data_dir, id)
                .expect("get row")
                .is_none(),
            "registry row should be deleted"
        );

        // The sibling account's state is untouched.
        assert!(sibling_sdk.exists(), "sibling sdk dir must remain");
        assert!(
            platform
                .keychain_get("session/01BX5ZZKBKACTAV9WEVGEMMVRZ")
                .expect("get sibling")
                .is_some(),
            "sibling keychain session must remain"
        );
        assert!(
            registry::get_account(&platform.data_dir, "01BX5ZZKBKACTAV9WEVGEMMVRZ")
                .expect("get sibling row")
                .is_some(),
            "sibling registry row must remain"
        );

        let _ = std::fs::remove_dir_all(&platform.data_dir);
    }

    #[test]
    fn sign_out_cleanup_is_idempotent_when_absent() {
        let platform = FakePlatform::new(temp_dir("cleanup-absent"));
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        // Nothing was ever persisted for this account: cleanup must still succeed.
        sign_out_cleanup(&platform, id).expect("cleanup of absent state should be ok");
        // And a second call is likewise a no-op.
        sign_out_cleanup(&platform, id).expect("second cleanup should be ok");
        let _ = std::fs::remove_dir_all(&platform.data_dir);
    }
}
