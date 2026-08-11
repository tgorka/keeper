//! The spaces keeper seeds into a vault, the names it claims doing so, and the
//! rule for when it may
//! (Story 44.3, Story 45.20, Story 47.4, FR-156, FR-198, AD-79, AD-80).
//!
//! Inbox, Journal, Pinned and Recordings used to be hard-coded rows above the
//! Spaces group — saved filters that nobody could edit, reorder, rename or give
//! an icon. They are spaces now, and the only thing that makes them special is
//! that keeper writes them once, into a vault that has never seen them.
//! Templates joined them in 45.20 and is the one that was never a row: 44.7 made
//! a template a note with a tag, and left the set of them with nowhere to stand.
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
//! **The ledger is a list of NAMES CLAIMED, not of notes written** (Story 47.4,
//! DW-191). A default keeper stood down for is recorded exactly like one it
//! created, because otherwise the protection is backwards: keeper's own spaces
//! stay deleted and the user's do not. See [`seed`] for what that costs and
//! [`claimed`] for the rule.
//!
//! **The queries are the ones the deleted rows ran, not new ones.** Inbox is
//! `is:untagged` — the honest home of the unfiled is the note no tag has
//! claimed, and `untagged` is what the index computes — Journal is `is:journal`,
//! which the index sets from `journal/` (`notes_vault::note_flags`), Pinned is
//! `is:pinned`, Recordings is `is:recording` and Templates is `is:template`.
//! Every one of them is already in [`crate::notes::query`]'s closed `is:` set.
//! Inventing an `is:inbox` alias for `untagged` would have been a second name
//! for one predicate, which is the one thing epic 44 says it adds none of — and
//! the same rule is why Templates reuses the predicate 44.7 already widened
//! rather than spelling itself `tag:template`.
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
/// row in `keeper.db` is per-machine, and `.keeper/` was, when this was
/// written, per-machine *and* documented as a deletable cache — a fact that
/// cannot be recomputed must not live somewhere a user is invited to clear.
///
/// AD-100 has since carved `.keeper/*.toml` out of the exclusion, so *part* of
/// that directory now syncs and survives a rebuild. This file stays where it
/// is: the carve-out is deliberately narrow — `*.toml` and nothing else — and
/// a JSON ledger under `.keeper/` would still be excluded, still be swept by
/// anyone who takes the old "delete the directory" advice, and still be the
/// only unrecomputable thing in a directory whose other contents are a cache
/// and a trash. The next reader who finds this comment and AD-100 together has
/// their answer here rather than in a diff.
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

/// The five, in the order the rail used to fix.
///
/// The order is also alphabetical by name, which is what `notes_spaces` sorts
/// by today — so a freshly seeded vault renders the rail the deleted rows
/// rendered, glyph for glyph, before Story 44.4 gives a space an explicit
/// `order`.
///
/// **Templates is the fifth, and it is not one of the deleted rows** (Story
/// 45.20). 44.7 made a template an ordinary note carrying an ordinary tag
/// (AD-82), which bought templates the tag tree, search and sync for free — and
/// cost them the one thing a folder gave them, a place to stand. `notes_templates`
/// can list them and the picker can offer them, and until now nothing in the
/// rail could show you the set. It reuses [`crate::notes::templates::TEMPLATE_TAG`]
/// through the `is:template` predicate rather than inventing anything: creating
/// a note in it makes a template, because `seed::seed_flag` already answers
/// `is:template` with that tag.
///
/// **`is:template` and not `tag:template`, deliberately, and the difference is
/// grandfathering.** 44.7 changed the predicate to
/// `templates::is_template(&fm) || rel.starts_with("templates/")` — the tag OR
/// the legacy folder — so that vaults seeded by builds before 44.7 keep seeing
/// their own templates. `tag:template` is strictly narrower and would omit every
/// grandfathered one, leaving this space showing fewer templates than the
/// template picker lists: two surfaces disagreeing about what a template is.
/// Whoever eventually retires the `templates/` clause changes what this space
/// selects, and should expect to.
pub const DEFAULT_SPACES: [DefaultSpace; 5] = [
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
    DefaultSpace {
        key: "templates",
        name: "Templates",
        query: "is:template",
        icon: "layout-template",
    },
];

/// The default carrying `key`, if any. The reverse of [`DefaultSpace::key`], for
/// a marker read back off disk.
pub fn by_key(key: &str) -> Option<&'static DefaultSpace> {
    DEFAULT_SPACES.iter().find(|space| space.key == key)
}

/// A space the vault already has, as the seeder needs to see it.
///
/// `default_key` and `name` are the two ways a default can already be present:
/// it is one keeper wrote, or it is one the *user* wrote and gave the same name
/// to. The second is not hypothetical — a person who wanted an Inbox before
/// keeper shipped one built it themselves.
///
/// `filename` is not part of that decision. It is here because the collision
/// counter needs the names that are taken, and taking them from the same
/// listing that answered the presence question is what stops a seed writing
/// over a space it just decided not to touch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingSpace {
    /// `keeper.default` from the note's frontmatter, when it carries one.
    pub default_key: Option<String>,
    /// The space's displayed name.
    pub name: String,
    /// The note's own file name inside `spaces/`, e.g. `2026-08-08-inbox.md`.
    pub filename: String,
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
/// `offered` is the ledger: **the names keeper has claimed in this vault**.
/// `None` means keeper could not read it — a file that is there and does not
/// parse. That is deliberately NOT the same as an absent file. An absent ledger
/// is a vault that has never been seeded and gets the four; an unreadable one is
/// a vault keeper knows nothing about, and the safe direction there is to write
/// nothing, because the cost of not offering a space is a menu item away and the
/// cost of resurrecting four the user deleted is keeper editing their vault
/// behind their back.
///
/// A default is skipped when the ledger names it, or when it is [`claimed`] —
/// present in the vault under its key or its name.
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
    let present = claimed(existing);
    DEFAULT_SPACES
        .iter()
        .filter(|space| !ledger.is_some_and(|keys| keys.contains(space.key)))
        .filter(|space| !present.contains(space.key))
        .collect()
}

/// The default keys this vault has taken, whoever took them (Story 47.4,
/// DW-191).
///
/// The two ways a default can be present, and the reason this is one function
/// rather than a filter inside [`plan`]: [`seed`] records exactly this set, so
/// "which defaults did keeper stand down for" and "which defaults will keeper
/// skip" have to be one answer. Two spellings of the presence rule would drift,
/// and the symptom is a name recorded as claimed that the planner still writes,
/// or the reverse.
///
/// - **By key.** `keeper.default` in the note's frontmatter, which survives a
///   rename: an Inbox someone renamed to "Unfiled" is still the inbox default.
/// - **By name.** [`naming::slug`]'s fold, so `Inbox`, `inbox` and `  INBOX  `
///   are one name — the same folding that decides two notes cannot share a
///   filename, and the reason two rows both saying "Inbox" never appear in the
///   rail. This is the case that is not hypothetical: a person who wanted an
///   Inbox before keeper shipped one built it themselves.
///
/// Only keys in [`DEFAULT_SPACES`] can come out, so a note carrying a
/// `keeper.default` this build does not know contributes nothing.
pub fn claimed(existing: &[ExistingSpace]) -> BTreeSet<String> {
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
        .filter(|space| {
            taken_keys.contains(space.key) || taken_names.contains(&naming::slug(space.name))
        })
        .map(|space| space.key.to_owned())
        .collect()
}

/// The vault directory, as the seeder needs it.
///
/// **This port exists because Story 44.3 shipped green and did nothing.** Every
/// test the story wrote drove [`plan`] with hand-placed inputs, and the whole
/// risk lived one layer out: reading a ledger off a pendrive, listing a
/// directory that might be asleep, and deciding what an `io::Error` means. Those
/// three reads are now behind four method signatures, so the run that decides
/// whether to write into somebody's vault can be driven against a real
/// directory — with real permission bits — in a crate that builds on every host.
///
/// What is left in the shell is the four bodies, and each is one `std::fs` call
/// or one existing `notes_vault` function.
pub trait SeedVault {
    /// Read a vault-relative file.
    ///
    /// The `io::Error` is handed back whole rather than folded into an
    /// `Option`, because [`seed`] treats `NotFound` and everything else as
    /// opposite answers and a caller that flattened them would put the bug back.
    fn read(&self, rel: &str) -> std::io::Result<String>;
    /// The file names directly inside a vault-relative directory, same contract.
    fn list(&self, rel_dir: &str) -> std::io::Result<Vec<String>>;
    /// Write a vault-relative file, creating parents.
    fn write(&mut self, rel: &str, text: &str) -> std::io::Result<()>;
    /// A fresh note id, and the two stamps a new note carries.
    fn new_id(&mut self) -> String;
    fn now_local(&self) -> String;
    fn today(&self) -> String;
}

/// What one seed run did, and — when it did nothing — why.
///
/// There is no silent arm. The original shipped with `Ok(Vec<String>)` and an
/// empty vector meaning both "already satisfied" and "could not tell, so I
/// declined", which is how a feature can be green on two hosts and invisible in
/// the log of the machine it failed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedOutcome {
    /// Wrote these vault-relative paths, in [`DEFAULT_SPACES`] order.
    Wrote(Vec<String>),
    /// The vault already has every default this run would have offered.
    ///
    /// It writes no space note. It MAY still have recorded a claim, on the one
    /// run that upgrades a pre-DW-191 ledger to the names keeper stood down for
    /// (Story 47.4) — which is why the sentence below says "wrote no spaces"
    /// rather than "wrote nothing". Not a fifth variant: the shell matches this
    /// enum exhaustively and a claim is bookkeeping about a decision already
    /// made, not an outcome of the run. Like every other ledger write here it is
    /// best effort ([`keys_recorded`]) and reports nothing of its own.
    AlreadySatisfied,
    /// Wrote nothing, deliberately, because something could not be read. The
    /// string is the sentence for the log. Nothing is recorded, so the next
    /// registration tries again.
    Blocked(String),
    /// Wrote some and then stopped. What landed is recorded.
    Stopped {
        written: Vec<String>,
        reason: String,
    },
}

/// The lowest level the desktop app's own subscriber will print.
///
/// `debug_log::init` installs `EnvFilter::try_from_default_env()` falling back
/// to `EnvFilter::new("info")`, and nothing sets `RUST_LOG` for the macOS app —
/// a GUI process launched from Finder inherits none. So **`tracing::debug!` is
/// dead code in production**, on stderr and in `keeper.log` alike.
///
/// This constant exists because the second attempt at this story replaced a
/// silent code path with a log line at a level the app cannot emit, which is the
/// same defect one layer out: the run said `AlreadySatisfied` at `debug!` and
/// the field report was still a blank log. A number here plus the test below is
/// what stops the third attempt doing it again.
pub const REPORT_FLOOR: tracing::Level = tracing::Level::INFO;

impl SeedOutcome {
    /// The level and the sentence this outcome deserves in the log.
    ///
    /// The choice lives here rather than at the `tracing::` call site so it can
    /// be asserted: every variant reports at [`REPORT_FLOOR`] or above, so no
    /// outcome of this run can ever be invisible on the machine it ran on.
    ///
    /// `AlreadySatisfied` is `INFO` rather than `DEBUG` on purpose. It is the
    /// ordinary case and it is chatty — one line per vault per refresh — and it
    /// is also the single line that answers "did the seed run at all", which is
    /// the question two field reports in a row have turned on. A handful of
    /// lines per launch is a trade already made in favour of being able to read
    /// the log.
    pub fn report(&self) -> (tracing::Level, String) {
        match self {
            Self::Wrote(written) if written.is_empty() => (
                REPORT_FLOOR,
                "seeded no default spaces; the plan was empty".to_owned(),
            ),
            Self::Wrote(written) => (
                REPORT_FLOOR,
                format!(
                    "seeded {} default spaces: {}",
                    written.len(),
                    written.join(", ")
                ),
            ),
            Self::AlreadySatisfied => (
                REPORT_FLOOR,
                "default spaces already settled for this vault; wrote no spaces".to_owned(),
            ),
            Self::Blocked(why) => (
                tracing::Level::WARN,
                format!("did not seed the default spaces; will try again next refresh. {why}"),
            ),
            Self::Stopped { written, reason } => (
                tracing::Level::WARN,
                format!(
                    "stopped after seeding {} default spaces; recorded what landed. {reason}",
                    written.len()
                ),
            ),
        }
    }
}

/// Run the seed against a vault.
///
/// The order is forced: read the ledger, read `spaces/`, plan, write, record.
/// Reading `spaces/` from the **directory** rather than the index is what makes
/// this correct on a vault registered a millisecond ago, and what makes a
/// half-written seed converge instead of doubling — see [`plan`].
///
/// **A read that fails is not an empty answer.** Both reads distinguish "absent"
/// from "could not tell":
///
/// - No ledger means never seeded; an unreadable one means keeper does not know
///   what this vault has been offered, and writing four notes on that basis is
///   the AD-79 failure.
/// - No `spaces/` means no spaces; an unlistable one means keeper cannot see
///   what is there, and writing four notes on that basis puts a second Inbox in
///   a vault that already had one.
///
/// The first version of this got the second case wrong in the other direction —
/// it swallowed the listing error and read it as "no spaces" — which on a
/// sleeping USB volume is a duplicate rail. Both now decline and say so.
///
/// **The ledger records the names keeper CLAIMED, not the notes it wrote**
/// (Story 47.4, DW-191). A default keeper stood down for — because the vault
/// already had a space of that name — is recorded exactly like one it created.
/// Recording only what it wrote left the user's own space unprotected by the
/// mechanism that protects keeper's: delete an Inbox you made yourself, and the
/// next run saw a name absent from the vault AND absent from the ledger and put
/// keeper's Inbox in its place. You deleted your space and a different one came
/// back, which is the surprise 44.7 refused for templates and AD-79 refuses
/// here.
///
/// It costs the case where a name is freed deliberately: rename your own
/// "Journal" to "Diary" and keeper will not offer its Journal, because it
/// already stood down for that name once. That is the asymmetry this module
/// already chose — not offering a space is one menu item away (Restore ignores
/// the ledger, which is its entire job), and writing into somebody's vault
/// uninvited is not undoable at all.
pub fn seed(vault: &mut dyn SeedVault, mode: SeedMode) -> SeedOutcome {
    let offered = match read_ledger(vault) {
        Ok(offered) => offered,
        Err(reason) => {
            // Restore is the user asking, and they are looking at the rail: an
            // unreadable ledger must not stop them repairing it.
            if mode == SeedMode::FirstRun {
                return SeedOutcome::Blocked(reason);
            }
            None
        }
    };
    let existing = match read_existing(vault) {
        Ok(existing) => existing,
        Err(reason) => return SeedOutcome::Blocked(reason),
    };

    // Whether the ledger was READABLE, kept before `offered` is consumed. On an
    // automatic run an unreadable one has already returned `Blocked`; on a
    // Restore it means keeper is looking at a file it could not parse, and it
    // may not invent a ledger over one — a newer build's, whose defaults this
    // one would then re-offer.
    let readable = offered.is_some();
    let planned = plan(mode, &existing, offered.as_ref());

    // The names this run claims: what the ledger already held, plus every
    // default that is present — whether keeper wrote it or the user did.
    let mut keys: BTreeSet<String> = offered.unwrap_or_default();
    let known = keys.len();
    keys.extend(claimed(&existing));
    // `extend` only adds, so a changed length is a new claim.
    let newly_claimed = keys.len() != known;

    if planned.is_empty() {
        // The upgrade write, and the only run that touches the ledger without
        // writing a note: a vault whose ledger predates DW-191 holds the keys
        // keeper WROTE, and the names it stood down for are missing from it.
        // Gated on an actual change so a settled vault does not rewrite this
        // file on every refresh — it is synced content, and a rewrite per launch
        // is a commit per launch.
        if newly_claimed && readable {
            keys_recorded(vault, &keys);
        }
        return SeedOutcome::AlreadySatisfied;
    }

    // One listing for the whole run, grown as each name is taken, so two
    // defaults written in the same pass cannot be handed one filename.
    let mut taken: Vec<String> = existing
        .iter()
        .map(|space| space.filename.clone())
        .collect();
    let mut written = Vec::new();
    for space in planned {
        let filename = naming::note_filename(space.name, &vault.today(), &taken);
        let rel = format!("{SPACES_DIR}/{filename}");
        let id = vault.new_id();
        let note = render_note(space, &id, &vault.now_local());
        if let Err(error) = vault.write(&rel, &note) {
            // Record what did land before giving up: a full disk halfway
            // through must not be retried as "write all four again".
            keys_recorded(vault, &keys);
            return SeedOutcome::Stopped {
                written,
                reason: format!("{rel}: {error}"),
            };
        }
        taken.push(filename);
        keys.insert(space.key.to_owned());
        written.push(rel);
    }
    keys_recorded(vault, &keys);
    SeedOutcome::Wrote(written)
}

/// Write the ledger, best effort.
///
/// A failure here is not a failure of the run: the notes are on disk, and the
/// worst case is that the next launch re-offers a default the on-disk check will
/// refuse anyway.
fn keys_recorded(vault: &mut dyn SeedVault, keys: &BTreeSet<String>) {
    let text = render_ledger(keys);
    let _ = vault.write(LEDGER_REL, &text);
}

/// What deleting a space left in the ledger, and — when it left nothing — why.
///
/// Same shape and same rule as [`SeedOutcome`]: no silent arm. A deletion that
/// failed to tombstone a default is a space that comes back on the next
/// refresh, and the person watching it reappear needs a line in the log saying
/// which file keeper could not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteRecord {
    /// The key is in the ledger now; the seeder will not offer it again.
    Recorded(String),
    /// The ledger already named it, which is the ordinary case: keeper claims a
    /// default's key the first time it seeds or stands down for it, so by the
    /// time anyone can delete the space the key is recorded.
    AlreadyRecorded(String),
    /// The note was not a default space. The ledger has nothing to say about a
    /// space a person wrote, and nothing would re-create it.
    NotADefault,
    /// Recorded nothing, deliberately. The string is the sentence for the log.
    Blocked(String),
}

impl DeleteRecord {
    /// The level and the sentence this outcome deserves in the log, for
    /// [`REPORT_FLOOR`]'s reason: a decision the app made about somebody's
    /// vault that only shows up as a space reappearing days later.
    pub fn report(&self) -> (tracing::Level, String) {
        match self {
            Self::Recorded(key) => (
                REPORT_FLOOR,
                format!("recorded the deleted default space {key} in {LEDGER_REL}; it will not be seeded again"),
            ),
            Self::AlreadyRecorded(key) => (
                REPORT_FLOOR,
                format!("{LEDGER_REL} already records the default space {key}; it will not be seeded again"),
            ),
            Self::NotADefault => (
                REPORT_FLOOR,
                "deleted a space keeper did not seed; nothing to record".to_owned(),
            ),
            Self::Blocked(why) => (
                tracing::Level::WARN,
                format!("deleted the space, but did not record it as deleted, so a later refresh may offer it again. {why}"),
            ),
        }
    }
}

/// Record that a default space was deleted on purpose, so [`seed`] will not put
/// it back (Story 45.17, FR-195).
///
/// **This invents no tombstone, because the ledger already is one.** `offered`
/// is the set of keys keeper has claimed in this vault, and [`plan`] skips every
/// key in it on a `FirstRun`. So "deleted on purpose" and "already claimed" are
/// the same fact from the seeder's side, and the deletion's whole job is to make
/// sure the key is in that set. A second file saying "and this one is deleted"
/// would be a second answer to one question, and the two would disagree the
/// first time somebody restored a vault from a backup that had one and not the
/// other.
///
/// It is not a no-op, and the case that makes it load-bearing is a ledger write
/// that FAILED: [`keys_recorded`] is best effort, so a full disk or a read-only
/// `.keeper-spaces.json` leaves a vault with keeper's spaces on disk and nothing
/// recorded. Deleting one of them then has to record it, because nothing did,
/// and the next automatic run would otherwise write it straight back.
///
/// **What this cannot do, and Story 47.4 had to fix in [`seed`] instead.** Until
/// DW-191 this comment claimed the load-bearing case was a default keeper stood
/// down for because the user already had a space of that name. It never was:
/// that space is the *user's*, it carries no `keeper.default`, so
/// [`default_key_of`] answers `None` and this returns [`DeleteRecord::NotADefault`]
/// — correctly, because the ledger has nothing to say about a space a person
/// wrote. The name was left unclaimed and keeper's version arrived in its place
/// on the next refresh. That is closed one layer up, where the stand-down
/// happens, by [`claimed`].
///
/// `source` is the note's text, read before the bytes move. An unreadable
/// ledger blocks rather than being overwritten, exactly as it does in [`seed`]:
/// a file that is there and is not a ledger keeper wrote may be a newer build's,
/// and replacing it would re-offer that build's defaults.
pub fn record_deleted(vault: &mut dyn SeedVault, source: &str) -> DeleteRecord {
    let Some(key) = default_key_of(source) else {
        return DeleteRecord::NotADefault;
    };
    let mut keys = match read_ledger(vault) {
        Ok(Some(keys)) => keys,
        // [`read_ledger`] documents this as unreachable — an absent ledger is
        // an empty set, which is a fact rather than an absence of one. Read as
        // the empty set rather than unwrapped, because the bytes have already
        // moved by the time this runs and a panic here would take the command
        // down after the deletion succeeded.
        Ok(None) => BTreeSet::new(),
        Err(reason) => return DeleteRecord::Blocked(reason),
    };
    if !keys.insert(key.clone()) {
        return DeleteRecord::AlreadyRecorded(key);
    }
    match vault.write(LEDGER_REL, &render_ledger(&keys)) {
        Ok(()) => DeleteRecord::Recorded(key),
        Err(error) => DeleteRecord::Blocked(format!("{LEDGER_REL}: {error}")),
    }
}

/// The keys this vault has already been offered, or the sentence explaining why
/// keeper cannot tell.
///
/// `Ok(None)` is impossible on purpose — an absent ledger is `Ok(Some(empty))`,
/// which is a fact, not an absence of one.
fn read_ledger(vault: &dyn SeedVault) -> Result<Option<BTreeSet<String>>, String> {
    match vault.read(LEDGER_REL) {
        Ok(text) => match parse_ledger(&text) {
            Some(keys) => Ok(Some(keys)),
            None => Err(format!(
                "{LEDGER_REL} is there and is not a seed ledger; leaving this vault's spaces alone"
            )),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Some(BTreeSet::new())),
        Err(error) => Err(format!(
            "{LEDGER_REL} could not be read ({error}); leaving this vault's spaces alone"
        )),
    }
}

/// The spaces already in `spaces/`, or the sentence explaining why keeper cannot
/// tell.
///
/// A directory that is not there is not an error: `spaces/` is created lazily,
/// and a vault without one has no spaces. A directory that is there and cannot
/// be listed is an error, because "I saw nothing" and "I could not look" lead to
/// opposite writes.
///
/// A single unreadable *file* inside it is neither: it is one space keeper
/// cannot identify, and it contributes a name that matches no default rather
/// than taking the whole run down. That is the conservative direction here —
/// the run still cannot write over it, because its filename is in `taken`.
fn read_existing(vault: &dyn SeedVault) -> Result<Vec<ExistingSpace>, String> {
    let names = match vault.list(SPACES_DIR) {
        Ok(names) => names,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            return Err(format!(
                "{SPACES_DIR}/ is there and could not be listed ({error}); leaving this vault's spaces alone"
            ))
        }
    };
    Ok(names
        .into_iter()
        .filter(|name| name.ends_with(".md"))
        .map(|filename| {
            let rel = format!("{SPACES_DIR}/{filename}");
            let source = vault.read(&rel).unwrap_or_default();
            let (fm, body_offset) = Frontmatter::parse(&source);
            let stem = filename.strip_suffix(".md").unwrap_or(&filename);
            ExistingSpace {
                default_key: default_key_of(&source),
                name: naming::note_title(fm.as_string("title"), &source[body_offset..], stem),
                filename,
            }
        })
        .collect())
}

/// Where a space note lives, named here because [`seed`] composes the path and
/// the shell must not compose a second one.
pub const SPACES_DIR: &str = "spaces";

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
            filename: format!("{}.md", naming::slug(name)),
        }
    }

    // -----------------------------------------------------------------------
    // A real directory, driven through the real port
    //
    // Story 44.3 shipped with every assertion below `plan` and none above it,
    // and it did nothing on the owner's vault. These tests exist so the run
    // that decides whether to write into somebody's vault is exercised against
    // a filesystem, with real permission bits, on the host it is written on.
    // -----------------------------------------------------------------------

    /// The production adapter, spelt out: four `std::fs` calls and two clocks.
    /// The shell's own impl is the same four calls over `notes_vault`.
    struct DiskVault {
        root: std::path::PathBuf,
        ids: u32,
        /// Every `write` the run attempted, in order, whether or not it landed.
        attempted: Vec<String>,
        /// A path whose write is refused, to stand in for a full disk.
        refuse: Option<String>,
    }

    impl DiskVault {
        fn new(root: std::path::PathBuf) -> Self {
            Self {
                root,
                ids: 0,
                attempted: Vec::new(),
                refuse: None,
            }
        }
    }

    impl SeedVault for DiskVault {
        fn read(&self, rel: &str) -> std::io::Result<String> {
            std::fs::read_to_string(self.root.join(rel))
        }
        fn list(&self, rel_dir: &str) -> std::io::Result<Vec<String>> {
            let mut out = Vec::new();
            for entry in std::fs::read_dir(self.root.join(rel_dir))? {
                out.push(entry?.file_name().to_string_lossy().into_owned());
            }
            out.sort();
            Ok(out)
        }
        fn write(&mut self, rel: &str, text: &str) -> std::io::Result<()> {
            self.attempted.push(rel.to_owned());
            if self.refuse.as_deref() == Some(rel) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::StorageFull,
                    "no space left on device",
                ));
            }
            let path = self.root.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, text)
        }
        fn new_id(&mut self) -> String {
            self.ids += 1;
            format!("01J8ZQ4M7T5R9V3XK2B6C0DF{:02}", self.ids)
        }
        fn now_local(&self) -> String {
            "2026-08-09T10:00:00+02:00".to_owned()
        }
        fn today(&self) -> String {
            "2026-08-09".to_owned()
        }
    }

    fn temp_vault() -> DiskVault {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-default-spaces-{}-{}-{n}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("temp vault");
        DiskVault::new(dir)
    }

    /// Put a space note in `spaces/`, the way `notes_space_save` writes one:
    /// no `title` key, the name as the body's heading.
    fn put_space(vault: &DiskVault, filename: &str, name: &str, query: &str) {
        let dir = vault.root.join(SPACES_DIR);
        std::fs::create_dir_all(&dir).expect("spaces/");
        let text = format!(
            "---\nid: 01USER{filename}\ncreated: 2026-08-08T09:00:00+02:00\nkeeper:\n  space: '{query}'\n  sort: modified desc\n  limit: 500\n---\n\n# {name}\n"
        );
        std::fs::write(dir.join(filename), text).expect("write a space");
    }

    /// The space names sitting in `spaces/`, read back the way the rail reads
    /// them. The independent side of every assertion below.
    fn names_on_disk(vault: &DiskVault) -> Vec<String> {
        let mut out: Vec<String> = read_existing(vault)
            .expect("spaces/ lists")
            .into_iter()
            .map(|space| space.name)
            .collect();
        out.sort();
        out
    }

    fn keys(of: &[&'static DefaultSpace]) -> Vec<&'static str> {
        of.iter().map(|space| space.key).collect()
    }

    fn ledger(of: &[&str]) -> BTreeSet<String> {
        of.iter().map(|key| (*key).to_owned()).collect()
    }

    /// The whole reason the defaults could become spaces: every query they run
    /// is already in the closed `is:` set. If one of them were not, seeding
    /// would write a space that refuses to parse into a fresh vault — a rail of
    /// broken rows on first run.
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
    ///
    /// Templates never was a row, and it is pinned here for the sharper version
    /// of the same reason: `tag:template` and `is:template` look interchangeable
    /// and are not. 44.7 widened the predicate to the frontmatter tag OR the
    /// legacy `templates/` prefix, so the tag spelling would silently omit every
    /// template a pre-44.7 vault still keeps in that folder. Writing the exact
    /// string down is what makes that a failing test rather than a shrug.
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
                ("templates", "is:template"),
            ]
        );
    }

    /// Every default's icon is one the picker can actually draw.
    ///
    /// The seeded set and the icon set live in two languages and two crates, and
    /// the only thing joining them is a string in frontmatter. A default whose
    /// glyph name is not in `SPACE_ICONS` renders the unknown-icon fallback on
    /// first run — a rail of default rows that all look broken, which is exactly
    /// what 44.3's own doc comment says the first four exist to prevent. The
    /// TypeScript half of this pair is `every seeded default names an icon the
    /// picker has` in `space-editor.test.tsx`; neither half can see the other,
    /// so both are written down.
    #[test]
    fn every_default_names_an_icon_and_no_two_defaults_share_a_key() {
        let mut keys: Vec<&str> = DEFAULT_SPACES.iter().map(|space| space.key).collect();
        keys.sort_unstable();
        let unique = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), unique, "two defaults share a key: {keys:?}");
        for space in &DEFAULT_SPACES {
            assert!(!space.icon.is_empty(), "{} names no icon", space.key);
            assert!(
                space
                    .icon
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{} names {:?}, which is not a lucide key",
                space.key,
                space.icon
            );
        }
    }

    #[test]
    fn a_fresh_vault_is_offered_every_default() {
        let plan = plan(SeedMode::FirstRun, &[], Some(&BTreeSet::new()));
        assert_eq!(
            keys(&plan),
            vec!["inbox", "journal", "pinned", "recordings", "templates"]
        );
    }

    /// The story's own acceptance: delete one, reopen, and it does not come
    /// back. The ledger remembers the offer, not the note.
    #[test]
    fn a_default_the_ledger_already_offered_is_never_written_again() {
        let all = ledger(&["inbox", "journal", "pinned", "recordings", "templates"]);
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
        assert_eq!(keys(&plan), vec!["pinned", "recordings", "templates"]);
    }

    /// Restore is the user asking, so the ledger does not veto it — but it still
    /// only fills holes.
    #[test]
    fn restore_writes_the_missing_and_leaves_the_present_alone() {
        let all = ledger(&["inbox", "journal", "pinned", "recordings", "templates"]);
        let present = [
            existing("Inbox", Some("inbox")),
            existing("Recordings", Some("recordings")),
        ];
        let plan = plan(SeedMode::Restore, &present, Some(&all));
        assert_eq!(keys(&plan), vec!["journal", "pinned", "templates"]);

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
            vec!["journal", "pinned", "templates"]
        );
        assert_eq!(
            keys(&plan(
                SeedMode::Restore,
                &renamed,
                Some(&ledger(&[
                    "inbox",
                    "journal",
                    "pinned",
                    "recordings",
                    "templates"
                ]))
            )),
            vec!["journal", "pinned", "templates"]
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
            vec!["inbox", "journal", "pinned", "recordings", "templates"]
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
            vec!["inbox", "journal", "pinned", "recordings", "templates"]
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
            assert_eq!(
                keys(&plan),
                vec!["journal", "pinned", "recordings", "templates"]
            );
        }
        // A name that folds to something else is a different space and blocks
        // nothing — the fold is the filename rule, so `In box` is `in-box` and
        // is not Inbox.
        for other in ["Unfiled", "In box", "Inboxes"] {
            let mine = [existing(other, None)];
            assert_eq!(
                keys(&plan(SeedMode::FirstRun, &mine, Some(&BTreeSet::new()))),
                vec!["inbox", "journal", "pinned", "recordings", "templates"],
                "{other} is not Inbox"
            );
        }
    }

    /// A ledger keeper cannot read is not "this vault was never seeded". Reading
    /// it that way would put every default back into a vault whose owner may
    /// have deleted all of them, which is the one outcome worth being timid
    /// about.
    #[test]
    fn an_unreadable_ledger_stops_the_automatic_seed_and_not_the_manual_one() {
        assert!(plan(SeedMode::FirstRun, &[], None).is_empty());
        assert_eq!(
            keys(&plan(SeedMode::Restore, &[], None)),
            vec!["inbox", "journal", "pinned", "recordings", "templates"]
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
            vec!["journal", "pinned", "recordings", "templates"]
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
            // By key, not by index: the array grew from four to five in Story
            // 45.20 and an index would have silently re-pointed this assertion
            // at a different space's bytes.
            by_key("recordings").expect("the Recordings default exists"),
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

    // -----------------------------------------------------------------------
    // The run, against a real directory
    // -----------------------------------------------------------------------

    /// The owner's vault, reproduced from the field report: four saved filters
    /// under `spaces/`, made from the Recordings lens on 2026-08-08, and no
    /// ledger. The installed build wrote nothing here and logged nothing.
    ///
    /// This is the test that would have caught it. It fails on any run that
    /// declines silently, because `AlreadySatisfied` and `Blocked` are different
    /// values and neither is `Wrote`.
    #[test]
    fn the_owners_vault_gets_its_defaults_beside_the_four_spaces_it_already_had() {
        let mut vault = temp_vault();
        for (n, filename) in [
            "2026-08-08-recordings-first-recording.md",
            "2026-08-08-recordings-first-recording-2.md",
            "2026-08-08-recordings-first-recording-3.md",
            "2026-08-08-recordings-first-recording-4.md",
        ]
        .iter()
        .enumerate()
        {
            put_space(
                &vault,
                filename,
                &format!("Recordings · first-recording{}", " ".repeat(n)),
                "is:recording tag:first-recording",
            );
        }

        let outcome = seed(&mut vault, SeedMode::FirstRun);

        assert_eq!(
            outcome,
            SeedOutcome::Wrote(vec![
                "spaces/2026-08-09-inbox.md".to_owned(),
                "spaces/2026-08-09-journal.md".to_owned(),
                "spaces/2026-08-09-pinned.md".to_owned(),
                "spaces/2026-08-09-recordings.md".to_owned(),
                "spaces/2026-08-09-templates.md".to_owned(),
            ])
        );
        // Their four are still there, untouched, beside keeper's five. Sorted,
        // so keeper's bare "Recordings" comes before their "Recordings · …".
        let names = names_on_disk(&vault);
        assert_eq!(names.len(), 9, "{names:?}");
        assert_eq!(
            &names[..4],
            ["Inbox", "Journal", "Pinned", "Recordings"],
            "{names:?}"
        );
        assert_eq!(names[8], "Templates", "{names:?}");
        for theirs in &names[4..8] {
            assert!(
                theirs.starts_with("Recordings · first-recording"),
                "{theirs}"
            );
        }
    }

    /// A fresh vault with no `spaces/` at all. An absent directory is not a
    /// failure to read one.
    #[test]
    fn a_vault_with_no_spaces_directory_is_seeded_rather_than_declined() {
        let mut vault = temp_vault();
        let outcome = seed(&mut vault, SeedMode::FirstRun);
        assert!(
            matches!(&outcome, SeedOutcome::Wrote(w) if w.len() == DEFAULT_SPACES.len()),
            "{outcome:?}"
        );
        assert_eq!(
            names_on_disk(&vault),
            ["Inbox", "Journal", "Pinned", "Recordings", "Templates"]
        );
    }

    /// Twice in a row writes nothing the second time, and says which of the two
    /// silences it is.
    #[test]
    fn a_second_run_over_the_same_directory_is_already_satisfied_and_says_so() {
        let mut vault = temp_vault();
        assert!(matches!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::Wrote(_)
        ));
        assert_eq!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::AlreadySatisfied
        );
        assert_eq!(
            names_on_disk(&vault).len(),
            DEFAULT_SPACES.len(),
            "nothing doubled"
        );
    }

    /// The story's own acceptance, now against files: delete one and reopen.
    #[test]
    fn a_deleted_default_is_not_resurrected_by_the_next_run() {
        let mut vault = temp_vault();
        assert!(matches!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::Wrote(_)
        ));
        std::fs::remove_file(vault.root.join("spaces/2026-08-09-pinned.md")).expect("delete");

        assert_eq!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::AlreadySatisfied
        );
        assert_eq!(
            names_on_disk(&vault),
            ["Inbox", "Journal", "Recordings", "Templates"]
        );

        // And restore brings back exactly the one that is gone.
        assert_eq!(
            seed(&mut vault, SeedMode::Restore),
            SeedOutcome::Wrote(vec!["spaces/2026-08-09-pinned.md".to_owned()])
        );
        assert_eq!(
            names_on_disk(&vault),
            ["Inbox", "Journal", "Pinned", "Recordings", "Templates"]
        );
    }

    /// Story 45.20's own acceptance for the fifth default, end to end against a
    /// real directory: seeded, ledgered, and gone for good once deleted.
    ///
    /// Its own test rather than a line added to the four above, because the
    /// three facts it asserts are three different failures and each has already
    /// happened to a sibling: a default that is planned and never written, a
    /// default written and never recorded (so the next launch writes it again),
    /// and a default the automatic run resurrects after the user threw it away.
    /// It also reads the note's own bytes rather than only the outcome list —
    /// the outcome names a path, and a path is not a query.
    #[test]
    fn the_templates_space_is_seeded_ledgered_and_stays_deleted() {
        let mut vault = temp_vault();
        let rel = "spaces/2026-08-09-templates.md";

        let outcome = seed(&mut vault, SeedMode::FirstRun);
        assert!(
            matches!(&outcome, SeedOutcome::Wrote(written) if written.iter().any(|w| w == rel)),
            "the first run writes it: {outcome:?}"
        );

        // Seeded: the note on disk is a space note carrying the predicate 44.7
        // widened, the glyph the picker draws, and the marker that survives a
        // rename. Read off the file, not off the constant.
        let note = std::fs::read_to_string(vault.root.join(rel)).expect("the note is there");
        assert!(note.contains("space: is:template"), "{note}");
        assert!(note.contains("icon: layout-template"), "{note}");
        assert_eq!(
            default_key_of(&note).as_deref(),
            Some("templates"),
            "{note}"
        );
        assert!(note.ends_with("# Templates\n"), "{note}");

        // Ledgered: recorded beside its four siblings, so the record is what
        // stops the next run rather than the file merely existing.
        let recorded = parse_ledger(
            &std::fs::read_to_string(vault.root.join(LEDGER_REL)).expect("a ledger was written"),
        )
        .expect("it is a ledger keeper wrote");
        assert!(recorded.contains("templates"), "{recorded:?}");

        // Deleted: it stays deleted, and the run says which silence it is.
        std::fs::remove_file(vault.root.join(rel)).expect("delete");
        assert_eq!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::AlreadySatisfied
        );
        assert_eq!(
            names_on_disk(&vault),
            ["Inbox", "Journal", "Pinned", "Recordings"],
            "keeper does not put back a space the user threw away"
        );

        // …until the user asks, which is the whole point of the two modes.
        assert_eq!(
            seed(&mut vault, SeedMode::Restore),
            SeedOutcome::Wrote(vec![rel.to_owned()])
        );
    }

    /// The unplugged drive: two notes landed, the ledger never did. The next run
    /// finishes the job rather than repeating the first two.
    #[test]
    fn a_half_written_seed_on_disk_converges_without_doubling() {
        let mut vault = temp_vault();
        vault.refuse = Some("spaces/2026-08-09-pinned.md".to_owned());
        let stopped = seed(&mut vault, SeedMode::FirstRun);
        assert!(
            matches!(&stopped, SeedOutcome::Stopped { written, reason }
                if written.len() == 2 && reason.contains("pinned")),
            "{stopped:?}"
        );

        vault.refuse = None;
        vault.attempted.clear();
        let finished = seed(&mut vault, SeedMode::FirstRun);
        assert_eq!(
            finished,
            SeedOutcome::Wrote(vec![
                "spaces/2026-08-09-pinned.md".to_owned(),
                "spaces/2026-08-09-recordings.md".to_owned(),
                "spaces/2026-08-09-templates.md".to_owned(),
            ])
        );
        assert_eq!(
            names_on_disk(&vault),
            ["Inbox", "Journal", "Pinned", "Recordings", "Templates"]
        );
    }

    /// A ledger keeper cannot read blocks the automatic run — and **says which
    /// file and why**. Silence here is what made the field report unanswerable.
    #[test]
    fn an_unreadable_ledger_blocks_the_run_with_a_sentence_naming_the_file() {
        let mut vault = temp_vault();
        std::fs::write(vault.root.join(LEDGER_REL), "{ this is not json").expect("write");

        match seed(&mut vault, SeedMode::FirstRun) {
            SeedOutcome::Blocked(why) => {
                assert!(why.contains(LEDGER_REL), "{why}");
                assert!(why.contains("not a seed ledger"), "{why}");
            }
            other => panic!("expected a spoken refusal, got {other:?}"),
        }
        assert!(names_on_disk(&vault).is_empty(), "nothing was written");

        // The user pressing Restore is not blocked by it, and repairs it.
        let repaired = seed(&mut vault, SeedMode::Restore);
        assert!(
            matches!(&repaired, SeedOutcome::Wrote(w) if w.len() == DEFAULT_SPACES.len()),
            "{repaired:?}"
        );
        assert_eq!(
            parse_ledger(&std::fs::read_to_string(vault.root.join(LEDGER_REL)).expect("read")),
            Some(
                ["inbox", "journal", "pinned", "recordings", "templates"]
                    .iter()
                    .map(|k| (*k).to_owned())
                    .collect()
            )
        );
    }

    /// **The one the field report needed.** A ledger that exists and cannot be
    /// *opened* — not one that fails to parse — is the class that fits a vault
    /// on removable media: EACCES from macOS TCC on `/Volumes`, EIO from a drive
    /// that spun down. The shipped code mapped every `io::Error` except
    /// `NotFound` to a silent permanent no-op, and no test distinguished an
    /// unopenable file from an absent one, which is precisely why it went green
    /// on two hosts and wrote nothing on the owner's.
    #[cfg(unix)]
    #[test]
    fn a_ledger_that_cannot_be_opened_is_not_read_as_an_absent_one() {
        use std::os::unix::fs::PermissionsExt as _;
        let mut vault = temp_vault();
        let path = vault.root.join(LEDGER_REL);
        std::fs::write(&path, render_ledger(&ledger(&["inbox"]))).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");

        let outcome = seed(&mut vault, SeedMode::FirstRun);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("restore");

        match &outcome {
            SeedOutcome::Blocked(why) => {
                assert!(why.contains(LEDGER_REL), "{why}");
                assert!(why.contains("could not be read"), "{why}");
                // The errno is in the sentence, so the next field report is one
                // grep rather than three candidates.
                assert!(
                    why.to_lowercase().contains("permission"),
                    "the reason has to name the errno: {why}"
                );
            }
            other => panic!("expected a spoken refusal, got {other:?}"),
        }
        assert!(
            names_on_disk(&vault).is_empty(),
            "an unreadable ledger must not become 'never seeded'"
        );

        // And it is not permanent: once the file is readable the next run
        // proceeds, honouring what the ledger actually said.
        let outcome = seed(&mut vault, SeedMode::FirstRun);
        assert_eq!(
            outcome,
            SeedOutcome::Wrote(vec![
                "spaces/2026-08-09-journal.md".to_owned(),
                "spaces/2026-08-09-pinned.md".to_owned(),
                "spaces/2026-08-09-recordings.md".to_owned(),
                "spaces/2026-08-09-templates.md".to_owned(),
            ]),
            "Inbox was already offered; the others were not"
        );
    }

    /// A run that stopped part way still records what landed, so a default that
    /// was written and then deleted stays deleted.
    ///
    /// The on-disk check alone makes the *immediate* retry correct, which is why
    /// this needs its own test: without the record, deleting the two notes that
    /// did land would make the next automatic run write them again — keeper
    /// putting back rows the user threw away, which is the AD-79 failure arriving
    /// through the crash path instead of the happy one.
    #[test]
    fn a_run_that_stopped_still_recorded_what_landed_so_deleting_it_sticks() {
        let mut vault = temp_vault();
        vault.refuse = Some("spaces/2026-08-09-pinned.md".to_owned());
        let stopped = seed(&mut vault, SeedMode::FirstRun);
        assert!(matches!(&stopped, SeedOutcome::Stopped { written, .. } if written.len() == 2));

        assert_eq!(
            parse_ledger(&std::fs::read_to_string(vault.root.join(LEDGER_REL)).expect("recorded")),
            Some(
                ["inbox", "journal"]
                    .iter()
                    .map(|k| (*k).to_owned())
                    .collect()
            ),
            "the two that landed were recorded before the run gave up"
        );

        // The user throws both away. The drive comes back. Nothing returns.
        vault.refuse = None;
        std::fs::remove_file(vault.root.join("spaces/2026-08-09-inbox.md")).expect("delete");
        std::fs::remove_file(vault.root.join("spaces/2026-08-09-journal.md")).expect("delete");

        assert_eq!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::Wrote(vec![
                "spaces/2026-08-09-pinned.md".to_owned(),
                "spaces/2026-08-09-recordings.md".to_owned(),
                "spaces/2026-08-09-templates.md".to_owned(),
            ])
        );
        assert_eq!(names_on_disk(&vault), ["Pinned", "Recordings", "Templates"]);
    }

    /// The bug the first version had in the other direction. A `spaces/` that is
    /// there and cannot be listed must not read as "this vault has no spaces" —
    /// on a sleeping USB volume that writes a second Inbox beside the first.
    #[cfg(unix)]
    #[test]
    fn a_spaces_directory_that_cannot_be_listed_blocks_the_run_rather_than_reading_as_empty() {
        use std::os::unix::fs::PermissionsExt as _;
        let mut vault = temp_vault();
        put_space(&vault, "2026-08-08-inbox.md", "Inbox", "is:untagged");
        let dir = vault.root.join(SPACES_DIR);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).expect("chmod");

        let outcome = seed(&mut vault, SeedMode::FirstRun);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("restore");

        match outcome {
            SeedOutcome::Blocked(why) => {
                assert!(why.contains("spaces/"), "{why}");
                assert!(why.contains("could not be listed"), "{why}");
            }
            other => panic!("expected a spoken refusal, got {other:?}"),
        }
        assert_eq!(
            names_on_disk(&vault),
            ["Inbox"],
            "their Inbox is alone and untouched"
        );
    }

    /// One space inside `spaces/` that cannot be read is not the whole vault.
    /// It contributes a name that matches no default, and its filename is still
    /// taken, so the run cannot write over it.
    #[cfg(unix)]
    #[test]
    fn one_unreadable_space_does_not_take_the_run_down_and_is_not_written_over() {
        use std::os::unix::fs::PermissionsExt as _;
        let mut vault = temp_vault();
        put_space(&vault, "2026-08-09-inbox.md", "Inbox", "is:untagged");
        let file = vault.root.join("spaces/2026-08-09-inbox.md");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).expect("chmod");

        let outcome = seed(&mut vault, SeedMode::FirstRun);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("restore");

        // Its name is unreadable, so Inbox is not stood down by name — but the
        // filename it holds is, so keeper's Inbox lands beside it under a
        // counter rather than on top of it.
        assert_eq!(
            outcome,
            SeedOutcome::Wrote(vec![
                "spaces/2026-08-09-inbox-2.md".to_owned(),
                "spaces/2026-08-09-journal.md".to_owned(),
                "spaces/2026-08-09-pinned.md".to_owned(),
                "spaces/2026-08-09-recordings.md".to_owned(),
                "spaces/2026-08-09-templates.md".to_owned(),
            ])
        );
        assert!(
            std::fs::read_to_string(&file)
                .expect("still readable now")
                .contains("01USER"),
            "the unreadable space was not overwritten"
        );
    }

    /// A user space really called Inbox, on disk, with no marker. keeper stands
    /// down for that one key and writes the rest.
    #[test]
    fn a_user_space_named_inbox_on_disk_stands_keepers_inbox_down() {
        let mut vault = temp_vault();
        put_space(&vault, "2026-08-08-inbox.md", "Inbox", "tag:unfiled");

        let outcome = seed(&mut vault, SeedMode::FirstRun);

        assert_eq!(
            outcome,
            SeedOutcome::Wrote(vec![
                "spaces/2026-08-09-journal.md".to_owned(),
                "spaces/2026-08-09-pinned.md".to_owned(),
                "spaces/2026-08-09-recordings.md".to_owned(),
                "spaces/2026-08-09-templates.md".to_owned(),
            ])
        );
        // Theirs is byte-identical: keeper never edits a space it did not write.
        let theirs = std::fs::read_to_string(vault.root.join("spaces/2026-08-08-inbox.md"))
            .expect("still there");
        assert!(theirs.contains("tag:unfiled"), "{theirs}");
        assert!(!theirs.contains("default:"), "{theirs}");
    }

    /// The seeded notes are readable as spaces by the code that reads spaces —
    /// which is what makes the rail render them at all.
    #[test]
    fn what_the_seed_wrote_reads_back_as_marked_defaults() {
        let mut vault = temp_vault();
        assert!(matches!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::Wrote(_)
        ));

        let read = read_existing(&vault).expect("spaces/ lists");
        let mut marked: Vec<(String, String)> = read
            .into_iter()
            .filter_map(|space| space.default_key.map(|key| (key, space.name)))
            .collect();
        marked.sort();
        assert_eq!(
            marked,
            vec![
                ("inbox".to_owned(), "Inbox".to_owned()),
                ("journal".to_owned(), "Journal".to_owned()),
                ("pinned".to_owned(), "Pinned".to_owned()),
                ("recordings".to_owned(), "Recordings".to_owned()),
                ("templates".to_owned(), "Templates".to_owned()),
            ]
        );
        for space in &DEFAULT_SPACES {
            let text = std::fs::read_to_string(
                vault
                    .root
                    .join(format!("spaces/2026-08-09-{}.md", naming::slug(space.name))),
            )
            .expect("the seeded note is where the outcome said it is");
            let after = &text[text.find("space: ").map_or(0, |i| i + 7)..];
            let query = after.lines().next().unwrap_or_default();
            assert!(
                query::parse(query).is_ok(),
                "the seeded query must parse: {query}"
            );
        }
    }

    /// **Every outcome has to be printable on the machine it ran on.**
    ///
    /// The second attempt at this story replaced a silent code path with
    /// `tracing::debug!`, and the desktop subscriber's default filter is
    /// `EnvFilter::new("info")` with no `RUST_LOG` anywhere in the macOS app —
    /// so the ordinary outcome was still invisible and the third field report
    /// was still a blank log. This is the assertion that would have caught it,
    /// and it is worth more than its two lines: it converts "remember not to use
    /// debug! here" into something the compiler's test runner enforces.
    #[test]
    fn no_seed_outcome_reports_below_the_level_the_app_can_print() {
        // Pinned to the literal level, not merely to itself. Comparing every
        // outcome against `REPORT_FLOOR` alone is vacuous the moment somebody
        // lowers the constant to make a `debug!` fit — which is exactly the
        // pressure that produced the round-2 defect. The number is the one
        // `debug_log::init` installs: `EnvFilter::new("info")`, with no
        // `RUST_LOG` anywhere in the macOS app to raise it.
        assert_eq!(
            REPORT_FLOOR,
            tracing::Level::INFO,
            "the desktop subscriber's default filter is `info`; a floor below it \
             means this whole test asserts nothing"
        );
        let every = [
            SeedOutcome::Wrote(vec!["spaces/a.md".to_owned()]),
            SeedOutcome::Wrote(Vec::new()),
            SeedOutcome::AlreadySatisfied,
            SeedOutcome::Blocked("the ledger could not be read".to_owned()),
            SeedOutcome::Stopped {
                written: vec!["spaces/a.md".to_owned()],
                reason: "no space left on device".to_owned(),
            },
        ];
        for outcome in &every {
            let (level, message) = outcome.report();
            // `tracing::Level` orders ERROR below WARN below INFO, so "at least
            // as severe as INFO" is `<=`.
            assert!(
                level <= REPORT_FLOOR,
                "{outcome:?} reports at {level}, which the app's own filter drops"
            );
            assert!(!message.is_empty(), "{outcome:?} says nothing");
        }

        // And a refusal keeps the reason it was given, so the sentence in the
        // log is the one that names the file.
        let (level, message) = SeedOutcome::Blocked(
            ".keeper-spaces.json could not be read (permission denied)".to_owned(),
        )
        .report();
        assert_eq!(level, tracing::Level::WARN);
        assert!(message.contains(".keeper-spaces.json"), "{message}");
        assert!(message.contains("permission denied"), "{message}");
    }

    // -----------------------------------------------------------------------
    // Deleting a space (Story 45.17, FR-195)
    // -----------------------------------------------------------------------

    /// The ledger the vault currently holds, as a sorted list, read the way
    /// [`seed`] reads it. The independent side of every assertion below.
    fn recorded(vault: &DiskVault) -> Vec<String> {
        parse_ledger(&vault.read(LEDGER_REL).unwrap_or_default())
            .map(|keys| keys.into_iter().collect())
            .unwrap_or_default()
    }

    /// Delete a space the way the shell does: read its bytes, remove it, then
    /// record. The order is the production order and it matters — the marker is
    /// in the file, and after the removal there is nothing to read it from.
    fn delete_space(vault: &mut DiskVault, rel: &str) -> DeleteRecord {
        let source = vault.read(rel).expect("read the space before deleting it");
        std::fs::remove_file(vault.root.join(rel)).expect("delete the space");
        record_deleted(vault, &source)
    }

    /// The ordinary case, end to end: seed, delete one of keeper's own, run the
    /// automatic seed again, and the space stays gone.
    ///
    /// The ledger already named it — `seed` recorded the key when it wrote the
    /// note — so the deletion has nothing to add, and saying `AlreadyRecorded`
    /// rather than writing is the assertion that the tombstone is the ledger
    /// and not something this story invented.
    #[test]
    fn deleting_a_seeded_default_leaves_it_deleted_across_a_reseed() {
        let mut vault = temp_vault();
        assert!(matches!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::Wrote(_)
        ));
        let before = recorded(&vault);
        assert!(before.contains(&"recordings".to_owned()), "{before:?}");

        assert_eq!(
            delete_space(&mut vault, "spaces/2026-08-09-recordings.md"),
            DeleteRecord::AlreadyRecorded("recordings".to_owned()),
        );
        assert_eq!(recorded(&vault), before, "the ledger did not need changing");

        assert_eq!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::AlreadySatisfied,
            "the automatic run must not put back a space the user threw away"
        );
        let names = names_on_disk(&vault);
        assert!(!names.contains(&"Recordings".to_owned()), "{names:?}");
    }

    /// **The case the ledger could not already answer, and the reason
    /// [`record_deleted`] exists.**
    ///
    /// [`keys_recorded`] is best effort — `let _ = vault.write(...)` — so a
    /// vault whose ledger write failed has its seeded spaces and no ledger at
    /// all. Deleting one of them then has to record it, because there is
    /// nothing there to have recorded it already, and the next automatic run
    /// would otherwise write it straight back.
    ///
    /// Reached the way it happens in the field rather than by hand-removing the
    /// file: the seed runs with the ledger path refused, which is what a full
    /// disk or a read-only `.keeper-spaces.json` does to it.
    #[test]
    fn deleting_a_default_the_ledger_never_recorded_still_tombstones_it() {
        let mut vault = temp_vault();
        vault.refuse = Some(LEDGER_REL.to_owned());
        assert!(matches!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::Wrote(_)
        ));
        vault.refuse = None;
        assert!(
            recorded(&vault).is_empty(),
            "the premise: spaces on disk and nothing recorded"
        );
        let before = names_on_disk(&vault);

        assert_eq!(
            delete_space(&mut vault, "spaces/2026-08-09-pinned.md"),
            DeleteRecord::Recorded("pinned".to_owned()),
        );
        assert_eq!(recorded(&vault), vec!["pinned".to_owned()]);

        assert_eq!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::AlreadySatisfied,
            "every other default is present and the deleted one is recorded"
        );
        let after = names_on_disk(&vault);
        assert!(!after.contains(&"Pinned".to_owned()), "{after:?}");
        assert_eq!(
            after.len(),
            before.len() - 1,
            "exactly the deleted one is gone and nothing else moved: {before:?} -> {after:?}"
        );
    }

    /// A space a person wrote is not keeper's, and the ledger has nothing to
    /// say about it. The bytes are compared rather than the parse, because the
    /// failure this guards against is a delete rewriting a file it had no
    /// reason to touch.
    #[test]
    fn deleting_a_space_keeper_did_not_seed_leaves_the_ledger_byte_identical() {
        let mut vault = temp_vault();
        assert!(matches!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::Wrote(_)
        ));
        put_space(&vault, "2026-08-09-clients.md", "Clients", "tag:clients");
        let before = vault.read(LEDGER_REL).expect("ledger");

        assert_eq!(
            delete_space(&mut vault, "spaces/2026-08-09-clients.md"),
            DeleteRecord::NotADefault,
        );
        assert_eq!(
            vault.read(LEDGER_REL).expect("ledger"),
            before,
            "not one byte of the ledger changed"
        );
    }

    /// A ledger keeper cannot parse is not overwritten, for [`seed`]'s reason:
    /// it may be a newer build's, and replacing it would re-offer that build's
    /// defaults. The deletion still happened; what is refused is the record.
    #[test]
    fn an_unreadable_ledger_blocks_the_record_and_is_left_alone() {
        let mut vault = temp_vault();
        assert!(matches!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::Wrote(_)
        ));
        let theirs = "{\"version\":99,\"written-by\":\"a keeper from the future\"}\n";
        vault.write(LEDGER_REL, theirs).expect("plant the ledger");

        match delete_space(&mut vault, "spaces/2026-08-09-journal.md") {
            DeleteRecord::Blocked(why) => assert!(why.contains(LEDGER_REL), "{why}"),
            other => panic!("expected a spoken refusal, got {other:?}"),
        }
        assert_eq!(
            vault.read(LEDGER_REL).expect("ledger"),
            theirs,
            "keeper must not replace a ledger it could not read"
        );
    }

    /// No outcome of a deletion is invisible in the app's own log, for
    /// [`REPORT_FLOOR`]'s reason — and every arm that knows a key names it,
    /// because "the space came back" is otherwise the only symptom anyone gets.
    #[test]
    fn no_delete_record_reports_below_the_level_the_app_can_print() {
        let every = [
            DeleteRecord::Recorded("pinned".to_owned()),
            DeleteRecord::AlreadyRecorded("pinned".to_owned()),
            DeleteRecord::NotADefault,
            DeleteRecord::Blocked(format!("{LEDGER_REL}: permission denied")),
        ];
        for outcome in &every {
            let (level, message) = outcome.report();
            assert!(
                level <= REPORT_FLOOR,
                "{outcome:?} reports at {level}, which the app's own filter drops"
            );
            assert!(!message.is_empty(), "{outcome:?} says nothing");
        }
        for outcome in [
            DeleteRecord::Recorded("pinned".to_owned()),
            DeleteRecord::AlreadyRecorded("pinned".to_owned()),
        ] {
            let (_, message) = outcome.report();
            assert!(message.contains("pinned"), "{outcome:?} does not name it");
        }
        let (level, message) =
            DeleteRecord::Blocked(format!("{LEDGER_REL}: permission denied")).report();
        assert_eq!(level, tracing::Level::WARN);
        assert!(message.contains(LEDGER_REL), "{message}");
        assert!(message.contains("permission denied"), "{message}");
    }

    // -----------------------------------------------------------------------
    // The names keeper claims (Story 47.4, DW-191)
    // -----------------------------------------------------------------------

    /// The presence rule, on its own, in both of its two forms.
    ///
    /// [`plan`] and [`seed`] read this one function — the planner to decide what
    /// to skip, the seeder to decide what to record — so a drift between "stood
    /// down for" and "claimed" is not expressible. That is the whole reason it
    /// is a function and not a filter written twice.
    #[test]
    fn a_default_is_claimed_by_its_key_or_by_its_name_and_by_nothing_else() {
        assert!(
            claimed(&[]).is_empty(),
            "an empty vault claims no name for keeper"
        );

        // By key, surviving a rename: the marker is the identity (AD-79).
        assert_eq!(
            claimed(&[existing("Unfiled", Some("inbox"))]),
            ledger(&["inbox"])
        );

        // By name, folded the way `naming::slug` folds it, so the three
        // spellings of one name are one claim and not three misses.
        for spelling in ["Inbox", "inbox", "  INBOX  "] {
            assert_eq!(
                claimed(&[existing(spelling, None)]),
                ledger(&["inbox"]),
                "{spelling:?} is the Inbox name"
            );
        }

        // A name that is nobody's default, and a marker this build does not
        // know, each claim nothing — an unrecognised `keeper.default` must not
        // become a key that stops a real default from ever being offered.
        assert!(claimed(&[existing("Clients", None)]).is_empty());
        assert!(claimed(&[existing("Clients", Some("archive"))]).is_empty());

        // The set, not the first hit: a vault mid-seed claims every one present.
        assert_eq!(
            claimed(&[
                existing("Inbox", None),
                existing("Sessions", Some("recordings")),
                existing("Clients", None),
            ]),
            ledger(&["inbox", "recordings"])
        );
    }

    /// **DW-191, on disk, end to end.** The user wrote their own Inbox before
    /// keeper shipped one. keeper stands down for the name — and now records it.
    /// They delete their Inbox. Nothing arrives in its place.
    ///
    /// Without the claim, the next run sees a name absent from the vault AND
    /// absent from the ledger and writes keeper's Inbox: you delete your space
    /// and a different one comes back. [`record_deleted`] cannot catch it —
    /// their note carries no `keeper.default`, so the deletion is correctly
    /// `NotADefault` and records nothing, which is asserted here rather than
    /// assumed.
    #[test]
    fn a_default_stood_down_for_a_name_the_user_took_is_claimed_and_their_space_stays_gone() {
        let mut vault = temp_vault();
        put_space(&vault, "2026-08-08-inbox.md", "Inbox", "tag:unfiled");

        let outcome = seed(&mut vault, SeedMode::FirstRun);
        assert_eq!(
            outcome,
            SeedOutcome::Wrote(vec![
                "spaces/2026-08-09-journal.md".to_owned(),
                "spaces/2026-08-09-pinned.md".to_owned(),
                "spaces/2026-08-09-recordings.md".to_owned(),
                "spaces/2026-08-09-templates.md".to_owned(),
            ]),
            "keeper still stands down rather than writing a second Inbox"
        );
        assert!(
            recorded(&vault).contains(&"inbox".to_owned()),
            "the name keeper stood down for is claimed: {:?}",
            recorded(&vault)
        );

        // Their space, deleted the way the shell deletes one. The ledger has
        // nothing to add — it is not keeper's space — and does not need to.
        assert_eq!(
            delete_space(&mut vault, "spaces/2026-08-08-inbox.md"),
            DeleteRecord::NotADefault,
        );

        assert_eq!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::AlreadySatisfied,
            "the name is claimed, so nothing is planned for it"
        );
        let names = names_on_disk(&vault);
        assert!(
            !names.contains(&"Inbox".to_owned()),
            "the user deleted their Inbox and keeper's must not arrive instead: {names:?}"
        );
        assert_eq!(
            names,
            ["Journal", "Pinned", "Recordings", "Templates"],
            "and nothing else moved"
        );

        // …until they ask. Restore ignores the ledger, which is its entire job,
        // so the escape hatch from a claim is still one menu item away.
        assert_eq!(
            seed(&mut vault, SeedMode::Restore),
            SeedOutcome::Wrote(vec!["spaces/2026-08-09-inbox.md".to_owned()])
        );
    }

    /// **The upgrade path**, which is the part of DW-191 that is a product call
    /// rather than a bug fix: vaults already exist with a ledger written under
    /// the old meaning, holding the keys keeper WROTE and not the names it stood
    /// down for.
    ///
    /// The decision, made here and not left to hope: the first run after the
    /// change reconciles. It writes no space note — nothing is missing — and it
    /// records the claims the old ledger could not have held. The alternative,
    /// waiting until a run happens to write something, leaves the defect live on
    /// exactly the vaults that already have it, which is every installed one.
    ///
    /// The old state is built by hand rather than by running the old code,
    /// because the old code is gone: keeper's four notes on disk, the user's own
    /// Inbox beside them, and a ledger naming only the four.
    #[test]
    fn a_ledger_written_under_the_old_meaning_is_reconciled_by_the_first_run_after_it() {
        let mut vault = temp_vault();
        put_space(&vault, "2026-08-08-inbox.md", "Inbox", "tag:unfiled");
        for space in DEFAULT_SPACES.iter().filter(|space| space.key != "inbox") {
            let filename = format!("2026-08-08-{}.md", naming::slug(space.name));
            vault
                .write(
                    &format!("{SPACES_DIR}/{filename}"),
                    &render_note(space, "01OLD", "2026-08-08T09:00:00+02:00"),
                )
                .expect("plant one of keeper's own");
        }
        let old = ledger(&["journal", "pinned", "recordings", "templates"]);
        vault
            .write(LEDGER_REL, &render_ledger(&old))
            .expect("plant the old ledger");
        assert_eq!(
            recorded(&vault),
            vec!["journal", "pinned", "recordings", "templates"],
            "the premise: the old ledger holds only what keeper wrote"
        );

        assert_eq!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::AlreadySatisfied,
            "nothing is missing, so no space note is written"
        );
        assert_eq!(
            recorded(&vault),
            vec!["inbox", "journal", "pinned", "recordings", "templates"],
            "…and the name keeper stood down for years ago is claimed now"
        );

        // The upgrade is what makes the deletion stick. Without it this vault
        // still hands the next refresh an unclaimed `inbox`.
        assert_eq!(
            delete_space(&mut vault, "spaces/2026-08-08-inbox.md"),
            DeleteRecord::NotADefault,
        );
        assert_eq!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::AlreadySatisfied
        );
        let names = names_on_disk(&vault);
        assert!(
            !names.contains(&"Inbox".to_owned()),
            "an upgraded vault protects the user's space too: {names:?}"
        );
    }

    /// The reconciliation writes ONCE. A ledger rewritten on every refresh is a
    /// synced file modified on every launch, which is a commit per launch in
    /// somebody's vault history — the sync engine cannot tell keeper's
    /// bookkeeping from a real edit.
    ///
    /// Asserted against every write the run attempted, not against the bytes:
    /// identical content rewritten still moves an mtime and still stages.
    #[test]
    fn a_settled_vault_touches_nothing_at_all_on_the_runs_after_the_upgrade() {
        let mut vault = temp_vault();
        put_space(&vault, "2026-08-08-inbox.md", "Inbox", "tag:unfiled");
        assert!(matches!(
            seed(&mut vault, SeedMode::FirstRun),
            SeedOutcome::Wrote(_)
        ));

        for run in 0..3 {
            vault.attempted.clear();
            assert_eq!(
                seed(&mut vault, SeedMode::FirstRun),
                SeedOutcome::AlreadySatisfied
            );
            assert!(
                vault.attempted.is_empty(),
                "run {run} after the claim rewrote {:?}",
                vault.attempted
            );
        }
    }

    /// A Restore over a ledger keeper cannot parse, with nothing to restore,
    /// must not invent one — [`seed`]'s standing rule that a file that is there
    /// and is not keeper's may be a newer build's, and replacing it re-offers
    /// that build's defaults.
    ///
    /// This is the one path where the claim write could have reached an
    /// unreadable ledger: an automatic run has already returned `Blocked` before
    /// it gets here.
    #[test]
    fn a_restore_with_nothing_to_restore_does_not_write_a_ledger_over_one_it_could_not_read() {
        let mut vault = temp_vault();
        for space in &DEFAULT_SPACES {
            let filename = format!("2026-08-08-{}.md", naming::slug(space.name));
            vault
                .write(
                    &format!("{SPACES_DIR}/{filename}"),
                    &render_note(space, "01OLD", "2026-08-08T09:00:00+02:00"),
                )
                .expect("plant one of keeper's own");
        }
        let theirs = "{\"version\":99,\"written-by\":\"a keeper from the future\"}\n";
        vault.write(LEDGER_REL, theirs).expect("plant the ledger");

        assert_eq!(
            seed(&mut vault, SeedMode::Restore),
            SeedOutcome::AlreadySatisfied
        );
        assert_eq!(
            vault.read(LEDGER_REL).expect("ledger"),
            theirs,
            "keeper must not replace a ledger it could not read"
        );
    }
}
