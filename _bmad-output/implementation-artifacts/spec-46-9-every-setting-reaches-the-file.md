# Spec 46.9 — Every setting reaches the file

story: 46.9
epic: 46 (the file is the setting)
binds: AD-98, AD-99, AD-101
agent: W2Coverage
date: 2026-08-10

## What shipped

`src-tauri/crates/keeper-core/src/config/keys.rs` — the settings-key registry as
data, plus the coverage tests that keep it honest, plus the generator for
`docs/settings-keys.md`.

`docs/settings-keys.md` — generated, pinned.

One defect found and fixed in the seam with story 46.10 (see **The defect this
story found**, below); the fix itself landed in `config/mod.rs` by its owner.

Nothing else was touched. `archive/mod.rs` carried a scratch mutation for eight
minutes and is byte-identical to `HEAD` again (`git diff` on it is empty).

## The shape of the thing

31 entries in `KEYS`: 28 exact keys and 3 key **families** (`notes.read.`,
`notes.capture_draft.`, `notes.capture_placement.` — prefixes, not keys, or the
coverage test would report one unclassified key per note in the vault).

Each entry carries scope, settability with a reason, value shape, stored
default, a one-line summary and a TOML example. The reason is the load-bearing
field: a key that must not be file-settable is *classified as such, with the
reason in the data*, which is the more valuable half of coverage and the half
that a "just make everything settable" fix would have destroyed.

| bucket | count | who may set it |
| --- | --- | --- |
| `Settable::AnyLayer` | 18 | any layer file whose tier may carry `[settings]` |
| `Settable::MachineFileOnly(why)` | 7 | only `keeper.<host>.toml`, refused by name elsewhere |
| `Settable::Never(why)` | 6 | no file, at any tier |

### The seven machine-local keys and why

Three groups, one rule each, so a reader holds three facts and not seven:

- **absolute paths** — `recording.destination_dir`, `sync.git_path`.
  `/Volumes/merope/…` and `/opt/homebrew/bin/git` do not exist on the other
  machine.
- **OS-global accelerators** — `hotkey.global`, `hotkey.recording`,
  `hotkey.capture`. Registered with this machine's window server; two machines
  cannot agree on one that is free on both.
- **`sync.db` row ids** — `recording.destination_profile_id`,
  `notes.active_vault`. A profile id is a locally minted ULID
  (`sync_ipc.rs:686`, `Ulid::new()` unless the caller supplies one), so the same
  folder is a different id on the other machine.

The third group is a judgement call the story did not name, and it is the one to
overturn first if anybody disagrees: it is one `Settable::` value per key and
the reason string is right there. It was classified restrictively because the
failure is silent — a shared file naming a profile id that does not exist here
degrades to "no recordings profile", which looks exactly like "no choice made".

All seven are accepted from `keeper.<host>.toml`. That file is precisely what
they are for; refusing them everywhere would make the per-machine tier useless
for the only keys that need it.

### `sdk_encryption`

`Settable::Never`, not `MachineFileOnly`. The row records whether a per-account
passphrase exists in *this* machine's Keychain; a file cannot create that item,
so a file that flips it describes a store state that is not true. Flipping it is
a re-key of the local store, not a toggle.

### Session/latch state: in the registry, as "never settable"

The story asked for a decision and a reason. **They are in the registry**, as
`Scope::SessionState` + `Settable::Never(why)`:
`notes.capture_draft.*`, `notes.capture_placement.*`, `notes.read.*`,
`ui.recovered_sessions_acknowledged`, `ui.ios_sync_disclosure_shown`.

Why in rather than out: the coverage test cannot distinguish "state,
deliberately not a preference" from "somebody forgot to classify it" if the
answer to both is *absent from the table*. Excluding them is exactly how they
would come to look the same, and the day somebody adds `notes.capture_theme.` as
a real preference is the day that distinction has to hold. Being in the registry
also means they appear in the docs, in their own section, with the sentence that
says why a file entry would not survive — which is the question a person asks
once and then stops asking.

`ui.ios_sync_disclosure_shown` was not on the story's list and is the same
thing: a one-time latch, pre-setting which would suppress a card nobody saw.

### There is no folder scope, and that is a finding

`Scope` has three variants, not the four the story's phrasing implies. **No
settings-table key is about one folder.** Everything a folder decides about
itself lives in its `SyncProfile` fields, which is what `[folder]` carries. That
is not an omission — it is the justification for the contract's rule that a
`[settings]` table outside the main folder is a fault: it could only ever be a
folder reaching for a key that is not about it. Written into the module doc and
the generated page so the next reader does not have to re-derive it.

## The coverage test

`keeper-syncd`'s config derives its accepted-key set from the type at runtime
(`config.rs:6-9`), which is the trick to copy and which is **not available
here**: these keys are `const` strings inside function bodies, and three are
built with `format!`. There is no type to reflect on. So the honest form is the
one this repo already uses twice in TypeScript
(`src/test/command-registration.test.ts`,
`src/test/file-scheme-registration.test.ts`): read the source.

`mod coverage` walks `src/` of `keeper-core`, `keeper`, `keeper-sync` and
`keeper-syncd`, splits each file's production half off its test module on the
sole `#[cfg(test)]\nmod tests` opener (the same marker `send.rs`'s existing
source scan uses, and asserted to occur at most once per file so the split can
never silently truncate), finds every `get_setting` / `set_setting` call that is
not the definition, and resolves argument two:

| argument form | resolution | example |
| --- | --- | --- |
| `"literal"` | the key | — |
| `UPPER_CONST` | `const NAME: &str = "…"` in the same file | `RECORDING_FPS_KEY` |
| `&some_fn(x)` | the `format!` prefix in `fn some_fn`, with a leading `{CONST}` resolved and everything from the first remaining `{` dropped | `&notes_read_mark_key(id)` → `notes.read.` |
| anything else | **dynamic** — must be declared in `DYNAMIC_SITES` with a reason | `set_setting(dir, key, &text)` |

`keeper-sync` and `keeper-syncd` hold no call site today. They are scanned
anyway: the day one grows one is exactly the day this needs to notice.

Four tests, and the fourth exists because the other three are worthless without
it:

1. `every_settings_key_in_the_sources_is_classified` — the story. Fails naming
   the key and every `file:line` that uses it.
2. `every_classified_key_is_still_used` — the other direction. A registry that
   only grows becomes a list of settings that do nothing, and a docs table of
   those is worse than no table.
3. `every_dynamic_call_site_is_declared_with_a_reason` — set equality, not
   containment. A *second* dynamic site needs a second declared reason; adding
   one without writing the reason fails by name. There is exactly one today:
   `registry.rs::import_config_file`, which writes whatever keys `config.json`
   holds and is the thing AD-98 replaces.
4. `the_scanner_actually_scans` — a source-reading test that reads no source
   passes silently, which is the exact defect it exists to catch. Three anchors:
   a floor of 60 call sites, `recording.fps` (only resolves if `const` lookup
   works) and `notes.read.` (only resolves if the `format!` walk works).

## The defect this story found

Story 46.10's loader mapped every TOML boolean to `"1"`/`"0"`, shape-blind.
Three keys predate that convention:

| key | stored spelling | comparison in the getter |
| --- | --- | --- |
| `honor_remote_deletions` | `"on"` / `"off"` | `archive/mod.rs:432` `== Some("on")` |
| `favorites_collapsed` | `"true"` / `"false"` | `keeper/src/ipc.rs:10483` `== Some("true")` |
| `sdk_encryption` | `"on"` / `"off"` | `auth.rs:422` (refused from files anyway) |

So `honor_remote_deletions = true` in a layer file resolved to the override
`"1"`, and the getter read it as **false**. No fault, no warning: the setting
reached the file and arrived inverted. That is a half-kept promise of exactly
the shape this story exists to catch, and it was invisible to every test on
either side of the seam — the loader's tests asserted the *convention*, and the
getter's tests never saw a layer.

Fixed by making the value shape authoritative: `Shape::coerce(key, toml_value)`
is now the single translation from a TOML value to the stored string, and the
loader calls it instead of guessing. `registry::scalar_setting_text` remains the
definition for the legacy `config.json` path only — which keeps the old bug,
deliberately, because changing it would move settings on machines already
running one. The divergence between the two paths is now pinned by a test on the
loader's side rather than being an accident.

Pinned here by `a_layer_file_lands_each_boolean_in_the_spelling_its_own_getter_reads`,
which drives the real `parse_layer_file` — no mock — and asserts all four
spellings land: `on`, `true`, `1`, `60`.

## I/O matrix

`layer_may_set(key, machine_scoped)`

| key | `machine_scoped = false` | `machine_scoped = true` |
| --- | --- | --- |
| `recording.fps` | `Ok` | `Ok` |
| `debug.mode` | `Ok` | `Ok` |
| `sync.git_path` | `Err(MachineLocal)` | `Ok` |
| `hotkey.global` | `Err(MachineLocal)` | `Ok` |
| `notes.active_vault` | `Err(MachineLocal)` | `Ok` |
| `recording.destination_profile_id` | `Err(MachineLocal)` | `Ok` |
| `sdk_encryption` | `Err(NotAPreference)` | `Err(NotAPreference)` |
| `ui.recovered_sessions_acknowledged` | `Err(NotAPreference)` | `Err(NotAPreference)` |
| `notes.read.01ABC` | `Err(NotAPreference)` | `Err(NotAPreference)` |
| `notes.read.` (bare prefix) | `Err(Unknown)` | `Err(Unknown)` |
| `notes.readable` | `Err(Unknown)` | `Err(Unknown)` |
| `recordng.fps` (typo) | `Err(Unknown)` | `Err(Unknown)` |

Every `Err` renders a sentence that names the key. `MachineLocal` also names
`keeper.<host>.toml` as where it belongs instead; `Unknown` names
`docs/settings-keys.md`.

`Shape::coerce(key, value)`

| shape | `true` | `"1"` | `30` | `"av1"` | `[1]` |
| --- | --- | --- | --- | --- | --- |
| `Flag01` | `"1"` | `"1"` | Err | Err | Err |
| `FlagOnOff` | `"on"` | Err | Err | Err | Err |
| `FlagTrueFalse` | `"true"` | Err | Err | Err | Err |
| `Int{1,600}` | Err | `"1"` | `"30"` | Err | Err |
| `Choice[h264,hevc]` | Err | Err | Err | Err | Err |
| `Choice[10,15,30,60]` | Err | Err | `"30"` | Err | Err |
| `Text` / `AbsolutePath` / `Accelerator` / `Json` | Err | `"1"` | Err | `"av1"` | Err |

An out-of-range integer is an `Err` naming the range, not a clamp. The getter
still clamps a row that rotted in the database; a number a person typed into a
file they can see is worth a sentence back instead.

## Edge cases

- **Family prefix vs. its own name.** `notes.read.` matches `notes.read.01ABC`
  and *not* itself: the bare prefix is not a key, and matching it would let a
  file write a row nothing reads. Asserted, along with `notes.readable` not
  being a child.
- **A family prefix that lost its dot** would match `notes.readable`. Asserted
  structurally: every `family: true` spec must end in `.`.
- **Duplicate keys** in the table would make `spec()` return whichever came
  first. Asserted unique.
- **Exact beats family.** `spec()` checks exact matches across the whole table
  before considering any prefix, so adding `notes.read.summary` as a real key
  later does not get swallowed by `notes.read.`.
- **Two `#[cfg(test)]\nmod tests` openers in one file** would make the scanner
  truncate at the first and hide every call site after it. Asserted per file.
- **A file with no test module** is scanned whole; harmless, since the marker
  simply does not match.
- **`get_setting` as a suffix** (a hypothetical `fn cached_get_setting`) is
  excluded by a word-boundary check on the preceding character, and the two
  definitions are excluded by the `fn ` prefix.
- **Nested parens and commas inside string literals** in an argument list —
  handled by a depth-and-string-aware splitter rather than `split(',')`.
- **A doc example that its own key would reject** — every settable spec's
  `example` is parsed as TOML and pushed through `coerce`. Every `Never` spec's
  example must be *empty*, because an example on a key no file may set is an
  invitation to write one.

## Mutation table

Every claim below was proved by removing the thing and watching the named test
fail, then restoring it and watching it pass.

| # | mutation | test that failed | how it failed | restored, verified by |
| --- | --- | --- | --- | --- |
| 1 | added `const SCRATCH_UNCLASSIFIED_SETTING: &str = "archive.scratch_probe"` and a getter using it to production `archive/mod.rs` | `coverage::every_settings_key_in_the_sources_is_classified` | `these settings keys are used but not classified … archive.scratch_probe (keeper-core/src/archive/mod.rs:447)` — by name, with file and line | `git diff -- src-tauri/crates/keeper-core/src/archive/mod.rs` empty; file absent from `git status` |
| 2 | changed one `summary` string in `KEYS` | `docs::the_checked_in_table_matches_the_registry` | `docs/settings-keys.md is stale. Regenerate it with …` | reverted the string, regenerated, suite green |
| 3 | (found, not injected) shape-blind boolean coercion in the loader | `a_layer_file_lands_each_boolean_in_the_spelling_its_own_getter_reads` | `left: "1"  right: "on"` | fix landed by the loader's owner; test green |

Mutation 1 is the story's stated acceptance and it fails by name, with the
call site's file and line, which is the difference between a test that tells you
what to do and a test that tells you something is wrong.

Mutation 3 is the one that justifies the story: it was not injected. It was
sitting in code that had just been written and tested by two people, and it
inverted a user-facing setting.

## Deliberately NOT done

- **`config.json`'s importer keeps the shape-blind bug.** It is the layer AD-98
  replaces, and changing how it maps a boolean would move a setting on a machine
  that is already running one. The divergence is pinned by a test rather than
  left to be rediscovered.
- **No new `Scope::Folder` variant.** Nothing would carry it (see above), and an
  unused variant that documents an idea is worse than a paragraph that does.
- **`Shape::coerce` is not wired into `set_setting`.** The registry's setters
  clamp and normalise already, on both sides, and putting a second validator in
  front of them would give two answers to "what happens to a bad value" in two
  crates. `coerce` is the *file* gate; the getters remain the *row* gate.
- **The `Never` keys are not removed from the settings table.** Whether
  `notes.read.*` belongs in a k/v table at all is a real question and not this
  story's; classifying it is.
- **No migration of `~/.keeper/keeper.toml` from any existing file.** Nobody has
  one yet.
- **The docs page is not linked from `docs/index` or the settings pane.** The
  pane's file-controlled surface is story 46.7's, and it names the file path
  directly, which is the more useful pointer.

## What I could not verify here, and why

The Linux box cannot build the `keeper` shell crate (AD-55, AD-56), so
`keeper/src/ipc.rs` was **read and scanned but never compiled** by me. The
scanner reads it as text, which is the whole reason a source-reading test is the
right instrument for this: `favorites_collapsed` is classified from a file this
machine cannot compile, and the classification is still checked on every run.

Everything in `keeper-core` — the registry, the coercion, all four coverage
tests, the docs pin — compiles and runs here.

**The scan reads live files, not the compiled crate.** That is what lets it
cover the shell crate this machine cannot build, and it is also why a red can
belong to somebody else: while the wave was running, four `config::` runs went
red at counts that moved between runs (2, 5, 2 failures) and then went green and
stayed green for six consecutive runs at 42/42 as peers' edits landed. A
deliberately-broken source in `keeper-core`, `keeper`, `keeper-sync` or
`keeper-syncd` — a mutation sweep, a half-saved file — can fail this test for a
reason that is not in it. Before chasing one, check `git diff` on the file the
failure names.

Ordered gate checks, on a machine that can build the shell:

1. `cargo test -p keeper-core --lib config::` — **run here, EXIT=0, 40 passed,
   1 ignored (the generator).** Re-run for confirmation, not discovery.
2. `cargo check -p keeper --all-targets` — the shell crate compiles. Nothing in
   this story changes a signature the shell calls, so a failure here is
   somebody else's.
3. `cargo clippy --workspace --all-targets -- -D warnings` — the workspace lint
   gate. Not run here (Main runs it once, per the wave contract).
4. `cargo test -p keeper-core` — the full core suite including integration
   tests. `tests/cold_start_perf.rs` writes `honor_remote_deletions` and `theme`
   directly through `set_setting`; both are in `tests/`, not `src/`, so the
   scanner does not see them and they cannot fail the coverage test. Worth one
   look that they still pass.
5. **On-device, once the app runs:** write
   `~/.keeper/keeper.toml` containing
   ```toml
   [settings]
   "honor_remote_deletions" = true
   "recording.fps" = 60
   ```
   and confirm in the app that Honor Remote Deletions reads **on** and the
   recording frame rate reads **60**. The first of those is mutation 3's fix on
   real hardware and is the only check in this list that a green suite does not
   already imply.
6. **On-device, the refusal path:** put `"sync.git_path" = "/usr/bin/git"` in
   the *shared* `~/.keeper/keeper.toml` and confirm the settings pane shows a
   named fault saying it belongs in `keeper.<host>.toml`, rather than silently
   ignoring it. The refusal is unit-tested; that it *reaches a person* is story
   46.7's surface and can only be seen running.
