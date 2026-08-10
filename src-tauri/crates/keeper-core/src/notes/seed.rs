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

use crate::notes::index::IndexEntry;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::default_spaces::DEFAULT_SPACES;
    use crate::notes::order::NoteOrder;
    use std::collections::BTreeMap;

    /// Ten in the morning on 2026-08-09, UTC, in ms. Every date assertion below
    /// is relative to it, so none of them changes meaning tomorrow.
    const NOW_MS: i64 = 1_786_600_000_000;

    /// The index entry the shell's parser would produce for a note created with
    /// `seed`.
    ///
    /// This mirrors `keeper::notes_vault::parse_note` for exactly the facts a
    /// seed can set, and nothing else — it is a test double for the *shell*, so
    /// that these tests can assert the round trip (a seed derived from a query
    /// really does produce a note that query selects) on a host that cannot
    /// build the Tauri crate (AD-56). Production never calls it: the shell
    /// hands `verdict` an entry from the real parser.
    fn as_created(seed: &Seed, title: &str, body: &str) -> IndexEntry {
        let dir = seed.dest.clone().unwrap_or_default();
        let path = if dir.is_empty() {
            "2026-08-09-note.md".to_owned()
        } else {
            format!("{dir}/2026-08-09-note.md")
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
        // 44.7's rule, mirrored: a template is a note TAGGED `template`, not a
        // note in a folder keeper owns (AD-82). `notes_vault::parse_note` reads
        // it through `templates::is_template`, off the same frontmatter tag
        // list the seed writes.
        if seed.tags.iter().any(|tag| tag == templates::TEMPLATE_TAG) {
            flags.push("template".to_owned());
        }
        let mut fields = BTreeMap::new();
        fields.insert("created".to_owned(), "2026-08-09T10:00:00+00:00".to_owned());
        fields.insert("updated".to_owned(), "2026-08-09T10:00:00+00:00".to_owned());
        let mut tags = seed.tags.clone();
        tags.sort();
        IndexEntry {
            id: "01SEEDNOTE".to_owned(),
            path,
            title: title.to_owned(),
            size: 0,
            mtime_ns: i128::from(NOW_MS) * 1_000_000,
            ino: 1,
            created_ms: NOW_MS,
            updated_ms: NOW_MS,
            tags,
            fields,
            links: Vec::new(),
            flags,
            snippet: body.to_owned(),
            order: NoteOrder::default(),
        }
    }

    /// Create into `query` and report what the user would be told.
    fn create_into(name: &str, query: &str) -> (Seed, Option<String>) {
        let seed = inherit(query);
        let entry = as_created(&seed, "Note", "");
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
        let entry = as_created(&seed, "Note", "");
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
}
