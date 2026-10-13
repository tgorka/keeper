//! The secret store of a build with no keychain keeper may use: Android
//! (Epic 98, story 98.3 #2; AD-414).
//!
//! Android has no keychain the way Apple's platforms do. Its Keystore holds
//! keys, never values, and an AES key it generates never leaves it. So
//! keeper's secrets — `session/<ulid>`, `store_passphrase/<ulid>`,
//! `sync/<profile>/credential` — live in one encrypted file in the app's
//! private files directory, and the 32-byte key that opens it is a data key
//! the Keystore wraps (the shell's Kotlin `KeystorePlugin` generates it once,
//! keeps only its wrapped form, and hands the unwrapped key to Rust through
//! [`KeyProvider`]). The `keyring` crate is never the answer here: off Apple
//! and desktop Linux it resolves to its in-memory mock store, which forgets
//! every secret at exit.
//!
//! # The file
//!
//! ```text
//! "KSEC" | version (1 byte) | nonce (24 bytes) | ciphertext ‖ tag
//! ```
//!
//! - **XChaCha20-Poly1305.** The nonce is random per write — 24 bytes, so a
//!   random nonce needs no counter beside it. The four-byte magic and the
//!   version are the associated data: a header altered without the key fails
//!   the tag like any other flipped byte.
//! - **One version.** [`VERSION`] is the only one this keeper reads. Another
//!   is refused *before* any decryption, with a sentence that says so — a
//!   newer keeper's file is not "tampered", and must not be overwritten.
//! - **The plaintext** is the JSON object of every key and its value. The key
//!   names carry account ids, so they are inside the ciphertext too; nothing
//!   in the file is readable without the key.
//! - **Written whole, atomically.** Every change re-encrypts the whole map
//!   into a sibling `*.tmp`, syncs it, and renames it over the file, so a
//!   crash leaves the old file or the new one, never a torn one. Owner-only
//!   permissions on Unix.
//!
//! # What it refuses
//!
//! A file that fails its tag (a flipped byte anywhere, a different key) or
//! names another version is refused, by `get` and by `set` alike: writing over
//! it would destroy the other secrets it may still hold for whoever can read
//! it. Starting over is the person's call — clearing keeper's storage in
//! Android's settings — never this module's.

use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;

use crate::error::{CoreError, PlatformError};

/// The first four bytes of every secret file.
pub const MAGIC: &[u8; 4] = b"KSEC";

/// The one format version this keeper writes and reads.
pub const VERSION: u8 = 1;

/// The data key's length: XChaCha20-Poly1305's 256-bit key.
pub const KEY_LEN: usize = 32;

/// XChaCha20's extended nonce.
const NONCE_LEN: usize = 24;

/// Magic + version: the associated data, and where the nonce starts.
const HEADER_LEN: usize = MAGIC.len() + 1;

/// The file's name in the data directory.
pub const FILE_NAME: &str = "secrets.bin";

/// Where the data key comes from. On Android, the Keystore plugin; in tests,
/// a fixed key. Called on every read and write, so an implementation that
/// reaches the platform should hold the key once it has it.
pub trait KeyProvider: Send + Sync {
    /// The 32-byte data key, or why there is none (the plugin is not up yet,
    /// the Keystore refused).
    fn key(&self) -> Result<[u8; KEY_LEN], CoreError>;
}

/// One encrypted file of named secrets. See the module docs for the format.
pub struct SecretFile<P> {
    path: PathBuf,
    provider: P,
    /// Every read-modify-write is one critical section: two concurrent `set`s
    /// must not each write a map missing the other's key.
    lock: Mutex<()>,
}

impl<P: KeyProvider> SecretFile<P> {
    /// The store at `path`; nothing is read or created until it is used.
    pub fn new(path: PathBuf, provider: P) -> Self {
        Self {
            path,
            provider,
            lock: Mutex::new(()),
        }
    }

    /// The secret stored under `key`, or `None` — a missing file holds none.
    pub fn get(&self, key: &str) -> Result<Option<String>, CoreError> {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(self.read()?.remove(key))
    }

    /// Store `value` under `key`, replacing what was there.
    pub fn set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut secrets = self.read()?;
        secrets.insert(key.to_owned(), value.to_owned());
        self.write(&secrets)
    }

    /// Remove `key`. Removing a key that is not there is not an error.
    pub fn delete(&self, key: &str) -> Result<(), CoreError> {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut secrets = self.read()?;
        if secrets.remove(key).is_none() {
            return Ok(());
        }
        self.write(&secrets)
    }

    fn read(&self) -> Result<BTreeMap<String, String>, CoreError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(error) => {
                return Err(keychain(format!(
                    "could not read the secret store: {error}"
                )))
            }
        };
        let plaintext = open(&self.provider.key()?, &bytes)?;
        serde_json::from_slice(&plaintext).map_err(|_| Refusal::NotSecrets.into())
    }

    fn write(&self, secrets: &BTreeMap<String, String>) -> Result<(), CoreError> {
        let plaintext = serde_json::to_vec(secrets)
            .map_err(|error| keychain(format!("could not encode secrets: {error}")))?;
        let sealed = seal(&self.provider.key()?, &plaintext)?;
        let mut tmp = self.path.clone().into_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        write_private(&tmp, &sealed)
            .and_then(|()| fs::rename(&tmp, &self.path))
            .map_err(|error| {
                let _ = fs::remove_file(&tmp);
                keychain(format!("could not write the secret store: {error}"))
            })
    }
}

/// Encrypt `plaintext` into the file's bytes under a fresh random nonce.
pub fn seal(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>, CoreError> {
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let mut sealed = Vec::with_capacity(HEADER_LEN + NONCE_LEN + plaintext.len() + 16);
    sealed.extend_from_slice(MAGIC);
    sealed.push(VERSION);
    let ciphertext = XChaCha20Poly1305::new(key.into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &sealed[..HEADER_LEN],
            },
        )
        .map_err(|_| keychain("could not encrypt the secret store"))?;
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ciphertext);
    Ok(sealed)
}

/// Why the store refuses a file or a key. Each is reported where a keychain's
/// refusal would be (`PlatformError::Keychain`, through `From<Refusal> for
/// CoreError`), with the sentence below; callers that need the kind — the
/// tests, a future way back (DW-1292) — match the variant, never the sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// Too short for a header, or not `KSEC`.
    #[error("the secret store is not a keeper secret file")]
    NotASecretFile,
    /// Another format version, refused before any decryption: a newer
    /// keeper's file is not "tampered", and is never overwritten.
    #[error("the secret store is version {0}; this keeper reads version {VERSION} only")]
    Version(u8),
    /// The tag does not verify: a changed byte, or another key.
    #[error(
        "the secret store failed its integrity check: it was changed, or it was written under \
         another key"
    )]
    Tampered,
    /// The file opened but its plaintext is not keeper's map of secrets.
    #[error("the secret store opened but does not hold keeper's secrets")]
    NotSecrets,
    /// The Keystore plugin handed over something other than 64 hex digits.
    #[error("the Android Keystore handed over a key keeper cannot use")]
    KeyUnusable,
}

impl From<Refusal> for CoreError {
    fn from(refusal: Refusal) -> Self {
        keychain(refusal.to_string())
    }
}

/// Decrypt the file's bytes, refusing another version before trying, and
/// anything whose tag does not verify.
pub fn open(key: &[u8; KEY_LEN], sealed: &[u8]) -> Result<Vec<u8>, Refusal> {
    if sealed.len() < HEADER_LEN || &sealed[..MAGIC.len()] != MAGIC {
        return Err(Refusal::NotASecretFile);
    }
    let version = sealed[MAGIC.len()];
    if version != VERSION {
        return Err(Refusal::Version(version));
    }
    let body = &sealed[HEADER_LEN..];
    if body.len() < NONCE_LEN {
        return Err(Refusal::Tampered);
    }
    let (nonce, ciphertext) = body.split_at(NONCE_LEN);
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: &sealed[..HEADER_LEN],
            },
        )
        .map_err(|_| Refusal::Tampered)
}

/// The data key as the Keystore plugin hands it over: 64 hex digits. Anything
/// else — a short key, a long one, a stray character — is refused rather than
/// padded, truncated or skipped, because a key that is merely *close* opens
/// nothing and would read as a tampered store.
pub fn key_from_hex(hex: &str) -> Result<[u8; KEY_LEN], Refusal> {
    let digits = hex.as_bytes();
    if digits.len() != KEY_LEN * 2 {
        return Err(Refusal::KeyUnusable);
    }
    let nibble = |digit: u8| (digit as char).to_digit(16).ok_or(Refusal::KeyUnusable);
    let mut key = [0u8; KEY_LEN];
    for (byte, [high, low]) in key.iter_mut().zip(digits.as_chunks::<2>().0) {
        let value = nibble(*high)? * 16 + nibble(*low)?;
        *byte = u8::try_from(value).map_err(|_| Refusal::KeyUnusable)?;
    }
    Ok(key)
}

/// A failure of the store that is not a refusal (I/O, encoding), reported
/// where a keychain's would be.
fn keychain(message: impl Into<String>) -> CoreError {
    PlatformError::Keychain(message.into()).into()
}

/// Create (or truncate) `path` readable by its owner only, write `bytes` and
/// sync them before the rename that publishes them.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
