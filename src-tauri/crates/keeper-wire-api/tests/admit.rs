//! 123.3 A-2/SR-4: the app's admission to its data directory, answered before anything there is opened.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::time::{Duration, Instant};

use keeper_wire_api::{admit, Admission, DataLock};
use tempfile::tempdir;

fn names(dir: &Path) -> Vec<String> {
    let mut entries: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    entries
}

#[test]
fn first_launch_creates_a_private_directory_and_holds_the_lock() {
    let tmp = tempdir().unwrap();
    let data = tmp.path().join("keeper-data");

    let answer = admit(&data, "keeper (the app), pid 1", Duration::from_millis(0));
    assert!(
        matches!(answer, Admission::Held(_)),
        "expected Held, got {answer:?}"
    );
    let mode = fs::metadata(&data).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    assert_eq!(names(&data), vec!["keeper.lock"]);
    drop(answer);
}

#[test]
fn held_elsewhere_is_refused_naming_the_holder_and_opens_nothing() {
    let tmp = tempdir().unwrap();
    let data = tmp.path();
    fs::set_permissions(data, fs::Permissions::from_mode(0o700)).unwrap();

    let other = DataLock::acquire(data, "keeper-cli serve, pid 4242").unwrap();
    let start = Instant::now();
    let answer = admit(data, "keeper (the app), pid 1", Duration::from_millis(400));
    assert!(start.elapsed() >= Duration::from_millis(400));
    let Admission::Refused(sentence) = answer else {
        panic!("expected Refused, got {answer:?}");
    };
    assert!(
        sentence.contains("keeper-cli serve, pid 4242"),
        "expected holder in sentence, got: {sentence}"
    );
    assert_eq!(names(data), vec!["keeper.lock"]);

    drop(other);
    let answer = admit(data, "keeper (the app), pid 1", Duration::ZERO);
    assert!(
        matches!(answer, Admission::Held(_)),
        "expected Held after release, got {answer:?}"
    );
}

#[test]
fn unsafe_directory_is_refused_at_once() {
    let tmp = tempdir().unwrap();
    let data = tmp.path();
    // Writable by the group: `check_data_dir` refuses it before any lock.
    fs::set_permissions(data, fs::Permissions::from_mode(0o775)).unwrap();

    let start = Instant::now();
    let answer = admit(data, "keeper (the app), pid 1", Duration::from_secs(5));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(
        matches!(answer, Admission::Refused(_)),
        "expected Refused, got {answer:?}"
    );
    assert!(names(data).is_empty());
}

#[test]
fn unopenable_lock_file_opens_the_data_with_the_socket_off() {
    let tmp = tempdir().unwrap();
    let data = tmp.path();
    fs::set_permissions(data, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir(data.join("keeper.lock")).unwrap();

    let answer = admit(data, "keeper (the app), pid 1", Duration::ZERO);
    assert!(
        matches!(answer, Admission::Unsupported(_)),
        "expected Unsupported, got {answer:?}"
    );
}
