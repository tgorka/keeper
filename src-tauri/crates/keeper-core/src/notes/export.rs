//! What an export of one note has to carry with it (Story 45.21, FR-199).
//!
//! # The decision this module encodes
//!
//! "Export a note" has two honest readings and they disagree about the bytes.
//!
//! *The markdown alone* is one file, and every `![[attachments/photo.png]]` in
//! it points at a folder that does not exist beside it. That is the exact
//! failure this epic exists to end — keeper handing somebody the name of a
//! thing instead of the thing.
//!
//! *The markdown with its embeds resolved* would mean rewriting each link to
//! wherever the copy landed. That loses the other half: the exported file would
//! no longer be the note. A note is a synced artefact people diff, review and
//! copy back, and an export whose bytes keeper silently edited is one nobody
//! can compare against the vault.
//!
//! So keeper does **neither**. The note's bytes are copied unchanged, and the
//! files it embeds are copied to the *same vault-relative paths* beneath the
//! export folder. `![[attachments/photo.png]]` still means
//! `attachments/photo.png`, because the neighbourhood the link names has been
//! reproduced rather than the link rewritten. Byte-identical and live, instead
//! of one or the other.
//!
//! # Nothing here touches a disk
//!
//! Which candidate exists is a question for the shell, for the reason
//! [`crate::notes::embed`] gives: containment needs a canonicalising `stat`
//! (AD-55/AD-56). [`plan`] takes an `exists` probe and returns a verdict, so
//! the ordering, the classification and every sentence are asserted on any
//! machine.
//!
//! # Not `crate::archive::export`
//!
//! That module renders a chat archive into markdown or JSON — it *composes* a
//! document. This one copies bytes out of a vault and changes none of them.
//! Two verbs, deliberately not one module.

use crate::notes::{attach, embed, links};

/// What one note's export must copy, and what it cannot.
///
/// Three lists rather than one, because the three have different remedies and
/// a receipt that merged them would leave the reader unable to tell "the file
/// moved" from "that is a note, export it separately".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteExportPlan {
    /// Vault-relative paths to copy beside the note, in document order,
    /// deduplicated. The order matters: a reader comparing the receipt against
    /// the note reads top to bottom, and a sorted list would not line up.
    pub attachments: Vec<String>,
    /// Embed targets that name a file and resolved to nothing on disk, spelled
    /// as the note spells them.
    ///
    /// Not a refusal. The note already has a dead embed and the export is not
    /// where that gets discovered — but an export that quietly carried five of
    /// six files would be one nobody could trust, so it is reported.
    pub missing: Vec<String>,
    /// Embed targets that name a *note* — `![[Other Note]]`, `![[daily.md]]`.
    ///
    /// Deliberately not carried and deliberately not "missing": a transclusion
    /// is an edge in the vault graph, and following it would make an export of
    /// one note an export of an unbounded set of them.
    pub notes: Vec<String>,
}

/// Whether an embed target names a note rather than a file to copy.
///
/// **Pinned to its TypeScript mirror by `attach-vectors.json`** since Story
/// 46.11 — see the test at the bottom of this module. It was private and merely
/// cited by `src/lib/notes/attach.ts` while the only reader of the rule in the
/// webview was 46.2's `attachments/`-scoped panel. 46.11 gave it two more: the
/// panel now lists a file wherever the vault holds it, and the in-vault chooser
/// declines to offer a `.md`. A drift would make the chooser offer a file the
/// panel does not list and the export refuses to carry.
///
/// Two shapes, both of which a real vault contains. `![[daily.md]]` is explicit.
/// `![[Other Note]]` has no extension at all, because a wikilink names a note by
/// its title and the index resolves it — so an extensionless target is a note by
/// construction, not a file whose extension somebody forgot. Treating it as a
/// file would put every transclusion in the "could not find" list and teach the
/// reader to ignore that list.
///
/// The test is on the last segment: `notes/2026/daily` is still a title-shaped
/// target, and a folder named `photo.png` in the middle of a path must not make
/// `photo.png/index` look like an image.
///
/// A dotfile needs no special case, and it had one until a mutation proved the
/// case was doing nothing. `.gitignore` splits into an empty stem and
/// `gitignore`, which is not `md`, so the ordinary arm already answers "file".
/// The early return that used to sit here only changed the answer for
/// `.hidden.md` — which it called a file, and a `.md` file is a note by every
/// other rule in this codebase. Untested and wrong; removed rather than kept
/// for the reassurance.
pub fn names_a_note(target: &str) -> bool {
    let name = attach::attachment_name(target);
    match name.rsplit_once('.') {
        Some((_, extension)) => extension.eq_ignore_ascii_case("md"),
        None => true,
    }
}

/// Everything one note's export must carry, given a probe for what is on disk.
///
/// `exists` is asked about vault-relative paths in [`embed::candidates`] order,
/// which is the same order the embed viewer resolves in — a second ordering
/// here would export a different file from the one the note renders.
///
/// Every embed is looked at exactly once even when the note holds it five
/// times: the export copies files, and copying the same photograph five times
/// is not a more faithful export.
pub fn plan(body: &str, attachments_dir: &str, exists: &dyn Fn(&str) -> bool) -> NoteExportPlan {
    let mut out = NoteExportPlan::default();
    let mut seen: Vec<String> = Vec::new();

    for link in links::extract(body) {
        if !link.embed {
            // A mention is not a copy of the picture — `!` is the whole of the
            // difference, exactly as `attach::embedded_attachment_names` reads
            // it.
            continue;
        }
        // No `trim()` and no emptiness guard: `links::extract` is the one link
        // grammar and it already trims both spellings and drops a wikilink with
        // no target. Both lines were here, both survived a mutation, and
        // `a_padded_or_empty_embed_target_is_handled_by_the_one_link_grammar`
        // is what proved no input reaches them. A guard with no input is a
        // second opinion about a rule that already has an owner.
        let target = link.target.as_str();
        let folded = target.to_lowercase();
        if seen.contains(&folded) {
            continue;
        }
        seen.push(folded);

        if names_a_note(target) {
            out.notes.push(target.to_owned());
            continue;
        }
        match embed::candidates(target, attachments_dir)
            .into_iter()
            .find(|candidate| exists(candidate))
        {
            Some(resolved) => {
                if !out.attachments.contains(&resolved) {
                    out.attachments.push(resolved);
                }
            }
            None => out.missing.push(target.to_owned()),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A probe over a fixed vault listing, so every test states its whole disk.
    fn vault<'a>(paths: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |candidate: &str| paths.contains(&candidate)
    }

    #[test]
    fn a_bare_embed_resolves_in_the_attachments_folder() {
        let disk = vault(&["attachments/photo.png", "attachments/chart.svg"]);
        let plan = plan("![[photo.png]] and ![[chart.svg]]", "attachments", &disk);
        assert_eq!(
            plan.attachments,
            vec!["attachments/photo.png", "attachments/chart.svg"]
        );
        assert!(plan.missing.is_empty(), "{plan:?}");
    }

    /// The candidate order is the embed viewer's, so an export carries the file
    /// the note renders and not a same-named one in the attachments folder.
    #[test]
    fn a_bare_embed_prefers_the_path_as_written_over_the_attachments_copy() {
        let disk = vault(&["photo.png", "attachments/photo.png"]);
        let plan = plan("![[photo.png]]", "attachments", &disk);
        assert_eq!(plan.attachments, vec!["photo.png"]);
    }

    /// A target with a slash is literal — prefixing the attachments folder onto
    /// it would carry a different file with the same name.
    #[test]
    fn a_pathed_embed_is_never_looked_for_in_the_attachments_folder() {
        let disk = vault(&["attachments/data/people.csv"]);
        let plan = plan("![[data/people.csv]]", "attachments", &disk);
        assert!(plan.attachments.is_empty(), "{plan:?}");
        assert_eq!(plan.missing, vec!["data/people.csv"]);
    }

    #[test]
    fn document_order_is_kept_and_a_repeat_is_carried_once() {
        let disk = vault(&["attachments/b.png", "attachments/a.png"]);
        let plan = plan(
            "![[b.png]]\n\n![[a.png]]\n\n![[B.PNG]]",
            "attachments",
            &disk,
        );
        assert_eq!(
            plan.attachments,
            vec!["attachments/b.png", "attachments/a.png"]
        );
    }

    #[test]
    fn a_markdown_embed_counts_and_a_plain_link_does_not() {
        let disk = vault(&["attachments/one.png", "attachments/two.png"]);
        let plan = plan(
            "![alt](attachments/one.png)\n[[attachments/two.png]]",
            "attachments",
            &disk,
        );
        assert_eq!(plan.attachments, vec!["attachments/one.png"]);
        assert!(plan.missing.is_empty(), "{plan:?}");
    }

    #[test]
    fn an_embed_inside_a_fence_is_documentation_about_embeds() {
        let disk = vault(&["attachments/photo.png", "attachments/other.png"]);
        let plan = plan(
            "```\n![[photo.png]]\n```\n\n![[other.png]]",
            "attachments",
            &disk,
        );
        assert_eq!(plan.attachments, vec!["attachments/other.png"]);
    }

    #[test]
    fn an_embedded_note_is_named_rather_than_carried_or_missed() {
        let disk = vault(&["attachments/photo.png"]);
        let plan = plan(
            "![[Other Note]]\n![[daily.MD]]\n![[photo.png]]",
            "attachments",
            &disk,
        );
        assert_eq!(plan.notes, vec!["Other Note", "daily.MD"]);
        assert!(plan.missing.is_empty(), "{plan:?}");
        assert_eq!(plan.attachments, vec!["attachments/photo.png"]);
    }

    /// Both halves of the dot rule, because they disagree and only one of them
    /// was ever asserted. `.gitignore` is a file — an empty stem with an
    /// extension that is not `md`. `.hidden.md` is a NOTE, because `.md` means
    /// note everywhere else in keeper, and the leading-dot special case this
    /// module used to carry got that one wrong while changing nothing about
    /// `.gitignore`.
    #[test]
    fn a_dotfile_embed_is_a_file_but_a_dotted_note_is_still_a_note() {
        let disk = vault(&[".gitignore", ".hidden.md"]);
        let plan = plan("![[.gitignore]]\n![[.hidden.md]]", "attachments", &disk);
        assert_eq!(plan.attachments, vec![".gitignore"]);
        assert_eq!(plan.notes, vec![".hidden.md"]);
        assert!(plan.missing.is_empty(), "{plan:?}");
    }

    /// A folder with a dot in it must not make an extensionless target look
    /// like a file.
    #[test]
    fn the_extension_is_read_off_the_last_segment_only() {
        assert!(names_a_note("photo.png/index"));
        assert!(!names_a_note("notes.d/photo.png"));
    }

    /// The vector table shared with the TypeScript mirror (Story 46.11).
    ///
    /// **This is the anti-drift mechanism, and it exists because the rule grew
    /// consumers.** Story 46.2 mirrored [`names_a_note`] in
    /// `src/lib/notes/attach.ts` and deliberately did NOT pin it, recording the
    /// trigger for pinning: a second caller. Story 46.11 added two — the
    /// Attachments panel now lists a body embed wherever the vault holds it, and
    /// the in-vault chooser declines to offer a file this rule calls a note. A
    /// drift now means a chooser offering a file the panel will not list and the
    /// export will not carry, which is the "two answers to one question" shape
    /// this whole feature keeps failing on.
    ///
    /// `include_str!` rather than a runtime read, like `attach.rs`'s: a deleted
    /// or renamed fixture has to be a build failure and not a test that quietly
    /// passes over no vectors.
    #[test]
    fn every_shared_note_target_vector_matches_and_the_table_is_not_empty() {
        const VECTORS_JSON: &str = include_str!("attach-vectors.json");
        let parsed: serde_json::Value =
            serde_json::from_str(VECTORS_JSON).expect("the shared vector fixture parses");
        let vectors = parsed["noteTargets"]
            .as_array()
            .expect("the fixture carries a noteTargets array");
        assert!(
            vectors.len() >= 12,
            "the shared table has been truncated to {} vectors; it is the contract with \
             src/lib/notes/attach.ts and shrinking it silently weakens both suites",
            vectors.len()
        );
        for vector in vectors {
            let target = vector["target"].as_str().expect("target is a string");
            let expected = vector["isNote"].as_bool().expect("isNote is a bool");
            assert_eq!(
                names_a_note(target),
                expected,
                "vector {:?} ({})",
                target,
                vector["why"].as_str().unwrap_or_default()
            );
        }
    }

    #[test]
    fn a_missing_embed_is_reported_and_the_rest_still_go() {
        let disk = vault(&["attachments/here.png", "attachments/also.png"]);
        let plan = plan(
            "![[here.png]] ![[gone.png]] ![[also.png]] ![[vanished.pdf]]",
            "attachments",
            &disk,
        );
        assert_eq!(
            plan.attachments,
            vec!["attachments/here.png", "attachments/also.png"]
        );
        assert_eq!(plan.missing, vec!["gone.png", "vanished.pdf"]);
    }

    /// The attachments folder is a parameter, not a constant: a vault
    /// configured with a different one must resolve there and only there.
    #[test]
    fn the_attachments_folder_is_the_one_the_vault_configured() {
        let disk = vault(&["media/a.png", "attachments/a.png"]);
        let plan = plan("![[a.png]]", "media", &disk);
        assert_eq!(plan.attachments, vec!["media/a.png"]);
    }

    /// Three survivors of one sweep, in one test, because they are one
    /// promise: **an embed is looked at once however many times, and however
    /// many ways, the note names it.**
    ///
    /// Nothing here was covered before. Every duplicate test in this file used
    /// embeds that RESOLVE, and the resolved list deduplicates itself — so the
    /// `seen` set, its case folding, and the second guard beside the push were
    /// all invisible. The two lists that push unconditionally are `missing` and
    /// `notes`, and a receipt naming `gone.png` three times is a receipt
    /// nobody reads twice.
    #[test]
    fn one_embed_named_several_ways_is_counted_once_in_every_list() {
        let disk = vault(&["attachments/photo.png"]);
        let body = [
            "![[gone.png]] ![[GONE.png]]",
            "![[Other Note]] ![[other note]]",
            "![[photo.png]] ![[attachments/photo.png]]",
        ]
        .join("\n");
        let plan = plan(&body, "attachments", &disk);
        // Folded, like every other duplicate rule in this codebase: the vault's
        // home is case-insensitive, so `GONE.png` is the same absent file.
        assert_eq!(plan.missing, vec!["gone.png"]);
        assert_eq!(plan.notes, vec!["Other Note"]);
        // And two DIFFERENT targets that resolve to one path are carried once.
        // This is the guard beside the push, and the `seen` set cannot cover
        // it: `seen` folds the target, and `photo.png` and
        // `attachments/photo.png` are two targets — they only become one file
        // after resolution.
        assert_eq!(plan.attachments, vec!["attachments/photo.png"]);
    }

    /// Whether the two defensive lines at the top of the loop have any input
    /// that reaches them. `links::extract` already trims both link grammars and
    /// already drops an anchor-only wikilink, so a padded target and an empty
    /// one are the two shapes that would prove they are not dead code.
    #[test]
    fn a_padded_or_empty_embed_target_is_handled_by_the_one_link_grammar() {
        let disk = vault(&["photo.png"]);
        // Padded: `extract` trims, so this resolves rather than looking for
        // " photo.png ".
        let padded = plan("![[  photo.png  ]]", "attachments", &disk);
        assert_eq!(padded.attachments, vec!["photo.png"]);
        // Empty: neither grammar produces a link with an empty target, so
        // nothing reaches the plan at all.
        let empty = plan("![[]]\n![[ ]]\n![alt]()", "attachments", &disk);
        assert_eq!(empty, NoteExportPlan::default());
    }

    #[test]
    fn a_note_with_no_embeds_carries_nothing() {
        let disk = vault(&["attachments/photo.png"]);
        let plan = plan("# Title\n\nJust words.", "attachments", &disk);
        assert_eq!(plan, NoteExportPlan::default());
    }
}
