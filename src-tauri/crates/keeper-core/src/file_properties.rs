//! A file's own properties: the frontmatter block of a file keeper did not
//! author (Story 50.4, FR-283, AD-120).
//!
//! # Why this is a module and not a line in `sync_ipc.rs`
//!
//! The whole of this story is byte preservation — a write that changes one
//! property leaves every other byte of the file identical — and the shell crate
//! does not build on a Linux developer machine (AD-55, AD-56). A splice written
//! in `sync_ipc.rs` would be a splice nobody could exercise until macOS, over
//! the one class of bug that is invisible until somebody's `README.md` comes
//! back with its body shifted. So the arithmetic is here, pure over `&str`, and
//! the command is a call site. [`crate::text_file`] is here for the same
//! reason.
//!
//! # Why it is not in `notes`
//!
//! The address is a sync-profile path, not a vault. AD-120 makes the tag the
//! thing that files a file into a space (`sessions::pool`), so a session's
//! `README.md` needs properties without ever being a note — it has no note id,
//! no subscription and no vault. What it shares with a note is the *bytes*, and
//! those come from [`crate::notes::frontmatter`], which stays the one
//! frontmatter parser and the one frontmatter writer. Nothing here re-implements
//! it: [`block_of`] and [`replace_block`] are two calls to
//! [`Frontmatter::parse`] and a splice around what it says.
//!
//! # Why the write takes the whole block, and not a key and a value
//!
//! The properties panel already speaks blocks. It reads a block, splices one
//! key's value span, and hands the whole block back — that is what
//! `notes_save`'s `frontmatter` parameter takes today, and it is what makes the
//! panel's FR-121 promise (every key the user did not edit survives) a property
//! of one code path rather than of two. A key-and-value command here would be a
//! second write protocol for one panel, and the panel would have to know which
//! address it was serving in order to pick. It does not, and it must not.
//!
//! What this side owes in exchange is that the block it is handed cannot damage
//! the rest of the file: [`replace_block`] refuses anything that is not exactly
//! one terminated `---` block, so a key carrying a stray fence cannot turn a
//! document's body into frontmatter.
//!
//! # The guard is the block, not the file
//!
//! `PlanStep::GuardedWrite` (`sessions::plan`) guards a whole-file rewrite on
//! the length the caller read, and the notes side guards a whole-note save on a
//! content fingerprint. Both are the right shape for a write that replaces
//! everything. This one replaces only the block, and takes the body from the
//! copy it just read — so the only edit it can lose is a concurrent edit *to
//! the block*, and the only precondition it needs is that the block is still
//! the one the surface was editing.
//!
//! Guarding on the whole file instead would be strictly worse in both
//! directions: it would refuse a person who happened to be typing in the body
//! at the time, and — length being length — it would still let a same-length
//! change to the block through. Comparing the bytes that are about to be
//! replaced is both cheaper and exact.

use crate::notes::bom_len;
use crate::notes::frontmatter::Frontmatter;

/// Why a properties write did not land.
///
/// Both arms are sentences a person reads, worded here rather than in the shell
/// so they are asserted on every machine (AD-56).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PropertiesRefusal {
    /// The block on disk is no longer the one the surface was editing.
    ///
    /// A refusal rather than a conflict copy, which is what the notes side
    /// writes: a note save carries a whole document the user typed and losing it
    /// would be unrecoverable, where a property edit is one field the person can
    /// re-apply in a second. Offering to re-read is the honest answer, and it is
    /// the sentence's last clause.
    #[error(
        "{subpath}'s properties changed on disk while they were being edited; \
         nothing was written — re-read the file and try again"
    )]
    Stale {
        /// The path that was asked for, profile-relative.
        subpath: String,
    },
    /// The block handed over is not exactly one terminated `---` block.
    #[error(
        "keeper will not write those properties to {subpath}: they are not a \
         well-formed `---` block, and writing them would read the rest of the \
         file as frontmatter"
    )]
    Malformed {
        /// The path that was asked for, profile-relative.
        subpath: String,
    },
}

/// The document's leading `---` block, verbatim, or `""` when it has none.
///
/// The byte-order mark is deliberately **not** part of the answer. A BOM before
/// the fence belongs to the file, not to its properties, and a surface that got
/// one back would either show it as a stray character or write it a second time
/// on the way in. [`replace_block`] re-attaches it from the original, so it
/// survives a round trip without anything above this layer knowing it exists.
pub fn block_of(source: &str) -> &str {
    let start = bom_len(source);
    let (_, body_offset) = Frontmatter::parse(source);
    // `body_offset` is 0 for a document with no block, which for a document
    // that also has a BOM would be an inverted range.
    &source[start..body_offset.max(start)]
}

/// Splice `block` in as the document's frontmatter, keeping every other byte.
///
/// `expect` is the block the caller read — see the module note on why the guard
/// is the block rather than the file. `subpath` names the file in whichever
/// refusal comes back, and is used for nothing else.
///
/// With no block present the new one goes in front of the body, and the body
/// keeps its first line unshifted and unblanked: this is the owner's live
/// `README.md`, whose first line is its `# Title`.
///
/// # Errors
///
/// [`PropertiesRefusal::Stale`] when the block on disk is not `expect`;
/// [`PropertiesRefusal::Malformed`] when `block` is not exactly one terminated
/// `---` block.
pub fn replace_block(
    source: &str,
    expect: &str,
    block: &str,
    subpath: &str,
) -> Result<String, PropertiesRefusal> {
    let start = bom_len(source);
    let (_, body_offset) = Frontmatter::parse(source);
    let split = body_offset.max(start);
    if &source[start..split] != expect {
        return Err(PropertiesRefusal::Stale {
            subpath: subpath.to_owned(),
        });
    }
    let body = &source[split..];
    if !well_formed(block, body) {
        return Err(PropertiesRefusal::Malformed {
            subpath: subpath.to_owned(),
        });
    }

    let mut out = String::with_capacity(split - start + block.len() + body.len());
    out.push_str(&source[..start]);
    out.push_str(block);
    out.push_str(body);
    Ok(out)
}

/// Whether `block` is exactly one frontmatter block, and one `body` can follow.
///
/// Empty is well-formed: it is what a file with no properties has, and writing
/// it is how the last property leaves.
fn well_formed(block: &str, body: &str) -> bool {
    if block.is_empty() {
        return true;
    }
    let (frontmatter, offset) = Frontmatter::parse(block);
    // Exactly a block: an opening fence that is closed, and nothing after the
    // closing one. `offset` is the byte just past the closing fence's line, so
    // anything trailing makes this shorter than the block.
    if !frontmatter.has_block() || offset != block.len() {
        return false;
    }
    // And the closing fence is terminated, unless there is nothing to terminate
    // it against. Without the newline the body's first line would be glued onto
    // the `---`, and a fence with text after it is not a fence.
    block.ends_with('\n') || body.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape `migrate.rs`'s fixture has and the owner's live session has:
    /// no frontmatter at all, a body that starts with its title.
    const README: &str = "# Weekly sync\n\nNotes from the call.\n";

    /// What the properties panel produces for a first `tags:` key.
    const ABOUT: &str = "---\ntags:\n  - about\n---\n";

    /// Row 1 — a file with no frontmatter gets a well-formed block, and the
    /// body keeps its first line rather than being shifted or blanked.
    #[test]
    fn a_file_with_no_frontmatter_gets_a_block_above_an_untouched_body() {
        let written = replace_block(README, "", ABOUT, "active/s/README.md")
            .expect("a file with no block accepts its first one");

        assert_eq!(written, format!("{ABOUT}{README}"));
        assert!(
            written.ends_with(README),
            "every byte of the body survives, first line included: {written:?}"
        );
        assert_eq!(
            block_of(&written),
            ABOUT,
            "and the block reads back as the one that was written"
        );
    }

    /// Row 2 — the acceptance sentence, at the byte level: the owner's live
    /// `README.md` becomes `tags: [about]` and nothing else about it changes.
    #[test]
    fn the_owners_readme_keeps_every_byte_below_its_new_block() {
        let written =
            replace_block(README, "", ABOUT, "60-sessions/active/weekly/README.md").expect("filed");

        let (_, body_offset) = Frontmatter::parse(&written);
        assert_eq!(
            &written[body_offset..],
            README,
            "the body below the block is byte-identical"
        );
        assert!(
            !written.contains("id:"),
            "and keeper stamped no identity into a file it did not author"
        );
    }

    /// Row 3 — a CRLF file stays a CRLF file everywhere the write did not go.
    #[test]
    fn crlf_endings_survive_outside_the_block() {
        let source = "---\r\ntitle: Sync\r\n---\r\nline one\r\nline two\r\n";
        let block = block_of(source);
        assert_eq!(block, "---\r\ntitle: Sync\r\n---\r\n");

        // The panel splices the value span and hands the block back with its
        // own line endings intact.
        let edited = block.replace("Sync", "Standup");
        let written = replace_block(source, block, &edited, "notes/sync.md").expect("written");

        assert_eq!(
            written,
            "---\r\ntitle: Standup\r\n---\r\nline one\r\nline two\r\n"
        );
        assert!(
            !written.contains("\n\n") && written.matches("\r\n").count() == 5,
            "no ending was rewritten: {written:?}"
        );
    }

    /// Row 4 — a `---` inside the body is a thematic break, and a document that
    /// opens with one is all body.
    #[test]
    fn a_dashed_line_in_the_body_is_not_a_block() {
        let source = "# Report\n\nAbove.\n\n---\n\nBelow.\n";
        assert_eq!(block_of(source), "", "the file has no frontmatter");

        let written = replace_block(source, "", ABOUT, "refs/report.md").expect("written");

        assert_eq!(written, format!("{ABOUT}{source}"));
        assert_eq!(
            written.matches("\n---\n").count(),
            2,
            "the closing fence and the body's own rule, and no third: {written:?}"
        );
    }

    /// Row 4, the other half — a block that IS there ends at its own fence, and
    /// a later `---` is body the write must not swallow.
    #[test]
    fn only_the_leading_block_is_replaced() {
        let source = "---\ntitle: Report\n---\n\nAbove.\n\n---\n\nBelow.\n";
        let block = block_of(source);
        assert_eq!(block, "---\ntitle: Report\n---\n");

        let written = replace_block(source, block, "---\ntitle: Recap\n---\n", "refs/report.md")
            .expect("written");

        assert_eq!(
            written,
            "---\ntitle: Recap\n---\n\nAbove.\n\n---\n\nBelow.\n"
        );
    }

    /// Row 5 — one key edited, and every other key keeps its place, its
    /// quoting, its list form and the comment beside it.
    #[test]
    fn an_edited_key_leaves_every_other_key_exactly_as_it_was() {
        let source = concat!(
            "---\n",
            "# hand-written, and it stays\n",
            "title: \"Weekly sync\"\n",
            "status: open  # was blocked\n",
            "tags: [standup, team]\n",
            "people:\n",
            "  - ada\n",
            "  - grace\n",
            "---\n",
            "Body.\n",
        );
        let block = block_of(source);
        // Exactly what `spliceProperty` produces: the value span of one key,
        // replaced, and not one byte else.
        let edited = block.replace("status: open", "status: done");

        let written = replace_block(source, block, &edited, "active/s/notes.md").expect("written");

        assert!(written.contains("# hand-written, and it stays\n"));
        assert!(written.contains("title: \"Weekly sync\"\n"), "quoting kept");
        assert!(
            written.contains("status: done  # was blocked\n"),
            "comment kept"
        );
        assert!(
            written.contains("tags: [standup, team]\n"),
            "flow list kept"
        );
        assert!(
            written.contains("people:\n  - ada\n  - grace\n"),
            "block list kept"
        );
        let title_at = written.find("title:").expect("title survives");
        let status_at = written.find("status:").expect("status survives");
        assert!(title_at < status_at, "and the order is the file's own");
        assert!(written.ends_with("---\nBody.\n"));
    }

    /// Row 10 — somebody else changed the properties in between, so the write
    /// refuses instead of dropping their edit.
    #[test]
    fn a_block_that_changed_underneath_refuses_rather_than_clobbering() {
        let read = "---\ntitle: Sync\n---\nBody.\n";
        let stale = block_of(read).to_owned();
        // An agent gets there first.
        let disk = "---\ntitle: Sync\nowner: ada\n---\nBody.\n";

        let refusal = replace_block(disk, &stale, "---\ntitle: Standup\n---\n", "active/s/n.md")
            .expect_err("a changed block refuses");

        assert_eq!(
            refusal,
            PropertiesRefusal::Stale {
                subpath: "active/s/n.md".to_owned()
            }
        );
        assert!(
            refusal
                .to_string()
                .contains("re-read the file and try again"),
            "and the sentence offers the way out: {refusal}"
        );
        assert!(
            refusal.to_string().starts_with("active/s/n.md"),
            "named, because a person may have several files open: {refusal}"
        );
    }

    /// The other side of that guard, and the reason it is the block rather than
    /// the file: an agent appending to the BODY loses nothing and refuses
    /// nothing, because the body written is the one just read.
    #[test]
    fn a_concurrent_body_edit_is_neither_refused_nor_lost() {
        let read = "---\ntitle: Sync\n---\nBody.\n";
        let block = block_of(read).to_owned();
        let disk = "---\ntitle: Sync\n---\nBody.\nAn agent added this line.\n";

        let written = replace_block(disk, &block, "---\ntitle: Standup\n---\n", "active/s/n.md")
            .expect("a body change is not a clobber");

        assert_eq!(
            written,
            "---\ntitle: Standup\n---\nBody.\nAn agent added this line.\n"
        );
    }

    /// Row 12 — the keeper-owned tier. Nothing here stamps: no `id:` into a file
    /// keeper did not author (the sessions contract), and no `updated:` either,
    /// which is what `notes_ipc::save_document` does for a note and must not do
    /// here.
    #[test]
    fn nothing_is_stamped_into_a_file_keeper_did_not_author() {
        let source = "# Someone else's file\n";

        let written = replace_block(source, "", ABOUT, "active/s/README.md").expect("written");

        assert_eq!(written, format!("{ABOUT}{source}"));
        for stamped in ["id:", "updated:", "created:", "keeper:"] {
            assert!(
                !written.contains(stamped),
                "keeper wrote `{stamped}` into a file that did not ask for it: {written:?}"
            );
        }
    }

    /// A byte-order mark belongs to the file, not to its properties: it is not
    /// in what the surface reads, and it is still there afterwards.
    #[test]
    fn a_byte_order_mark_is_kept_out_of_the_block_and_left_on_the_file() {
        let source = "\u{feff}---\ntitle: Sync\n---\nBody.\n";
        let block = block_of(source);
        assert_eq!(block, "---\ntitle: Sync\n---\n", "no marker in the block");

        let written =
            replace_block(source, block, "---\ntitle: Standup\n---\n", "n.md").expect("written");

        assert_eq!(written, "\u{feff}---\ntitle: Standup\n---\nBody.\n");
    }

    /// And with a marker but no block, the new block goes after the marker
    /// rather than in front of it.
    #[test]
    fn a_first_block_lands_after_the_byte_order_mark() {
        let source = "\u{feff}# Title\n";
        assert_eq!(block_of(source), "");

        let written = replace_block(source, "", ABOUT, "n.md").expect("written");

        assert_eq!(written, format!("\u{feff}{ABOUT}# Title\n"));
    }

    /// A block whose closing fence is not terminated would glue the body onto
    /// the `---` and stop being a fence, so it is refused rather than written.
    #[test]
    fn an_unterminated_block_is_refused_when_a_body_would_follow_it() {
        let refusal = replace_block(README, "", "---\ntags: [about]\n---", "n.md")
            .expect_err("no terminator, and a body underneath");

        assert_eq!(
            refusal,
            PropertiesRefusal::Malformed {
                subpath: "n.md".to_owned()
            }
        );
        assert!(refusal.to_string().contains("well-formed"), "{refusal}");
    }

    /// The same block over an empty body is fine: there is nothing to glue.
    #[test]
    fn an_unterminated_block_is_written_when_nothing_follows_it() {
        let written = replace_block("", "", "---\ntags: [about]\n---", "n.md").expect("written");

        assert_eq!(written, "---\ntags: [about]\n---");
    }

    /// A key carrying a stray fence would turn the body into frontmatter. It is
    /// refused at this boundary rather than trusted from the surface.
    #[test]
    fn a_block_with_anything_after_its_fence_is_refused() {
        let refusal = replace_block(README, "", "---\ntitle: a\n---\nsmuggled body\n", "n.md")
            .expect_err("more than a block");

        assert_eq!(
            refusal,
            PropertiesRefusal::Malformed {
                subpath: "n.md".to_owned()
            }
        );
    }

    /// Text that never opens a fence is not a block either.
    #[test]
    fn text_that_is_not_a_block_at_all_is_refused() {
        let refusal =
            replace_block(README, "", "tags: [about]\n", "n.md").expect_err("no fence at all");

        assert!(matches!(refusal, PropertiesRefusal::Malformed { .. }));
    }

    /// An empty block is how the last property leaves, and it takes the fences
    /// with it rather than leaving `---\n---\n` behind.
    #[test]
    fn an_empty_block_removes_the_frontmatter_and_keeps_the_body() {
        let source = "---\ntitle: Sync\n---\nBody.\n";

        let written = replace_block(source, block_of(source), "", "n.md").expect("written");

        assert_eq!(written, "Body.\n");
    }

    /// YAML's other closing fence. The parser accepts `...`, so the block that
    /// reads back has to end there too — otherwise the guard would refuse every
    /// write to a file that used it.
    #[test]
    fn a_dot_terminated_block_reads_back_whole() {
        let source = "---\ntitle: Sync\n...\nBody.\n";

        assert_eq!(block_of(source), "---\ntitle: Sync\n...\n");
    }
}
