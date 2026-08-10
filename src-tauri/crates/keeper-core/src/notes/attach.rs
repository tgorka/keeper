//! What a note already holds, and where a file has to live to be in one
//! (Story 45.13, FR-188, FR-189).
//!
//! Story 45.13 gives attachment insertion three entry points — a file picked
//! off the drive, a multiselection in the Files pane, and the panel Story 43.7
//! built — and one result. Two rules have to be the same at all three, and this
//! module is where the half that Rust can answer lives:
//!
//! 1. **Is this file already in that note?** A surface that silently does
//!    nothing when the answer is yes is a failure this epic has shipped before;
//!    a surface that writes the embed twice is worse. So the question is asked
//!    before the write, and it is asked the same way whether the note is open
//!    in the editor (where the buffer is the truth and the webview answers) or
//!    closed on disk (where only this process can read it).
//! 2. **Where does the file have to be for a note to name it?** FR-145 forbids
//!    an absolute path in a synced artefact, so a note can only ever name a
//!    vault-relative path. A source already inside the vault is named where it
//!    is; a source outside it has to be copied in first, and that copy is the
//!    shell's job — deciding *which* case a path is is this one's.
//!
//! # The one duplicate rule, and why it is by name
//!
//! [`embedded_attachment_names`] answers with file names, not paths, which is
//! the join key Story 43.7 chose and for the reason it gave: Story 40.4 renames
//! a session folder after its note is written, so `![[old/screen.mov]]` and
//! `![[new/screen.mov]]` are one file shown twice — a duplicate by the only
//! definition a reader can see. `recording-embed.ts` resolves by name too, so
//! "already there" and "this is that file" cannot come apart.
//!
//! # Both embed spellings count as holding the file
//!
//! `![[photo.png]]` is what keeper writes. `![alt](attachments/photo.png)` is
//! ordinary CommonMark, it is what Obsidian writes for a markdown-style embed,
//! and until this story it was what keeper's own dead `attachment_markdown`
//! produced. A note that already shows the photograph shows it whichever
//! spelling put it there, so both are "already there". Only the first is ever
//! *written* from here on — see the story spec for which spelling won.
//!
//! A link is not an embed. `[[photo.png]]` mentions the file; `!` is the whole
//! of the difference, and a mention is not a copy of the picture.
//!
//! # No second link grammar
//!
//! Every shape above is read by [`crate::notes::links::extract`], which is the
//! one place this codebase knows what a link looks like. That is not tidiness:
//! `extract` skips fenced and inline code, drops `#heading` anchors,
//! percent-decodes a markdown destination and refuses an external URL. A
//! private scanner here would get one of those wrong, and the note the chooser
//! offers would disagree with the panel that refuses.
//!
//! `src/lib/notes/attach.ts` is a second implementation of
//! [`embedded_attachment_names`] in TypeScript, needed because the open
//! editor's buffer exists only in the webview and never reaches this process.
//! It is pinned to this one by `attach-vectors.json`, which both test suites
//! load — see the test below.

use std::collections::BTreeSet;
use std::path::{Component, Path};

use crate::notes::links;

/// The last `/`-separated component of a path, which is the file's own name.
///
/// `/`-only, deliberately: every path this sees is vault-relative or has
/// already been through [`vault_relative`], and both are `/`-joined regardless
/// of the platform's separator. Splitting on `\` as well would turn a
/// backslash in a *filename* — legal on macOS — into a separator.
pub fn attachment_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// The file names this body already embeds, in either embed spelling, folded
/// to lower case.
///
/// **Folded, like [`crate::notes::index::link_key`] and for its reason.** The
/// vault's home is APFS, which is case-insensitive by default, so `Photo.PNG`
/// and `photo.png` are one file on the machine that wrote the note — and the
/// vault syncs to filesystems where they would not be. Under-reporting is the
/// worse error of the two: it writes the picture into the note twice, which is
/// the exact failure this story exists to refuse, while over-reporting says
/// "already there" about a file the person can see is already there.
///
/// Sorted and deduplicated by the `BTreeSet`, because callers compare and
/// report these rather than rendering them in document order.
pub fn embedded_attachment_names(body: &str) -> BTreeSet<String> {
    links::extract(body)
        .into_iter()
        .filter(|link| link.embed)
        .map(|link| attachment_name(&link.target).to_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

/// Of `names`, the ones `body` already embeds — in the order asked, spelled as
/// asked.
///
/// The caller's order rather than sorted: the sentence a surface shows names
/// the files back in the order the person selected them, and a list that
/// reordered itself would read as being about something else. The caller's
/// spelling rather than the note's, because the name a person recognises is
/// the one on the file they just picked.
pub fn already_attached(body: &str, names: &[String]) -> Vec<String> {
    let held = embedded_attachment_names(body);
    let mut seen = BTreeSet::new();
    names
        .iter()
        .filter(|name| {
            let folded = name.to_lowercase();
            held.contains(&folded) && seen.insert(folded)
        })
        .cloned()
        .collect()
}

/// The vault-relative path of `source`, or `None` when it is not inside the
/// vault at all.
///
/// **Both paths must already be canonical.** This does no IO and resolves no
/// symlink: it is the pure half of "is this file in the vault", and the shell
/// canonicalises before calling so that a symlink pointing out of the vault
/// cannot answer yes. Given a non-canonical pair the answer is a best-effort
/// prefix test, which is why the shell — not this — is the containment check of
/// record.
///
/// A source that IS the vault root answers `None`. The root is a directory, a
/// directory is not an attachment, and `""` is not a path a note can name.
///
/// Anything but a plain name in the remainder — a `..`, a bare `/`, a Windows
/// prefix — answers `None` rather than being flattened. A traversal that
/// survived as text would end up inside `![[…]]` in a synced file.
pub fn vault_relative(vault_root: &Path, source: &Path) -> Option<String> {
    let rest = source.strip_prefix(vault_root).ok()?;
    let mut segments = Vec::new();
    for component in rest.components() {
        match component {
            Component::Normal(part) => segments.push(part.to_str()?.to_owned()),
            _ => return None,
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vector table shared with the TypeScript mirror.
    ///
    /// `include_str!` rather than a runtime read, for the reason
    /// `keeper_core::size`'s does it that way: a deleted or renamed fixture has
    /// to be a build failure, not a test that quietly passes over no vectors.
    const VECTORS_JSON: &str = include_str!("attach-vectors.json");

    /// Every vector in `attach-vectors.json`, which `src/lib/notes/attach.ts`
    /// asserts against as well (Story 45.13).
    ///
    /// **This is the anti-drift mechanism.** The TypeScript module is a second
    /// implementation of [`embedded_attachment_names`], needed because the open
    /// editor's buffer never reaches this process. If the two disagree, the note
    /// chooser offers a note that the attachments panel then refuses to write
    /// into — precisely the "two answers to one question" this story exists to
    /// end. A mirror merely documented as a mirror drifts; a mirror pinned to a
    /// table both suites load fails on the commit that breaks it.
    #[test]
    fn every_shared_vector_matches_and_the_table_is_not_empty() {
        let parsed: serde_json::Value =
            serde_json::from_str(VECTORS_JSON).expect("the shared vector fixture parses");
        let vectors = parsed["vectors"]
            .as_array()
            .expect("the fixture carries a vectors array");
        assert!(
            vectors.len() >= 18,
            "the shared table has been truncated to {} vectors; it is the contract with \
             src/lib/notes/attach.ts and shrinking it silently weakens both suites",
            vectors.len()
        );
        for vector in vectors {
            let body = vector["body"].as_str().expect("body is a string");
            let expected: Vec<String> = vector["embedded"]
                .as_array()
                .expect("embedded is an array")
                .iter()
                .map(|name| name.as_str().expect("a name is a string").to_owned())
                .collect();
            let actual: Vec<String> = embedded_attachment_names(body).into_iter().collect();
            assert_eq!(
                actual,
                expected,
                "vector {:?} ({})",
                body,
                vector["why"].as_str().unwrap_or_default()
            );
        }
    }

    #[test]
    fn a_name_is_the_last_slash_separated_segment() {
        assert_eq!(attachment_name("a/b/c.png"), "c.png");
        assert_eq!(attachment_name("c.png"), "c.png");
        // A backslash is a legal character in a macOS filename, so it is part
        // of the name rather than a separator.
        assert_eq!(attachment_name(r"a/b\c.png"), r"b\c.png");
    }

    #[test]
    fn already_attached_keeps_the_callers_order_and_drops_repeats() {
        let body = "![[attachments/a.png]] and ![[deep/b.png]]\n";
        let asked = vec![
            "b.png".to_owned(),
            "c.png".to_owned(),
            "a.png".to_owned(),
            "b.png".to_owned(),
        ];
        assert_eq!(
            already_attached(body, &asked),
            vec!["b.png".to_owned(), "a.png".to_owned()],
            "the order asked, each name once, and only the ones the note holds"
        );
    }

    #[test]
    fn a_source_inside_the_vault_is_named_relative_to_it() {
        assert_eq!(
            vault_relative(Path::new("/v"), Path::new("/v/notes/a.png")).as_deref(),
            Some("notes/a.png")
        );
    }

    #[test]
    fn a_source_outside_the_vault_or_the_root_itself_is_not_vault_relative() {
        assert_eq!(
            vault_relative(Path::new("/v"), Path::new("/other/a.png")),
            None
        );
        assert_eq!(vault_relative(Path::new("/v"), Path::new("/v")), None);
        // A sibling whose name merely starts with the root's is not inside it.
        assert_eq!(
            vault_relative(Path::new("/v"), Path::new("/vault/a.png")),
            None
        );
    }

    #[test]
    fn a_traversal_in_the_remainder_is_refused_rather_than_flattened() {
        assert_eq!(
            vault_relative(Path::new("/v"), Path::new("/v/../v/a.png")),
            None,
            "a `..` that survived as text would be written into a synced file"
        );
    }
}
