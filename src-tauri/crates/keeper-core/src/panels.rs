//! What a panel is looking at (Story 45.1, FR-173, AD-90).
//!
//! # Why this is one enum, in this crate
//!
//! Four surfaces produce a thing-to-open — the command palette, the Files tree,
//! the notes list and the recordings list — and every one of them already knows
//! how to name what it holds. Left alone, each would grow its own shape: a
//! `{profileId, subpath}` here, a bare note id there, a session folder somewhere
//! else. The fifth surface then cannot join, and "open this beside that" becomes
//! a boolean on four components (AD-90). One vocabulary, generated into
//! TypeScript by ts-rs, is what makes a target something a surface *passes on*
//! rather than something it re-invents.
//!
//! It lives in `keeper-core` and not in the shell for the ordinary reason
//! (AD-55/AD-56): the shell does not compile on every developer's machine, and a
//! vocabulary nobody can build is a vocabulary nobody can check.
//!
//! # What each variant carries, and why nothing else
//!
//! Every field here is one a producer can actually supply today, and the shape
//! is taken from the row that supplies it rather than invented:
//!
//! - `Note` carries `vault_id` **and** `note_id` because a note id is scoped to
//!   its vault — [`crate::notes::vm::NoteRefVm`], which every mutating notes
//!   command already returns, carries both for exactly that reason. AD-90 spells
//!   the address `note:<id>`; the id alone would be ambiguous the moment a second
//!   vault is configured, which is a supported configuration today.
//! - `File` carries the sync profile and the profile-relative path, field for
//!   field what [`crate::vm::FilesEntryVm`] renders.
//! - `Recording` carries the session id — the recordings browser already keys its
//!   rows by it, and it is the one handle that survives a Story 40.4 retitle.
//! - `Task` carries the task's id, which is the whole of a task's identity. A
//!   task id is its primary key and cannot be edited once the task exists —
//!   every run in the history joins on it, so the record has no way to spell a
//!   rename — which makes it exactly the kind of handle this list is for: the
//!   one string that outlives every change a task can undergo, as a note id
//!   outlives a rename. It is also the handle every task verb already takes,
//!   one argument and nothing else, so a panel holding a task target can ask
//!   `sync_task_history` about it without composing anything.
//!
//! **No absolute path, anywhere.** A panel is restored after a restart, and a
//! panel that had stored an absolute path would come back pointing at a volume
//! that is no longer mounted at that name — or, worse, at a *different* volume
//! mounted there since. The frontend never joins a root and a subpath (AD-65);
//! Rust resolves a target when it is opened, and an absolute path exists only for
//! as long as one action needs it. This is also FR-145: nothing here can end up
//! written into a note carrying somebody's home directory.
//!
//! **No name, no size, no kind, no title.** Those are properties of what the
//! target resolves to right now, and a panel that cached them would render a
//! stale name over a file that had been replaced. A target is an identity; the
//! surface resolves it every time it shows it, and the resolution is where
//! "this is no longer here" gets said.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The thing one panel is a view of (FR-173, AD-90).
///
/// Externally tagged as `{"kind": "...", ...}` rather than serde's default
/// external tagging, so a TypeScript consumer narrows on `target.kind` — the
/// idiom every other tagged VM in this codebase already uses
/// ([`crate::notes::vm::NoteBodyBatch`], [`crate::notes::vm::NoteListOp`]) and
/// the one a `switch` statement can be exhaustive over.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum PanelTargetVm {
    /// One note in one vault.
    Note {
        /// The vault the note lives in. Present because a note id is only unique
        /// within its vault, and more than one vault is an ordinary setup.
        vault_id: String,
        /// The note's stable id, which survives a rename (FR-97) — so a panel
        /// left open on a note that is renamed on another device comes back
        /// pointing at the same note rather than at a path that moved.
        note_id: String,
    },
    /// One file inside one synced folder.
    File {
        /// The sync profile whose root this path is relative to. This is AD-90's
        /// `<vault>`: a vault *is* a synced folder plus a flag (FR-94), so there
        /// is one id for both and no second identifier to keep in step.
        profile_id: String,
        /// The file's path relative to that profile's root, `/`-joined on every
        /// platform. The same frame [`crate::vm::FilesEntryVm::relative_path`]
        /// carries, so a Files row hands its own string over unchanged.
        relative_path: String,
    },
    /// One recording session.
    Recording {
        /// The session's immutable identity — what the recordings browser
        /// already keys its rows by, and the only handle that outlives a retitle
        /// of the session folder.
        session_id: String,
    },
    /// One scheduled task.
    Task {
        /// The task's id: its primary key in the task record, unchangeable once
        /// the task exists because every run joins on it, and the single
        /// argument every task verb takes.
        task_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Story 45.1: the wire shape is the contract four surfaces and one cookie
    /// agree on, so the tags and the field spellings are asserted literally.
    ///
    /// Not a tautology and not a serde smoke test: the panel list is persisted as
    /// this JSON and read back after a restart, so renaming `relativePath` to
    /// `relPath` — a change rustc would accept and the TypeScript compiler would
    /// accept the moment the bindings regenerate — silently empties every
    /// restored panel of a file. The literal is what makes that a red test.
    #[test]
    fn panel_target_wire_shape_is_kind_tagged_and_camel_case() {
        assert_eq!(
            serde_json::to_string(&PanelTargetVm::Note {
                vault_id: "vault-a".into(),
                note_id: "note-1".into(),
            })
            .expect("serialize note"),
            r#"{"kind":"note","vaultId":"vault-a","noteId":"note-1"}"#
        );
        assert_eq!(
            serde_json::to_string(&PanelTargetVm::File {
                profile_id: "prof-a".into(),
                relative_path: "docs/report.pdf".into(),
            })
            .expect("serialize file"),
            r#"{"kind":"file","profileId":"prof-a","relativePath":"docs/report.pdf"}"#
        );
        assert_eq!(
            serde_json::to_string(&PanelTargetVm::Recording {
                session_id: "sess-1".into(),
            })
            .expect("serialize recording"),
            r#"{"kind":"recording","sessionId":"sess-1"}"#
        );
        assert_eq!(
            serde_json::to_string(&PanelTargetVm::Task {
                task_id: "nightly".into(),
            })
            .expect("serialize task"),
            r#"{"kind":"task","taskId":"nightly"}"#
        );
    }

    /// A persisted panel list is read back into these variants, so the reverse
    /// direction is a real code path and not the serializer's mirror image.
    #[test]
    fn panel_target_round_trips_through_its_persisted_json() {
        for target in [
            PanelTargetVm::Note {
                vault_id: "vault-a".into(),
                note_id: "note-1".into(),
            },
            PanelTargetVm::File {
                profile_id: "prof-a".into(),
                relative_path: "a/b/c.csv".into(),
            },
            PanelTargetVm::Recording {
                session_id: "sess-1".into(),
            },
            PanelTargetVm::Task {
                task_id: "nightly".into(),
            },
        ] {
            let json = serde_json::to_string(&target).expect("serialize");
            let back: PanelTargetVm = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, target, "round trip of {json}");
        }
    }

    /// Two targets naming different things are different values, including the
    /// pair that a `<vault>/<rel>` string address would collapse.
    ///
    /// This is why the vocabulary is a struct-variant enum and not a formatted
    /// string: a profile id ending in `/` — or a relative path beginning with one
    /// — makes `profile + "/" + rel` ambiguous, and the ambiguity would show up
    /// as one panel restoring as another panel's file. Nothing in this codebase
    /// formats a target into a single string, and this test is the reason that
    /// restraint is not accidental.
    #[test]
    fn file_targets_with_a_shifted_separator_are_not_equal() {
        let a = PanelTargetVm::File {
            profile_id: "prof".into(),
            relative_path: "a/b.txt".into(),
        };
        let b = PanelTargetVm::File {
            profile_id: "prof/a".into(),
            relative_path: "b.txt".into(),
        };
        assert_ne!(a, b);
        assert_ne!(
            serde_json::to_string(&a).expect("a"),
            serde_json::to_string(&b).expect("b")
        );
    }

    /// A note target and a file target that share their two strings are still
    /// different panels: the tag is part of the identity, and a panel list that
    /// compared only the payload would focus a note when asked for a file.
    ///
    /// The recording/task pair is the sharper half of the same claim, and the
    /// reason it is asserted rather than assumed: those two carry ONE string
    /// each, so the payloads are not merely equal by coincidence — they are the
    /// same shape, and a comparison that reached past the tag would make a task
    /// panel and a recording panel indistinguishable whenever a session and a
    /// task happened to be named alike.
    #[test]
    fn kind_is_part_of_a_target_identity() {
        assert_ne!(
            PanelTargetVm::Note {
                vault_id: "x".into(),
                note_id: "y".into(),
            },
            PanelTargetVm::File {
                profile_id: "x".into(),
                relative_path: "y".into(),
            }
        );
        assert_ne!(
            PanelTargetVm::Recording {
                session_id: "x".into(),
            },
            PanelTargetVm::Task {
                task_id: "x".into(),
            }
        );
    }
}
