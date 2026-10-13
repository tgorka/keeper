//! The "On this …" disclosures and their docs say the same thing (Story 62.3,
//! FR-400; Epic 98, story 98.3 #4, codemap Q12 (b)).
//!
//! The lists are composed in `keeper_core::disclosure` and the docs a person
//! reads at length are `docs/ios.md` and `docs/android.md`, each saying its
//! *Limitations* list is the in-app one. A sentence in a doc is a request; this
//! is the guard. Each list is read from disk, never restated here, so a third
//! copy cannot drift either.

use std::path::PathBuf;

use keeper_core::disclosure::{disclosure, Device};

/// The repository's `docs/<name>`, from this crate's manifest directory.
fn doc(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The first run of `- ` bullets under the line `heading`: the list stops at the
/// first line that is not a bullet once it has begun, so the prose below is not
/// read as one more item. A missing heading or an empty list fails rather than
/// passing vacuously.
fn first_bullet_run(doc: &str, heading: &str) -> Vec<String> {
    let start = doc
        .find(&format!("{heading}\n"))
        .unwrap_or_else(|| panic!("no '{heading}' section"));
    let bullets: Vec<String> = doc[start + heading.len() + 1..]
        .lines()
        .skip_while(|line| !line.starts_with("- "))
        .take_while(|line| line.starts_with("- "))
        .map(|line| line[2..].to_owned())
        .collect();
    assert!(!bullets.is_empty(), "'{heading}' has no bullet list");
    bullets
}

#[test]
fn the_iphone_disclosure_is_docs_ios_limitations_list_in_order() {
    assert_eq!(
        first_bullet_run(&doc("ios.md"), "## Limitations"),
        disclosure(Device::Iphone).lines
    );
}

#[test]
fn the_tablet_disclosure_is_docs_android_limitations_list_in_order() {
    assert_eq!(
        first_bullet_run(&doc("android.md"), "## Limitations"),
        disclosure(Device::AndroidTablet).lines
    );
}

#[test]
fn each_reduced_build_names_what_bots_does_not_do_there() {
    // FR-400: the drive tools are on the Mac, and a phone or a tablet says so
    // where the person reads what the build lacks — not only in a doc.
    for device in [Device::Iphone, Device::AndroidTablet, Device::AndroidPhone] {
        assert!(
            disclosure(device)
                .lines
                .iter()
                .any(|line| line.contains("drive tools live on your Mac")),
            "{device:?}"
        );
    }
}

#[test]
fn an_android_phone_is_never_called_a_tablet() {
    let phone = disclosure(Device::AndroidPhone);
    assert_eq!(phone.heading, "On this phone");
    let tablet = disclosure(Device::AndroidTablet);
    assert_eq!(phone.lines.len(), tablet.lines.len());
    for (phone_line, tablet_line) in phone.lines.iter().zip(&tablet.lines) {
        assert!(!phone_line.contains("tablet"), "{phone_line}");
        assert_eq!(phone_line, &tablet_line.replace("tablet", "phone"));
    }
}
