# Spec 46.8 — A folder and a machine can each say something

story: 46.8
binds: AD-99, AD-100, AD-101, FR-200…FR-221
crates touched: `keeper-sync` (owner), `keeper-syncd`, `keeper`, `keeper-core`
gate: `cargo test -p keeper-sync --lib -- exclude:: profile::` → **67 passed, EXIT=0**

## What shipped

1. **The `.keeper/*.toml` carve-out** (`keeper-sync/src/exclude.rs`). `.keeper/` stays a
   tier-0 exclusion; a `*.toml` **directly under it** is not. globset has no negation — a
   `!.keeper/*.toml` entry compiles as a literal glob beginning with a bang — so the
   exemption is a pre-check in one function, `is_exempt_config_file`, answered before the
   `GlobSet` is consulted.
2. **The folder tier** (`keeper-sync/src/profile/folder.rs`, new). `<folder>/.keeper/keeper.toml`
   then `keeper.<host>.toml`, overlaid onto the `SyncProfile` the row holds, at read time,
   through `SyncProfile::validate`.
3. **`notes_vault::rebuild` is selective, and the doc that called the directory
   safe-to-delete is corrected** (`keeper/src/notes_vault.rs`, `keeper-core/src/notes/default_spaces.rs`).
4. **One profile↔TOML mapping**, moved out of `keeper-syncd` into `keeper-sync` beside the
   type both callers deserialize into.

## How I shared `keeper-syncd`'s profile↔TOML mapping — moved, not called across, not duplicated

The mapping was three things in `keeper-syncd/src/config.rs`: `canonical_key`
(snake_case → the schema's camelCase), `accepted_profile_keys` (the accepted set *derived
from `SyncProfile`* at runtime), and the loop in `parse_profile` that folds a table's keys
and refuses an unknown key or one key under two spellings.

I **moved all three down into `keeper-sync/src/profile/mod.rs`**, beside `SyncProfile`
itself, as `accepted_profile_keys`, `canonical_key` and `canonical_profile_fields`.
`keeper-syncd` now calls them; its `parse_profile` keeps only what is genuinely the
daemon's — the `toml::Value → serde_json::Value` hop, the `[[profile]] #N (name):` prefix,
and the duplicate-id check.

Why moved rather than called across: `keeper-syncd` depends on `keeper-sync`, so the edge
already exists in that direction and in no other. The alternative — the folder tier calling
into `keeper-syncd` — is a dependency from the engine onto a binary crate, which is not a
thing. Duplicating was the option I was told to avoid and it is the right instruction: the
whole value of `accepted_profile_keys` is that it is derived from the type, and a second
copy of a derived thing is a copy that can be derived from a *different* type after the next
refactor.

Consequence I accepted: the shared functions return errors with **no positional prefix**.
The daemon says `[[profile]] #2 (docs): unknown key ...`; the folder tier says
`<path>: [folder] unknown key ...`. Both wrap the same sentence. The one syncd test that
unit-tested `canonical_key` moved with the function (`keeper-sync`'s
`canonical_key_folds_snake_case_onto_the_profile_schema`); the daemon keeps its end-to-end
`snake_case_keys_are_the_same_keys`, which is the behaviour it actually promises.

What I did **not** share: `render_profile_table`. It renders a whole `[[profile]]` table for
`keeper-syncd add`, comments and all, and nothing in the folder tier writes a file. Reaching
for it would have meant generalising a writer for a caller that does not exist.

## The crate boundary, and why there are two readers of the main folder's file

`keeper-sync` is deliberately `keeper-core`-free (AD-40, asserted by
`bun run check:core-sync-free`). W2Layers' layer engine — which reads `~/.keeper/*` and the
main folder's files for the **settings table** — lives in `keeper-core`. So the folder tier
cannot call it and it cannot call the folder tier. The division landed as:

| file | `[settings]` read by | `[folder]` read by |
| --- | --- | --- |
| `~/.keeper/keeper*.toml` | `keeper-core::config` | nobody (no folder) |
| `<main>/.keeper/keeper*.toml` | `keeper-core::config` | `keeper-sync::profile::folder` |
| `<other>/.keeper/keeper*.toml` | **refused, named fault** | `keeper-sync::profile::folder` |

The main folder's file is therefore opened twice, by two crates, for two different tables.
That is the price of AD-40 and it is cheap (one `read_to_string` of a file measured in
hundreds of bytes). The faults do **not** double up: `keeper-core` only ever parses `~` and
main, and the folder tier stays silent about `[settings]` **in the main folder** precisely so
one file cannot earn two faults from two crates that cannot see each other's fault list.

## The overlay is resolved at read time and taken back off at write time

`db::list_profiles` and `db::get_profile` return the profile **in force** — the row with the
folder's files layered on. `db::upsert_profile` calls `profile::as_stored` first, which
restores every field the folder files currently own from the pre-write row (or from the
type's own default for a profile being created).

This is AD-98 enforced at the write funnel and it is not optional. Without it the first
read-modify-write — `Engine::set_enabled` is the plain one: `get_profile` → mutate →
`upsert_profile` — copies the file's values into the row, and deleting the file later
reveals a copy of it rather than the value the user chose. That is exactly the `config.json`
failure the epic exists to end, re-created one layer down. A field a folder file owns and the
user edits in the UI is kept out of the row and said out loud at `warn!` naming the profile,
the fields and the folder; it is not silently dropped and it is not refused (every other
field in that write lands).

Default is **uninstalled**: until `install_folder_tier` is called, no folder file is opened
and both funnels are identity. `keeper-syncd` never calls it, so the daemon is byte-for-byte
unchanged (see "deliberately NOT done").

## What a folder file may say

```toml
# <folder>/.keeper/keeper.toml         — this folder, every machine
# <folder>/.keeper/keeper.<host>.toml  — this folder, this machine (LATER WINS)

[folder]
branch = "trunk"
excludes = ["*.psd"]
lfs_never = ["*.csv"]              # snake_case and camelCase are one key
lfsThresholdBytes = 1048576
commitSubjectTemplate = "keeper: {profile}"
tags = ["work"]

[folder.notes]
subfolder = "40-notes"             # partial: journalTemplate keeps its stored value

[folder.recordings]
subfolder = "40-media/recordings"
```

`FOLDER_FIELD_RULES` classifies **every** `SyncProfile` field into one of three answers, and
`folder_field_rules_cover_every_profile_field` asserts the list is exactly
`accepted_profile_keys()` — so a field added to `SyncProfile` tomorrow fails that test until
somebody decides, rather than defaulting into whichever half is convenient.

| rule | fields | why |
| --- | --- | --- |
| **Allowed** | `branch`, `excludes`, `lfsNever`, `lfsThresholdBytes`, `commitSubjectTemplate`, `tags`, `notes`, `recordings` | repository policy and what the folder contains. These must be the same on every clone or the clones disagree about what they are committing. |
| **Identity** | `id`, `name`, `remoteUrl`, `volumeId` | says which folder this is or which repository it belongs to. A file *inside* the folder must not be able to re-point the clone that read it. |
| **MachineLocal** | `localPath`, `direction`, `lane`, `subpaths`, `removable`, `lfsMode`, `lfsPruneLocal`, `settleMs`, `pollIntervalMs`, `authorOverride`, `enabled` | true of this clone only. `localPath` is where *this* machine mounted it; `lane`/`direction` is whether *this* clone is an agent's push-only worktree; `lfsMode`/`lfsPruneLocal`/`subpaths` are this disk's size; `settleMs`/`pollIntervalMs` are this medium's latency; `authorOverride` is who is at this keyboard; `enabled` is whether this machine is syncing right now. |

The four the owner named — `localPath`, `remoteUrl`, `volumeId`, `id` — are refused **by
name**, with the rule in the sentence:

```
/x/.keeper/keeper.toml: [folder] may not set `localPath`: machine-local — it is true of this
clone only, and this file travels to machines where it is not. A folder file may set branch,
excludes, lfsNever, lfsThresholdBytes, commitSubjectTemplate, tags, notes, recordings
```

`name` is classified Identity rather than Allowed. It reads like a label, but it is the label
in the tray, in every log span and in every generated commit subject; a folder that could
rename itself on the machine that read its file would produce two machines writing
differently-attributed commits into one history. If that turns out to be wanted it is a
one-line change plus a test, and the test that forces the decision already exists.

`[settings]` outside the main folder:

```
/x/.keeper/keeper.toml: may not carry `[settings]` key(s) hotkey.global: a folder that is not
the main sync folder may only set keys about itself, or two folders would fight over one
app-wide setting. Move them to `~/.keeper/keeper.toml` or to the main sync folder's file
```

## I/O matrix

| input | output |
| --- | --- |
| no `.keeper/` at all | profile unchanged, no fault (the ordinary case; two `ENOENT` opens) |
| `keeper.toml` only | its `[folder]` applied |
| `keeper.toml` + `keeper.<host>.toml` | shared first, machine second, **later wins**, key by key |
| `keeper.<other-host>.toml` | not read on this machine; it still syncs to the machine it names |
| `[folder]` with an allowed key | applied; key recorded in `owned` |
| `[folder]` with an identity/machine-local key | layer dropped **whole**, one fault naming file+key+rule |
| `[folder]` with a key `SyncProfile` does not have | layer dropped, fault listing the accepted keys |
| one key under both spellings | layer dropped, "given twice" |
| `[folder]` that fails `validate()` | layer dropped, the validator's own sentence, prefixed |
| a key nested inside `[folder.notes]` that the profile did not take | layer dropped, fault naming the dotted path (`notes.subfoldr`) |
| `[settings]`, non-main folder | layer dropped whole, fault naming the keys and the rule |
| `[settings]`, main folder | ignored here (keeper-core owns it); `[folder]` still applies |
| `mainSyncFolder` in any folder file | layer dropped, fault |
| unknown top-level key | layer dropped, fault naming it |
| malformed TOML | layer dropped, fault carrying toml's line/column/caret snippet unflattened |
| unreadable file (EACCES) | layer dropped, fault carrying the io error |
| host label empty / containing `/` or `\` | only the shared file is read; no second path is composed |
| tier not installed | every read and write is identity |

Exclusion side:

| path | `is_excluded` (file) | `is_excluded_directory` |
| --- | --- | --- |
| `.keeper/keeper.toml`, `sub/.keeper/keeper.hesperia.toml` | **false** | — |
| `.keeper/index.json` | true | — |
| `.keeper/sub/x.toml`, `sub/.keeper/trash/01J/draft.toml` | true | — |
| `.keeper/x.toml` as a **directory** | — | **true** |
| `.keeper/x.toml/index.json` | true | — |
| `.keeper`, `sub/.keeper` | true | true |
| `sub/.keeperrc` | false (unchanged) | false |
| `.keeper/keeper.toml` with a profile pattern `*.toml` | false, verdict `Included` | — |

## Edge cases and the decisions behind them

- **A layer is dropped whole, never half-applied.** A file with one bad key does not get its
  good keys applied. A half-honoured configuration is one nobody can reason about, and the
  alternative is deciding on the user's behalf which half of their file they meant. All
  problems in one file are reported together (joined by `; `) rather than one per fix.
- **Nested tables deep-merge; arrays replace.** `[folder.notes] subfolder = "x"` keeps the
  stored `journalTemplate`. Replacing the whole `notes` object would silently reset every
  sibling to its serde default — the "I changed one thing and something else moved" failure
  this epic is about. Arrays replace wholesale because appending to `excludes` would leave no
  way to remove an entry.
- **An override that did not take is a fault.** After merge, deserialize and validate, the
  overlay is compared leaf-by-leaf against the resulting profile; anything the profile did not
  carry is named by dotted path. This catches what a top-level key check structurally cannot:
  a misspelling *inside* `[folder.notes]`, and a field dropped by an internally-tagged enum
  that did not want it (`PushPolicy`'s `quietFrom` under `kind = "sessionEnd"`). Serde ignores
  both silently, and a silent no-op is the exact defect shape epic 46 was written about.
- **The machine-variant file syncs**, deliberately, so one machine's settings can be edited
  from another. `the_machine_variant_config_file_syncs_like_the_shared_one` exists to argue
  with the next person who tries to "fix" that.
- **The carve-out is the corpus's and it is absolute.** A user's own `*.toml` exclude pattern
  cannot take the config file back. `verdict` now routes through `is_excluded` /
  `is_excluded_directory` rather than matching the set itself, so a browser cannot mark a file
  excluded while the engine is committing it. A profile pattern is only ever *named* by
  `verdict`, never applied — which was already true (every profile pattern is in `set` too),
  and is now true by construction rather than by two code paths agreeing.
- **Directory vs file.** `is_excluded_directory` deliberately does **not** delegate to
  `is_excluded` any more, because the carve-out is for files. A directory named `x.toml` under
  `.keeper/` stays hidden, and so does everything inside it.
- **`sub/.keeperrc` still visible.** The pre-existing regression test at the old
  `exclude.rs:548-551` is untouched and passing; the carve-out matches on a `/`-separated
  parent segment equal to `.keeper`, so a name merely beginning with it is unaffected.
- **The notes walk still refuses `.keeper/` by name before descent**, so no folder config file
  is ever parsed as a note or stat'd by the indexer. That refusal is `is_refused_dir`, not the
  `ExcludeSet`, which is why the carve-out did not leak into it — recorded in the walk's doc
  so the next reader does not "fix" the apparent inconsistency.
- **`is_internal` in `notes_vault`** still treats any `.keeper` path segment as internal, so a
  config write does not wake the notes reconciler. Unchanged, and now load-bearing.
- **Fault list is a live snapshot, not a log.** An entry appears when a profile whose file is
  broken is read and disappears the moment that file reads cleanly, so nobody chases a fault
  they already fixed.
- **Process-global under a parallel test runner.** Three tests arm the tier; they take a
  `TIER_TEST` mutex and disarm through a `Drop` guard rather than a call at the end, because a
  failing assertion unwinds past a trailing call and leaves every later test with an armed
  tier. This was not theoretical — the first run of the suite failed exactly that way.

## `notes_vault::rebuild`

`rebuild` already deleted only `<vault>/.keeper/index.json`; the danger was entirely in the
prose. Three claims were false after AD-100 and are corrected in this commit:

- `rebuild`'s doc said "deleting `.keeper/` by hand is the same repair". It now says the
  opposite, names AD-100, and says this function must never grow into a `remove_dir_all` —
  the trash is the user's recoverable deletions and the `*.toml` are their settings, and
  neither is a cache a rescan can regenerate.
- The `load_cache` log line told the user, at `info`, that deleting `.keeper/` is a supported
  repair. It now names `index.json` and says why not the directory.
- `default_spaces.rs`'s rationale for putting `.keeper-spaces.json` at the vault root rested
  on "`.keeper/` is documented as a deletable cache". The epic predicted this comment would be
  the next reader's confusion, so it now records that AD-100 changed the premise and why the
  conclusion still holds (the carve-out is `*.toml` and nothing else; a JSON ledger under
  `.keeper/` would still be excluded and still be swept by anyone following the old advice).

No code change was needed in `rebuild` itself. Saying so plainly is better than moving a line
to look busy.

## Mutation table

Each mutation applied to `profile/folder.rs` alone, run as
`cargo test -p keeper-sync --lib profile::folder::`, then restored and the restore verified by
sha256 against a pristine copy (`/tmp/mutate.py`) — not by memory of the edit.

| # | mutation | result | tests that caught it |
| --- | --- | --- | --- |
| 1 | drop the `is_exempt_config_file` pre-check in `ExcludeSet::is_excluded` (`exclude.rs`) | 3 failed / 17 passed | `a_folders_own_config_file_is_exempt_from_the_cache_exclusion`, `the_machine_variant_config_file_syncs_like_the_shared_one`, `a_profile_pattern_cannot_take_the_config_file_back` |
| 2 | `"settings" => problems.push(settings_refusal(value))` → `=> {}` | 1 failed / 18 passed | `a_non_main_folder_may_not_carry_settings` |
| 3 | `match folder_field_rule(&name)` → `match Some(Allowed)` (every field allowed) | 3 failed / 16 passed | `a_folder_file_cannot_set_a_machine_local_or_identity_field`, `a_refused_layer_leaves_the_stored_profile_exactly_as_it_was`, `a_fixed_file_stops_being_reported` |
| 4 | `if let Err(err) = next.validate()` → `let _ = next.validate();` | 2 failed / 17 passed | `the_overlay_goes_through_the_engines_own_validator`, `a_validator_rule_below_the_top_level_still_refuses_the_layer` |
| 5 | `if !unobserved.is_empty()` → `if false` | 1 failed / 18 passed | `a_key_the_profile_did_not_take_is_a_fault_rather_than_a_silent_no_op` |
| 6 | drop `target.insert(key, kept)` in `as_stored` (report but do not strip) | 1 failed / 18 passed | `a_write_never_stores_what_the_file_said` |

Mutation 1 predates the file split and was verified by re-reading `git diff` for the restored
call site, plus the `grep` tool on the five mutation anchors afterwards.

Mutation 5's **first** run was invalid: it came back with five compile errors in
`keeper-sync/src/files_write.rs` (`WriteRoute`, `UnmanagedPath`, `VaultPath` not found), which
is W3Outside's story 46.14 mid-edit in a file I do not touch. Re-run after their file
compiled; recorded here because "the mutation broke the build" would otherwise read as proof.

## Deliberately NOT done

- **`keeper-syncd` does not install the folder tier.** The daemon's operator hand-edits a TOML
  that already carries the complete `[[profile]]` table. Arming the tier there would give one
  profile two file authorities for the same fields with no stated precedence, and — worse —
  `as_stored` would then strip the daemon's own configured values on `reconcile`. Deciding
  which file wins for a headless server is a real decision and it is not this story's. The
  daemon is byte-for-byte unchanged: `install_folder_tier` is never called, so both funnels are
  identity.
- **No folder file is ever written.** The tier reads. There is no "save these settings to the
  folder" path, no `render_profile_table` reuse, and no migration that moves stored fields into
  a file. A file appears because a person wrote one.
- **Nothing re-reads on a watcher event.** The files are read on every profile read, which
  means an edit takes effect on the next read (≤1 supervisor tick for anything the scheduler
  touches) without a relaunch. No inotify watch, no cache, no invalidation to get wrong.
- **No `[settings]` from a folder ever reaches the settings table**, including from the main
  folder — this module does not touch the settings table at all. W2Main's spec records the
  consequence: the keys consumed between `lib.rs:227` and `:428` can only come from the four
  phase-one files.
- **`.keeper/*.toml` deeper than one level stays excluded** and there is no plan to relax it.
  Nothing keeper writes puts config a level down, and the trash tree lives there.
- **The 59 broken `.gitattributes` lines and the `keeper.<host>.toml` conflict story** are
  other stories. Two machines editing the same `keeper.toml` produce an ordinary git conflict
  and an ordinary conflict copy; the tier reports the malformed result as a fault rather than
  guessing.

## What I could not verify here, and why — ordered gate checks

`keeper` (the Tauri shell) does not link on Linux, so `notes_vault.rs` and the shell side of
the wiring were edited and read but never compiled by me. My edits there are three doc
comments and one `tracing::info!` string literal — no expressions, no types, no signatures.

Run these in order on the macOS host:

1. `cargo test -p keeper-sync --lib -- exclude:: profile::` — the story gate.
   **Already green here: 67 passed, EXIT=0.**
2. `cargo test -p keeper-sync --lib` — **already green here: 665 passed, 0 failed.**
3. `cargo test -p keeper-syncd` — the shared mapping's other caller.
   **Already green here: 72 + 6 + 12 passed.**
4. `cargo test -p keeper-core --lib notes::default_spaces` — the comment edit compiles.
   **Already green here: 37 passed.**
5. `cargo clippy --all-targets -- -D warnings` — **not run** (Main runs the linter once).
   The one thing to look for is `folder.rs`'s `TierGuard(#[allow(dead_code)] MutexGuard)`.
6. `cargo build -p keeper` / `cargo test -p keeper` — **cannot run on Linux.** Confirms the
   three doc comments and the log-line continuation in `notes_vault.rs` compile.
7. `bun run check:core-sync-free` — asserts `keeper-core` still names neither `gix` nor
   `keeper-sync`. I added no dependency in that direction (the new `toml` dep is on
   `keeper-sync`, which `keeper-syncd` already resolved, so the lockfile gains nothing), but
   this is the check that would catch it if I were wrong.
8. **The end-to-end behaviour nobody can test on this box**, once W2Main lands the
   `install_folder_tier` call: put `[folder]\ntags = ["work"]` in `<a synced folder>/.keeper/keeper.toml`,
   confirm the next commit from that folder carries the trailer; delete the file, confirm the
   trailer goes away rather than persisting (that is mutation 6's property, live); put
   `[settings]` in a non-main folder's file and confirm the settings surface names the file,
   the key and the rule.

## Files changed

| file | change |
| --- | --- |
| `keeper-sync/src/exclude.rs` | `is_exempt_config_file` + one pre-check; `is_excluded_directory` stops delegating; `verdict` routes through both booleans; 4 tests |
| `keeper-sync/src/profile.rs` → `profile/mod.rs` | `git mv`; module doc; `accepted_profile_keys`, `canonical_key`, `canonical_profile_fields`; 3 tests |
| `keeper-sync/src/profile/folder.rs` | **new** — the whole tier, 19 tests |
| `keeper-sync/src/db.rs` | `list_profiles`/`get_profile` return the profile in force; `upsert_profile` strips through `as_stored`; new private `stored_profile` |
| `keeper-sync/Cargo.toml` | `toml = { workspace = true }` |
| `keeper-syncd/src/config.rs` | calls the moved mapping; its `canonical_key` unit test moved with the function |
| `keeper/src/notes_vault.rs` | `rebuild` doc corrected; cache-discard log line corrected; walk doc records why `.keeper/` stays skipped |
| `keeper-core/src/notes/default_spaces.rs` | the AD-79 rationale records AD-100 and why the conclusion holds |
