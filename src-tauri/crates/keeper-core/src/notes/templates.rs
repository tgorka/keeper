//! What a template is, and what applying one does (FR-100, FR-161, FR-162, AD-82).
//!
//! **A template is a note tagged `template`.** Not a file type, not a directory
//! keeper owns. That is AD-82, and it is what makes a template searchable,
//! syncable, and editable with the tools that already exist — the same editor,
//! the same tag tree, the same sync engine. [`is_template`] is the one predicate
//! that decides, so the surface that lists templates and the code that strips
//! the marker cannot disagree about what one is.
//!
//! **Applying a template copies its BODY and drops that one tag.** The copy is
//! not a template; leaving the tag on would make every note a template of
//! itself, and the next note made from the copy would inherit the marker again.
//! Every *other* tag rides along, because a journal template tagged
//! `journal, daily` exists precisely so the notes it makes are tagged
//! `journal, daily`.
//!
//! ## Two spellings, one renderer
//!
//! Placeholders come in two shapes, and this is the only file that resolves
//! either:
//!
//! | Shape | Set | Where it came from |
//! |-------|-----|--------------------|
//! | `{{date:FMT}}`, `{{time:FMT}}`, `{{title}}`, `{{cursor}}`, `{{id}}` | moment tokens | Obsidian's own Templates plugin — a template authored in Obsidian must keep working here |
//! | `{yyyy}`, `{yy}`, `{mm}`, `{dd}`, `{HH}`, `{MM}`, `{SS}` | fixed | the recording path template and the journal path template already publish it |
//!
//! The second row is the reason this module grew rather than a second one being
//! written. A user who has learned `{yyyy}/{mm}/{dd}` from the recording
//! destination field has learned the vocabulary of this app, and a body that
//! refused it would be teaching them the app has two. The path renderers
//! ([`crate::notes::naming::journal_path`] and
//! [`crate::recording::path_template`]) could not be called here: both sanitise
//! their output into a *path* — dropping `..`, folding separators, appending
//! `.md`, collapsing empty folder components — which is correct for a filename
//! and destroys a document. So the vocabulary is shared and the renderer is
//! this one.
//!
//! **`{mm}` is the month and `{MM}` is the minute; inside `{{date:…}}` it is the
//! other way round**, because moment defines `MM` as the month and `mm` as the
//! minute. That collision is not invented here — it is the existing state of
//! both vocabularies, documented at [`crate::recording::path_template`] — but
//! this is the one file where both are in scope at once, so it is worth reading
//! twice.
//!
//! ## Why the set is closed
//!
//! Everything unrecognised is left exactly as written. That is not laziness:
//! templates are ordinary notes, they sync, and an agent may write one. A
//! template engine that evaluates arbitrary expressions in a file an agent can
//! author is a code-execution surface, and one that *guesses* at unknown braces
//! silently eats a user's literal `{{TODO}}` or the `{n}` in their maths.
//! Leaving the unknown alone is the only option that is both safe and honest.
//!
//! Dates come from `ctx.now_local`, an RFC 3339 string the shell already has —
//! keeper-core carries no clock, and a pure function that reads the wall clock
//! could not be tested.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::notes::default_spaces;
use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::naming;
use crate::notes::tags;

/// What a template is allowed to know about the note being created.
#[derive(Debug, Clone, Default)]
pub struct TemplateCtx {
    /// The note's title, as `{{title}}`.
    pub title: String,
    /// The note's ULID, as `{{id}}`.
    pub id: String,
    /// Local wall-clock time as RFC 3339, e.g. `2026-08-02T14:35:09+02:00`.
    /// Local, not UTC: a journal entry written at 00:30 belongs to the day the
    /// writer thinks it is.
    pub now_local: String,
}

/// Expand a template's text, returning it and the byte offset the editor should
/// place the caret at.
///
/// **Text, not a note**: this is the string-level renderer, and it neither knows
/// nor cares whether what it was handed had a frontmatter block. [`expand`] is
/// the note-level entry point and the one a create path should call; this stays
/// public because Story 44.8 re-renders a template to work out what a note would
/// have said, and a second renderer there would be a second grammar.
///
/// The cursor offset is `None` when the template has no `{{cursor}}`; the first
/// occurrence wins and any further ones are simply removed, because two carets
/// is not a thing an editor can honour.
pub fn expand_body(template: &str, ctx: &TemplateCtx) -> (String, Option<usize>) {
    let stamp = Stamp::parse(&ctx.now_local);
    let mut out = String::with_capacity(template.len());
    let mut cursor: Option<usize> = None;
    let mut rest = template;

    while let Some(open) = rest.find("{{") {
        push_literal(&mut out, &rest[..open], stamp.as_ref());
        let after = &rest[open + 2..];

        let Some(close) = after.find("}}") else {
            // An unterminated `{{` is literal text, not a broken template — and
            // verbatim, without the single-brace pass. Everything from here on
            // is inside a construct the author did not finish, and quietly
            // rewriting part of it would make the failure harder to see than
            // leaving the whole tail exactly as typed.
            out.push_str(&rest[open..]);
            return (out, cursor);
        };

        let token = &after[..close];
        match resolve(token.trim(), ctx, stamp.as_ref()) {
            Resolved::Text(text) => out.push_str(&text),
            Resolved::Cursor => {
                if cursor.is_none() {
                    cursor = Some(out.len());
                }
            }
            // Re-emit the original bytes, spacing and all — and NOT through
            // `push_literal`, or `{{yyyy}}` would come back as `{2026}`: an
            // unknown double-brace token would have its inside silently
            // rewritten by the other vocabulary, which is the opposite of
            // "unknown is left alone".
            Resolved::Unknown => {
                out.push_str("{{");
                out.push_str(token);
                out.push_str("}}");
            }
        }

        rest = &after[close + 2..];
    }

    push_literal(&mut out, rest, stamp.as_ref());
    (out, cursor)
}

/// Copy a run of ordinary text into `out`, resolving the single-brace date
/// vocabulary on the way.
///
/// Applied only to the literal runs BETWEEN `{{…}}` constructs, never inside
/// one. A `{` with no closing `}`, or a `{word}` outside the closed set, is
/// copied through untouched — a note is full of braces that are not
/// placeholders, and `{n}` in someone's maths must survive being written down.
fn push_literal(out: &mut String, chunk: &str, stamp: Option<&Stamp>) {
    let Some(stamp) = stamp else {
        // No usable timestamp: every date placeholder stays visible, exactly as
        // `{{date}}` does. A visible `{yyyy}` in a new note is a bug report; a
        // silent 1970 is not.
        out.push_str(chunk);
        return;
    };

    let mut rest = chunk;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return;
        };
        match date_token(&after[..close], stamp) {
            Some(text) => {
                out.push_str(&text);
                rest = &after[close + 1..];
            }
            // Not ours. Emit the brace and resume *after* it rather than after
            // the `}`, so the closing brace of `{a}` is still available to open
            // nothing and the `{yyyy}` in `{a{yyyy}` is still found.
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
}

/// The closed single-brace set, spelled exactly as the recording path template
/// and the journal path template spell it.
///
/// Date and time only. `{slug}`, `{title}` and `{seq}` are deliberately absent:
/// the first two already have a body spelling in `{{title}}`, and two spellings
/// of one value inside one document is the divergence this module exists to
/// avoid, while `{seq}` is a *path* concept — it disambiguates a colliding
/// folder and means nothing in a paragraph.
fn date_token(token: &str, stamp: &Stamp) -> Option<String> {
    let mut out = String::with_capacity(4);
    // `let _`, not `?`: writing into a `String` is infallible, and mapping a
    // formatting error onto `None` would spell "this is not a date token" for a
    // condition that has nothing to do with the token.
    let _ = match token {
        "yyyy" => write!(out, "{:04}", stamp.year),
        "yy" => write!(out, "{:02}", stamp.year.rem_euclid(100)),
        "mm" => write!(out, "{:02}", stamp.month),
        "dd" => write!(out, "{:02}", stamp.day),
        "HH" => write!(out, "{:02}", stamp.hour),
        "MM" => write!(out, "{:02}", stamp.minute),
        "SS" => write!(out, "{:02}", stamp.second),
        _ => return None,
    };
    Some(out)
}

// ---------------------------------------------------------------------------
// A template is a note
// ---------------------------------------------------------------------------

/// The tag that marks a note as a template (AD-82).
///
/// Normalised form, because that is the only form a tag exists in once
/// [`crate::notes::tags::normalise`] has seen it: `#Template`, `template` and
/// ` TEMPLATE ` are one tag, and comparing against anything else here would
/// make the marker case-sensitive in exactly one place.
pub const TEMPLATE_TAG: &str = "template";

/// The key inside a **space** note's reserved `keeper:` map naming the template
/// notes created in that space start from (FR-162).
///
/// `template` bare, because on a space the word is unambiguous: a space is a
/// lens, it is not made from anything, so the only thing `keeper.template` can
/// mean there is "the template this space hands out".
pub const SPACE_TEMPLATE_KEY: &str = "template";

/// The key inside a **new note's** reserved `keeper:` map recording which
/// template made it, as a vault-relative path (FR-161).
///
/// `from_template` rather than `template`, and the difference is load-bearing:
/// [`SPACE_TEMPLATE_KEY`] is the same word in the same map on a note that is
/// also a note, and one spelling meaning "hands this out" on a space and "was
/// made from this" on a note is a trap for whoever reads a vault next.
pub const FROM_TEMPLATE_KEY: &str = "from_template";

/// The template note's own `id`, recorded beside [`FROM_TEMPLATE_KEY`].
///
/// Both, and the id is the one that matters. A path breaks the moment the
/// template is renamed or moved, which in a synced vault is not hypothetical —
/// and Story 44.8's finder would then return zero notes and report success.
/// A silent nothing is the failure this epic has already shipped twice; one
/// extra line of frontmatter turns it into a hit.
pub const FROM_TEMPLATE_ID_KEY: &str = "from_template_id";

/// Which template a note was made from, as recorded in its frontmatter.
///
/// Both fields are optional and independently so: a note written before this
/// landed, or by hand, may carry the path and no id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Provenance {
    /// Vault-relative path of the template, as resolved when the note was made.
    pub path: Option<String>,
    /// The template note's `id`. Survives a rename; match on this first.
    pub id: Option<String>,
}

impl Provenance {
    /// Whether the note claims no template at all.
    pub fn is_empty(&self) -> bool {
        self.path.is_none() && self.id.is_none()
    }
}

/// Everything applying a template produces, for a caller that will write the
/// note (Story 44.6 owns the write; this owns what goes into it).
// No `Eq`: `FieldValue` carries an `f64`, so the properties a template hands
// over are only ever partially comparable.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Expanded {
    /// The template's **body**, placeholders resolved. Never its frontmatter:
    /// see [`expand`].
    pub body: String,
    /// Byte offset into `body` where the template asked for the caret.
    pub caret: Option<usize>,
    /// The template's own tags, normalised, with [`TEMPLATE_TAG`] removed.
    pub tags: Vec<String>,
    /// The template's other frontmatter properties, in its own key order, ready
    /// to merge into the new note's block *underneath* the keys keeper writes
    /// itself. See [`expand`] for the five that never cross.
    pub properties: Vec<(String, FieldValue)>,
    /// The template note's `id`, for [`provenance_pairs`].
    pub source_id: Option<String>,
}

/// Is this note a template? (AD-82.)
///
/// The **frontmatter** tag, not an inline `#template` in the body. Deliberate:
/// the body is copied verbatim into every note made from the template, so an
/// inline marker would ride along and make each copy a template of itself —
/// which is the exact failure AD-82 names. Keeping the marker in frontmatter is
/// what lets [`expand`] drop it by not copying a property, instead of editing
/// somebody's prose on the way past.
///
/// A note *about* templates that mentions `#template` in a sentence is therefore
/// a note, which is also the right answer.
pub fn is_template(fm: &Frontmatter) -> bool {
    frontmatter_tags(fm).iter().any(|tag| tag == TEMPLATE_TAG)
}

/// Apply a template note to a new note.
///
/// `source` is the template note's whole file. What crosses over is the body,
/// the tags, and the template's other frontmatter **properties** — a template
/// that declares `status: draft` and `project:` exists so the notes it makes
/// declare them too, and that properties block is what the note renders as a
/// header in keeper and in Obsidian alike.
///
/// Five keys never cross, and each for its own reason:
///
/// - `id`, `created`, `updated` are the TEMPLATE's identity and history. A copy
///   carrying them would claim to be the template, and two notes sharing an
///   `id` is how a pin, an unread mark and a sync conflict land on the wrong
///   file.
/// - `title` would name every note after the template. `Daily Template` is the
///   scaffold's name, never the name of Tuesday.
/// - `keeper` is the reserved bookkeeping map. Copying it would carry
///   [`SPACE_TEMPLATE_KEY`] onto an ordinary note, and it is also where the
///   caller writes [`provenance_pairs`] — a copied one would be overwritten or,
///   worse, kept, leaving a note claiming the template's provenance instead of
///   its own.
///
/// This is the fix for a defect that made the feature unusable: the create path
/// used to hand the whole file to [`expand_body`], so a templated note was
/// written with a literal `---\nid: …\ntags: [template]\n---` block pasted into
/// its body, underneath the real frontmatter. Nobody could have used templates
/// and not noticed, which is why nobody had.
///
/// Total. A template with no frontmatter, an empty template, and a template
/// whose frontmatter keeper cannot parse all expand to something writable —
/// losing a thought over a malformed scaffold is the wrong trade.
pub fn expand(source: &str, ctx: &TemplateCtx) -> Expanded {
    let (fm, body_offset) = Frontmatter::parse(source);
    let (body, caret) = expand_body(source.get(body_offset..).unwrap_or(""), ctx);
    Expanded {
        body,
        caret,
        tags: copied_tags(&fm),
        properties: copied_properties(&fm),
        source_id: fm.as_string("id").map(str::to_owned),
    }
}

/// Frontmatter keys that never cross into a copy as a *property*.
/// See [`expand`] for what each of the first five would break.
///
/// `tags` is here for a different reason and it is the sharpest of the six: the
/// tags cross over, but through [`Expanded::tags`], where [`TEMPLATE_TAG`] is
/// stripped. Leaving `tags` in the properties as well would hand the caller the
/// raw list a second time — marker included — and the copy would be a template
/// after all, which is the single thing AD-82 says must not happen. A test
/// caught exactly that.
const PRIVATE_KEYS: [&str; 6] = ["id", "created", "updated", "title", "keeper", "tags"];

/// The tags a copy inherits: every one the template carries except the marker.
///
/// Only the exact `template` tag leaves. A `template/daily` is left alone —
/// it is somebody's own filing under a word keeper happens to reserve at the
/// root, and [`is_template`] does not treat it as a marker either, so removing
/// it would be this module deleting a tag that was never doing anything.
fn copied_tags(fm: &Frontmatter) -> Vec<String> {
    let mut tags = frontmatter_tags(fm);
    tags.retain(|tag| tag != TEMPLATE_TAG);
    tags
}

/// The properties a copy inherits, in the template's own key order.
///
/// Source order, not sorted: Obsidian renders properties in the order the file
/// lists them, so a template author who put `project` above `status` arranged
/// that on purpose.
///
/// A key whose value the parser could not model is skipped rather than guessed
/// at — [`Frontmatter::get`] returns `None` for it, and inventing a value for a
/// construct keeper does not understand would put something in the copy that
/// was never in the template.
fn copied_properties(fm: &Frontmatter) -> Vec<(String, FieldValue)> {
    fm.keys()
        .filter(|key| !PRIVATE_KEYS.contains(key))
        .filter_map(|key| Some((key.to_owned(), fm.get(key)?.clone())))
        .collect()
}

/// A note's frontmatter tags, through the one normalisation rule (Story 42.5).
///
/// Frontmatter only — [`crate::notes::tags::note_tags`] unions in the body's
/// inline tags, which is right for the index and wrong here: the body is
/// already being copied, so an inline tag arrives in the copy by itself and
/// adding it to the copy's `tags:` property would file it twice.
fn frontmatter_tags(fm: &Frontmatter) -> Vec<String> {
    let raw = fm.as_list("tags").unwrap_or_default();
    tags::normalise_all(raw.iter().map(String::as_str))
}

/// The frontmatter pairs recording which template made a note, ready to merge
/// into the note's reserved `keeper:` map.
///
/// A `Vec` rather than two `Option`s so the caller merges one thing and decides
/// nothing; empty when there is no template, which is the ordinary case.
pub fn provenance_pairs(rel: &str, source_id: Option<&str>) -> Vec<(String, FieldValue)> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Vec::new();
    }
    let mut pairs = vec![(
        FROM_TEMPLATE_KEY.to_owned(),
        FieldValue::Str(rel.to_owned()),
    )];
    if let Some(id) = source_id.map(str::trim).filter(|id| !id.is_empty()) {
        pairs.push((
            FROM_TEMPLATE_ID_KEY.to_owned(),
            FieldValue::Str(id.to_owned()),
        ));
    }
    pairs
}

/// Read a note's template provenance back out (Story 44.8).
///
/// Total: a note with no frontmatter, no `keeper:` map, or a `keeper:` value
/// that is not a map has no provenance rather than an error. 44.8 runs this over
/// every note in a vault, and a vault where one malformed note aborts the scan
/// is a vault where the feature never runs.
pub fn provenance(source: &str) -> Provenance {
    let (fm, _) = Frontmatter::parse(source);
    let Some(FieldValue::Map(pairs)) = fm.get("keeper") else {
        return Provenance::default();
    };
    let mut found = Provenance::default();
    for (key, value) in pairs {
        let FieldValue::Str(text) = value else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        match key.as_str() {
            FROM_TEMPLATE_KEY => found.path = Some(text.to_owned()),
            FROM_TEMPLATE_ID_KEY => found.id = Some(text.to_owned()),
            _ => {}
        }
    }
    found
}

/// The template a space hands to notes created in it, or `None` (FR-162).
///
/// Pure, over the space note's frontmatter, so the shell reads the file and this
/// decides what it says. Empty and absent are one state: a user who cleared the
/// field means "no template", not "a template whose path is nothing".
pub fn space_default_template(fm: &Frontmatter) -> Option<String> {
    let FieldValue::Map(pairs) = fm.get("keeper")? else {
        return None;
    };
    pairs.iter().find_map(|(key, value)| {
        if key != SPACE_TEMPLATE_KEY {
            return None;
        }
        let FieldValue::Str(path) = value else {
            return None;
        };
        let path = path.trim();
        (!path.is_empty()).then(|| path.to_owned())
    })
}

/// What the user is told when the template a create asked for is not there.
///
/// Composed here, as a finished sentence, because this crate is where the fact
/// is known — and because the note IS still created: the sentence has to say
/// both halves or it reads as a failure. A code for TypeScript to turn into
/// words could not name the path, and naming the path is the whole value.
pub fn missing_template_notice(named: &str) -> String {
    format!(
        "The template \"{}\" is not in this vault, so this note was created without it. \
Check the space's template setting, or restore the template and create the note again.",
        named.trim()
    )
}

// ---------------------------------------------------------------------------
// The templates keeper ships
// ---------------------------------------------------------------------------

/// The vault-relative directory keeper writes its own templates into.
///
/// A **default**, not a rule. AD-82 puts the marker in the tag precisely so a
/// template can live anywhere the user likes; this is only where keeper puts the
/// three it ships, so they land somewhere findable instead of in the vault root.
/// Moving one out of here does not stop it being a template.
pub const TEMPLATES_DIR: &str = "templates";

/// Where the template seed ledger lives, vault-relative.
///
/// Its own file rather than a key inside `.keeper-spaces.json`: the two seeds
/// answer different questions ("has this vault been offered the default spaces"
/// and "…the default templates"), they were added in different releases, and a
/// vault seeded by the older build has the spaces ledger and no templates
/// ledger — which is exactly the state that must read as "offer the templates",
/// and could not if one absent file had to mean two things.
///
/// Same reasoning as [`crate::notes::default_spaces::LEDGER_REL`] for the rest:
/// in the vault so it syncs, dot-prefixed so Obsidian hides it, not a `.md` so
/// the note walk never collects it.
pub const TEMPLATE_LEDGER_REL: &str = ".keeper-templates.json";

/// One template keeper ships.
///
/// `key` is the identity and the only field the user cannot change: the moment
/// the note exists its name, its tags and every byte of its body are theirs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultTemplate {
    /// The stable identity, recorded in the ledger.
    pub key: &'static str,
    /// The note's title, which is also its filename stem.
    pub name: &'static str,
    /// The body, before expansion.
    pub body: &'static str,
}

/// The three keeper ships, for the three spaces that most want one.
///
/// **None of them tags the notes it makes**, and that is a decision rather than
/// an omission. Each of the three spaces selects on something that is not a tag:
/// Inbox is `is:untagged`, Journal is the `journal/` path, and Recordings is the
/// `session:` frontmatter key. An Inbox template that helpfully tagged its notes
/// `inbox` would file every one of them straight OUT of the Inbox — the space
/// that offered the template would be the one space the note could not appear
/// in. So the shipped templates carry `template` and nothing else, and the tags
/// a note gets come from the space it was created in (Story 44.6).
///
/// **What the bodies use is what both renderers draw**: ATX headings, an aligned
/// GFM table, task lists and paragraphs. No callouts. keeper's live preview
/// (`src/components/notes/editor/live-preview.ts`) styles `Blockquote` and knows
/// nothing of `> [!note]`, so a callout would render in Obsidian and show a
/// literal `[!note]` in the app that wrote it — and a shipped template that
/// looks broken in keeper is worse than a plain one.
///
/// The tables are aligned to the same rule the toolbar's builder uses (every
/// cell padded to its column's widest), because this vault is also read in
/// `git diff` and in Obsidian's source mode.
///
/// **No placeholder goes inside a table cell**, and that rule was learned here:
/// the journal's Log table first read `| {HH}:{MM} |`, which is nine source
/// characters padded to a nine-wide column and five characters once expanded.
/// The template file looked aligned and every note made from it did not. A
/// placeholder's rendered width is not its source width, so a cell holding one
/// can be aligned in the scaffold or in the note but never in both — and the
/// note is the artefact somebody reads. `a_shipped_templates_tables_are_aligned_after_expansion`
/// is the assertion that keeps it true.
pub const DEFAULT_TEMPLATES: [DefaultTemplate; 3] = [
    DefaultTemplate {
        key: "inbox",
        name: "Inbox note",
        // Deliberately the shortest of the three. An inbox exists to catch a
        // thought before it is gone; a scaffold with six sections to delete is
        // friction at the one moment friction costs the thought.
        body: "# {{title}}\n\n{{cursor}}\n\n## Next\n\n- [ ]\n",
    },
    DefaultTemplate {
        key: "journal",
        name: "Journal entry",
        // The heading is the date in the single-brace vocabulary, so a journal
        // entry opens with the day it is about even before anyone types.
        body: "# {yyyy}-{mm}-{dd}\n\
\n\
## Focus\n\
\n\
{{cursor}}\n\
\n\
## Log\n\
\n\
| Time | What |\n\
| ---- | ---- |\n\
|      |      |\n\
\n\
## Carried forward\n\
\n\
- [ ]\n",
    },
    DefaultTemplate {
        key: "recording",
        name: "Recording notes",
        // For a note *about* a recording. It does not pretend to be a recording
        // note: that is the `session:` key keeper writes when a capture stops
        // (Story 42.4), and a template cannot forge one — nor should it, or the
        // Recordings space would fill with notes that have no recording.
        body: "# {{title}}\n\
\n\
Recorded {yyyy}-{mm}-{dd} at {HH}:{MM}.\n\
\n\
## Summary\n\
\n\
{{cursor}}\n\
\n\
## Chapters\n\
\n\
| Time  | Topic |\n\
| ----- | ----- |\n\
| 00:00 |       |\n\
\n\
## Actions\n\
\n\
- [ ]\n",
    },
];

/// The file keeper writes for one shipped template.
///
/// Tagged [`TEMPLATE_TAG`] and nothing else — that tag is what makes it a
/// template (AD-82), and [`expand`] is what takes it back off again on the way
/// into a copy.
pub fn render_template_note(template: &DefaultTemplate, id: &str, now: &str) -> String {
    let front = Frontmatter::serialise_new(&[
        ("id".to_owned(), FieldValue::Str(id.to_owned())),
        (
            "title".to_owned(),
            FieldValue::Str(template.name.to_owned()),
        ),
        ("created".to_owned(), FieldValue::Str(now.to_owned())),
        ("updated".to_owned(), FieldValue::Str(now.to_owned())),
        (
            "tags".to_owned(),
            FieldValue::List(vec![FieldValue::Str(TEMPLATE_TAG.to_owned())]),
        ),
    ]);
    format!("{front}\n{}", template.body)
}

/// Which shipped templates to write, in [`DEFAULT_TEMPLATES`] order.
///
/// The same three-way rule the space seeder uses, for the same reasons
/// ([`crate::notes::default_spaces::plan`]): the ledger says what this vault has
/// already been offered, `existing` says what is on disk right now, and an
/// unreadable ledger on an automatic run means keeper writes nothing.
///
/// `existing` is the file names already inside [`TEMPLATES_DIR`]. A default is
/// present when a file is already called what it would be called, folded through
/// [`naming::slug`] — the same fold that decides two notes cannot share a
/// filename, so `Journal entry.md` and `journal-entry.md` are one template and
/// the seed does not write a second.
pub fn plan_templates(
    mode: default_spaces::SeedMode,
    existing: &[String],
    offered: Option<&BTreeSet<String>>,
) -> Vec<&'static DefaultTemplate> {
    let ledger = match (mode, offered) {
        (default_spaces::SeedMode::Restore, _) => None,
        (default_spaces::SeedMode::FirstRun, Some(keys)) => Some(keys),
        // Unreadable ledger, automatic run: keeper stays out of the vault.
        (default_spaces::SeedMode::FirstRun, None) => return Vec::new(),
    };
    let taken: BTreeSet<String> = existing
        .iter()
        .map(|file| naming::slug(file.strip_suffix(".md").unwrap_or(file)))
        .collect();
    DEFAULT_TEMPLATES
        .iter()
        .filter(|template| !ledger.is_some_and(|keys| keys.contains(template.key)))
        .filter(|template| !taken.contains(&naming::slug(template.name)))
        .collect()
}

/// Run the template seed against a vault.
///
/// Reuses [`default_spaces::SeedVault`], [`default_spaces::SeedMode`] and
/// [`default_spaces::SeedOutcome`] rather than declaring a second port and a
/// second outcome enum: the two seeds do the same dangerous thing — write notes
/// into somebody's real vault, on removable media, through the sync engine — and
/// they must agree about what "absent" and "could not tell" mean. Only the
/// ledger, the contents and the wording differ.
///
/// Order is forced: read the ledger, list the directory, plan, write, record.
/// A read that fails is never an empty answer — an unlistable `templates/` is a
/// directory keeper cannot see, and writing three notes on that basis is how a
/// vault gets two of each.
pub fn seed_templates(
    vault: &mut dyn default_spaces::SeedVault,
    mode: default_spaces::SeedMode,
) -> default_spaces::SeedOutcome {
    let offered = match read_template_ledger(vault) {
        Ok(offered) => offered,
        Err(reason) => {
            // Restore is the user asking. An unreadable ledger must not stop
            // them repairing what they can see is missing.
            if mode == default_spaces::SeedMode::FirstRun {
                return default_spaces::SeedOutcome::Blocked(reason);
            }
            None
        }
    };
    let existing = match vault.list(TEMPLATES_DIR) {
        Ok(files) => files,
        // An absent directory is no templates; anything else is keeper being
        // unable to see, and it declines.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            return default_spaces::SeedOutcome::Blocked(format!(
                "could not list \"{TEMPLATES_DIR}\" in this vault, so keeper cannot tell which \
default templates are already there: {error}"
            ));
        }
    };

    let plan = plan_templates(mode, &existing, offered.as_ref());
    if plan.is_empty() {
        return default_spaces::SeedOutcome::AlreadySatisfied;
    }

    let mut written = Vec::new();
    let mut seeded: BTreeSet<String> = offered.unwrap_or_default();
    for template in plan {
        let id = vault.new_id();
        let now = vault.now_local();
        let rel = format!("{TEMPLATES_DIR}/{}.md", naming::slug(template.name));
        if let Err(error) = vault.write(&rel, &render_template_note(template, &id, &now)) {
            let reason = format!("could not write \"{rel}\": {error}");
            // Record what landed before giving up, or the next run writes the
            // ones that already exist a second time.
            record_template_ledger(vault, &seeded);
            return default_spaces::SeedOutcome::Stopped { written, reason };
        }
        seeded.insert(template.key.to_owned());
        written.push(rel);
    }
    record_template_ledger(vault, &seeded);
    default_spaces::SeedOutcome::Wrote(written)
}

/// The sentence and level one template-seed run deserves in the log.
///
/// Its own wording rather than [`default_spaces::SeedOutcome::report`]'s, which
/// says "default spaces" in every arm — but the same floor, and for the same
/// reason: nothing sets `RUST_LOG` in the packaged app, so `tracing::debug!` is
/// dead code there (DW-162). **Every arm reports at
/// [`default_spaces::REPORT_FLOOR`] or above**, asserted below, so a run that
/// declined to act can never be invisible on the machine it declined on.
pub fn report_template_seed(outcome: &default_spaces::SeedOutcome) -> (tracing::Level, String) {
    let floor = default_spaces::REPORT_FLOOR;
    match outcome {
        default_spaces::SeedOutcome::Wrote(written) if written.is_empty() => (
            floor,
            "seeded no default templates; the plan was empty".to_owned(),
        ),
        default_spaces::SeedOutcome::Wrote(written) => (
            floor,
            format!(
                "seeded {} default templates: {}",
                written.len(),
                written.join(", ")
            ),
        ),
        default_spaces::SeedOutcome::AlreadySatisfied => (
            floor,
            "default templates already settled for this vault; wrote nothing".to_owned(),
        ),
        default_spaces::SeedOutcome::Blocked(why) => (
            tracing::Level::WARN,
            format!("did not seed the default templates; will try again next refresh. {why}"),
        ),
        default_spaces::SeedOutcome::Stopped { written, reason } => (
            tracing::Level::WARN,
            format!(
                "stopped after seeding {} default templates; recorded what landed. {reason}",
                written.len()
            ),
        ),
    }
}

/// The sentence written into the template ledger, so the file explains itself to
/// whoever finds it in their vault rather than looking like debris.
const TEMPLATE_LEDGER_NOTE: &str = "keeper has already offered this vault its default \
templates, and will not add them again on its own. Delete a template you do not want and it \
stays deleted. Use Restore default templates to get the missing ones back, or delete this file \
to be offered all of them again.";

/// The keys this vault has already been offered, or the sentence explaining why
/// keeper cannot tell.
///
/// **`Ok(None)` is impossible on purpose**, exactly as in
/// [`crate::notes::default_spaces`]: an absent ledger is `Ok(Some(empty))`,
/// which is the fact "this vault has been offered nothing", while `None` at the
/// [`plan_templates`] boundary means "keeper could not tell" and stops an
/// automatic run dead. Returning `Ok(None)` for a missing file — which the first
/// draft did — collapses those two into one and makes a fresh vault report
/// `AlreadySatisfied` while writing nothing: a feature green on every test and
/// silent on the owner's machine, which is the failure this epic keeps shipping.
fn read_template_ledger(
    vault: &dyn default_spaces::SeedVault,
) -> Result<Option<BTreeSet<String>>, String> {
    match vault.read(TEMPLATE_LEDGER_REL) {
        Ok(text) => default_spaces::parse_ledger(&text)
            .map(Some)
            .ok_or_else(|| {
                format!(
                "\"{TEMPLATE_LEDGER_REL}\" is there and is not a seed ledger, so keeper cannot \
tell which default templates this vault has already been offered; leaving them alone"
            )
            }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Some(BTreeSet::new())),
        Err(error) => Err(format!(
            "\"{TEMPLATE_LEDGER_REL}\" could not be read ({error}); leaving this vault's \
templates alone"
        )),
    }
}

/// Record what this vault has been offered. A failure here is deliberately not
/// fatal: the templates are on disk and the user can see them, and the cost of
/// an unrecorded seed is that the next run finds them by name and declines.
fn record_template_ledger(vault: &mut dyn default_spaces::SeedVault, keys: &BTreeSet<String>) {
    let text = render_template_ledger(keys);
    if let Err(error) = vault.write(TEMPLATE_LEDGER_REL, &text) {
        tracing::warn!(
            %error,
            "notes: wrote the default templates but could not record them in the ledger; \
        the next run will find them by name"
        );
    }
}

/// The ledger file's bytes.
///
/// Written here rather than through [`default_spaces::render_ledger`] — the two
/// files share a *format* but not a sentence, and reaching for the spaces
/// renderer and then string-replacing its note out would leave this file
/// silently wrong the day that sentence is reworded. The reader is shared,
/// because parsing genuinely is one rule.
fn render_template_ledger(keys: &BTreeSet<String>) -> String {
    let value = serde_json::json!({
        "version": TEMPLATE_LEDGER_VERSION,
        "note": TEMPLATE_LEDGER_NOTE,
        "seeded": keys.iter().collect::<Vec<_>>(),
    });
    // Pretty, with a trailing newline: this lands in a folder a person browses
    // and a line-based sync diffs.
    format!(
        "{}\n",
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_owned())
    )
}

/// The ledger format this build writes and understands.
const TEMPLATE_LEDGER_VERSION: u64 = 1;

enum Resolved {
    Text(String),
    Cursor,
    Unknown,
}

fn resolve(token: &str, ctx: &TemplateCtx, stamp: Option<&Stamp>) -> Resolved {
    match token {
        "title" => Resolved::Text(ctx.title.clone()),
        "id" => Resolved::Text(ctx.id.clone()),
        "cursor" => Resolved::Cursor,
        "date" => stamp.map_or(Resolved::Unknown, |s| {
            Resolved::Text(render(s, "YYYY-MM-DD"))
        }),
        "time" => stamp.map_or(Resolved::Unknown, |s| Resolved::Text(render(s, "HH:mm"))),
        _ => {
            let format = token
                .strip_prefix("date:")
                .or_else(|| token.strip_prefix("time:"));
            match (format, stamp) {
                // An unparseable timestamp leaves the placeholder literal rather
                // than expanding to a wrong or empty date: a visible `{{date}}`
                // in a new note is a bug report, a silent 1970 is not.
                (Some(f), Some(s)) => Resolved::Text(render(s, f)),
                _ => Resolved::Unknown,
            }
        }
    }
}

/// A wall-clock instant, decomposed. No timezone maths happens here — the shell
/// already resolved the offset, and this only ever reformats what it was given.
///
/// Crate-visible rather than private because Story 42.4's recording-note stub
/// needs exactly this and nothing more: the manifest hands it `started_at` and
/// `ended_at` as RFC 3339 strings whose offset the shell already applied, and
/// the stub wants the calendar fields back out of them. Duplicating the slicing
/// there would be a second parser for a format keeper itself writes — and the
/// two would eventually disagree about what `2026-08-08T00:30:00+02:00` is the
/// date of, which is the one thing a note's filename must not be wrong about.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stamp {
    pub(crate) year: i32,
    pub(crate) month: u32,
    pub(crate) day: u32,
    pub(crate) hour: u32,
    pub(crate) minute: u32,
    pub(crate) second: u32,
}

impl Stamp {
    /// Slice an RFC 3339 timestamp into fields. Deliberately positional: the
    /// format is fixed-width by specification, and a parser combinator here
    /// would be more code defending against inputs keeper itself produces.
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let bytes = s.as_bytes();
        if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }

        let year: i32 = s.get(0..4)?.parse().ok()?;
        let month: u32 = s.get(5..7)?.parse().ok()?;
        let day: u32 = s.get(8..10)?.parse().ok()?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }

        let has_time = bytes.len() >= 19
            && matches!(bytes[10], b'T' | b't' | b' ')
            && bytes[13] == b':'
            && bytes[16] == b':';
        let (hour, minute, second) = if has_time {
            (
                s.get(11..13)?.parse().ok()?,
                s.get(14..16)?.parse().ok()?,
                s.get(17..19)?.parse().ok()?,
            )
        } else {
            (0, 0, 0)
        };
        if hour > 23 || minute > 59 || second > 60 {
            return None;
        }

        Some(Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        })
    }
}

/// The moment.js token subset Obsidian users already have in their templates.
/// `MM` is the month and `mm` the minute — case-sensitive, as moment defines it.
/// Longest first, so `YYYY` is never read as two `YY`s.
const TOKENS: [&str; 7] = ["YYYY", "YY", "MM", "DD", "HH", "mm", "ss"];

fn render(stamp: &Stamp, pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut rest = pattern;

    'outer: while !rest.is_empty() {
        for token in TOKENS {
            if let Some(tail) = rest.strip_prefix(token) {
                match token {
                    "YYYY" => {
                        let _ = write!(out, "{:04}", stamp.year);
                    }
                    "YY" => {
                        let _ = write!(out, "{:02}", stamp.year.rem_euclid(100));
                    }
                    "MM" => {
                        let _ = write!(out, "{:02}", stamp.month);
                    }
                    "DD" => {
                        let _ = write!(out, "{:02}", stamp.day);
                    }
                    "HH" => {
                        let _ = write!(out, "{:02}", stamp.hour);
                    }
                    "mm" => {
                        let _ = write!(out, "{:02}", stamp.minute);
                    }
                    _ => {
                        let _ = write!(out, "{:02}", stamp.second);
                    }
                }
                rest = tail;
                continue 'outer;
            }
        }

        // Not a token: copy one character through verbatim.
        let Some(c) = rest.chars().next() else { break };
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> TemplateCtx {
        TemplateCtx {
            title: "Weekly review".to_owned(),
            id: "01J8ZQ0000000000000000000A".to_owned(),
            now_local: "2026-08-02T14:35:09+02:00".to_owned(),
        }
    }

    #[test]
    fn expansion_reports_the_cursor_offset_and_removes_the_marker() {
        let (text, cursor) = expand_body("# {{title}}\n\n{{cursor}}\n", &ctx());
        assert_eq!(text, "# Weekly review\n\n\n");
        assert_eq!(cursor, Some("# Weekly review\n\n".len()));
        // The offset is a real byte index into the expanded text.
        let at = cursor.unwrap_or_default();
        assert_eq!(&text[..at], "# Weekly review\n\n");
    }

    #[test]
    fn cursor_offset_is_a_byte_index_after_multibyte_expansion() {
        let mut c = ctx();
        c.title = "Café ☕".to_owned();
        let (text, cursor) = expand_body("{{title}}{{cursor}}!", &c);
        assert_eq!(text, "Café ☕!");
        assert_eq!(cursor, Some("Café ☕".len()));
    }

    #[test]
    fn only_the_first_cursor_survives_and_the_rest_vanish() {
        let (text, cursor) = expand_body("a{{cursor}}b{{cursor}}c", &ctx());
        assert_eq!(text, "abc");
        assert_eq!(cursor, Some(1));
    }

    #[test]
    fn no_cursor_placeholder_means_no_offset() {
        let (text, cursor) = expand_body("plain body", &ctx());
        assert_eq!(text, "plain body");
        assert_eq!(cursor, None);
    }

    #[test]
    fn an_unknown_placeholder_is_left_literal_byte_for_byte() {
        let (text, _) = expand_body("{{TODO}} and {{ weather:oslo }} and {{}}", &ctx());
        assert_eq!(text, "{{TODO}} and {{ weather:oslo }} and {{}}");
    }

    #[test]
    fn an_unterminated_brace_pair_is_literal_text() {
        let (text, _) = expand_body("half open {{title", &ctx());
        assert_eq!(text, "half open {{title");
    }

    #[test]
    fn dates_and_times_use_the_moment_token_subset() {
        let (text, _) = expand_body(
            "{{date}} {{time}} | {{date:YYYY/MM/DD}} {{time:HH:mm:ss}} {{date:YY}}",
            &ctx(),
        );
        assert_eq!(text, "2026-08-02 14:35 | 2026/08/02 14:35:09 26");
    }

    #[test]
    fn month_and_minute_tokens_do_not_collide() {
        let (text, _) = expand_body("{{date:MM-mm}}", &ctx());
        assert_eq!(text, "08-35");
    }

    #[test]
    fn literal_text_inside_a_format_survives() {
        let (text, _) = expand_body("{{date:[week of] YYYY-MM-DD}}", &ctx());
        assert_eq!(text, "[week of] 2026-08-02");
    }

    #[test]
    fn whitespace_inside_the_braces_is_tolerated_for_known_tokens() {
        let (text, _) = expand_body("{{ title }}", &ctx());
        assert_eq!(text, "Weekly review");
    }

    #[test]
    fn an_unparseable_timestamp_leaves_date_placeholders_visible() {
        let broken = TemplateCtx {
            title: "T".to_owned(),
            id: "I".to_owned(),
            now_local: "not a timestamp".to_owned(),
        };
        let (text, _) = expand_body("{{date}} {{date:YYYY}} {{title}}", &broken);
        assert_eq!(text, "{{date}} {{date:YYYY}} T");
    }

    #[test]
    fn a_date_only_timestamp_still_expands_dates() {
        let c = TemplateCtx {
            now_local: "2026-08-02".to_owned(),
            ..ctx()
        };
        let (text, _) = expand_body("{{date}} {{time}}", &c);
        assert_eq!(text, "2026-08-02 00:00");
    }

    #[test]
    fn an_empty_template_expands_to_nothing() {
        assert_eq!(expand_body("", &ctx()), (String::new(), None));
    }

    // -----------------------------------------------------------------------
    // The single-brace vocabulary (Story 44.7)
    // -----------------------------------------------------------------------

    #[test]
    fn the_recording_path_vocabulary_resolves_in_a_body() {
        // Exactly the spellings `crate::recording::path_template` publishes, so
        // a user who learned them in the destination field has learned them
        // here. `{mm}` is the MONTH and `{MM}` is the MINUTE.
        let (text, _) = expand_body("{yyyy}-{mm}-{dd} {HH}:{MM}:{SS} {yy}", &ctx());
        assert_eq!(text, "2026-08-02 14:35:09 26");
    }

    #[test]
    fn the_two_vocabularies_disagree_about_mm_and_both_are_honoured() {
        // The collision worth reading twice, asserted rather than described:
        // single-brace `{mm}` is moment's `MM`, and moment's `mm` is the minute.
        let (text, _) = expand_body("{mm} {MM} | {{date:MM}} {{date:mm}}", &ctx());
        assert_eq!(text, "08 35 | 08 35");
    }

    #[test]
    fn an_unknown_single_brace_word_is_left_alone() {
        // A body is full of braces that are not placeholders. `{n}` in someone's
        // maths, a literal `{}`, and a `{seq}` that belongs to paths and not
        // here all survive being written down.
        let (text, _) = expand_body("{n} {} {seq} {slug} {title} {YYYY}", &ctx());
        assert_eq!(text, "{n} {} {seq} {slug} {title} {YYYY}");
    }

    #[test]
    fn an_unclosed_single_brace_is_literal_and_a_later_token_still_resolves() {
        // The scan resumes after the `{`, not after the `}`, so one stray brace
        // cannot swallow the placeholder behind it.
        let (text, _) = expand_body("{ {yyyy}", &ctx());
        assert_eq!(text, "{ 2026");
        let (tail, _) = expand_body("a { b", &ctx());
        assert_eq!(tail, "a { b");
    }

    #[test]
    fn a_double_brace_unknown_is_not_rewritten_by_the_single_brace_pass() {
        // The trap this ordering exists to avoid: `{{yyyy}}` is an unknown
        // double-brace token, and running the other vocabulary over the bytes
        // re-emitted for it would produce `{2026}` — an unknown silently
        // rewritten, which is the opposite of the promise.
        let (text, _) = expand_body("{{yyyy}} {{ dd }}", &ctx());
        assert_eq!(text, "{{yyyy}} {{ dd }}");
    }

    #[test]
    fn an_unparseable_timestamp_leaves_single_brace_placeholders_visible() {
        let broken = TemplateCtx {
            now_local: "not a timestamp".to_owned(),
            ..ctx()
        };
        let (text, _) = expand_body("{yyyy}-{mm}-{dd}", &broken);
        assert_eq!(text, "{yyyy}-{mm}-{dd}");
    }

    #[test]
    fn a_cursor_offset_survives_a_single_brace_expansion_before_it() {
        // The offset is a byte index into the EXPANDED text, and the expansion
        // that runs before the marker changes the text's length.
        let (text, cursor) = expand_body("{yyyy}-{mm}{{cursor}}!", &ctx());
        assert_eq!(text, "2026-08!");
        assert_eq!(cursor, Some("2026-08".len()));
    }

    // -----------------------------------------------------------------------
    // A template is a note (Story 44.7)
    // -----------------------------------------------------------------------

    /// A template note as it actually sits in a vault.
    fn template_note(tags: &str, body: &str) -> String {
        format!("---\nid: 01TEMPLATE0000000000000000\ncreated: 2026-01-01T00:00:00+01:00\ntags: {tags}\n---\n{body}")
    }

    #[test]
    fn the_copy_carries_every_tag_except_the_marker() {
        let out = expand(
            &template_note("[journal, Daily, template, work/notes]", "# Hi\n"),
            &ctx(),
        );
        assert_eq!(out.tags, vec!["journal", "daily", "work/notes"]);
        // The whole point: the copy is not a template, or every note made from
        // it would be a template of itself.
        assert!(!out.tags.iter().any(|tag| tag == TEMPLATE_TAG));
    }

    #[test]
    fn the_marker_is_matched_after_normalisation_not_as_typed() {
        // `#Template` and ` TEMPLATE ` are the same tag; a case-sensitive
        // comparison here would let a capitalised marker through into the copy.
        let out = expand(
            &template_note("[\"#Template\", \" TEMPLATE \"]", "x"),
            &ctx(),
        );
        assert!(out.tags.is_empty(), "{:?}", out.tags);
    }

    #[test]
    fn a_tag_merely_filed_under_template_is_kept() {
        // Only the exact marker leaves. `template/daily` is somebody's own
        // filing, `is_template` does not read it as a marker, and dropping it
        // would be deleting a tag that was never doing anything.
        let out = expand(&template_note("[template, template/daily]", "x"), &ctx());
        assert_eq!(out.tags, vec!["template/daily"]);
    }

    #[test]
    fn the_templates_own_frontmatter_never_reaches_the_body() {
        // The defect this function exists to fix. The create path used to hand
        // the whole FILE to the expander, so a templated note was written with a
        // literal `---` block pasted into its body under the real one.
        let out = expand(
            &template_note("[template]", "# Standup\n\nNotes.\n"),
            &ctx(),
        );
        assert_eq!(out.body, "# Standup\n\nNotes.\n");
        assert!(!out.body.contains("---"), "{:?}", out.body);
        assert!(!out.body.contains("id:"), "{:?}", out.body);
    }

    #[test]
    fn a_template_without_placeholders_copies_byte_identically() {
        // The body is a document, not a format string: every byte that is not a
        // placeholder is the author's, including the braces that are not ours.
        let body = "# Meeting\n\n| Who | What |\n| --- | ---- |\n|     |      |\n\n- [ ] item {n}\n> quote\n";
        let out = expand(&template_note("[template]", body), &ctx());
        assert_eq!(out.body, body);
        assert_eq!(out.caret, None);
    }

    #[test]
    fn a_template_with_no_frontmatter_at_all_is_still_a_body() {
        // A note somebody wrote in a plain editor. No tags to carry, no id to
        // record, and the whole file is the body.
        let out = expand("# Bare\n\n{yyyy}\n", &ctx());
        assert_eq!(out.body, "# Bare\n\n2026\n");
        assert!(out.tags.is_empty());
        assert_eq!(out.source_id, None);
        // And with no id, provenance records the path alone rather than a pair
        // with a hole in it.
        let pairs = provenance_pairs("templates/bare.md", out.source_id.as_deref());
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, FROM_TEMPLATE_KEY);
    }

    #[test]
    fn expansion_reports_the_templates_own_id_for_provenance() {
        let out = expand(&template_note("[template]", "x"), &ctx());
        assert_eq!(out.source_id.as_deref(), Some("01TEMPLATE0000000000000000"));

        let pairs = provenance_pairs("templates/journal.md", out.source_id.as_deref());
        assert_eq!(
            pairs,
            vec![
                (
                    FROM_TEMPLATE_KEY.to_owned(),
                    FieldValue::Str("templates/journal.md".to_owned())
                ),
                (
                    FROM_TEMPLATE_ID_KEY.to_owned(),
                    FieldValue::Str("01TEMPLATE0000000000000000".to_owned())
                ),
            ]
        );
    }

    #[test]
    fn provenance_round_trips_through_a_written_note() {
        // Written the way the create path writes it — inside the reserved map —
        // and read back by the one reader Story 44.8 uses.
        let pairs = provenance_pairs("templates/journal.md", Some("01TEMPLATE"));
        let note = Frontmatter::serialise_new(&[
            ("id".to_owned(), FieldValue::Str("01NOTE".to_owned())),
            ("keeper".to_owned(), FieldValue::Map(pairs)),
        ]);
        let found = provenance(&note);
        assert_eq!(found.path.as_deref(), Some("templates/journal.md"));
        assert_eq!(found.id.as_deref(), Some("01TEMPLATE"));
        assert!(!found.is_empty());
    }

    #[test]
    fn a_note_from_no_template_has_no_provenance_and_does_not_error() {
        // Every shape 44.8's scan will meet on a real vault. None is an error:
        // one malformed note must not abort a ten-thousand-note walk.
        for source in [
            "",
            "# just a body\n",
            "---\nid: 01NOTE\n---\nbody\n",
            "---\nkeeper: not-a-map\n---\n",
            "---\nkeeper:\n  capture: true\n---\n",
            "---\nkeeper:\n  from_template: \"   \"\n---\n",
        ] {
            assert!(provenance(source).is_empty(), "{source:?}");
        }
    }

    #[test]
    fn provenance_survives_a_note_that_also_carries_other_keeper_keys() {
        // The map is shared with `capture`, `default` and the space keys, and a
        // reader that stopped at the first unrecognised entry would miss ours.
        let note = "---\nkeeper:\n  capture: true\n  from_template: templates/x.md\n  from_template_id: 01T\n---\n";
        let found = provenance(note);
        assert_eq!(found.path.as_deref(), Some("templates/x.md"));
        assert_eq!(found.id.as_deref(), Some("01T"));
    }

    #[test]
    fn a_template_is_marked_by_its_frontmatter_tag_and_not_by_its_body() {
        let tagged = template_note("[template]", "x");
        assert!(is_template(&Frontmatter::parse(&tagged).0));

        // An inline `#template` is a word in a sentence. It must NOT make the
        // note a template, because the body is copied verbatim into every note
        // made from one — an inline marker would ride along and make each copy a
        // template of itself.
        let mentions = template_note("[notes]", "How I use #template notes\n");
        assert!(!is_template(&Frontmatter::parse(&mentions).0));

        // And a bare scalar `tags: template` counts, because Obsidian writes it.
        let scalar = "---\ntags: template\n---\nx";
        assert!(is_template(&Frontmatter::parse(scalar).0));

        // A note with no tags at all is not one, whatever folder it sits in.
        assert!(!is_template(&Frontmatter::parse("---\nid: x\n---\n").0));
    }

    #[test]
    fn a_space_names_its_default_template() {
        let space =
            "---\nkeeper:\n  space: \"is:journal\"\n  template: templates/journal.md\n---\n";
        let (fm, _) = Frontmatter::parse(space);
        assert_eq!(
            space_default_template(&fm).as_deref(),
            Some("templates/journal.md")
        );
    }

    #[test]
    fn a_space_with_no_usable_template_setting_names_none() {
        // Cleared and never set are one state: a user who emptied the field
        // means "no template", not "a template whose path is nothing".
        for space in [
            "---\nkeeper:\n  space: \"is:journal\"\n---\n",
            "---\nkeeper:\n  template: \"\"\n---\n",
            "---\nkeeper:\n  template: \"   \"\n---\n",
            "---\nkeeper:\n  template: 7\n---\n",
            "---\nkeeper: nonsense\n---\n",
            "---\nid: x\n---\n",
            "",
        ] {
            let (fm, _) = Frontmatter::parse(space);
            assert_eq!(space_default_template(&fm), None, "{space:?}");
        }
    }

    #[test]
    fn the_missing_template_sentence_names_the_path_and_says_the_note_was_made() {
        let notice = missing_template_notice("templates/journal.md");
        assert!(notice.contains("templates/journal.md"), "{notice}");
        // Both halves, or it reads as a failure when the note is right there.
        assert!(notice.contains("was created without it"), "{notice}");
        // A finished sentence, composed here, not a code for TypeScript to word.
        assert!(notice.ends_with('.'), "{notice}");
    }

    // -----------------------------------------------------------------------
    // The templates keeper ships, and the seed that writes them
    // -----------------------------------------------------------------------

    /// A vault in memory, driven through the real port the shell implements.
    #[derive(Default)]
    struct FakeVault {
        files: std::collections::BTreeMap<String, String>,
        /// Directories whose listing fails, and the error kind it fails with.
        unlistable: BTreeSet<String>,
        /// Paths whose write fails, so a half-finished seed can be driven.
        unwritable: BTreeSet<String>,
        next_id: usize,
    }

    impl default_spaces::SeedVault for FakeVault {
        fn read(&self, rel: &str) -> std::io::Result<String> {
            self.files
                .get(rel)
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"))
        }

        fn list(&self, rel_dir: &str) -> std::io::Result<Vec<String>> {
            if self.unlistable.contains(rel_dir) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "denied",
                ));
            }
            let prefix = format!("{rel_dir}/");
            let files: Vec<String> = self
                .files
                .keys()
                .filter_map(|rel| rel.strip_prefix(&prefix))
                .filter(|rest| !rest.contains('/'))
                .map(str::to_owned)
                .collect();
            if files.is_empty() {
                // An absent directory, which the seeder must tell apart from an
                // unlistable one.
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no such directory",
                ));
            }
            Ok(files)
        }

        fn write(&mut self, rel: &str, text: &str) -> std::io::Result<()> {
            if self.unwritable.contains(rel) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "denied",
                ));
            }
            self.files.insert(rel.to_owned(), text.to_owned());
            Ok(())
        }

        fn new_id(&mut self) -> String {
            self.next_id += 1;
            format!("01SEEDEDTEMPLATE{:010}", self.next_id)
        }

        fn now_local(&self) -> String {
            "2026-08-09T09:00:00+02:00".to_owned()
        }

        fn today(&self) -> String {
            "2026-08-09".to_owned()
        }
    }

    #[test]
    fn a_first_run_on_a_fresh_vault_writes_all_three_and_records_them() {
        let mut vault = FakeVault::default();
        let outcome = seed_templates(&mut vault, default_spaces::SeedMode::FirstRun);
        assert_eq!(
            outcome,
            default_spaces::SeedOutcome::Wrote(vec![
                "templates/inbox-note.md".to_owned(),
                "templates/journal-entry.md".to_owned(),
                "templates/recording-notes.md".to_owned(),
            ])
        );
        // The ledger is what stops the second run happening at all.
        let ledger = default_spaces::parse_ledger(&vault.files[TEMPLATE_LEDGER_REL])
            .expect("the ledger keeper just wrote must parse");
        assert_eq!(ledger.len(), 3);
        assert!(ledger.contains("journal"));
    }

    #[test]
    fn a_second_run_writes_nothing_and_says_so_out_loud() {
        let mut vault = FakeVault::default();
        seed_templates(&mut vault, default_spaces::SeedMode::FirstRun);
        let again = seed_templates(&mut vault, default_spaces::SeedMode::FirstRun);
        assert_eq!(again, default_spaces::SeedOutcome::AlreadySatisfied);

        // And a deleted template STAYS deleted: the ledger, not the directory,
        // is what keeper remembers, so it does not put back what was thrown away.
        vault.files.remove("templates/journal-entry.md");
        let third = seed_templates(&mut vault, default_spaces::SeedMode::FirstRun);
        assert_eq!(third, default_spaces::SeedOutcome::AlreadySatisfied);
        assert!(!vault.files.contains_key("templates/journal-entry.md"));
    }

    #[test]
    fn restore_ignores_the_ledger_and_replaces_only_what_is_missing() {
        let mut vault = FakeVault::default();
        seed_templates(&mut vault, default_spaces::SeedMode::FirstRun);
        vault.files.remove("templates/journal-entry.md");
        // The user asking is the whole point of Restore, so the ledger does not
        // get a vote — but the two templates still on disk are left alone.
        let outcome = seed_templates(&mut vault, default_spaces::SeedMode::Restore);
        assert_eq!(
            outcome,
            default_spaces::SeedOutcome::Wrote(vec!["templates/journal-entry.md".to_owned()])
        );
    }

    #[test]
    fn a_template_already_there_under_the_same_name_is_never_doubled() {
        // Whatever the user's file says inside, the NAME is taken — folded the
        // way `naming::slug` folds it, so `Journal Entry.md` blocks the seed too.
        let mut vault = FakeVault::default();
        vault
            .files
            .insert("templates/Journal Entry.md".to_owned(), "mine".to_owned());
        let outcome = seed_templates(&mut vault, default_spaces::SeedMode::FirstRun);
        assert_eq!(
            outcome,
            default_spaces::SeedOutcome::Wrote(vec![
                "templates/inbox-note.md".to_owned(),
                "templates/recording-notes.md".to_owned(),
            ])
        );
        assert_eq!(vault.files["templates/Journal Entry.md"], "mine");
    }

    #[test]
    fn an_unreadable_ledger_stops_an_automatic_run_and_never_a_restore() {
        let mut vault = FakeVault::default();
        vault
            .files
            .insert(TEMPLATE_LEDGER_REL.to_owned(), "{ not json".to_owned());

        // A ledger that is THERE and does not parse is not an absent one: keeper
        // does not know what this vault has been offered, and writing three
        // notes on that basis is the AD-79 failure.
        let blocked = seed_templates(&mut vault, default_spaces::SeedMode::FirstRun);
        let default_spaces::SeedOutcome::Blocked(why) = &blocked else {
            panic!("expected Blocked, got {blocked:?}");
        };
        assert!(why.contains(TEMPLATE_LEDGER_REL), "{why}");
        assert!(!vault.files.contains_key("templates/inbox-note.md"));

        // The user pressing Restore is looking at the gap and asking for it back.
        let restored = seed_templates(&mut vault, default_spaces::SeedMode::Restore);
        assert!(matches!(
            &restored,
            default_spaces::SeedOutcome::Wrote(written) if written.len() == 3
        ));
    }

    #[test]
    fn a_directory_keeper_cannot_list_blocks_the_seed_rather_than_reading_as_empty() {
        // A sleeping USB volume. Swallowing the listing error and calling it "no
        // templates" is how a vault ends up with two of each.
        let mut vault = FakeVault::default();
        vault.unlistable.insert(TEMPLATES_DIR.to_owned());
        let outcome = seed_templates(&mut vault, default_spaces::SeedMode::FirstRun);
        let default_spaces::SeedOutcome::Blocked(why) = &outcome else {
            panic!("expected Blocked, got {outcome:?}");
        };
        assert!(why.contains(TEMPLATES_DIR), "{why}");
        assert!(vault.files.is_empty(), "nothing may be written");
    }

    #[test]
    fn a_write_that_fails_partway_records_what_landed() {
        let mut vault = FakeVault::default();
        vault
            .unwritable
            .insert("templates/journal-entry.md".to_owned());
        let outcome = seed_templates(&mut vault, default_spaces::SeedMode::FirstRun);
        let default_spaces::SeedOutcome::Stopped { written, .. } = &outcome else {
            panic!("expected Stopped, got {outcome:?}");
        };
        assert_eq!(written, &vec!["templates/inbox-note.md".to_owned()]);
        // What landed is in the ledger, so the next run does not write it twice.
        let ledger =
            default_spaces::parse_ledger(&vault.files[TEMPLATE_LEDGER_REL]).expect("ledger parses");
        assert_eq!(ledger.iter().collect::<Vec<_>>(), vec!["inbox"]);
    }

    #[test]
    fn every_seed_outcome_is_reported_at_a_level_the_app_can_actually_print() {
        // DW-162, one layer out: nothing sets `RUST_LOG` in the packaged app, so
        // a `debug!` here is a run that did something invisible on the machine it
        // ran on. This is the assertion, not the promise.
        for outcome in [
            default_spaces::SeedOutcome::Wrote(vec!["templates/x.md".to_owned()]),
            default_spaces::SeedOutcome::Wrote(Vec::new()),
            default_spaces::SeedOutcome::AlreadySatisfied,
            default_spaces::SeedOutcome::Blocked("why".to_owned()),
            default_spaces::SeedOutcome::Stopped {
                written: Vec::new(),
                reason: "why".to_owned(),
            },
        ] {
            let (level, sentence) = report_template_seed(&outcome);
            assert!(
                level <= default_spaces::REPORT_FLOOR,
                "{outcome:?} reports at {level}, below the floor the app prints"
            );
            assert!(
                sentence.contains("template"),
                "the sentence must say what it is about: {sentence}"
            );
        }
    }

    #[test]
    fn every_shipped_template_is_a_template_and_makes_notes_that_are_not() {
        // The round trip that matters: what the seeder writes, read back through
        // the predicate that finds it and the function that copies it.
        for shipped in &DEFAULT_TEMPLATES {
            let note = render_template_note(shipped, "01T", "2026-08-09T09:00:00+02:00");
            let (fm, _) = Frontmatter::parse(&note);
            assert!(is_template(&fm), "{} must be a template", shipped.key);

            let made = expand(&note, &ctx());
            assert!(
                made.tags.is_empty(),
                "{} hands its copy {:?}; a shipped template must add no tag, or the \
space that offered it (Inbox is `is:untagged`) may be the one space the note cannot appear in",
                shipped.key,
                made.tags
            );
            // `title` is the template's name and must not become the note's.
            assert!(
                !made.properties.iter().any(|(key, _)| key == "title"),
                "{} would name every note after itself",
                shipped.key
            );
            // No frontmatter fence smuggled into the body. A bare `---` LINE,
            // not the substring: a GFM delimiter row (`| ---- | ---- |`) also
            // contains three dashes, and the first version of this assertion
            // failed the journal template for having a table in it.
            assert!(
                !made.body.lines().any(|line| line.trim_end() == "---"),
                "{} smuggles a frontmatter fence into its body",
                shipped.key
            );
            // Each one puts the caret somewhere deliberate.
            assert!(made.caret.is_some(), "{} places no caret", shipped.key);
        }
    }

    #[test]
    fn a_shipped_templates_tables_are_aligned_after_expansion() {
        // The defect this catches was in the first draft of the journal
        // template: `| {HH}:{MM} |` is nine source characters and five rendered
        // ones, so the scaffold looked aligned and every note made from it did
        // not. The assertion is on the EXPANDED text, because that is what a
        // person reads.
        for shipped in &DEFAULT_TEMPLATES {
            let note = render_template_note(shipped, "01T", "2026-08-09T09:00:00+02:00");
            let body = expand(&note, &ctx()).body;
            let rows: Vec<&str> = body
                .lines()
                .filter(|line| line.trim_start().starts_with('|'))
                .collect();
            if rows.is_empty() {
                continue;
            }
            let pipes: Vec<Vec<usize>> = rows
                .iter()
                .map(|row| {
                    row.char_indices()
                        .filter_map(|(at, c)| (c == '|').then_some(at))
                        .collect()
                })
                .collect();
            assert!(
                pipes.windows(2).all(|pair| pair[0] == pair[1]),
                "{}'s table pipes do not line up once expanded:\n{}",
                shipped.key,
                rows.join("\n")
            );
        }
    }

    #[test]
    fn no_shipped_template_uses_syntax_the_apps_own_renderer_cannot_draw() {
        // A callout renders in Obsidian and shows a literal `[!note]` in keeper:
        // `live-preview.ts` styles `Blockquote` and knows nothing of them. A
        // shipped template that looks broken in the app that wrote it is worse
        // than a plain one, so this is a gate rather than a note in a doc.
        for shipped in &DEFAULT_TEMPLATES {
            assert!(
                !shipped.body.contains("[!"),
                "{} uses a callout",
                shipped.key
            );
            // No trailing whitespace either: two spaces at end of line is a hard
            // break in markdown, and nobody means one in an empty scaffold.
            for line in shipped.body.lines() {
                assert_eq!(line, line.trim_end(), "{} has a padded line", shipped.key);
            }
        }
    }

    #[test]
    fn a_template_hands_its_own_properties_to_the_copy_but_never_its_identity() {
        let template = "---\nid: 01T\ntitle: Weekly Template\ncreated: 2026-01-01T00:00:00+01:00\nupdated: 2026-02-02T00:00:00+01:00\ntags: [template, work]\nstatus: draft\nproject: Acme\nkeeper:\n  template: templates/other.md\n---\n# x\n";
        let out = expand(template, &ctx());
        assert_eq!(
            out.properties,
            vec![
                ("status".to_owned(), FieldValue::Str("draft".to_owned())),
                ("project".to_owned(), FieldValue::Str("Acme".to_owned())),
            ],
            "source order, and only the author's own keys"
        );
        // Each exclusion is its own failure: a shared `id` puts a pin on the
        // wrong file, a copied `title` names every note after the scaffold, and a
        // copied `keeper` map would carry the template's provenance onto a note
        // that has its own.
        for private in PRIVATE_KEYS {
            assert!(
                !out.properties.iter().any(|(key, _)| key == private),
                "{private} crossed over"
            );
        }
        assert_eq!(out.tags, vec!["work"]);
    }
}
