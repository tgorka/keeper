//! Which vault file an `![[…]]` embed means, what keeper says when it means
//! nothing, and what an embed may not write (Story 45.12, FR-186, FR-187).
//!
//! **One resolution, one sentence.** Story 44.16 resolved a `.csv` embed by
//! trying the target as written and then inside the attachments folder, and
//! said "this note embeds a file the vault does not have" when neither existed.
//! That sentence never named the second place it looked, so a reader who moved
//! `people.csv` into `data/` was told the file is missing and left to guess
//! which two paths keeper had in mind. This module holds the candidate list and
//! the sentence together, so the words cannot describe a search the code did
//! not run.
//!
//! **Nothing here touches a disk.** Producing the candidates is a rule; trying
//! them is the shell's, because containment is `note_protocol::contained_read`'s
//! and that needs a canonicalising `stat` (AD-55/AD-56). The split is what lets
//! the ordering, the sentence and the write refusals be asserted on any machine.
//!
//! **The write refusal is here and not at the call site**, because it is the
//! one rule in this story that can lose somebody's work. A note is written
//! through `notes_save`, which carries `base_rev`, writes a conflict copy and
//! reindexes. An embed's raw editor carries none of those. If a `.md` target
//! ever reached the embed writer, a stale buffer would silently overwrite a
//! note and the user's other machine would find no conflict copy — so the
//! refusal is a value with a test, not a comment above an `if`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::archive::recordings_fts::kind_for_file_name;
use crate::text_file::TextFileVm;
use crate::vm::RecordingNoteTargetKind;

/// Every vault-relative path an embed target may name, in the order they are
/// tried.
///
/// A bare name is looked for where it is written first and in the attachments
/// folder second, because the attachments panel writes `![[attachments/x.csv]]`
/// in full and a human types `![[x.csv]]`. A target that already has a slash in
/// it is a path the writer meant literally: prefixing the attachments folder
/// onto it would let `![[data/people.csv]]` resolve to a different file with
/// the same name somewhere else in the vault, and an edit would then write to
/// whichever one keeper happened to find today.
///
/// `attachments_dir` is a parameter rather than a constant here so that the
/// shell's own `ATTACHMENTS_DIR` stays the single spelling of that folder. A
/// copy in this crate would be a second name for one directory, and the day one
/// of them changed the embed would look in a folder nothing writes to.
pub fn candidates(target: &str, attachments_dir: &str) -> Vec<String> {
    let mut out = vec![target.to_owned()];
    if !target.contains('/') {
        out.push(format!("{attachments_dir}/{target}"));
    }
    out
}

/// What the reader is told when none of [`candidates`] exists.
///
/// It names the paths, because the acceptance criterion is that an embed whose
/// file has moved says so **where the embed is, naming the path it looked
/// for**. "keeper could not find it" sends somebody to search a vault of four
/// hundred folders; "keeper looked for `people.csv` and
/// `attachments/people.csv`" tells them the file is one `mv` from working.
///
/// The target leads the sentence so the message identifies itself when several
/// embeds in one note are all broken — which is the usual case, because the
/// folder moved rather than the file.
pub fn not_found_notice(target: &str, candidates: &[String]) -> String {
    let looked = candidates
        .iter()
        .map(|rel| rel.as_str())
        .collect::<Vec<_>>()
        .join(" and ");
    format!(
        "{target}: this note embeds a file the vault does not have — \
         keeper looked for {looked}"
    )
}

/// Why this path may not be written through an embed's raw editor, or `None`.
///
/// `extension` is the caller's already-lowercased extension (the shell's
/// `notes_vault::extension`), so this crate does not grow a second answer to
/// "what is this file's extension" that could disagree with the one the vault
/// walk uses.
///
/// Only notes are refused, and deliberately only notes: every other refusal in
/// this story belongs to a layer that already owns it — the format's
/// `writable` flag is the viewer registry's, "these bytes are not text" is
/// [`TextFileVm::binary`]'s, and "this file is too big to edit" is
/// [`crate::text_file`]'s. Adding a second allow-list of writable formats here
/// would be a table with nothing keeping it in step with the registry the
/// frontend actually renders from.
pub fn write_refusal(rel: &str, extension: Option<&str>) -> Option<String> {
    if extension == Some("md") {
        return Some(format!(
            "{rel} is a note, and a note is saved through the note editor so that \
             a change made on another machine becomes a conflict copy instead of \
             being overwritten"
        ));
    }
    None
}

/// Where an embed target resolved to, and what keeper says it is
/// (Story 46.11; `kind` added by Story 55.4).
///
/// The answer [`notes_embed_paths`] gives per target, and the whole of what a
/// decoration needs in order to draw a photograph, a video, an audio file or a
/// PDF inside a note: a path to compose a `keeper-note://` URL from, and the
/// one classification this repo has.
///
/// **Why the kind is here and not computed in the webview.** It is the same
/// rule [`NoteEmbedVm::kind`] states — one classifier, in Rust (AD-87) — with
/// one addition this story made unavoidable: an image and a video share no
/// extension with anything in the frontend's viewer registry, deliberately, so
/// the frontend *cannot* tell them apart even if it were allowed to. The
/// alternative was reading a bounded prefix of every embedded file through
/// [`notes_embed_read`] purely to learn what it was, which is a file read per
/// photograph to answer a question the resolver already knows the answer to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteEmbedPathVm {
    /// The vault-relative path that actually resolved, on the same terms as
    /// [`NoteEmbedVm::rel_path`]: a bare `photo.png` comes back as
    /// `attachments/photo.png` when that is where it is.
    pub rel_path: String,
    /// What keeper says this file is (`kind_for_file_name`), from the resolved
    /// file's own name.
    pub kind: RecordingNoteTargetKind,
}

/// A file embedded in a note, as the note's own viewer needs it
/// (Story 45.12, FR-186, FR-187).
///
/// **`kind` is here because the frontend's viewer registry refuses to answer
/// without it.** Story 45.2 made `kind` a required field of every resolution
/// so that no surface can classify a file by its extension; a note embed is a
/// surface like any other, and the only way for it to obey that rule is for
/// Rust to say what the file is. The embed's decoration makes a *syntactic*
/// guess in order to decide that the embed gets to try at all — this field is
/// the answer that then decides what is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteEmbedVm {
    /// The vault-relative path that actually resolved — which is not
    /// necessarily the target the note spells, because a bare name resolves
    /// inside the attachments folder.
    ///
    /// Relative, never absolute: it is rendered next to the embed, and FR-145
    /// is the rule that keeps a home directory out of a screenshot.
    pub rel_path: String,
    /// The file's own name, with no path in it — what the viewer renders and
    /// what `kind` was decided from.
    pub name: String,
    /// What keeper says this file is (`kind_for_file_name`).
    pub kind: RecordingNoteTargetKind,
    /// The bytes, the size and the honest refusals, from the one text reader
    /// Story 45.6 wrote. Not a second reader with its own limits: a file too
    /// big to edit in a panel is too big to edit in a note, and one constant is
    /// the only way those two can agree.
    pub file: TextFileVm,
}

/// Assemble the view model, deciding `name` and `kind` from the resolved path.
///
/// Both are decided here rather than in the webview, and that is the point:
/// splitting a `/`-joined path is trivial, and doing it in the frontend would
/// put the name `kind` was computed from in one language and the name the
/// viewer resolves on in another.
pub fn describe(rel_path: String, file: TextFileVm) -> NoteEmbedVm {
    let name = rel_path
        .rsplit('/')
        .next()
        .unwrap_or(rel_path.as_str())
        .to_owned();
    let kind = kind_for_file_name(&name);
    NoteEmbedVm {
        rel_path,
        name,
        kind,
        file,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_name_is_looked_for_where_it_is_written_and_then_in_attachments() {
        assert_eq!(
            candidates("people.csv", "attachments"),
            vec!["people.csv".to_owned(), "attachments/people.csv".to_owned()]
        );
    }

    #[test]
    fn a_target_with_a_slash_is_taken_literally() {
        // Prefixing the attachments folder here would let one note's embed
        // resolve to a different file with the same name.
        assert_eq!(
            candidates("data/people.csv", "attachments"),
            vec!["data/people.csv".to_owned()]
        );
    }

    #[test]
    fn the_missing_file_notice_names_every_path_that_was_tried() {
        let target = "people.csv";
        let notice = not_found_notice(target, &candidates(target, "attachments"));
        assert!(
            notice.contains("people.csv and attachments/people.csv"),
            "{notice}"
        );
        assert!(notice.starts_with("people.csv:"), "{notice}");
    }

    #[test]
    fn the_notice_for_a_literal_target_names_the_one_path() {
        let target = "data/people.csv";
        let notice = not_found_notice(target, &candidates(target, "attachments"));
        assert!(
            notice.ends_with("keeper looked for data/people.csv"),
            "{notice}"
        );
    }

    #[test]
    fn a_note_is_never_written_through_an_embed() {
        let refusal =
            write_refusal("Weekly review.md", Some("md")).expect("a note must be refused");
        assert!(refusal.contains("conflict copy"), "{refusal}");
    }

    #[test]
    fn a_data_file_is_written() {
        assert_eq!(write_refusal("attachments/people.csv", Some("csv")), None);
        assert_eq!(write_refusal("attachments/rows.jsonl", Some("jsonl")), None);
        assert_eq!(write_refusal("Makefile", None), None);
    }

    #[test]
    fn the_name_and_the_kind_come_from_the_resolved_path() {
        let vm = describe("attachments/people.csv".to_owned(), text("a,b\n"));
        assert_eq!(vm.name, "people.csv");
        assert_eq!(vm.kind, RecordingNoteTargetKind::File);
        assert_eq!(vm.rel_path, "attachments/people.csv");
    }

    #[test]
    fn a_root_level_file_keeps_its_whole_name() {
        assert_eq!(
            describe("people.csv".to_owned(), text("")).name,
            "people.csv"
        );
    }

    #[test]
    fn the_kind_is_rusts_and_not_the_extensions() {
        // A `.mov` embedded in a note is not a data file, and the frontend must
        // hear that from here rather than deciding it from the spelling.
        assert_eq!(
            describe("attachments/clip.mov".to_owned(), text("")).kind,
            RecordingNoteTargetKind::Video
        );
    }

    fn text(body: &str) -> TextFileVm {
        TextFileVm {
            text: Some(body.to_owned()),
            size_bytes: body.len() as u64,
            size_label: format!("{} bytes", body.len()),
            oversize: false,
            binary: false,
            detail: None,
        }
    }
}
