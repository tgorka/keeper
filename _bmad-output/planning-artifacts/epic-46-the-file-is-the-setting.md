# Epic 46 — The file is the setting

status: draft
created: 2026-08-10
altitude: epic
parent: Phase 5 (Notes), Epic 45 (open it, change it, put it back)
source: a fourth field report from the owner, taken hours after epic 45 was installed — seventeen items
binds: FR-200–FR-221, AD-98–AD-106, UX-DR77–UX-DR84

## Why this epic exists

Seventeen items. Six of them are defects in code that shipped this morning, and every one of
those six has the same shape:

**keeper does the thing and then fails to show you that it did.**

An attach copies the file and the panel named "Attachments" cannot list it. A note delete ships
complete and lives behind an unlabelled `⋯`. A save succeeds and the toolbar jumps. A folder tree
remembers what you expanded until you look at something else. A `.gitattributes` rule is written
and then written again, fifty-nine times, because the reader that checks whether it is already
there cannot recognise its own output.

The rest of the report is one sentence:

**Every setting should be a file the owner can edit, and the settings that belong to a folder
should travel with the folder.**

That is not a preference for TOML. It is the observation that keeper has, by count, six settings
stores — a SQLite k/v table, a flat `config.json` imported destructively over it, a JSON blob per
sync profile, `NotesConfig` inside that blob, four browser cookies, and a TOML file only the Linux
daemon reads — and that the one which is hand-editable is the one nobody documented, holds a flat
namespace, and is overwritten by the first UI toggle after boot.

## Where we take a position

**A settings value is resolved from a stack of layers, not imported into a table** (AD-98). Today
`import_config_file` writes `config.json`'s contents into the `settings` rows; the file wins exactly
once, at boot, and the next UI toggle erases it. That is why nobody uses it. A layer stack resolves
at read time: the file keeps winning, and a UI control that would be overridden says so instead of
silently losing. This is the whole difference between "a config file" and "a config file that
works".

**The layer order the owner wrote is the layer order we implement** (AD-99):

```
~/.keeper/*.toml                          user, every machine, every folder
<main>/.keeper/*.toml                     shared, every machine        \ the designated
<main>/.keeper/*.<machine>.toml           shared folder, this machine  / main sync folder
<other>/.keeper/*.toml                    that folder only
<other>/.keeper/*.<machine>.toml          that folder, this machine
```

with the constraint the owner also stated: a non-main folder may only set keys that are *about
itself*. That is not a courtesy — it is what stops two folders fighting over `hotkey.global`.

**`.keeper/*.toml` is exempted from the `.keeper/` exclusion, deliberately and narrowly** (AD-100).
`.keeper/` is a tier-0 built-in exclude (`exclude.rs:159`) and `notes_vault::rebuild` deletes it as
a supported repair, because everything in it so far has been a cache (`index.json`) or machine-local
(`trash/`). Config in a sync folder that does not sync is a contradiction: the reason to put it
there instead of `~/.keeper/` is precisely that it should reach the other machine. So `*.toml`
directly under `.keeper/` syncs, and survives a rebuild. Nothing else about the directory changes.
Recorded as an exemption rather than a relaxation, because `default_spaces.rs:47-50` chose
`.keeper-spaces.json` at the vault root over `.keeper/` for exactly the reason this exemption
removes, and the next reader will find that comment.

The mechanism is a pre-check, not a pattern. `exclude.rs` compiles one `globset::GlobSet`, and
globset has **no negation** — a `!.keeper/*.toml` entry would be read as a literal pattern
beginning with a bang. So the exemption is answered before the set is consulted, in one place,
and tested for the three ways it could leak: a `.toml` deeper than one level under `.keeper/`, a
non-`.toml` file directly under it, and a directory named `something.toml`.

**One header shape, extracted on the second consumer and not before** (AD-104). `NoteEditor`'s
header and `PanelFrame`'s header are the same construction — a `flex-1` title, then a variable
status element, then buttons, all shrink participants in one non-wrapping row — and both acquire
the same jump the moment a status element grows. The structure is three groups: identity absorbs
all slack, status is `shrink-0` and independent, actions are `shrink-0` and may themselves vary in
width (`Show in Files` resolves asynchronously after first paint, which is a second source of the
jump the caption fix alone would not have removed). Story 46.4 lands it concretely in the note
editor; story 46.13 extracts the shared component when it adds the Files pane's Save control,
because that is the second real consumer. It is recorded here so 46.13 reproduces a decision
rather than a shape half-remembered from a diff.

**Two-phase load, because the layers below need a database the layers above configure** (AD-101).
`import_config_file` runs at `lib.rs:227`, before `debug_log::init`, because `debug.mode` must apply
to this boot. Three of the five layers are keyed on sync-folder paths, which live in `sync.db`,
which is not open until `start_supervisor` at `lib.rs:428` — and which itself needs `sync.git_path`
from the settings. A real cycle. It is cut by recording the main folder's path in the user-global
layer: phase one reads `~/.keeper/`, learns where main is, and reads it directly from disk without
the database; phase two, after the engine opens, layers the per-folder files for the keys a folder
is allowed to set.

**A write outside the vault is a different promise, made out loud** (AD-102). AD-89's three
promises — only inside a vault, one writer, an announced removal — are not scope, they are what
makes an edit reach the reconciler and a deletion recoverable. The owner has now asked to edit and
delete files that are not in any vault. We do it, and we say what it costs: an out-of-vault edit is
a plain atomic write that takes no vault machinery, and an out-of-vault delete goes to the
operating system's trash rather than the vault's, because there is no vault trash to reach. Both
surfaces say so before they act. AD-89 is not overturned; it is scoped to what it always described,
and the second path is named rather than grown by accident.

**Corrected during 46.14, and the correction matters more than the decision.** This paragraph
originally said an out-of-vault edit is "a plain atomic write that no sync engine learns about".
That is false for the exact file the owner complained about. `AGENTS.md` sits **inside a sync
profile**, just outside the notes vault; the folder engine watches the whole profile root and
`browse::classify` reports it `Synced`. So the edit *is* committed and *does* reach the other
machine. What it does not get is what the **vault** provides: no note history, no search index, no
conflict copy. The caveat the surface shows names those and only those, and a test asserts it never
says "does not sync" — because a caveat that overstates its own cost teaches the reader to ignore
the next one.

**The panel named Attachments lists attachments** (AD-103). It currently reads the `files:`
frontmatter key and short-circuits on any note without a recording session, which made it a
recording-session panel wearing a general name. An attach writes an embed into the body. The panel
becomes a reader of *both*: the session's `files:` list where there is one, and the body's own
embeds pointing into `attachments/`, which is what the person who pressed "Attach a file" is
looking for.

## What we are deliberately not doing

**Not merging the recordings subfolder into the path template.** The head is per-profile in
`sync.db`; the tail is per-machine in the settings table. Merging them puts a fact that must be the
same on both machines into a key that cannot be, and the second machine syncing that folder writes
somewhere else. The owner's ask — "let me choose the whole path" — is answered by showing the head
where the tail is chosen, and by letting the head be more than one segment.

**Not allowing recordings at the profile root.** Eight sites depend on the root being a non-empty
subfolder disjoint from the notes vault; two of them are safety arguments rather than conveniences
(the `keeper-recording://` sandbox's non-overlap proof, and the tier-2-skipping commit gate that
would otherwise apply to every file in the folder including notes). The subfolder is already
free-form and may be nested, which is the part of the ask that is real.

**Not repairing the fifty-nine broken `.gitattributes` lines in the same story that stops writing
them.** The mechanism exists — `ensure_attributes` rewrites the whole file — but rewriting lines a
user may have edited is a different decision from not emitting them, and bundling the two means a
bug fix that cannot be reverted without also reverting a data migration.

## A ruling story 46.2 asked for, made here so 46.11 does not invent one

46.2 scoped the Attachments panel to files under `attachments/`, and named the residual gap: a file
attached from **inside** the vault but living somewhere else (`photos/a.png`) never acquires the
`attachments/` prefix, so the panel does not list it. That costs nothing today, because the only
attach path copies into `attachments/`.

Story 46.11 makes it live — attaching from a folder you already sync means attaching files that are
already in the vault, at their own paths, and copying them into `attachments/` would be the wrong
answer: it duplicates a file the vault already holds and the sync engine already carries.

**So the panel lists an embed that resolves to a file in this vault, wherever it lives, and
`attachments/` stops being the test.** What makes a row is that the note embeds it and the vault
holds it — which is also what `keeper_core::notes::export::plan` already means by "a file this note
needs", and 46.2 deliberately mirrored `names_a_note` so the two could not disagree. 46.11 owns the
widening; 46.2's narrower reader is correct for what existed when it shipped.

## One deviation from the layout the owner wrote

The owner listed five tiers and gave a machine variant only to the two sync-folder tiers. The
implementation has **six**: `~/.keeper/keeper.<machine>.toml` exists too, and wins over
`~/.keeper/keeper.toml`.

The argument made for it — "the only place a per-machine absolute path can live" — is not quite
right, because `~` is already per-machine and `keeper.toml` there is already machine-local. The
reason to keep it anyway is narrower and real: **a home directory can itself be synced.** Dotfile
syncing is common, and a `~/.keeper/keeper.toml` that travels needs somewhere to put the one value
that must not — which is exactly what the sync-folder tiers needed a machine variant for. The tier
is free when `~` is not synced (the file simply does not exist) and load-bearing when it is.

Recorded rather than folded in silently, because it is a deviation from a layout the owner wrote
out explicitly, and they should get to disagree with it.
