//! The git `clean`/`smudge` filter, shared by every binary that can be one
//! (Story 34.19, DW-121).
//!
//! # Why this is shared rather than owned by the daemon
//!
//! [`crate::engine::Engine::open`] registers `filter.lfs.clean` /
//! `filter.lfs.smudge` as `std::env::current_exe()`, whichever executable that
//! is — the daemon in a CLI run, the app binary in a desktop run. The
//! implementation lived in `keeper-syncd` alone, so the claim in that comment
//! ("both understand `lfs clean|smudge`") was only half true: the app binary had
//! no such subcommand and the filter failed on **every** invocation from the
//! desktop app.
//!
//! It failed silently, which is why it stood so long. keeper also sets
//! `filter.lfs.required=false` — deliberately, so a moved binary degrades to
//! pointer files instead of hard-failing every git command in the repository —
//! and git cannot tell a filter that failed from one that was never configured.
//! In both cases it stores the bytes it was handed.
//!
//! The consequence is not cosmetic. A racily-clean LFS entry (one whose mtime is
//! not older than the index) is re-read by git as a *content* comparison, which
//! runs this filter. With a working one the worktree bytes are cleaned back into
//! the pointer the index already holds and the path reads clean; without one the
//! raw gigabytes are hashed and every LFS-tracked file reads as modified. The two
//! platforms keeper ships to do not even agree on the failure: the same fixture
//! reports MODIFIED on Linux/ext4 and CLEAN on macOS/APFS.
//!
//! So the logic lives here, once, and both binaries call it. AD-52 makes the
//! engine shared verbatim between the app and the daemon; a filter with two
//! implementations would be the same mistake one layer down.
//!
//! # Contract
//!
//! git hands the filter one file on stdin and reads the replacement from stdout.
//! Neither direction may write anything else to stdout — not a log line, not a
//! warning — because every byte is content.

use std::io::{Read, Write};
use std::path::Path;

use crate::error::{Result, SyncError};
use crate::lfs::pointer::{Pointer, MAX_POINTER_BYTES};
use crate::lfs::store::LfsStore;

/// Which direction git is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// stdin = worktree bytes, stdout = pointer. Staging a file.
    Clean,
    /// stdin = pointer, stdout = worktree bytes. Checking a file out.
    Smudge,
}

/// Run one filter invocation against the repository at `repo`.
///
/// `repo` is the **working tree** root, as `%f`'s sibling `--repo` argument
/// supplies it; the object store is `<repo>/.git/lfs`.
///
/// Generic over the streams rather than locking stdio itself, which is what makes
/// it testable: a filter that could only be exercised by spawning a process and
/// feeding it a pipe is a filter nobody tests, and this one went untested and
/// unimplemented for exactly that shape of reason.
///
/// Blocking and streaming. Nothing here sizes a buffer from the file — a clean
/// hashes straight into the store as the bytes arrive (NFR-23), which is the
/// whole reason LFS exists in this engine.
pub fn run(
    repo: &Path,
    direction: Direction,
    input: &mut impl Read,
    output: &mut impl Write,
) -> Result<()> {
    let store = LfsStore::in_git_dir(repo.join(".git"));
    store.ensure_layout()?;
    match direction {
        Direction::Smudge => smudge(&store, repo, input, output),
        Direction::Clean => clean(&store, repo, input, output),
    }
}

/// Pointer in, content out.
fn smudge(
    store: &LfsStore,
    repo: &Path,
    input: &mut impl Read,
    output: &mut impl Write,
) -> Result<()> {
    // A pointer is under 1 KiB by specification, so reading it whole is bounded.
    // One byte over is proof it is not a pointer, and it passes straight through.
    let mut buffer = Vec::with_capacity(MAX_POINTER_BYTES);
    // `by_ref` so the reader stays usable for the pass-through branch below,
    // which has to stream whatever remains after this prefix.
    Read::by_ref(input)
        .take(MAX_POINTER_BYTES as u64 + 1)
        .read_to_end(&mut buffer)
        .map_err(|err| SyncError::io("read smudge input", repo, err))?;

    let resolved = Pointer::parse(&buffer)
        .filter(|pointer| store.contains(&pointer.oid, pointer.size))
        .map(|pointer| store.object_path(&pointer.oid));

    match resolved {
        Some(path) => {
            let mut object = std::fs::File::open(&path)
                .map_err(|err| SyncError::io("open lfs object", &path, err))?;
            std::io::copy(&mut object, output)
                .map_err(|err| SyncError::io("stream lfs object", &path, err))?;
        }
        // Not a pointer, or an object we do not hold yet. Passing the bytes
        // through unchanged is what git-lfs does, and it keeps a partial fetch
        // usable instead of failing the checkout.
        None => {
            output
                .write_all(&buffer)
                .map_err(|err| SyncError::io("pass through pointer", repo, err))?;
            std::io::copy(input, output)
                .map_err(|err| SyncError::io("pass through remainder", repo, err))?;
        }
    }
    output
        .flush()
        .map_err(|err| SyncError::io("flush smudge output", repo, err))
}

/// Content in, pointer out.
///
/// # An input that is already a pointer is re-emitted, not hashed
///
/// Pointer text in the worktree is not a corner case: it is a state this very
/// module produces. [`smudge`]'s `None` arm writes the pointer back whenever the
/// object is not in the store, and under `LfsMode::PointerOnly` every LFS path
/// holds pointer text permanently. So `git add`, `git commit -a`, `git stash`
/// and a racily-clean re-read all hand those ~130 bytes to this function, and
/// hashing them emits `Pointer::new(hash(P), len(P))` — a pointer naming a
/// pointer.
///
/// That fails in two stages. First the path reads as MODIFIED, because the
/// emitted pointer is not the one the index holds — enough for
/// `git merge -X theirs` or `--ff-only` to refuse with "local changes would be
/// overwritten". Then, if the index does take the clean, the commit replaces the
/// only reference every peer has to the real object with a reference to 130
/// bytes of text, and the object's oid is no longer named anywhere in the tree.
///
/// Story 34.19 makes the desktop app binary a filter, which is exactly where a
/// human runs plain `git` by hand, so the blast radius is wider here than it was
/// when only the daemon could be one. Upstream git-lfs guards the same case —
/// `CleanPointerError`, whose handler re-emits the original bytes.
///
/// Re-emitting rather than re-encoding is deliberate. [`Pointer::parse`] accepts
/// non-canonical spellings (a legacy version URL, unsorted keys), and rendering
/// one of those afresh would change the blob hash and make the path read
/// modified for a second, subtler reason.
fn clean(
    store: &LfsStore,
    repo: &Path,
    input: &mut impl Read,
    output: &mut impl Write,
) -> Result<()> {
    // The bounded-prefix trick [`smudge`] uses, for the same reason: a pointer is
    // under 1 KiB by specification, so one byte past the ceiling is proof this is
    // content. `by_ref` so the reader stays usable for the streaming branch.
    let mut head = Vec::with_capacity(MAX_POINTER_BYTES);
    Read::by_ref(input)
        .take(MAX_POINTER_BYTES as u64 + 1)
        .read_to_end(&mut head)
        .map_err(|err| SyncError::io("read clean input", repo, err))?;

    // A short read is EOF, so a `head` inside the ceiling IS the whole input —
    // which is what makes "this file is a pointer" answerable after one bounded
    // read. The emptiness guard is not redundant: `Pointer::parse` reads no bytes
    // as the *empty pointer*, and an empty file must keep taking the path below,
    // where it stores the empty object and `render` emits nothing for it.
    if !head.is_empty() && head.len() <= MAX_POINTER_BYTES && Pointer::parse(&head).is_some() {
        output
            .write_all(&head)
            .map_err(|err| SyncError::io("re-emit pointer", repo, err))?;
        return output
            .flush()
            .map_err(|err| SyncError::io("flush clean output", repo, err));
    }

    // Hashed straight into the object store as it streams, so the object is
    // already present by the time the pointer naming it is emitted. A crash
    // between the two costs a re-clean, never a pointer to nothing. `head` is
    // chained back on rather than re-read: stdin does not rewind, and those first
    // bytes are as much of the object as any other.
    let (oid, size) = store.insert_streaming(head.as_slice().chain(input))?;
    let pointer = Pointer::new(oid, size);
    output
        .write_all(pointer.render().as_bytes())
        .map_err(|err| SyncError::io("write pointer", repo, err))?;
    output
        .flush()
        .map_err(|err| SyncError::io("flush clean output", repo, err))
}

/// Parse the filter invocation out of a raw argument list, if it is one.
///
/// Returns `None` for any argv that is not a filter call, which is the answer a
/// normal application launch needs: a GUI binary must not grow a CLI that
/// swallows the arguments macOS passes it. Finder launches pass none, and older
/// macOS passes `-psn_0_12345`; neither begins with `lfs`.
///
/// Deliberately hand-rolled rather than a `clap` surface. The app binary would
/// otherwise gain a whole argument parser — with its own `--help`, its own error
/// text on stdout, and its own opinions about unknown flags — to serve one
/// invocation whose exact shape [`crate::git::repo`] writes itself.
///
/// The accepted shape is what that writer produces:
///
/// ```text
/// <exe> lfs clean  --repo <dir> [<path>]
/// <exe> lfs smudge --repo <dir> [<path>]
/// ```
///
/// `<path>` is git's `%f`, advisory only: the object is addressed by digest, and
/// the path is not consulted for anything. It is accepted so the registered
/// command line can keep carrying it, because it is what makes a failing filter
/// legible in a `GIT_TRACE` log.
pub fn parse_args<I, S>(args: I) -> Option<(Direction, std::path::PathBuf)>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter().map(|arg| arg.as_ref().to_owned());
    if args.next().as_deref() != Some("lfs") {
        return None;
    }
    let direction = match args.next().as_deref() {
        Some("clean") => Direction::Clean,
        Some("smudge") => Direction::Smudge,
        _ => return None,
    };
    // `--repo` is required and may be followed by the advisory path in either
    // order, so the list is scanned rather than read positionally.
    let mut repo = None;
    let mut rest = args.collect::<Vec<_>>().into_iter().peekable();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--repo" => repo = rest.next(),
            other if other.starts_with("--repo=") => {
                repo = Some(other.trim_start_matches("--repo=").to_owned());
            }
            // The advisory path, or an argument a future writer added. Neither
            // is a reason to refuse to filter.
            _ => {}
        }
    }
    repo.filter(|value| !value.is_empty())
        .map(|value| (direction, std::path::PathBuf::from(value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many objects the store holds, for a test that has to prove nothing
    /// new was written.
    fn stored_objects(store: &LfsStore) -> usize {
        fn walk(dir: &Path) -> usize {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return 0;
            };
            entries
                .flatten()
                .map(|entry| {
                    let path = entry.path();
                    if path.is_dir() {
                        walk(&path)
                    } else {
                        1
                    }
                })
                .sum()
        }
        walk(&store.root().join("objects"))
    }

    /// The exact command line `git::repo::enforce_local_config_with_filter`
    /// writes, minus the program. Getting this wrong means the filter silently
    /// does nothing, which is the defect this module exists to end.
    #[test]
    fn the_registered_command_line_is_recognised() {
        let (direction, repo) =
            parse_args(["lfs", "clean", "--repo", "/w/folder", "notes/a.bin"]).expect("a clean");
        assert_eq!(direction, Direction::Clean);
        assert_eq!(repo, Path::new("/w/folder"));

        let (direction, repo) =
            parse_args(["lfs", "smudge", "--repo", "/w/folder", "notes/a.bin"]).expect("a smudge");
        assert_eq!(direction, Direction::Smudge);
        assert_eq!(repo, Path::new("/w/folder"));

        // git may hand `%f` over with no path when it has none to give, and a
        // `--repo=` spelling is the same request.
        assert!(parse_args(["lfs", "clean", "--repo", "/w"]).is_some());
        assert_eq!(
            parse_args(["lfs", "clean", "--repo=/w"])
                .expect("joined form")
                .1,
            Path::new("/w")
        );
    }

    /// A GUI binary must not grow a CLI that eats what the OS passes it.
    #[test]
    fn an_ordinary_launch_is_not_a_filter_invocation() {
        for argv in [
            vec![],
            // What Finder and older macOS actually pass.
            vec!["-psn_0_774République"],
            vec!["--flag"],
            // The right verb, no direction.
            vec!["lfs"],
            vec!["lfs", "explode", "--repo", "/w"],
            // A direction with no repository is not actionable: the object store
            // location is the one thing the filter cannot guess.
            vec!["lfs", "clean"],
            vec!["lfs", "clean", "--repo", ""],
            // Not the first argument.
            vec!["serve", "lfs", "clean", "--repo", "/w"],
        ] {
            assert!(parse_args(argv.clone()).is_none(), "{argv:?}");
        }
    }

    #[test]
    fn a_clean_stores_the_object_and_emits_the_pointer_naming_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        // Several chunks' worth, so this exercises the streaming loop rather
        // than one read.
        let payload = vec![9u8; 300_000];

        let mut out = Vec::new();
        run(repo, Direction::Clean, &mut payload.as_slice(), &mut out).expect("clean");

        let pointer = Pointer::parse(&out).expect("the output is a pointer");
        assert_eq!(pointer.size, payload.len() as u64);
        let store = LfsStore::in_git_dir(repo.join(".git"));
        assert!(
            store.contains(&pointer.oid, pointer.size),
            "the object is in the store before the pointer naming it is emitted"
        );
        assert_eq!(
            std::fs::read(store.object_path(&pointer.oid)).expect("read back"),
            payload
        );
    }

    /// The one that costs an object rather than a round trip.
    ///
    /// A `clean` whose input is already a pointer must re-emit it. Pointer text
    /// in the worktree is a state this module itself produces — `smudge` leaves
    /// it for every object the store does not hold, and `LfsMode::PointerOnly`
    /// leaves it there permanently — so `git add`, `git commit -a` and a
    /// racily-clean re-read all reach here with it. Hashing it emits a pointer
    /// naming the pointer: the path reads MODIFIED, and a commit that takes the
    /// clean replaces every peer's only reference to the real object with 130
    /// bytes of text.
    #[test]
    fn a_clean_of_a_pointer_re_emits_it_rather_than_naming_the_pointer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        let store = LfsStore::in_git_dir(repo.join(".git"));

        // The fixture is `smudge`'s OWN output for an object the store does not
        // hold, rather than a hand-written pointer: these are the exact bytes the
        // filter leaves in the worktree, which is what makes the round trip the
        // thing under test.
        let published = Pointer::new("b".repeat(64), 4_096).render().into_bytes();
        let mut worktree = Vec::new();
        run(
            repo,
            Direction::Smudge,
            &mut published.as_slice(),
            &mut worktree,
        )
        .expect("smudge an object the store does not hold");
        assert_eq!(
            worktree, published,
            "which is where the pointer text comes from"
        );

        let before = stored_objects(&store);
        let mut out = Vec::new();
        run(repo, Direction::Clean, &mut worktree.as_slice(), &mut out).expect("clean");

        assert_eq!(
            out, published,
            "byte-for-byte, so the index entry is unchanged and the path reads \
             clean; anything else is a phantom modification at best and the loss \
             of the object at worst"
        );
        assert_eq!(
            stored_objects(&store),
            before,
            "and nothing was stored — hashing pointer text creates an object \
             nobody will ever ask for"
        );
    }

    /// The other half of the prefix read: content that merely *starts* like a
    /// pointer must keep every byte.
    #[test]
    fn a_clean_keeps_every_byte_of_content_that_only_begins_like_a_pointer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        let store = LfsStore::in_git_dir(repo.join(".git"));

        // A real pointer's bytes and then a great deal more. The prefix read stops
        // one byte past the pointer ceiling, so a version that forgot to chain
        // `head` back on would store the tail alone — and silently, because the
        // pointer it emitted would describe those bytes perfectly.
        let mut long = Pointer::new("c".repeat(64), 1).render().into_bytes();
        long.resize(long.len() + MAX_POINTER_BYTES * 3, b'z');

        // And the boundary: one byte more than a pointer is content, and this one
        // fits inside `head`, so nothing is chained on at all.
        let mut barely = Pointer::new("d".repeat(64), 1).render().into_bytes();
        barely.push(b'!');

        for payload in [long, barely] {
            let mut out = Vec::new();
            run(repo, Direction::Clean, &mut payload.as_slice(), &mut out).expect("clean");
            let pointer = Pointer::parse(&out).expect("content, so a fresh pointer");
            assert_eq!(
                pointer.size,
                payload.len() as u64,
                "the pointer must account for all {} bytes",
                payload.len()
            );
            assert_eq!(
                std::fs::read(store.object_path(&pointer.oid)).expect("read back"),
                payload,
                "and the store must hold all of them"
            );
        }
    }

    #[test]
    fn a_smudge_returns_the_content_the_pointer_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        let payload = vec![4u8; 200_000];

        // Clean first, so the store holds the object a smudge has to find.
        let mut pointer_bytes = Vec::new();
        run(
            repo,
            Direction::Clean,
            &mut payload.as_slice(),
            &mut pointer_bytes,
        )
        .expect("clean");

        let mut restored = Vec::new();
        run(
            repo,
            Direction::Smudge,
            &mut pointer_bytes.as_slice(),
            &mut restored,
        )
        .expect("smudge");
        assert_eq!(restored, payload, "the round trip is byte-exact");
    }

    /// The tolerance that keeps a partial fetch usable. git-lfs does the same:
    /// a checkout must not fail because an object has not been downloaded yet.
    #[test]
    fn a_smudge_passes_through_what_it_cannot_resolve() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();

        // Ordinary content, which is what a smudge sees for any non-LFS path
        // that happens to be routed through the filter.
        let plain = b"this is not a pointer, it is a note\n";
        let mut out = Vec::new();
        run(repo, Direction::Smudge, &mut &plain[..], &mut out).expect("smudge");
        assert_eq!(out, plain);

        // A well-formed pointer for an object the store does not hold. Passing
        // it through leaves the pointer text in the worktree, which is
        // recoverable; failing here would break the whole checkout.
        let orphan = Pointer::new("b".repeat(64), 4_096).render().into_bytes();
        let mut out = Vec::new();
        run(repo, Direction::Smudge, &mut orphan.as_slice(), &mut out).expect("smudge");
        assert_eq!(out, orphan);
    }

    /// A file larger than the pointer ceiling cannot be a pointer, and the
    /// prefix read must not swallow the rest of it.
    #[test]
    fn a_smudge_of_something_far_too_large_streams_all_of_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let big = vec![7u8; MAX_POINTER_BYTES * 3];
        let mut out = Vec::new();
        run(dir.path(), Direction::Smudge, &mut big.as_slice(), &mut out).expect("smudge");
        assert_eq!(
            out.len(),
            big.len(),
            "the bounded prefix read must not truncate the pass-through"
        );
        assert_eq!(out, big);
    }
}
