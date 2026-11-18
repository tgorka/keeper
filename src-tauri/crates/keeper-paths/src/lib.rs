//! Shared path containment, filename and write-routing rules (AD-65, AD-159).
//! The lexical half always runs before filesystem resolution. No engine dependencies.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

pub mod conflict;
pub mod names;
mod stamp;
mod write;
pub use names::{ForDisplay, UnspellableName};
pub use stamp::{civil_from_unix_ms, conflict_stamp};
pub use write::{
    last_segment, resolve_existing, CreateTarget, UnmanagedPath, VaultPath, WriteOwner,
    WriteRefusal, WriteRoute, WriteScope, MAX_NAME_BYTES,
};

/// Why a browse call was refused before anything was read.
///
/// Refusals are separate from the directory listing because they are not states of
/// the folder — they are states of the *request*, and a caller that produces
/// one has asked for something it must never be able to ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowseRefusal {
    /// The subpath is not a plain descendant of the profile root: it was
    /// absolute, or it contained `..`, `.`, an empty component, or (on Windows)
    /// a drive prefix or a backslash separator.
    Escapes {
        /// The offending subpath, verbatim, so the log names what was asked
        /// for.
        subpath: String,
    },
    /// The subpath resolved to a plain descendant lexically and then, once
    /// symlinks were followed, landed outside the profile root. A planted
    /// symlink is the one escape no string test can catch (AD-59).
    EscapesAfterResolution {
        /// The offending subpath, verbatim.
        subpath: String,
    },
    /// The directory is there and could not be read — a permissions failure,
    /// most often. Distinct from a missing listing: the folder exists,
    /// so telling someone it is gone would send them looking for a backup.
    Unreadable {
        /// The OS's own words, which are the most useful thing to show.
        reason: String,
    },
    /// The subpath contains `U+FFFD`, so it may be this module's own lossy
    /// rendering of a name that is not valid UTF-8 rather than a name (Story
    /// 47.2).
    ///
    /// **Refused because joining it can reach a different, real file.** A
    /// listing renders `a\xFF.txt` as `a<FFFD>.txt`; if the same folder also
    /// holds a file genuinely named `a<FFFD>.txt` — three ordinary UTF-8 bytes,
    /// a name a user can type — then joining the rendering back onto the root
    /// does not fail, it succeeds at the wrong file. That was measured before
    /// this variant existed, and [`crate::write`] shares the join, so the
    /// wrong file could be deleted.
    ///
    /// **Both cases are refused, and that is the right way round.** Nothing can
    /// tell a rendering apart from a real `U+FFFD` name, so the choice is
    /// between refusing a file that is almost certainly not there and silently
    /// acting on a file the user did not pick. A user who really does own a
    /// file with `U+FFFD` in its name loses the ability to expand it from this
    /// surface; a user who does not, keeps the file the browser would otherwise
    /// have eaten. See [`crate::names`].
    Unspellable {
        /// The offending subpath, verbatim.
        subpath: String,
    },
}

impl std::fmt::Display for BrowseRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Escapes { subpath } => write!(
                f,
                "\"{subpath}\" is not a path inside this folder, so it will not be listed"
            ),
            Self::EscapesAfterResolution { subpath } => write!(
                f,
                "\"{subpath}\" resolves outside this folder, so it will not be listed"
            ),
            Self::Unreadable { reason } => write!(f, "this folder could not be read: {reason}"),
            Self::Unspellable { subpath } => write!(
                f,
                "\"{subpath}\" holds the replacement character, so it may be a rendering of a \
                 name that is not text rather than a name; keeper will not act on it, because \
                 doing so could reach a different file"
            ),
        }
    }
}

/// Turn a profile-relative subpath into an absolute path under `root`, or
/// refuse it.
///
/// **This is the whole of AD-65 for the Files tab.** The frontend never joins a
/// root and a subpath; it hands back a `relative_path` this module produced and
/// this function does the join, once, in Rust — and gets to say no.
///
/// The lexical half runs first and runs unconditionally, before the disk is
/// consulted at all, so `..` is refused identically whether the drive is in,
/// out, or replaced by a stranger's. Each component must be exactly one
/// `Component::Normal`, which is a single test that covers every shape an
/// escape takes and covers it *per platform*: `..` is `ParentDir`, `.` is
/// `CurDir`, a leading `/` is `RootDir`, `C:` is `Prefix`, and on Windows —
/// where `\` is a separator and on Linux it is an ordinary filename character —
/// `a\b` yields two components there and one here, which is exactly the
/// difference that matters.
///
/// The canonicalizing half catches what no string test can: a symlink inside
/// the folder pointing somewhere else (AD-59, the same two-halves idiom
/// `recording_open_path` uses). Canonicalizing the root as well is what keeps a
/// folder reached through a symlink — a relocated home directory — from
/// refusing every path under it.
///
/// `Ok(None)` means the lexical test passed and the path is simply not on disk;
/// the caller decides whether that is absent media or a missing folder, because
/// only the caller knows whether the profile is removable.
pub fn resolve(root: &Path, subpath: &str) -> Result<Option<PathBuf>, BrowseRefusal> {
    let target = lexical_join(root, subpath)?;

    let (Ok(canonical_root), Ok(canonical_target)) = (root.canonicalize(), target.canonicalize())
    else {
        // Either the profile folder is not there (absent media, or moved), or
        // the entry under it is not. Not a refusal: nothing was asked for that
        // may not be asked for.
        return Ok(None);
    };
    if !canonical_target.starts_with(&canonical_root) {
        return Err(BrowseRefusal::EscapesAfterResolution {
            subpath: subpath.to_owned(),
        });
    }
    Ok(Some(canonical_target))
}

/// [`resolve`] for a caller that must tell "not there" from "not known" —
/// keeper-paths' one such resolver, for every reader that acts on an
/// absence or reads a landing: absent only where the disk says `NotFound`
/// for the entry under a root that is there ([`Known::Absent`]), or for the
/// root itself under a folder that is ([`Known::RootAbsent`],
/// [`absent_root`]), and no dangling link names it ([`landing`] refuses
/// one). Anything else the disk cannot vouch for is
/// [`BrowseRefusal::Unreadable`]: a folder on the way that may not be
/// searched, an I/O error, a file where a folder of the path should be
/// (`NotADirectory`), a root that is not a folder, or one on the way to it
/// that is a file or a dangling link — an absence a caller acts on has to
/// be one the disk established. The root is asked about in its plain
/// spelling — `80-agents/`, `./80-agents` and `80-agents//` are `80-agents`
/// ([`Path::components`]) — so a trailing separator, which makes the disk
/// follow a link before it answers, never hides a link to nothing as an
/// absence. A landing outside the root is refused, and one keeper cannot
/// spell is [`BrowseRefusal::Unspellable`]: a caller admits a path where it
/// is asked for AND where it lands ([`Landing::relative`]).
pub fn resolve_known(root: &Path, subpath: &str) -> Result<Known, BrowseRefusal> {
    use std::io::ErrorKind::NotFound;
    let root: PathBuf = root.components().collect();
    let root = root.as_path();
    let target = lexical_join(root, subpath)?;
    let unreadable = |error: std::io::Error| BrowseRefusal::Unreadable {
        reason: error.to_string(),
    };
    let canonical_root = match root.canonicalize() {
        Ok(canonical) if canonical.is_dir() => canonical,
        Ok(_) => {
            return Err(BrowseRefusal::Unreadable {
                reason: format!("{} is not a folder", root.display()),
            })
        }
        Err(error) if error.kind() == NotFound => {
            absent_root(root)?;
            return Ok(Known::RootAbsent);
        }
        Err(error) => return Err(unreadable(error)),
    };
    let canonical_target = match target.canonicalize() {
        Ok(target) => target,
        Err(error) if error.kind() == NotFound => {
            landing(root, subpath)?;
            return Ok(Known::Absent);
        }
        Err(error) => return Err(unreadable(error)),
    };
    let inside = canonical_target
        .strip_prefix(&canonical_root)
        .map_err(|_| BrowseRefusal::EscapesAfterResolution {
            subpath: subpath.to_owned(),
        })?;
    if inside
        .components()
        .any(|part| part.as_os_str().to_str().is_none())
    {
        return Err(BrowseRefusal::Unspellable {
            subpath: subpath.to_owned(),
        });
    }
    let depth = canonical_root.components().count();
    Ok(Known::Landed(Landing {
        path: canonical_target,
        depth,
    }))
}

/// What the disk positively says of a path ([`resolve_known`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Known {
    /// It is there, and lands here.
    Landed(Landing),
    /// Nothing is at it: `NotFound` under a root that is there.
    Absent,
    /// The root itself is not there, under a folder that is.
    RootAbsent,
}

impl Known {
    /// For a caller whose root's absence is the entry's: a missing agents
    /// folder holds no `_drive.toml`.
    pub fn landed(self) -> Option<Landing> {
        match self {
            Self::Landed(landing) => Some(landing),
            Self::Absent | Self::RootAbsent => None,
        }
    }

    /// For a caller whose root must be there — a drive's checkout: a root
    /// that is not is refused, since nothing under it can be told apart
    /// from a drive that is gone.
    pub fn under_root(self) -> Result<Option<Landing>, BrowseRefusal> {
        match self {
            Self::Landed(landing) => Ok(Some(landing)),
            Self::Absent => Ok(None),
            Self::RootAbsent => Err(BrowseRefusal::Unreadable {
                reason: "the folder is not there".to_owned(),
            }),
        }
    }
}

/// Where a path [`resolve_known`] found lands: canonical, under the
/// canonical root, every name of it spellable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landing {
    path: PathBuf,
    /// How many components of `path` are the canonical root's.
    depth: usize,
}

impl Landing {
    /// The canonical path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn into_path(self) -> PathBuf {
        self.path
    }

    /// Its root-relative names, `/`-joined (empty for the root): what a
    /// caller admits and reads by, so it composes no path itself (AD-65).
    pub fn relative(&self) -> String {
        let mut names = String::new();
        for part in self.path.components().skip(self.depth) {
            if !names.is_empty() {
                names.push('/');
            }
            // Spellable: `resolve_known` refused any other landing.
            names.push_str(&part.as_os_str().to_string_lossy());
        }
        names
    }
}

/// `Ok` when `root`, which does not canonicalize, is positively not there:
/// the nearest of it and its ancestors that is on the disk is a folder,
/// reached through no dangling link. Refused when that one is `root`
/// itself — a dangling link — or is not a folder, does not resolve, or
/// cannot be looked at: then what `root` names is not known.
fn absent_root(root: &Path) -> Result<(), BrowseRefusal> {
    let refused = |reason: String| BrowseRefusal::Unreadable { reason };
    for ancestor in root.ancestors() {
        match ancestor.symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(refused(format!("{}: {error}", ancestor.display()))),
            Ok(_) if ancestor == root => {
                return Err(refused(format!("{} is a link to nothing", root.display())))
            }
            Ok(_) => {
                return match ancestor.canonicalize() {
                    Ok(canonical) if canonical.is_dir() => Ok(()),
                    Ok(_) => Err(refused(format!("{} is not a folder", ancestor.display()))),
                    Err(error) => Err(refused(format!("{}: {error}", ancestor.display()))),
                }
            }
        }
    }
    Ok(())
}

/// The lexical half of [`resolve`], on its own.
///
/// Split out because [`resolve`] is not the only caller that needs it: a create
/// names a file that does not exist yet, so it cannot be canonicalized, and
/// [`crate::write`] has to be able to refuse `..` in a path it is about
/// to write to *before* the disk is consulted. Two copies of this loop would be
/// two chances for one of them to be wrong, and the one that is wrong would be
/// the one guarding the write.
///
/// Never canonicalizes and never touches the disk, so it is total over strings.
pub fn lexical_join(root: &Path, subpath: &str) -> Result<PathBuf, BrowseRefusal> {
    let mut target = root.to_path_buf();
    for name in plain_segments(subpath)? {
        target.push(name);
    }
    Ok(target)
}

/// Where `subpath` lands under `root` on the disk, as root-relative names:
/// the canonical form of the deepest part of it that exists — through every
/// link on the way — followed by the names that are not there yet.
///
/// [`resolve`]'s two halves for a path a write may be about to create, which
/// cannot be canonicalized whole: the lexical test first, then the landing of
/// what exists, which must be under the canonical root. A caller fencing
/// names inside the root asks this rather than the requested string, because
/// a folder link (`workspace/back -> ..`) makes the two differ. A dangling
/// link is refused: what it would lead to cannot be vouched for.
pub fn landing(root: &Path, subpath: &str) -> Result<Vec<String>, BrowseRefusal> {
    let segments = plain_segments(subpath)?;
    let escapes = || BrowseRefusal::EscapesAfterResolution {
        subpath: subpath.to_owned(),
    };
    let canonical_root = root
        .canonicalize()
        .map_err(|error| BrowseRefusal::Unreadable {
            reason: error.to_string(),
        })?;
    let mut existing = root.to_path_buf();
    let mut found = 0;
    for name in &segments {
        let next = existing.join(name);
        if next.symlink_metadata().is_err() {
            break;
        }
        existing = next;
        found += 1;
    }
    let resolved = existing.canonicalize().map_err(|_| escapes())?;
    let inside = resolved
        .strip_prefix(&canonical_root)
        .map_err(|_| escapes())?;
    let mut names = Vec::with_capacity(segments.len());
    for name in inside
        .components()
        .map(Component::as_os_str)
        .chain(segments[found..].iter().copied())
    {
        names.push(
            name.to_str()
                .ok_or_else(|| BrowseRefusal::Unspellable {
                    subpath: subpath.to_owned(),
                })?
                .to_owned(),
        );
    }
    Ok(names)
}
/// Split a profile-relative subpath into its components, refusing any that is
/// not a plain name.
///
/// Each segment must parse to exactly one `Component::Normal`, which is a single
/// test that covers every shape an escape takes and covers it *per platform*:
/// `..` is `ParentDir`, `.` is `CurDir`, a leading `/` is `RootDir`, `C:` is
/// `Prefix`, and on Windows — where `\` is a separator and on Linux an ordinary
/// filename character — `a\b` yields two components there and one here, which is
/// exactly the difference that matters.
///
/// The empty subpath is the root and yields no segments. Every other empty
/// segment — a leading, trailing or doubled `/` — is refused, because tolerating
/// it would make `"a//b"` and `"a/b"` two spellings of one request and make
/// `"/"` a second spelling of the root.
///
/// # Why `U+FFFD` is refused here and not somewhere more specific
///
/// This function is the single place a root and a caller-supplied subpath meet
/// (AD-65), which is why [`crate::write`] borrows it rather than writing
/// its own loop. That makes it the only place a rule can be added once and hold
/// for looking *and* for writing. A subpath carrying the replacement character
/// may be the directory listing's own lossy rendering of a name that is not valid
/// UTF-8, and joining a rendering can land on a different real file — so the
/// refusal belongs at the join, not at the listing. See
/// [`BrowseRefusal::Unspellable`] and [`crate::names`].
pub fn plain_segments(subpath: &str) -> Result<Vec<&OsStr>, BrowseRefusal> {
    let escapes = || BrowseRefusal::Escapes {
        subpath: subpath.to_owned(),
    };
    if subpath.is_empty() {
        return Ok(Vec::new());
    }
    // Before the component walk, because this is a property of the whole
    // string and refusing it early keeps the per-segment loop about escapes.
    if crate::names::is_lossy_rendering(subpath) {
        return Err(BrowseRefusal::Unspellable {
            subpath: subpath.to_owned(),
        });
    }
    let mut out = Vec::new();
    for segment in subpath.split('/') {
        if segment.is_empty() {
            return Err(escapes());
        }
        let mut components = Path::new(segment).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => out.push(name),
            _ => return Err(escapes()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parent_traversal_is_refused_at_every_position() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir(root.path().join("inner")).expect("inner");
        for subpath in ["..", "../..", "inner/..", "inner/../..", "../inner"] {
            assert_eq!(
                resolve(root.path(), subpath),
                Err(BrowseRefusal::Escapes {
                    subpath: subpath.to_owned()
                }),
                "{subpath} must not resolve"
            );
        }
    }

    #[test]
    fn absolute_and_degenerate_subpaths_are_refused() {
        let root = tempfile::tempdir().expect("temp");
        for subpath in ["/etc", "/", "inner/", "/inner", "inner//child", ".", "./x"] {
            assert!(
                matches!(
                    resolve(root.path(), subpath),
                    Err(BrowseRefusal::Escapes { .. })
                ),
                "{subpath} must not resolve"
            );
        }
    }

    #[test]
    fn the_root_itself_is_the_empty_subpath() {
        let root = tempfile::tempdir().expect("temp");
        assert_eq!(
            resolve(root.path(), "").expect("root resolves"),
            Some(root.path().canonicalize().expect("canonical root"))
        );
    }

    #[test]
    fn a_plain_descendant_resolves_under_the_root() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::create_dir_all(root.path().join("a/b")).expect("tree");
        let resolved = resolve(root.path(), "a/b")
            .expect("resolves")
            .expect("exists");
        assert!(resolved.starts_with(root.path().canonicalize().expect("canonical")));
        assert!(resolved.ends_with("a/b"));
    }
    /// R95K3-01: [`resolve_known`] calls a root absent only where the disk
    /// says so — a root not there under a folder that is, and an entry not
    /// there under a root that is. A root that is a link to nothing, a file
    /// where the root's folder should be, or a root under a file or a
    /// dangling link is not known, never an absence. R244: so in every
    /// spelling a profile's subfolder may take — a trailing separator, a
    /// `./` — while those spellings of a root that is there, or positively
    /// is not, answer as the plain one does.
    #[cfg(unix)]
    #[test]
    fn a_root_the_disk_cannot_vouch_for_is_no_absence() {
        let drive = tempfile::tempdir().expect("drive");
        let at = |rel: &str| drive.path().join(rel);
        std::fs::create_dir(at("agents")).expect("root");
        std::fs::write(at("file"), "x").expect("file");
        std::os::unix::fs::symlink(at("nowhere"), at("dangling")).expect("link");

        for spelling in ["{}", "{}/", "./{}", "./{}//"] {
            let root = |name: &str| at(&spelling.replace("{}", name));
            assert_eq!(
                resolve_known(&root("agents"), "_drive.toml"),
                Ok(Known::Absent),
                "{spelling}"
            );
            assert_eq!(
                resolve_known(&root("absent"), "_drive.toml"),
                Ok(Known::RootAbsent),
                "{spelling}"
            );
            assert_eq!(
                resolve_known(&root("absent/deeper"), "_drive.toml"),
                Ok(Known::RootAbsent),
                "{spelling}"
            );
            for name in ["dangling", "file", "file/agents", "dangling/agents"] {
                // The root itself as well: a file where the root's folder
                // should be never lands as one.
                for subpath in ["_drive.toml", ""] {
                    let known = resolve_known(&root(name), subpath);
                    assert!(
                        matches!(known, Err(BrowseRefusal::Unreadable { .. })),
                        "{spelling} {name} {subpath:?}: {known:?}"
                    );
                }
            }
        }
    }

    /// R218 carried to every caller of the one resolver: only `NotFound` is
    /// an absence — an entry whose path runs through a file (`ENOTDIR`) is
    /// not known, so neither a search's config nor a promotion's
    /// declaration is taken for absent there; a drive's checkout that is
    /// gone is refused where the root must be there.
    #[cfg(unix)]
    #[test]
    fn only_not_found_is_an_absence() {
        let drive = tempfile::tempdir().expect("drive");
        std::fs::write(drive.path().join(".okf"), "x").expect("file");
        let known = resolve_known(drive.path(), ".okf/config.yaml");
        assert!(
            matches!(known, Err(BrowseRefusal::Unreadable { .. })),
            "{known:?}"
        );
        let gone = resolve_known(&drive.path().join("gone"), ".okf/config.yaml");
        assert_eq!(gone.clone().map(Known::landed), Ok(None));
        assert!(
            matches!(
                gone.and_then(Known::under_root),
                Err(BrowseRefusal::Unreadable { .. })
            ),
            "a gone checkout"
        );
    }
    /// The refusal is about the replacement character and nothing else.
    #[test]
    fn ordinary_names_still_join_including_non_ascii_ones() {
        // Non-ASCII is not non-UTF-8. A Polish or Japanese filename is text and
        // a rule that refused it would break far more than it fixed.
        for subpath in ["notes/zaświadczenie.pdf", "議事録/2026.md", "plain.txt"] {
            assert!(
                plain_segments(subpath).is_ok(),
                "{subpath} is ordinary text and must still resolve"
            );
        }
    }

    /// The refusal sentence has to say what to do about it, not only that it
    /// happened — this string is rendered verbatim by the shell.
    #[test]
    fn the_unspellable_refusal_says_why_it_will_not_act() {
        let said = BrowseRefusal::Unspellable {
            subpath: "a\u{FFFD}.txt".to_owned(),
        }
        .to_string();
        assert!(said.contains("a\u{FFFD}.txt"), "got: {said}");
        assert!(said.contains("could reach a different file"), "got: {said}");
    }
}
