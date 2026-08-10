# Spec 46.7 — The main sync folder holds the shared settings

story: 46.7
epic: 46 — the file is the setting
binds: AD-98 (layer stack, and the surface that makes it honest), AD-99 (layer order),
AD-101 (two-phase load), FR-200–FR-203
agent: W2Main
status: implemented, gated (the `keeper` shell does not compile on Linux)

## What shipped

Four things, in the order a boot meets them.

1. **Phase one** (`keeper/src/lib.rs`, `setup()` ~:245-330). The TOML layer stack is
   loaded and installed *before* `debug_log::init`, so a hand-edited `debug.mode`
   applies to the boot that goes wrong rather than the one after it. The folder tier
   is installed in the same block. `import_config_file` is unchanged and now sits at
   the bottom of the stack. Every outcome — faults, the layered key list, the
   `config.json` import — is logged after `init`, so the subscriber exists to carry it.
2. **The main folder is named in the user-global file** and read straight off the disk
   in phase one. Nothing about that read needs `sync.db`, which is what dissolves
   AD-101's cycle.
3. **Phase two** (`lib.rs` ~:545-585, right after `sync::start_supervisor`). The one
   fact phase one could not check: that `mainSyncFolder` names a folder keeper
   actually syncs. A path that exists but is not a sync folder disables the whole
   shared tier while looking exactly like a configuration that works, so it is
   reported at `error` in the log and pushed into the fault list the Settings pane
   renders.
4. **The surface.** `config_layers` (`keeper/src/ipc.rs` :1724-1769) projects the
   stack through a new `ConfigLayersVm` (`keeper-core/src/vm.rs`), and Settings
   grows a `ConfigSourceSection` plus a `FileControlled` badge on nine controls
   (eight distinct keys — `notify.dock_badge_mode` is worn by both the phone
   badge group and the desktop one) that now say so when a file decides their
   value.

## The ordering contract, restated with the reason

The comment at the old `lib.rs:222-227` said "order matters" and gave one instance.
The instance generalises, and the generalisation is the whole of AD-101:

> Everything a person edited by hand must be in force before anything reads a
> setting — and `setup()` reads a lot of settings.

| Read | Where | Key(s) | In force from a layer? |
| --- | --- | --- | --- |
| `debug_log::init` | lib.rs, phase one | `debug.mode` | yes — installed one statement earlier |
| `hotkey::install{,_recording,_capture}` | lib.rs ~:355-375 | `hotkey.global`, `hotkey.recording`, `hotkey.capture` | yes |
| tray presence | lib.rs ~:390-405 | `system.menu_bar_presence` | yes |
| `Engine::open` ← `configured_git_path` | sync.rs:288, via `start_supervisor` | `sync.git_path` | yes |

All four are read *inside* `setup`, after phase one. This is why phase one is where
the stack is installed and not one line later.

**ScoutConfig's trap, and why it does not bite.** The concern was that a *phase-two*
layer could set a key already consumed between :227 and :428, producing a silent
next-boot no-op. It cannot, and the reason is structural rather than lucky:

- The four phase-one files (`~/.keeper/` ×2, `<main>/.keeper/` ×2) are read before
  any of those consumers. `sync.git_path` reaches `Engine::open` from
  `~/.keeper/keeper.<host>.toml` correctly.
- A **non-main folder may not carry `[settings]` at all** — `LayerTier::may_set_settings`
  refuses it as a named `SettingsInNonMainFolder` fault. So the only tier installed
  after phase one carries no settings keys, only `[folder]`, which the sync engine
  consumes at profile-read time and never through the settings overlay.

There is therefore no settings key that arrives too late to matter, and no
silent no-op to document.

## Why the folder tier is installed in phase one

AD-101 predicted phase two for it: three of the five layers are keyed on sync-folder
paths, which live in `sync.db`. W2Folder's tier does not need those paths handed to
it — `keeper-sync` applies each profile's own `<local_path>/.keeper/keeper*.toml`
*inside* `db::list_profiles`/`db::get_profile`, at the moment it already holds the
path. Installing it carries only the host label and which folder is main, both known
before any database opens. The call therefore sits beside `config::install`, with the
reason in a comment so the next reader does not "fix" it back to phase two.

## `config.json` is now the bottom of the stack, and is otherwise untouched

`import_config_file` still writes every key it holds into the `settings` rows, at every
boot, destructively over the table. What changed is that the table is no longer the
last word: `registry::get_setting` consults the overlay first (Story 46.6), so a TOML
layer setting the same key wins at read time. Ordered after `config::install` purely so
the block reads in precedence order — the import reads no setting itself, so the order
between those two statements is cosmetic and the comment says so.

`docs/recording.md`'s "config.json — file-based overrides" section still describes
`config.json` accurately (imported verbatim, file wins over the table, malformed is
logged and skipped) but does not yet mention that a `keeper.toml` layer outranks it.
That sentence belongs to Story 46.6's doc surface, which is where
`docs/settings-keys.md` was generated; **flagged, not edited here** — see "deliberately
NOT done".

## I/O matrix — phase one

| `HOME` | `~/.keeper/keeper.toml` | `mainSyncFolder` names | Result |
| --- | --- | --- | --- |
| unset | — | — | no user layer, no main folder; `warn` "HOME is unset, so no ~/.keeper/*.toml was read this boot"; boot continues |
| set | absent | — | empty stack, no fault. The normal install. |
| set | unreadable | — | `Unreadable` fault, layer skipped whole, boot continues |
| set | malformed TOML | — | `Malformed` fault carrying `toml`'s caret snippet; layer skipped whole |
| set | present | nothing | user layers only; `main_folder = None`; the two `<main>` files are not looked for |
| set | present | a path that does not exist | `MainFolderMissing`; `main_folder` kept, so the UI shows the typo next to the fault |
| set | present | a file, not a directory | `MainFolderNotADirectory`; same |
| set | present | a directory with no `.keeper/` | no fault, no main layers. An empty `.keeper/` and a missing one are the same claim: nothing is set. |
| set | present | a directory with `.keeper/keeper.toml` | main-shared and main-machine layers applied over the user ones |

**`HOME` unset does not fall back to `temp_dir()`,** which is what `debug_log::app_log_path`
does. A log in `/tmp` is a lost log; *reading settings* out of a world-writable directory
would let any local process choose this app's global hotkeys and its `git` binary path.

## I/O matrix — phase two

Runs only on desktop, only when `mainSyncFolder` named something.

| Engine | Profile list | `mainSyncFolder` | Result |
| --- | --- | --- | --- |
| not open (no usable `git`) | — | any | silent. The `git` report already explains this machine far better than a fault blaming the folder would. |
| open | error | any | `warn` only, no fault. An unreadable profile list is the engine's problem; "your main folder is wrong" would send someone to edit a file that is fine. |
| open | ok | equals some profile's `local_path` | nothing. Includes **disabled** profiles: a paused folder is still a sync folder whose files phase one really did read. |
| open | ok | canonicalises to some profile's `local_path` | nothing — see below |
| open | ok | matches nothing | `MainFolderNotAProfile` at `error` in the log **and** pushed onto the fault list |

**Why `same_folder` canonicalises.** A profile's stored `local_path` and the path the
owner typed routinely differ by a symlink macOS inserted (`/tmp` is `/private/tmp`; an
external volume is reachable through more than one mount point). Component equality is
tried first and is free; `canonicalize` is the fallback, on both sides, and a failure on
either side falls back to "not the same" rather than claiming a match. At most two
`stat`-class syscalls on a path already known to exist — not a walk, because nothing in
`setup` may touch a tree before the window is up.

## The surface

`ConfigLayersVm::new(overrides, faults, main_folder)` is **pure** and lives in
`keeper-core`, which compiles on Linux. Every sentence a user reads about their
settings files is composed there and asserted by twelve tests, because the shell that
calls it cannot be compiled on the machine this was written on.

- `ConfigSourceSection` renders **always**, including the install where nothing is
  overridden. The `SyncGitRow` argument: a section that appeared only once a
  `keeper.toml` existed would be invisible to everyone who has not discovered that
  files are possible.
- **Faults render above overrides.** A key that came from the wrong file is a
  curiosity; a file that did not load at all is the thing someone must go and fix.
- `FileControlled` **says so and does not disable**. AD-98 asks a control to say it
  would be overridden. Disabling looks tidier and is wrong: `set_setting` still writes
  the table, the table is still the fallback, and the value set here is exactly what
  takes effect the moment the file stops setting the key. A disabled control would
  make the honest fallback unreachable and turn a temporary override into a permanent
  one. There is a test for exactly this.
- Faults reach the UI through `LayerFault::summary()` (one line) and the boot log
  through `Display` (multi-line, keeping `toml`'s caret diagram, which is the only
  thing that locates a typo). Getting this backwards is a mutation in the table below.

Controls marked: `honor_remote_deletions`, `notify.previews_enabled`,
`notify.dock_badge_mode` (both the phone "App icon badge" and the desktop "Dock badge"
group), `system.menu_bar_presence`, `incognito.global`, `undo_send.window`,
`hotkey.global`, `hotkey.recording`.

## Edge cases and the decisions behind them

- **A read that fails keeps the last good list** and adds a caption admitting it may be
  behind. Wiping to `null` would render as "nothing overrides anything", which is the
  same silent substitution the story removes. Before the *first* read resolves the
  section renders nothing at all — "no settings file" is a claim the frontend has not
  yet earned.
- **The section re-reads on every open**, not once per lifetime. The stack itself is
  installed once, but `faults()` is not static: phase two pushes into it after the
  engine opens, and the folder tier's faults are a live snapshot that changes as
  profiles are read.
- **`config_layers` takes no `State`.** The stack is process-global and was installed
  before `AppState` held anything worth reading; taking the state would imply a
  dependency that does not exist.
- **`config_layers` is registered in the shared macro body**, not spliced in as a
  desktop `$extra`. `keeper_core::config` has no desktop gate, so a phone can answer
  the question honestly (with an empty stack) instead of rejecting the call.
- **Folder faults are folded in at the shell**, because AD-40 keeps `keeper-sync` free
  of `keeper-core` and vice versa (`bun run check:core-sync-free` asserts both edges).
  The shell is the only crate that can see a `FolderFault` and a `LayerFault` at once.
  `ConfigFaultVm::folder` takes `&Path` and `String` rather than two `String`s
  precisely because its one call site is in a file no CI on Linux compiles — two
  different types cannot be swapped.
- **Phase two names the fault and unwinds nothing.** The keys the shared layer did
  apply stay applied; re-seeding the debug gate, three hotkeys and the tray from a
  different stack halfway through boot is worse than a folder honestly reported wrong.
- **A `mainSyncFolder` that is a real directory but not yet a sync folder** is a fault,
  not a refusal. The person may be about to add it. The fault says which of the two
  fixes to make.

## Mutation table

Every fix below was removed, the named test observed failing, then restored and the
restoration verified — by `cmp` against a pre-mutation copy for the files `git diff`
can see, and by `md5sum -c` for the ones it cannot (three of the five frontend files
are new and therefore untracked).

| # | Mutation | Test that failed | Restored |
| --- | --- | --- | --- |
| 1 | drop `ipc::config_layers` from `generate_handler!` | `command-registration.test.ts` › "are each registered on the builder" — named `config_layers` | md5 match |
| 2 | move `ipc::config_layers` from the shared body to the **desktop** `$extra` list | `config-command-platform.test.ts` › "registers config_layers…". **`command-registration.test.ts` still passed** — which is the gap this test exists for | `cmp` IDENTICAL |
| 3 | `FileControlled` always returns `null` | `config-source-section.test.tsx` × 2 ("marks a control…", "…without disabling…") | md5 match |
| 4 | stop rendering the faults list | `config-source-section.test.tsx` › "shows a fault, which is the loud half" | md5 match |
| 5 | `applyFailure` also wipes `layers` | `config-source-section.test.tsx` › "keeps the last good list on screen…" | md5 match |
| 6 | `MainMachine`'s phrase says "for every machine" | `vm.rs` › `every_tier_gets_a_distinct_sentence_naming_its_reach` | `cmp` IDENTICAL |
| 7 | plural summary used for a single override | `vm.rs` › `an_override_names_the_key_the_file_and_how_far_the_file_reaches` | `cmp` IDENTICAL |
| 8 | project a fault with `Display` instead of `summary()` | `vm.rs` › `a_multi_line_parser_fault_reaches_the_surface_as_one_line` | `cmp` IDENTICAL |
| 9 | `with_folder_faults` appends without recomputing the summary | `vm.rs` › `folder_faults_join_the_same_list_and_are_counted_with_the_rest` | `cmp` IDENTICAL |
| 10 | typo one `settingKey` (`notify.preview_enabled`) | `file-controlled-keys.test.ts` › "each name a key a settings file may actually set". Every other frontend test still passed | `cmp` IDENTICAL |

Mutation 8's first attempt produced a *compile* failure rather than a test failure,
from a concurrent edit adding a field to `capture::Placement` in another agent's story.
It was re-run in a single shell statement (mutate → test → restore → `cmp`) once the
tree compiled again, and the second run is the one recorded. A peer also reported the
mutated line as a bug while it was in flight — recorded here because it is the clearest
possible independent evidence that the test bites.

## Two guards nobody asked for, and why they earn their place

Both are the shape Story 45.19 generalised: *does this thing name something, and does
anything check the thing it names exists?*

- **`src/test/config-command-platform.test.ts`** — `keeper_with_commands!` is one macro
  invoked twice. An entry in the wrong half compiles, passes the existing registration
  test (whose scan is deliberately not scoped to the literal), and works on every
  machine any of this is developed on. It fails on iOS only, at runtime, and the
  Settings surface then renders no section and no markers. Mutation 2 shows the
  existing test cannot see it.
- **`src/test/file-controlled-keys.test.ts`** — `FileControlled` renders `null` when it
  finds no override, which is correct for a key no file sets and *indistinguishable*
  from a key that does not exist. A typo produces a marker that never appears, forever,
  with a green tree. It compares against `docs/settings-keys.md`, which is generated
  from `keys::KEYS` and pinned by W2Coverage's test — so it reads the real registry
  without needing the Rust that owns it. Deliberately compared against the two
  *settable* sections only: a marker on a key from "Keys no file may set" would be a
  promise keeper cannot keep.

## Deliberately NOT done

- **No TOML editor, and no write path of any kind.** The premise of the epic is that
  the file is the setting and the owner edits the file. A surface that wrote the file
  back would reintroduce the two-writers problem the layer stack exists to end. Every
  path in the section is shown so it can be opened somewhere else.
- **No "reveal this file" button.** It would be the right affordance and it needs a
  desktop capability gate, an iOS story, and a decision about revealing a file that
  does not exist yet. Out of scope for the smallest honest form the story asked for.
- **The section does not name the path a user *could* create** (`~/.keeper/keeper.toml`)
  in its empty state. It would be useful; it is also one step past "smallest honest
  form", and it needs `HOME` plumbed into the command for a sentence.
- **No marker on `launch-at-login`.** The autostart plugin owns that state
  authoritatively (AD-25); it is not a settings-table key and no file can set it.
- **Phase two does not unwind the shared layer** when the folder turns out not to be a
  sync folder. Reasoned above.
- **`docs/recording.md` not updated.** Its `config.json` section is still accurate but
  no longer complete now that a `keeper.toml` layer outranks `config.json`. That
  sentence belongs with Story 46.6's documentation surface
  (`docs/settings-keys.md`), not to a startup-wiring story — bundling it means a
  doc change that cannot be reverted without also reverting boot wiring.
  **Handed to Main / W2Layers.**
- **No test that the *order* of the four phase-one statements is preserved.** It cannot
  be asserted from Linux and it cannot be asserted in Rust without executing `setup`,
  which needs a Tauri app. Gate check G1 below is the assertion.

## What I could not verify here, and why — ordered gate checks

**The `keeper` shell crate does not build on Linux (AD-55/AD-56).** Everything in
`keeper/src/lib.rs` and `keeper/src/ipc.rs` below was written, read twice, and
type-reasoned against the real signatures in `keeper-core/src/config/mod.rs` and
`keeper-sync/src/profile/folder.rs` — both of which I read after they landed — but
**none of it has been compiled**. What *was* compiled and tested on Linux: all of
`keeper-core/src/vm.rs` (12 tests), the four generated TS bindings, and every frontend
file (17 tests across three files, three consecutive runs).

Run these in order on macOS. G1 is the one that matters.

**G1 — a hand-edited `debug.mode` applies to *this* boot.** This is the ordering
contract the whole phase-one block exists to keep, and the only check that proves it.

```sh
mkdir -p ~/.keeper && printf '[settings]\n"debug.mode" = true\n' > ~/.keeper/keeper.toml
rm -f ~/Library/Logs/keeper/keeper.log
# launch keeper, then:
head -5 ~/Library/Logs/keeper/keeper.log
```
Expect the log to exist and its first lines to include
`debug mode: on-disk logging enabled` **and** `settings: these are set by a file`
with `keys=["debug.mode"]`. A log that exists only from the *second* launch means the
install landed after `debug_log::init` and the ordering broke.

**G2 — the surface shows it.** Settings → "Where your settings come from" lists
`debug.mode`, "your settings file, for every machine and folder", and the path.

**G3 — a control says so.** Add `"notify.previews_enabled" = false` to the same file,
relaunch. Settings → Notifications → "Show message previews" carries a **Set by a
file** badge whose tooltip names the file; the switch is **not** disabled; toggling it
is accepted and the badge stays.

**G4 — a malformed file is loud and non-fatal.** `printf 'this is not toml\n' >
~/.keeper/keeper.toml`, relaunch. The app starts. The log carries
`settings file: this layer was skipped` with `toml`'s caret snippet. The section shows
one fault, one line, with no caret snippet in it.

**G5 — the main folder.** Put `mainSyncFolder = "<a folder you sync>"` in
`~/.keeper/keeper.toml` and `[settings]\n"recording.fps" = 15` in
`<that folder>/.keeper/keeper.toml`. Relaunch. The section shows `recording.fps` from
"the shared settings file in …, for every machine", and `Shared settings folder` names
the path.

**G6 — the loud one.** Change `mainSyncFolder` to a real directory that is *not* a sync
folder (`mainSyncFolder = "/tmp"`). Relaunch. The log carries
`settings: the shared layer is not in force` at `error`, and the section shows a
`mainFolderNotAProfile` fault naming `/tmp` **and still shows `/tmp` as the designated
folder**. Then point it at a *disabled* sync profile's folder and confirm **no** fault:
a paused folder is still a sync folder.

**G7 — iOS.** Build the iOS shell. Open Settings. The section renders its empty-state
line rather than a blank space, which is the proof `config_layers` reached the
non-desktop handler list. (`config-command-platform.test.ts` asserts the registration;
only a device proves the call.)

**G8 — the compile gates I could not run.**
```sh
cargo fmt --manifest-path src-tauri/Cargo.toml --all --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo nextest run --manifest-path src-tauri/Cargo.toml
```
Two specific things to watch, both `#[cfg]`-shaped and therefore only visible on one
target each:
- `keeper/src/ipc.rs`: `ConfigFaultVm` is **not** imported, it is written out in full
  inside the `#[cfg(desktop)]` arm. If clippy still reports an unused import on the
  iOS build, something re-added it.
- `keeper/src/lib.rs`: `same_folder` is `#[cfg(desktop)]` and used only from the
  `#[cfg(desktop)]` phase-two block; `host` is moved into `FolderTier::new` on desktop
  and merely borrowed on iOS.

## Verified here, on Linux

```
cargo test -p keeper-core --lib vm::tests::config_layers   12 passed
cargo test -p keeper-core --lib config::                   42 passed, 1 ignored
cargo test -p keeper-core --lib config::keys               16 passed, 1 ignored
cargo test -p keeper-core --lib export_bindings           202 passed  (4 new Config*.ts)
bun run typecheck                                          clean
bun run test src/components/settings/                     161 passed (10 files)
bun run test src/test/command-registration.test.ts          3 passed
bun run test <the three files above>  × 3 consecutive runs 17 passed each
```

## Files changed

| File | Change |
| --- | --- |
| `keeper-core/src/vm.rs` | new `ConfigTierVm`, `ConfigOverrideVm`, `ConfigFaultVm`, `ConfigLayersVm` + `ConfigLayersVm::{new,with_folder_faults}`, `ConfigFaultVm::folder`, `fault_kind`, `layers_summary`; a `config_layers` test module (12 tests) |
| `keeper/src/lib.rs` | phase-one install block (rewritten from the `import_config_file` block, both halves of the old ordering contract kept); phase-two `mainSyncFolder` validation; `same_folder`; one entry in the shared `generate_handler!` body; the misfiled OAuth deep-link comment moved down to the block it describes |
| `keeper/src/ipc.rs` | `config_layers` command; `ConfigLayersVm` added to the `keeper_core::vm` import list |
| `src/lib/ipc/gen/Config{Tier,Override,Fault,Layers}Vm.ts` | generated |
| `src/lib/ipc/client.ts` | four type re-exports, one import, `configLayers()` |
| `src/lib/stores/config-layers.ts` | **new** — the mirror, `refreshConfigLayers`, `overrideFor`, `useSettingOverride` |
| `src/components/settings/config-source-section.tsx` | **new** — `ConfigSourceSection`, `FileControlled` |
| `src/components/settings/config-source-section.test.tsx` | **new** — 12 tests |
| `src/components/settings/settings-dialog.tsx` | mounts the section; eight controls wear `FileControlled` |
| `src/test/config-command-platform.test.ts` | **new** — 2 tests |
| `src/test/file-controlled-keys.test.ts` | **new** — 3 tests |
