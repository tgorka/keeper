//! The Android secret store's file (Epic 98, story 98.3 #2): it round-trips,
//! refuses a flipped byte, another key and another version, never writes a
//! secret in plaintext, and takes a fresh nonce per write. A fixed key stands
//! in for the Keystore plugin (`keeper_core::secret_file::KeyProvider`).

use std::fs;
use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use keeper_core::error::{CoreError, PlatformError};
use keeper_core::secret_file::{
    key_from_hex, open, seal, KeyProvider, Refusal, SecretFile, FILE_NAME, KEY_LEN, MAGIC, VERSION,
};

/// Magic + version: where the nonce starts.
const HEADER_LEN: usize = MAGIC.len() + 1;
/// XChaCha20's nonce.
const NONCE_LEN: usize = 24;

struct Fixed([u8; KEY_LEN]);

impl KeyProvider for Fixed {
    fn key(&self) -> Result<[u8; KEY_LEN], CoreError> {
        Ok(self.0)
    }
}

const KEY: [u8; KEY_LEN] = [7; KEY_LEN];
const SESSION: &str = "session/01J8ZK4Q3Y8V9W2X3Y4Z5A6B7C";
const PASSPHRASE: &str = "store_passphrase/01J8ZK4Q3Y8V9W2X3Y4Z5A6B7C";
const SECRET: &str = "syt_not_a_real_token_but_long_enough_to_find";

fn store(dir: &Path, key: [u8; KEY_LEN]) -> SecretFile<Fixed> {
    SecretFile::new(dir.join(FILE_NAME), Fixed(key))
}

/// The refusal a store operation reported, as the variant it was raised as:
/// `SecretFile` reports refusals where a keychain's would be, so the variant
/// is recognised by its own rendering, never by a fragment of prose.
fn refused(error: CoreError, expected: Refusal) -> bool {
    match error {
        CoreError::Platform(PlatformError::Keychain(message)) => message == expected.to_string(),
        other => panic!("expected a keychain error, got {other:?}"),
    }
}

#[test]
fn a_secret_round_trips_through_a_fresh_store_and_is_deleted() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(store(dir.path(), KEY).get(SESSION).expect("empty"), None);

    store(dir.path(), KEY).set(SESSION, SECRET).expect("set");
    store(dir.path(), KEY)
        .set(PASSPHRASE, "pass")
        .expect("second set");

    let reopened = store(dir.path(), KEY);
    assert_eq!(reopened.get(SESSION).expect("get").as_deref(), Some(SECRET));
    reopened.set(SESSION, "replaced").expect("replace");
    assert_eq!(
        store(dir.path(), KEY).get(SESSION).expect("get").as_deref(),
        Some("replaced")
    );

    reopened.delete(SESSION).expect("delete");
    reopened.delete(SESSION).expect("deleting a missing key");
    let after = store(dir.path(), KEY);
    assert_eq!(after.get(SESSION).expect("get"), None);
    assert_eq!(
        after.get(PASSPHRASE).expect("other key").as_deref(),
        Some("pass"),
        "deleting one key keeps the others"
    );
    assert!(
        !dir.path().join(format!("{FILE_NAME}.tmp")).exists(),
        "no temporary file is left behind"
    );
}

#[test]
fn a_flipped_byte_anywhere_is_refused_by_reads_and_writes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    store(dir.path(), KEY).set(SESSION, SECRET).expect("set");
    let good = fs::read(&path).expect("file");

    // Every byte after the header: the nonce, the ciphertext, the tag.
    // The header's own bytes are the next tests'.
    for at in HEADER_LEN..good.len() {
        let mut bad = good.clone();
        bad[at] ^= 0x01;
        assert_eq!(open(&KEY, &bad), Err(Refusal::Tampered), "byte {at}");
        fs::write(&path, &bad).expect("tamper");
        let read = store(dir.path(), KEY).get(SESSION);
        assert!(
            refused(read.expect_err("tampered read"), Refusal::Tampered),
            "byte {at}"
        );
        let write = store(dir.path(), KEY).set("other", "value");
        assert!(
            write.is_err(),
            "a write over a tampered file is refused (byte {at})"
        );
        assert_eq!(
            fs::read(&path).expect("file"),
            bad,
            "and leaves it as it was"
        );
    }

    fs::write(&path, &good[..good.len() - 1]).expect("truncate");
    assert!(
        refused(
            store(dir.path(), KEY).get(SESSION).expect_err("truncated"),
            Refusal::Tampered
        ),
        "a truncated file"
    );
    assert_eq!(
        open(&KEY, &good[..HEADER_LEN + NONCE_LEN - 1]),
        Err(Refusal::Tampered),
        "a body shorter than its nonce"
    );
}

#[test]
fn another_key_cannot_open_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    store(dir.path(), KEY).set(SESSION, SECRET).expect("set");
    let mut other = KEY;
    other[0] ^= 0x80;
    let error = store(dir.path(), other)
        .get(SESSION)
        .expect_err("wrong key");
    assert!(refused(error, Refusal::Tampered));
    let sealed = fs::read(dir.path().join(FILE_NAME)).expect("file");
    assert_eq!(open(&other, &sealed), Err(Refusal::Tampered));
}

#[test]
fn another_version_is_refused_as_a_version_and_never_overwritten() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    store(dir.path(), KEY).set(SESSION, SECRET).expect("set");
    let mut newer = fs::read(&path).expect("file");
    newer[MAGIC.len()] = VERSION + 1;
    fs::write(&path, &newer).expect("bump");

    assert_eq!(open(&KEY, &newer), Err(Refusal::Version(VERSION + 1)));
    let error = store(dir.path(), KEY).get(SESSION).expect_err("newer");
    assert!(refused(error, Refusal::Version(VERSION + 1)));
    assert!(store(dir.path(), KEY).set("other", "value").is_err());
    assert_eq!(
        fs::read(&path).expect("file"),
        newer,
        "a newer file is not overwritten"
    );

    let mut foreign = newer;
    foreign[0] = b'X';
    assert_eq!(open(&KEY, &foreign), Err(Refusal::NotASecretFile));
    assert_eq!(
        open(&KEY, &MAGIC[..]),
        Err(Refusal::NotASecretFile),
        "no version byte"
    );
    fs::write(&path, &foreign).expect("magic");
    let error = store(dir.path(), KEY).get(SESSION).expect_err("magic");
    assert!(refused(error, Refusal::NotASecretFile));
}

#[test]
fn a_file_that_opens_but_holds_no_map_of_secrets_is_refused_and_kept() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    let sealed = seal(&KEY, br#"["not", "a", "map"]"#).expect("seal");
    fs::write(&path, &sealed).expect("write");
    let error = store(dir.path(), KEY).get(SESSION).expect_err("not a map");
    assert!(refused(error, Refusal::NotSecrets));
    assert!(store(dir.path(), KEY).set(SESSION, SECRET).is_err());
    assert_eq!(
        fs::read(&path).expect("file"),
        sealed,
        "and is not overwritten"
    );
}

#[test]
fn a_body_sealed_under_another_header_does_not_open_under_this_one() {
    // The header is the associated data: a body sealed for version 2, or
    // sealed with no associated data at all, and labelled version 1 fails the
    // tag — nobody can move a body between versions without the key.
    let plaintext = br#"{"k":"v"}"#;
    let nonce = [9u8; NONCE_LEN];
    for aad in [&b"KSEC\x02"[..], &b""[..]] {
        let ciphertext = XChaCha20Poly1305::new((&KEY).into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .expect("encrypt");
        let mut relabelled = MAGIC.to_vec();
        relabelled.push(VERSION);
        relabelled.extend_from_slice(&nonce);
        relabelled.extend_from_slice(&ciphertext);
        assert_eq!(open(&KEY, &relabelled), Err(Refusal::Tampered), "{aad:?}");
    }
    assert_eq!(
        open(&KEY, &seal(&KEY, plaintext).expect("seal")).expect("own header"),
        plaintext
    );
}

#[test]
fn nothing_is_written_in_plaintext_and_no_nonce_is_reused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    let secrets = store(dir.path(), KEY);
    secrets.set(SESSION, SECRET).expect("set");
    let first = fs::read(&path).expect("file");
    for needle in [SECRET.as_bytes(), SESSION.as_bytes(), b"session/", b"syt_"] {
        assert!(
            !first.windows(needle.len()).any(|window| window == needle),
            "{} is in the file",
            String::from_utf8_lossy(needle)
        );
    }
    secrets.set(SESSION, SECRET).expect("same value again");
    let second = fs::read(&path).expect("file");
    assert_ne!(
        first[HEADER_LEN..HEADER_LEN + NONCE_LEN],
        second[HEADER_LEN..HEADER_LEN + NONCE_LEN],
        "every write takes a fresh nonce"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o077, 0, "readable by its owner only: {mode:o}");
    }
}

#[test]
fn a_provider_that_has_no_key_writes_nothing() {
    struct Absent;
    impl KeyProvider for Absent {
        fn key(&self) -> Result<[u8; KEY_LEN], CoreError> {
            Err(CoreError::Unsupported(
                "the Keystore plugin is not up".into(),
            ))
        }
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let secrets = SecretFile::new(dir.path().join(FILE_NAME), Absent);
    assert!(matches!(
        secrets.set(SESSION, SECRET),
        Err(CoreError::Unsupported(_))
    ));
    assert!(!dir.path().join(FILE_NAME).exists());
    assert_eq!(secrets.get(SESSION).expect("no file, no key needed"), None);
}

#[test]
fn the_keystore_key_is_exactly_sixty_four_hex_digits() {
    let hex = "00ff7f80a1B2c3D4e5F6071829304a5b6c7d8e9f0011223344556677889900aa";
    let key = key_from_hex(hex).expect("a whole key");
    assert_eq!(key[0], 0x00);
    assert_eq!(key[1], 0xff);
    assert_eq!(key[2], 0x7f);
    assert_eq!(key[3], 0x80);
    assert_eq!(key[4], 0xa1);
    assert_eq!(key[5], 0xb2);
    assert_eq!(key[KEY_LEN - 1], 0xaa);
    let bad = [
        hex[..62].to_owned(),
        format!("{hex}00"),
        format!("+f{}", &hex[2..]),
        format!("{}g0", &hex[..62]),
        format!(" {}", &hex[1..]),
        String::new(),
    ];
    for bad in &bad {
        assert_eq!(key_from_hex(bad), Err(Refusal::KeyUnusable), "{bad}");
    }
}
