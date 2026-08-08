//! The profile's `subpaths[]`, interpreted exactly once (Story 27.2, AD-47).
//!
//! # One list, two consumers
//!
//! A partial profile names the directories it wants in
//! [`SyncProfile::subpaths`](crate::profile::SyncProfile::subpaths), and that
//! one list has to drive two entirely different mechanisms:
//!
//! * the **cone sparse-checkout**, applied by `git sparse-checkout set --cone`
//!   through the shim ([`crate::git::cli::GitCli::sparse_set`]), which decides
//!   what git materializes in the working tree. It goes through the shim
//!   because nothing in gitoxide reads `.git/info/sparse-checkout` at all
//!   (AD-41);
//! * the **LFS path filter** (Story 25.5), which decides which large objects are
//!   worth downloading. It is needed *in addition* to the checkout because
//!   git-lfs is entirely sparse-checkout-unaware: a cone checkout on its own
//!   reduces no LFS traffic whatsoever.
//!
//! Two consumers reading one list is only safe if they agree on what the list
//! *means*, and the meaning is not the obvious one. So it is written down here,
//! once, and both consumers ask this type rather than re-deriving it.
//!
//! # What cone mode actually materializes
//!
//! `git sparse-checkout set --cone media/video` does **not** materialize
//! `media/video` and nothing else. It expands to these patterns:
//!
//! ```text
//! /*
//! !/*/
//! /media/
//! !/media/*/
//! /media/video/
//! ```
//!
//! which is three rules, not one:
//!
//! 1. every file at the repository **root** is materialized (`/*` minus `!/*/`,
//!    i.e. root files but not root directories);
//! 2. every file sitting **directly in an ancestor** of a cone root is
//!    materialized — `media/README.md` is present even though only
//!    `media/video` was asked for;
//! 3. everything **under** a cone root is materialized, recursively.
//!
//! Rules 1 and 2 exist because a cone is defined by whole directories and git
//! will not hide the path leading to one. A path filter that implemented only
//! rule 3 would disagree with the checkout on exactly those files, and the two
//! consumers would no longer be reading the same list — which is the failure
//! this module exists to prevent.
//!
//! Stated as one predicate, and that is what [`SparseCone::includes`] is: a path
//! is inside the cone when its **directory** and some cone root are
//! prefix-comparable — either the directory is at or under the root (rule 3), or
//! it is an ancestor of the root (rules 1 and 2, the repository root being the
//! ancestor of everything).

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::{Component, Path};

/// One profile's `subpaths[]`, normalized into the cone git will be given.
///
/// An empty cone means the whole repository: that is what an empty `subpaths[]`
/// has always meant on the profile, and it is also what a cone rooted at the
/// repository root would materialize anyway.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SparseCone {
    /// Normalized cone roots: slash-separated, no leading or trailing slash, no
    /// empty or `.` components, sorted, and with any root that is nested inside
    /// another dropped. Empty means the whole repository.
    roots: Vec<String>,
}

impl SparseCone {
    /// Interpret a profile's `subpaths[]`.
    ///
    /// Normalization is not cosmetic. The roots are handed to `git` verbatim by
    /// [`Self::roots`] *and* compared component-wise by [`Self::includes`], so
    /// `"docs/"`, `"./docs"` and `"docs"` have to collapse to one spelling or
    /// the two consumers would disagree about a trailing slash. It also makes
    /// [`Self::is_already_applied`] a stable comparison against what git wrote
    /// back, instead of a spelling contest keeper loses on every open.
    ///
    /// A subpath that normalizes away to nothing — `""`, `"."`, `"./"` — names
    /// the repository root, and a cone rooted there materializes everything. It
    /// widens the whole cone to full rather than being dropped, because
    /// dropping it would silently narrow the profile to the *other* subpaths,
    /// which is the one outcome the user cannot have meant.
    ///
    /// Nested roots are dropped for the same reason git ignores them: a cone is
    /// a union, so `["media", "media/video"]` and `["media"]` materialize
    /// exactly the same tree. Dropping the redundant one here keeps the
    /// comparison against git's own pattern file exact.
    pub fn new(subpaths: &[String]) -> Self {
        let mut roots: Vec<String> = Vec::with_capacity(subpaths.len());
        for subpath in subpaths {
            let normalized = normalize(subpath);
            if normalized.is_empty() {
                // The repository root: the cone is the whole repository, and
                // nothing else in the list can narrow that.
                return Self::default();
            }
            roots.push(normalized);
        }
        roots.sort_unstable();
        roots.dedup();
        // Checked against every root kept so far rather than only the previous
        // one: sorted order does not put a parent adjacent to its children,
        // because a sibling can sort between them (`a` < `a-b` < `a/c`).
        let mut kept: Vec<String> = Vec::with_capacity(roots.len());
        for root in roots {
            if kept.iter().any(|parent| is_under(&root, parent)) {
                continue;
            }
            kept.push(root);
        }
        Self { roots: kept }
    }

    /// Whether this cone is the whole repository, so there is nothing to
    /// restrict and no sparse-checkout to apply.
    pub fn is_full(&self) -> bool {
        self.roots.is_empty()
    }

    /// The cone roots, in the exact form `git sparse-checkout set --cone` is
    /// given them.
    pub fn roots(&self) -> &[String] {
        &self.roots
    }

    /// Whether a repository-relative path is inside the cone.
    ///
    /// The LFS path filter's entire question, and — because it is derived from
    /// the same roots the shim is handed — the same answer git's checkout gives.
    /// See the module docs for why "inside" includes root files and the files
    /// along the way to a cone root.
    ///
    /// `path` names a **file**: its last component is the file name and plays no
    /// part, exactly as in git's own cone matching.
    pub fn includes(&self, path: &Path) -> bool {
        if self.roots.is_empty() {
            return true;
        }
        let directory = path.parent().unwrap_or_else(|| Path::new(""));
        self.roots
            .iter()
            .any(|root| prefix_comparable(directory, root))
    }

    /// Whether `patterns` — the content of `.git/info/sparse-checkout` — already
    /// materializes exactly this cone.
    ///
    /// The reason keeper asks at all: applying the cone is idempotent in effect
    /// but not in cost. `git sparse-checkout set` re-reads the tree and re-stats
    /// the working copy every time, and the repository is opened on every sync
    /// tick — on a pendrive (AD-48) that is the difference between a background
    /// daemon and a device that never spins down. So the cone is re-applied only
    /// when it actually differs.
    ///
    /// A file that is not in the shape `set --cone` writes answers `false`, and
    /// the caller re-applies. That covers a hand-edited non-cone pattern set,
    /// which keeper cannot honour anyway — the profile declares the cone, and
    /// git itself refuses to mix cone and non-cone patterns.
    pub fn is_already_applied(&self, patterns: &str) -> bool {
        let Some(applied) = applied_roots(patterns) else {
            return false;
        };
        applied.len() == self.roots.len()
            && self
                .roots
                .iter()
                .all(|root| applied.contains(root.as_str()))
    }
}

/// Collapse one subpath to its canonical cone spelling.
///
/// Backslashes are left alone rather than treated as separators: git's index
/// paths are slash-separated on every platform, and a backslash in a path is a
/// legal file name character on the ones that matter here.
fn normalize(subpath: &str) -> String {
    let mut out = String::with_capacity(subpath.len());
    for component in subpath.split('/') {
        let component = component.trim();
        if component.is_empty() || component == "." {
            continue;
        }
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(component);
    }
    out
}

/// Whether `candidate` is `parent` itself or nested inside it.
fn is_under(candidate: &str, parent: &str) -> bool {
    candidate
        .strip_prefix(parent)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Whether a directory and a cone root are prefix-comparable, component-wise.
///
/// Component-wise rather than by string prefix, so `media/videos` is not
/// mistaken for something under `media/video`.
fn prefix_comparable(directory: &Path, root: &str) -> bool {
    let mut directory = directory.components().filter_map(named);
    let mut root = root.split('/');
    loop {
        match (directory.next(), root.next()) {
            // One side ran out: the shorter is a prefix of the longer, which is
            // true both for a file under the root and for a file in one of the
            // directories leading to it.
            (None, _) | (_, None) => return true,
            (Some(from_path), Some(from_root)) if from_path == from_root => continue,
            _ => return false,
        }
    }
}

/// The name of a path component, for the components that have one.
///
/// A repository-relative index path holds nothing but `Normal` components — the
/// profile refuses a subpath that is absolute or escapes (see
/// `SyncProfile::validate`) — so the discarded arms are unreachable rather than
/// meaningful. A `.` is dropped because it names the directory it sits in.
///
/// Lossy, and deliberately so: a non-UTF-8 component must still occupy its
/// position, or `a/<invalid>/b` would silently compare as `a/b` and a path could
/// match a cone it is not in.
fn named(component: Component<'_>) -> Option<Cow<'_, str>> {
    match component {
        Component::Normal(name) => Some(name.to_string_lossy()),
        _ => None,
    }
}

/// The cone roots a `.git/info/sparse-checkout` file materializes, or `None`
/// when the file is not in the shape `set --cone` writes.
///
/// The inverse of the expansion documented at the top of this module: every
/// directory named by a `/dir/` line is materialized recursively **unless** a
/// `!/dir/*/` line takes its subdirectories back, which is how git spells "this
/// directory is only on the way to a deeper cone root".
///
/// Borrows from `patterns`; nothing here allocates a string.
fn applied_roots(patterns: &str) -> Option<BTreeSet<&str>> {
    let mut lines = patterns
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    // Cone mode always opens with "root files, but no directories yet".
    if lines.next()? != "/*" || lines.next()? != "!/*/" {
        return None;
    }

    let mut recursive = BTreeSet::new();
    let mut on_the_way = BTreeSet::new();
    for line in lines {
        // git escapes a glob metacharacter in a directory name with a
        // backslash. Un-escaping it correctly is not worth guessing at, and the
        // caller's fallback — re-apply the cone — is always safe.
        if line.contains('\\') {
            return None;
        }
        if let Some(directory) = line.strip_prefix("!/").and_then(|l| l.strip_suffix("/*/")) {
            on_the_way.insert(directory);
            continue;
        }
        // Anything that is neither an on-the-way marker nor a recursive root is
        // a cone this parser does not model, and the caller's fallback — write
        // the cone again — is always safe. `?` is the refusal.
        let directory = line.strip_prefix('/').and_then(|l| l.strip_suffix('/'))?;
        recursive.insert(directory);
    }
    Some(recursive.difference(&on_the_way).copied().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cone(subpaths: &[&str]) -> SparseCone {
        SparseCone::new(
            &subpaths
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<String>>(),
        )
    }

    #[test]
    fn an_empty_subpath_list_is_the_whole_repository() {
        let full = cone(&[]);
        assert!(full.is_full());
        assert!(full.roots().is_empty());
        for path in ["README.md", "media/video/v.mp4", "anything/at/all.bin"] {
            assert!(full.includes(Path::new(path)), "{path} must be included");
        }
    }

    #[test]
    fn a_cone_includes_its_own_subtree_the_root_files_and_the_way_in() {
        // Exactly what `git sparse-checkout set --cone media/video` materializes
        // — see the module docs. Getting any of these three wrong makes the LFS
        // path filter disagree with the checkout.
        let cone = cone(&["media/video"]);

        assert!(cone.includes(Path::new("media/video/clip.mp4")), "subtree");
        assert!(cone.includes(Path::new("media/video/raw/a.mov")), "deeper");
        assert!(cone.includes(Path::new("README.md")), "a root file");
        assert!(cone.includes(Path::new("media/NOTES.md")), "on the way in");

        assert!(!cone.includes(Path::new("media/audio/a.flac")), "a sibling");
        assert!(!cone.includes(Path::new("docs/d.md")), "another root dir");
        assert!(!cone.includes(Path::new("docs/deep/d.md")), "and deeper");
    }

    #[test]
    fn a_sibling_sharing_a_name_prefix_is_outside_the_cone() {
        // The bug a string-prefix comparison would ship: `media/videos` is not
        // inside `media/video`, and a filter that thought so would download the
        // very objects the profile excluded.
        let cone = cone(&["media/video"]);
        assert!(!cone.includes(Path::new("media/videos/other.mp4")));
        assert!(!cone.includes(Path::new("media/video-old/other.mp4")));
    }

    #[test]
    fn several_cone_roots_are_a_union() {
        let cone = cone(&["docs", "media/video"]);
        assert!(cone.includes(Path::new("docs/d.md")));
        assert!(cone.includes(Path::new("media/video/v.mp4")));
        assert!(!cone.includes(Path::new("media/audio/a.flac")));
    }

    #[test]
    fn spellings_of_the_same_directory_normalize_to_one_root() {
        for spelling in [
            "media/video",
            "media/video/",
            "./media/video",
            "media//video",
        ] {
            assert_eq!(
                cone(&[spelling]).roots(),
                ["media/video"],
                "{spelling} must normalize"
            );
        }
    }

    #[test]
    fn a_root_nested_inside_another_is_dropped_as_redundant() {
        // A cone is a union, so the nested one materializes nothing extra —
        // and leaving it in would make the comparison against git's own
        // pattern file, which drops it, never match.
        //
        // `media-old` is in the list on purpose: it sorts *between* `media` and
        // `media/video` (`-` is 0x2D, `/` is 0x2F), so a dedup that only looked
        // at the previously kept root would keep `media/video` after all.
        let cone = cone(&["media", "media-old", "media/video", "media/video/raw"]);
        assert_eq!(cone.roots(), ["media", "media-old"]);
        assert!(cone.includes(Path::new("media/audio/a.flac")));
    }

    #[test]
    fn a_subpath_naming_the_repository_root_widens_the_cone_to_everything() {
        // Narrowing to the other entries instead would silently exclude
        // directories the user just asked for in the same list.
        for root_spelling in ["", ".", "./", "/"] {
            let cone = cone(&["docs", root_spelling]);
            assert!(cone.is_full(), "{root_spelling:?} must mean everything");
            assert!(cone.includes(Path::new("media/audio/a.flac")));
        }
    }

    /// Verbatim from `git version 2.53.0` after
    /// `git sparse-checkout set --cone media/video nested/deep`.
    const APPLIED: &str = "/*
!/*/
/media/
!/media/*/
/nested/
!/nested/*/
/media/video/
/nested/deep/
";

    #[test]
    fn the_cone_git_wrote_back_is_recognized_as_already_applied() {
        // What keeps a sync tick from re-running `sparse-checkout set` — and
        // re-stating the whole working tree — on every single open.
        assert!(cone(&["media/video", "nested/deep"]).is_already_applied(APPLIED));
        // Order in the profile is not order in the file.
        assert!(cone(&["nested/deep", "media/video"]).is_already_applied(APPLIED));
    }

    #[test]
    fn a_different_cone_is_not_already_applied() {
        assert!(
            !cone(&["media/video"]).is_already_applied(APPLIED),
            "a narrower cone must be re-applied"
        );
        assert!(
            !cone(&["media/video", "nested/deep", "docs"]).is_already_applied(APPLIED),
            "a wider cone must be re-applied"
        );
        assert!(
            !cone(&["media", "nested/deep"]).is_already_applied(APPLIED),
            "a root that is on the way in is not a root that is materialized"
        );
    }

    #[test]
    fn patterns_that_are_not_cone_shaped_are_never_treated_as_applied() {
        // Each of these would otherwise be read as "nothing to do", leaving the
        // working tree disagreeing with the profile indefinitely.
        for patterns in [
            // Empty, and a truncated preamble.
            "",
            "/*\n",
            // No cone preamble at all.
            "/media/video/\n",
            // A non-cone pattern mixed in.
            "/*\n!/*/\n*.mp4\n",
            // An escaped glob metacharacter in a directory name.
            "/*\n!/*/\n/media/vid\\*eo/\n",
        ] {
            assert!(
                !cone(&["media/video"]).is_already_applied(patterns),
                "{patterns:?} must force a re-apply"
            );
        }
    }
}
