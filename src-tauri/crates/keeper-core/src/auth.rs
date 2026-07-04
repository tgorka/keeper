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
/// caller. See the module docs for the strict ordering and rollback contract.
pub async fn login_password(
    platform: &dyn Platform,
    homeserver: &str,
    username: &str,
    password: &str,
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

        client
            .matrix_auth()
            .login_username(username, password)
            .initial_device_display_name("keeper")
            .send()
            .await
            .map_err(|e| map_login_error(&e))?;

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

        registry::insert_account(
            &data_dir,
            &account_id,
            &user_id,
            resolved.as_str(),
            &device_id,
            now_ms(),
        )?;

        Ok::<AccountVm, CoreError>(AccountVm {
            account_id: account_id.clone(),
            user_id,
            homeserver_url: resolved.to_string(),
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
}
