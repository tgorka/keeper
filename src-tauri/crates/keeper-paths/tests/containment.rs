use keeper_paths::{lexical_join, plain_segments, resolve, BrowseRefusal};

#[test]
fn a_replacement_character_never_reaches_the_real_file_of_that_name() {
    let root = tempfile::tempdir().expect("root");
    let name = "a\u{FFFD}.txt";
    std::fs::write(root.path().join(name), "a different file").expect("decoy");
    let refused = BrowseRefusal::Unspellable {
        subpath: name.to_owned(),
    };
    assert_eq!(plain_segments(name), Err(refused.clone()));
    assert_eq!(lexical_join(root.path(), name), Err(refused.clone()));
    assert_eq!(resolve(root.path(), name), Err(refused));
    assert_eq!(
        std::fs::read_to_string(root.path().join(name)).expect("untouched"),
        "a different file"
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_escape_is_refused_even_when_its_spelling_is_plain() {
    let root = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("secret"), "secret").expect("file");
    std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).expect("link");
    assert_eq!(
        resolve(root.path(), "escape/secret"),
        Err(BrowseRefusal::EscapesAfterResolution {
            subpath: "escape/secret".to_owned()
        })
    );
}
