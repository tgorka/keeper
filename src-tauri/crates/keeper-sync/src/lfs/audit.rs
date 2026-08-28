//! Asking the server whether it really holds what the pointers promise
//! (DW-140).
//!
//! # The gap this closes
//!
//! [`crate::engine::Engine::verify`] checks the *local* half: every pointer in
//! the worktree names an object this machine still has, at the right length.
//! That is the half that can be answered without a network, and it is the half
//! that was implemented.
//!
//! The other half is the one that loses data. A pointer is a promise about
//! content that lives somewhere else, and the whole point of pushing it is that
//! peers can redeem it. If the object never reached the server, every peer that
//! clones gets ~130 bytes of text, the machine that made the commit is the only
//! one that can still supply the bytes, and **nothing anywhere says so**. git is
//! satisfied — the pointer is a perfectly valid blob. The remote is "up to
//! date". The UI is green.
//!
//! Story 34.15 built a gate against exactly this: [`crate::engine::Engine`]
//! holds a push until the objects it names have landed. The gate is real and it
//! is correct. It also, demonstrably, did not hold — on 2026-08-12 a commit of
//! 127 objects published with four of them never uploaded, and on two real
//! repositories an audit found **16 objects, 8.0 GB, missing on the server**
//! while both folders reported a clean sync. Four of those recordings are
//! unrecoverable: the only copies were on a machine that has since replaced
//! them with pointer text.
//!
//! A gate that can be wrong needs a check that runs afterwards. This is that
//! check: cheap, read-only, and answerable in one batch round trip per few
//! hundred pointers, because the LFS batch API reports a `download` object the
//! server cannot serve as a per-object 404 rather than failing the request.
//!
//! # What it deliberately does not do
//!
//! It does not re-upload. Detection and repair are different operations with
//! different risks, and the bytes are frequently not here to send: the machine
//! that ran the audit is usually not the machine that made the commit. Naming
//! the paths is what lets a human find the machine that still has them, which
//! is the only step that can actually recover anything.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::lfs::batch::ObjectId;
use crate::lfs::stage::indexed_pointer;

/// One pointer whose object the server does not have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingObject {
    /// Repository-relative, as a human reads it.
    pub path: String,
    pub oid: String,
    /// What the pointer says the content weighs. The size of the hole.
    pub size: u64,
    /// The server's per-object status code.
    pub code: i64,
    /// That code as a sentence, written here rather than quoted from the
    /// server: `error.rs` requires a message to carry nothing but ids, hosts,
    /// paths, counts and status codes, and an LFS error's `message` is
    /// attacker-influenced text.
    pub reason: String,
}

/// What a per-object status code means for the content it was asked about.
fn explain(code: i64) -> &'static str {
    match code {
        404 => "the server does not have it",
        410 => "the server had it and removed it",
        422 => "the server rejected the object as invalid",
        _ => "the server refused to serve it",
    }
}

/// What one folder's pointers add up to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAudit {
    /// Pointers examined at `HEAD`.
    pub checked: u64,
    /// Distinct objects those pointers name — fewer, when paths share content.
    pub objects: u64,
    /// Bytes the pointers promise, in total.
    pub bytes: u64,
    /// Everything the server could not serve, largest hole first.
    pub missing: Vec<MissingObject>,
}

impl RemoteAudit {
    /// Bytes that exist only as a promise.
    pub fn missing_bytes(&self) -> u64 {
        self.missing.iter().map(|object| object.size).sum()
    }

    /// Is every promise redeemable?
    pub fn is_intact(&self) -> bool {
        self.missing.is_empty()
    }
}

/// Every distinct object the index's pointers name, and where each is used.
///
/// Deduplicated by oid because the batch API is asked about *objects*: two
/// paths with identical content share one, and asking twice would both waste a
/// slot and report one hole as two. The paths are kept — all of them, sorted —
/// because a missing object has to be reported as the file a human recognises,
/// not as a digest.
pub fn tracked_objects(
    repo: &gix::Repository,
    tracked: &[PathBuf],
) -> (Vec<ObjectId>, BTreeMap<String, Vec<PathBuf>>) {
    let mut objects: BTreeMap<String, ObjectId> = BTreeMap::new();
    let mut paths: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for rela in tracked {
        let Some(pointer) = indexed_pointer(repo, rela) else {
            continue; // an ordinary file, not an LFS path
        };
        // The empty object is not content anyone can lose, and a server is not
        // obliged to store it. Asking about it would report a hole that is not
        // one.
        if pointer.size == 0 {
            continue;
        }
        paths
            .entry(pointer.oid.clone())
            .or_default()
            .push(rela.clone());
        objects
            .entry(pointer.oid.clone())
            .or_insert_with(|| ObjectId::new(pointer.oid.clone(), pointer.size));
    }
    for used in paths.values_mut() {
        used.sort();
    }
    (objects.into_values().collect(), paths)
}

/// Turn the batch answer into the report, largest hole first.
///
/// `specs` is what the server said about the objects `tracked_objects`
/// produced. An object carrying an `error` is one the server cannot serve; the
/// spec allows a bare object with neither actions nor error on a *download*
/// batch only as a server quirk, and it is not treated as missing, because
/// claiming a hole that is not there would send someone hunting for bytes that
/// are fine.
pub fn report(
    specs: &[crate::lfs::batch::ObjectSpec],
    paths: &BTreeMap<String, Vec<PathBuf>>,
    checked: u64,
    bytes: u64,
) -> RemoteAudit {
    let mut missing = Vec::new();
    for spec in specs {
        let Some(error) = spec.error.as_ref() else {
            continue;
        };
        let used = paths.get(&spec.oid).cloned().unwrap_or_default();
        // One entry per path, not per object: a human is looking for files.
        for path in used {
            missing.push(MissingObject {
                path: display_path(&path),
                oid: spec.oid.clone(),
                size: spec.size,
                code: error.code,
                reason: explain(error.code).to_owned(),
            });
        }
    }
    missing.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    RemoteAudit {
        checked,
        objects: specs.len() as u64,
        bytes,
        missing,
    }
}

/// Did the server affirmatively say it can serve **this** object?
///
/// The proof a deletion needs, and it is the exact inverse of [`report`]'s
/// default. `report` treats a bare object — neither `actions` nor `error` — as
/// present, because claiming a hole that is not there would send somebody
/// hunting for bytes that are fine. That is the right polarity for a *report*:
/// its worst outcome is wasted time.
///
/// A *deletion* cannot borrow that default. Here the worst outcome is content
/// that existed nowhere else, so this reads the same wire shape the way
/// [`crate::lfs::batch::ObjectSpec::disposition`] reads it for a download —
/// which is the crate's one reading of it, and the strict one:
///
/// * **A row for the oid must exist.** Silence is not proof: a server that
///   answered about some other object, or answered with an empty list, has
///   said nothing about this one.
/// * **No row for the oid may carry an `error`.** `disposition` says an error
///   wins over any action, "and if a server sends both, refusing to transfer
///   is the safe reading" — so a healthy row cannot outvote an errored one for
///   the same object.
/// * **Every row must offer a `download` action.** A bare object is precisely
///   what `disposition` calls "a server that owes us an href and did not
///   supply one", and a server that cannot say where the bytes are has not
///   established that it has them.
/// * **Every row's `size` must be the size asked about.** The filesystem
///   branch beside this one proves presence through
///   [`crate::lfs::store::LfsStore::contains`], which verifies the length
///   exactly; an answer about the same digest at a different length is an
///   answer about something else.
///
/// Takes the specs rather than making the round trip so the predicate is a pure
/// function with its own tests, while the caller
/// ([`crate::engine::Engine::remote_serves`]) owns the one thing that varies:
/// how the question got asked. It takes the [`ObjectId`] it asked about rather
/// than a bare oid so the size is part of the question, not a fact the caller
/// is trusted to re-check.
pub fn serves(specs: &[crate::lfs::batch::ObjectSpec], object: &ObjectId) -> bool {
    let mut rows = specs
        .iter()
        .filter(|spec| spec.oid == object.oid)
        .peekable();
    if rows.peek().is_none() {
        return false;
    }
    rows.all(|spec| {
        spec.error.is_none()
            && spec.size == object.size
            && spec
                .action(crate::lfs::batch::Operation::Download)
                .is_some()
    })
}

/// A repository-relative path as the rest of the engine spells one.
fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lfs::batch::{ObjectError, ObjectSpec};

    fn spec(oid: &str, size: u64, error: Option<(i64, &str)>) -> ObjectSpec {
        ObjectSpec {
            oid: oid.to_owned(),
            size,
            authenticated: false,
            actions: None,
            error: error.map(|(code, message)| ObjectError {
                code,
                message: message.to_owned(),
            }),
        }
    }

    #[test]
    fn an_object_the_server_cannot_serve_is_reported_as_every_path_using_it() {
        let mut paths = BTreeMap::new();
        paths.insert(
            "aa".to_owned(),
            vec![PathBuf::from("meetings/a.mov"), PathBuf::from("copy/a.mov")],
        );
        let audit = report(&[spec("aa", 400, Some((404, "Not Found")))], &paths, 2, 400);
        assert_eq!(audit.missing.len(), 2, "both paths name the same hole");
        assert_eq!(audit.missing[0].code, 404);
        assert_eq!(audit.missing[0].reason, "the server does not have it");
        // The server's own message never reaches the report: it is
        // attacker-influenced text, and `error.rs` forbids carrying it.
        assert!(!audit.missing[0].reason.contains("Not Found"));
        assert!(!audit.is_intact());
    }

    /// The holes are ranked by what they cost, because that is the order
    /// somebody chasing them down needs.
    #[test]
    fn the_biggest_hole_is_reported_first() {
        let mut paths = BTreeMap::new();
        paths.insert("small".to_owned(), vec![PathBuf::from("a.mov")]);
        paths.insert("big".to_owned(), vec![PathBuf::from("b.mov")]);
        let audit = report(
            &[
                spec("small", 10, Some((404, "Not Found"))),
                spec("big", 4_000, Some((404, "Not Found"))),
            ],
            &paths,
            2,
            4_010,
        );
        assert_eq!(audit.missing[0].path, "b.mov");
        assert_eq!(audit.missing_bytes(), 4_010);
    }

    /// A server that answers without an error is a server that has the object.
    /// Reporting a hole here would send someone hunting for bytes that are
    /// fine — the expensive kind of false positive.
    #[test]
    fn an_object_the_server_has_is_not_a_hole() {
        let mut paths = BTreeMap::new();
        paths.insert("aa".to_owned(), vec![PathBuf::from("a.mov")]);
        let audit = report(&[spec("aa", 400, None)], &paths, 1, 400);
        assert!(audit.is_intact());
        assert_eq!(audit.missing_bytes(), 0);
    }

    /// A row the server offered a download for, which is the only shape that
    /// authorizes a deletion.
    fn served(oid: &str, size: u64) -> ObjectSpec {
        ObjectSpec {
            actions: Some(crate::lfs::batch::Actions {
                download: Some(crate::lfs::batch::Action {
                    href: "https://example.invalid/o".to_owned(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..spec(oid, size, None)
        }
    }

    /// Proof is affirmative, complete, and about the object asked for.
    ///
    /// Every false case here is a way a deletion could have been authorized by
    /// something that is not evidence, and each one is a real wire shape:
    /// silence, an answer about another object, an errored row, an errored row
    /// standing beside a healthy one, a **bare** row with neither an error nor
    /// an href, and a row about the same digest at a different length.
    /// `report`'s own default says "present" for the bare row and for silence,
    /// which is why this is a separate predicate rather than a reuse.
    #[test]
    fn only_an_affirmative_row_for_that_oid_proves_the_server_serves_it() {
        let asked = ObjectId::new("aa", 400);

        assert!(
            serves(&[served("aa", 400)], &asked),
            "no error, the size asked about, and an href to fetch it from — the \
             server has said it can hand this over"
        );
        assert!(
            serves(&[served("bb", 1), served("aa", 400)], &asked),
            "and the row is found wherever in the answer it sits"
        );

        assert!(
            !serves(&[], &asked),
            "silence is not proof — this is exactly where `report`'s default \
             would have said `present`"
        );
        assert!(
            !serves(&[served("bb", 400)], &asked),
            "an answer about another object says nothing about this one"
        );
        assert!(
            !serves(&[spec("aa", 400, Some((404, "Not Found")))], &asked),
            "an errored row is the server saying it cannot"
        );
        assert!(
            !serves(
                &[served("aa", 400), spec("aa", 400, Some((404, "Not Found")))],
                &asked
            ),
            "one affirmative row does not outvote an errored one for the SAME \
             object: `disposition` says an error wins"
        );
        assert!(
            !serves(&[spec("aa", 400, None)], &asked),
            "a BARE row — no error, no href — is what `disposition` calls a \
             server that owes us a download action and did not supply one"
        );
        assert!(
            !serves(&[served("aa", 399)], &asked),
            "the same digest at a different length is an answer about \
             something else; `LfsStore::contains` verifies the length exactly \
             and so does this"
        );
    }
}
