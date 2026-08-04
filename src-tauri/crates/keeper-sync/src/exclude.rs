//! Tier 0 of the completeness gate — name and shape exclusion (Story 26.1,
//! AD-45).
//!
//! One compiled [`globset::GlobSet`] answers "is this path something the world
//! has already told us not to sync?". An excluded path is **invisible**: it is
//! not staged, not queued, not counted, and never reported as pending. That is
//! the difference between an exclusion and a deferral, and it matters —
//! a `.part` file that showed up as "1 file pending" forever would be a bug
//! report, not a feature.
//!
//! # This tier is not, and cannot be, sufficient
//!
//! The built-in list is seeded from Nextcloud's shipped `sync-exclude.lst`, the
//! best-curated corpus in the industry, and it defeats every browser and every
//! office suite. It does not defeat `curl -O` or `wget`, which write their
//! **final filename from byte 0** and have no partial-file convention at all.
//! Those are the two most common large-file fetchers on Linux. Tier 0 is
//! therefore a cheap filter that removes known noise, never a completeness
//! proof; only [`crate::stability::verify_while_reading`] is that.
//!
//! # Directory-shaped conventions need subtree rules
//!
//! Safari's in-progress download is a *package directory* named `*.download`
//! containing the partial data, so a suffix rule matches the wrapper and leaks
//! every file inside it. The same is true of `.Spotlight-V100`, `.fseventsd`,
//! `.Trashes` and friends. Each of those carries an explicit `/**` subtree rule
//! below, and adding a new directory-shaped convention means adding both forms.

use std::borrow::Cow;
use std::path::{Component, Path};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::error::{Result, SyncError};

/// The always-on exclusion corpus.
///
/// Public so a unit test can assert every entry compiles and so the settings UI
/// can show the user exactly what is filtered before their own patterns apply.
///
/// A pattern **without** a `/` matches a basename at any depth; a pattern
/// **with** a `/` is anchored at the repository root. That is gitignore's rule
/// and users already know it.
pub const BUILTIN_EXCLUDES: &[&str] = &[
    // --- Partial downloads -------------------------------------------------
    // The classic tier-0 corpus. Present because these names are *promises*
    // from the writing application; absent from it are curl and wget, which
    // make no such promise (see the module docs).
    "*.crdownload",     // Chrome, Chromium, Edge
    "*.part",           // Firefox, wget --continue
    "*.filepart",       // Firefox (legacy), Nextcloud desktop client
    "*.partial",        // rclone, legacy IE/Edge
    "*.download",       // Safari, matched as a name...
    "**/*.download/**", // ...and as a subtree, because Safari's is a package
    // directory: without this rule the wrapper is skipped and every partial
    // byte inside it is committed.
    "*.crswap", // Chrome File System Access API swap file
    // --- Sync tools (ours and other people's) ------------------------------
    ".syncthing.*.tmp",
    "~syncthing~*.tmp",
    ".keeper.*.tmp", // our own atomic-staging prefix; committing it would
    // make the engine race with itself.
    // --- Office and creative-suite lock / owner files ----------------------
    "~$*",       // MS Office owner file
    "~WRD*.tmp", // Word autorecovery scratch
    ".~lock.*#", // LibreOffice
    "*.idlk",    // Adobe InDesign
    "*.prlock",  // Adobe Premiere
    "*.dwl",     // AutoCAD
    "*.dwl2",    // AutoCAD
    "*~lock~",
    // --- Editors -----------------------------------------------------------
    ".*.sw?",     // vim swap (.foo.swp, .foo.swo, .foo.swn)
    ".*.*sw?",    // vim swap for names that already contain a dot
    "*.kate-swp", // Kate
    "*~",         // emacs and countless others
    // --- macOS metadata ----------------------------------------------------
    ".DS_Store",
    "._*", // AppleDouble resource forks
    ".apdisk",
    ".Spotlight-V100",
    "**/.Spotlight-V100/**",
    ".fseventsd",
    "**/.fseventsd/**",
    ".TemporaryItems",
    "**/.TemporaryItems/**",
    ".Trashes",
    "**/.Trashes/**",
    ".DocumentRevisions-V100",
    "**/.DocumentRevisions-V100/**",
    "*.sb-*", // APFS/rsync temporary clone suffix
    // --- Linux metadata ----------------------------------------------------
    ".fuse_hidden*", // an unlinked-but-open file on a FUSE mount
    ".nfs*",         // an unlinked-but-open file on NFS ("silly rename")
    ".directory",    // KDE folder settings
    ".Trash-*",
    "**/.Trash-*/**",
    // --- Regenerable dependency and cache trees ----------------------------
    // Names a toolchain *reserves*, never ones a human picks for their own
    // content, so excluding them unconditionally cannot surprise anyone. Both
    // forms, per the module doc: the name so the wrapper directory is skipped,
    // and the subtree so nothing inside it leaks.
    //
    // Deliberately NOT here: `target`, `dist` and `build`. Tier 0 is
    // unconditional and invisible — an excluded path is never staged and never
    // reported as pending — and those three are ordinary English words that a
    // photo library, a woodworking archive or a marketing folder will use for
    // real content. Their build-output meaning is *contextual*: only a `target`
    // beside a `Cargo.toml` is Rust's, only a `build` beside a `CMakeLists.txt`
    // is CMake's, and this tier matches names, not context (it never touches
    // the filesystem, which is what makes it one compiled glob set). The right
    // authority for them is `.gitignore`: git already honours it
    // (`git::repo`'s status walk), every real project already carries those
    // three in it, and the user can read and edit it. So putting them here
    // would buy almost nothing for projects and cost someone their `build/`
    // folder, silently. A user who wants them gone can say so per profile
    // through `SyncProfile::excludes`, which is visible and reversible.
    "node_modules",
    "**/node_modules/**",
    "__pycache__",
    "**/__pycache__/**",
    ".venv",
    "**/.venv/**",
    ".next",
    "**/.next/**",
    ".cache",
    "**/.cache/**",
    // --- Engine-owned trees ------------------------------------------------
    // The repository's own metadata and our volume marker are engine state,
    // never user content. Both forms: anchored for the profile root, and
    // basename-relative so a nested repository's internals are invisible too.
    ".git/**",
    "**/.git/**",
    ".keeper-sync/**",
    "**/.keeper-sync/**",
    // keeper's own per-folder cache. Unlike the two above it can sit at any
    // depth — it belongs to whatever subfolder it describes, not to the profile
    // root — so it takes the name-plus-subtree pairing the regenerable trees
    // use rather than the anchored form. Its contents are machine state
    // (entries are validated against a local inode number), rebuildable from
    // the folder itself, and committing them would sync a cache that is only
    // ever true on the machine that wrote it.
    ".keeper",
    "**/.keeper/**",
];

/// The compiled tier-0 filter for one profile.
///
/// Compiling the corpus builds a regex set, so build it **once per profile**
/// and hold it for the lifetime of the supervisor. Recompiling per scan would
/// dominate the cost of the scan itself on a 100 000-file tree.
#[derive(Debug, Clone)]
pub struct ExcludeSet {
    set: GlobSet,
}

impl ExcludeSet {
    /// Compile the built-in corpus plus `extra` per-profile patterns.
    ///
    /// A malformed user pattern is [`SyncError::Config`], never a panic: these
    /// strings arrive from a hand-edited `config.json` and from
    /// `keeper-syncd`'s TOML, so they are untrusted input on the ordinary
    /// startup path.
    pub fn new(extra: &[String]) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        for pattern in BUILTIN_EXCLUDES {
            add_pattern(&mut builder, pattern)?;
        }
        for pattern in extra {
            add_pattern(&mut builder, pattern)?;
        }
        let set = builder
            .build()
            .map_err(|err| SyncError::Config(format!("could not compile exclude set: {err}")))?;
        Ok(Self { set })
    }

    /// Whether `relative_path` — repository-relative, never absolute — is
    /// filtered out entirely.
    pub fn is_excluded(&self, relative_path: &Path) -> bool {
        let candidate = match_string(relative_path);
        if candidate.is_empty() {
            // The repository root itself is never excluded; matching an empty
            // string against `**/…` patterns would be undefined-ish anyway.
            return false;
        }
        self.set.is_match(candidate.as_str())
    }
}

/// Add one pattern, applying gitignore's anchoring rule.
///
/// `literal_separator(true)` is what makes "basename vs anchored" a real
/// distinction: without it `*` crosses `/`, so `~$*` would swallow an entire
/// subtree whose top directory happened to start with `~$`.
fn add_pattern(builder: &mut GlobSetBuilder, pattern: &str) -> Result<()> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        // A blank line in a user's exclude list means nothing, not "match
        // everything" — silently ignoring it is the only safe reading.
        return Ok(());
    }
    let effective: Cow<'_, str> = if pattern.contains('/') {
        Cow::Borrowed(pattern)
    } else {
        Cow::Owned(format!("**/{pattern}"))
    };
    let glob = GlobBuilder::new(&effective)
        .literal_separator(true)
        .build()
        .map_err(|err| SyncError::Config(format!("invalid exclude pattern {pattern:?}: {err}")))?;
    builder.add(glob);
    Ok(())
}

/// The `/`-separated string a path is matched as, on every platform.
///
/// Built component by component rather than through `Path::display` or
/// `to_string_lossy` because on Windows the platform separator is `\` while
/// every pattern in the corpus — and every pattern a user will ever type — is
/// written with `/`. One profile configuration has to mean the same thing on a
/// Mac, on a Windows desktop and in the Linux daemon, so the match string is
/// normalized here and nowhere else.
fn match_string(relative_path: &Path) -> String {
    let mut out = String::with_capacity(relative_path.as_os_str().len());
    for component in relative_path.components() {
        let part: Cow<'_, str> = match component {
            Component::Normal(part) => part.to_string_lossy(),
            Component::ParentDir => Cow::Borrowed(".."),
            // A repository-relative path has no root, prefix or `.` segment.
            // If one arrives anyway, dropping it keeps the remainder anchored
            // where the caller meant it to be.
            Component::CurDir | Component::RootDir | Component::Prefix(_) => continue,
        };
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(&part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_set() -> ExcludeSet {
        ExcludeSet::new(&[]).expect("built-in corpus must compile")
    }

    fn excluded(set: &ExcludeSet, path: &str) -> bool {
        set.is_excluded(Path::new(path))
    }

    #[test]
    fn every_industry_convention_is_excluded() {
        let set = default_set();
        // One row per convention the corpus claims to cover. If a row here has
        // no corresponding entry in BUILTIN_EXCLUDES the claim is false.
        let cases = [
            ("Downloads/movie.mkv.crdownload", "chrome partial"),
            ("report.part", "firefox partial"),
            ("archive.zip.filepart", "firefox legacy partial"),
            ("big.iso.partial", "rclone partial"),
            ("a.download/x.bin", "safari package subtree"),
            ("deep/nest/b.download/data/part1", "safari package, nested"),
            ("draft.txt.crswap", "chrome file-system-access swap"),
            (".syncthing.notes.txt.tmp", "syncthing temp"),
            ("~syncthing~notes.txt.tmp", "syncthing temp, alt shape"),
            (".keeper.01JABCDEF.tmp", "our own staging prefix"),
            ("~$budget.xlsx", "ms office owner file"),
            ("~WRD0001.tmp", "word autorecovery"),
            (".~lock.notes.odt#", "libreoffice lock"),
            ("art/layout.idlk", "indesign lock"),
            ("cut/edit.prlock", "premiere lock"),
            ("plans/site.dwl", "autocad lock"),
            ("plans/site.dwl2", "autocad lock v2"),
            ("design~lock~", "generic lock suffix"),
            (".notes.txt.swp", "vim swap"),
            (".notes.txt.swo", "vim swap, second generation"),
            ("notes.kate-swp", "kate swap"),
            ("draft.txt~", "emacs backup"),
            (".DS_Store", "macos finder metadata"),
            ("photos/.DS_Store", "macos finder metadata, nested"),
            ("._resourcefork", "appledouble"),
            (".apdisk", "macos disk metadata"),
            (
                ".Spotlight-V100/Store-V2/abc/live.0.indexHead",
                "spotlight subtree",
            ),
            (".fseventsd/0000000000a1b2c3", "fsevents subtree"),
            (".TemporaryItems/folders.501/x", "macos temp subtree"),
            (".Trashes/501/deleted.txt", "macos trash subtree"),
            (
                ".DocumentRevisions-V100/db-V1/x.db",
                "version store subtree",
            ),
            ("clip.mp4.sb-1a2b3c4d-XyZq9", "apfs rsync clone suffix"),
            (".fuse_hidden000012340001", "fuse unlinked-but-open"),
            (".nfs0000000004a1b2c300000001", "nfs silly rename"),
            (".directory", "kde folder settings"),
            (".Trash-1000/files/old.txt", "linux trash subtree"),
            (".git/config", "repository metadata"),
            ("vendor/lib/.git/HEAD", "nested repository metadata"),
            (".keeper-sync/volume.json", "engine volume marker"),
            ("node_modules", "npm tree, as a name"),
            ("node_modules/react/index.js", "npm tree, as a subtree"),
            ("app/node_modules/left-pad/x.js", "npm tree, nested"),
            ("__pycache__/mod.cpython-312.pyc", "python bytecode cache"),
            ("pkg/sub/__pycache__/a.pyc", "python bytecode cache, nested"),
            (".venv/lib/python3.12/site-packages/x.py", "virtualenv"),
            (".next/server/pages/index.js", "next.js build cache"),
            (".cache/babel/abc.json", "generic tool cache"),
        ];
        for (path, why) in cases {
            assert!(excluded(&set, path), "{path} should be excluded ({why})");
        }
    }

    /// The three names the field report asked for that tier 0 must NOT take.
    ///
    /// `target`, `dist` and `build` are ordinary English words. Tier 0 is
    /// unconditional and invisible, so putting them here would silently and
    /// permanently unsync a woodworking archive's `build/`, a design studio's
    /// `dist/`, or a marketing folder's `target/` — with no pending row to
    /// reveal that it happened. `.gitignore` is the authority for them: git
    /// honours it, every real project already lists them there, and a user can
    /// read and change it.
    #[test]
    fn ordinary_english_build_directory_names_are_not_tier_zero() {
        let set = default_set();
        for path in [
            "build/bench-plan.pdf",
            "target/q3-audience.numbers",
            "dist/press-kit.zip",
            "workshop/build/cut-list.txt",
            "campaign/target/brief.docx",
        ] {
            assert!(
                !excluded(&set, path),
                "{path} must stay visible — tier 0 cannot tell a build tree from a folder \
                 someone named after one, and .gitignore already covers the build tree"
            );
        }
        for name in ["build", "dist", "target", "**/build/**"] {
            assert!(
                !BUILTIN_EXCLUDES.contains(&name),
                "{name} is contextual, not a reserved name; it belongs in .gitignore"
            );
        }
    }

    /// Both forms or neither: a name rule alone leaks every file inside the
    /// directory, and a subtree rule alone leaks the directory itself.
    #[test]
    fn every_directory_shaped_convention_carries_a_name_and_a_subtree_rule() {
        for name in [
            "node_modules",
            "__pycache__",
            ".venv",
            ".next",
            ".cache",
            ".Spotlight-V100",
            ".fseventsd",
            ".TemporaryItems",
            ".Trashes",
            ".DocumentRevisions-V100",
            ".keeper",
        ] {
            assert!(
                BUILTIN_EXCLUDES.contains(&name),
                "{name} needs a bare name rule so the directory itself is skipped"
            );
            let subtree = format!("**/{name}/**");
            assert!(
                BUILTIN_EXCLUDES.contains(&subtree.as_str()),
                "{name} needs {subtree} too, or every file inside it is synced"
            );
        }
    }

    /// keeper's own cache directory is machine state living inside a tree the
    /// user syncs, so tier 0 has to make it invisible wherever it sits — at the
    /// profile root or several levels down, since it belongs to the subfolder
    /// it describes rather than to the profile.
    #[test]
    fn keepers_own_cache_directory_is_excluded_wherever_it_sits() {
        let set = default_set();
        for path in [
            ".keeper",
            ".keeper/index.json",
            "sub/.keeper",
            "sub/.keeper/index.json",
            "sub/.keeper/trash/01JABCDEF/draft.md",
        ] {
            assert!(excluded(&set, path), "{path} is keeper's own cache");
        }
        // The name is claimed exactly, not as a prefix: a user file that merely
        // begins with it stays visible, like every other rule in the corpus.
        for path in ["sub/.keeperrc", "keeper/index.json"] {
            assert!(!excluded(&set, path), "{path} is the user's file");
        }
    }

    #[test]
    fn ordinary_files_with_similar_names_are_kept() {
        let set = default_set();
        // The failure mode this guards is the expensive one: an over-broad
        // pattern makes a real user file permanently invisible, with no
        // "pending" state to reveal that it happened.
        let cases = [
            "partition.md",            // *.part must anchor at the end
            "mydownloads/a.txt",       // the .download rule is a suffix, not a substring
            "notes/data.download.txt", // ...and must not fire mid-name
            "crdownload.txt",
            "swap.txt",
            "lockfile.txt",
            ".gitignore", // .git/** must not swallow sibling dotfiles
            ".gitattributes",
            "keeper-sync/notes.md", // the marker rule is a dotted directory
            "src/main.rs",
            "Reports/annual.docx",
            "a~b.txt", // `*~` anchors at the end of the basename
            "docs/spotlight/notes.md",
        ];
        for path in cases {
            assert!(!excluded(&set, path), "{path} must not be excluded");
        }
    }

    #[test]
    fn extra_profile_patterns_apply_with_gitignore_anchoring() {
        let set = ExcludeSet::new(&["*.bak".to_owned(), "build/**".to_owned()])
            .expect("valid user patterns compile");
        // No separator => basename at any depth.
        assert!(excluded(&set, "old.bak"));
        assert!(excluded(&set, "deep/nest/old.bak"));
        // Separator => anchored at the repository root, so a same-named
        // directory further down is still synced.
        assert!(excluded(&set, "build/out.o"));
        assert!(!excluded(&set, "crate/build/out.o"));
        assert!(!excluded(&set, "notes.md"));
    }

    #[test]
    fn an_invalid_user_glob_is_a_config_error_not_a_panic() {
        let err = ExcludeSet::new(&["[oops".to_owned()])
            .expect_err("an unclosed character class must not compile");
        assert!(
            matches!(err, SyncError::Config(_)),
            "expected Config, got {err:?}"
        );
        assert_eq!(err.code(), "config");
        // The offending pattern is echoed so the user can find it; it is a
        // glob, never a secret.
        assert!(err.to_string().contains("[oops"), "message: {err}");

        // globset is deliberately lenient about a stray `**` (it degrades to
        // `*`), so the cases that genuinely fail are the unbalanced ones.
        let err = ExcludeSet::new(&["{a,b".to_owned()])
            .expect_err("unclosed alternates must not compile");
        assert!(matches!(err, SyncError::Config(_)), "got {err:?}");

        let err = ExcludeSet::new(&["[z-a]".to_owned()])
            .expect_err("a reversed character range must not compile");
        assert!(matches!(err, SyncError::Config(_)), "got {err:?}");

        // ...and a valid pattern alongside an invalid one still fails: the
        // whole set is rejected rather than silently applying a subset.
        let err = ExcludeSet::new(&["*.bak".to_owned(), "[oops".to_owned()])
            .expect_err("one bad pattern rejects the whole set");
        assert!(matches!(err, SyncError::Config(_)), "got {err:?}");
    }

    #[test]
    fn matching_is_case_sensitive() {
        let set = default_set();
        assert!(excluded(&set, ".DS_Store"));
        // On a case-sensitive filesystem `.ds_store` is a different, ordinary
        // file. Folding case here would make it silently unsyncable.
        assert!(!excluded(&set, ".ds_store"));
        assert!(!excluded(&set, "REPORT.PART"));
    }

    #[test]
    fn the_match_string_is_slash_separated_on_every_platform() {
        // Built from components rather than a literal so the assertion is
        // meaningful on a platform whose separator is not `/`.
        let path = Path::new("notes").join("drafts").join("a.part");
        assert_eq!(match_string(&path), "notes/drafts/a.part");
        assert!(default_set().is_excluded(&path));
    }

    #[test]
    fn a_leading_current_dir_segment_does_not_break_anchoring() {
        let set = default_set();
        // Directory walkers routinely yield `./x`; the anchored `.git/**` rule
        // must still fire, and must still be anchored.
        assert!(excluded(&set, "./.git/config"));
        assert_eq!(match_string(Path::new("./a/b")), "a/b");
    }

    #[test]
    fn the_repository_root_itself_is_never_excluded() {
        let set = default_set();
        assert!(!excluded(&set, ""));
        assert!(!excluded(&set, "."));
    }

    #[test]
    fn the_builtin_corpus_is_non_empty_and_fully_compilable() {
        // `default_set` already proves compilability; this pins the corpus
        // against being emptied by a bad merge.
        assert!(BUILTIN_EXCLUDES.len() > 30);
        assert!(BUILTIN_EXCLUDES.contains(&"*.crdownload"));
        assert!(BUILTIN_EXCLUDES.contains(&"**/*.download/**"));
        assert!(BUILTIN_EXCLUDES.contains(&".keeper.*.tmp"));
    }
}
