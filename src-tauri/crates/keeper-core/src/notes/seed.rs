//! What a new note must carry for the space it was created in to select it
//! (Story 44.6, FR-160).
//!
//! "New note in this space" is only honest if the note then appears in that
//! space. A create that drops the file at the vault root and lets the user
//! discover it is missing is the failure this module exists to prevent: the
//! space is a saved query, so creating "into" one means writing a note the
//! query already matches.
//!
//! Two functions, in the order the shell calls them.
//!
//! [`inherit`] reads the query and says what to give the note — its tags, its
//! folder, its flags. It is a **best effort**: the DSL can ask for facts no
//! creation can produce (`is:recording` names a note keeper writes about a
//! finished recording; `is:unread` is a per-device mark; `backlink:` is
//! somebody else's file), and it can ask for two folders at once.
//!
//! [`verdict`] is the authority. The shell composes the note, indexes the bytes
//! it is about to write through the same parser the reconciler uses, and asks
//! this module whether the query selects it. There is therefore no second
//! opinion about what a space matches anywhere in the app: the answer comes
//! from [`crate::notes::query::eval`], the one evaluator, run over the one
//! index shape. A seed that turns out not to be enough produces a sentence
//! naming the terms that defeated it, and the note is still created — losing
//! the thought over an unsatisfiable saved view would be the worse trade.
//!
//! Nothing here parses a query in TypeScript, and nothing here re-derives what
//! `is:` means. The frontend sends "which space", and Rust answers with a note
//! and, when it must, one finished sentence (AD-55, AD-58).

use std::collections::BTreeMap;

use crate::notes::index::IndexEntry;
use crate::notes::naming;
use crate::notes::order::NoteOrder;
use crate::notes::query::{self, Term};
use crate::notes::tags;
use crate::notes::templates;

/// The folder whose prefix makes the index flag a note `journal`.
///
/// It inverts `keeper::notes_vault::parse_note`, which decides the flag with
/// `rel.starts_with("journal/")`. Deliberately **not** the vault's configured
/// journal path template: that template names where `⌘⌥J` puts today's entry,
/// while this names what the `is:journal` predicate is actually true of, and a
/// vault whose template points elsewhere would otherwise get a note that says
/// it is in the Journal space and is not.
pub const JOURNAL_DIR: &str = "journal";

/// The folder prefix that makes the index flag a note `space`.
///
/// Inverts `keeper::notes_vault::parse_note`'s `rel.starts_with("spaces/")`,
/// with the separator baked in so a folder called `spaces-archive` is not one.
pub const SPACES_DIR: &str = "spaces/";

/// Characters that make a path segment a pattern rather than a folder name.
const GLOB_META: [char; 6] = ['*', '?', '[', ']', '{', '}'];

/// What a create should give a new note so a space's query selects it.
///
/// Every field is something the create path already writes. There is no field
/// here for a frontmatter key a query happened to name (`field:priority=high`),
/// because writing arbitrary frontmatter out of a filter would make "new note"
/// a data-entry form, and because the story's vocabulary is the three things a
/// space actually selects on: its tags, its folder, its flag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seed {
    /// Tags to add, normalised and de-duplicated, in the order the query wrote
    /// them.
    pub tags: Vec<String>,
    /// The vault-relative directory to create in, or `None` for the vault root.
    pub dest: Option<String>,
    /// Write `pinned: true`.
    pub pinned: bool,
    /// Write `archived: true`.
    pub archived: bool,
    /// Write the reserved `keeper.capture` mark.
    pub capture: bool,
}

impl Seed {
    /// Whether this seed asks for anything at all.
    ///
    /// A space that seeds nothing is not a broken space — `is:untagged` is
    /// satisfied by a note with no tags, so the empty seed is exactly right for
    /// Inbox — which is why the caller must not read this as a failure.
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
            && self.dest.is_none()
            && !self.pinned
            && !self.archived
            && !self.capture
    }
}

/// Read a space's query as instructions for creating a note in it.
///
/// Never fails and never refuses: a query that does not parse, or that has
/// structure a single note cannot stand in for (`|`, a group), yields the empty
/// seed. [`verdict`] is what tells the user about it, once, in a sentence, and
/// only when the note that was actually written really is outside the space.
///
/// A negated term seeds nothing on purpose. `-tag:draft` is already true of a
/// note with no tags, and there is no such thing as writing "not in this
/// folder" into a file.
pub fn inherit(query: &str) -> Seed {
    let mut seed = Seed::default();
    let Some(terms) = query::conjunction(query) else {
        return seed;
    };
    for term in &terms {
        if term.negated {
            continue;
        }
        seed_term(&mut seed, term);
    }
    seed
}

/// Apply one term to the seed. Anything not named here is left to [`verdict`].
fn seed_term(seed: &mut Seed, term: &Term) {
    match term.key.as_deref() {
        // `tag:x/*` is the subtree *without* its own node, so there is no one
        // tag that satisfies it — the same refusal the chip vocabulary makes.
        // A value that is not a tag normalises away and seeds nothing, which
        // leaves `tag:---` matching nothing rather than tagging a note `---`.
        Some("tag") if !term.value.ends_with("/*") => {
            if let Some(tag) = tags::normalise(&term.value) {
                add_tag(seed, tag);
            }
        }
        Some("is") => seed_flag(seed, term.value.trim()),
        Some("path") => {
            if let Some(dir) = literal_dir(&term.value) {
                set_dest(seed, dir);
            }
        }
        _ => {}
    }
}

/// Add a tag the seed does not already carry.
///
/// One function rather than the check written twice, because two producers now
/// reach it — a `tag:` term and `is:template` — and a duplicate in the list
/// becomes a duplicate in the note's frontmatter, which Obsidian renders as two
/// identical chips on a note nobody tagged twice.
fn add_tag(seed: &mut Seed, tag: String) {
    if !seed.tags.contains(&tag) {
        seed.tags.push(tag);
    }
}

/// The `is:` flags a create can make true, and only those.
///
/// The parser folds case before it matches its closed flag set, so this does
/// too: `is:Pinned` and `is:pinned` are one flag written twice, and a seed that
/// recognised only the lowercase spelling would silently create an unpinned
/// note in the Pinned space.
///
/// Deliberately absent, each for a stated reason:
///
/// - `recording`, `conflict`, `orphan`, `unparsed`, `oversize`,
///   `unstable_identity` — facts another subsystem produces. A note keeper
///   fabricated one of these into would be lying about its own vault.
/// - `unread` — a per-device mark about somebody else's write. A note you just
///   typed is read by definition.
/// - `space` — `spaces/` is the one folder where the folder decides the file's
///   *kind*. A note with no query dropped there is a broken space row the user
///   never asked for, so creation declines rather than manufacturing one.
/// - `untagged` — already true of a note with no tags, so there is nothing to
///   do. Seeding it would also have to *remove* a tag another term asked for,
///   and a seed that argues with itself is worse than one that declines.
fn seed_flag(seed: &mut Seed, flag: &str) {
    if flag.eq_ignore_ascii_case("pinned") {
        seed.pinned = true;
    } else if flag.eq_ignore_ascii_case("archived") {
        seed.archived = true;
    } else if flag.eq_ignore_ascii_case("capture") {
        seed.capture = true;
    } else if flag.eq_ignore_ascii_case("journal") {
        set_dest(seed, JOURNAL_DIR.to_owned());
    } else if flag.eq_ignore_ascii_case(templates::TEMPLATE_TAG) {
        // A template is a note tagged `template` (AD-82, Story 44.7), so making
        // a note a template is adding one tag — not putting it in a folder
        // keeper owns. New Note in a space filtered `is:template` therefore
        // makes a new template, which is the only thing that ask can mean.
        //
        // This arm was deliberately absent while 44.7 was in flight, because
        // the predicate was moving off the `templates/` prefix and onto the tag
        // as this story was written, and seeding either would have been seeding
        // against a rule that was about to stop being true. 44.7 has landed —
        // `notes_vault::parse_note` now reads `templates::is_template` — so it
        // is here, and DW-167 is closed.
        add_tag(seed, templates::TEMPLATE_TAG.to_owned());
    }
}

/// The first folder named wins.
///
/// Two folders in one query is a query no single file satisfies, and picking
/// the second would make the outcome depend on term order in a grammar whose
/// terms are otherwise commutative. The note lands in the first, and
/// [`verdict`] says it will not appear.
fn set_dest(seed: &mut Seed, dir: String) {
    if seed.dest.is_none() {
        seed.dest = Some(dir);
    }
}

/// The literal directory a `path:` glob commits to, if any.
///
/// The **last** segment is always dropped: it is the filename pattern, and a
/// creation whose filename comes from the note's first line cannot promise to
/// match it. So `journal/**` gives `journal`, `journal/2026/*.md` gives
/// `journal/2026`, and `*.md` gives nothing. Everything up to the first
/// segment carrying a glob character is taken; the rest is a pattern, not a
/// place.
///
/// `.`, `..` and an absolute leading `/` end the prefix rather than being
/// walked, because a destination assembled from a query must never be able to
/// name a path outside the vault (AD-65, FR-145).
fn literal_dir(glob: &str) -> Option<String> {
    let trimmed = glob.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut segments = trimmed.split('/').collect::<Vec<_>>();
    segments.pop();
    let mut dir: Vec<&str> = Vec::new();
    for segment in segments {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains(GLOB_META) {
            break;
        }
        dir.push(segment);
    }
    if dir.is_empty() {
        None
    } else {
        Some(dir.join("/"))
    }
}

/// Whether the space will list the note that was just written, and what to say
/// when it will not.
///
/// `created` is the index entry for the bytes the shell is about to write,
/// produced by the reconciler's own parser — so this asks the real evaluator
/// about the real note rather than about the seed's intentions. `body` is that
/// note's body, for the one predicate (`text:`) that reads a file.
///
/// `None` means the note is selected and there is nothing to say. `Some` is one
/// finished sentence, composed here rather than in the webview so that the
/// wording, the term names and the space's name cannot drift from what the
/// query actually did (AD-55).
pub fn verdict(
    space_name: &str,
    query: &str,
    created: &IndexEntry,
    body: &str,
    now_ms: i64,
) -> Option<String> {
    let Ok(parsed) = query::parse(query) else {
        return Some(format!(
            "{space_name}'s query can't be read, so it selects nothing. This note is in the vault, but it won't appear there."
        ));
    };
    if query::eval(&parsed, created, &mut || body.to_owned(), now_ms) {
        return None;
    }
    // Which terms defeated it. Every term of a conjunction has to hold, so a
    // term that fails on its own is a term that failed here — and naming them
    // is the difference between "this didn't work" and "keeper can't make a
    // new note that is a recording".
    let failed: Vec<String> = query::conjunction(query)
        .unwrap_or_default()
        .into_iter()
        .filter(|term| {
            query::parse(&term.source)
                .is_ok_and(|one| !query::eval(&one, created, &mut || body.to_owned(), now_ms))
        })
        .map(|term| term.source)
        .collect();
    if failed.is_empty() {
        // A query with structure — `|`, a group — has no single term to blame.
        return Some(format!(
            "A new note can't satisfy {space_name}'s query, so this note is in the vault but won't appear there."
        ));
    }
    Some(format!(
        "A new note can't satisfy {}, so this note is in the vault but won't appear in {space_name}.",
        join_terms(&failed)
    ))
}

/// `a`, `a and b`, `a, b and c` — the Oxford-less list an English sentence
/// wants, so a two-term reason does not read as a list of one.
fn join_terms(terms: &[String]) -> String {
    match terms {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The tag a configured capture-tag setting actually yields, or `None`
/// (Story 45.16, FR-193).
///
/// **One rule, read by both ends.** The settings save stores what this returns
/// so the form shows the value actually in force (AD-34-8), and [`capture`]
/// applies it. Storing what the user typed and folding it later would put two
/// spellings of one tag in front of them — the field saying `#Quick Capture`
/// and the note saying `quick-capture`.
///
/// Two refusals, both of which produce "no tag" rather than a bad one:
///
/// - **A tag that is not a tag.** `tags::normalise` already rejects `"   "`, a
///   bare `#` and `---`. Cleared and unusable are one state; inventing a
///   literal `---` tag out of the second is how a vault acquires a tag nobody
///   can type again.
/// - **The `template` marker itself.** AD-82 makes `template` mean "this note
///   is a scaffold", so a capture tag spelled `template` would make every
///   thought the user captures a template of itself — the exact failure 44.7's
///   marker-stripping exists to prevent, arriving through the front door
///   instead. A nested `template/inbox` is somebody's own filing under a word
///   keeper reserves at the root and is left alone, which is 44.7's ruling for
///   the copy path spelled the same way here.
pub fn capture_tag(configured: &str) -> Option<String> {
    let tag = tags::normalise(configured)?;
    (tag != templates::TEMPLATE_TAG).then_some(tag)
}

/// The seed a quick capture creates with (Story 45.16, FR-193).
///
/// One producer for both things a captured note carries, because there were
/// about to be two: the reserved `keeper.capture` mark lived in the shell's
/// commit path and the configured tag would have landed beside it. Two
/// producers of "what a capture carries" drift the moment one of them gains a
/// rule, and the symptom is a note that is a capture to one surface and not to
/// another.
pub fn capture(tag: Option<&str>) -> Seed {
    Seed {
        capture: true,
        tags: tag.and_then(capture_tag).into_iter().collect(),
        ..Seed::default()
    }
}

/// Whether `query` selects the note a quick capture *would* write, and what to
/// say when it does not.
///
/// This exists because 44.7 refused to tag its shipped templates and wrote down
/// why: Inbox is `is:untagged`, so a template that tags its copies files every
/// one of them straight out of the space that offered it. A capture tag is the
/// same hazard with a wider blast radius — it is not one template's notes, it
/// is every thought the user captures — and the honest way to know is to run
/// the space's real query through [`verdict`] rather than to reason about
/// `is:untagged` in a comment.
///
/// **Two facts about a capture are unknowable in advance**, and both are passed
/// as empty rather than invented: its title (the first line of text not yet
/// typed) and its body. So a space selecting on `text:` or on a title reports
/// that a capture will not appear, which is the honest answer to *will captures
/// appear here* and not a claim about any particular one.
pub fn capture_verdict(
    space_name: &str,
    query: &str,
    tag: Option<&str>,
    stamp: &str,
    now_ms: i64,
) -> Option<String> {
    let entry = projected(&capture(tag), "", "", stamp, now_ms);
    verdict(space_name, query, &entry, "", now_ms)
}

/// What configuring `tag` would COST this space, or `None` when it costs it
/// nothing (Story 45.16, FR-193).
///
/// `Some` only when the space lists a capture today and would stop. The
/// filtering is the decision, and it lives here rather than in the shell's
/// command because it is the difference between a surface that names the one
/// space you are about to lose and one that lists your whole rail with a
/// warning beside every row nobody could act on.
///
/// The three cases that are deliberately silent:
///
/// - **A space that never listed captures.** Not a cost of turning the tag on;
///   it was already not listing them and the tag changed nothing.
/// - **A space that lists them either way.** Nothing to say.
/// - **A space the tag would ADD captures to** — the `tag:` space this setting
///   exists to make possible. A gain is not a cost, and a surface that
///   announced gains would be promising that a space the user has not written
///   yet will fill up.
pub fn capture_tag_cost(
    space_name: &str,
    query: &str,
    tag: Option<&str>,
    stamp: &str,
    now_ms: i64,
) -> Option<String> {
    if capture_verdict(space_name, query, None, stamp, now_ms).is_some() {
        return None;
    }
    capture_verdict(space_name, query, tag, stamp, now_ms)
}

/// The index entry a note created with `seed` would produce.
///
/// It mirrors `keeper::notes_vault::parse_note` for exactly the facts a [`Seed`]
/// can set — the two boolean flags, the journal folder, the capture mark, the
/// template tag and the tag list — and **nothing else**: a fact a seed cannot
/// set is absent here rather than guessed, so a query that reads one gets the
/// same answer it would get for a note that has not acquired it yet.
///
/// It is a model of the shell's parser, and that is a real risk worth naming:
/// the shell is where the bytes are actually read, and this crate cannot
/// compile it (AD-56). What keeps the two honest is that the *rules* mirrored
/// here are each one line over there and each already lives in this crate —
/// `templates::is_template` decides the template flag, [`JOURNAL_DIR`] inverts
/// the journal one — so the model can only drift by someone adding a rule to
/// the parser, which is the moment to add it here.
///
/// `stamp` is the local timestamp keeper writes into `created:` and `updated:`;
/// its first ten characters are the date [`naming::note_filename`] prefixes, so
/// the filename this projects is the filename a create would pick in an empty
/// folder. `now_ms` must mean the same instant as `stamp`, because the date
/// predicates compare against it.
pub fn projected(seed: &Seed, title: &str, body: &str, stamp: &str, now_ms: i64) -> IndexEntry {
    let dir = seed.dest.clone().unwrap_or_default();
    let date = stamp.get(..DATE_LEN).unwrap_or_default();
    let filename = naming::note_filename(title, date, &[]);
    let path = if dir.is_empty() {
        filename
    } else {
        format!("{dir}/{filename}")
    };
    let mut flags = Vec::new();
    if seed.pinned {
        flags.push("pinned".to_owned());
    }
    if seed.archived {
        flags.push("archived".to_owned());
    }
    if path.starts_with(&format!("{JOURNAL_DIR}/")) {
        flags.push("journal".to_owned());
    }
    if seed.capture {
        flags.push("capture".to_owned());
    }
    // The one folder whose PREFIX decides a file's kind. `is:space` declines to
    // seed a destination for exactly that reason, but `path:spaces/**` does
    // not — so a seed really can land here, and a projection that missed it
    // would tell a spaces space its new note will not appear when it will.
    if path.starts_with(SPACES_DIR) {
        flags.push("space".to_owned());
    }
    // 44.7's rule, mirrored whole: a template is a note TAGGED `template`
    // (AD-82), **or** one under the grandfathered `templates/` prefix.
    // `notes_vault::parse_note` is `is_template(&fm) || rel.starts_with(…)` and
    // both halves are reachable from a seed — the tag through `is:template`,
    // the prefix through `path:templates/**`.
    if seed.tags.iter().any(|tag| tag == templates::TEMPLATE_TAG)
        || path.starts_with(&format!("{}/", templates::TEMPLATES_DIR))
    {
        flags.push("template".to_owned());
    }
    let mut fields = BTreeMap::new();
    fields.insert("created".to_owned(), stamp.to_owned());
    fields.insert("updated".to_owned(), stamp.to_owned());
    let mut tags = seed.tags.clone();
    tags.sort();
    IndexEntry {
        id: PROJECTED_ID.to_owned(),
        path,
        title: title.to_owned(),
        size: 0,
        mtime_ns: i128::from(now_ms) * 1_000_000,
        ino: 1,
        created_ms: now_ms,
        updated_ms: now_ms,
        tags,
        fields,
        links: Vec::new(),
        flags,
        snippet: body.to_owned(),
        order: NoteOrder::default(),
    }
}

/// `YYYY-MM-DD` — the leading characters of the stamp keeper writes, which is
/// the date a note's filename is prefixed with.
const DATE_LEN: usize = 10;

/// The id [`projected`] gives a note that does not exist yet.
///
/// A note keeper is about to write has no ULID until the create path mints one,
/// and no query predicate reads an id — `link:` and `backlink:` read the links
/// list, which a new note has none of. A recognisable constant rather than an
/// empty string, so anything that does surface it says where it came from.
const PROJECTED_ID: &str = "01PROJECTEDNOTE";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::default_spaces::DEFAULT_SPACES;

    /// Ten in the morning on 2026-08-09, UTC, in ms. Every date assertion below
    /// is relative to it, so none of them changes meaning tomorrow.
    const NOW_MS: i64 = 1_786_600_000_000;

    /// The same instant as [`NOW_MS`], spelled the way keeper writes it into
    /// `created:` — the two have to agree or the date predicates and the
    /// filename would be describing different days.
    const STAMP: &str = "2026-08-09T10:00:00+00:00";

    /// Create into `query` and report what the user would be told.
    ///
    /// Goes through [`projected`], which is production code since 45.16 — the
    /// test double it replaced was a second model of the same parser, and two
    /// models of one thing is the defect this whole module exists to refuse.
    fn create_into(name: &str, query: &str) -> (Seed, Option<String>) {
        let seed = inherit(query);
        let entry = projected(&seed, "Note", "", STAMP, NOW_MS);
        let told = verdict(name, query, &entry, "", NOW_MS);
        (seed, told)
    }

    /// AC1, over the four spaces a fresh vault actually has (Story 44.3).
    ///
    /// The seeded defaults are the queries every user meets first, so the round
    /// trip is asserted against `default_spaces::DEFAULT_SPACES` itself rather than
    /// against copies — a default whose query changed would fail here instead of
    /// quietly creating notes nobody can find.
    #[test]
    fn creating_into_a_seeded_default_produces_a_note_that_default_selects() {
        let expected = [
            ("inbox", true),
            ("journal", true),
            ("pinned", true),
            // The one the story names: keeper does not write recordings.
            ("recordings", false),
        ];
        for (key, selected) in expected {
            let space = DEFAULT_SPACES
                .iter()
                .find(|space| space.key == key)
                .unwrap_or_else(|| panic!("no default named {key}"));
            let (_, told) = create_into(space.name, space.query);
            assert_eq!(
                told.is_none(),
                selected,
                "{key} ({}) said: {told:?}",
                space.query
            );
        }
    }

    #[test]
    fn a_tag_space_tags_the_note_it_creates() {
        let (seed, told) = create_into("Client work", "tag:client/acme tag:billable");
        assert_eq!(seed.tags, ["client/acme", "billable"]);
        assert_eq!(told, None);
    }

    /// The tag vocabulary is the one the index uses, so a query written with a
    /// hash and a capital reaches the same node the tag tree shows.
    #[test]
    fn a_tag_term_is_normalised_by_the_one_definition_of_a_tag() {
        assert_eq!(inherit("tag:#Client/Acme").tags, ["client/acme"]);
    }

    /// A subtree-only term names no single tag, so nothing is invented for it —
    /// and because nothing is, the note honestly does not appear.
    #[test]
    fn a_descendants_only_tag_term_seeds_nothing_and_says_so() {
        let (seed, told) = create_into("Deep", "tag:client/*");
        assert!(seed.tags.is_empty());
        assert_eq!(
            told.as_deref(),
            Some("A new note can't satisfy tag:client/*, so this note is in the vault but won't appear in Deep.")
        );
    }

    #[test]
    fn a_negated_tag_is_already_true_of_a_new_note_and_seeds_nothing() {
        let (seed, told) = create_into("Not drafts", "tag:work -tag:draft");
        assert_eq!(seed.tags, ["work"]);
        assert_eq!(told, None);
    }

    #[test]
    fn the_pinned_space_creates_a_pinned_note() {
        let (seed, told) = create_into("Pinned", "is:pinned");
        assert!(seed.pinned);
        assert_eq!(told, None);
    }

    /// Case is the parser's business, not a second rule here.
    #[test]
    fn an_is_flag_is_matched_the_way_the_parser_folds_it() {
        assert!(inherit("is:Pinned").pinned);
    }

    #[test]
    fn the_journal_space_creates_in_the_folder_the_flag_is_computed_from() {
        let (seed, told) = create_into("Journal", "is:journal");
        assert_eq!(seed.dest.as_deref(), Some("journal"));
        assert_eq!(told, None);
    }

    #[test]
    fn a_path_glob_commits_to_its_literal_directory_and_never_to_its_pattern() {
        assert_eq!(literal_dir("journal/**"), Some("journal".to_owned()));
        assert_eq!(
            literal_dir("journal/2026/*.md"),
            Some("journal/2026".to_owned())
        );
        assert_eq!(literal_dir("journal/**/*.md"), Some("journal".to_owned()));
        // The last segment is dropped even when it is a plain word: `path:notes/inbox`
        // names one FILE, and a note whose filename comes from its first line
        // cannot promise to be it. Keeping it would file the note in a folder
        // called `inbox` that the query never mentioned.
        assert_eq!(literal_dir("notes/inbox"), Some("notes".to_owned()));
        assert_eq!(literal_dir("inbox"), None);
        assert_eq!(literal_dir("*.md"), None);
        assert_eq!(literal_dir(""), None);
    }

    /// AD-65 and FR-145: a destination assembled out of a saved query must not
    /// be able to name anything above the vault.
    #[test]
    fn a_traversal_in_a_path_glob_never_becomes_a_destination() {
        assert_eq!(literal_dir("../../etc/*.md"), None);
        assert_eq!(literal_dir("/etc/*.md"), None);
        assert_eq!(literal_dir("notes/../../*.md"), Some("notes".to_owned()));
    }

    /// Two folders is a query no one file satisfies. The note goes to the first
    /// and the user is told, rather than the create silently preferring one.
    #[test]
    fn two_destinations_take_the_first_and_the_second_is_reported() {
        let (seed, told) = create_into("Both", "path:journal/** path:archive/**");
        assert_eq!(seed.dest.as_deref(), Some("journal"));
        assert_eq!(
            told.as_deref(),
            Some("A new note can't satisfy path:archive/**, so this note is in the vault but won't appear in Both.")
        );
    }

    /// The story's own example. The sentence has to name the term, because
    /// "this note won't appear here" with no reason sends someone looking for a
    /// bug in the create path.
    #[test]
    fn a_space_no_creation_can_satisfy_names_the_term_that_defeated_it() {
        let (_, told) = create_into("Recordings", "is:recording");
        assert_eq!(
            told.as_deref(),
            Some("A new note can't satisfy is:recording, so this note is in the vault but won't appear in Recordings.")
        );
    }

    #[test]
    fn several_unsatisfiable_terms_are_listed_as_a_sentence() {
        let (_, told) = create_into("Odd", "is:recording is:conflict is:unread");
        assert_eq!(
            told.as_deref(),
            Some("A new note can't satisfy is:recording, is:conflict and is:unread, so this note is in the vault but won't appear in Odd.")
        );
    }

    /// A satisfiable term beside an unsatisfiable one is still applied: the
    /// note is tagged even though it will not be listed, so moving it into the
    /// space later is one edit rather than a re-tag.
    #[test]
    fn a_satisfiable_term_is_still_applied_beside_one_that_is_not() {
        let (seed, told) = create_into("Tagged recordings", "tag:standup is:recording");
        assert_eq!(seed.tags, ["standup"]);
        assert!(told.is_some_and(
            |sentence| sentence.contains("is:recording") && !sentence.contains("tag:standup")
        ));
    }

    /// A broken space matches nothing, so a note created in it appears nowhere
    /// — and the sentence sends the reader to the query rather than to the note.
    #[test]
    fn a_space_whose_query_cannot_be_read_still_creates_and_says_why() {
        let (seed, told) = create_into("Broken", "tag:work | ");
        assert!(seed.is_empty());
        assert_eq!(
            told.as_deref(),
            Some("Broken's query can't be read, so it selects nothing. This note is in the vault, but it won't appear there.")
        );
    }

    /// Structure has no single term to blame, so the sentence names the space
    /// instead of guessing.
    #[test]
    fn a_query_with_structure_seeds_nothing_and_blames_no_term() {
        let (seed, told) = create_into("Either", "tag:a | tag:b");
        assert!(seed.is_empty());
        assert_eq!(
            told.as_deref(),
            Some("A new note can't satisfy Either's query, so this note is in the vault but won't appear there.")
        );
    }

    /// `date:` needs no seed and must not produce a false refusal: a note
    /// created now was created inside "the last seven days" by construction.
    #[test]
    fn a_recent_date_window_is_satisfied_by_the_act_of_creating() {
        let (seed, told) = create_into("Recent", "tag:work date:created>=-7d");
        assert_eq!(seed.tags, ["work"]);
        assert_eq!(told, None);
    }

    /// The other direction, and the reason `verdict` evaluates rather than
    /// trusting the seed: a window in the past is unreachable, and only running
    /// the query can know that.
    #[test]
    fn a_date_window_in_the_past_is_reported_rather_than_ignored() {
        let (_, told) = create_into("Old", "date:created<=2020-01-01");
        assert_eq!(
            told.as_deref(),
            Some("A new note can't satisfy date:created<=2020-01-01, so this note is in the vault but won't appear in Old.")
        );
    }

    /// `origin:` is not seeded, and does not need to be: a note this device
    /// just wrote has no origin field yet, which the evaluator reads as local.
    #[test]
    fn a_local_origin_space_is_satisfied_without_a_seed() {
        let (seed, told) = create_into("Mine", "origin:local");
        assert!(seed.is_empty());
        assert_eq!(told, None);
    }

    #[test]
    fn an_agent_origin_space_is_refused_because_keeper_is_not_the_agent() {
        let (_, told) = create_into("Agent's", "origin:agent");
        assert!(told.is_some_and(|sentence| sentence.contains("origin:agent")));
    }

    /// The body is read, not assumed: a template that puts the needle in the
    /// note satisfies a `text:` space, and an empty note does not.
    #[test]
    fn a_text_term_is_answered_from_the_body_the_note_will_actually_have() {
        let seed = inherit("text:agenda");
        let entry = projected(&seed, "Note", "", STAMP, NOW_MS);
        assert!(verdict("Agenda", "text:agenda", &entry, "", NOW_MS).is_some());
        assert_eq!(
            verdict("Agenda", "text:agenda", &entry, "## Agenda\n", NOW_MS),
            None
        );
    }

    #[test]
    fn a_capture_space_writes_the_capture_mark() {
        let (seed, told) = create_into("Unfiled", "is:capture");
        assert!(seed.capture);
        assert_eq!(told, None);
    }

    #[test]
    fn an_archived_space_creates_an_archived_note() {
        let (seed, told) = create_into("Archive", "is:archived");
        assert!(seed.archived);
        assert_eq!(told, None);
    }

    /// `spaces/` is the one folder that decides what kind of file lives in it.
    /// Creating a plain note there would manufacture a space with no query — a
    /// broken row in the rail nobody asked for — so the seed declines and the
    /// note is created where a note belongs.
    #[test]
    fn a_space_of_spaces_does_not_manufacture_a_broken_space() {
        let (seed, told) = create_into("Saved views", "is:space");
        assert_eq!(seed.dest, None);
        assert!(told.is_some_and(|sentence| sentence.contains("is:space")));
    }

    /// The counterpart, and the reason `is:space` is refused on principle while
    /// `is:template` is not: a template is a note TAGGED `template` (AD-82,
    /// Story 44.7), so "new note in Templates" is one tag, not a file dropped
    /// into a folder keeper owns. It is satisfiable, so it is satisfied.
    #[test]
    fn a_template_space_creates_a_template() {
        let (seed, told) = create_into("Templates", "is:template");
        assert_eq!(seed.tags, [templates::TEMPLATE_TAG]);
        assert_eq!(seed.dest, None, "never a folder keeper owns");
        assert_eq!(told, None);
    }

    /// The tag is added once however it was asked for. Two producers reach the
    /// seed's tag list now — a `tag:` term and `is:template` — and a duplicate
    /// here is a duplicate in the note's frontmatter.
    #[test]
    fn a_template_space_that_also_names_the_tag_adds_it_once() {
        let (seed, told) = create_into("Templates", "is:template tag:template");
        assert_eq!(seed.tags, [templates::TEMPLATE_TAG]);
        assert_eq!(told, None);
    }

    /// A contradiction answers itself rather than being resolved by precedence,
    /// exactly as it does in the list's chip bar.
    #[test]
    fn a_query_that_contradicts_itself_is_reported_and_not_silently_resolved() {
        let (seed, told) = create_into("Impossible", "tag:work is:untagged");
        assert_eq!(seed.tags, ["work"]);
        assert!(told.is_some_and(|sentence| sentence.contains("is:untagged")));
    }

    // -----------------------------------------------------------------------
    // Story 45.16 — what a quick capture carries, and where it lands
    // -----------------------------------------------------------------------

    #[test]
    fn a_capture_carries_the_mark_and_the_configured_tag() {
        let seed = capture(Some("#Quick Capture"));
        assert!(seed.capture, "a capture is still a capture");
        assert_eq!(
            seed.tags,
            ["quick-capture"],
            "the stored tag is the canonical one, not what was typed"
        );
        assert_eq!(
            seed.dest, None,
            "a capture is filed by tag, never by folder"
        );
    }

    /// The default every existing vault keeps: no capture tag configured, so a
    /// capture is exactly the note it has always been.
    #[test]
    fn a_capture_with_no_tag_configured_is_untagged_and_still_a_capture() {
        let seed = capture(None);
        assert!(seed.capture);
        assert!(seed.tags.is_empty());
    }

    /// Cleared and unusable are one state, at both ends of the setting.
    #[test]
    fn a_configured_value_that_is_not_a_tag_yields_no_tag_at_all() {
        for typed in ["", "   ", "#", "###", "---", "/", "//"] {
            assert_eq!(capture_tag(typed), None, "{typed:?} was accepted");
            let seed = capture(Some(typed));
            assert!(seed.tags.is_empty(), "{typed:?} became {:?}", seed.tags);
            assert!(seed.capture, "{typed:?} stopped being a capture");
        }
    }

    /// AD-82's marker is not available as a capture tag: it would make every
    /// thought the user captures a template of itself, which is the failure
    /// 44.7 strips the marker on copy to prevent.
    #[test]
    fn the_template_marker_is_refused_as_a_capture_tag_however_it_is_spelled() {
        for typed in ["template", "#Template", "  TEMPLATE  "] {
            assert_eq!(capture_tag(typed), None, "{typed:?} was accepted");
            assert!(capture(Some(typed)).tags.is_empty(), "{typed:?}");
        }
        assert!(
            !projected(&capture(Some("template")), "", "", STAMP, NOW_MS)
                .has_flag(templates::TEMPLATE_TAG),
            "a capture must never be indexed as a template"
        );
    }

    /// Somebody's own filing under a word keeper reserves at the root. 44.7
    /// leaves `template/daily` on a copied note for the same reason.
    #[test]
    fn a_tag_merely_filed_under_template_is_a_usable_capture_tag() {
        assert_eq!(
            capture_tag("Template/Inbox").as_deref(),
            Some("template/inbox")
        );
        assert!(
            !projected(&capture(Some("template/inbox")), "", "", STAMP, NOW_MS)
                .has_flag(templates::TEMPLATE_TAG)
        );
    }

    /// **The finding this story had to answer, asked rather than reasoned
    /// about.** 44.7 shipped its templates untagged because Inbox is
    /// `is:untagged`; a capture tag is the same hazard aimed at every captured
    /// thought instead of one template's copies. The answer comes from Inbox's
    /// own stored query, run by the one evaluator.
    #[test]
    fn a_capture_tag_files_every_capture_out_of_the_inbox() {
        let inbox = DEFAULT_SPACES
            .iter()
            .find(|space| space.key == "inbox")
            .expect("a vault has an Inbox");
        assert_eq!(
            inbox.query, "is:untagged",
            "this test is only meaningful while Inbox selects the unfiled"
        );
        assert_eq!(
            capture_verdict(inbox.name, inbox.query, None, STAMP, NOW_MS),
            None,
            "an untagged capture is what the Inbox is for"
        );
        let told = capture_verdict(inbox.name, inbox.query, Some("capture"), STAMP, NOW_MS)
            .expect("a tagged capture is not untagged, and the user has to be told");
        assert!(
            told.contains("is:untagged") && told.contains("Inbox"),
            "the sentence must name the term and the space: {told}"
        );
    }

    /// The other half of the trade: the tag buys a space of its own, and that
    /// space is a `tag:` query rather than a folder (FR-193).
    #[test]
    fn a_space_selecting_the_capture_tag_lists_a_capture_and_only_a_tagged_one() {
        assert_eq!(
            capture_verdict("Captures", "tag:capture", Some("capture"), STAMP, NOW_MS),
            None
        );
        assert!(
            capture_verdict("Captures", "tag:capture", None, STAMP, NOW_MS).is_some(),
            "with no tag configured there is nothing for a tag space to select"
        );
        // A subtree term is satisfied by the tag underneath it, so a user who
        // files captures at `inbox/capture` still gets a `tag:inbox` space.
        assert_eq!(
            capture_verdict("Unfiled", "tag:inbox", Some("inbox/capture"), STAMP, NOW_MS),
            None
        );
    }

    /// Every space a fresh vault has, both ways, in one table — because the
    /// question "what does this setting cost me" has exactly as many answers as
    /// 44.3 seeds and no fewer.
    ///
    /// The length assertion is deliberate: a new default space is a new answer
    /// to "does a captured thought land here", and a silently unconsidered one
    /// is how a shipped surface stops showing captures without anybody deciding
    /// it should.
    #[test]
    fn what_a_capture_tag_does_to_every_space_a_fresh_vault_is_seeded_with() {
        // key, lists an untagged capture, lists a capture tagged `capture`
        let expected = [
            ("inbox", true, false),
            ("journal", false, false),
            ("pinned", false, false),
            ("recordings", false, false),
            // 45.20's Templates space. A capture is never a template, whichever
            // way the setting is left — `capture_tag` refuses the marker, so
            // there is no configuration that files captured thoughts in among
            // the scaffolds.
            ("templates", false, false),
        ];
        assert_eq!(
            expected.len(),
            DEFAULT_SPACES.len(),
            "a default space was added or removed without saying what a capture tag does to it — \
             add its key here with the two answers this test measures"
        );
        for (key, untagged, tagged) in expected {
            let space = DEFAULT_SPACES
                .iter()
                .find(|space| space.key == key)
                .unwrap_or_else(|| panic!("no default named {key}"));
            assert_eq!(
                capture_verdict(space.name, space.query, None, STAMP, NOW_MS).is_none(),
                untagged,
                "{key} ({}) with no capture tag",
                space.query
            );
            assert_eq!(
                capture_verdict(space.name, space.query, Some("capture"), STAMP, NOW_MS).is_none(),
                tagged,
                "{key} ({}) with the capture tag",
                space.query
            );
        }
    }

    /// `is:capture` was already in the vocabulary before this story and stays
    /// the tag-free way to find captures — a vault that wants them out of the
    /// Inbox has the tag, and a vault that wants them in it still has this.
    #[test]
    fn the_reserved_capture_mark_selects_a_capture_with_or_without_a_tag() {
        for tag in [None, Some("capture")] {
            assert_eq!(
                capture_verdict("Unfiled", "is:capture", tag, STAMP, NOW_MS),
                None,
                "{tag:?}"
            );
        }
    }

    /// A space seeded from `tag:<capture tag>` is the space the setting is for,
    /// and creating into it by hand must produce the same note capture does.
    #[test]
    fn creating_by_hand_into_a_capture_space_seeds_the_same_tag_capture_writes() {
        let (seeded, told) = create_into("Captures", "tag:capture");
        assert_eq!(seeded.tags, capture(Some("capture")).tags);
        assert_eq!(told, None);
    }

    /// [`projected`] answers about the note a create would write, so the two
    /// facts a capture cannot know in advance must be absent rather than
    /// invented — an invented body would make a `text:` space claim a capture
    /// will appear in it.
    #[test]
    fn a_capture_verdict_never_pretends_to_know_what_has_not_been_typed() {
        assert!(
            capture_verdict("Agenda", "text:agenda", Some("capture"), STAMP, NOW_MS).is_some(),
            "keeper cannot promise a space that reads the body"
        );
        let entry = projected(&capture(None), "", "", STAMP, NOW_MS);
        assert!(entry.snippet.is_empty());
        assert!(entry.title.is_empty());
    }

    /// The filename comes from the real namer and the stamp's own date, so the
    /// path a `path:` space is asked about is the path a create would pick.
    #[test]
    fn a_projected_note_is_named_the_way_the_create_path_names_one() {
        assert_eq!(
            projected(&Seed::default(), "Standup notes", "", STAMP, NOW_MS).path,
            "2026-08-09-standup-notes.md"
        );
        assert_eq!(
            projected(&capture(None), "", "", STAMP, NOW_MS).path,
            "2026-08-09-untitled.md"
        );
        // A stamp too short to carry a date leaves the name undated rather than
        // panicking on a slice: `note_filename` already treats "" as no date.
        assert_eq!(
            projected(&Seed::default(), "Note", "", "2026", NOW_MS).path,
            "note.md"
        );
    }

    // -----------------------------------------------------------------------
    // Shape audit (see the story spec) — probes from shapes peers were bitten
    // by, after the sweep was already green.
    // -----------------------------------------------------------------------

    /// A5. [`projected`]'s doc comment claims to mirror
    /// `notes_vault::parse_note` "for exactly the facts a Seed can set". It did
    /// not: the parser flags `space` from a `spaces/` prefix and `template`
    /// from a `templates/` prefix, and a seed reaches BOTH through `path:` —
    /// `is:space` declines to seed a destination, but `path:spaces/**` does
    /// not. So a `path:templates/**` space was told its new note would not
    /// appear when it would.
    #[test]
    fn a_projected_note_carries_the_flags_its_folder_gives_it_and_not_only_its_tags() {
        let into_spaces = inherit("path:spaces/**");
        assert_eq!(into_spaces.dest.as_deref(), Some("spaces"), "reachable");
        assert!(projected(&into_spaces, "Note", "", STAMP, NOW_MS).has_flag("space"));
        assert_eq!(
            verdict(
                "Saved views",
                "path:spaces/** is:space",
                &projected(&into_spaces, "Note", "", STAMP, NOW_MS),
                "",
                NOW_MS
            ),
            None
        );

        let into_templates = inherit("path:templates/**");
        assert_eq!(
            into_templates.dest.as_deref(),
            Some("templates"),
            "reachable"
        );
        assert!(
            projected(&into_templates, "Note", "", STAMP, NOW_MS).has_flag(templates::TEMPLATE_TAG),
            "44.7 grandfathers the folder, and the projection has to grandfather it too"
        );
        // A folder that merely BEGINS with the word is not the folder.
        let elsewhere = Seed {
            dest: Some("spaces-archive".to_owned()),
            ..Seed::default()
        };
        assert!(!projected(&elsewhere, "Note", "", STAMP, NOW_MS).has_flag("space"));
    }

    /// A9. The positive witness for every `!has_flag(TEMPLATE_TAG)` assertion
    /// above. Without one, a renamed or broken `has_flag` would make each of
    /// them pass for the wrong reason and nothing in the file could tell.
    #[test]
    fn the_template_flag_is_really_set_when_the_seed_really_asks_for_it() {
        let seed = inherit("is:template");
        assert_eq!(seed.tags, [templates::TEMPLATE_TAG]);
        assert!(projected(&seed, "Note", "", STAMP, NOW_MS).has_flag(templates::TEMPLATE_TAG));
    }

    /// A6. The filter the Settings surface renders, which was a decision living
    /// in an uncompilable shell command until this probe. All four quadrants,
    /// because three of them are silent and a function that returned a sentence
    /// for any of those three would bury the one row that matters.
    #[test]
    fn the_cost_of_a_capture_tag_is_only_what_the_tag_takes_away() {
        // Lists an untagged capture, not a tagged one — the only cost there is.
        let told = capture_tag_cost("Inbox", "is:untagged", Some("capture"), STAMP, NOW_MS)
            .expect("the Inbox stops listing captures and that is the whole point");
        assert!(
            told.contains("is:untagged") && told.contains("Inbox"),
            "{told}"
        );
        // Lists them either way.
        assert_eq!(
            capture_tag_cost("Unfiled", "is:capture", Some("capture"), STAMP, NOW_MS),
            None
        );
        // Never listed them: not a cost of turning the tag on.
        assert_eq!(
            capture_tag_cost("Pinned", "is:pinned", Some("capture"), STAMP, NOW_MS),
            None
        );
        // A GAIN — the space this setting exists to make possible. Silent,
        // because a surface that announced it would be promising a space fills.
        assert_eq!(
            capture_tag_cost("Captures", "tag:capture", Some("capture"), STAMP, NOW_MS),
            None
        );
        // And no tag costs nothing anywhere, which is what makes the control's
        // off state honest rather than merely quiet.
        for space in DEFAULT_SPACES {
            assert_eq!(
                capture_tag_cost(space.name, space.query, None, STAMP, NOW_MS),
                None,
                "{}",
                space.key
            );
        }
    }
}
