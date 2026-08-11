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
    "*.crdownload", // Chrome, Chromium, Edge
    "*.part",       // Firefox, wget --continue
    "*.filepart",   // Firefox (legacy), Nextcloud desktop client
    // `.partial` is also keeper's OWN in-progress marker (Story 41.3, AD-69):
    // `keeper-rec` writes each segment as `<name>.<ext>.partial` and renames
    // it onto its final name the instant `finishWriting` returns. A SUFFIX
    // rule, deliberately, and not a glob over the recordings directory: git
    // sees that rename as an add plus a delete, so nothing about the
    // *directory* can hide the growing file — the name is the only thing that
    // is true for the whole of its life, and it has to hold wherever the
    // recording destination resolves, at whatever depth, in whichever profile.
    // It is also all this crate ever learns about recording: one string, no
    // notion of segments, rotation or ledgers.
    //
    // Deliberate consequence: an unrelated `x.partial` a user keeps in a
    // synced folder is now excluded like any other tier-0 name — invisible to
    // `Engine::pending`, never staged, never in the activity feed. That is a
    // behaviour change for that one file, and it is the price of a rule that
    // is total: nothing on disk distinguishes it from a segment mid-write.
    "*.partial",        // rclone, legacy IE/Edge, and keeper-rec's in-progress segment
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
    //
    // With ONE carve-out: `*.toml` directly under it is the folder's own
    // configuration and must reach the other machine (AD-100). That cannot be
    // written as a pattern — globset has no negation, so a `!…` entry compiles
    // as a literal glob beginning with a bang — so it lives in
    // [`is_exempt_config_file`], consulted before this set is.
    ".keeper",
    "**/.keeper/**",
];

/// The one path shape [`BUILTIN_EXCLUDES`] names and tier 0 still lets through:
/// a folder's own configuration file (AD-100).
///
/// `<anything>/.keeper/<name>.toml`, and nothing else. Config that does not
/// sync defeats the reason for putting it in the sync folder rather than in
/// `~/.keeper/` — including `keeper.<host>.toml`, which syncs deliberately so
/// one machine's settings can be edited from another.
///
/// Narrow on three axes, each of which is a way this could leak the cache it
/// sits beside:
///
/// * **Depth.** Only a direct child. `.keeper/sub/x.toml` stays excluded —
///   nothing keeper writes puts config a level down, so a `.toml` down there is
///   something else's, and the trash tree is exactly the "something else" that
///   would start syncing.
/// * **Suffix.** Only `.toml`. `.keeper/index.json` is the cache this rule
///   exists to keep excluded.
/// * **Shape.** Files only. A *directory* named `x.toml` under `.keeper/` is
///   still engine state, so this is asked only where the answer is known to be
///   about a file — see [`ExcludeSet::is_excluded_directory`].
///
/// Takes the already-normalized match string rather than a `Path` so it reads
/// the same `/`-joined frame the globs do, on every platform.
fn is_exempt_config_file(candidate: &str) -> bool {
    let Some((parent, name)) = candidate.rsplit_once('/') else {
        // A bare `keeper.toml` at the profile root is an ordinary file the
        // corpus never matched; it needs no exemption and gets none.
        return false;
    };
    (parent == ".keeper" || parent.ends_with("/.keeper")) && name.ends_with(".toml")
}

/// The compiled tier-0 filter for one profile.
///
/// Compiling the corpus builds a regex set, so build it **once per profile**
/// and hold it for the lifetime of the supervisor. Recompiling per scan would
/// dominate the cost of the scan itself on a 100 000-file tree.
#[derive(Debug, Clone)]
pub struct ExcludeSet {
    set: GlobSet,
    /// The same corpus, read as a question about directories: every pattern
    /// that excludes a *subtree* (`.git/**`, `**/node_modules/**`) contributes
    /// the directory it is rooted at (`.git`, `**/node_modules`). Derived from
    /// the one list in [`ExcludeSet::new`] — including the profile's own
    /// patterns — so there is still exactly one place a path can be named.
    dir_set: GlobSet,
    /// The profile's own `extra` patterns, and only those, compiled a second
    /// time. Not a second rule: the same strings, asked a narrower question —
    /// *did the user write this rule, or did keeper?* See
    /// [`ExcludeSet::verdict`] for why anything needs to know.
    profile_set: GlobSet,
    /// The directory form of the profile's own patterns, derived exactly as
    /// [`Self::dir_set`] is.
    profile_dir_set: GlobSet,
}

/// How a *browser* should treat a path the exclusion corpus matches (Story
/// 44.17, FR-173).
///
/// The sync path only ever needs a boolean, and [`ExcludeSet::is_excluded`]
/// stays that boolean for every caller that stages, queues or counts. A
/// browser needs one distinction on top of it, and it needs it because of a
/// specific failure: the Files tab used to drop every excluded entry silently,
/// so a user who had written `*.psd` into their own profile saw their
/// photoshop files simply *absent* from the folder they were looking at, with
/// nothing on screen connecting that to the rule they typed. Marking them
/// excluded is what turns a hole in a listing into an answer.
///
/// The built-in corpus gets the opposite treatment for the opposite reason.
/// Nobody chose `.DS_Store`, `.git/**` or `*.crdownload`; showing them marked
/// would fill every folder with rows about keeper's own housekeeping, which is
/// the browser nobody scrolls twice that Story 43.8 exists to avoid.
///
/// A path both a profile pattern and the corpus match reads as
/// [`Self::ProfilePattern`]. The user's own rule wins the naming, because a
/// rule someone typed is a rule they are entitled to see working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcludeVerdict {
    /// No pattern matches; the entry is ordinary content.
    Included,
    /// A pattern from this profile's own configuration matches. Show it, and
    /// say why it will never sync.
    ProfilePattern,
    /// Only [`BUILTIN_EXCLUDES`] matches. Keeper's own noise; do not list it.
    BuiltinNoise,
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
        let mut dir_builder = GlobSetBuilder::new();
        let mut profile_builder = GlobSetBuilder::new();
        let mut profile_dir_builder = GlobSetBuilder::new();
        let corpus = BUILTIN_EXCLUDES
            .iter()
            .copied()
            .map(|pattern| (pattern, false))
            .chain(extra.iter().map(|pattern| (pattern.as_str(), true)));
        for (pattern, from_profile) in corpus {
            add_pattern(&mut builder, pattern)?;
            // A `…/**` pattern is already anchored by its own `/`, so its
            // directory form goes in verbatim rather than back through the
            // basename rule.
            let directory = pattern.trim().strip_suffix("/**");
            if let Some(directory) = directory {
                add_glob(&mut dir_builder, directory, pattern)?;
            }
            if from_profile {
                add_pattern(&mut profile_builder, pattern)?;
                if let Some(directory) = directory {
                    add_glob(&mut profile_dir_builder, directory, pattern)?;
                }
            }
        }
        let build = |builder: GlobSetBuilder| {
            builder
                .build()
                .map_err(|err| SyncError::Config(format!("could not compile exclude set: {err}")))
        };
        Ok(Self {
            set: build(builder)?,
            dir_set: build(dir_builder)?,
            profile_set: build(profile_builder)?,
            profile_dir_set: build(profile_dir_builder)?,
        })
    }

    /// Whether `relative_path` — repository-relative, never absolute — is
    /// filtered out entirely.
    ///
    /// The **file** question, which is the only one the scan, the stager and
    /// the watcher ever ask. [`is_exempt_config_file`] is answered here, before
    /// the set is consulted, and this is the single place it is answered: the
    /// directory question deliberately does not delegate to it, and
    /// [`Self::verdict`] routes through this method rather than matching the
    /// set itself, so the carve-out cannot apply in one of the three and not
    /// the others.
    pub fn is_excluded(&self, relative_path: &Path) -> bool {
        let candidate = match_string(relative_path);
        if candidate.is_empty() {
            // The repository root itself is never excluded; matching an empty
            // string against `**/…` patterns would be undefined-ish anyway.
            return false;
        }
        if is_exempt_config_file(candidate.as_str()) {
            return false;
        }
        self.set.is_match(candidate.as_str())
    }

    /// Whether `relative_path`, known to be a **directory**, is one the corpus
    /// excludes wholesale.
    ///
    /// A second question against the same rules, not a second rule set. The
    /// scan path never needed it: the tree walk asks about files, and a file
    /// under `.git/` is caught by `.git/**` on its own. A *browser* needs it,
    /// because a directory whose every descendant is excluded is a folder that
    /// can only ever open onto nothing — and `.git/` and `.keeper-sync/`, which
    /// the corpus calls engine state and never user content, are exactly that.
    /// Showing them would be the "browser nobody scrolls twice" this exists to
    /// prevent.
    ///
    /// Directory-only on purpose. Applying the subtree rules to a *file* would
    /// hide a plain file named `.git`, which the corpus deliberately does not
    /// exclude — and, in the other direction, a directory named `x.toml` under
    /// `.keeper/` is engine state like everything else in there, so this asks
    /// the set directly rather than through [`Self::is_excluded`], whose
    /// AD-100 carve-out is for files.
    pub fn is_excluded_directory(&self, relative_path: &Path) -> bool {
        let candidate = match_string(relative_path);
        !candidate.is_empty()
            && (self.set.is_match(candidate.as_str()) || self.dir_set.is_match(candidate.as_str()))
    }

    /// Which kind of exclusion, if any, applies to one browsed entry.
    ///
    /// `is_dir` selects the same question [`Self::is_excluded_directory`] asks,
    /// so a browser gets one call rather than a branch it could get backwards.
    /// The two halves stay in lockstep by construction: the profile sets are
    /// built from the same strings, in the same loop, by the same two helpers,
    /// and the excluded-at-all question is the very method each half exposes.
    ///
    /// A profile pattern is only ever *named* here, never applied: every
    /// profile pattern is in `set` too, so nothing this returns
    /// [`ExcludeVerdict::ProfilePattern`] for is anything the boolean questions
    /// call included. That is what keeps a user's own `*.toml` rule from
    /// marking a file the sync path is meanwhile committing (AD-100).
    pub fn verdict(&self, relative_path: &Path, is_dir: bool) -> ExcludeVerdict {
        let excluded = if is_dir {
            self.is_excluded_directory(relative_path)
        } else {
            self.is_excluded(relative_path)
        };
        if !excluded {
            return ExcludeVerdict::Included;
        }
        let candidate = match_string(relative_path);
        let profile = self.profile_set.is_match(candidate.as_str())
            || (is_dir && self.profile_dir_set.is_match(candidate.as_str()));
        if profile {
            ExcludeVerdict::ProfilePattern
        } else {
            ExcludeVerdict::BuiltinNoise
        }
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
    add_glob(builder, &effective, pattern)
}

/// Compile and add one already-anchored pattern.
///
/// Split out from [`add_pattern`] because the directory question derives its
/// patterns by stripping `/**`, and a stripped pattern must keep the anchoring
/// the original had. Re-running `scratch/**`'s remainder through the basename
/// rule would turn a profile's root-anchored pattern into "any `scratch` at any
/// depth" — a rule the user never wrote, silently hiding their folders.
/// `original` is named in the error so a user reads back the string they typed.
fn add_glob(builder: &mut GlobSetBuilder, effective: &str, original: &str) -> Result<()> {
    let glob = GlobBuilder::new(effective)
        .literal_separator(true)
        .build()
        .map_err(|err| SyncError::Config(format!("invalid exclude pattern {original:?}: {err}")))?;
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

    /// AD-100: a folder's own `.keeper/*.toml` is the one thing in that
    /// directory that has to reach the other machine. Config that does not sync
    /// defeats the reason for putting it in the sync folder rather than in
    /// `~/.keeper/`.
    #[test]
    fn a_folders_own_config_file_is_exempt_from_the_cache_exclusion() {
        let set = default_set();
        for path in [
            ".keeper/keeper.toml",
            ".keeper/keeper.mnemosyne.toml",
            "sub/.keeper/keeper.toml",
            "sub/deeper/.keeper/keeper.hesperia.toml",
        ] {
            assert!(
                !excluded(&set, path),
                "{path} is the folder's own configuration and must sync"
            );
        }
    }

    /// The machine-variant file syncs exactly like the shared one, and that is
    /// the point of it: it is how one machine's settings are edited from
    /// another. A future reader tempted to exclude it has this test to argue
    /// with.
    #[test]
    fn the_machine_variant_config_file_syncs_like_the_shared_one() {
        let set = default_set();
        assert_eq!(
            excluded(&set, ".keeper/keeper.toml"),
            excluded(&set, ".keeper/keeper.hesperia.toml"),
            "the per-machine file is not more local than the shared one"
        );
        assert!(!excluded(&set, ".keeper/keeper.hesperia.toml"));
    }

    /// The three ways the carve-out could leak the cache it sits beside. Each
    /// row is a path a reading of the rule as "anything `.toml`-ish under
    /// `.keeper/`" would let through.
    #[test]
    fn the_config_carve_out_leaks_neither_depth_suffix_nor_shape() {
        let set = default_set();
        // Deeper than one level: the trash tree lives down there, and a note
        // called `notes.toml` in it must not start syncing out of the grave.
        for path in [
            ".keeper/sub/x.toml",
            "sub/.keeper/trash/01JABCDEF/draft.toml",
        ] {
            assert!(
                excluded(&set, path),
                "{path} is deeper than a direct child and is not config"
            );
        }
        // A non-`.toml` directly under it: this is the cache the whole rule
        // exists to keep excluded.
        for path in [".keeper/index.json", "sub/.keeper/index.json"] {
            assert!(excluded(&set, path), "{path} is the cache, not config");
        }
        // A DIRECTORY named `x.toml`: the carve-out is answered on the file
        // question only, so the directory question still hides it — and so does
        // everything inside it.
        assert!(
            set.is_excluded_directory(Path::new(".keeper/x.toml")),
            "a directory named x.toml under .keeper/ is still engine state"
        );
        assert!(
            excluded(&set, ".keeper/x.toml/index.json"),
            "nothing inside a directory named x.toml is exempt either"
        );
        // And the directory itself stays hidden, which is what makes the
        // exemption an exemption rather than a relaxation.
        assert!(set.is_excluded_directory(Path::new(".keeper")));
        assert!(set.is_excluded_directory(Path::new("sub/.keeper")));
    }

    /// The carve-out is the corpus's, and it is absolute: a profile pattern is
    /// only ever *named* by [`ExcludeSet::verdict`], never applied, so a user
    /// who writes `*.toml` in their own excludes still gets their folder's
    /// configuration synced rather than a file the browser marks excluded while
    /// the engine commits it.
    #[test]
    fn a_profile_pattern_cannot_take_the_config_file_back() {
        let set = ExcludeSet::new(&["*.toml".to_owned()]).expect("compiles");
        assert!(!excluded(&set, ".keeper/keeper.toml"));
        assert_eq!(
            set.verdict(Path::new(".keeper/keeper.toml"), false),
            ExcludeVerdict::Included
        );
        // The same rule over an ordinary file is untouched: it is the user's
        // rule, so it applies and is named.
        assert!(excluded(&set, "notes/pyproject.toml"));
        assert_eq!(
            set.verdict(Path::new("notes/pyproject.toml"), false),
            ExcludeVerdict::ProfilePattern
        );
    }

    /// The subtree rules read as a directory question (Story 43.8). `.git` and
    /// `.keeper-sync` are named in the corpus only as `…/**`, so the file
    /// question rightly says nothing about the directories themselves — and a
    /// browser that asked only the file question would put both of them on
    /// screen at the root of every synced folder.
    #[test]
    fn a_directory_whose_whole_subtree_is_excluded_is_excluded_as_a_directory() {
        let set = default_set();
        for path in [
            ".git",
            ".keeper-sync",
            "sub/.git",
            "node_modules",
            ".keeper",
        ] {
            assert!(
                set.is_excluded_directory(Path::new(path)),
                "{path} opens onto nothing a browser may show"
            );
            // …and the file question is deliberately unchanged, which is the
            // whole reason the directory question had to be added.
            if path == ".git" || path == ".keeper-sync" || path == "sub/.git" {
                assert!(
                    !excluded(&set, path),
                    "{path} must stay visible to the file question"
                );
            }
        }
    }

    #[test]
    fn an_ordinary_directory_is_not_excluded_by_the_directory_question() {
        let set = default_set();
        for path in ["Notes", "2026/Standup", "target", "build", "git"] {
            assert!(
                !set.is_excluded_directory(Path::new(path)),
                "{path} is the user's folder"
            );
        }
    }

    /// A profile's own `foo/**` becomes a directory rule too, so the pair stays
    /// derived from one list rather than from a second hard-coded one.
    #[test]
    fn a_profile_subtree_pattern_also_answers_the_directory_question() {
        let set = ExcludeSet::new(&["scratch/**".to_owned()]).expect("compiles");
        assert!(set.is_excluded_directory(Path::new("scratch")));
        assert!(!set.is_excluded_directory(Path::new("sub/scratch")));
    }

    /// A segment being written is `<name>.<ext>.partial`, and the recordings
    /// root can sit anywhere inside a profile — so the suffix has to hold at
    /// the profile root and at every depth below it, whatever the media
    /// extension underneath. This is the whole of Story 41.3's sync side: if
    /// it fails, a growing multi-gigabyte segment is staged mid-write.
    #[test]
    fn an_in_progress_recording_segment_is_excluded_at_any_depth() {
        let set = default_set();
        for path in [
            "screen-0003.mov.partial",
            "recordings/screen-0003.mov.partial",
            "recordings/keeper-rec 2026-08-07 11-02-13/screen-0003.mov.partial",
            "a/b/c/d/e/f/g/camera-0012.mov.partial",
            "recordings/session/audio-0000.m4a.partial",
        ] {
            assert!(
                excluded(&set, path),
                "{path} is a segment mid-write; staging it commits a torn file forever"
            );
        }
        // The final name — the only thing `SegmentClosed` ever carries — is an
        // ordinary file the moment the rename lands. Excluding it too would
        // mean recordings never sync at all.
        for path in [
            "recordings/session/screen-0003.mov",
            "recordings/session/camera-0012.mov",
            "recordings/session/audio-0000.m4a",
            "notes/partial.md",   // the suffix anchors at the end...
            "notes/partial/x.md", // ...of the basename, not of a directory
        ] {
            assert!(!excluded(&set, path), "{path} must stay visible");
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
        // Story 41.3 depends on this one entry existing verbatim: it is the
        // only thing standing between a segment mid-write and the commit path.
        assert!(BUILTIN_EXCLUDES.contains(&"*.partial"));
    }
}
