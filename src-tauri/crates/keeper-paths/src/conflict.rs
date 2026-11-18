//! Conflict-copy names and rescue names, independent of the merge engine.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Filesystems cap a component at ~255 bytes; the stem, the marker and the
/// extension all have to fit beside the label, so it gets a hard budget.
const DEVICE_LABEL_CAP: usize = 32;
/// Longest timestamp admitted. A well-formed `yyyymmdd-hhmmss` is 15.
const TIMESTAMP_CAP: usize = 24;

/// Build `<stem>.sync-conflict-<yyyymmdd-hhmmss>-<device>.<ext>`.
///
/// Returns a **file name**, not a path: the caller joins it with the conflicted
/// path's own parent so the copy lands beside the original, which is where a
/// user will look for it.
///
/// The extension is preserved so the copy still opens in the same application,
/// and only the final `.` counts as the separator — `a.tar.gz` keeps `.gz`, and
/// a dotfile like `.bashrc` has no extension at all rather than an extension of
/// `bashrc`.
///
/// Built from the name's **bytes**, not from `to_string_lossy`: that rendering
/// maps `a\xFF.txt` and `a\xFE.txt` onto one string, so two distinct files
/// would have collapsed onto one copy name and the second would have overwritten
/// the first. `names.rs` states the rule — a lossy rendering is fine to *show*
/// and must never be used to *reach* — and a copy name is used to reach.
pub fn conflict_name(path: &Path, now_utc: &str, device: &str) -> OsString {
    build_name(path, &marker(now_utc, device))
}

/// [`conflict_name`] with an ordinal, for the second and later copies of one
/// path within one stamp: `<stem>.sync-conflict-<stamp>-<device>-<n>.<ext>`.
///
/// Two passes within one second contest the same path when the first pass'
/// merge was undone and retried, and `fs::copy` onto the first copy would have
/// truncated it. The engine opens every copy with `create_new` and counts up
/// from 2 on collision; the first copy keeps the plain name so the common case
/// reads as it always has.
pub fn conflict_name_numbered(path: &Path, now_utc: &str, device: &str, ordinal: u32) -> OsString {
    build_name(path, &format!("{}-{ordinal}", marker(now_utc, device)))
}

fn marker(now_utc: &str, device: &str) -> String {
    format!(
        "sync-conflict-{}-{}",
        sanitize_component(now_utc, TIMESTAMP_CAP),
        sanitize_component(device, DEVICE_LABEL_CAP)
    )
}

fn build_name(path: &Path, marker: &str) -> OsString {
    let Some(name) = path.file_name() else {
        // A path with no final component (`/`, `..`) is not a file we could be
        // conflicting over, but returning a usable name beats panicking.
        return OsString::from(marker);
    };
    let name = name_bytes(name);
    let mut out = Vec::with_capacity(name.len() + marker.len() + 2);
    match name.iter().rposition(|byte| *byte == b'.') {
        // `idx > 0` keeps a leading dot from being read as a separator;
        // `idx + 1 < len` keeps a trailing dot from producing an empty suffix.
        Some(idx) if idx > 0 && idx + 1 < name.len() => {
            out.extend_from_slice(&name[..idx]);
            out.push(b'.');
            out.extend_from_slice(marker.as_bytes());
            out.push(b'.');
            out.extend_from_slice(&name[idx + 1..]);
        }
        _ => {
            out.extend_from_slice(&name);
            out.push(b'.');
            out.extend_from_slice(marker.as_bytes());
        }
    }
    name_from_bytes(out)
}

/// The paths a conflict copy at `rel` may have been copied from: `rel` with
/// its `.sync-conflict-<stamp>-<device>[-n]` marker taken out, extension
/// kept and not (Story 102.2). Empty when `rel` is no conflict copy.
///
/// Two answers rather than one because the marker cannot be told from the
/// extension when the device's label holds a dot: `a.sync-conflict-…-my.box`
/// is `a` copied on `my.box` or `a.box` copied on `my`. A caller looks for
/// both, and a path that never existed simply is not found.
pub fn originals_of(rel: &str) -> Vec<String> {
    const MARK: &str = ".sync-conflict-";
    let (dir, name) = rel
        .rsplit_once('/')
        .map_or(("", rel), |(dir, name)| (dir, name));
    let Some(at) = name.find(MARK) else {
        return Vec::new();
    };
    let stem = &name[..at];
    if stem.is_empty() {
        return Vec::new();
    }
    let mut names = Vec::with_capacity(2);
    if let Some((_, ext)) = name[at + MARK.len()..].rsplit_once('.') {
        if !ext.is_empty() {
            names.push(format!("{stem}.{ext}"));
        }
    }
    names.push(stem.to_owned());
    names
        .into_iter()
        .map(|name| {
            if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            }
        })
        .collect()
}

/// The canonical path behind one of git's rescue names, if `path` is one.
///
/// When a merge finds a directory where the other side has a file (or two
/// different object types at one path), git cannot put both at the name and
/// writes the file as `<path>~<label>`, then `<path>~<label>_0`, `_1`, … if
/// that is taken — `label` being `HEAD` for ours and the merged ref with `/`
/// as `_` for theirs. Measured on git 2.53: the unmerged index entry is
/// recorded *at the rescue name*, not at the canonical one, so this is how
/// the engine finds out which path the conflict is really about and turns
/// the rescue into an ordinary conflict copy instead of leaving litter.
pub fn rescue_origin(path: &Path, label: &str) -> Option<PathBuf> {
    let name = name_bytes(path.file_name()?);
    let tilde = name.iter().rposition(|byte| *byte == b'~')?;
    let suffix = &name[tilde + 1..];
    let rest = suffix.strip_prefix(label.as_bytes())?;
    let ordinal_ok = rest.is_empty()
        || (rest.len() > 1 && rest[0] == b'_' && rest[1..].iter().all(u8::is_ascii_digit));
    if tilde == 0 || !ordinal_ok {
        return None;
    }
    let stem = name_from_bytes(name[..tilde].to_vec());
    Some(match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(stem),
        _ => PathBuf::from(stem),
    })
}

/// The label git uses in a rescue name for the side merged in.
pub fn rescue_label(reference: &str) -> String {
    reference.replace('/', "_")
}

#[cfg(unix)]
fn name_bytes(name: &OsStr) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt as _;
    std::borrow::Cow::Borrowed(name.as_bytes())
}

/// Off unix an `OsStr` is not bytes; the rendering is lossy, and the platform
/// that produced every non-UTF-8 name this crate has met is unix.
#[cfg(not(unix))]
fn name_bytes(name: &OsStr) -> std::borrow::Cow<'_, [u8]> {
    std::borrow::Cow::Owned(name.to_string_lossy().into_owned().into_bytes())
}

#[cfg(unix)]
fn name_from_bytes(bytes: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt as _;
    OsString::from_vec(bytes)
}

#[cfg(not(unix))]
fn name_from_bytes(bytes: Vec<u8>) -> OsString {
    OsString::from(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reduce a label to characters that are safe in a file name on every platform
/// we target, capped at `cap` characters.
///
/// A device label is user-editable text: a `/` in it would turn the conflict
/// copy into a path, and a `:` or `\` would make it unrepresentable on Windows.
fn sanitize_component(value: &str, cap: usize) -> String {
    let cleaned: String = value
        .chars()
        .take(cap)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    // An all-illegal label would otherwise collapse the marker into `--`.
    if cleaned.chars().all(|c| c == '-') {
        "device".to_owned()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TS: &str = "20260725-120000";
    const DEV: &str = "laptop";
    /// The name as text, for assertions: every input here is UTF-8, so the
    /// rendering is exact.
    fn text(name: OsString) -> String {
        name.into_string()
            .expect("a UTF-8 input yields a UTF-8 name")
    }

    #[test]
    fn conflict_name_preserves_the_extension() {
        assert_eq!(
            text(conflict_name(Path::new("dir/a.txt"), TS, DEV)),
            "a.sync-conflict-20260725-120000-laptop.txt"
        );
    }

    #[test]
    fn conflict_name_splits_a_multi_dot_name_at_the_last_dot() {
        assert_eq!(
            text(conflict_name(Path::new("a.tar.gz"), TS, DEV)),
            "a.tar.sync-conflict-20260725-120000-laptop.gz"
        );
    }

    #[test]
    fn conflict_name_treats_a_dotfile_as_having_no_extension() {
        assert_eq!(
            text(conflict_name(Path::new(".bashrc"), TS, DEV)),
            ".bashrc.sync-conflict-20260725-120000-laptop"
        );
    }

    #[test]
    fn conflict_name_handles_a_name_without_an_extension() {
        assert_eq!(
            text(conflict_name(Path::new("noext"), TS, DEV)),
            "noext.sync-conflict-20260725-120000-laptop"
        );
    }

    #[test]
    fn conflict_name_never_lets_a_device_label_become_a_path() {
        let name = text(conflict_name(Path::new("a.txt"), TS, "work/box"));
        assert_eq!(name, "a.sync-conflict-20260725-120000-work-box.txt");
        assert!(!name.contains('/'), "a separator would relocate the copy");

        let spaced = text(conflict_name(Path::new("a.txt"), TS, "my box"));
        assert_eq!(spaced, "a.sync-conflict-20260725-120000-my-box.txt");
    }

    #[test]
    fn conflict_name_caps_a_hostile_device_label() {
        let long = "x".repeat(200);
        let name = text(conflict_name(Path::new("a.txt"), TS, &long));
        assert!(
            name.len() < 100,
            "a 200-char label must not blow the filename budget: {name}"
        );
    }

    #[test]
    fn conflict_name_survives_a_path_with_no_file_name() {
        let name = text(conflict_name(Path::new("/"), TS, DEV));
        assert_eq!(name, "sync-conflict-20260725-120000-laptop");
    }

    #[test]
    fn a_numbered_copy_keeps_the_ordinal_inside_the_marker() {
        assert_eq!(
            text(conflict_name_numbered(Path::new("a.txt"), TS, DEV, 2)),
            "a.sync-conflict-20260725-120000-laptop-2.txt"
        );
        assert_eq!(
            text(conflict_name_numbered(Path::new(".bashrc"), TS, DEV, 3)),
            ".bashrc.sync-conflict-20260725-120000-laptop-3"
        );
    }

    #[cfg(unix)]
    #[test]
    fn two_names_that_render_alike_get_two_different_copy_names() {
        // The bug `names.rs` documents: `to_string_lossy` maps both onto one
        // string, and one copy name means one of the two files is lost.
        use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
        let ff = PathBuf::from(OsString::from_vec(b"a\xFF.txt".to_vec()));
        let fe = PathBuf::from(OsString::from_vec(b"a\xFE.txt".to_vec()));
        let ff_copy = conflict_name(&ff, TS, DEV);
        let fe_copy = conflict_name(&fe, TS, DEV);
        assert_ne!(ff_copy, fe_copy);
        assert_eq!(
            ff_copy.as_bytes(),
            b"a\xFF.sync-conflict-20260725-120000-laptop.txt"
        );
    }

    #[test]
    fn a_rescue_name_resolves_to_its_canonical_path_and_nothing_else_does() {
        let label = rescue_label("refs/remotes/origin/main");
        assert_eq!(label, "refs_remotes_origin_main");
        assert_eq!(
            rescue_origin(Path::new("dir/d~HEAD"), "HEAD"),
            Some(PathBuf::from("dir/d"))
        );
        assert_eq!(
            rescue_origin(Path::new("d~HEAD_0"), "HEAD"),
            Some(PathBuf::from("d"))
        );
        assert_eq!(
            rescue_origin(Path::new("d~refs_remotes_origin_main_12"), &label),
            Some(PathBuf::from("d"))
        );
        // The other side's label is not this side's rescue.
        assert_eq!(rescue_origin(Path::new("d~HEAD"), &label), None);
        // A `~` a user typed, a suffix that is not an ordinal, an empty stem.
        assert_eq!(rescue_origin(Path::new("backup~HEADS"), "HEAD"), None);
        assert_eq!(rescue_origin(Path::new("d~HEAD_x"), "HEAD"), None);
        assert_eq!(rescue_origin(Path::new("~HEAD"), "HEAD"), None);
        assert_eq!(rescue_origin(Path::new("plain.txt"), "HEAD"), None);
    }
    /// Every name `conflict_name` builds leads back to the file it was
    /// copied from — the path a heal looks for the bytes under (102.2).
    #[test]
    fn a_conflict_copy_names_the_file_it_was_copied_from() {
        let copy = |path: &str, device: &str| {
            let name = conflict_name(Path::new(path), TS, device);
            let name = name.to_str().expect("utf-8").to_owned();
            match path.rsplit_once('/') {
                Some((dir, _)) => format!("{dir}/{name}"),
                None => name,
            }
        };
        for (path, device) in [
            ("70-comms/x/manifest.json", "maczek"),
            ("a.tar.gz", DEV),
            (".bashrc", DEV),
            ("noext", DEV),
            ("notes/a.md", "my.box"),
        ] {
            let originals = originals_of(&copy(path, device));
            assert!(originals.iter().any(|o| o == path), "{path}: {originals:?}");
        }
        let numbered = conflict_name_numbered(Path::new("a.txt"), TS, DEV, 2);
        assert!(originals_of(numbered.to_str().expect("utf-8")).contains(&"a.txt".to_owned()));
        assert!(originals_of("dir/manifest.json").is_empty());
    }
}
