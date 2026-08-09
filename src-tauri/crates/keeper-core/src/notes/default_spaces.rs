//! The four spaces keeper seeds into a vault, and the rule for when it may
//! (Story 44.3, FR-156, AD-79, AD-80).
//!
//! Inbox, Journal, Pinned and Recordings used to be hard-coded rows above the
//! Spaces group — saved filters that nobody could edit, reorder, rename or give
//! an icon. They are spaces now, and the only thing that makes them special is
//! that keeper writes them once, into a vault that has never seen them.
//!
//! Everything in this module is a decision over values, deliberately, because
//! the effect it drives is the worst kind keeper has: **writing notes into
//! somebody's real vault**, on a pendrive, through the sync engine. The two
//! failures that matters are seeding twice (a rail with two Inboxes) and seeding
//! after a deletion (keeper putting back a row the user threw away). Both are
//! decided by [`plan`], which takes what is on disk and what the ledger
//! remembers and returns a list — so both can be proved on a host where the
//! shell crate does not even build (AD-55, AD-56).
//!
//! **The queries are the ones the deleted rows ran, not new ones.** Inbox is
//! `is:untagged` — the honest home of the unfiled is the note no tag has
//! claimed, and `untagged` is what the index computes — Journal is `is:journal`,
//! which the index sets from `journal/` (`notes_vault::note_flags`), Pinned is
//! `is:pinned` and Recordings is `is:recording`. Every one of them is already in
//! [`crate::notes::query`]'s closed `is:` set. Inventing an `is:inbox` alias for
//! `untagged` would have been a second name for one predicate, which is the one
//! thing epic 44 says it adds none of.
//!
//! **Today is not here.** It never filtered anything (AD-80): it opened or
//! created today's journal entry, which is an action on one note and still lives
//! on `⌘⌥J`, the tray and the palette. There is no query it could run that an
//! ordinary space cannot express.

use std::collections::BTreeSet;

use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::naming;

/// Where the seed ledger lives, vault-relative.
///
/// **In the vault, and it syncs.** "keeper has already offered its defaults
/// here" is a fact about this vault, not about this laptop, so it has to travel
/// with the vault — otherwise deleting Pinned on the desktop is undone the next
/// time the laptop opens the same synced folder, which is exactly the forever-
/// ownership AD-79 refuses. That rules out the two cheaper homes: the profile
/// row in `keeper.db` is per-machine, and `.keeper/` is per-machine *and*
/// documented as a deletable cache — a fact that cannot be recomputed must not
/// live somewhere a user is invited to clear.
///
/// A leading dot, and not a `.md` file: Obsidian's explorer hides it, the note
/// walk only ever collects `.md`, and `keeper-sync`'s tier-0 corpus excludes the
/// `.keeper` *directory* and not names merely beginning with it (its own
/// `sub/.keeperrc` case), so this is ordinary synced content.
pub const LEDGER_REL: &str = ".keeper-spaces.json";

/// The sentence written into the ledger, so the file explains itself to whoever
/// finds it in their vault rather than looking like debris keeper left behind.
const LEDGER_NOTE: &str = "keeper has already offered this vault its default \
spaces, and will not add them again on its own. Delete a space you do not want \
and it stays deleted. Use Restore default spaces to get the missing ones back, \
or delete this file to be offered all of them again.";

/// The ledger format this build writes and understands.
const LEDGER_VERSION: u64 = 1;

/// One seeded default: a saved query with a name and a glyph.
///
/// `key` is the identity, and it is the one field the user cannot change. The
/// name, the icon, the query, the sort and the position are all theirs the
/// moment the note exists — which is the whole point of AD-79 — so none of them
/// can be what "this is the Recordings space" means. The key rides in the note's
/// own frontmatter as `keeper.default`, so a renamed Recordings space is still
/// the one the empty state can speak about, and restore still knows it is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultSpace {
    /// The stable identity, written to `keeper.default` and to the ledger.
    pub key: &'static str,
    /// The name keeper gives it. Renaming it changes nothing but the name.
    pub name: &'static str,
    /// The query, in the DSL [`crate::notes::query`] parses.
    pub query: &'static str,
    /// The icon name, from the editor's fixed set.
    pub icon: &'static str,
}

/// The four, in the order the rail used to fix.
///
/// The order is also alphabetical by name, which is what `notes_spaces` sorts
/// by today — so a freshly seeded vault renders the rail the deleted rows
/// rendered, glyph for glyph, before Story 44.4 gives a space an explicit
/// `order`.
pub const DEFAULT_SPACES: [DefaultSpace; 4] = [
    DefaultSpace {
        key: "inbox",
        name: "Inbox",
        query: "is:untagged",
        icon: "inbox",
    },
    DefaultSpace {
        key: "journal",
        name: "Journal",
        query: "is:journal",
        icon: "calendar-days",
    },
    DefaultSpace {
        key: "pinned",
        name: "Pinned",
        query: "is:pinned",
        icon: "pin",
    },
    DefaultSpace {
        key: "recordings",
        name: "Recordings",
        query: "is:recording",
        icon: "video",
    },
];

/// The default carrying `key`, if any. The reverse of [`DefaultSpace::key`], for
/// a marker read back off disk.
pub fn by_key(key: &str) -> Option<&'static DefaultSpace> {
    DEFAULT_SPACES.iter().find(|space| space.key == key)
}

/// A space the vault already has, as the seeder needs to see it.
///
/// Two fields because there are two ways a default can already be present: it
/// is one keeper wrote (`default_key`), or it is one the *user* wrote and gave
/// the same name to (`name`). The second is not hypothetical — a person who
/// wanted an Inbox before keeper shipped one built it themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingSpace {
    /// `keeper.default` from the note's frontmatter, when it carries one.
    pub default_key: Option<String>,
    /// The space's displayed name.
    pub name: String,
}

/// Why keeper is writing defaults right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedMode {
    /// Automatic, on a vault keeper has not seeded before. Obeys the ledger.
    FirstRun,
    /// The user pressed "Restore default spaces". Ignores the ledger, because
    /// the ledger's entire job is to stop keeper acting on its own, and this is
    /// the user acting.
    Restore,
}

/// Which defaults to write, in [`DEFAULT_SPACES`] order.
///
/// `offered` is the ledger: the keys this vault has already been given.
/// `None` means keeper could not read it — a file that is there and does not
/// parse. That is deliberately NOT the same as an absent file. An absent ledger
/// is a vault that has never been seeded and gets the four; an unreadable one is
/// a vault keeper knows nothing about, and the safe direction there is to write
/// nothing, because the cost of not offering a space is a menu item away and the
/// cost of resurrecting four the user deleted is keeper editing their vault
/// behind their back.
///
/// A default is *present* when a space carries its key, or when a space is
/// already called what it would be called. The name comparison is
/// [`naming::slug`]'s fold, so `Inbox`, `inbox` and `  INBOX  ` are one name —
/// the same folding that decides two notes cannot share a filename, and the
/// reason two rows both saying "Inbox" never appear in the rail.
pub fn plan(
    mode: SeedMode,
    existing: &[ExistingSpace],
    offered: Option<&BTreeSet<String>>,
) -> Vec<&'static DefaultSpace> {
    let ledger = match (mode, offered) {
        (SeedMode::Restore, _) => None,
        (SeedMode::FirstRun, Some(keys)) => Some(keys),
        // Unreadable ledger, automatic run: keeper stays out.
        (SeedMode::FirstRun, None) => return Vec::new(),
    };
    let taken_keys: BTreeSet<&str> = existing
        .iter()
        .filter_map(|space| space.default_key.as_deref())
        .collect();
    let taken_names: BTreeSet<String> = existing
        .iter()
        .map(|space| naming::slug(&space.name))
        .collect();
    DEFAULT_SPACES
        .iter()
        .filter(|space| !ledger.is_some_and(|keys| keys.contains(space.key)))
        .filter(|space| !taken_keys.contains(space.key))
        .filter(|space| !taken_names.contains(&naming::slug(space.name)))
        .collect()
}

/// The note keeper writes for one default.
///
/// Byte for byte the shape [`notes_space_save`](../../../keeper/notes_ipc)
/// writes for a hand-made space — same key order, same `# <name>` body — plus
/// the one key that makes it a default. A seeded space that differed from a
/// saved one would be a second kind of space note, and the editor would be the
/// place it went wrong.
///
/// `id` and `now` are parameters rather than reads so this is a function of its
/// inputs: the shell mints the ULID it mints everywhere else, and a test gets
/// the same bytes on every machine.
pub fn render_note(space: &DefaultSpace, id: &str, now: &str) -> String {
    let front = Frontmatter::serialise_new(&[
        ("id".to_owned(), FieldValue::Str(id.to_owned())),
        ("created".to_owned(), FieldValue::Str(now.to_owned())),
        ("updated".to_owned(), FieldValue::Str(now.to_owned())),
        (
            "keeper".to_owned(),
            FieldValue::Map(vec![
                ("space".to_owned(), FieldValue::Str(space.query.to_owned())),
                ("sort".to_owned(), FieldValue::Str(DEFAULT_SORT.to_owned())),
                ("icon".to_owned(), FieldValue::Str(space.icon.to_owned())),
                ("default".to_owned(), FieldValue::Str(space.key.to_owned())),
            ]),
        ),
    ]);
    format!("{front}\n# {}\n", space.name)
}

/// The sort a seeded space carries, matching what `space_def` falls back to for
/// a space that names none — so the four are not quietly a different lens from
/// every other space before Story 44.4 makes sort a real choice.
const DEFAULT_SORT: &str = "modified desc";

/// The `keeper.default` marker inside an already-read `keeper:` map.
///
/// Trimmed and matched against [`DEFAULT_SPACES`], so a hand-written
/// `default: whatever` is not a key: an unrecognised marker names no default,
/// which means restore will happily add the real one beside it rather than
/// treating a stranger as one of keeper's.
pub fn default_key(pairs: &[(String, FieldValue)]) -> Option<String> {
    pairs
        .iter()
        .find_map(|(key, value)| match (key.as_str(), value) {
            ("default", FieldValue::Str(raw)) => {
                by_key(raw.trim()).map(|space| space.key.to_owned())
            }
            _ => None,
        })
}

/// The same marker, read straight off a note's source.
///
/// The seeder's entry point, and the reason it exists separately: seeding runs
/// on a vault whose index has not been built yet, so it reads `spaces/` off the
/// disk rather than asking a snapshot that is empty. One rule, two ways in.
pub fn default_key_of(source: &str) -> Option<String> {
    let (fm, _) = Frontmatter::parse(source);
    match fm.get("keeper") {
        Some(FieldValue::Map(pairs)) => default_key(pairs),
        _ => None,
    }
}

/// The ledger's keys, or `None` when the text is not a ledger keeper wrote.
///
/// Unknown keys survive a round trip only in the sense that they are dropped
/// and rewritten from [`DEFAULT_SPACES`]; a key the ledger names and this build
/// does not know is kept, because a vault opened by a newer keeper and then by
/// an older one must not have the newer build's defaults re-offered.
pub fn parse_ledger(text: &str) -> Option<BTreeSet<String>> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let seeded = value.get("seeded")?.as_array()?;
    seeded
        .iter()
        .map(|entry| entry.as_str().map(str::to_owned))
        .collect()
}

/// The ledger file's text.
pub fn render_ledger(keys: &BTreeSet<String>) -> String {
    let value = serde_json::json!({
        "version": LEDGER_VERSION,
        "note": LEDGER_NOTE,
        "seeded": keys.iter().collect::<Vec<_>>(),
    });
    // Pretty, with a trailing newline: this lands in a folder a person browses
    // and a line-based sync diffs.
    format!(
        "{}\n",
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_owned())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::query;

    fn existing(name: &str, default_key: Option<&str>) -> ExistingSpace {
        ExistingSpace {
            default_key: default_key.map(str::to_owned),
            name: name.to_owned(),
        }
    }

    fn keys(of: &[&'static DefaultSpace]) -> Vec<&'static str> {
        of.iter().map(|space| space.key).collect()
    }

    fn ledger(of: &[&str]) -> BTreeSet<String> {
        of.iter().map(|key| (*key).to_owned()).collect()
    }

    /// The whole reason the four could become spaces: every query they run is
    /// already in the closed `is:` set. If one of them were not, seeding would
    /// write a space that refuses to parse into a fresh vault — a rail of four
    /// rows, all broken, on first run.
    #[test]
    fn every_default_query_parses_against_the_closed_flag_set() {
        for space in &DEFAULT_SPACES {
            assert!(
                query::parse(space.query).is_ok(),
                "{} stores an unparseable query: {}",
                space.key,
                space.query
            );
        }
    }

    /// The queries are the rows', not new ones. Pinned all over again as
    /// `tag:pinned` would list a different set of notes from the row it
    /// replaced, and nobody would notice until the vault had a `pinned` tag.
    #[test]
    fn the_defaults_run_the_queries_the_deleted_rows_ran() {
        let queries: Vec<(&str, &str)> = DEFAULT_SPACES
            .iter()
            .map(|space| (space.key, space.query))
            .collect();
        assert_eq!(
            queries,
            vec![
                ("inbox", "is:untagged"),
                ("journal", "is:journal"),
                ("pinned", "is:pinned"),
                ("recordings", "is:recording"),
            ]
        );
    }

    #[test]
    fn a_fresh_vault_is_offered_all_four() {
        let plan = plan(SeedMode::FirstRun, &[], Some(&BTreeSet::new()));
        assert_eq!(
            keys(&plan),
            vec!["inbox", "journal", "pinned", "recordings"]
        );
    }

    /// The story's own acceptance: delete one, reopen, and it does not come
    /// back. The ledger remembers the offer, not the note.
    #[test]
    fn a_default_the_ledger_already_offered_is_never_written_again() {
        let all = ledger(&["inbox", "journal", "pinned", "recordings"]);
        // Every one deleted off disk, every one already offered.
        assert!(plan(SeedMode::FirstRun, &[], Some(&all)).is_empty());
        // And the ordinary case: three still there, one thrown away.
        let kept = [
            existing("Inbox", Some("inbox")),
            existing("Journal", Some("journal")),
            existing("Recordings", Some("recordings")),
        ];
        assert!(plan(SeedMode::FirstRun, &kept, Some(&all)).is_empty());
    }

    /// The drive was unplugged after two files landed. Reopening must finish the
    /// job rather than write the two that exist a second time.
    #[test]
    fn a_half_written_seed_converges_instead_of_doubling_up() {
        let half = [
            existing("Inbox", Some("inbox")),
            existing("Journal", Some("journal")),
        ];
        // The ledger was written last, so it never landed: nothing recorded.
        let plan = plan(SeedMode::FirstRun, &half, Some(&BTreeSet::new()));
        assert_eq!(keys(&plan), vec!["pinned", "recordings"]);
    }

    /// Restore is the user asking, so the ledger does not veto it — but it still
    /// only fills holes.
    #[test]
    fn restore_writes_the_missing_and_leaves_the_present_alone() {
        let all = ledger(&["inbox", "journal", "pinned", "recordings"]);
        let present = [
            existing("Inbox", Some("inbox")),
            existing("Recordings", Some("recordings")),
        ];
        let plan = plan(SeedMode::Restore, &present, Some(&all));
        assert_eq!(keys(&plan), vec!["journal", "pinned"]);

        // Nothing missing, nothing written — pressing it twice is a no-op.
        let full: Vec<ExistingSpace> = DEFAULT_SPACES
            .iter()
            .map(|space| existing(space.name, Some(space.key)))
            .collect();
        assert!(plan_is_empty(SeedMode::Restore, &full, &all));
    }

    fn plan_is_empty(mode: SeedMode, existing: &[ExistingSpace], led: &BTreeSet<String>) -> bool {
        plan(mode, existing, Some(led)).is_empty()
    }

    /// The point of the marker, and the reason the name check alone is not
    /// enough. A default is editable like any other space (AD-79), so someone
    /// renames Inbox to "Unfiled". It is still the Inbox default: neither the
    /// automatic run nor restore may write a second one beside it, and only the
    /// key can say so — the name no longer can.
    #[test]
    fn a_default_that_was_renamed_is_still_that_default() {
        let renamed = [
            existing("Unfiled", Some("inbox")),
            existing("Sessions", Some("recordings")),
        ];
        assert_eq!(
            keys(&plan(SeedMode::FirstRun, &renamed, Some(&BTreeSet::new()))),
            vec!["journal", "pinned"]
        );
        assert_eq!(
            keys(&plan(
                SeedMode::Restore,
                &renamed,
                Some(&ledger(&["inbox", "journal", "pinned", "recordings"]))
            )),
            vec!["journal", "pinned"]
        );
    }

    /// The other half of the same coin: a space that carries no marker and is
    /// not named after a default is somebody's own, however much it looks like
    /// one, and it stands nothing down.
    #[test]
    fn a_hand_built_lookalike_with_no_marker_stands_nothing_down() {
        let mine = [existing("My unfiled things", None)];
        assert_eq!(
            keys(&plan(SeedMode::FirstRun, &mine, Some(&BTreeSet::new()))),
            vec!["inbox", "journal", "pinned", "recordings"]
        );
    }

    /// An existing vault migrates: the user's own spaces are not touched, and
    /// they do not stop the defaults arriving beside them.
    #[test]
    fn a_users_own_spaces_neither_block_the_defaults_nor_are_counted_as_them() {
        let mine = [
            existing("Active work", None),
            existing("Archive triage", None),
        ];
        let plan = plan(SeedMode::FirstRun, &mine, Some(&BTreeSet::new()));
        assert_eq!(
            keys(&plan),
            vec!["inbox", "journal", "pinned", "recordings"]
        );
    }

    /// The case the story asks to be stated: a space the user built and called
    /// Inbox. keeper does not write a second row with the same name on it, and
    /// it never edits theirs — it simply stands down for that one key.
    #[test]
    fn a_user_space_that_already_has_a_defaults_name_stands_the_default_down() {
        for spelling in ["Inbox", "inbox", "  INBOX  ", "Ínbóx"] {
            let mine = [existing(spelling, None)];
            let plan = plan(SeedMode::FirstRun, &mine, Some(&BTreeSet::new()));
            assert!(
                !keys(&plan).contains(&"inbox"),
                "{spelling} folds to the Inbox name and must stand it down"
            );
            assert_eq!(keys(&plan), vec!["journal", "pinned", "recordings"]);
        }
        // A name that folds to something else is a different space and blocks
        // nothing — the fold is the filename rule, so `In box` is `in-box` and
        // is not Inbox.
        for other in ["Unfiled", "In box", "Inboxes"] {
            let mine = [existing(other, None)];
            assert_eq!(
                keys(&plan(SeedMode::FirstRun, &mine, Some(&BTreeSet::new()))),
                vec!["inbox", "journal", "pinned", "recordings"],
                "{other} is not Inbox"
            );
        }
    }

    /// A ledger keeper cannot read is not "this vault was never seeded". Reading
    /// it that way would put four notes back into a vault whose owner may have
    /// deleted all four, which is the one outcome worth being timid about.
    #[test]
    fn an_unreadable_ledger_stops_the_automatic_seed_and_not_the_manual_one() {
        assert!(plan(SeedMode::FirstRun, &[], None).is_empty());
        assert_eq!(
            keys(&plan(SeedMode::Restore, &[], None)),
            vec!["inbox", "journal", "pinned", "recordings"]
        );
    }

    /// A newer build's default, recorded by that build, is not re-offered by
    /// this one — the ledger carries keys it does not recognise.
    #[test]
    fn a_ledger_key_this_build_does_not_know_survives_a_read() {
        let text = render_ledger(&ledger(&["inbox", "someday"]));
        let read = parse_ledger(&text).expect("keeper's own ledger reads back");
        assert!(read.contains("someday"));
        assert_eq!(
            keys(&plan(SeedMode::FirstRun, &[], Some(&read))),
            vec!["journal", "pinned", "recordings"]
        );
    }

    #[test]
    fn a_ledger_that_is_not_one_reads_as_unknown_rather_than_as_empty() {
        for text in [
            "",
            "not json",
            "{}",
            "{\"seeded\": \"inbox\"}",
            "{\"seeded\": [1, 2]}",
            "[]",
        ] {
            assert!(
                parse_ledger(text).is_none(),
                "{text:?} must not read as a ledger"
            );
        }
        assert_eq!(
            parse_ledger("{\"seeded\": []}"),
            Some(BTreeSet::new()),
            "a ledger that recorded nothing is still a ledger"
        );
    }

    #[test]
    fn the_ledger_round_trips_and_says_what_it_is() {
        let written = ledger(&["inbox", "pinned"]);
        let text = render_ledger(&written);
        assert!(
            text.contains("Restore default spaces"),
            "the file has to explain itself: {text}"
        );
        assert!(text.ends_with('\n'));
        assert_eq!(parse_ledger(&text), Some(written));
    }

    /// The marker is what survives a rename, so it is what identity means.
    #[test]
    fn a_seeded_note_carries_its_key_and_reads_it_back() {
        for space in &DEFAULT_SPACES {
            let note = render_note(
                space,
                "01J8ZQ4M7T5R9V3XK2B6C0DFGH",
                "2026-08-09T10:00:00+02:00",
            );
            assert_eq!(
                default_key_of(&note).as_deref(),
                Some(space.key),
                "{} lost its marker: {note}",
                space.key
            );
        }
    }

    /// A default the note names but keeper does not have is not a default. It
    /// must not silently stand a real one down.
    #[test]
    fn an_unrecognised_default_marker_names_no_default() {
        let note = concat!(
            "---\n",
            "id: x\n",
            "keeper:\n",
            "  space: 'tag:a'\n",
            "  default: someday\n",
            "---\n",
            "\n# Someday\n"
        );
        assert!(default_key_of(note).is_none());

        // A note with no `keeper:` block at all, and one with no marker.
        assert!(default_key_of("---\nid: x\n---\n\n# Plain\n").is_none());
        assert!(default_key_of("# No frontmatter at all\n").is_none());
    }

    /// A seeded note is an ordinary space note. If this drifts, the editor
    /// opens a default and cannot read its query.
    #[test]
    fn a_seeded_note_is_the_same_shape_a_saved_space_is() {
        let note = render_note(
            &DEFAULT_SPACES[3],
            "01J8ZQ4M7T5R9V3XK2B6C0DFGH",
            "2026-08-09T10:00:00+02:00",
        );
        assert_eq!(
            note,
            concat!(
                "---\n",
                "id: 01J8ZQ4M7T5R9V3XK2B6C0DFGH\n",
                "created: 2026-08-09T10:00:00+02:00\n",
                "updated: 2026-08-09T10:00:00+02:00\n",
                "keeper:\n",
                "  space: is:recording\n",
                "  sort: modified desc\n",
                "  icon: video\n",
                "  default: recordings\n",
                "---\n",
                "\n",
                "# Recordings\n"
            )
        );
        // And the body's first line is the title, so `note_title` reads
        // "Recordings" rather than the filename stem.
        assert_eq!(
            naming::title_from_body(note.split("---\n").nth(2).expect("a body")),
            "Recordings"
        );
    }
}
