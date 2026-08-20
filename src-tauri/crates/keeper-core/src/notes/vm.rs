//! Notes view models (AD-7, AD-8, Phase 5).
//!
//! Every type that crosses the Tauri IPC boundary for the notes surface lives
//! here, derives `serde` + [`ts_rs::TS`], is `#[ts(export)]`, and renames fields
//! to camelCase. Bindings land in `src/lib/ipc/gen/` from the ts-rs export step
//! that `cargo nextest run` drives, and `bun run bindings:check` fails CI if the
//! committed tree differs — so a field added here without regenerating is a red
//! build, by design.
//!
//! Two conventions carried over from [`crate::vm`] and `keeper::copy_ipc`, both
//! load-bearing rather than stylistic:
//!
//! - Timestamps are `i64` milliseconds since the Unix epoch, never strings, and
//!   every 64-bit field carries `#[ts(type = "number")]`. ts-rs would otherwise
//!   emit `bigint`, which is not what Tauri's `JSON.parse` actually delivers;
//!   ms-epoch values stay far inside `Number.MAX_SAFE_INTEGER`.
//! - Anything the user reads as a sentence is composed **in Rust**. `origin` on a
//!   row is a finished phrase, not an enum the webview words, because wording a
//!   Rust-owned fact in TypeScript is how two surfaces start disagreeing about
//!   what the same commit means.
//!
//! Nothing here holds note *content* beyond a snippet and the body the editor is
//! actually showing: the list never ships bodies (AD-58), and attachment bytes
//! never cross IPC at all — [`NoteAttachmentVm`] carries a URL for the custom
//! protocol to serve.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::notes::index::NoteTagTerm;
use crate::notes::order::NoteOrder;
use crate::vm::RecordingNoteTargetKind;

/// One notes-flagged sync profile, with its index state (FR-94, FR-95).
///
/// A vault *is* a synced folder plus a flag — there is no vault picker and no
/// second configuration store — so this VM is a projection of a `SyncProfile`,
/// not a record of its own. `indexed` is the honest "the cold scan for this vault
/// has finished" signal; before it flips, `note_count` and `unread_count` are the
/// best known so far rather than totals.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteVaultVm {
    /// Opaque vault id (stable for the lifetime of the profile).
    pub id: String,
    /// The sync profile this vault is a flag on.
    pub profile_id: String,
    /// Display name, inherited from the profile.
    pub name: String,
    /// Vault subfolder inside the profile root, e.g. `notes`.
    pub subfolder: String,
    /// Absolute vault root, display-only — every command addresses notes by id or
    /// vault-relative path, never by a path the webview composed.
    pub root: String,
    /// Whether the cold scan has completed.
    pub indexed: bool,
    /// Notes currently indexed.
    pub note_count: u32,
    /// Notes changed by an agent or another device since this device last
    /// acknowledged them (FR-113).
    pub unread_count: u32,
    /// The template a quick capture starts from, vault-relative, or `None`
    /// (Story 45.16, FR-193). Mirrored back so the settings form shows the
    /// value actually in force rather than the one it last sent (AD-34-8).
    pub capture_template: Option<String>,
    /// The tag every quick capture carries, in its canonical form, or `None`.
    ///
    /// Canonical rather than as typed, for the same reason: `keeper_core`
    /// folded `#Quick Capture` to `quick-capture` on the way in, and a form
    /// still showing the typed spelling would be describing a tag that is not
    /// in any note.
    pub capture_tag: Option<String>,
    /// The commit/push cadence in force for this vault.
    pub cadence: NoteCadenceVm,
}

/// The commit/push cadence of one vault (FR-120).
///
/// `commit_idle_ms` is measured from the *last* change, not the first, so a
/// typing burst produces one commit rather than one per keystroke. Both intervals
/// are clamped when the profile validates (500 ms / 5 s floors), so a value that
/// arrives here is already in force, not merely requested.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteCadenceVm {
    /// Quiet period after the last change before a commit is attempted.
    #[ts(type = "number")]
    pub commit_idle_ms: u64,
    /// How long after a commit the push is due.
    #[ts(type = "number")]
    pub push_interval_ms: u64,
    /// Bring the push deadline forward when the main window loses focus — the
    /// user walking away is the strongest available signal that the other machine
    /// wants these bytes.
    pub push_on_blur: bool,
}

/// One row of the note list (FR-103).
///
/// Everything the row paints, so rendering a window of a ten-thousand-note vault
/// touches no filesystem: the snippet is pre-extracted and the flags are
/// index-computed.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteRowVm {
    /// The note's stable id, which survives renames (FR-97).
    pub id: String,
    /// Vault-relative path with `/` separators.
    pub path: String,
    /// Display title.
    pub title: String,
    /// Short body excerpt for the row.
    pub snippet: String,
    /// Normalised tag paths.
    pub tags: Vec<String>,
    /// Last modification, ms since the Unix epoch.
    #[ts(type = "number")]
    pub updated_ms: i64,
    pub pinned: bool,
    pub archived: bool,
    /// Changed by an agent or another device since this device acknowledged it.
    pub unread: bool,
    /// A conflict copy exists beside this note (FR-116).
    pub conflict: bool,
    /// Provenance as a finished phrase composed in Rust, e.g. `changed by agent`
    /// or `changed on hesperia`. Empty when the note has no commit yet — the row
    /// branches on emptiness, never on null.
    pub origin: String,
    /// The head revision that last touched this note's path: the revision
    /// `unread` was computed against (`head_rev != acknowledged_rev`).
    ///
    /// It is on the row because clearing an unread mark from the list has to
    /// acknowledge a *revision*; acknowledging a timestamp instead would clear
    /// the mark against the wrong bytes and silently lose an agent's edit, which
    /// is exactly the failure NFR-30 exists to forbid. Empty when the note has no
    /// commit yet, the same "absent" spelling `origin` uses.
    pub head_rev: String,
    /// The note's own position, and whether the note said so (Story 44.5,
    /// AD-81).
    ///
    /// The row renders it, which is the whole point of the story: an ordering the
    /// reader cannot account for reads as randomness, and the cheapest way to
    /// account for it is to show the number the sort used. `source` is on the row
    /// because "0 because the note is silent" and "0 because the note says
    /// `order: soon` and that is not a number" are different sentences, and the
    /// second one has to be sayable.
    pub order: NoteOrder,
}

/// One window of the note list, with the counts behind it (FR-103, FR-166).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteListVm {
    /// The rows in this window, in list order.
    pub rows: Vec<NoteRowVm>,
    /// How many notes this lens SELECTS, so the scrollbar is honest about a
    /// window it has not been sent and the count is honest about a vault the
    /// viewport never rendered (Story 44.11).
    ///
    /// Never a count of rendered rows and never a count of the page: it is
    /// [`crate::notes::counts::Selection::total`], taken over the whole matched
    /// set before any offset or window.
    pub total: u32,
    /// How many notes the lens MATCHED, before the space's `keeper.limit`
    /// declined any (Story 44.11, DW-163).
    ///
    /// Equal to `total` for every lens with no cap in force, which is every
    /// list outside a space and every space that sets no limit. When it is
    /// larger, the surface says both numbers — a cap that quietly shrank a
    /// count is the same defect as a count of the rendered window.
    pub matched: u32,
    /// Offset of `rows[0]` within `total`.
    pub offset: u32,
}

/// A pointer to one note: what every mutating command returns so the caller can
/// navigate to what it just made, without shipping a body.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteRefVm {
    pub vault_id: String,
    pub id: String,
    pub path: String,
    pub title: String,
}

/// The hierarchical tag tree with counts (FR-104), over every producer of a tag
/// (FR-143).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteTagTreeVm {
    /// Root tags, each carrying its own subtree.
    pub nodes: Vec<NoteTagNodeVm>,
}

/// One node of the tag tree.
///
/// Named `Note…` because it is served by the notes surface; its contents are
/// not notes-only (Story 42.5).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteTagNodeVm {
    /// The last path segment — what the row displays.
    pub name: String,
    /// The full tag path, which is also the `tag:` value that selects it.
    pub path: String,
    /// Distinct things in this node's whole subtree — notes AND recording
    /// sessions, summed (Story 42.5) — so the number the chip shows is the
    /// number of things behind it rather than the number of one kind of thing.
    pub count: u32,
    pub children: Vec<NoteTagNodeVm>,
}

/// One level of the physical folder lens (FR-106).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteFolderVm {
    /// The directory being listed, vault-relative; empty for the vault root.
    pub rel_dir: String,
    /// Immediate subdirectory names, not paths.
    pub dirs: Vec<String>,
    /// Notes directly in this directory.
    pub notes: Vec<NoteRowVm>,
}

/// One space: a saved query that lives in a markdown note (FR-105).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteSpaceVm {
    /// The id of the note that defines the space.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The query source text, exactly as stored.
    pub query: String,
    /// How this space orders the notes it lists, exactly as stored, e.g.
    /// `modified desc` (FR-158, Story 44.4). Kept as the file's own text rather
    /// than as a parsed enum, so a value keeper could not read survives a round
    /// trip through the editor unrewritten — the same promise the query and the
    /// icon make. [`crate::notes::sort::read`] is what turns it into an
    /// ordering, and into the sentence in `warnings` when it cannot.
    pub sort: String,
    /// The ordering the list is actually running, as the canonical
    /// `<key> <dir>` — always one of the ten [`crate::notes::sort`] knows, even
    /// when `sort` above holds nothing, or holds `bananas`.
    ///
    /// It exists so the editor never parses `sort`. A dropdown that had to work
    /// out for itself what an empty string or an unknown word resolves to would
    /// be a second copy of the fallback rule, in the language that cannot run
    /// the tests — and the two copies would disagree the first time the rule
    /// changed. Rust decides; the form selects what Rust decided; saving sends
    /// that back.
    pub sort_effective: String,
    /// The most notes this space holds — a cap on what it SELECTS, not on what
    /// a surface renders (Story 44.11, DW-163). Zero is "no cap", which is what
    /// a space with no `keeper.limit` key sends and what saving zero back
    /// leaves the file without.
    ///
    /// Applied after the sort, so a space capped at twenty keeps the twenty its
    /// own ordering put first. Not clamped to the list's page size: the page is
    /// how many rows one read carries, and shrinking a space to fit one would
    /// drop notes the space genuinely holds.
    pub limit: u32,
    /// The icon the sidebar draws for this space, as the name of one member of
    /// the fixed set the editor offers (FR-149, UX-DR55). `None` for a space
    /// nobody has given one, and — deliberately — also the spelling for a space
    /// whose stored name is not in that set any more: the *name* survives on
    /// disk untouched, because keeper rewriting an icon it did not recognise is
    /// the same class of mistake as rewriting a query term it could not parse.
    pub icon: Option<String>,
    /// Which seeded default this space is, when it is one
    /// ([`crate::notes::default_spaces`], Story 44.3). `None` for every space a
    /// person or an agent wrote.
    ///
    /// It is the identity, not the name: a default is editable like any other
    /// space, so renaming Recordings to "Sessions" must not stop the empty list
    /// saying who writes recording notes, and must not make restore offer a
    /// second copy. Read from `keeper.default`, which only keeper writes and the
    /// editor never touches.
    pub default_key: Option<String>,
    /// The template a note created in this space starts from — a vault-relative
    /// path, or a bare name inside the template directory (FR-162, Story 44.7).
    /// `None` for a space that hands out no template, which is most of them.
    ///
    /// Carried as the stored text, unresolved: whether the path still names a
    /// note is a question about the vault at create time, not at render time, so
    /// the editor shows what the file says and the create path is what reports a
    /// template that has gone missing.
    pub template: Option<String>,
    /// The folder a note created in this space is written to — vault-relative,
    /// or `None` to let the query answer (Story 44.13).
    ///
    /// A `path:` query already implies a folder and still does; this is what a
    /// `tag:` space has instead, because a tag names a set and never a place.
    /// Stored as typed, unresolved: whether the folder exists is a question for
    /// create time, and a space that names one keeper has to make is not an
    /// error the editor should refuse.
    pub folder: Option<String>,
    /// The presentation keys of this space's frontmatter that keeper could not
    /// read, each already worded as a finished sentence (Story 44.4).
    ///
    /// Separate from `error` because the severity is different and so is the
    /// remedy: a query that does not parse means the space selects **nothing**,
    /// while an unreadable `sort` or `order` means the space still works and is
    /// simply not obeying one line of its own file. Both have to be visible —
    /// frontmatter is hand-edited and agent-edited, so these values will be
    /// wrong, and a fallback nobody is told about is indistinguishable from
    /// keeper ignoring what the user wrote.
    ///
    /// A list rather than an `Option`, because a file with a bad `sort` usually
    /// has a bad `order` too — whoever was guessing at one was guessing at both
    /// — and showing one of the two would send them round the loop twice.
    pub warnings: Vec<String>,
    /// Where this space sits in the rail: lower first, ties by name
    /// (FR-157, AD-81).
    ///
    /// Zero for a space nobody has positioned, which is every space that exists
    /// before this story — so a rail nobody has ordered is still the
    /// alphabetical rail it was, and the seeded defaults still render Inbox,
    /// Journal, Pinned, Recordings in the order the deleted fixed rows did.
    /// Negative is allowed and is how a space floats above that block.
    ///
    /// `f64` for the reason a note's own order is one (Story 44.5): `1.5` is how
    /// a person slots a row between 1 and 2 without renumbering everything under
    /// it, and an integer would read `1.5` and `1.2` as the same position.
    pub order: f64,
    /// The parse failure, when the stored query does not parse. A broken space
    /// matches nothing and says so; it never falls back to matching everything.
    pub error: Option<String>,
}

/// What a deletion is about to remove, in the words the confirmation shows
/// (Story 45.17, FR-195, UX-DR78).
///
/// **Composed in Rust for `FilesDeletePlanVm`'s reason** (Story 45.3): the
/// sentence has to be built by code that knows what the delete will actually
/// do, or the dialog promises one thing and the command does another. A
/// confirmation assembled in TypeScript from a name and a boolean is a second
/// reading of the removal rule, in the one place a wrong reading costs a file.
///
/// One struct for a note and for a space, because a space **is** a note and
/// [`crate::notes::default_spaces`] is the only thing that makes one special.
/// Two structs would be two dialogs, and the second one would be the one that
/// forgot to say where the bytes went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteDeletePlanVm {
    /// The vault-relative path of the file that moves. Shown under the
    /// question, because two notes may carry one title and the path is the
    /// only thing on screen that tells them apart.
    pub path: String,
    /// Names the thing. Never a count and never "this item": the whole point
    /// of a confirmation is that it says what goes.
    pub question: String,
    /// What goes, and — for a space — what conspicuously does not.
    pub consequence: String,
    /// Where the bytes end up. Never absent: a delete nobody said was
    /// recoverable is a delete people do not press.
    pub recovery: String,
}

/// Where a deleted note's bytes go, said once.
///
/// Worded to match `FilesDeletePlanVm`'s own recovery clause, because it is the
/// same `notes_vault::trash_note` under both and a person who deletes a file in
/// the Files pane and a note in the Notes pane must not be told two different
/// stories about whether keeper kept a copy (NFR-30).
const TRASH_RECOVERY: &str = "keeper moves it into the vault's trash rather than erasing it, and \
the removal is recorded in this vault's history.";

impl NoteDeletePlanVm {
    /// The plan for an ordinary note.
    ///
    /// The link clause is unconditional and is the honest half of the sentence:
    /// links resolve through the note's ULID (FR-97), so a wiki-link to a
    /// deleted note stops resolving whether or not anything currently points at
    /// it. Counting the backlinks here would mean running the link index inside
    /// a confirmation, and a count that is right only while nothing else is
    /// writing is worse than a rule that is always true.
    pub fn for_note(title: &str, path: &str) -> Self {
        Self {
            question: format!("Delete \"{title}\"?"),
            consequence: format!(
                "keeper removes {path} from this vault. Links to this note stop resolving."
            ),
            recovery: TRASH_RECOVERY.to_owned(),
            path: path.to_owned(),
        }
    }

    /// The plan for a space.
    ///
    /// **The sentence exists to answer the question that stops people deleting
    /// a saved view: does this take the notes with it.** It does not, and a
    /// confirmation that leaves that unsaid is why unused spaces accumulate.
    ///
    /// `default_key` is the field that DRIVES the second clause, not the
    /// space's name: a seeded Recordings space renamed to "Sessions" is still
    /// keeper's, and a space of the user's own called "Recordings" is not.
    /// Partitioning on the name would get both of those backwards.
    pub fn for_space(name: &str, path: &str, default_key: Option<&str>) -> Self {
        let mut consequence = format!(
            "A space is a saved view. Deleting it removes {path} and nothing else — every note \
             it lists stays where it is."
        );
        if default_key.is_some() {
            // Named because the alternative is a person deleting a default,
            // seeing it survive a restart, and concluding the delete failed —
            // and because the way back is a control they can see (FR-156).
            consequence.push_str(
                " keeper seeded this space, and will not add it back on its own; \
                 \"Restore default spaces\" brings it back.",
            );
        }
        Self {
            question: format!("Delete the space \"{name}\"?"),
            consequence,
            recovery: TRASH_RECOVERY.to_owned(),
            path: path.to_owned(),
        }
    }
}

/// One tag term of a space's query, in the shape the three-state chip holds
/// (Story 43.3's `TagChip`, field for field).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteSpaceTagVm {
    /// The tag, already read through the one vocabulary (Story 42.5).
    pub tag: String,
    pub term: NoteTagTerm,
}

/// One `field:` term of a space's query, in the shape a removable chip holds.
///
/// **Only `=` and `!=` reach this type**, and that is the whole of its
/// contract — see [`crate::notes::query::decompose`] for why the four ordered
/// operators and the negated form stay outside the chip vocabulary. A chip that
/// could not be re-emitted as the term it came from would be a chip that edits
/// a query by being read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteSpaceFieldVm {
    /// The frontmatter key, trimmed — `status` in `field:status=todo`.
    pub key: String,
    /// Exactly `"="` or `"!="`, as the query spelled it. A string rather than a
    /// two-case enum because it is re-emitted verbatim on save and never
    /// branched on; an enum here would buy a match arm on both sides of the
    /// wire and change nothing about what is written.
    pub op: String,
    /// The compared value, trimmed and unquoted.
    pub value: String,
}

/// A space's stored query said in the vocabulary the editor's controls speak,
/// or the reason it cannot be (FR-149, UX-DR55).
///
/// Two variants rather than one struct with a residue list, because the residue
/// is not extra information about an editable query — it is the fact that the
/// query is **not** editable through chips. A struct carrying both would let a
/// caller render three chips out of a four-term query and save it, which is
/// exactly the silent term-dropping this story exists to refuse. Here that call
/// site does not compile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum NoteSpaceTermsVm {
    /// Every term of the query, and the chip vocabulary holds all of them.
    Chips {
        /// Tag terms in the order the query wrote them — the order the chip bar
        /// will show, so an edited space keeps the shape its author gave it.
        tags: Vec<NoteSpaceTagVm>,
        /// `is:` flags, verbatim as written. The editor shows them and does not
        /// let them be cycled (this story widens tags, not lenses), so they are
        /// carried only so that re-emitting the query cannot lose them.
        flags: Vec<String>,
        /// `origin:`'s value, verbatim.
        origin: Option<String>,
        /// `text:`'s needle, unquoted — the editor re-quotes it on the way out.
        text: Option<String>,
        /// `field:key=value` and `field:key!=value` terms, in written order.
        ///
        /// Unlike the three above, a query may hold several: `status` and
        /// `priority` are different questions, and a board asks both. The bar
        /// shows one chip per term and removes them one at a time.
        fields: Vec<NoteSpaceFieldVm>,
    },
    /// At least one term is outside the chip vocabulary, so no chip may claim to
    /// stand for this query. `terms` is the offending source text, verbatim, for
    /// a surface that has to name what it will not touch.
    Unrepresentable { terms: Vec<String> },
}

/// The result of parsing a query without running it — the live underline while
/// someone types one.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteQueryCheckVm {
    pub ok: bool,
    /// The failure, phrased for display; `None` when `ok`.
    pub message: Option<String>,
    /// Index of the offending token.
    pub token_index: Option<u32>,
    /// **Byte** range of the offending token in the query string. Byte, not
    /// character: a query with an accented tag underlines a character early
    /// otherwise.
    pub span: Option<(u32, u32)>,
}

/// One template offered by the new-note path (FR-100).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteTemplateVm {
    /// Display name (the template's filename stem).
    pub name: String,
    /// Vault-relative path, which is what `NoteCreateReq.template` names.
    pub path: String,
}

/// A document-state message on an open note's body channel (AD-58).
///
/// The stream always opens with [`NoteBodyBatch::Reset`]. Everything after it
/// describes what happened to the note *underneath* the editor — an agent's
/// write, another device's checkout, a rename — so the webview never has to poll
/// and never has to guess whether its buffer is still the truth.
///
/// **The frontmatter block travels beside the body, never inside it.** Every
/// variant that carries text carries the body only, with the block it belongs to
/// in `frontmatter`. That split is what FR-107 asks for — the block renders as a
/// typed properties panel, not as YAML in the editor — and it is also what makes
/// the caret bug unrepresentable: there is no `---` in the buffer for a caret to
/// land in front of, so typing at offset 0 can no longer push the block into the
/// body. Rust owns the block; the editor owns the body; a save re-joins them.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum NoteBodyBatch {
    /// The note as it stands. Opens every subscription, and re-opens one after a
    /// change the editor can adopt wholesale.
    Reset {
        /// The revision these bytes are.
        rev: String,
        /// The note's vault-relative path.
        ///
        /// **Added by Story 45.18, and the absence it replaces was load-bearing.**
        /// `path` reached the editor only through `Renamed` or a completed save,
        /// so a note that was merely OPENED had none until its first autosave —
        /// which left the header's path caption blank on open, and would have
        /// left 45.18's "Show in Files" absent for exactly the case it exists
        /// for. The value is in hand here anyway; not sending it was the gap.
        path: String,
        /// The `---` block verbatim — fences and trailing newline included — or
        /// empty when the note has none.
        frontmatter: String,
        /// The body: every byte after the block.
        text: String,
        /// Where to put the caret, as a byte offset **into `text`**, set only
        /// when the template this note was created from declared a `{{cursor}}`.
        /// `None` leaves the choice to the editor, which is the end of the body —
        /// where someone continuing a note wants it. No `#[ts(type)]` override
        /// here: `u32` already emits `number`, and forcing it would erase the
        /// `| null` the option actually carries.
        cursor: Option<u32>,
    },
    /// Someone else changed the note and the local buffer is clean, so the new
    /// text can be applied in one `external`-annotated transaction without
    /// polluting the user's undo history.
    External {
        rev: String,
        frontmatter: String,
        text: String,
    },
    /// Someone else changed the note and the local buffer is dirty in a way that
    /// overlaps. Never auto-applied: the editor raises the inline diff bar and the
    /// user decides (UX-DR40).
    Diverged {
        rev: String,
        /// The block on disk, which the user adopts along with `theirs`.
        frontmatter: String,
        /// The body now on disk.
        theirs: String,
    },
    /// The note moved. Its id is unchanged, so the editor retargets rather than
    /// closing.
    Renamed { path: String },
    /// The note no longer exists.
    Gone,
}

/// A batch of note-list operations on the changes channel.
///
/// Coalesced to at most one message per 250 ms per subscription, so a 500-file
/// agent run is a handful of messages per second rather than five hundred.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteChangeBatch {
    pub vault_id: String,
    /// Ops in order; apply them in sequence.
    pub ops: Vec<NoteListOp>,
    /// The lens's counts as of this batch, on
    /// [`NoteListVm::total`]/[`NoteListVm::matched`]'s terms (Story 44.11).
    ///
    /// **On the envelope rather than inside `Reset`, and not derived by the
    /// receiver.** Both numbers are recomputed over the whole matched set for
    /// every batch this loop sends, so the count on screen is the count Rust
    /// just took. The frontend used to carry `total` forward itself, adding one
    /// per `Upsert` of an unseen id and subtracting one per `Remove` — which is
    /// right only while every change to the matched set also changes the
    /// window. A note that starts matching a filter three thousand rows below
    /// the page moves no row and used to move no count, and after Story 44.10
    /// windowed the list there is no scroll that would have corrected it.
    pub total: u32,
    /// How many the lens matched before `keeper.limit` declined any.
    pub matched: u32,
}

/// One index-based note-list operation.
///
/// Tagged `op` rather than `kind`, matching `RoomListOp`/`TimelineOp`/`InboxOp`:
/// this is the same index-diff shape those three already established, and the
/// frontend applies it to a plain array by index without re-sorting.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum NoteListOp {
    /// Replace the whole window. Opens every subscription.
    ///
    /// Carries no count of its own: [`NoteChangeBatch::total`] is the one
    /// answer, and a second copy on the op that only some batches contain is a
    /// second copy that goes stale between them.
    Reset { rows: Vec<NoteRowVm> },
    /// Insert or replace the row at `index`.
    Upsert { index: u32, row: NoteRowVm },
    /// Drop the row with this note id, wherever it currently sits.
    Remove { id: String },
}

/// The outcome of a write (NFR-30).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteWriteVm {
    /// The revision the bytes on disk now are.
    pub rev: String,
    /// The note's path after the write (a retitle can move it).
    pub path: String,
    /// The frontmatter block as it now stands on disk, in the same space as
    /// [`NoteBodyBatch`]'s. Returned rather than assumed because every save
    /// rewrites `updated`, so the block the caller sent is never quite the block
    /// that landed — and the properties panel would otherwise show a stale
    /// timestamp until the next external write.
    pub frontmatter: String,
    /// The conflict copy written before the save, when the save was based on a
    /// revision older than disk. An ordinary tracked file, so it becomes a
    /// conflict row and a commit like anything else — nothing is lost, and the
    /// user is told where the other side went.
    pub conflict_copy: Option<String>,
}

/// One revision of a note, projected from commit trailers (FR-114, AD-63).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteRevisionVm {
    pub rev: String,
    /// Commit time, ms since the Unix epoch.
    #[ts(type = "number")]
    pub when_ms: i64,
    /// `Keeper-Device`: which machine wrote it.
    pub device: String,
    /// `Keeper-Origin`: how the change entered keeper.
    pub origin: String,
    /// `Keeper-Source`: `bot` for an agent write, else the human path.
    pub source: String,
    /// The commit subject, already single-line sanitised.
    pub subject: String,
}

/// A diff between two revisions of one note (FR-114).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteDiffVm {
    pub from_rev: String,
    /// `None` means the working tree — what is on disk right now.
    pub to_rev: Option<String>,
    pub hunks: Vec<NoteHunkVm>,
}

/// One unified-diff hunk. Line numbers are 1-based, matching what the gutter
/// shows.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteHunkVm {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// The hunk body, `+`/`-`/space prefixed per line.
    pub text: String,
}

/// One unresolved conflict (FR-116).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteConflictVm {
    /// The note the conflict is about.
    pub id: String,
    /// The note's own path.
    pub path: String,
    /// The revision this device's copy is based on.
    pub mine_rev: String,
    /// The conflict copy's path — a real, tracked file the user can open.
    pub theirs_path: String,
}

/// An attachment that has been written into the vault (FR-110).
///
/// Only the reference crosses IPC. The bytes were read and written entirely in
/// Rust, and the webview reaches them through the custom protocol at `url`
/// (AD-58) rather than through a base64 payload.
///
/// **There was a `markdown` field here and Story 45.13 deleted it.** It carried
/// `![name](attachments/name.png)` — CommonMark's embed — while the attachments
/// panel wrote `![[attachments/name.png]]`, Obsidian's. Two spellings for one
/// act, and only the second is decorated by `live-preview.ts`, so an attachment
/// imported through this VM would have rendered as flat text. It was never
/// noticed because nothing in the webview has read this field since epic 37:
/// `notes_attachment_drop` and `notes_attachment_paste` have client wrappers
/// and no callers. A dead field is not a spare part, it is an untested code
/// path waiting for its first caller — which is how `NoteCreateReq.dest`
/// turned out to be an armed data-loss path the moment something set it. The
/// one spelling now lives in `src/lib/notes/attach.ts` and nowhere else.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteAttachmentVm {
    /// Vault-relative path of the written file.
    pub rel_path: String,
    /// `keeper-note://…` URL the webview can render.
    pub url: String,
}

/// One file offered for attaching, resolved to something a note can name
/// (Story 45.13, FR-188, FR-189).
///
/// The three entry points hand over three different kinds of path — a picker's
/// absolute path, a Files-pane row's absolute path, a recording note's own
/// relative one — and this is what they all become before any note is touched.
/// The webview never turns one into the other: it does not know where the vault
/// is (AD-65) and must never hold an absolute path long enough to write it
/// (FR-145).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteAttachSourceVm {
    /// The file's own name, so a refusal can say which file it is about even
    /// when there is no path to show.
    pub name: String,
    /// The vault-relative path a note may name, or `None` when keeper refused
    /// this source. Exactly one of this and `refusal` is `Some`.
    pub rel_path: Option<String>,
    /// Whether keeper had to copy the file into the vault to make it nameable.
    ///
    /// Reported rather than inferred, because it is a thing that happened to
    /// the user's disk and the surface says so. `false` means the file was
    /// already in the vault and the note names it where it lies — no second
    /// copy, which is what the dead `notes_attachment_drop` would have made.
    pub copied: bool,
    /// Why this source produced no path — a directory, an unreadable file, a
    /// copy that failed. A finished sentence, worded here on this module's
    /// standing rule that Rust words what Rust decided.
    pub refusal: Option<String>,
}

/// One note offered as somewhere to attach files (Story 45.13, FR-189).
///
/// A sibling of [`NoteLinkTargetVm`] rather than a field on it: the wikilink
/// autocomplete asks "which note do you mean", and this asks "which note should
/// receive these files", which is a different question with a different answer
/// for the same note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteAttachTargetVm {
    pub id: String,
    pub title: String,
    pub path: String,
    /// Of the file names the caller asked about, the ones this note's body
    /// already embeds — folded to lower case, in no particular order.
    ///
    /// **The subset, not the note's whole set, and never the body.** A list
    /// never ships bodies (AD-58), and shipping every embed of every candidate
    /// would make the payload a function of how much the vault holds rather
    /// than of what was asked.
    pub holds: Vec<String>,
}

/// A note's body as it is on disk right now (Story 45.13).
///
/// The read half of the one read-modify-write a surface can do to a note it has
/// not opened in the editor. `rev` is what the write must be based on, so a
/// note that changed underneath is conflict-copied rather than clobbered — the
/// same guarantee `notes_save` gives the editor, through the same code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteBodyVm {
    /// Content revision of the whole file these bytes came from.
    pub rev: String,
    /// The body, with the frontmatter block removed — the same space
    /// [`NoteBodyBatch`] and `notes_save` speak in.
    pub text: String,
}

/// A CSV attachment projected as a table (Story 44.16, FR-172).
///
/// Cells, not bytes: the file's quoting, terminators and byte-order mark stay
/// in [`crate::notes::csv`], which is the only thing that ever writes them
/// back. The webview holds displayed values and the coordinates they came from,
/// so an edit is "row 4, column 2 is now this" rather than a re-serialised
/// file — the webview cannot reformat what it cannot spell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteCsvVm {
    /// Vault-relative path of the file that was actually read, which may differ
    /// from the embed's target: `![[data.csv]]` names a file the shell locates.
    pub rel_path: String,
    /// Content revision of the bytes these cells came from. An edit sends it
    /// back, so a file that changed underneath is refused instead of clobbered.
    pub rev: String,
    /// Columns the first record has — the width the table draws.
    pub columns: u32,
    /// Records in the whole file, which `rows` may be only the first of.
    pub total_rows: u32,
    /// The records this table shows, in file order.
    pub rows: Vec<NoteCsvRowVm>,
    /// Finished sentences about anything odd: a ragged row, a quote that never
    /// closes, a row count that was capped. Empty when there is nothing to say.
    /// Worded here rather than in the webview, on this module's standing rule.
    pub notices: Vec<String>,
}

/// One record of a CSV table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteCsvRowVm {
    /// 0-based index of this record in the **file**, not in `rows`. An edit
    /// sends this back, so a capped window still names the right record.
    pub index: u32,
    /// 1-based line the record starts on, so a notice about it points at
    /// something the user can find in a text editor.
    pub line: u32,
    /// The displayed values, exactly as many as the record has.
    pub cells: Vec<String>,
    /// Whether this record's field count differs from `columns`. Shown as odd
    /// rather than padded or dropped: a table that loses somebody's row is
    /// worse than a table that admits the row is strange.
    pub ragged: bool,
}

/// One folder, listed for a note's gallery block (Story 44.15, FR-171, AD-84).
///
/// **Every entry, classified — not only the ones a gallery shows.** The kind is
/// [`crate::vm::RecordingNoteTargetKind`], decided by the one classifier
/// (AD-73), and a `File` or a `Folder` crosses the wire with the rest. Which
/// kinds a gallery renders is the gallery's rule and not the listing's, and
/// filtering here would make "a non-media file is skipped" a claim no test
/// outside the Tauri shell can reach.
///
/// **Pinning is nowhere in this VM, on purpose.** A pin lives in the NOTE that
/// holds the block, so two notes over one folder pin different things. A pin
/// stored beside the photos would be one note editing another note's view, and
/// there is no field here it could be written into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteGalleryVm {
    /// The vault-relative folder that was listed, echoed back so a reply that
    /// arrives after the block was retargeted can be discarded rather than
    /// rendered under the wrong heading.
    pub folder: String,
    /// The folder's entries in the listing's own order, or empty when
    /// `problem` says why there are none.
    pub items: Vec<NoteGalleryItemVm>,
    /// Whether the listing cap cut the folder short. Said, never hidden.
    pub truncated: bool,
    /// A finished sentence for a folder that could not be listed — missing,
    /// unreadable, or a path that escapes the vault. Composed in Rust because
    /// the reason is Rust's: the webview never learns which of the three it
    /// was, only what to show. `None` when the listing succeeded, including
    /// when it succeeded and found nothing.
    pub problem: Option<String>,
}

/// One entry of a listed gallery folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteGalleryItemVm {
    /// The entry's own file name, with no path in it — what a tile is labelled.
    pub name: String,
    /// The entry's vault-relative path, `/`-joined. This is what a pin is
    /// written as, so the note holds a path Obsidian resolves and never an
    /// absolute one (FR-145).
    pub rel_path: String,
    /// What this entry is, from the one classifier (Story 43.5, AD-73).
    pub kind: RecordingNoteTargetKind,
    /// The `keeper-note://…` URL a tile's element loads, composed here so the
    /// webview never joins a root and a subpath (AD-65). Present for every
    /// entry the protocol will serve and `None` for the rest — a `File` or a
    /// `Folder` has no URL because nothing asks for its bytes.
    pub url: Option<String>,
}

/// One wikilink autocomplete candidate (FR-108).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteLinkTargetVm {
    pub id: String,
    pub title: String,
    pub path: String,
}

/// One batch of streamed content-search results (FR-118).
///
/// The first batch may be empty; `done` is what ends the spinner, so a search
/// that finds nothing still terminates honestly.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteSearchBatch {
    pub done: bool,
    pub hits: Vec<NoteSearchHitVm>,
}

/// One content-search hit.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteSearchHitVm {
    pub id: String,
    pub path: String,
    pub title: String,
    /// 1-based line number of the hit.
    pub line: u32,
    /// The matching line, trimmed for display.
    pub snippet: String,
}

/// Cold-scan progress for one vault.
///
/// `total_estimate` is an estimate and is allowed to move: the scan discovers the
/// vault as it walks it, and a progress bar that lies about being nearly done is
/// less annoying than one that refuses to appear until the count is exact.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteIndexProgressVm {
    pub vault_id: String,
    pub scanned: u32,
    pub total_estimate: u32,
    /// Which phase the scan is in, e.g. `enumerating`, `parsing`, `done`.
    pub phase: String,
}

/// The note-list query the frontend sends (FR-103).
///
/// `origin` and `flags` are plain strings rather than closed enums on purpose:
/// the space DSL's `origin:` accepts `device:<label>`, which no enum can spell,
/// and the flag set grows without a binding regeneration.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteQueryReq {
    /// Free text; `None` for no text filter.
    pub text: Option<String>,
    /// The tag chips, keyed by tag and ANDed together (FR-148, UX-DR54).
    ///
    /// A map rather than an include list beside an exclude list, because the
    /// chip that produces these has three states and a tag is in exactly one of
    /// them: keyed by tag, "include and exclude the same tag" is a request that
    /// cannot be written down rather than one
    /// [`IndexEntry::matches_tags`](crate::notes::index::IndexEntry::matches_tags)
    /// has to resolve by precedence. An off chip is an absent key — a term that
    /// admits everything has no business on the wire.
    pub tags: BTreeMap<String, NoteTagTerm>,
    /// When set, the space whose query further narrows the result.
    pub space_id: Option<String>,
    /// `local` | `agent` | `remote` | `device:<label>`.
    pub origin: Option<String>,
    /// `is:` flag names the result must carry.
    pub flags: Vec<String>,
    /// Where this page starts in the selected set.
    pub offset: u32,
    /// How many rows this PAGE carries — the transport window, and the only
    /// limit a caller owns. A space's own `keeper.limit` caps what the space
    /// selects and is not this (Story 44.11); the page walks over whatever the
    /// space selected.
    pub limit: u32,
}

/// A new note (FR-98). No dialog anywhere in this path — every field is optional
/// because the fast path is "make me a note now" with nothing filled in.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteCreateReq {
    /// Title; derived from the first body line when absent.
    pub title: Option<String>,
    pub body: Option<String>,
    /// Vault-relative path of a template to expand.
    pub template: Option<String>,
    /// Destination directory, vault-relative; the vault root when absent.
    pub dest: Option<String>,
    pub tags: Vec<String>,
    /// The id of the space note this create was asked for from, when the ask
    /// came from a space row rather than from the rail (Story 44.6, FR-160).
    ///
    /// The space's **id**, never its query text: the shell reads the space note
    /// and derives what the new note has to carry through
    /// [`crate::notes::seed`], so no surface outside Rust ever parses a query
    /// or decides what `is:pinned` means (AD-58). An id naming no space creates
    /// an ordinary note — a space deleted between the click and the write is
    /// not a reason to lose the thought.
    pub space: Option<String>,
}

/// What a create produced, and anything the person who asked for it has to be
/// told (Story 44.6).
///
/// `notices` exists because creation can succeed and still not do what the user
/// meant: a note created in a space whose query no new note can satisfy is in
/// the vault and not in that space, and a create that returned only a
/// [`NoteRefVm`] would leave the surface to guess at that — or, worse, to work
/// it out by parsing the query itself. Each entry is a finished sentence
/// composed in Rust; an empty list is the ordinary case and renders nothing.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteCreateVm {
    /// The note that now exists.
    pub note: NoteRefVm,
    /// Sentences to show beside the list, in the order they were decided.
    pub notices: Vec<String>,
}

/// A streamed content search (FR-118).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteSearchReq {
    pub text: String,
    pub limit: u32,
}

/// Create or update a space (FR-105, FR-149).
///
/// A complete description of the space, not a patch: an update rewrites the
/// definition wholesale, so a caller that omits a field is saying "this space
/// has none" rather than "leave it alone". That is the opposite of
/// [`NoteVaultSettingsReq`]'s rule and it is deliberate — a space is a handful
/// of values on one form, all of them on screen at once, so "absent means
/// unchanged" would only be a way for a stale form to resurrect a term the
/// user deleted.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteSpaceReq {
    /// The space note's id when updating; `None` creates one.
    pub id: Option<String>,
    /// The space's name. On an update this retitles the note and renames its
    /// file, so it is the one field here that touches bytes outside the
    /// `keeper:` key.
    pub name: String,
    pub query: String,
    /// How the space orders what it lists: `<key> <dir>`, the text
    /// [`crate::notes::sort::read`] accepts. The editor sends the canonical
    /// spelling of whatever it had selected, which is the one place a value
    /// keeper could not read *is* rewritten — the form showed the fallback and
    /// said why, so pressing Save is a repair the user watched happen rather
    /// than a rewrite behind their back.
    pub sort: String,
    /// The selection cap to store, or zero for none. Zero writes no
    /// `keeper.limit` key at all, on the same rule `icon` and `order` follow: a
    /// space nobody capped keeps the frontmatter it had rather than growing a
    /// key to explain a cap it does not have (Story 44.11).
    pub limit: u32,
    /// The chosen icon's name, or `None` to leave the space without one.
    pub icon: Option<String>,
    /// The space's position in the rail. Zero is "unpositioned" and is not
    /// written to the file, so a space nobody ordered grows no key to explain.
    pub order: f64,
    /// The template to hand out, or `None`/empty to leave the space without one.
    /// An empty string clears the key rather than storing a template whose path
    /// is nothing.
    pub template: Option<String>,
    /// Where notes created in this space are written — vault-relative, or
    /// `None`/empty to leave the destination to the query (Story 44.13). An
    /// empty string clears the key rather than storing a folder that names
    /// nothing.
    pub folder: Option<String>,
}

/// Change a vault's settings (FR-120).
///
/// Every field is optional because a form that does not show a knob must never
/// reset it (AD-34-9): the request carries what the user changed, and everything
/// absent keeps the value already in force.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteVaultSettingsReq {
    pub subfolder: Option<String>,
    pub journal_template: Option<String>,
    pub default_template: Option<String>,
    /// The template a quick capture starts from. An empty string clears it —
    /// "the user chose no template" and "the user never touched the field" are
    /// different requests, and only the first may unset what is stored.
    pub capture_template: Option<String>,
    /// The tag every quick capture carries, as typed. keeper folds it to the
    /// canonical form before storing it, and an empty string clears it.
    pub capture_tag: Option<String>,
    pub cadence: Option<NoteCadenceVm>,
}

/// How to resolve one conflict (FR-116). Whichever arm is chosen, the conflict
/// copy is deleted afterwards — the resolution is a real write, not a dismissal.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum NoteConflictChoiceReq {
    /// Keep this device's copy.
    TakeMine,
    /// Keep the other side's copy.
    TakeTheirs,
    /// Keep text the user merged by hand. The **body** only, in the same space as
    /// [`NoteBodyBatch`]'s text: the resolver aligns bodies, and the canonical
    /// note's frontmatter block is re-attached by the write.
    Merged { text: String },
}

/// The two per-note flags a user can toggle (FR-119).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoteFlag {
    Pinned,
    Archived,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_serialises_camel_case_including_the_two_absent_by_empty_string_fields() {
        let row = NoteRowVm {
            id: "n1".to_owned(),
            path: "notes/a.md".to_owned(),
            title: "A".to_owned(),
            snippet: String::new(),
            tags: vec!["x".to_owned()],
            updated_ms: 1_700_000_000_000,
            pinned: false,
            archived: false,
            unread: true,
            conflict: false,
            origin: String::new(),
            head_rev: String::new(),
            order: NoteOrder::own(2.5),
        };
        let json = serde_json::to_string(&row).expect("serialize row");
        assert!(json.contains("\"updatedMs\":1700000000000"), "json: {json}");
        assert!(json.contains("\"headRev\":\"\""), "json: {json}");
        assert!(json.contains("\"origin\":\"\""), "json: {json}");
        // The webview switches on this discriminant, so it has to be the
        // camelCase name and not a Rust variant spelling.
        assert!(
            json.contains("\"order\":{\"value\":2.5,\"source\":\"own\"}"),
            "json: {json}"
        );
        let back: NoteRowVm = serde_json::from_str(&json).expect("deserialize row");
        assert_eq!(back.id, row.id);
        assert!(back.unread);
    }

    #[test]
    fn the_body_channel_is_tagged_kind_and_the_list_channel_is_tagged_op() {
        // Two different tags on purpose: a document-state batch and an index-diff
        // op are different shapes, and the list op matches the three list-diff
        // enums the frontend already applies (RoomListOp/TimelineOp/InboxOp).
        let reset = NoteBodyBatch::Reset {
            rev: "r1".to_owned(),
            path: "inbox/note.md".to_owned(),
            frontmatter: "---\nid: 01AAA\n---\n".to_owned(),
            text: "hello".to_owned(),
            cursor: Some(3),
        };
        let json = serde_json::to_string(&reset).expect("serialize body batch");
        assert!(json.contains("\"kind\":\"reset\""), "json: {json}");
        // The block is its own field, so the body the editor gets holds no `---`.
        assert!(
            json.contains("\"frontmatter\":\"---\\nid: 01AAA\\n---\\n\""),
            "json: {json}"
        );
        assert!(json.contains("\"text\":\"hello\""), "json: {json}");

        let gone = serde_json::to_string(&NoteBodyBatch::Gone).expect("serialize gone");
        assert_eq!(gone, "{\"kind\":\"gone\"}");

        let op = NoteListOp::Remove {
            id: "n1".to_owned(),
        };
        let json = serde_json::to_string(&op).expect("serialize list op");
        assert_eq!(json, "{\"op\":\"remove\",\"id\":\"n1\"}");
    }

    #[test]
    fn request_enums_serialise_to_the_names_the_frontend_switches_on() {
        let merged = NoteConflictChoiceReq::Merged {
            text: "resolved".to_owned(),
        };
        let json = serde_json::to_string(&merged).expect("serialize choice");
        assert!(json.contains("\"kind\":\"merged\""), "json: {json}");
        let mine = serde_json::to_string(&NoteConflictChoiceReq::TakeMine).expect("serialize mine");
        assert_eq!(mine, "{\"kind\":\"takeMine\"}");
        assert_eq!(
            serde_json::to_string(&NoteFlag::Pinned).expect("serialize flag"),
            "\"pinned\""
        );
    }

    #[test]
    fn an_omitted_settings_knob_stays_omitted_rather_than_becoming_a_default() {
        // AD-34-9: a form that does not show a knob must not reset it, which only
        // works if the wire can actually express "not mentioned".
        let req: NoteVaultSettingsReq =
            serde_json::from_str("{\"subfolder\":\"notes\"}").expect("deserialize partial");
        assert_eq!(req.subfolder.as_deref(), Some("notes"));
        assert!(req.journal_template.is_none());
        assert!(req.cadence.is_none());
    }

    #[test]
    fn a_query_check_carries_a_byte_span_the_editor_can_underline() {
        let check = NoteQueryCheckVm {
            ok: false,
            message: Some("unknown search key `colour`".to_owned()),
            token_index: Some(1),
            span: Some((6, 16)),
        };
        let json = serde_json::to_string(&check).expect("serialize check");
        assert!(json.contains("\"tokenIndex\":1"), "json: {json}");
        assert!(json.contains("\"span\":[6,16]"), "json: {json}");
    }

    /// Story 45.17: a confirmation NAMES what goes, and says where it went.
    ///
    /// Both halves matter and they fail differently. A confirmation that does
    /// not name the note is one people cancel out of; one that does not say the
    /// bytes are recoverable is one they never press at all.
    #[test]
    fn a_note_deletion_names_the_note_and_where_its_bytes_go() {
        let plan = NoteDeletePlanVm::for_note("Standup", "meetings/2026-08-09-standup.md");

        assert!(plan.question.contains("Standup"), "{}", plan.question);
        assert!(
            plan.consequence.contains("meetings/2026-08-09-standup.md"),
            "{}",
            plan.consequence
        );
        assert!(plan.recovery.contains("trash"), "{}", plan.recovery);
        assert_eq!(plan.path, "meetings/2026-08-09-standup.md");
    }

    /// A space's confirmation answers the question that stops people deleting a
    /// saved view: does this take the notes with it. It does not, and the
    /// sentence has to say so.
    #[test]
    fn a_space_deletion_says_the_notes_it_lists_stay() {
        let plan = NoteDeletePlanVm::for_space("Clients", "spaces/clients.md", None);

        assert!(plan.question.contains("Clients"), "{}", plan.question);
        assert!(plan.question.contains("space"), "{}", plan.question);
        assert!(
            plan.consequence
                .contains("every note it lists stays where it is"),
            "{}",
            plan.consequence
        );
        assert!(plan.recovery.contains("trash"), "{}", plan.recovery);
    }

    /// **The clause is driven by the marker, not by the name.**
    ///
    /// A seeded Recordings space renamed to "Sessions" is still keeper's, and a
    /// space of the user's own called "Recordings" is not — so both are checked
    /// here, and a composer partitioning on the name would get both backwards.
    /// The promise it carries is specific: keeper will not add it back, and
    /// Restore is how you get it.
    #[test]
    fn only_a_seeded_space_promises_to_stay_deleted() {
        // One path in both, so the ONLY difference between the two sentences is
        // the clause the marker adds. Two paths would have made the comparison
        // below trivially false, and the test would have been asserting that
        // two paths differ, which nobody doubts.
        let path = "spaces/2026-08-09-recordings.md";
        let renamed = NoteDeletePlanVm::for_space("Sessions", path, Some("recordings"));
        assert!(
            renamed.consequence.contains("will not add it back"),
            "{}",
            renamed.consequence
        );
        assert!(
            renamed.consequence.contains("Restore default spaces"),
            "{}",
            renamed.consequence
        );

        // A space of the user's own that happens to be called Recordings is
        // not keeper's, and keeper promises nothing about it.
        let theirs = NoteDeletePlanVm::for_space("Recordings", path, None);
        assert!(
            !theirs.consequence.contains("will not add it back"),
            "keeper must not promise anything about a space it did not seed: {}",
            theirs.consequence
        );
        assert!(
            renamed.consequence.starts_with(&theirs.consequence),
            "the two must differ by exactly the added clause\nseeded: {}\nnot seeded: {}",
            renamed.consequence,
            theirs.consequence
        );
    }
}
