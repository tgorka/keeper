//! What the Android build knows about the device it runs on, and the
//! Keystore half of its secret store (Epic 98, story 98.3; AD-414).
//!
//! # Facts from `MainActivity`
//!
//! `AppState::new` runs inside `run()`, before Tauri has an app handle or a
//! plugin to ask — and on Android `run()` starts from `Rust.create()`, which
//! the activity's `super.onCreate` triggers. So `MainActivity.onCreate`
//! (`gen/android/app/src/main/java/dev/tgorka/keeper/MainActivity.kt`) writes
//! the three facts keeper needs that early into the process environment with
//! `android.system.Os.setenv` *before* `super.onCreate`: the app's private
//! files directory, the device class and the model. They are not secrets.
//!
//! # The data key
//!
//! The secrets themselves are `keeper_core::secret_file`'s: one AEAD file in
//! the data directory. Its key comes from `KeystorePlugin.kt`, a Tauri plugin
//! in the app module that generates a random data key once, keeps it only
//! wrapped by a non-exportable AES key in the Android Keystore, and unwraps
//! it on request. [`KeystoreKey`] asks once per process and holds the answer.
//! It is never `keyring`, which off Apple and desktop Linux is a mock store
//! that forgets every secret at exit (`check:android-graph` keeps it out of
//! the Android build altogether).

#![cfg(target_os = "android")]

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};

use keeper_core::error::CoreError;
use keeper_core::org_account::layout::DeviceClass;
use keeper_core::secret_file::{key_from_hex, KeyProvider, KEY_LEN};
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::Wry;

/// The app's private files directory (`Context.getFilesDir()`).
const FILES_DIR_ENV: &str = "KEEPER_ANDROID_FILES_DIR";
/// `tablet` when the smallest screen width is at least 600 dp, else `mobile`.
const DEVICE_CLASS_ENV: &str = "KEEPER_ANDROID_DEVICE_CLASS";
/// `Build.MODEL`.
const MODEL_ENV: &str = "KEEPER_ANDROID_MODEL";

/// The data directory: the app's private files directory, which Android
/// removes with the app and which the manifest keeps out of every backup and
/// device transfer. `None` when the activity did not say (a build whose
/// `MainActivity` lost its `setenv`) — never a guessed `/data/data/…` path.
pub fn files_dir() -> Option<PathBuf> {
    std::env::var_os(FILES_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// This device's class (AD-314): a tablet by Android's own 600 dp rule.
pub fn device_class() -> DeviceClass {
    match std::env::var(DEVICE_CLASS_ENV).as_deref() {
        Ok("tablet") => DeviceClass::Tablet,
        _ => DeviceClass::Mobile,
    }
}

/// This device's model as Android names it; "android" when unknown.
pub fn device_model() -> String {
    std::env::var(MODEL_ENV)
        .ok()
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| "android".to_owned())
}

/// The registered `KeystorePlugin`, set once by [`keystore_plugin`]'s setup.
static KEYSTORE: OnceLock<PluginHandle<Wry>> = OnceLock::new();

/// The Tauri plugin that registers the Kotlin `KeystorePlugin` (class
/// `dev.tgorka.keeper.KeystorePlugin` in the app module). Added to the
/// builder on Android only, before any account restores.
pub fn keystore_plugin() -> TauriPlugin<Wry> {
    Builder::new("keeper-keystore")
        .setup(|_app, api| {
            let handle = api.register_android_plugin("dev.tgorka.keeper", "KeystorePlugin")?;
            let _ = KEYSTORE.set(handle);
            Ok(())
        })
        .build()
}

/// What `KeystorePlugin.dataKey` answers.
#[derive(serde::Deserialize)]
struct DataKey {
    /// 64 hex digits.
    key: String,
}

/// The data key of the secret file, asked of the Keystore once per process.
#[derive(Default)]
pub struct KeystoreKey {
    held: Mutex<Option<[u8; KEY_LEN]>>,
}

impl KeyProvider for KeystoreKey {
    /// Blocks on the plugin's answer, which Android runs on the activity's
    /// thread: never call it from that thread (keeper reads secrets from its
    /// async tasks, never from the UI thread).
    fn key(&self) -> Result<[u8; KEY_LEN], CoreError> {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(key) = *held {
            return Ok(key);
        }
        let handle = KEYSTORE.get().ok_or_else(|| {
            CoreError::Unsupported(
                "the Android Keystore is not reachable before keeper has started".to_owned(),
            )
        })?;
        let answer: DataKey = handle
            .run_mobile_plugin("dataKey", serde_json::Map::new())
            .map_err(|error| {
                CoreError::from(keeper_core::error::PlatformError::Keychain(format!(
                    "the Android Keystore did not hand over keeper's key: {error}"
                )))
            })?;
        let key = key_from_hex(&answer.key)?;
        *held = Some(key);
        Ok(key)
    }
}
