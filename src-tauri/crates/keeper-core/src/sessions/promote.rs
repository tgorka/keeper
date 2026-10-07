//! The README's `## Promote` table, parsed and spliced (FR-243, FR-244, AD-108).
//!
//! The table IS the promotion contract — the zone's own README says so:
//! "promotion = copy under a stable name, listed here". This module reads the
//! documented shape and **preserves everything it does not understand**: an
//! unparseable row is carried verbatim as [`PromoteRow::Unreadable`], surfaced
//! in the panel, and never rewritten (PRD §8). Writes are span-splices over
//! the original bytes under the same discipline as frontmatter (NFR-39): a
//! row update touches its row, an append touches the table's end, and every
//! other byte of the README survives untouched.
//!
//! The documented shape, from `_template/README.md` on both live drives:
//!
//! ```markdown
//! ## Promote
//!
//! | workspace | → artifacts | note |
//! | --------- | ----------- | ---- |
//! | workspace/draft.md | artifacts/report.md | weekly report |
//! ```
//!
//! A target outside `artifacts/` is the drive's, drive-relative: a harvested
//! note promoted into the notes vault is recorded as
//! `| artifacts/knowledge/<…>.md | 10-notes/knowledge/<note>.md | knowledge | <digest> |`
//! (R138), its fourth cell the [`copy_digest`] of the copy that promotion
//! published there — written once the copy is durable, and the only thing
//! that lets a later promotion replace that copy or a review be written into
//! it (R244). A table's header names three columns; a fourth cell is past
//! what a markdown renderer shows. [`promote_panel`] renders the table with
//! each row's state for the promote panel.

use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::agents::knowledge;
use crate::notes::frontmatter::Frontmatter;
use crate::notes::okf;
use crate::sessions::model::{ARTIFACTS_DIR, WORKSPACE_DIR};
use crate::sessions::offer::{
    self, ArtifactOfferVm, ChoiceVm, DestinationVm, PanelIntentVm, ReadState, UnlistedVm,
    VaultCopyVm,
};
use crate::sessions::plan::sha256_hex;

/// One row of the promote table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromoteRow {
    /// A row in the documented three-column shape, or with the fourth cell
    /// a promotion out records.
    Entry {
        /// The `workspace/…` source, verbatim as written.
        source: String,
        /// The `artifacts/…` target, verbatim as written.
        target: String,
        /// The free-text note column, verbatim (may be empty).
        note: String,
        /// The [`copy_digest`] of the copy this source published at `target`
        /// — lowercase hex SHA-256 — or `None` when the row records none:
        /// a promotion into `artifacts/`, or one out whose copy has not
        /// landed.
        published: Option<String>,
    },
    /// A pipe-delimited line the parser could not read as three columns.
    /// Preserved so the panel can show it and a rewrite can never eat it.
    Unreadable {
        /// The raw line, byte-for-byte.
        raw: String,
        /// 0-based line number in the README, for the panel's located note.
        line: usize,
    },
}

/// The parsed table: rows plus the byte spans a splice needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoteTable {
    /// Data rows, in file order. Header and delimiter rows are structure, not
    /// data, and are not carried here.
    pub rows: Vec<PromoteRow>,
    /// Byte offset just past the last row (or past the delimiter when the
    /// table is empty) — where an appended row is spliced in.
    pub append_at: usize,
    /// Byte span `(start, end)` of each data row's line INCLUDING its
    /// terminator, parallel to `rows` — what a row update replaces.
    pub row_spans: Vec<(usize, usize)>,
}

/// Find and parse the `## Promote` table in a README body. `None` when the
/// section or its table is absent — which the panel reports as "no promote
/// table", never invents (files are truth).
pub fn parse(body: &str) -> Option<PromoteTable> {
    let section_start = heading_offset(body, "## Promote")?;
    let after_heading = &body[section_start..];

    // Walk lines after the heading until the next `## ` heading or EOF,
    // looking for the table: a header row, a delimiter row, then data rows.
    let mut offset = section_start;
    let mut lines = after_heading.split_inclusive('\n');
    lines.next().map(|l| offset += l.len())?; // consume the heading line

    let mut rows = Vec::new();
    let mut row_spans = Vec::new();
    let mut append_at = None;
    let mut header_seen = false;
    let first_line_no = body[..offset].matches('\n').count();

    for (line_no, line) in (first_line_no + 1..).zip(lines) {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim_end_matches(['\n', '\r']).trim();
        if trimmed.starts_with("## ") {
            break;
        }
        if !trimmed.starts_with('|') {
            // Prose between the heading and the table (the template carries an
            // HTML comment there) — or the blank line after the table, which
            // ends it once rows have been seen.
            if append_at.is_some() && trimmed.is_empty() {
                break;
            }
            continue;
        }
        if !header_seen {
            header_seen = true; // the `| workspace | → artifacts | note |` row
            continue;
        }
        if append_at.is_none() && is_delimiter_row(trimmed) {
            append_at = Some(offset);
            continue;
        }
        // A data row.
        append_at = Some(offset);
        match split_row(trimmed) {
            Some((source, target, note, published)) => {
                rows.push(PromoteRow::Entry {
                    source,
                    target,
                    note,
                    published,
                });
            }
            None => rows.push(PromoteRow::Unreadable {
                raw: trimmed.to_owned(),
                line: line_no,
            }),
        }
        row_spans.push((start, offset));
    }

    append_at.map(|append_at| PromoteTable {
        rows,
        append_at,
        row_spans,
    })
}

/// Render one data row in the canonical spelling the writer uses: three
/// cells, and the fourth when it records a published copy.
pub fn render_row(source: &str, target: &str, note: &str, published: Option<&str>) -> String {
    match published {
        Some(digest) => format!("| {source} | {target} | {note} | {digest} |\n"),
        None => format!("| {source} | {target} | {note} |\n"),
    }
}

/// Why a row is not written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RowRefusal {
    #[error("this session's README has no ## Promote table, so nothing records a promotion; add the section first.")]
    NoTable,
    #[error("`{cell}` cannot be a cell of the ## Promote table — a `|`, a line break or padding would make the row read back as something else — so nothing was promoted.")]
    Unrepresentable { cell: String },
}

/// Whether `cell` reads back from [`render_row`]'s row as itself.
fn representable(cell: &str) -> bool {
    !cell.contains('|') && !cell.chars().any(char::is_control) && cell.trim() == cell
}

/// The body with one row appended to the table (or updated in place when a
/// row with the same source already exists). Everything outside the touched
/// span is byte-identical (NFR-39). Refused when the body has no table to
/// write into — the caller decides whether to create the section, and that
/// is a different, louder act — and when a cell would not read back as
/// written: a row is only written in a form [`parse`] reads as these three
/// cells. A row it updates keeps the copy it records as published while it
/// names the same target, and records none for another.
pub fn upsert_row(
    body: &str,
    source: &str,
    target: &str,
    note: &str,
) -> Result<String, RowRefusal> {
    splice_row(body, source, target, note, None)
}

/// [`upsert_row`], recording `digest` ([`copy_digest`]) as the copy this
/// source has now published at `target`: written only once that copy is
/// durable.
pub fn upsert_published_row(
    body: &str,
    source: &str,
    target: &str,
    note: &str,
    digest: &str,
) -> Result<String, RowRefusal> {
    splice_row(body, source, target, note, Some(digest))
}

fn splice_row(
    body: &str,
    source: &str,
    target: &str,
    note: &str,
    published: Option<&str>,
) -> Result<String, RowRefusal> {
    for (cell, may_be_empty) in [(source, false), (target, false), (note, true)] {
        if !representable(cell) || (cell.is_empty() && !may_be_empty) {
            return Err(RowRefusal::Unrepresentable {
                cell: cell.escape_debug().to_string(),
            });
        }
    }
    if let Some(digest) = published.filter(|digest| !is_digest(digest)) {
        return Err(RowRefusal::Unrepresentable {
            cell: digest.escape_debug().to_string(),
        });
    }
    let table = parse(body).ok_or(RowRefusal::NoTable)?;
    for (row, span) in table.rows.iter().zip(&table.row_spans) {
        if let PromoteRow::Entry {
            source: s,
            target: t,
            published: kept,
            ..
        } = row
        {
            if s == source {
                let published = published.or_else(|| kept.as_deref().filter(|_| t == target));
                let rendered = render_row(source, target, note, published);
                let mut out = String::with_capacity(body.len() + rendered.len());
                out.push_str(&body[..span.0]);
                out.push_str(&rendered);
                out.push_str(&body[span.1..]);
                return Ok(out);
            }
        }
    }
    let rendered = render_row(source, target, note, published);
    let mut out = String::with_capacity(body.len() + rendered.len() + 1);
    out.push_str(&body[..table.append_at]);
    // A table that ends the file without a line break: the row goes on a
    // line of its own, or it would read back as part of the last one.
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&rendered);
    out.push_str(&body[table.append_at..]);
    Ok(out)
}

/// Whether `cell` is a digest as [`copy_digest`] spells one.
fn is_digest(cell: &str) -> bool {
    cell.len() == 64 && cell.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether a target cell names a file of the session — `artifacts/…`,
/// session-relative — rather than a file of the drive the session's work
/// was promoted out into, drive-relative (R138): `10-notes/knowledge/x.md`.
/// One row per source either way.
pub fn target_in_session(target: &str) -> bool {
    target.split('/').next() == Some(ARTIFACTS_DIR)
}

/// What the panel knows of one file it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFact {
    /// When what it says last changed, ms since the epoch: how "newer" is
    /// told. The session runtime gives a synced file's commit time — of the
    /// oldest commit of the run that still says what it says now, review
    /// keys aside — and the mtime of a `workspace/` file or of a change not
    /// committed yet, never a checkout's mtime. `None` when no content fact
    /// tells: the run's start lies past the history read, or a vault copy a
    /// review may have touched has no commit saying what it says.
    pub changed_ms: Option<i64>,
    /// What "differs" compares ([`digest_of`]).
    pub digest: String,
}

/// How much of a file's head [`digest_of`] holds to read its frontmatter.
pub const HEAD_BYTES: u64 = 64 * 1024;

/// The review keys a comparison leaves out.
const REVIEW_KEYS: [&str; 3] = ["verified", "verified_by", knowledge::HUMAN_REVIEWED];

/// What "differs" compares for the file `rel`, streamed from `reader`. A
/// markdown file whose frontmatter ends within its first [`HEAD_BYTES`] is
/// compared without its review keys (`verified`, `verified_by`,
/// `human_reviewed`): a person's tick lands in the vault's copy only (R139),
/// and a review is not a change of what the note says, so it never makes the
/// candidate look stale. Anything else is compared byte for byte. Only the
/// head is ever held in memory. Whether a file is the copy a promotion
/// published is [`copy_digest`]'s, never this.
///
/// # Errors
/// The reader's.
pub fn digest_of(rel: &str, mut reader: impl Read) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};

    let mut head = Vec::new();
    (&mut reader).take(HEAD_BYTES).read_to_end(&mut head)?;
    let name = rel.rsplit('/').next().unwrap_or(rel).to_ascii_lowercase();
    let markdown = name.ends_with(".md") || name.ends_with(".markdown");
    let text = match std::str::from_utf8(&head) {
        Ok(text) => text,
        Err(error) => std::str::from_utf8(&head[..error.valid_up_to()]).unwrap_or(""),
    };
    let (fm, body_offset) = if markdown {
        Frontmatter::parse(text)
    } else {
        (Frontmatter::default(), 0)
    };
    let mut hasher = Sha256::new();
    if fm.has_block() {
        hasher.update(without_reviews(&text[..body_offset]).as_bytes());
        hasher.update(&head[body_offset..]);
    } else {
        hasher.update(&head);
    }
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// A frontmatter `block` with every review key's lines taken out, as an
/// untick takes them out — the whole block when nothing else is left in it.
fn without_reviews(block: &str) -> String {
    let mut block = block.to_owned();
    for key in REVIEW_KEYS {
        block = Frontmatter::remove_all_in(&block, key);
    }
    block
}

/// The panel's fact of the file `rel` read from `reader`, whose content
/// last changed at `changed_ms` ([`FileFact::changed_ms`]).
///
/// # Errors
/// The reader's.
pub fn fact_of(rel: &str, reader: impl Read, changed_ms: Option<i64>) -> std::io::Result<FileFact> {
    Ok(FileFact {
        changed_ms,
        digest: digest_of(rel, reader)?,
    })
}

/// The digest a row records for the copy its `source` published, `bytes`
/// byte for byte, and that a file at the row's target must have to be that
/// copy (R244). The copy of a harvested note — whatever its target is named,
/// however large its frontmatter — is told apart from every other text by its
/// frontmatter's byte order mark, what lies between its fences once the
/// review keys' lines are out (nothing, when only whitespace is left), and
/// its body: each framed by its length, so where the frontmatter ends is part
/// of what is compared, and a review or its untick — the only change
/// [`knowledge::review`] makes — never changes it. Any other copy, and one
/// that is not UTF-8, is digested byte for byte.
pub fn copy_digest(source: &str, bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    match std::str::from_utf8(bytes) {
        Ok(text) if knowledge::is_note(source) => {
            let bom = crate::notes::bom_len(text);
            let (fm, body_offset) = Frontmatter::parse(text);
            let (inner, body) = if fm.has_block() {
                let kept = without_reviews(&text[..body_offset]);
                let inner = Frontmatter::parse(&kept).0.inner_text().to_owned();
                (inner, &text[body_offset..])
            } else {
                (String::new(), &text[bom..])
            };
            let inner = if inner.trim().is_empty() { "" } else { &inner };
            hasher.update(b"keeper note copy 1\0");
            for part in [&text[..bom], inner] {
                hasher.update((part.len() as u64).to_le_bytes());
                hasher.update(part.as_bytes());
            }
            hasher.update(body.as_bytes());
        }
        _ => {
            hasher.update(b"keeper copy 1\0");
            hasher.update(bytes);
        }
    }
    hex::encode(hasher.finalize())
}

/// What the panel knows of the file at a harvested note's row's target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyFact {
    /// Its [`copy_digest`] as the note's copy.
    pub digest: String,
    /// Every person whose review it carries, as its OKF `verified` names
    /// them, in the file's order.
    pub reviewers: Vec<String>,
}

/// The [`CopyFact`] of `bytes`, the file at the target of `source`'s row.
pub fn copy_fact(source: &str, bytes: &[u8]) -> CopyFact {
    let text = String::from_utf8_lossy(bytes);
    CopyFact {
        digest: copy_digest(source, bytes),
        reviewers: okf::read(&Frontmatter::parse(&text).0)
            .verified
            .into_iter()
            .filter(|entry| entry.actor_kind() == okf::ActorKind::Person)
            .map(|entry| entry.by)
            .collect(),
    }
}

/// Why the file at a row's target is not the copy its source published,
/// so nothing of the note replaces it or is written into it (R244, R252).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyLoss {
    /// The row records no publication: written before keeper recorded one,
    /// or one whose copy never finished landing.
    Unrecorded,
    /// The file is no longer the copy the row records: edited since — a
    /// person's own edit included — or another file put there.
    Changed,
}

impl CopyLoss {
    /// What a refusal, and the panel, say of it: why, and what the person
    /// can do that keeps the file as it is.
    pub fn explain(self, source: &str, target: &str) -> String {
        let why = match self {
            CopyLoss::Unrecorded => format!(
                "{target} is not known to be the copy {source} published: its row records no publication — written before keeper recorded one, or one that did not finish"
            ),
            CopyLoss::Changed => format!(
                "{target} was changed since {source} published it there — edited, or replaced by another file — so it is no longer that copy"
            ),
        };
        format!(
            "{why}. keeper neither replaces it nor writes a review of the note into it, and promoting again does not make it the note's copy. The file stays as it is, every edit kept; to publish the note, promote it under another name, or first move that file elsewhere in the vault, keeping it."
        )
    }
}

/// Whether `there`, the file at the target of the row recording `source`
/// with the publication `published` ([`PromoteRow::Entry::published`]), is
/// the copy that row records — the session's synced row and the synced file
/// agreeing — or why not. Only that lets a promotion replace the file or a
/// review be written into it: never identical bytes, a row alone, or
/// anything a device keeps (R244).
///
/// # Errors
/// [`CopyLoss`].
pub fn standing(source: &str, published: Option<&str>, there: &[u8]) -> Result<(), CopyLoss> {
    match published {
        None => Err(CopyLoss::Unrecorded),
        Some(digest) if copy_digest(source, there) == digest => Ok(()),
        Some(_) => Err(CopyLoss::Changed),
    }
}

/// The row of `table` that records `source` — the first naming it, the one
/// [`upsert_row`] updates — as its index, target and published copy. Every
/// reader of a source's row takes all three from this one row, so a second
/// row naming the source (a hand edit, a merge) never lends its target or
/// its publication to the first.
pub fn entry_of<'a>(
    table: &'a PromoteTable,
    source: &str,
) -> Option<(usize, &'a str, Option<&'a str>)> {
    table
        .rows
        .iter()
        .enumerate()
        .find_map(|(at, row)| match row {
            PromoteRow::Entry {
                source: s,
                target,
                published,
                ..
            } if s == source => Some((at, target.as_str(), published.as_deref())),
            _ => None,
        })
}

/// One harvested note as the session runtime found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeFile {
    /// Session-relative.
    pub path: String,
    /// Its size on the disk.
    pub bytes: u64,
    /// Its whole text, or why the panel does not hold it: it could not be
    /// read, or it is larger than a knowledge note holds.
    pub text: Result<String, String>,
}

/// Everything the panel is composed from, read by the session runtime.
#[derive(Debug, Clone, Default)]
pub struct PanelFacts {
    /// Every file a row names that is there, by the cell's spelling —
    /// session-relative for a source and an `artifacts/` target,
    /// drive-relative for a target out of the session — with its fact, or
    /// why it could not be read. A file that is not there is not here.
    pub files: BTreeMap<String, Result<FileFact, String>>,
    /// Every entry under `workspace/` but a folder, session-relative —
    /// hidden ones, links and special files too: what an archive's emptying
    /// removes, each a choice of the checklist.
    pub workspace: Vec<String>,
    /// Every entry under `workspace/`, folders too, with its stamp
    /// ([`offer::is_regular`]) that a choice about it and the archive
    /// checklist are bound to ([`offer::snapshot_revision`]).
    pub stamps: BTreeMap<String, String>,
    /// Every note under `artifacts/knowledge/`.
    pub knowledge: Vec<KnowledgeFile>,
    /// For each harvested note whose row records a publication out of the
    /// session at a file that is there, by the note's path: that file's
    /// [`CopyFact`], or why it was not read.
    pub copies: BTreeMap<String, Result<CopyFact, String>>,
    /// What the runtime could not see: a folder that would not list, a
    /// listing cut at its cap.
    pub problems: Vec<String>,
}

/// One row's state (FR-244, UX-DR90).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum PromoteState {
    /// The target holds what the source holds, or newer.
    Ok,
    /// The source differs from the target and is newer: promote again.
    Stale,
    /// The source is gone — cleaned up after promotion, as a workspace is
    /// at archive. Quiet: the target is what was kept.
    MissingSource,
    /// The target the table promises is not there. Loud.
    MissingTarget,
    /// A line of the table keeper cannot read, shown as written.
    Unreadable,
    /// A file the row names is there but could not be read, or which of
    /// two differing files changed last is not known, so its state is not
    /// known; the row's `problem` says why.
    Unknown,
}

/// One row of the panel: one row of the README's table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PromoteRowVm {
    pub state: PromoteState,
    /// The source cell, session-relative; empty for an unreadable line.
    pub source: String,
    /// The target cell: session-relative under `artifacts/`, else
    /// drive-relative ([`target_in_session`]); empty for an unreadable line.
    pub target: String,
    pub note: String,
    /// Whether the target lies out of the session, in the drive.
    pub out: bool,
    /// An unreadable line, verbatim.
    pub raw: Option<String>,
    /// An unreadable line's 0-based line in the README.
    pub line: Option<u32>,
    /// Why an `unknown` row's file could not be read.
    pub problem: Option<String>,
    /// What a choice about this row is bound to ([`offer::row_revision`]).
    pub revision: String,
    /// Why this row offers no promotion into the session, or `None`
    /// ([`offer::refused_in`]).
    pub refused: Option<String>,
    /// The person's choice about this row, while it still holds
    /// ([`offer::decide`]).
    pub choice: Option<ChoiceVm>,
}

/// One harvested note of the session (UX-DR137).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeNoteVm {
    /// Session-relative: `artifacts/knowledge/<slug>/<note>.md`.
    pub path: String,
    /// The OKF title, when it has one.
    pub title: Option<String>,
    /// The agent and host its `generated` names (`agent:<agent>@<host>`),
    /// when it names one.
    pub agent: Option<String>,
    pub host: Option<String>,
    /// Its size, every byte of which the panel shows (S-32).
    #[ts(type = "number")]
    pub bytes: u64,
    /// The lowercase hex SHA-256 of its bytes as read for this panel: what
    /// a promotion of it names as the version the person read. `None`
    /// when it was not read whole (`problem`).
    pub revision: Option<String>,
    /// Where it was promoted to, drive-relative, by its row.
    pub promoted_to: Option<String>,
    /// That row's state: `stale` when the note changed here after it was
    /// promoted.
    pub state: Option<PromoteState>,
    /// The last person whose review the promoted copy carries.
    pub reviewed_by: Option<String>,
    /// Whether the person this keeper records reviews as is among those
    /// the promoted copy's reviews name.
    pub reviewed_by_me: bool,
    /// Why its text is not shown: it could not be read, or it is larger
    /// than a knowledge note holds.
    pub problem: Option<String>,
    /// Why the file at `promoted_to` is not taken as the copy this note
    /// published — its row records no publication, or the file changed
    /// since, a person's own edit included (R252) — so no review in it is
    /// shown and keeper neither replaces it nor writes a review into it;
    /// with what the person can do instead ([`CopyLoss::explain`]). Or why
    /// that could not be told. `None` when it is the copy, or nothing is
    /// there.
    pub foreign_copy: Option<String>,
    /// The vault copy its row names, when it was promoted out.
    pub copy: Option<VaultCopyVm>,
    /// Where promoting it goes — the vault for a note not promoted yet,
    /// its row's target to repair a missing copy or to publish a newer
    /// candidate — or `None` when there is nothing to promote or it may
    /// not be.
    pub destination: Option<DestinationVm>,
    /// Why this note may not be promoted, when it is the note's own reason.
    pub unavailable: Option<String>,
    /// Whether the candidate the person read is the version shown
    /// ([`offer::decide`]).
    pub candidate_read: ReadState,
    /// Whether the vault copy the person read is the version shown.
    pub copy_read: ReadState,
    /// Whether the person said they reviewed the candidate version they
    /// read and that is shown: what a promotion of it needs.
    pub consented: bool,
}

/// The promote panel of one session (FR-243, FR-244, UX-DR90, UX-DR137).
/// The README's table is the truth the panel renders and never owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionPromoteVm {
    /// Whether the README has a `## Promote` table at all.
    pub has_table: bool,
    pub rows: Vec<PromoteRowVm>,
    /// `workspace/` files no row names: promotable.
    pub unlisted: Vec<UnlistedVm>,
    pub knowledge: Vec<KnowledgeNoteVm>,
    /// The session's other artifacts, each with what promoting it out
    /// offers.
    pub artifacts: Vec<ArtifactOfferVm>,
    /// The session's label chip, for an agent's session.
    pub label: Option<crate::agents::label::LabelVm>,
    /// The drive's notes vault, drive-relative: where a note may be
    /// promoted out to. `None` when the drive holds none.
    pub vault: Option<String>,
    /// Why nothing of this session may be promoted into the drive's vault —
    /// its label keeps it from some of the drive's readers, or who reads the
    /// session or the drive cannot be established — or `None`.
    pub out_refused: Option<String>,
    /// What the panel could not see, said rather than left out: a folder
    /// that would not list, a listing cut at its cap.
    pub problems: Vec<String>,
    /// What the archive checklist is bound to ([`offer::snapshot_revision`]).
    pub revision: String,
    /// What the person did in the panel, as much of it as still holds: the
    /// panel forwards it back with its next read ([`offer::decide`]).
    pub intent: PanelIntentVm,
    /// Every row and unlisted file has a choice: the checklist may archive.
    pub complete: bool,
}

/// The panel for the README `readme` over `facts`: each row with its
/// state, the unlisted workspace files and the harvested notes, each note
/// marked reviewed by `me` (`human:<localpart>`) when its copy says so —
/// only a copy its row records as the one it published ([`standing`]),
/// that row's target and publication read together ([`entry_of`]): a file
/// someone else put at the row's target, or one edited since, carries no
/// review of the note, and the note says why ([`KnowledgeNoteVm::foreign_copy`])
/// (R244, R252). The label, the vault and whether a promotion out is
/// refused are the caller's.
pub fn promote_panel(readme: &str, facts: &PanelFacts, me: Option<&str>) -> SessionPromoteVm {
    let table = parse(readme);
    let mut rows: Vec<PromoteRowVm> = table
        .as_ref()
        .map(|table| table.rows.iter().map(|row| row_vm(row, facts)).collect())
        .unwrap_or_default();
    // A line written twice is two items of the checklist, each with a
    // choice of its own: every repeat after the first is told apart by
    // where it falls among them, and offers no promotion of its own.
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for row in &mut rows {
        let earlier = seen.entry(row.revision.clone()).or_insert(0);
        if *earlier > 0 {
            row.revision = offer::occurrence(&row.revision, *earlier);
            row.refused
                .get_or_insert_with(|| offer::repeated(&row.source, &row.target));
        }
        *earlier += 1;
    }
    let listed = |path: &str| {
        rows.iter()
            .any(|row| row.raw.is_none() && row.source == path)
    };
    let unlisted = facts
        .workspace
        .iter()
        // The emptying keeps `workspace/.gitkeep`, and only that one.
        .filter(|path| path.strip_prefix(WORKSPACE_DIR) != Some("/.gitkeep") && !listed(path))
        .map(|path| {
            let stamp = facts.stamps.get(path).map(String::as_str);
            UnlistedVm {
                source: path.clone(),
                suggested: offer::suggested_artifact(path),
                revision: offer::row_revision(path, "", "", stamp, None),
                refused: offer::refused_unlisted(path, stamp)
                    .or_else(|| table.is_none().then(|| RowRefusal::NoTable.to_string())),
                choice: None,
            }
        })
        .collect();
    let knowledge = facts
        .knowledge
        .iter()
        .map(|file| {
            let doc = file
                .text
                .as_ref()
                .ok()
                .map(|text| okf::read(&Frontmatter::parse(text).0));
            let signer = doc
                .as_ref()
                .and_then(|doc| doc.generated.as_ref())
                .and_then(|generated| knowledge::signer_of(&generated.by));
            let entry = table.as_ref().and_then(|table| entry_of(table, &file.path));
            let row = entry.map(|(at, ..)| &rows[at]);
            let (reviewers, foreign_copy): (&[String], _) = match entry {
                Some((_, target, published))
                    if !target_in_session(target)
                        && facts.files.get(target).is_some_and(Result::is_ok) =>
                {
                    match (published, facts.copies.get(&file.path)) {
                        (None, _) => (&[], Some(CopyLoss::Unrecorded.explain(&file.path, target))),
                        (Some(digest), Some(Ok(copy))) if copy.digest == digest => {
                            (&copy.reviewers, None)
                        }
                        (Some(_), Some(Ok(_))) => {
                            (&[], Some(CopyLoss::Changed.explain(&file.path, target)))
                        }
                        (Some(_), Some(Err(why))) => (&[], Some(why.clone())),
                        (Some(_), None) => (&[], None),
                    }
                }
                _ => (&[], None),
            };
            KnowledgeNoteVm {
                path: file.path.clone(),
                title: doc.as_ref().and_then(|doc| doc.title.clone()),
                agent: signer.map(|(agent, _)| agent.to_owned()),
                host: signer.map(|(_, host)| host.to_owned()),
                bytes: file.bytes,
                revision: file.text.as_ref().ok().map(|text| sha256_hex(text)),
                promoted_to: row.map(|row| row.target.clone()),
                state: row.map(|row| row.state),
                reviewed_by: reviewers.last().cloned(),
                reviewed_by_me: me.is_some_and(|me| reviewers.iter().any(|by| by == me)),
                problem: file.text.as_ref().err().cloned(),
                foreign_copy,
                copy: None,
                destination: None,
                unavailable: None,
                candidate_read: ReadState::Unread,
                copy_read: ReadState::Unread,
                consented: false,
            }
        })
        .collect();
    let targets = table
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| match row {
            PromoteRow::Entry { target, .. } => {
                Some((target.clone(), offer::target_fact(facts.files.get(target))))
            }
            PromoteRow::Unreadable { .. } => None,
        })
        .collect();
    SessionPromoteVm {
        has_table: table.is_some(),
        rows,
        unlisted,
        knowledge,
        artifacts: Vec::new(),
        label: None,
        vault: None,
        out_refused: None,
        problems: facts.problems.clone(),
        revision: offer::snapshot_revision(readme, &facts.stamps, &targets),
        intent: PanelIntentVm::default(),
        complete: false,
    }
}

fn row_vm(row: &PromoteRow, facts: &PanelFacts) -> PromoteRowVm {
    let mut vm = match row {
        PromoteRow::Entry {
            source,
            target,
            note,
            ..
        } => {
            let (state, problem) = match (facts.files.get(source), facts.files.get(target)) {
                (Some(Err(why)), _) | (_, Some(Err(why))) => {
                    (PromoteState::Unknown, Some(why.clone()))
                }
                (_, None) => (PromoteState::MissingTarget, None),
                (None, Some(_)) => (PromoteState::MissingSource, None),
                (Some(Ok(from)), Some(Ok(to))) if from.digest == to.digest => {
                    (PromoteState::Ok, None)
                }
                (Some(Ok(from)), Some(Ok(to))) => match (from.changed_ms, to.changed_ms) {
                    (Some(from), Some(to)) if from > to => (PromoteState::Stale, None),
                    (Some(_), Some(_)) => (PromoteState::Ok, None),
                    (from, _) => (
                        PromoteState::Unknown,
                        Some(format!(
                            "{source} and {target} differ, and when {} last changed is not known from its history, so which is newer is not known.",
                            if from.is_none() { source } else { target }
                        )),
                    ),
                },
            };
            PromoteRowVm {
                state,
                source: source.clone(),
                target: target.clone(),
                note: note.clone(),
                out: !target_in_session(target),
                raw: None,
                line: None,
                problem,
                revision: offer::row_revision(
                    source,
                    target,
                    note,
                    facts.stamps.get(source).map(String::as_str),
                    Some(&offer::target_fact(facts.files.get(target))),
                ),
                refused: None,
                choice: None,
            }
        }
        PromoteRow::Unreadable { raw, line } => PromoteRowVm {
            state: PromoteState::Unreadable,
            source: String::new(),
            target: String::new(),
            note: String::new(),
            out: false,
            raw: Some(raw.clone()),
            line: Some(u32::try_from(*line).unwrap_or(u32::MAX)),
            problem: None,
            revision: offer::row_revision("", "", raw, Some(&line.to_string()), None),
            refused: None,
            choice: None,
        },
    };
    vm.refused = offer::refused_in(&vm, facts.files.contains_key(&vm.source));
    vm
}

/// Byte offset of a `## ` heading line in a body, at a line start.
fn heading_offset(body: &str, heading: &str) -> Option<usize> {
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.trim() == heading {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// A `| --- | --- | --- |` separator, any dash count, optional colons.
fn is_delimiter_row(line: &str) -> bool {
    let inner = line.trim_matches('|');
    !inner.is_empty()
        && inner
            .split('|')
            .all(|cell| cell.trim().chars().all(|c| matches!(c, '-' | ':')))
        && inner.contains('-')
}

/// Split a `| a | b | c |` row into exactly three trimmed cells, or four
/// whose last is a published copy's digest ([`is_digest`]).
fn split_row(line: &str) -> Option<(String, String, String, Option<String>)> {
    let inner = line.strip_prefix('|')?.strip_suffix('|')?;
    let cells: Vec<&str> = inner.split('|').map(str::trim).collect();
    let published = match cells.len() {
        3 => None,
        4 if is_digest(cells[3]) => Some(cells[3].to_owned()),
        _ => return None,
    };
    Some((
        cells[0].to_owned(),
        cells[1].to_owned(),
        cells[2].to_owned(),
        published,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE_TAIL: &str = "\
# a session

## Log

### 2026-08-12 — began

## Promote

<!-- promotion notes -->

| workspace | → artifacts | note |
| --------- | ----------- | ---- |
";

    /// The template's own empty table parses to zero rows with a valid append
    /// point — the state every fresh session starts in.
    #[test]
    fn the_template_table_parses_empty() {
        let table = parse(TEMPLATE_TAIL).expect("the template has a table");
        assert!(table.rows.is_empty());
        assert_eq!(table.append_at, TEMPLATE_TAIL.len());
    }

    /// Rows parse in order; a malformed line is carried, located, and NOT
    /// dropped — the panel shows it, the writer never touches it (PRD §8).
    #[test]
    fn rows_parse_and_a_malformed_line_is_preserved_not_dropped() {
        let body = format!(
            "{TEMPLATE_TAIL}| workspace/a.md | artifacts/a.md | first |\n| broken row without cells\n| workspace/b.csv | artifacts/b.csv | |\n"
        );
        let table = parse(&body).expect("a table");
        assert_eq!(table.rows.len(), 3);
        assert_eq!(
            table.rows[0],
            PromoteRow::Entry {
                source: "workspace/a.md".into(),
                target: "artifacts/a.md".into(),
                note: "first".into(),
                published: None,
            }
        );
        assert!(matches!(&table.rows[1], PromoteRow::Unreadable { raw, .. }
            if raw == "| broken row without cells"));
        assert!(matches!(&table.rows[2], PromoteRow::Entry { note, .. } if note.is_empty()));
    }

    /// An upsert re-promotes under the same source in place; a new source
    /// appends. Every byte outside the touched span survives — asserted by
    /// reconstruction, not by trust (NFR-39).
    #[test]
    fn upsert_replaces_by_source_or_appends_and_touches_nothing_else() {
        let body = format!(
            "{TEMPLATE_TAIL}| workspace/a.md | artifacts/a.md | v1 |\n\n## After\n\ntext\n"
        );
        let updated = upsert_row(&body, "workspace/a.md", "artifacts/a.md", "v2").expect("upserts");
        assert!(updated.contains("| workspace/a.md | artifacts/a.md | v2 |\n"));
        assert!(!updated.contains("| v1 |"));
        assert!(
            updated.ends_with("## After\n\ntext\n"),
            "the tail is untouched"
        );

        let appended =
            upsert_row(&updated, "workspace/b.md", "artifacts/b.md", "").expect("appends");
        let a_at = appended.find("workspace/a.md").expect("a stays");
        let b_at = appended.find("workspace/b.md").expect("b lands");
        assert!(a_at < b_at, "appends go after existing rows");
    }

    /// A README with no Promote section refuses rather than inventing one —
    /// creating the section is a different, louder act the caller owns.
    #[test]
    fn a_body_without_a_table_is_a_none_not_a_scaffold() {
        assert_eq!(parse("# nothing here\n"), None);
        assert_eq!(
            upsert_row("# nothing\n", "workspace/x", "artifacts/x", ""),
            Err(RowRefusal::NoTable)
        );
    }

    /// R95K-14: a cell that would not read back as itself — a `|`, a line
    /// break, padding, an empty source or target — is refused before
    /// anything is written; any other note is written as a row that parses
    /// back to exactly its three cells, every byte around it kept.
    #[test]
    fn a_row_is_written_only_as_it_reads_back() {
        let tail = "\n## After\n\ntext\n";
        let body = format!("{TEMPLATE_TAIL}| workspace/a.md | artifacts/a.md | v1 |\n{tail}");
        for (source, target, note) in [
            ("workspace/a.md", "artifacts/a.md", "weekly | final"),
            ("workspace/a.md", "artifacts/a.md", "weekly\n## Heading"),
            ("workspace/a.md", "artifacts/a.md", "weekly\r"),
            ("workspace/a.md", "artifacts/a.md", " padded"),
            ("workspace/a|b.md", "artifacts/a.md", ""),
            ("workspace/a.md", "", ""),
        ] {
            assert!(
                matches!(
                    upsert_row(&body, source, target, note),
                    Err(RowRefusal::Unrepresentable { .. })
                ),
                "{source:?} {target:?} {note:?}"
            );
        }
        for note in ["weekly, final — ünïcode", "", "a: b # c"] {
            let updated = upsert_row(&body, "artifacts/k.md", "10-notes/k.md", note).expect("row");
            let table = parse(&updated).expect("table");
            assert_eq!(
                table.rows.last(),
                Some(&PromoteRow::Entry {
                    source: "artifacts/k.md".to_owned(),
                    target: "10-notes/k.md".to_owned(),
                    note: note.to_owned(),
                    published: None,
                })
            );
            let (start, end) = *table.row_spans.last().expect("span");
            assert_eq!(format!("{}{}", &updated[..start], &updated[end..]), body);
        }
    }

    /// R244: a promotion out's row records the digest of the copy it
    /// published as a fourth cell, which reads back; an update that names the
    /// same target keeps it, one that names another target drops it, and a
    /// fourth cell that is not a digest is a line keeper cannot read.
    #[test]
    fn a_published_copy_is_recorded_kept_and_dropped() {
        let digest = sha256_hex("copy\n");
        let published = upsert_published_row(
            TEMPLATE_TAIL,
            "artifacts/k.md",
            "10-notes/k.md",
            "knowledge",
            &digest,
        )
        .expect("row");
        assert!(published.ends_with(&format!(
            "| artifacts/k.md | 10-notes/k.md | knowledge | {digest} |\n"
        )));
        let entry = |body: &str| parse(body).expect("table").rows.remove(0);
        assert_eq!(
            entry(&published),
            PromoteRow::Entry {
                source: "artifacts/k.md".to_owned(),
                target: "10-notes/k.md".to_owned(),
                note: "knowledge".to_owned(),
                published: Some(digest.clone()),
            }
        );
        let same = upsert_row(&published, "artifacts/k.md", "10-notes/k.md", "again").expect("row");
        assert!(
            matches!(entry(&same), PromoteRow::Entry { published, .. } if published == Some(digest.clone()))
        );
        let moved =
            upsert_row(&published, "artifacts/k.md", "10-notes/m.md", "again").expect("row");
        assert!(matches!(
            entry(&moved),
            PromoteRow::Entry {
                published: None,
                ..
            }
        ));
        let foreign =
            format!("{TEMPLATE_TAIL}| artifacts/k.md | 10-notes/k.md | knowledge | mine |\n");
        assert!(matches!(entry(&foreign), PromoteRow::Unreadable { .. }));
    }

    /// R95K2-08: a table that ends the file with no line break — empty or
    /// with a row — takes an appended row on a line of its own, which reads
    /// back as its three cells; every byte before it is kept.
    #[test]
    fn a_row_appended_where_the_file_ends_reads_back() {
        let a = PromoteRow::Entry {
            source: "workspace/a.md".to_owned(),
            target: "artifacts/a.md".to_owned(),
            note: "v1".to_owned(),
            published: None,
        };
        let k = PromoteRow::Entry {
            source: "artifacts/k.md".to_owned(),
            target: "10-notes/k.md".to_owned(),
            note: "knowledge".to_owned(),
            published: None,
        };
        let empty = TEMPLATE_TAIL.trim_end_matches('\n').to_owned();
        let one = format!("{TEMPLATE_TAIL}| workspace/a.md | artifacts/a.md | v1 |");
        for (body, rows) in [(empty, vec![k.clone()]), (one, vec![a, k])] {
            let updated =
                upsert_row(&body, "artifacts/k.md", "10-notes/k.md", "knowledge").expect("row");
            assert_eq!(parse(&updated).expect("table").rows, rows, "{updated:?}");
            assert_eq!(
                updated,
                format!("{body}\n| artifacts/k.md | 10-notes/k.md | knowledge |\n")
            );
        }
    }

    /// The fixture session's files, as the session runtime reads them: each
    /// path's bytes and when its content last changed.
    fn facts(files: &[(&str, &str, i64)], knowledge: &[&str]) -> PanelFacts {
        PanelFacts {
            files: files
                .iter()
                .map(|(rel, text, changed)| {
                    (
                        (*rel).to_owned(),
                        Ok(fact_of(rel, text.as_bytes(), Some(*changed)).expect("read")),
                    )
                })
                .collect(),
            workspace: files
                .iter()
                .filter(|(rel, _, _)| rel.starts_with("workspace/"))
                .map(|(rel, _, _)| (*rel).to_owned())
                .collect(),
            stamps: files
                .iter()
                .filter(|(rel, _, _)| rel.starts_with("workspace/"))
                .map(|(rel, text, changed)| {
                    ((*rel).to_owned(), format!("file:{}:{changed}", text.len()))
                })
                .collect(),
            knowledge: files
                .iter()
                .filter(|(rel, _, _)| knowledge.contains(rel))
                .map(|(rel, text, _)| KnowledgeFile {
                    path: (*rel).to_owned(),
                    bytes: text.len() as u64,
                    text: Ok((*text).to_owned()),
                })
                .collect(),
            copies: BTreeMap::new(),
            problems: Vec::new(),
        }
    }

    const NOTE: &str = "---\ntype: Note\ntitle: Taxes, in short\ngenerated:\n  by: agent:tola-grey@electra\n  at: 2026-10-06T09:00:00Z\nhuman_reviewed: false\nstatus: draft\n---\n\nThree papers.\n";

    /// 95.5 acceptance 4 (FR-244): a five-row table — one in order, one
    /// whose workspace source is newer than its target, one whose source was
    /// cleaned up, one whose target is missing, one unreadable line — gives
    /// `ok`, `stale`, `missingSource`, `missingTarget` and the line verbatim (a
    /// row with both ends gone is the loud one);
    /// the two workspace files no row names are listed as promotable. A
    /// source that differs but is older than its target is `ok` (someone
    /// edited the target), and a harvested note promoted into the vault is a
    /// row out of the session whose copy's review is the note's; edited
    /// here after promotion, it is `stale` (acceptance 7). R244: a copy its
    /// row does not record as the one it published carries no review of the
    /// note.
    #[test]
    fn promote_panel_rows() {
        let copied = copy_digest("artifacts/knowledge/taxes/short.md", NOTE.as_bytes());
        let readme = format!(
            "{TEMPLATE_TAIL}| workspace/ok.md | artifacts/ok.md | in order |\n| workspace/newer.md | artifacts/newer.md | stale |\n| workspace/gone.md | artifacts/gone.md | cleaned up |\n| workspace/lost.md | artifacts/lost.md | target gone |\n| workspace/both.md | artifacts/both.md | both gone |\n| workspace/x.md | artifacts/x.md |\n| workspace/edited.md | artifacts/edited.md | target edited |\n| artifacts/knowledge/taxes/short.md | 10-notes/knowledge/short.md | knowledge | {copied} |\n\n## After\n"
        );
        let reviewed =
            crate::agents::knowledge::review(NOTE, "tgorka", "2026-10-06T10:00:00Z", true);
        let edited = NOTE.replace("Three papers.", "Four papers.");
        let files = [
            ("workspace/ok.md", "same\n", 10),
            ("artifacts/ok.md", "same\n", 20),
            ("workspace/newer.md", "v2\n", 30),
            ("artifacts/newer.md", "v1\n", 20),
            ("artifacts/gone.md", "kept\n", 20),
            ("workspace/lost.md", "here\n", 20),
            ("workspace/edited.md", "old\n", 10),
            ("artifacts/edited.md", "hand edited\n", 20),
            ("workspace/draft.md", "unlisted\n", 5),
            ("workspace/data/run.csv", "a,b\n", 5),
            ("workspace/.gitkeep", "", 1),
            ("artifacts/knowledge/taxes/short.md", NOTE, 10),
            // Older than the candidate (a checkout touched it): only the
            // review differs, which is no change of what the note says.
            ("10-notes/knowledge/short.md", &reviewed, 5),
        ];
        // The vault copy as the session runtime reads it for the note.
        let with_copy = |files: &[(&str, &str, i64)]| {
            let mut facts = facts(files, &["artifacts/knowledge/taxes/short.md"]);
            facts.copies.insert(
                "artifacts/knowledge/taxes/short.md".to_owned(),
                Ok(copy_fact(
                    "artifacts/knowledge/taxes/short.md",
                    reviewed.as_bytes(),
                )),
            );
            facts
        };
        let vm = promote_panel(&readme, &with_copy(&files), Some("human:tgorka"));
        assert!(vm.has_table);
        let states: Vec<PromoteState> = vm.rows.iter().map(|row| row.state).collect();
        assert_eq!(
            states,
            [
                PromoteState::Ok,
                PromoteState::Stale,
                PromoteState::MissingSource,
                PromoteState::MissingTarget,
                PromoteState::MissingTarget,
                PromoteState::Unreadable,
                PromoteState::Ok,
                PromoteState::Ok,
            ]
        );
        assert_eq!(
            vm.rows[5].raw.as_deref(),
            Some("| workspace/x.md | artifacts/x.md |")
        );
        assert_eq!(
            vm.rows.iter().map(|row| row.out).collect::<Vec<_>>(),
            [false, false, false, false, false, false, false, true]
        );
        assert_eq!(
            vm.unlisted
                .iter()
                .map(|file| (
                    file.source.as_str(),
                    file.suggested.as_str(),
                    file.refused.is_some()
                ))
                .collect::<Vec<_>>(),
            [
                ("workspace/draft.md", "artifacts/draft.md", false),
                ("workspace/data/run.csv", "artifacts/run.csv", false)
            ]
        );
        // R95P-08: a row offers promotion into the session only where
        // Rust's admission can take it: not an unreadable line, not a row
        // out into the drive, not a source that is gone.
        assert_eq!(
            vm.rows
                .iter()
                .map(|row| row.refused.is_none())
                .collect::<Vec<_>>(),
            [true, true, false, true, false, false, true, false]
        );
        assert_eq!(
            vm.knowledge,
            [KnowledgeNoteVm {
                path: "artifacts/knowledge/taxes/short.md".to_owned(),
                title: Some("Taxes, in short".to_owned()),
                agent: Some("tola-grey".to_owned()),
                host: Some("electra".to_owned()),
                bytes: NOTE.len() as u64,
                revision: Some(sha256_hex(NOTE)),
                promoted_to: Some("10-notes/knowledge/short.md".to_owned()),
                state: Some(PromoteState::Ok),
                reviewed_by: Some("human:tgorka".to_owned()),
                reviewed_by_me: true,
                problem: None,
                foreign_copy: None,
                copy: None,
                destination: None,
                unavailable: None,
                candidate_read: ReadState::Unread,
                copy_read: ReadState::Unread,
                consented: false,
            }]
        );
        let theirs = promote_panel(&readme, &with_copy(&files), Some("human:marta"));
        assert!(!theirs.knowledge[0].reviewed_by_me);
        for (unearned, loss) in [
            (
                readme.replace(&format!(" | {copied} |"), " |"),
                CopyLoss::Unrecorded,
            ),
            (
                readme.replace(&copied, &sha256_hex("another copy\n")),
                CopyLoss::Changed,
            ),
        ] {
            let vm = promote_panel(&unearned, &with_copy(&files), Some("human:tgorka"));
            assert_eq!(vm.knowledge[0].reviewed_by, None, "{unearned}");
            assert!(!vm.knowledge[0].reviewed_by_me, "{unearned}");
            assert_eq!(
                vm.knowledge[0].foreign_copy,
                Some(loss.explain(
                    "artifacts/knowledge/taxes/short.md",
                    "10-notes/knowledge/short.md"
                )),
                "{unearned}"
            );
        }

        let mut after = files;
        after[11] = ("artifacts/knowledge/taxes/short.md", &edited, 30);
        let vm = promote_panel(&readme, &with_copy(&after), None);
        assert_eq!(vm.rows[7].state, PromoteState::Stale);
        assert_eq!(vm.knowledge[0].state, Some(PromoteState::Stale));
    }

    /// R95K2-06: freshness is told by content facts only. Two files that
    /// differ while one's content time is not known — its run of commits
    /// starts past the history read, or a vault copy has none — are
    /// `unknown` with why, never `ok` by default; the same bytes are `ok`
    /// whatever is known of their times.
    #[test]
    fn an_unknown_content_time_is_never_read_as_in_order() {
        let readme = format!(
            "{TEMPLATE_TAIL}| artifacts/knowledge/taxes/short.md | 10-notes/knowledge/short.md | knowledge |\n"
        );
        let edited = NOTE.replace("Three papers.", "Four papers.");
        let with = |candidate: &str, copy_changed: Option<i64>| {
            let mut facts = facts(
                &[("artifacts/knowledge/taxes/short.md", candidate, 30)],
                &[],
            );
            facts.files.insert(
                "10-notes/knowledge/short.md".to_owned(),
                Ok(
                    fact_of("10-notes/knowledge/short.md", NOTE.as_bytes(), copy_changed)
                        .expect("read"),
                ),
            );
            promote_panel(&readme, &facts, None).rows.remove(0)
        };
        let unknown = with(&edited, None);
        assert_eq!(unknown.state, PromoteState::Unknown);
        assert!(unknown
            .problem
            .is_some_and(|why| why.contains("10-notes/knowledge/short.md")));
        assert_eq!(with(&edited, Some(20)).state, PromoteState::Stale);
        assert_eq!(with(NOTE, None).state, PromoteState::Ok);
    }

    /// R95K-12: a file a row names that could not be read is `unknown`
    /// with why — never `missingSource` or `missingTarget` — a harvested
    /// note that could not be read is listed with why and no revision, and
    /// what the listing could not see is said.
    #[test]
    fn what_could_not_be_read_is_said_not_absent() {
        let readme = format!(
            "{TEMPLATE_TAIL}| workspace/a.md | artifacts/a.md | |\n| workspace/b.md | artifacts/b.md | |\n"
        );
        let mut facts = facts(
            &[("workspace/a.md", "a\n", 10), ("artifacts/b.md", "b\n", 10)],
            &[],
        );
        facts
            .files
            .insert("artifacts/a.md".to_owned(), Err("denied".to_owned()));
        facts
            .files
            .insert("workspace/b.md".to_owned(), Err("denied".to_owned()));
        facts.knowledge.push(KnowledgeFile {
            path: "artifacts/knowledge/t/n.md".to_owned(),
            bytes: 7,
            text: Err("denied".to_owned()),
        });
        facts
            .problems
            .push("workspace/deep could not be listed".to_owned());
        let vm = promote_panel(&readme, &facts, None);
        assert_eq!(
            vm.rows
                .iter()
                .map(|row| (row.state, row.problem.as_deref()))
                .collect::<Vec<_>>(),
            [
                (PromoteState::Unknown, Some("denied")),
                (PromoteState::Unknown, Some("denied"))
            ]
        );
        assert_eq!(vm.knowledge.len(), 1);
        assert_eq!(vm.knowledge[0].problem.as_deref(), Some("denied"));
        assert_eq!(vm.knowledge[0].revision, None);
        assert_eq!(vm.knowledge[0].bytes, 7);
        assert_eq!(vm.problems, ["workspace/deep could not be listed"]);
    }

    /// R95K-13: the comparison digest streams past the head and leaves out
    /// only a whole frontmatter's review keys; the same bytes digest alike
    /// read whole or in pieces.
    #[test]
    fn a_digest_streams_and_ignores_only_review_keys() {
        let note = format!("{NOTE}{}", "x".repeat(3 * HEAD_BYTES as usize));
        let reviewed = crate::agents::knowledge::review(&note, "tgorka", "2026-10-06", true);
        let plain = digest_of("n.md", note.as_bytes()).expect("digest");
        let ticked = digest_of("n.md", reviewed.as_bytes()).expect("digest");
        assert_eq!(plain, ticked);
        let longer = format!("{note}y");
        assert_ne!(digest_of("n.md", longer.as_bytes()).expect("digest"), plain);
        let raw = digest_of("n.bin", reviewed.as_bytes()).expect("digest");
        assert_eq!(raw, sha256_hex(&reviewed));
    }

    /// R253 (R95K5-01, R95K5-02): a harvested note's copy digest binds where
    /// its frontmatter ends. Metadata in the block, and the same lines in the
    /// body behind a block that holds only a review, are one byte stream once
    /// the review keys are out — two notes, two digests; so too a harvested
    /// note's own metadata pushed into a second block. And no review moves
    /// it: a tick, its untick and another person's tick leave every copy's
    /// digest as the note's — a frontmatter the tick carries past
    /// [`HEAD_BYTES`], a note with none, one behind a byte order mark, an
    /// empty block, a block of reviews alone or an empty one in front of a
    /// body that opens with a block of its own — while a changed body moves
    /// it. Any other source's copy is digested byte for byte.
    #[test]
    fn a_copy_digest_binds_where_the_frontmatter_ends_and_no_review_moves_it() {
        use crate::agents::knowledge::review;
        const SOURCE: &str = "artifacts/knowledge/t/n.md";
        const AT: &str = "2026-10-06T10:00:00Z";
        let digest = |text: &str| copy_digest(SOURCE, text.as_bytes());

        let block = "---\ntitle: T\nhuman_reviewed: true\n---\nBody.\n";
        let in_body = "---\nhuman_reviewed: true\n---\n---\ntitle: T\n---\nBody.\n";
        assert_ne!(digest(block), digest(in_body));
        let pushed = format!(
            "---\nhuman_reviewed: true\n---\n{}",
            NOTE.replace("human_reviewed: false\n", "")
        );
        assert_ne!(digest(NOTE), digest(&pushed));

        let pad = HEAD_BYTES as usize - "---\ntitle: T\nnotes: \n---\n".len() - 20;
        let near_the_head = format!("---\ntitle: T\nnotes: {}\n---\nBody.\n", "x".repeat(pad));
        assert!(
            Frontmatter::parse(&review(&near_the_head, "tgorka", AT, true)).1 > HEAD_BYTES as usize
        );
        for note in [
            NOTE.to_owned(),
            near_the_head,
            "Body only.\n".to_owned(),
            format!("\u{feff}{NOTE}"),
            "\u{feff}Body only.\n".to_owned(),
            "---\n---\nBody.\n".to_owned(),
            in_body.to_owned(),
            "---\n---\n---\ntitle: T\n---\nBody.\n".to_owned(),
        ] {
            let ticked = review(&note, "tgorka", AT, true);
            let unticked = review(&ticked, "tgorka", AT, false);
            let again = review(&unticked, "marta", AT, true);
            for copy in [&ticked, &unticked, &again] {
                assert_eq!(digest(copy), digest(&note), "{note:?} → {copy:?}");
            }
            assert_ne!(digest(&format!("{note}More.\n")), digest(&note), "{note:?}");
        }

        let ticked = review(NOTE, "tgorka", AT, true);
        assert_ne!(
            copy_digest("artifacts/plan.md", ticked.as_bytes()),
            copy_digest("artifacts/plan.md", NOTE.as_bytes())
        );
    }

    /// 95.5 acceptance 4, the rename (§3 row 31): a file renamed in the
    /// tree, whose table cell `refs::rewrite_pointers` rewrote, keeps its
    /// row under the new name, `ok`, and is not listed again as unlisted.
    #[test]
    fn a_renamed_artifact_keeps_its_row() {
        let readme =
            format!("{TEMPLATE_TAIL}| workspace/draft.md | artifacts/report.md | weekly |\n");
        let rewritten = crate::sessions::refs::rewrite_pointers(
            &readme,
            "README.md",
            "workspace/draft.md",
            "workspace/weekly.md",
        )
        .expect("the row names the renamed file");
        let files = [
            ("workspace/weekly.md", "report\n", 10),
            ("artifacts/report.md", "report\n", 20),
        ];
        let vm = promote_panel(&rewritten, &facts(&files, &[]), None);
        assert_eq!(vm.rows.len(), 1);
        assert_eq!(vm.rows[0].source, "workspace/weekly.md");
        assert_eq!(vm.rows[0].state, PromoteState::Ok);
        assert!(vm.unlisted.is_empty(), "{:?}", vm.unlisted);
    }

    /// R95K-15: a promote-out target is drive-relative, so renaming a
    /// session file of the same spelling leaves it alone; renaming the
    /// artifact it was promoted from rewrites only the source, and the copy
    /// its row records as published goes with it (R244).
    #[test]
    fn a_rename_in_the_session_never_moves_a_drive_target() {
        let copied = sha256_hex("copy\n");
        let readme = format!(
            "{TEMPLATE_TAIL}| artifacts/knowledge/t/n.md | 10-notes/knowledge/n.md | knowledge | {copied} |\n"
        );
        let rewrite = |from: &str, to: &str| {
            crate::sessions::refs::rewrite_pointers(&readme, "README.md", from, to)
        };
        assert_eq!(
            rewrite("10-notes/knowledge/n.md", "10-notes/knowledge/m.md"),
            None
        );
        let moved = rewrite("artifacts/knowledge/t/n.md", "artifacts/knowledge/t/m.md")
            .expect("the row names the renamed source");
        assert_eq!(
            parse(&moved).expect("table").rows,
            [PromoteRow::Entry {
                source: "artifacts/knowledge/t/m.md".to_owned(),
                target: "10-notes/knowledge/n.md".to_owned(),
                note: "knowledge".to_owned(),
                published: Some(copied),
            }]
        );
    }
}
