//! The listing a human or an agent can ask for: which LFS paths this clone
//! holds, and which it only knows the name of (Story 56.2, FR-336, FR-337,
//! FR-340).
//!
//! # Why the data lives here and not in the daemon
//!
//! `keeper-syncd` may depend on `keeper-sync` and on nothing else first-party,
//! and the desktop shell reaches the same facts through the same engine. A
//! listing assembled in the CLI would be a listing the app could not show and
//! a listing no test in this crate could assert, which is the shape AD-52
//! exists to prevent. So the *data* is here — one pure function over an index
//! snapshot, a worktree and a ledger — and only its two renderings live in the
//! daemon.
//!
//! # What decides a path's state
//!
//! The worktree bytes, and nothing else. A path is [`LfsFileState::Virtual`]
//! because [`crate::lfs::stage::worktree_pointer`] says those bytes are the
//! committed pointer; it is [`LfsFileState::Materialized`] because they are
//! not; it is [`LfsFileState::Absent`] because there are none. Deliberately
//! **not** derived from `VirtualPolicy`: a policy says which paths *may* stay
//! unmaterialized, which is a different question from what is on the disk right
//! now, and answering the second with the first is how a listing starts
//! disagreeing with `git status`.
//!
//! The `materialized` ledger is joined in rather than consulted for the state,
//! for the same reason. A row in it means "content for this path landed here at
//! least once" — a true and useful fact, reported as
//! [`LfsFile::materialized_at_ms`], and not evidence about the present.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db::MaterializedRow;
use crate::lfs::pointer::Pointer;
use crate::lfs::stage;

/// Whether this machine holds the content behind one LFS path.
///
/// Three answers rather than a boolean, because "there is no file here" is not
/// the same fact as "the file here is a pointer" and a listing that folded them
/// together would report a deleted recording as one waiting on the remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LfsFileState {
    /// The worktree file *is* the committed pointer, so this machine is not
    /// holding the content it names.
    ///
    /// Deliberately a statement about **here**. Where the bytes are instead is
    /// a different question, and this row has no evidence for it: a pointer
    /// whose object never reached the server is a valid blob and a clean
    /// `git status`, which is the loss `verify --remote` exists to find. Ask
    /// `--remote` for that answer; it is absent from a plain listing precisely
    /// because it cannot be had for free.
    Virtual,
    /// The worktree file holds content — whatever it is, it is not pointer
    /// text. The honest reading of "this machine has bytes here", which is the
    /// question, and not "these bytes match the pointer's digest", which is
    /// `Engine::verify`'s job and costs a full read per file.
    Materialized,
    /// The index records a pointer for this path and the worktree is not
    /// holding a file at it. An uncommitted deletion, a sparse cone that
    /// excludes it, a checkout that never finished — and also a directory or a
    /// symlink standing where the content should be, because none of those is
    /// content this machine holds either. All of them are "keeper is not
    /// holding this", which is what the row is for.
    Absent,
}

impl std::fmt::Display for LfsFileState {
    /// The one word a human rendering shows, and **deliberately the same
    /// string serde emits** for the same variant.
    ///
    /// Two spellings of these three words is exactly how a CLI's prose form and
    /// its `--json` form come to disagree about a row, so they are one match
    /// here and `the_word_and_the_wire_agree` pins them together. `f.pad`
    /// rather than `write_str` so a caller's `{:<12}` column alignment works.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::Virtual => "virtual",
            Self::Materialized => "materialized",
            Self::Absent => "absent",
        })
    }
}

/// One LFS path, as a listing reports it.
///
/// # The size and the oid come from the pointer, always
///
/// Not from the worktree's `stat` (FR-336). For a virtual path the `stat` is
/// about 130 bytes of pointer text and the number a person asked for is the one
/// written inside it; for a materialized path the two agree, and taking the
/// index's answer in both cases means one rule instead of a branch that can be
/// got wrong in only one of them. A locally *modified* materialized file is the
/// one case where they differ and the index is still the right answer here: the
/// row is about the object this path names, and the uncommitted edit is what
/// `git status` reports.
///
/// # `camelCase` because this is a wire type
///
/// `keeper-syncd ls-files --json` serializes these directly and FR-337 calls
/// the result a stable form, so the field names are the contract — see
/// `commands::ls_files_entry` and the test that pins the key set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsFile {
    /// Repository-relative and `/`-joined, exactly as the index spells it —
    /// the frame every other path in this crate is already in.
    pub path: String,
    /// The pointer's object id, bare hex with no `sha256:` prefix: the form the
    /// batch API and the local store both want.
    pub oid: String,
    /// What the pointer says the content is, in bytes.
    pub size_bytes: u64,
    /// Whether this machine holds those bytes.
    pub state: LfsFileState,
    /// When the worktree file was last written, ms since the Unix epoch.
    /// `None` for an [`LfsFileState::Absent`] path — there is no file to ask —
    /// and for a platform that would not say.
    pub mtime_ms: Option<i64>,
    /// When content for this path last landed on this machine, from the
    /// `materialized` ledger. `None` when the ledger has never seen it, which
    /// includes every path this clone has only ever held as a pointer.
    ///
    /// Independent of [`Self::state`] on purpose: a path can be `Absent` and
    /// still carry this, because the ledger records history and the state
    /// records the present.
    pub materialized_at_ms: Option<i64>,
    /// When the content was last read through keeper. Written by every use
    /// keeper can observe — an open, a text or document read, an export, the
    /// start of a media stream — and by every arrival, since landing here is
    /// the first thing that happened to the content (Story 56.5).
    pub last_used_ms: Option<i64>,
    /// When the remote last confirmed it holds the object. Written where a
    /// per-path proof already exists: an upload unit that completed, or an
    /// object the remote audit affirmed (Story 56.5). Never at `mark_synced`,
    /// which is a per-profile edge that says nothing about one path.
    pub synced_at_ms: Option<i64>,
    /// Whether the owner has asked for this path to stay on this machine.
    /// `false` for every path with no ledger row and every row that has never
    /// said otherwise.
    pub pinned: bool,
}

/// Join an index snapshot, the worktree and the ledger into one listing.
///
/// Pure in the only sense that matters here: the three inputs are handed in, so
/// every rule below is asserted over a temp directory with no engine, no
/// database and no repository — and the one thing it does reach for, the
/// worktree file, is the fact being reported.
///
/// `pointers` is [`stage::indexed_pointers`]' answer and is therefore already
/// the whole set of LFS paths: a path with no pointer in the index is not an
/// LFS path and has no row here, however large it is. Ordered by path because
/// the map is, which makes the human rendering stable without a sort.
///
/// The ledger is joined through a map built once. A linear scan per row would
/// be quadratic against a ledger that grows with every materialization, and the
/// listing is exactly the caller that has both at their largest.
pub fn collect(
    root: &Path,
    pointers: &BTreeMap<String, Pointer>,
    ledger: &[MaterializedRow],
) -> Vec<LfsFile> {
    let held: BTreeMap<&str, &MaterializedRow> =
        ledger.iter().map(|row| (row.path.as_str(), row)).collect();

    pointers
        .iter()
        .map(|(path, pointer)| {
            let absolute = root.join(path);
            // `metadata`, not `symlink_metadata`, and that is a decision rather
            // than a default: it is the call `crate::browse::list_resolved`
            // binds, so a path the Files pane calls virtual is a path this verb
            // calls virtual. Two documented invariants that contradicted each
            // other would be worse than either — a tracked symlink whose target
            // is pointer text would read `virtual` on one surface and
            // `materialized` on the other.
            let meta = std::fs::metadata(&absolute).ok();
            let state = match &meta {
                // No file, or something that is not one: a directory or a
                // dangling symlink where content should be is not content this
                // machine is holding, and calling it `materialized` would put
                // it in the count of paths that need no download.
                None => LfsFileState::Absent,
                Some(meta) if !meta.is_file() => LfsFileState::Absent,
                Some(meta) if stage::worktree_pointer(&absolute, meta).is_some() => {
                    LfsFileState::Virtual
                }
                Some(_) => LfsFileState::Materialized,
            };
            let row = held.get(path.as_str());
            LfsFile {
                path: path.clone(),
                oid: pointer.oid.clone(),
                size_bytes: pointer.size,
                state,
                // The same conversion `BrowseEntry::mtime_ms` documents, taken
                // from there rather than respelled here.
                mtime_ms: meta.as_ref().and_then(crate::browse::mtime_ms),
                materialized_at_ms: row.map(|row| row.at_ms),
                last_used_ms: row.and_then(|row| row.last_used_ms),
                synced_at_ms: row.and_then(|row| row.synced_at_ms),
                pinned: row.is_some_and(|row| row.pinned),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(size: u64) -> Pointer {
        Pointer::new(
            "1111111111111111111111111111111111111111111111111111111111111111",
            size,
        )
    }

    fn row(path: &str, at_ms: i64) -> MaterializedRow {
        MaterializedRow {
            path: path.to_owned(),
            at_ms,
            last_used_ms: None,
            synced_at_ms: None,
            oid: None,
            size_bytes: None,
            pinned: false,
            local_origin: false,
        }
    }

    /// The three states, each produced by what is actually on disk.
    ///
    /// The virtual row is the claim of the whole story: its worktree file is
    /// the ~130-byte pointer and the size it reports is the four megabytes the
    /// pointer names. An implementation that reached for `metadata().len()`
    /// fails on that number alone.
    #[test]
    fn the_state_and_the_size_come_from_the_worktree_and_the_pointer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let virtual_pointer = pointer(4 * 1024 * 1024);
        std::fs::create_dir_all(root.join("media")).expect("zone");
        std::fs::write(root.join("media/held.mp4"), vec![7u8; 2_048]).expect("content");
        std::fs::write(root.join("media/away.mp4"), virtual_pointer.render()).expect("pointer");

        let pointers = BTreeMap::from([
            ("media/away.mp4".to_owned(), virtual_pointer.clone()),
            ("media/held.mp4".to_owned(), pointer(2_048)),
            ("media/gone.mp4".to_owned(), pointer(99)),
        ]);
        let files = collect(root, &pointers, &[]);

        let states: Vec<(&str, LfsFileState, u64)> = files
            .iter()
            .map(|file| (file.path.as_str(), file.state, file.size_bytes))
            .collect();
        assert_eq!(
            states,
            vec![
                ("media/away.mp4", LfsFileState::Virtual, 4 * 1024 * 1024),
                ("media/gone.mp4", LfsFileState::Absent, 99),
                ("media/held.mp4", LfsFileState::Materialized, 2_048),
            ],
            "the pointer's size is reported for all three, and the state is what \
             the worktree holds"
        );
        assert_eq!(
            files[0].oid, virtual_pointer.oid,
            "the oid is the pointer's, so a caller can fetch the object"
        );
        let now_ms = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("after 1970")
                .as_millis(),
        )
        .expect("fits");
        for index in [0usize, 2] {
            let mtime = files[index]
                .mtime_ms
                .expect("a file that exists has an mtime");
            assert!(
                (now_ms - 600_000..=now_ms + 600_000).contains(&mtime),
                "{}'s mtime is within ten minutes of now, in MILLISECONDS — \
                 `as_secs()` or `created()` would both fail here: {mtime} vs {now_ms}",
                files[index].path
            );
        }
        assert_eq!(
            files[1].mtime_ms, None,
            "a path with no file has no modification time to report"
        );
    }

    /// A directory or a symlink standing where content should be is not content
    /// this machine holds.
    ///
    /// `Materialized` feeds the "needs no download" half of the header count, so
    /// a catch-all that read "metadata exists" as "the bytes are here" would put
    /// a dangling symlink — a plausible hand-rolled workaround for a virtual
    /// path — in the column that says nothing needs fetching. And the metadata
    /// call is `metadata`, not `symlink_metadata`, so a symlink pointing at
    /// pointer text answers `virtual` here exactly as it does in the Files pane.
    #[test]
    fn something_that_is_not_a_file_is_not_held_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir(root.join("folder.mp4")).expect("a directory in its place");

        let p = pointer(4_000);
        let pointers = BTreeMap::from([("folder.mp4".to_owned(), p)]);
        let files = collect(root, &pointers, &[]);

        assert_eq!(
            files[0].state,
            LfsFileState::Absent,
            "a directory is not the content the pointer names"
        );
    }

    /// A path with no ledger row is not pinned and has no timestamps, and one
    /// with a row carries every column the ledger holds. The two must not be
    /// confused: `pinned == false` for an absent row is the default answer, not
    /// a recorded one, and reporting the ledger's `at_ms` as the state would
    /// call a released path materialized.
    #[test]
    fn the_ledger_is_joined_without_deciding_the_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let p = pointer(1_000_000);
        std::fs::write(root.join("kept.bin"), p.render()).expect("pointer");

        let mut pinned = row("kept.bin", 1_700);
        pinned.pinned = true;
        pinned.last_used_ms = Some(1_800);
        pinned.synced_at_ms = Some(1_900);

        let pointers = BTreeMap::from([
            ("kept.bin".to_owned(), p.clone()),
            ("unknown.bin".to_owned(), p),
        ]);
        let files = collect(root, &pointers, &[pinned]);

        assert_eq!(files[0].path, "kept.bin");
        assert_eq!(files[0].materialized_at_ms, Some(1_700));
        assert_eq!(files[0].last_used_ms, Some(1_800));
        assert_eq!(files[0].synced_at_ms, Some(1_900));
        assert!(files[0].pinned);
        assert_eq!(
            files[0].state,
            LfsFileState::Virtual,
            "the ledger records that content landed here once; the worktree \
             records that it is not here now, and the state is the worktree's"
        );

        assert_eq!(files[1].path, "unknown.bin");
        assert_eq!(files[1].materialized_at_ms, None);
        assert!(!files[1].pinned, "no row means unpinned, not unknown");
    }

    /// An empty worktree file is not a virtual path.
    ///
    /// `Pointer::parse` reads zero bytes as the empty pointer, and inheriting
    /// that carve-out here would call every empty tracked file virtual —
    /// exactly the failure `stage::pointer_blob` documents at the blob layer.
    /// `worktree_pointer` refuses it, so the row reports `materialized`, which
    /// is true: the machine holds every byte this path has.
    #[test]
    fn an_empty_file_is_held_content_and_not_a_pointer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("empty.bin"), b"").expect("empty");
        let files = collect(
            root,
            &BTreeMap::from([("empty.bin".to_owned(), pointer(0))]),
            &[],
        );
        assert_eq!(files[0].state, LfsFileState::Materialized);
    }

    /// FR-337 calls the JSON form stable, so the key set is the contract.
    /// Asserted here as well as in the daemon because this struct is where a
    /// renamed field would come from, and a serde attribute is exactly the kind
    /// of change that looks harmless in review.
    #[test]
    fn the_serialized_shape_is_camel_case_and_exactly_nine_keys() {
        let files = collect(
            tempfile::tempdir().expect("tempdir").path(),
            &BTreeMap::from([("a.bin".to_owned(), pointer(5))]),
            &[],
        );
        let value = serde_json::to_value(&files[0]).expect("serialize");
        let object = value.as_object().expect("an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "lastUsedMs",
                "materializedAtMs",
                "mtimeMs",
                "oid",
                "path",
                "pinned",
                "sizeBytes",
                "state",
                "syncedAtMs",
            ]
        );
        assert_eq!(
            object.get("state").and_then(serde_json::Value::as_str),
            Some("absent"),
            "the state crosses as a lower-case word, not a Rust variant name"
        );
    }

    /// The word the human rendering prints and the word the JSON carries are
    /// one string, so `keeper-syncd ls-files` and `ls-files --json` cannot come
    /// to call the same row two different things.
    #[test]
    fn the_word_and_the_wire_agree() {
        for state in [
            LfsFileState::Virtual,
            LfsFileState::Materialized,
            LfsFileState::Absent,
        ] {
            let wire = serde_json::to_value(state).expect("serialize");
            assert_eq!(
                wire.as_str(),
                Some(state.to_string().as_str()),
                "{state} must serialize as itself"
            );
        }
    }
}
