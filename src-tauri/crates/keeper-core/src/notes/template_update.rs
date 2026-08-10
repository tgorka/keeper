//! Offering a template's own edit to the notes made from it (FR-163, UX-DR59).
//!
//! # The rule, and why it is this one
//!
//! Editing a template raises a question with one wrong answer. The wrong answer
//! is "replace the note with the new template", because a note somebody has
//! written in is not a copy of a template any more — it is the thing the
//! template existed to produce. Overwriting it is not an update, it is a
//! deletion with a progress bar.
//!
//! So this module never replaces a note. It **re-applies the template's own
//! edit**, change by change, and a change lands in a note only when the note
//! still says, byte for byte, what the template used to say there:
//!
//! - the template's old text and its new text are diffed **as that note saw
//!   them** — both expanded through [`templates::expand`] with the note's own
//!   creation context, so `{{date:YYYY-MM-DD}}` is compared as the date the note
//!   actually carries and never written into a note as a literal placeholder;
//! - each change is located in the note by an **anchor of unchanged template
//!   lines**, and it is applied only when that anchor occurs **exactly once**;
//! - the only lines a change may delete are lines identical to what the old
//!   template said. A line the user wrote cannot be removed by this module,
//!   because no such line is ever in a `removed` set;
//! - a change whose anchor is gone (the user edited that part) is reported as
//!   skipped, with the reason, and the rest of the note still updates.
//!
//! That makes the destructive reading unreachable rather than merely
//! discouraged: there is no code path here that emits a note body containing
//! fewer of the user's own lines than it started with.
//!
//! **Frontmatter is never touched.** A note's identity, tags, order and reserved
//! `keeper:` map are its own; a template edit is a change to prose. This is also
//! why a note whose recorded template path has gone stale (the template was
//! renamed, and [`templates::FROM_TEMPLATE_ID_KEY`] is what found it) is
//! *reported* rather than repaired — repairing it would mean writing
//! frontmatter on the same button that writes a body, and one button with two
//! write shapes is how a "safe" feature grows a way to lose a property.
//!
//! # Why an update must be undoable before it is offered
//!
//! keeper's note history is a projection of the commits `keeper-sync` writes
//! (AD-63); there is no parallel store. That has a consequence this module is
//! required to respect: **a note is only recoverable when git's copy of it is
//! byte-identical to what is on disk.** If the note has changes the vault has
//! not committed yet, the newest revision is not the text this update is about
//! to overwrite, and "undo" would silently discard the user's most recent
//! writing as well.
//!
//! So recoverability is an input ([`Recoverability`]), not an afterthought, and
//! a note that is not recoverable is listed with its changes and cannot be
//! selected. keeper commits on its own within seconds, so the remedy is to wait
//! — never to force a commit from here, which would be a second committer over
//! one repository.
//!
//! # What is deliberately absent
//!
//! There is no "apply to all". Not an oversight: a single control that changes
//! every note made from a template is the destructive reading wearing a
//! checkbox, and the whole point of UX-DR59 is that the user chose. Selection is
//! per note, and [`MAX_OFFER_NOTES`] bounds what is offered at all, because an
//! offer nobody can read is not consent.
//!
//! Everything here is pure: values in, values out. No filesystem, no git, no
//! clock (AD-55). The shell reads the bodies, asks git the one question
//! [`Recoverability`] answers, and writes what [`apply`] returns.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::notes::index::{FIELD_LIST_SEPARATOR, RESERVED_FIELD_PREFIX};
use crate::notes::templates::{
    self, Provenance, TemplateCtx, FROM_TEMPLATE_ID_KEY, FROM_TEMPLATE_KEY,
};

/// How many unchanged template lines are tried as an anchor on each side.
///
/// Three, the unified-diff convention, and then progressively fewer: a shorter
/// anchor is a *looser* match, so the search always takes the longest anchor
/// that occurs at all and refuses when that one occurs more than once. Loosening
/// past a match would be guessing, and this module does not guess.
const CONTEXT: usize = 3;

/// The largest template, in lines, this module will diff.
///
/// The line diff is O(old x new); at this bound the table is under 1.5 MB and
/// the walk is instant. Past it the whole template is treated as one change,
/// which by the anchoring rule can only land in a note nobody has touched —
/// degraded, still safe, and it says so.
const MAX_DIFF_LINES: usize = 600;

/// How many notes one offer may contain.
///
/// A cap, not a page: the surface has no "apply to all", so an offer larger than
/// a person will actually read is an offer that would be accepted unread. When a
/// template has more notes than this, keeper declines and says the number
/// (see [`too_many_notes`]) rather than showing the first two hundred and
/// letting the rest look handled.
pub const MAX_OFFER_NOTES: usize = 200;

// ---------------------------------------------------------------------------
// Which notes came from this template
// ---------------------------------------------------------------------------

/// The template being edited, as the finder needs to see it.
#[derive(Debug, Clone, Copy)]
pub struct TemplateRef<'a> {
    /// Its current vault-relative path.
    pub path: &'a str,
    /// Its own frontmatter `id`, when it has one.
    pub id: Option<&'a str>,
}

/// How a note was recognised as this template's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matched {
    /// The note records the template's id. Survives a rename.
    ById,
    /// The note records only a path, and it is this template's path. The only
    /// evidence available for a note written before the id was stamped, or by
    /// hand — and the case that breaks silently when a template moves, which is
    /// why the id exists.
    ByPath,
}

/// Whether `provenance` names `template`.
///
/// The id is decisive when both sides have one: a note that records some *other*
/// template's id is not this template's note even if the paths now coincide,
/// which is what stops a template deleted and replaced at the same path from
/// adopting the previous one's children.
#[must_use]
pub fn made_from(provenance: &Provenance, template: &TemplateRef<'_>) -> Option<Matched> {
    if let (Some(noted), Some(actual)) = (provenance.id.as_deref(), template.id) {
        return (noted == actual).then_some(Matched::ById);
    }
    match provenance.path.as_deref() {
        Some(noted) if noted == template.path => Some(Matched::ByPath),
        _ => None,
    }
}

/// The `IndexEntry.fields` key a note's whole reserved `keeper:` map flattens
/// into.
///
/// Derived from [`RESERVED_FIELD_PREFIX`] rather than spelled again, because the
/// two must not be able to disagree: the prefix documents that a one-level
/// `keeper:` map indexes under the bare key, and this is that bare key.
#[must_use]
pub fn keeper_field() -> &'static str {
    RESERVED_FIELD_PREFIX.trim_end_matches('.')
}

/// Read a note's template provenance out of its **index entry** rather than its
/// file.
///
/// [`templates::provenance`] is the authority and takes the note's source; this
/// takes the flattened string the index already holds, so the finder runs over
/// an in-memory snapshot with zero reads on a ten-thousand-note vault. That
/// matters: the offer appears right after a save, and a scan that opens every
/// note would make editing a template feel like a fault.
///
/// It inverts exactly one thing — [`crate::notes::frontmatter::FieldValue::index_string`]'s
/// rendering of a one-level map, `"key: value"` joined by
/// [`FIELD_LIST_SEPARATOR`]. A line without a `": "` separator, or a value the
/// flattening could not have produced, is skipped rather than guessed at, and
/// the round trip is asserted in this module's tests against the real renderer.
#[must_use]
pub fn provenance_from_index(fields: &BTreeMap<String, String>) -> Provenance {
    let Some(flattened) = fields.get(keeper_field()) else {
        return Provenance::default();
    };
    let mut found = Provenance::default();
    for line in flattened.split(FIELD_LIST_SEPARATOR) {
        let Some((key, value)) = line.split_once(": ") else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            FROM_TEMPLATE_KEY => found.path = Some(value.to_owned()),
            FROM_TEMPLATE_ID_KEY => found.id = Some(value.to_owned()),
            _ => {}
        }
    }
    found
}

// ---------------------------------------------------------------------------
// The template's own edit
// ---------------------------------------------------------------------------

/// One change the template made to itself, with the unchanged template lines
/// around it that will locate it in a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Up to [`CONTEXT`] unchanged lines immediately above.
    pub before: Vec<String>,
    /// The lines the old template had here. Empty for a pure insertion. These
    /// are the ONLY lines this module will ever delete from a note.
    pub removed: Vec<String>,
    /// The lines the new template has here. Empty for a pure deletion.
    pub added: Vec<String>,
    /// Up to [`CONTEXT`] unchanged lines immediately below.
    pub after: Vec<String>,
}

/// The changes between two texts, as line-level hunks with context.
///
/// A longest-common-subsequence walk rather than a token diff: a template is
/// prose and headings, the unit a person means by "the template changed" is a
/// line, and a line is also the unit that can be located in a note safely. A
/// word-level diff would produce changes whose anchor is a fragment, and a
/// fragment matches in far more places than it should.
#[must_use]
pub fn changes(old: &str, new: &str) -> Vec<Change> {
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    if old_lines == new_lines {
        return Vec::new();
    }
    if old_lines.len() > MAX_DIFF_LINES || new_lines.len() > MAX_DIFF_LINES {
        // Degraded but honest: one change whose `removed` set is the WHOLE old
        // text. The rule is unchanged — only lines the template wrote may go —
        // so it lands only in a note that still holds the old template
        // contiguously and exactly once. A note written into the middle of it
        // is reported as diverged rather than rebuilt.
        return vec![Change {
            before: Vec::new(),
            removed: owned(&old_lines),
            added: owned(&new_lines),
            after: Vec::new(),
        }];
    }
    group(&steps(&old_lines, &new_lines), &old_lines, &new_lines)
}

/// One position in the edit script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The line is in both texts.
    Keep,
    /// The line is only in the old text.
    Del,
    /// The line is only in the new text.
    Ins,
}

/// The edit script taking `old` to `new`, longest common subsequence first.
fn steps(old: &[&str], new: &[&str]) -> Vec<Step> {
    let rows = old.len() + 1;
    let cols = new.len() + 1;
    let mut table = vec![0u32; rows * cols];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            table[i * cols + j] = if old[i] == new[j] {
                table[(i + 1) * cols + j + 1] + 1
            } else {
                table[(i + 1) * cols + j].max(table[i * cols + j + 1])
            };
        }
    }

    let mut script = Vec::with_capacity(old.len() + new.len());
    let (mut i, mut j) = (0usize, 0usize);
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            script.push(Step::Keep);
            i += 1;
            j += 1;
        } else if table[(i + 1) * cols + j] >= table[i * cols + j + 1] {
            script.push(Step::Del);
            i += 1;
        } else {
            script.push(Step::Ins);
            j += 1;
        }
    }
    script.extend(std::iter::repeat_n(Step::Del, old.len() - i));
    script.extend(std::iter::repeat_n(Step::Ins, new.len() - j));
    script
}

/// Fold an edit script into hunks, each carrying its surrounding context.
fn group(script: &[Step], old: &[&str], new: &[&str]) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    let (mut i, mut j, mut at) = (0usize, 0usize, 0usize);

    while at < script.len() {
        if script[at] == Step::Keep {
            i += 1;
            j += 1;
            at += 1;
            continue;
        }

        // `i` is the index just past the last kept old line, so the context
        // above is the tail of what has already been consumed.
        let before = owned(&old[i.saturating_sub(CONTEXT)..i]);
        let (mut removed, mut added) = (Vec::new(), Vec::new());
        while at < script.len() && script[at] != Step::Keep {
            match script[at] {
                Step::Del => {
                    removed.push(old[i].to_owned());
                    i += 1;
                }
                Step::Ins => {
                    added.push(new[j].to_owned());
                    j += 1;
                }
                Step::Keep => unreachable!("the loop condition excludes it"),
            }
            at += 1;
        }
        let after = owned(&old[i..(i + CONTEXT).min(old.len())]);
        out.push(Change {
            before,
            removed,
            added,
            after,
        });
    }
    out
}

fn owned(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}

/// The real lines of a text, without the phantom the terminator produces.
///
/// `"a\n".split('\n')` is `["a", ""]`, and that trailing empty element is not a
/// line anybody wrote — it is the newline. Carrying it into the diff was a real
/// defect and not a cosmetic one: the phantom became part of a change's context,
/// so "the template gained a section at the end" anchored on a blank line that
/// exists exactly once in the template and twice in any note somebody had
/// written in, and the change came back ambiguous in precisely the case the
/// story is about.
///
/// Whether the text ended with a terminator is kept separately, by
/// [`ends_with_newline`], and put back by [`rejoin`] — so a note that had no
/// final newline does not gain one and one that had it does not lose it.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

fn ends_with_newline(text: &str) -> bool {
    text.ends_with('\n')
}

/// Put lines back together the way [`split_lines`] took them apart.
fn rejoin(lines: &[String], trailing_newline: bool) -> String {
    let mut out = lines.join("\n");
    if trailing_newline {
        out.push('\n');
    }
    out
}

/// Whether every line in an anchor is blank.
///
/// A blank anchor is not an anchor. Matching one would place a change at
/// whichever empty line happened to be unique, which is a coin toss dressed as a
/// decision — so an anchor made only of whitespace is passed over, and a change
/// with no better one is reported unanchored rather than guessed at.
fn all_blank(change: &Change, pre: usize, post: usize) -> bool {
    change.before[change.before.len() - pre..]
        .iter()
        .chain(change.removed.iter())
        .chain(change.after[..post].iter())
        .all(|line| line.trim().is_empty())
}

// ---------------------------------------------------------------------------
// Planning one note
// ---------------------------------------------------------------------------

/// Whether the note's pre-update bytes are already in this vault's history.
///
/// The shell answers this from git — one `status` call for the whole vault, not
/// one per note — and it is the gate on applying anything, because an update
/// that cannot be undone is not one keeper will make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recoverability {
    /// git's copy is byte-identical to the file on disk. The newest revision IS
    /// the text about to be replaced, so restoring it is an exact undo.
    Committed,
    /// The note has edits the vault has not committed yet. Restoring the newest
    /// revision would discard those too, so this update is refused.
    Modified,
    /// git has never seen this note. There is nothing to restore to at all.
    Untracked,
}

/// Why a change was not applied to a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// The note no longer says what the template used to say here.
    Diverged,
    /// The anchor occurs in more than one place.
    Ambiguous,
    /// There is no unchanged template text to position this against.
    Unanchored,
}

impl Skip {
    /// A finished sentence, composed here because this crate is where the reason
    /// is known and because "diverged" is not a word to show anybody.
    #[must_use]
    pub fn sentence(self) -> &'static str {
        match self {
            Self::Diverged => {
                "You have written over this part of the note, so keeper left it as you wrote it."
            }
            Self::Ambiguous => {
                "This text appears more than once in the note, so keeper cannot tell which \
                 place the template means."
            }
            Self::Unanchored => {
                "The template gives keeper nothing in this note to position this against, \
                 so keeper will not guess where it goes."
            }
        }
    }
}

/// What would happen to one change in one note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It applies, replacing `removed` at this 0-based line index. For a pure
    /// insertion the index is where the new lines go.
    Applies { at: usize },
    /// It does not, for this reason.
    Skipped(Skip),
}

/// One change, resolved against one note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedChange {
    pub removed: Vec<String>,
    pub added: Vec<String>,
    pub outcome: Outcome,
}

impl PlannedChange {
    /// Whether selecting this change would change the note.
    #[must_use]
    pub fn appliable(&self) -> bool {
        matches!(self.outcome, Outcome::Applies { .. })
    }
}

/// One note as the planner needs to see it.
#[derive(Debug, Clone)]
pub struct NoteInput<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub path: &'a str,
    /// Everything after the note's frontmatter block. The frontmatter itself is
    /// never an input here, because it is never an output.
    pub body: &'a str,
    /// The context this note's own expansion used when it was created, so the
    /// template's placeholders are compared and written as this note has them.
    pub ctx: TemplateCtx,
    /// The template path the note records, when the template has since moved.
    /// Reported, never repaired.
    pub stale_path: Option<String>,
    pub recoverability: Recoverability,
}

/// Everything that would happen to one note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotePlan {
    pub note_id: String,
    pub title: String,
    pub path: String,
    pub changes: Vec<PlannedChange>,
    /// Why this note cannot be selected at all, as a finished sentence. The
    /// changes are still listed when this is set — hiding them would leave the
    /// user unable to see what waiting buys them.
    pub blocked: Option<String>,
    pub stale_path: Option<String>,
}

impl NotePlan {
    /// Whether anything here would actually be written.
    #[must_use]
    pub fn selectable(&self) -> bool {
        self.blocked.is_none() && self.changes.iter().any(PlannedChange::appliable)
    }
}

/// Resolve the template's edit against one note.
///
/// `old_source` and `new_source` are the template note's whole file before and
/// after the edit; both are expanded through the note's own context, so what is
/// diffed is what this note would have received then and what it would receive
/// now. A template whose text did not change once expanded produces no changes
/// at all — which is the right answer for an edit that only touched the
/// template's own frontmatter.
#[must_use]
pub fn plan_note(old_source: &str, new_source: &str, note: &NoteInput<'_>) -> NotePlan {
    let old = templates::expand(old_source, &note.ctx).body;
    let new = templates::expand(new_source, &note.ctx).body;
    let lines = split_lines(note.body);

    let changes = changes(&old, &new)
        .into_iter()
        .map(|change| {
            let outcome = locate(&lines, &change);
            PlannedChange {
                removed: change.removed,
                added: change.added,
                outcome,
            }
        })
        .collect();

    NotePlan {
        note_id: note.id.to_owned(),
        title: note.title.to_owned(),
        path: note.path.to_owned(),
        changes,
        blocked: blocked_sentence(note.title, note.recoverability),
        stale_path: note.stale_path.clone(),
    }
}

/// Why this note is not eligible, or `None`.
fn blocked_sentence(title: &str, recoverability: Recoverability) -> Option<String> {
    match recoverability {
        Recoverability::Committed => None,
        Recoverability::Modified => Some(format!(
            "\u{201c}{title}\u{201d} has changes this vault has not committed yet, so undoing \
             this update would throw those away too. It commits on its own within a few \
             seconds — reopen this after it does."
        )),
        Recoverability::Untracked => Some(format!(
            "\u{201c}{title}\u{201d} is not in this vault's history yet, so keeper would have \
             nothing to put it back from. It commits on its own within a few seconds — reopen \
             this after it does."
        )),
    }
}

/// Find the one place in `lines` a change belongs, or say why there is not one.
///
/// The anchors are tried longest first and the search stops at the first one
/// that matches anywhere: a shorter anchor matches a superset of the places a
/// longer one does, so if the longest matching anchor is not unique, no shorter
/// one can be either. That is what turns "we could not find it" and "we found it
/// twice" into two different, honest answers instead of one silent skip.
///
/// **A section the template gained at its end goes to the end of the note**, not
/// to wherever the template's last line now sits. That is the one placement rule
/// that is not literal, and it is the difference between right and useless: a
/// journal template that grows an `## Actions` heading, applied to a note whose
/// author wrote under `## Notes`, would otherwise slide the new heading ABOVE
/// everything they wrote and silently re-file all of it under a section they
/// have never seen. Appending re-parents nothing. For a note nobody has written
/// in the two placements are the same position, so the untouched case is
/// unaffected.
fn locate(lines: &[&str], change: &Change) -> Outcome {
    // A change at the template's end that only adds. `after` empty means nothing
    // followed it in the old template, so "after the anchor" and "at the end"
    // agree for an untouched note and disagree only where it matters.
    let appends = change.removed.is_empty() && change.after.is_empty();

    let mut tried: Vec<(usize, usize)> = Vec::new();
    let mut evaluated = false;
    for (pre, post) in anchors(change) {
        if tried.contains(&(pre, post)) || all_blank(change, pre, post) {
            continue;
        }
        tried.push((pre, post));
        evaluated = true;
        match matches_of(lines, change, pre, post) {
            Found::None => {}
            Found::One(at) => {
                return Outcome::Applies {
                    at: if appends { lines.len() } else { at + pre },
                };
            }
            Found::Many => return Outcome::Skipped(Skip::Ambiguous),
        }
    }
    if evaluated {
        Outcome::Skipped(Skip::Diverged)
    } else {
        Outcome::Skipped(Skip::Unanchored)
    }
}

/// Every anchor to try, as `(lines of context above, lines of context below)`,
/// loosest last.
///
/// Two-sided anchors first, because a change surrounded by text the note still
/// has is the case there is no doubt about. Then one-sided, which is what keeps
/// a section the template appended landing in a note whose author has since
/// written below it. Then, only for a change that has `removed` lines of its
/// own, no context at all — those lines are themselves an anchor, and they are
/// the template's, not the user's.
fn anchors(change: &Change) -> Vec<(usize, usize)> {
    let before = change.before.len().min(CONTEXT);
    let after = change.after.len().min(CONTEXT);
    let mut out = Vec::new();
    for k in (1..=CONTEXT).rev() {
        if before >= k && after >= k {
            out.push((k, k));
        }
    }
    for k in (1..=CONTEXT).rev() {
        if before >= k {
            out.push((k, 0));
        }
    }
    for k in (1..=CONTEXT).rev() {
        if after >= k {
            out.push((0, k));
        }
    }
    if !change.removed.is_empty() {
        out.push((0, 0));
    }
    out
}

enum Found {
    None,
    One(usize),
    Many,
}

/// Where `before[-pre..] ++ removed ++ after[..post]` occurs in `lines`.
///
/// Comparison ignores a trailing carriage return on the note's side: a vault
/// edited on Windows is full of CRLF, and a template keeper wrote is not, so a
/// byte-exact comparison would report every note as diverged. The splice puts
/// the note's own terminators back (see [`apply`]).
fn matches_of(lines: &[&str], change: &Change, pre: usize, post: usize) -> Found {
    let before = &change.before[change.before.len() - pre..];
    let after = &change.after[..post];
    let width = pre + change.removed.len() + post;
    if width == 0 || width > lines.len() {
        return Found::None;
    }

    let mut found: Option<usize> = None;
    for start in 0..=lines.len() - width {
        let window = &lines[start..start + width];
        let same = window
            .iter()
            .map(|line| line.trim_end_matches('\r'))
            .eq(before
                .iter()
                .chain(change.removed.iter())
                .chain(after.iter())
                .map(String::as_str));
        if same {
            if found.is_some() {
                return Found::Many;
            }
            found = Some(start);
        }
    }
    match found {
        Some(start) => Found::One(start),
        None => Found::None,
    }
}

// ---------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------

/// The note's body with the accepted changes spliced in, or `None` when nothing
/// would change.
///
/// `None` is the load-bearing return: an empty selection, a selection of only
/// skipped changes, and a blocked note all produce it, so "the user declined"
/// and "there is nothing to write" reach the shell as the same thing — no write.
/// The shell has no branch in which it writes a body this function did not
/// return, which is what makes declining byte-for-byte inert rather than
/// merely intended to be.
///
/// **One place decides "nothing to write", and it is the final comparison.**
/// `split_lines`/`rejoin` round-trip any text exactly, so a run with no accepted
/// span reconstructs the body byte for byte and falls out as `None` on its own.
/// There used to be an early return for the empty-selection case as well; a
/// mutation removing it survived the whole suite, which is the correct verdict
/// and the right reason to delete it — it was an unreachable second answer to a
/// question already answered below, and a guard no test can distinguish from its
/// absence is a guard the next reader will trust for a promise it does not make.
///
/// Splices run bottom-up so the line indices resolved against the original body
/// stay valid, and any accepted change whose span overlaps one already taken is
/// dropped: two changes cannot both own a line.
#[must_use]
pub fn apply(body: &str, plan: &NotePlan, accepted: &[usize]) -> Option<String> {
    if plan.blocked.is_some() {
        return None;
    }

    let mut spans: Vec<(usize, usize, &Vec<String>)> = Vec::new();
    for index in accepted {
        let Some(change) = plan.changes.get(*index) else {
            continue;
        };
        let Outcome::Applies { at } = change.outcome else {
            continue;
        };
        spans.push((at, at + change.removed.len(), &change.added));
    }
    spans.sort_by_key(|(start, _, _)| *start);

    let mut lines: Vec<String> = split_lines(body).into_iter().map(str::to_owned).collect();
    let terminator = if body.contains("\r\n") { "\r" } else { "" };

    // Bottom-up, and each span checked against the one that followed it, so an
    // overlap drops the *later* change rather than corrupting both.
    let mut floor = lines.len();
    for (start, end, added) in spans.into_iter().rev() {
        if end > floor || end > lines.len() {
            continue;
        }
        let replacement = added
            .iter()
            .map(|line| format!("{line}{terminator}"))
            .collect::<Vec<_>>();
        lines.splice(start..end, replacement);
        floor = start;
    }

    let updated = rejoin(&lines, ends_with_newline(body));
    (updated != body).then_some(updated)
}

// ---------------------------------------------------------------------------
// Sentences the surface prints verbatim
// ---------------------------------------------------------------------------

/// What keeper says when the template's edit changed nothing a note could see.
///
/// Said out loud rather than showing an empty dialog, and logged by the shell at
/// INFO: a feature that can decline to act and does so silently is one nobody
/// can tell apart from a feature that is broken.
#[must_use]
pub fn nothing_changed(template: &str) -> String {
    format!(
        "Nothing in \u{201c}{template}\u{201d}'s text changed, so there is nothing to offer the \
         notes made from it."
    )
}

/// What keeper says when no note records this template.
#[must_use]
pub fn no_notes(template: &str) -> String {
    format!(
        "No note in this vault records \u{201c}{template}\u{201d} as the template it came from."
    )
}

/// What keeper says when the template has more notes than it will offer at once.
#[must_use]
pub fn too_many_notes(template: &str, count: usize) -> String {
    format!(
        "{count} notes came from \u{201c}{template}\u{201d}. keeper only offers an update it \
         expects you to read, and it will not change {count} notes on one click, so this one is \
         not offered. Open the notes you want updated and edit them, or narrow the template."
    )
}

/// What keeper says when every note it found is currently ineligible.
#[must_use]
pub fn nothing_applies(template: &str) -> String {
    format!(
        "keeper found the notes made from \u{201c}{template}\u{201d}, but none of them still \
         match the parts of the template that changed, so there is nothing it can apply without \
         writing over what you wrote."
    )
}

// ---------------------------------------------------------------------------
// View models
// ---------------------------------------------------------------------------

/// One change, as the preview shows it (UX-DR59).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TemplateChangeVm {
    /// Its index in the note's plan, and the token
    /// [`TemplateUpdateSelectionVm`] selects by.
    pub index: u32,
    /// Lines that would leave the note. Every one of them is text the template
    /// put there and the note has not altered.
    pub removed: Vec<String>,
    /// Lines that would arrive.
    pub added: Vec<String>,
    /// 1-based line in the note where it lands, for the preview to say where.
    /// `None` when it does not land.
    pub at_line: Option<u32>,
    /// Why it will not be applied, as a finished sentence. `None` means it will.
    pub skipped: Option<String>,
}

/// One note in the offer.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TemplateUpdateNoteVm {
    pub note_id: String,
    pub title: String,
    pub path: String,
    pub changes: Vec<TemplateChangeVm>,
    /// Why this note cannot be chosen. Present ⇒ the surface disables it and
    /// prints this.
    pub blocked: Option<String>,
    /// The template path this note records, when the template has since moved.
    /// Shown so a renamed template is visible rather than merely survived.
    pub stale_path: Option<String>,
}

/// What keeper offers after a template was edited (FR-163, UX-DR59).
///
/// `notes` non-empty and `declined` absent is an offer. `declined` present is a
/// refusal with its reason, and `notes` is then empty — exactly one side is
/// populated, the shape `SyncGitVm` established, so no surface has to decide
/// which of two half-populated fields to believe.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TemplateUpdateOfferVm {
    /// Vault-relative path of the template that was edited.
    pub template_path: String,
    /// Its title, for the sentences the surface prints.
    pub template_title: String,
    pub notes: Vec<TemplateUpdateNoteVm>,
    /// Why there is no offer, as a finished sentence composed in Rust.
    pub declined: Option<String>,
}

/// One note's accepted changes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TemplateUpdateSelectionVm {
    pub note_id: String,
    /// [`TemplateChangeVm::index`] values. A note whose list is empty, or which
    /// is absent from the request, is not touched.
    pub changes: Vec<u32>,
}

/// What the user accepted.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TemplateUpdateApplyReq {
    pub template_path: String,
    pub selections: Vec<TemplateUpdateSelectionVm>,
}

/// One note that was updated, and the revision that undoes it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TemplateUpdateAppliedVm {
    pub note_id: String,
    pub title: String,
    /// How many changes landed.
    pub applied: u32,
    /// The commit holding the note exactly as it was a moment ago — the same
    /// revision `notes_history` lists and `notes_restore_revision` writes back.
    /// This is the undo, and it is the existing history rather than a private
    /// one.
    pub undo_rev: String,
}

/// The result of applying (FR-163).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TemplateUpdateResultVm {
    pub updated: Vec<TemplateUpdateAppliedVm>,
    /// A finished sentence per note keeper was asked to change and did not.
    pub skipped: Vec<String>,
}

/// Project one note's plan for the preview.
#[must_use]
pub fn note_vm(plan: &NotePlan) -> TemplateUpdateNoteVm {
    TemplateUpdateNoteVm {
        note_id: plan.note_id.clone(),
        title: plan.title.clone(),
        path: plan.path.clone(),
        changes: plan
            .changes
            .iter()
            .enumerate()
            .map(|(index, change)| TemplateChangeVm {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                removed: change.removed.clone(),
                added: change.added.clone(),
                at_line: match change.outcome {
                    Outcome::Applies { at } => Some(u32::try_from(at + 1).unwrap_or(u32::MAX)),
                    Outcome::Skipped(_) => None,
                },
                skipped: match change.outcome {
                    Outcome::Applies { .. } => None,
                    Outcome::Skipped(skip) => Some(skip.sentence().to_owned()),
                },
            })
            .collect(),
        blocked: plan.blocked.clone(),
        stale_path: plan.stale_path.clone(),
    }
}

/// Assemble the offer, choosing the refusal when there is one.
///
/// The precedence is deliberate and is the whole of "never automatic": no notes,
/// then too many notes, then nothing changed, then nothing applies. Each is a
/// different fact and each gets its own sentence, because "nothing happened" is
/// the message this epic has already shipped twice by accident.
#[must_use]
pub fn offer(
    template_path: &str,
    template_title: &str,
    found: usize,
    plans: &[NotePlan],
) -> TemplateUpdateOfferVm {
    let declined = if found == 0 {
        Some(no_notes(template_title))
    } else if found > MAX_OFFER_NOTES {
        Some(too_many_notes(template_title, found))
    } else if plans.iter().all(|plan| plan.changes.is_empty()) {
        Some(nothing_changed(template_title))
    } else if !plans.iter().any(NotePlan::selectable) {
        Some(nothing_applies(template_title))
    } else {
        None
    };

    TemplateUpdateOfferVm {
        template_path: template_path.to_owned(),
        template_title: template_title.to_owned(),
        notes: if declined.is_some() {
            Vec::new()
        } else {
            plans.iter().map(note_vm).collect()
        },
        declined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::frontmatter::FieldValue;

    fn ctx() -> TemplateCtx {
        TemplateCtx {
            title: "Monday".to_owned(),
            id: "01JNOTE".to_owned(),
            now_local: "2026-08-09T09:00:00+02:00".to_owned(),
        }
    }

    fn note<'a>(id: &'a str, title: &'a str, body: &'a str) -> NoteInput<'a> {
        NoteInput {
            id,
            title,
            path: "notes/monday.md",
            body,
            ctx: ctx(),
            stale_path: None,
            recoverability: Recoverability::Committed,
        }
    }

    // -- the finder ---------------------------------------------------------

    #[test]
    fn a_note_stamped_with_this_templates_id_is_found_after_a_rename() {
        let provenance = Provenance {
            path: Some("templates/journal.md".to_owned()),
            id: Some("01JTPL".to_owned()),
        };
        // The template moved. Path no longer agrees; the id still does.
        let moved = TemplateRef {
            path: "templates/daily/journal.md",
            id: Some("01JTPL"),
        };
        assert_eq!(made_from(&provenance, &moved), Some(Matched::ById));
    }

    #[test]
    fn a_note_from_a_different_template_is_not_found_even_at_the_same_path() {
        // A template was deleted and a new one written at the same path. The
        // old notes must not become the new one's children.
        let provenance = Provenance {
            path: Some("templates/journal.md".to_owned()),
            id: Some("01JOLD".to_owned()),
        };
        let replacement = TemplateRef {
            path: "templates/journal.md",
            id: Some("01JNEW"),
        };
        assert_eq!(made_from(&provenance, &replacement), None);
    }

    #[test]
    fn a_note_with_only_a_path_is_found_by_it_and_nothing_else_is() {
        let by_path = Provenance {
            path: Some("templates/journal.md".to_owned()),
            id: None,
        };
        let template = TemplateRef {
            path: "templates/journal.md",
            id: Some("01JTPL"),
        };
        assert_eq!(made_from(&by_path, &template), Some(Matched::ByPath));

        let elsewhere = Provenance {
            path: Some("templates/recording.md".to_owned()),
            id: None,
        };
        assert_eq!(made_from(&elsewhere, &template), None);
        assert_eq!(made_from(&Provenance::default(), &template), None);
    }

    #[test]
    fn provenance_reads_back_out_of_the_index_exactly_as_it_went_in() {
        // The round trip that matters: what the reconciler flattens is what the
        // finder reads. Built through the real renderer, never a hand-written
        // string, so a change to `index_string` breaks this and not production.
        let map = FieldValue::Map(templates::provenance_pairs(
            "templates/journal.md",
            Some("01JTPL"),
        ));
        let mut fields = BTreeMap::new();
        fields.insert(keeper_field().to_owned(), map.index_string());

        assert_eq!(
            provenance_from_index(&fields),
            Provenance {
                path: Some("templates/journal.md".to_owned()),
                id: Some("01JTPL".to_owned()),
            }
        );
    }

    #[test]
    fn an_index_entry_with_no_keeper_map_has_no_provenance() {
        let mut fields = BTreeMap::new();
        fields.insert("tags".to_owned(), "journal\ndraft".to_owned());
        assert!(provenance_from_index(&fields).is_empty());

        // A user's own `keeper:` map that says nothing about templates.
        let mut theirs = BTreeMap::new();
        theirs.insert(keeper_field().to_owned(), "colour: blue".to_owned());
        assert!(provenance_from_index(&theirs).is_empty());
    }

    #[test]
    fn the_index_key_is_the_reserved_prefix_without_its_dot() {
        assert_eq!(keeper_field(), "keeper");
        assert_eq!(format!("{}.", keeper_field()), RESERVED_FIELD_PREFIX);
    }

    // -- the diff -----------------------------------------------------------

    #[test]
    fn an_unchanged_template_produces_no_changes() {
        assert!(changes("# Day\n\n## Notes\n", "# Day\n\n## Notes\n").is_empty());
    }

    #[test]
    fn an_appended_section_is_one_insertion_anchored_above() {
        let found = changes("# Day\n\n## Notes\n", "# Day\n\n## Notes\n\n## Actions\n");
        assert_eq!(found.len(), 1);
        assert!(found[0].removed.is_empty());
        assert_eq!(found[0].added, vec!["", "## Actions"]);
        assert!(found[0].before.contains(&"## Notes".to_owned()));
    }

    #[test]
    fn a_rewritten_heading_is_a_replacement_carrying_the_old_text() {
        let found = changes("# Day\n## Notes\n", "# Day\n## Observations\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].removed, vec!["## Notes".to_owned()]);
        assert_eq!(found[0].added, vec!["## Observations".to_owned()]);
    }

    // -- the rule, arm by arm ----------------------------------------------

    #[test]
    fn an_untouched_note_takes_every_change() {
        let old = "# Day\n\n## Notes\n";
        let new = "# Day\n\n## Notes\n\n## Actions\n";
        let plan = plan_note(old, new, &note("n1", "Monday", "# Day\n\n## Notes\n"));

        assert_eq!(plan.changes.len(), 1);
        assert!(plan.changes[0].appliable());
        assert_eq!(
            apply("# Day\n\n## Notes\n", &plan, &[0]).as_deref(),
            Some("# Day\n\n## Notes\n\n## Actions\n")
        );
    }

    #[test]
    fn a_note_written_in_since_keeps_what_was_written_and_still_gains_the_section() {
        // The case the whole story is about: the author has written below the
        // template's last line.
        let old = "# Day\n\n## Notes\n";
        let new = "# Day\n\n## Notes\n\n## Actions\n";
        let body = "# Day\n\n## Notes\nrang the bank, they will call back\nbought bread\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        // The WHOLE document, not a `contains`: where the new section lands is
        // the load-bearing half of this decision and a containment assertion
        // cannot see it. Anchoring literally would put `## Actions` on line 4 —
        // above "rang the bank" — and silently re-file both of the author's
        // lines under a heading they have never seen. Nothing they wrote is
        // removed either way, which is exactly why the position has to be
        // asserted rather than inferred from survival.
        assert_eq!(
            apply(body, &plan, &[0]).as_deref(),
            Some(
                "# Day\n\n## Notes\nrang the bank, they will call back\nbought bread\n\n## Actions\n"
            )
        );
    }

    #[test]
    fn a_blank_line_is_not_an_anchor_even_when_it_is_the_only_unique_one() {
        // The template gained a line in its middle. In this note the author has
        // rewritten the text on both sides of it, so the only context left that
        // matches anything is a single empty line — which happens to occur
        // exactly once.
        let old = "intro\n\nend\n";
        let new = "intro\n\nmiddle\nend\n";
        let body = "hello\n\nbye\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        // Unique is not the same as meaningful. Placing a change against a blank
        // line is a coin toss with a straight face: here it would drop `middle`
        // above `bye` on no evidence at all.
        assert_eq!(plan.changes[0].outcome, Outcome::Skipped(Skip::Diverged));
        assert_eq!(apply(body, &plan, &[0]), None);

        // And the same change DOES land when a real anchor survives, so the rule
        // above is about blankness and not about this shape of change.
        let kept = "intro\n\nbye\n";
        let plan = plan_note(old, new, &note("n1", "Monday", kept));
        assert_eq!(
            apply(kept, &plan, &[0]).as_deref(),
            Some("intro\n\nmiddle\nbye\n")
        );
    }

    #[test]
    fn a_change_over_text_the_author_rewrote_is_skipped_and_says_why() {
        let old = "# Day\n\n## Notes\n";
        let new = "# Day\n\n## Observations\n";
        // The author renamed that heading themselves.
        let body = "# Day\n\n## What happened\nlong day\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        assert_eq!(plan.changes.len(), 1);
        assert_eq!(
            plan.changes[0].outcome,
            Outcome::Skipped(Skip::Diverged),
            "the note no longer says what the template said"
        );
        assert_eq!(apply(body, &plan, &[0]), None, "and nothing is written");
    }

    #[test]
    fn a_change_that_could_land_in_two_places_is_refused_rather_than_guessed() {
        let old = "## Notes\n";
        let new = "## Notes\nremember to date this\n";
        // `## Notes` twice: the anchor is real but not unique.
        let body = "## Notes\nmorning\n\n## Notes\nafternoon\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        assert_eq!(plan.changes[0].outcome, Outcome::Skipped(Skip::Ambiguous));
        assert_eq!(apply(body, &plan, &[0]), None);
    }

    #[test]
    fn a_deletion_only_ever_removes_the_templates_own_line() {
        let old = "# Day\n\nTODO: fill this in\n\n## Notes\n";
        let new = "# Day\n\n## Notes\n";
        let body = "# Day\n\nTODO: fill this in\n\n## Notes\nmy own writing\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        let updated = apply(body, &plan, &[0]).expect("the scaffold line goes");
        assert!(!updated.contains("TODO: fill this in"));
        assert!(updated.contains("my own writing"));
    }

    #[test]
    fn a_note_that_rewrote_the_line_a_deletion_targets_keeps_it() {
        let old = "# Day\n\nTODO: fill this in\n";
        let new = "# Day\n";
        // The author turned the placeholder into their own sentence.
        let body = "# Day\n\nTODO: ring the dentist\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        assert_eq!(plan.changes[0].outcome, Outcome::Skipped(Skip::Diverged));
        assert_eq!(apply(body, &plan, &[0]), None);
        // Belt and braces: the author's line is not in any `removed` set, so no
        // selection at all could take it out.
        assert!(!plan.changes[0]
            .removed
            .iter()
            .any(|line| line.contains("dentist")));
    }

    // -- declining ----------------------------------------------------------

    #[test]
    fn declining_returns_no_text_to_write_at_all() {
        let old = "# Day\n";
        let new = "# Day\n\n## Actions\n";
        let body = "# Day\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        assert!(plan.changes[0].appliable(), "it would have applied");
        assert_eq!(apply(body, &plan, &[]), None, "but nothing was accepted");
    }

    /// The promise of this module, checked over a table rather than asserted in
    /// prose: **a change can only ever delete lines the template itself wrote**.
    ///
    /// If this ever fails, some anchor started matching text the user typed, and
    /// the whole "impossible to trigger by accident" claim is void.
    #[test]
    fn no_change_ever_proposes_removing_a_line_the_template_did_not_write() {
        let cases: [(&str, &str, &str); 6] = [
            (
                "# Day\n\n## Notes\n",
                "# Day\n\n## Actions\n",
                "# Day\n\nmine\n",
            ),
            ("# Day\n", "", "# Day\nmine\n"),
            ("a\nb\nc\n", "c\nb\na\n", "a\nb\nmine\nc\n"),
            ("## Notes\n", "## Notes\nx\n", "## Notes\n## Notes\n"),
            (
                "",
                "everything\n",
                "a note that came from an empty template\n",
            ),
            (
                "# {{date:YYYY-MM-DD}}\n\n## Notes\n",
                "# {{date:YYYY-MM-DD}}\n\n## Log\n",
                "# 2026-08-09\n\nmine\n",
            ),
        ];

        for (old, new, body) in cases {
            let plan = plan_note(old, new, &note("n1", "Monday", body));
            let template_lines: Vec<String> = templates::expand(old, &ctx())
                .body
                .split('\n')
                .map(str::to_owned)
                .collect();
            for change in &plan.changes {
                for line in &change.removed {
                    assert!(
                        template_lines.contains(line),
                        "{line:?} is not the template's, from {old:?} -> {new:?}"
                    );
                }
            }

            // And the write itself: every line of the note that the template did
            // not contribute is still there afterwards, in order.
            let every: Vec<usize> = (0..plan.changes.len()).collect();
            if let Some(updated) = apply(body, &plan, &every) {
                let survivors: Vec<&str> = updated.split('\n').collect();
                let mut at = 0usize;
                for line in body.split('\n') {
                    if template_lines.iter().any(|known| known == line) {
                        continue;
                    }
                    let found = survivors[at..].iter().position(|kept| *kept == line);
                    let offset = found.unwrap_or_else(|| {
                        panic!("{line:?} was lost from {body:?} -> {updated:?}")
                    });
                    at += offset + 1;
                }
            }
        }
    }

    #[test]
    fn a_blocked_note_writes_nothing_for_any_selection() {
        let mut input = note("n1", "Monday", "# Day\n");
        input.recoverability = Recoverability::Untracked;
        let plan = plan_note("# Day\n", "# Day\n## A\n## B\n", &input);

        // Every subset of every change, including the full one.
        for accepted in [vec![], vec![0], vec![0, 0], vec![0, 1, 2]] {
            assert_eq!(
                apply("# Day\n", &plan, &accepted),
                None,
                "selection {accepted:?} still wrote something"
            );
        }
    }

    #[test]
    fn accepting_a_change_that_cannot_land_writes_nothing() {
        let old = "## Notes\n";
        let new = "## Notes\nnew line\n";
        let body = "something else entirely\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        assert_eq!(apply(body, &plan, &[0]), None);
    }

    #[test]
    fn an_out_of_range_selection_is_ignored_rather_than_panicking() {
        let plan = plan_note(
            "# Day\n",
            "# Day\n## Actions\n",
            &note("n1", "M", "# Day\n"),
        );
        assert_eq!(apply("# Day\n", &plan, &[99]), None);
    }

    // -- recoverability -----------------------------------------------------

    #[test]
    fn a_note_the_vault_has_not_committed_cannot_be_updated() {
        for state in [Recoverability::Modified, Recoverability::Untracked] {
            let mut input = note("n1", "Monday", "# Day\n");
            input.recoverability = state;
            let plan = plan_note("# Day\n", "# Day\n## Actions\n", &input);

            let blocked = plan.blocked.as_deref().expect("blocked");
            assert!(blocked.contains("Monday"), "the sentence names the note");
            assert!(!plan.selectable());
            // The changes are still visible, so the user can see what waiting buys.
            assert!(plan.changes[0].appliable());
            // And the write is impossible even if the caller asks for it.
            assert_eq!(apply("# Day\n", &plan, &[0]), None);
        }
    }

    #[test]
    fn a_committed_note_is_not_blocked() {
        let plan = plan_note(
            "# Day\n",
            "# Day\n## Actions\n",
            &note("n1", "M", "# Day\n"),
        );
        assert_eq!(plan.blocked, None);
        assert!(plan.selectable());
    }

    // -- placeholders -------------------------------------------------------

    #[test]
    fn a_placeholder_is_compared_and_written_as_this_note_has_it() {
        // The template's date placeholder resolved to the note's creation date
        // when the note was made, so that is what the note contains — and that
        // is what has to be matched and what has to be written.
        let old = "# {{date:YYYY-MM-DD}}\n\n## Notes\n";
        let new = "# {{date:YYYY-MM-DD}}\n\n## Notes\n\n## On {{date:YYYY-MM-DD}}\n";
        let body = "# 2026-08-09\n\n## Notes\nwrote this\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        let updated = apply(body, &plan, &[0]).expect("it lands");
        assert!(
            updated.contains("## On 2026-08-09"),
            "the placeholder resolved, got: {updated}"
        );
        assert!(
            !updated.contains("{{date"),
            "no placeholder is ever written into a note"
        );
    }

    #[test]
    fn an_edit_to_only_the_templates_own_frontmatter_changes_nothing() {
        let old = "---\nid: 01JTPL\ntags: [template]\n---\n# Day\n";
        let new = "---\nid: 01JTPL\ntags: [template, journal]\nupdated: 2026-08-09\n---\n# Day\n";
        let plan = plan_note(old, new, &note("n1", "Monday", "# Day\n"));

        assert!(
            plan.changes.is_empty(),
            "the body is identical; got {:?}",
            plan.changes
        );
    }

    // -- shapes that must not corrupt --------------------------------------

    #[test]
    fn a_crlf_note_keeps_its_line_endings() {
        let old = "# Day\n\n## Notes\n";
        let new = "# Day\n\n## Notes\n\n## Actions\n";
        let body = "# Day\r\n\r\n## Notes\r\nwrote this\r\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        let updated = apply(body, &plan, &[0]).expect("it lands through CRLF");
        assert!(updated.contains("## Actions\r\n"), "got: {updated:?}");
        assert!(
            !updated.contains("\n\n\n"),
            "no bare LF was introduced: {updated:?}"
        );
        assert!(updated.contains("wrote this\r\n"));
    }

    #[test]
    fn a_note_with_no_trailing_newline_does_not_gain_one() {
        let old = "# Day";
        let new = "# Day\n## Actions";
        let body = "# Day";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        assert_eq!(
            apply(body, &plan, &[0]).as_deref(),
            Some("# Day\n## Actions")
        );
    }

    #[test]
    fn two_accepted_changes_both_land_at_the_right_lines() {
        let old = "# Day\n\n## Notes\n\n## Later\n";
        let new = "# Day\nwhat happened\n\n## Notes\n\n## Later\nand then\n";
        let body = "# Day\n\n## Notes\n\n## Later\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        assert_eq!(plan.changes.len(), 2);
        let indices: Vec<usize> = (0..plan.changes.len()).collect();
        assert_eq!(
            apply(body, &plan, &indices).as_deref(),
            Some("# Day\nwhat happened\n\n## Notes\n\n## Later\nand then\n")
        );
    }

    #[test]
    fn one_of_two_changes_can_be_accepted_on_its_own() {
        let old = "# Day\n\n## Notes\n\n## Later\n";
        let new = "# Day\nwhat happened\n\n## Notes\n\n## Later\nand then\n";
        let body = "# Day\n\n## Notes\n\n## Later\n";
        let plan = plan_note(old, new, &note("n1", "Monday", body));

        assert_eq!(
            apply(body, &plan, &[1]).as_deref(),
            Some("# Day\n\n## Notes\n\n## Later\nand then\n")
        );
    }

    #[test]
    fn an_insertion_with_nothing_to_anchor_it_is_refused() {
        // The old template was empty, so there is no template text in the note
        // to position anything against.
        let plan = plan_note("", "## Actions\n", &note("n1", "Monday", "my own note\n"));
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].outcome, Outcome::Skipped(Skip::Unanchored));
        assert_eq!(apply("my own note\n", &plan, &[0]), None);
    }

    #[test]
    fn a_template_past_the_diff_bound_degrades_to_one_whole_replacement() {
        let old: String = (0..MAX_DIFF_LINES + 5)
            .map(|n| format!("line {n}\n"))
            .collect();
        let new = format!("{old}extra\n");
        let found = changes(&old, &new);

        assert_eq!(found.len(), 1);
        assert!(found[0].before.is_empty() && found[0].after.is_empty());

        // Degraded, not unsafe. The `removed` set is the whole old template, so
        // the note must still hold every line of it, contiguously and once.
        let plan = plan_note(&old, &new, &note("n1", "Monday", &old));
        assert!(plan.changes[0].appliable());

        // Written under it: the old template is still intact above, so it lands
        // and the author's line is untouched.
        let below = format!("{old}mine\n");
        let plan = plan_note(&old, &new, &note("n1", "Monday", &below));
        let updated = apply(&below, &plan, &[0]).expect("the tail is still intact");
        assert!(updated.contains("\nmine\n"));
        assert!(updated.contains("\nextra\n"));

        // Written INTO it: the old template is no longer contiguous, so there is
        // no safe splice and keeper says so instead of rebuilding the note.
        let within = old.replacen("line 3\n", "line 3\nmine\n", 1);
        let plan = plan_note(&old, &new, &note("n1", "Monday", &within));
        assert_eq!(plan.changes[0].outcome, Outcome::Skipped(Skip::Diverged));
        assert_eq!(apply(&within, &plan, &[0]), None);
    }

    // -- the offer ----------------------------------------------------------

    #[test]
    fn each_reason_for_declining_gets_its_own_sentence() {
        let empty: Vec<NotePlan> = Vec::new();
        assert!(offer("templates/j.md", "Journal", 0, &empty)
            .declined
            .expect("no notes")
            .contains("No note in this vault"));

        let plan = plan_note("# Day\n", "# Day\n", &note("n1", "M", "# Day\n"));
        let unchanged = offer("templates/j.md", "Journal", 1, std::slice::from_ref(&plan));
        assert!(unchanged
            .declined
            .expect("nothing changed")
            .contains("Nothing in"));
        assert!(unchanged.notes.is_empty(), "a refusal offers no notes");

        let diverged = plan_note(
            "## Notes\n",
            "## Notes\nx\n",
            &note("n1", "M", "different\n"),
        );
        assert!(offer("templates/j.md", "Journal", 1, &[diverged])
            .declined
            .expect("nothing applies")
            .contains("none of them still match"));

        let live = plan_note("# Day\n", "# Day\n## A\n", &note("n1", "M", "# Day\n"));
        let real = offer("templates/j.md", "Journal", 1, &[live]);
        assert_eq!(real.declined, None);
        assert_eq!(real.notes.len(), 1);
    }

    #[test]
    fn more_notes_than_keeper_will_show_is_a_refusal_that_names_the_count() {
        let plan = plan_note("# Day\n", "# Day\n## A\n", &note("n1", "M", "# Day\n"));
        let declined = offer(
            "templates/j.md",
            "Journal",
            MAX_OFFER_NOTES + 1,
            std::slice::from_ref(&plan),
        )
        .declined
        .expect("too many");
        assert!(declined.contains(&(MAX_OFFER_NOTES + 1).to_string()));

        // Exactly at the bound is still offered.
        let at_bound = offer("templates/j.md", "Journal", MAX_OFFER_NOTES, &[plan]);
        assert_eq!(at_bound.declined, None);
    }

    #[test]
    fn the_preview_says_where_each_change_lands_and_why_the_others_do_not() {
        // Two changes: the template renamed a heading, and it appended a line.
        let old = "# Day\n\n## Notes\n\n## Later\n";
        let new = "# Day\n\n## Observations\n\n## Later\nand then\n";
        // The author renamed that same heading themselves, and wrote under it.
        let body = "# Day\n\n## What I saw\nmine\n\n## Later\n";
        let vm = note_vm(&plan_note(old, new, &note("n1", "Monday", body)));

        assert_eq!(vm.changes.len(), 2);

        let landed: Vec<&TemplateChangeVm> =
            vm.changes.iter().filter(|c| c.skipped.is_none()).collect();
        assert_eq!(landed.len(), 1, "the append lands, the rename does not");
        assert_eq!(landed[0].added, vec!["and then".to_owned()]);
        assert_eq!(landed[0].removed, Vec::<String>::new());
        assert_eq!(
            landed[0].at_line,
            Some(7),
            "1-based, at the end of a six-line note"
        );

        let refused: Vec<&TemplateChangeVm> =
            vm.changes.iter().filter(|c| c.skipped.is_some()).collect();
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].removed, vec!["## Notes".to_owned()]);
        assert_eq!(refused[0].at_line, None);
        assert_eq!(
            refused[0].skipped.as_deref(),
            Some(Skip::Diverged.sentence()),
            "and the preview says which of the three reasons it was"
        );
    }

    #[test]
    fn a_renamed_template_is_reported_on_the_note_and_the_note_is_untouched_otherwise() {
        let mut input = note("n1", "Monday", "# Day\n");
        input.stale_path = Some("templates/old-journal.md".to_owned());
        let vm = note_vm(&plan_note("# Day\n", "# Day\n## A\n", &input));

        assert_eq!(vm.stale_path.as_deref(), Some("templates/old-journal.md"));
    }
}
