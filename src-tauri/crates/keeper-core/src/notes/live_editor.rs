//! What keeper holds for one live editor over a note (AD-58), and the one kind
//! of write keeper itself makes into a note that may be open: a change to its
//! frontmatter block alone (story 88.9's recording tags).
//!
//! The shell keeps one [`LiveEditor`] per body subscription behind a lock and
//! does the IO. The decisions are here, over the bytes and revisions it is
//! handed: whether the buffer is dirty, whether an editor's report or save is
//! against what keeper holds, and whether a keeper block change is adopted
//! under an open editor instead of reading as somebody else's edit.

use std::collections::VecDeque;

use crate::notes::frontmatter::Frontmatter;
use crate::notes::NotesError;

/// How many revisions a run of keeper block changes remembers as superseded.
/// A save or report still in flight from before the first of them is what
/// they are for, and one session tags and untags a note: two.
const AMENDED_KEPT: usize = 8;

/// Split a note into its frontmatter block and its body.
///
/// The block is `source[..body_offset]` — fences, any byte-order mark and the
/// newline after the closing fence included — so the two halves concatenated
/// are the source again, byte for byte. An empty block means the note has none.
pub fn split_note(source: &str) -> (&str, &str) {
    let (_, body_offset) = Frontmatter::parse(source);
    source.split_at(body_offset)
}

/// `note` with its frontmatter block changed by `amend`, or `Ok(None)` when
/// `amend` declines or leaves the note as it was. A change that reaches the
/// body is refused: the body is the editor's, and keeper writing into it is
/// exactly what an open editor cannot absorb.
pub fn amend_block(
    note: &str,
    amend: impl FnOnce(&str) -> Option<String>,
) -> Result<Option<String>, NotesError> {
    let Some(next) = amend(note).filter(|next| next != note) else {
        return Ok(None);
    };
    if split_note(&next).1 != split_note(note).1 {
        return Err(NotesError::Name(
            "keeper's change to a note's properties would have changed its body, so it was not written"
                .to_owned(),
        ));
    }
    Ok(Some(next))
}

/// What keeper holds for a live editor.
///
/// `base` is the exact bytes keeper last wrote or last delivered — the whole
/// document, block included — and is **never re-read from disk**, which is what
/// makes it a true common ancestor and what makes the clean/dirty distinction
/// meaningful (AD-58). `mine` is the editor's buffer, which is the **body alone**,
/// kept current by the buffer report. `written` is the revision THIS
/// subscription last wrote, and nothing else moves it — `base` and `rev` also
/// follow external edits — so it is what tells our own autosave apart from
/// somebody else's write.
///
/// A keeper block change adopted under the editor moves `base` and `rev` too,
/// and remembers the revisions it superseded: an editor that has not heard of
/// it yet still reports and saves against one of those, and that is not
/// somebody else's edit — the disk differs only by keeper's own block change.
#[derive(Debug, Clone)]
pub struct LiveEditor {
    /// The note, relative to its vault; follows a rename.
    pub rel: String,
    pub base: String,
    pub rev: String,
    pub mine: Option<String>,
    pub written: Option<String>,
    /// Revisions a run of keeper block changes replaced, oldest first.
    amended_from: VecDeque<String>,
    /// The revision the last keeper block change wrote.
    amended_to: Option<String>,
}

impl LiveEditor {
    /// An editor opened on `base`, revision `rev`.
    pub fn opened(rel: String, base: String, rev: String) -> Self {
        Self {
            rel,
            base,
            rev,
            mine: None,
            written: None,
            amended_from: VecDeque::new(),
            amended_to: None,
        }
    }

    /// The block and the body of `base`.
    pub fn split(&self) -> (&str, &str) {
        split_note(&self.base)
    }

    /// Whether the editor has unsaved edits. Body against body: the block is not
    /// the editor's to change, so it can never be what makes a buffer dirty.
    pub fn is_dirty(&self) -> bool {
        self.mine
            .as_ref()
            .is_some_and(|mine| mine.as_str() != self.split().1)
    }

    /// Whether `rev` is one a keeper block change replaced, while nothing but
    /// keeper block changes have happened since.
    fn superseded(&self, rev: &str) -> bool {
        self.amended_to.as_deref() == Some(self.rev.as_str())
            && self.amended_from.iter().any(|from| from == rev)
    }

    /// The editor's buffer heartbeat: `text` is its body as of revision `rev`.
    ///
    /// A report against a revision keeper has moved past is stale — the editor
    /// sent it before it applied an external change — so it must not resurrect
    /// the old buffer as "mine". A revision only a keeper block change replaced
    /// is not stale: the body under it is the same.
    pub fn report(&mut self, text: String, rev: &str) {
        if self.rev == rev || self.superseded(rev) {
            self.mine = Some(text);
        }
    }

    /// What a save against `base_rev` over a disk at `disk_rev` writes over.
    /// A body-only save over [`SaveBase::Amended`] is current: it keeps the
    /// block in `base`, which is keeper's amendment. A save that brings its own
    /// block must be refused there instead — that block was composed before
    /// keeper's change reached the editor and would undo it.
    pub fn save_base(&self, base_rev: &str, disk_rev: &str) -> SaveBase {
        if disk_rev == base_rev {
            SaveBase::Current
        } else if disk_rev == self.rev && self.superseded(base_rev) {
            SaveBase::Amended
        } else {
            SaveBase::Stale
        }
    }

    /// [`Self::save_base`] for a save that brings its own block (`own_block`,
    /// the properties panel) or keeps `base`'s (the autosave): the first is
    /// refused over keeper's amendment rather than written.
    pub fn admit_save(
        &self,
        base_rev: &str,
        disk_rev: &str,
        own_block: bool,
    ) -> Result<SaveBase, NotesError> {
        match self.save_base(base_rev, disk_rev) {
            SaveBase::Amended if own_block => Err(NotesError::Name(
                "keeper changed this note's properties while you edited them, so your change \
                 was not saved. Make it again."
                    .to_owned(),
            )),
            base => Ok(base),
        }
    }

    /// keeper changed the note's block on disk from `disk` to `next` (revision
    /// `next_rev`), holding this editor's saves back while it did. Adopted when
    /// the editor's base body is the disk's — its buffer, dirty or not, then
    /// descends from `next` exactly as it did from `disk` — and `true` then, so
    /// the editor is told. An editor whose base body differs has an external
    /// change pending already, and the watcher's usual answer covers both.
    pub fn adopt_amendment(&mut self, disk: &str, next: String, next_rev: String) -> bool {
        if self.split().1 != split_note(disk).1 {
            return false;
        }
        if self.amended_to.as_deref() != Some(self.rev.as_str()) {
            self.amended_from.clear();
        }
        if self.amended_from.len() == AMENDED_KEPT {
            self.amended_from.pop_front();
        }
        self.amended_from
            .push_back(std::mem::replace(&mut self.rev, next_rev.clone()));
        self.base = next;
        self.amended_to = Some(next_rev);
        true
    }

    /// This editor's save landed: `stamped` at revision `rev` is on disk.
    pub fn saved(&mut self, stamped: String, rev: String) {
        self.base = stamped;
        self.written = Some(rev.clone());
        self.rev = rev;
        self.mine = None;
    }
}

/// What a save's base revision is against the disk ([`LiveEditor::save_base`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveBase {
    /// The disk is the base: nobody else wrote.
    Current,
    /// The disk differs from the base by keeper's own block changes alone.
    Amended,
    /// Somebody else wrote: the disk is kept aside as a conflict copy.
    Stale,
}

#[cfg(test)]
mod tests {
    use std::hash::{DefaultHasher, Hash, Hasher};

    use super::*;
    use crate::notes::note_recording::{with_recording_tags, without_recording_tags};

    fn rev(text: &str) -> String {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    const OPENED: &str = "---\ntitle: Standup\n---\nAgenda.\n";
    const TYPED: &str = "Agenda.\nTyped while it records.\n";

    fn dirty_editor() -> LiveEditor {
        let mut editor = LiveEditor::opened("n.md".to_owned(), OPENED.to_owned(), rev(OPENED));
        editor.report(TYPED.to_owned(), &rev(OPENED));
        assert!(editor.is_dirty());
        editor
    }

    fn tagged(note: &str) -> String {
        amend_block(note, |text| Some(with_recording_tags(text, "hesperia")))
            .expect("a block change")
            .expect("the tags were missing")
    }

    #[test]
    fn a_dirty_editor_saves_its_words_under_keepers_tags_without_a_conflict_copy() {
        let mut editor = dirty_editor();
        let tags = tagged(OPENED);
        assert!(editor.adopt_amendment(OPENED, tags.clone(), rev(&tags)));

        // The editor's autosave was composed before it heard of the tags, so it
        // still carries the revision it opened on.
        assert!(
            editor.save_base(&rev(OPENED), &rev(&tags)) == SaveBase::Amended,
            "the disk differs only by keeper's tags: no conflict copy"
        );
        let saved = format!("{}{TYPED}", editor.split().0);
        assert_eq!(saved, format!("{}{TYPED}", split_note(&tags).0));
        assert!(saved.contains("recording/hesperia"), "{saved}");
        assert!(editor.is_dirty(), "the typed words are still unsaved");

        // And the tags go the same way at Stop, with the first save still in
        // flight.
        let untagged = amend_block(&tags, |text| Some(without_recording_tags(text, "hesperia")))
            .expect("a block change")
            .expect("the tags were there");
        assert!(editor.adopt_amendment(&tags, untagged.clone(), rev(&untagged)));
        // Untagged, the note is byte for byte what the editor opened on.
        assert_eq!(
            editor.save_base(&rev(OPENED), &rev(&untagged)),
            SaveBase::Current
        );
        assert_eq!(editor.split().0, split_note(OPENED).0);
    }

    #[test]
    fn a_report_from_before_keepers_block_change_still_counts() {
        let mut editor = LiveEditor::opened("n.md".to_owned(), OPENED.to_owned(), rev(OPENED));
        let tags = tagged(OPENED);
        assert!(editor.adopt_amendment(OPENED, tags.clone(), rev(&tags)));
        editor.report(TYPED.to_owned(), &rev(OPENED));
        assert!(
            editor.is_dirty(),
            "the words typed before the tags are the buffer"
        );
    }

    #[test]
    fn somebody_elses_edit_after_keepers_block_change_is_still_a_conflict() {
        let mut editor = dirty_editor();
        let tags = tagged(OPENED);
        assert!(editor.adopt_amendment(OPENED, tags.clone(), rev(&tags)));
        // The watcher saw another device's write land on top; the buffer is
        // dirty, so only the revision moves.
        let theirs = format!("{}Their words.\n", split_note(&tags).0);
        editor.rev = rev(&theirs);

        assert_eq!(
            editor.save_base(&rev(OPENED), &rev(&theirs)),
            SaveBase::Stale
        );
        assert_eq!(
            editor.save_base(&rev(&tags), &rev(&theirs)),
            SaveBase::Stale
        );
        editor.report("stale".to_owned(), &rev(OPENED));
        assert_eq!(
            editor.mine.as_deref(),
            Some(TYPED),
            "a stale report is dropped"
        );
    }

    #[test]
    fn a_property_edit_composed_before_keepers_tags_is_refused_not_written() {
        let mut editor = dirty_editor();
        let tags = tagged(OPENED);
        assert!(editor.adopt_amendment(OPENED, tags.clone(), rev(&tags)));

        assert!(
            editor.admit_save(&rev(OPENED), &rev(&tags), true).is_err(),
            "its block would undo keeper's tags"
        );
        assert_eq!(
            editor
                .admit_save(&rev(OPENED), &rev(&tags), false)
                .expect("a body-only save keeps keeper's block"),
            SaveBase::Amended
        );
        assert_eq!(
            editor
                .admit_save(&rev(&tags), &rev(&tags), true)
                .expect("composed after the tags arrived"),
            SaveBase::Current
        );
    }

    #[test]
    fn an_editor_with_a_change_pending_does_not_adopt_keepers() {
        let mut editor = dirty_editor();
        let disk = "---\ntitle: Standup\n---\nSomebody else's agenda.\n";
        let tags = tagged(disk);
        assert!(!editor.adopt_amendment(disk, tags.clone(), rev(&tags)));
        assert_eq!(editor.base, OPENED);
        assert_eq!(editor.save_base(&rev(OPENED), &rev(&tags)), SaveBase::Stale);
    }

    #[test]
    fn a_block_change_that_reaches_the_body_is_refused() {
        assert!(amend_block(OPENED, |text| Some(format!("{text}More.\n"))).is_err());
        assert_eq!(amend_block(OPENED, |_| None).expect("declined"), None);
        assert_eq!(
            amend_block(OPENED, |text| Some(text.to_owned())).expect("unchanged"),
            None
        );
    }

    /// Dirtiness is a body-against-body question. `base` is the whole document,
    /// `mine` is the editor's buffer, and the block between them is not the
    /// editor's to change — so a buffer holding exactly the delivered body is
    /// clean, block or no block.
    #[test]
    fn a_dirty_buffer_is_the_one_that_differs_from_what_we_delivered() {
        let mut state = LiveEditor::opened(
            "a.md".to_owned(),
            "---\nid: 01AAA\n---\nhello".to_owned(),
            "5-x".to_owned(),
        );
        assert!(!state.is_dirty(), "no report yet is not dirty");
        state.mine = Some("hello".to_owned());
        assert!(
            !state.is_dirty(),
            "a buffer identical to the body we delivered is not dirty"
        );
        // The whole document is NOT what the editor holds: a buffer that somehow
        // carried the block would be a buffer that had diverged.
        state.mine = Some(state.base.clone());
        assert!(state.is_dirty());
        state.mine = Some("hello world".to_owned());
        assert!(state.is_dirty());
    }
}
