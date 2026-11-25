//! The names keeper's own machinery and git read unfiltered (story 119.1; AD-230).
//!
//! Moved from `keeper_sync::lfs::stage` and `keeper_sync::profile::folder`
//! with the virtualization grammar that reads them.

use std::path::Path;

/// The per-folder directory both config files live in. The same name
/// [`crate::exclude`] excludes and carves `*.toml` out of.
pub const FOLDER_CONFIG_DIR: &str = ".keeper";

/// Files git and keeper read for their own configuration, before any filter
/// runs.
///
/// git and gitoxide read these straight out of the worktree (or the index) as
/// bytes. No smudge filter is ever applied to them, so a path in here that is
/// tracked through LFS is not stored content that comes back on checkout — it
/// is a file whose *only* readable content is now a pointer. `.gitattributes`
/// is the one that bites: git parses the pointer as rules, so
/// `version https://git-lfs.github.com/spec/v1` becomes a pattern with two
/// attributes that are not valid attribute names, and every rule the file was
/// there to carry silently stops applying to the whole subtree below it.
///
/// It is not a hypothetical. On the owner's drive two generated eclipse
/// `.gitattributes` files crossed the threshold, keeper wrote itself an
/// anchored rule for each, and every later git or gitoxide operation on the
/// repository printed `is not a valid attribute name` — while the subtree they
/// governed had no attributes at all. 1 314 669 such lines on 2026-08-27/28.
///
/// keeper's own two are here for the same reason (Story 70.4, AD-230):
/// `.lfsconfig` is read unfiltered by the endpoint resolver, so a pointerised
/// one reads as "no LFS endpoint"; `.keepervirtual` is compiled from the
/// worktree, so a pointerised one is a policy that erased itself. Both used to
/// be protected from *virtualization* and not from *conversion*, which is the
/// step that produces the pointer the first protection feared.
pub const CONTROL_FILES: [&str; 5] = [
    ".gitattributes",
    ".gitignore",
    ".gitmodules",
    ".lfsconfig",
    crate::virtual_policy::VIRTUAL_PATTERN_FILE,
];

/// Is this a file git or keeper reads unfiltered, and therefore must never be
/// a pointer — from any door?
///
/// Matched on the file name at any depth, which is how git finds them: a
/// `.gitattributes` is read in every directory it appears in. Anything under
/// a `.git` or `.keeper` component counts too: `.keeper/keeper.toml` is the
/// folder's own configuration and carries the `toml` **extension**, so one
/// oversized TOML anywhere in the repository writes `*.toml filter=lfs` and
/// every folder-config file in the tree is then routed by attribute without
/// ever crossing the threshold itself.
///
/// The ONE predicate (AD-230). Before 70.4 there were two lists that had to
/// agree — this one, and `virtual_policy::is_control_file`'s wider one — and
/// only one of them was complete. Every routing door consults this:
/// `keeper_sync::lfs::stage`'s `LfsPolicy::applies`, `already_routed`,
/// `mismatched_filtered_paths`, `unconverted_after_repair`,
/// `Engine::ensure_lfs_rule`, and the virtualization policy delegates here.
pub fn is_control_file(path: &Path) -> bool {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| CONTROL_FILES.contains(&name))
    {
        return true;
    }
    path.components().any(|part| {
        matches!(
            part,
            std::path::Component::Normal(part)
                if part == ".git" || part == FOLDER_CONFIG_DIR
        )
    })
}
