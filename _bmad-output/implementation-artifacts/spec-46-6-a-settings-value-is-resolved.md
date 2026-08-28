# Spec 46.6 — A settings value is resolved, not imported

story: 46.6
epic: 46 (The file is the setting)
binds: AD-98 (resolve, don't import), AD-99 (the owner's layer order), AD-101 (two-phase load)
author: W2Layers
date: 2026-08-10
branch: `feat/epic-46-config-and-the-gaps`

## What shipped

A new `keeper-core/src/config/` module resolves a settings value from a stack of
TOML layer files, and `registry::get_setting` consults it **before** it opens a
database. That one line gives every one of the ~40 typed getters layering for
free, with their existing clamping intact.

| file | change |
| --- | --- |
| `keeper-core/src/config/mod.rs` | new — the layer engine (types, parser, loader, process-global) |
| `keeper-core/src/config/keys.rs` | owned by story 46.9 (W2Coverage); I call `keys::layer_may_set` and `keys::Shape::coerce` |
| `keeper-core/src/registry.rs` | `get_setting` consults the overlay first; `scalar_setting_text` extracted; `import_config_file` re-documented as the bottom layer |
| `keeper-core/src/lib.rs` | `pub mod config;` |
| `keeper-core/Cargo.toml` | `toml = { workspace = true }` |
| `keeper/src/sync.rs` | `read_host_label` moved to keeper-core; `pub(crate) use` re-export left behind; the duplicated test deleted |

`keeper/src/lib.rs` and `keeper/src/ipc.rs` are **not** touched — the startup
wiring and the IPC surface are story 46.7 (W2Main), who is coding against the
signatures below.

## The API

```rust
pub enum LayerTier { UserGlobal, UserGlobalMachine, MainShared, MainMachine, FolderShared, FolderMachine }
pub struct LayerSource   { tier: LayerTier, path: PathBuf, folder: Option<String> }
pub struct SettingOverride { value: String, source: LayerSource }
pub struct AppLayers     { overrides: BTreeMap<String, SettingOverride>, main_folder: Option<PathBuf>, faults: Vec<LayerFault> }
pub struct LayerFile     { path, tier, settings, folder: Option<toml::Table>, main_sync_folder, faults }
pub struct LayerFault    { kind: LayerFaultKind, path, tier, folder, key, line, message }

pub fn read_host_label() -> String;
pub fn keeper_dir(root: &Path) -> PathBuf;
pub fn layer_paths(keeper_dir: &Path, host: &str) -> [PathBuf; 2];
pub fn parse_layer_file(path: &Path, tier: LayerTier, folder: Option<&str>, text: &str) -> LayerFile;
pub fn load_app_layers(home: &Path, host: &str) -> AppLayers;
pub fn install(layers: AppLayers);
pub fn setting_override(key: &str) -> Option<SettingOverride>;
pub fn overrides() -> Vec<(String, LayerSource)>;
pub fn main_folder() -> Option<PathBuf>;
pub fn faults() -> Vec<LayerFault>;
pub fn push_fault(fault: LayerFault);
```

### Deviations from the batch contract, and why

1. **`LayerTier::UserGlobalMachine` added.** The contract's enum had five
   variants but the file layout has six files; `~/.keeper/keeper.<host>.toml`
   had no tier. It is not decorative: it is the only file an absolute path that
   differs per machine can live in, and `keys::layer_may_set` needs the tier to
   decide whether `sync.git_path` is legitimate.
2. **`mainSyncFolder` is honoured in BOTH user-global files**, machine wins, not
   only `~/.keeper/keeper.toml`. The owner's own example value —
   `/Volumes/merope/tgdrive` — is a macOS-only mount path; on the Linux box the
   same folder is somewhere else. Still refused with a named fault in every
   folder tier. Broadcast to Main, W2Folder and W2Main before implementing.
3. **`push_fault` and `main_folder` added** at W2Main's request: phase two has to
   report "`mainSyncFolder` names a real directory but no sync profile", which
   cannot be known until the engine opens.
4. **`LayerFault::summary()` added.** `Display` is deliberately multi-line for a
   TOML syntax error; the UI wants one line.

## I/O matrix

`load_app_layers(home, host)` reads exactly six paths and nothing else. It
discovers nothing, opens no database, and cannot fail.

| # | path | tier | `mainSyncFolder` | `[settings]` | `[folder]` |
| --- | --- | --- | --- | --- | --- |
| 1 | `<home>/.keeper/keeper.toml` | UserGlobal | honoured | honoured | fault (`UnknownTable`) |
| 2 | `<home>/.keeper/keeper.<host>.toml` | UserGlobalMachine | honoured, beats 1 | honoured, machine-local keys allowed | fault |
| 3 | `<main>/.keeper/keeper.toml` | MainShared | fault (`MainFolderInFolderLayer`) | honoured | passed through raw |
| 4 | `<main>/.keeper/keeper.<host>.toml` | MainMachine | fault | honoured, machine-local allowed | passed through raw |
| 5 | `<folder>/.keeper/keeper.toml` | FolderShared | fault | **fault** (`SettingsInNonMainFolder`) | passed through raw |
| 6 | `<folder>/.keeper/keeper.<host>.toml` | FolderMachine | fault | **fault** | passed through raw |

Rows 5–6 are parsed by the same `parse_layer_file` but applied by the shell's
phase two and `keeper_sync::profile::apply_folder_layers` (story 46.8); this
module never interprets `[folder]`, because `keeper-sync` cannot depend on
`keeper-core` (AD-40).

Precedence is row order, later wins, **per key**: the merge is
`BTreeMap::extend`, so a machine file that sets one key leaves the shared file's
other keys standing.

### Value mapping

Handled by `keys::Shape::coerce` (story 46.9), not by a local formatter.

| written | key's shape | stored |
| --- | --- | --- |
| `true` | `Flag01` | `"1"` |
| `false` | `Flag01` | `"0"` |
| `true` | `FlagOnOff` (`honor_remote_deletions`) | `"on"` |
| `false` | `FlagTrueFalse` (`favorites_collapsed`) | `"false"` |
| `800` / `"800"` | `Int{100,5000}` | `"800"` |
| `99999` | `Int{100,5000}` | **fault**, naming the range and the value |
| `"hevc"` | `Choice(["h264","hevc"])` | `"hevc"` |
| `nan`, `inf`, `[1,2]`, a datetime | any | **fault** |
| `"/opt/homebrew/bin/git"` | `AbsolutePath` | verbatim |

Three key spellings are one key, because TOML reads an unquoted dot as nesting:
`"recording.fps" = 30`, `recording.fps = 30`, and `[settings.recording]` +
`fps = 30`. `flatten_settings` folds nested tables into dotted keys before the
per-key gate, so a typo inside a nested table still names the whole key
(`recording.frames_per_second`, not `recording`).

## Edge cases

| case | behaviour |
| --- | --- |
| no files at all | silent; empty overrides, no faults |
| a file is absent | silent — the normal case |
| a file is unreadable (permissions) | `Unreadable` fault, that layer skipped, others apply |
| a file is not valid TOML | `Malformed` fault with the 1-based line; **the layer is skipped whole**, including the lines above the error |
| `mainSyncFolder` names a missing path | `MainFolderMissing`; the declared path is KEPT so the UI can show what was asked for; rows 3–4 not read |
| `mainSyncFolder` names a file | `MainFolderNotADirectory`, same handling |
| `mainSyncFolder = "~/tgdrive"` | expanded against `home`; nothing else expands it and a directory literally named `~` costs an afternoon |
| `mainSyncFolder = ""` | "cleared" = "never set", the convention every other path setting uses |
| `main_sync_folder` (snake_case) | accepted, like syncd accepts both spellings |
| a hostile hostname (`../../etc/passwd`, `my host`, `""`) | folded to `[A-Za-z0-9._-]`, empty ⇒ `unknown-host`; the machine file can never escape the `.keeper/` directory |
| a key with no declared shape | reported and skipped — `layer_may_set` should have refused it first, but the boot path does not `expect` |
| `install` called twice | second call ignored and logged, not a panic |
| `push_fault` before `install` | works; `faults()` returns installed ++ late |
| `set_setting` under a shadowed key | **writes the table**; the write is not refused, the settings pane reports it as shadowed |

## Storage: `OnceLock`, and why not `RwLock`

`static LAYERS: OnceLock<AppLayers>`. There is exactly one writer, `install`, and
it runs before anything reads; after phase one the resolved set never changes,
because the only later layers are per-folder and a folder may not set a settings
key at all. A lock would guard nothing and is not free — an `RwLock` read is a
read-modify-write on a shared cacheline, and `setting_override` is called by
every typed getter on the startup path. `OnceLock::get` is one acquire load.

The one thing that genuinely mutates after install is the fault list, so that —
and only that — gets `static LATE_FAULTS: Mutex<Vec<LayerFault>>`, off the hot
path. Poisoning is ignored (`into_inner`): the list is append-only prose with no
half-broken invariant, and dropping the settings pane's diagnostics because some
other thread panicked helps nobody.

**Tests use a thread-local overlay, not a resettable global.** A `OnceLock`
cannot be re-set, and `cargo test` gives each test its own thread, so
`install_for_test` is isolated by construction where a resettable global would
need a mutex every test had to remember to take. It is `#[cfg(test)]`, so
production pays nothing, and both paths share one lookup. The production
`OnceLock` path is still covered by
`install_then_read_resolves_through_the_process_global`, the one test allowed to
spend it — with deliberately fictitious keys, because a real key installed there
would shadow the table for every other test in the binary.

## The highest-leverage line

```rust
pub fn get_setting(data_dir: &Path, key: &str) -> Result<Option<String>, CoreError> {
    if let Some(resolved) = crate::config::setting_override(key) {
        return Ok(Some(resolved.value));
    }
    let conn = open(data_dir)?;
    ...
```

Before `open`, which is a fresh connection, a WAL pragma and eight
`CREATE TABLE IF NOT EXISTS` statements — a layered read must not pay for a
database it will not use.

Typed getters that inherit layering with their clamping intact (verified):

- `get_recording_segment_mb` — `clamp(100, 5000)`; a TOML `99999` reaching the
  overlay still returns `5000`.
- `get_recording_duration_cap_minutes` — `clamp(1, 600)`; `0` returns `1`.
- `get_undo_send_window` — `min(60)`; `99` returns `60`.
- `get_recording_fps` — normalize-to-a-legal-set; `"banana"` returns the
  documented default.
- `archive::get_honor_remote_deletions` — the legacy `"on"` comparison, tested
  end to end from a layer file.

Tested in `registry::tests::a_layer_value_out_of_range_still_clamps_in_the_typed_getter`
and `..._in_range_reaches_the_typed_getter_unchanged` (the out-of-range test
alone would also pass if layering did nothing).

## The defect W2Coverage found, and the fix

The first cut mapped every TOML boolean to `"1"`/`"0"`. Three keys predate that
convention: `honor_remote_deletions` and `sdk_encryption` are read against
`"on"` (`archive/mod.rs:432`), `favorites_collapsed` against `"true"`
(`keeper/src/ipc.rs:10483`). So `honor_remote_deletions = true` in a layer file
resolved to `"1"` and its getter read it as **false** — the setting silently
doing the opposite of what the file said, which is worse than not having the
file. `sdk_encryption` is refused from files anyway.

Fixed by routing every `[settings]` value through `keys::Shape::coerce`, which
is the one place a TOML value becomes the string the table stores. `ShapeError`
becomes a `ValueShape` fault carrying its own sentence.

`config.json`'s importer has the same bug and **keeps** it, deliberately: it is
the layer AD-98 replaces, it sits at the bottom of the stack, and changing what
it writes would move settings on a machine already running one. The divergence
is pinned by
`the_toml_layer_agrees_with_config_json_except_on_the_legacy_spellings` rather
than left to be rediscovered.

## `sdk_encryption`: refused by name

A TOML layer may not set it, at any tier. The row records whether an at-rest
passphrase exists in **this machine's** Keychain; a file cannot create that
item, so a file claiming `"on"` makes every account store fail to open, and it
would travel to a machine whose Keychain is a different one. The refusal names
the key and says why (`the_legacy_un_namespaced_keys_resolve_or_are_refused_by_name`).

The other two legacy un-namespaced keys resolve normally — nothing in the lookup
is keyed on a dot.

## `config.json`: retire later, and what has to happen first

Kept, at the bottom of the stack, doc-commented as superseded. Retirement is a
separate decision because deleting it silently reverts a machine that is running
one back to defaults at the next update. Before it can go:

1. A migration that reads `config.json` and writes the equivalent
   `~/.keeper/keeper.toml` — not a delete, a translation, and it has to handle
   keys `keys::KEYS` refuses (an unknown key, a machine-local key in what is now
   a shared file) by reporting rather than dropping.
2. `keys::Shape` applied to the JSON path too, or the migration reproduces the
   `"1"`-for-`"on"` bug into the file it writes.
3. One release where both are read and the app says, in the settings pane, that
   `config.json` was translated and can be deleted.

## Mutation table

Every mutation applied, the suite run, the mutation reverted, and the revert
verified by `md5sum` against the pre-mutation checksum — `git diff` is blind to
`config/mod.rs` and `config/keys.rs`, which are new files.

| # | mutation | expected | observed |
| --- | --- | --- | --- |
| 1 | `apply_file`: `overrides.extend(file.settings)` → `entry(key).or_insert(over)` (earlier wins) | precedence tests fail | `later_layers_win_per_key_across_the_whole_stack` FAILED, `every_adjacent_pair_of_app_tiers_resolves_to_the_later_one` FAILED; 22 others pass |
| 2 | `load_app_layers`: swap the UserGlobal / UserGlobalMachine `apply_file` calls | the adjacent-pair ordering test fails | `every_adjacent_pair_of_app_tiers_resolves_to_the_later_one` FAILED, `the_per_machine_user_file_may_name_the_main_folder_and_wins` FAILED |
| 3 | `get_setting`: move the overlay check BELOW `open(data_dir)?` | the ordering probe fails | `the_overlay_is_consulted_before_the_database_is_opened` FAILED; 61 others pass |
| 4 | `setting_text`: replace `Shape::coerce` with a convention-blind boolean→`"1"` formatter | the legacy-spelling tests fail | 5 FAILED, including W2Coverage's `a_layer_file_lands_each_boolean_in_the_spelling_its_own_getter_reads` and my `each_boolean_lands_in_the_spelling_its_own_getter_reads` |

Every restore was verified by `md5sum` against the checksum taken immediately
before that mutation. Round one (mutations 1–4) ran against
`config/mod.rs cccb94d6e51ccd0ded86a646b84fc75c` /
`registry.rs e5ce06413433b376169cf3f4dec2e4b7` and restored to exactly those.

`flatten_settings` and its two tests landed **after** round one, so the two
mutations the acceptance criteria name were **re-run against the final code**:

- mutation 1 on `config/mod.rs cd556121348b8deb13ff320b5e5dd8d8` — same two
  precedence tests FAILED, 24 others passed, restored to `cd556121…`;
- mutation 3 on `registry.rs 6c305bf80823a1099b0d8ecab48b1d5f` — the ordering
  probe FAILED, 61 others passed, restored to `6c305bf8…`.

`registry.rs`'s baseline moved between the two rounds because W3Capture-2 landed
`Placement.size` tests in the same file concurrently; `git diff -U0` confirms my
own hunks (`get_setting`'s overlay check at :209, `scalar_setting_text`,
`import_config_file`'s doc, and the seven overlay tests) are all intact and are
the only registry changes attributable to this story.

Mutation 3's probe deserves a note because it is not an equality assertion: the
test passes a `data_dir` that is a **regular file**, so `open`'s `create_dir_all`
cannot succeed. It first asserts an *unlayered* read of that path errors — proof
the probe is real — then asserts a layered read of the same path succeeds. That
can only happen if the overlay is consulted before `open`.

## Deliberately NOT done

- **`import_config_file` not deleted, not `#[deprecated]`.** Deleting it reverts
  a running machine; the attribute would fire a warning in the `keeper` shell,
  which I cannot compile here to check does not deny warnings. Documented
  instead, with the retirement path above.
- **`set_setting` does not refuse a shadowed write.** A refusal would have to be
  handled by every caller; a write that lands and is reported as shadowed is
  handled in one place, the settings pane.
- **`config.json`'s convention-blind mapping not fixed.** See above — a
  behaviour change to a file people are running belongs in its own commit.
- **No `[folder]` interpretation.** `LayerFile.folder` is the raw `toml::Table`;
  `keeper_sync::profile::apply_folder_layers` (46.8) owns the profile↔TOML
  mapping, because `keeper-sync` cannot depend on `keeper-core`.
- **Phase two is not wired.** `load_app_layers` is phase one only. The shell's
  install site, the per-folder layers, and the IPC surface are 46.7 and 46.8.
- **`.keeper/*.toml` is not exempted from the tier-0 exclusion here.** AD-100's
  exemption lives in `keeper-sync/src/exclude.rs`; this module only decides what
  a file means once it is read.
- **Per-key faults carry no line number.** `toml::Table` does not preserve
  spans, and searching the text for the key name would point at the wrong line
  the first time the key appears in a comment. Only the TOML syntax error, which
  `toml` gives a span for, carries a line. Fixing this means `toml::Spanned`
  over a typed document, which is a different parser.

## What I could not verify here, and why

The `keeper` shell crate does not build on Linux, so **the one line I changed in
it is uncompiled**: `keeper/src/sync.rs:306`,
`pub(crate) use keeper_core::config::read_host_label;`, replacing the local
`fn read_host_label()` at the same place. Its three call sites
(`sync.rs:46`, `ipc.rs:4886`, and the shell's own test module) all spell it
`read_host_label()` / `crate::sync::read_host_label()` and are unchanged, and
`pub(crate) use` makes both paths resolve. I also deleted the shell's
`a_host_label_is_always_produced_and_is_a_short_name`, which now lives in
`keeper_core::config` verbatim and runs on this box.

Ordered gate checks on the macOS host, in this order:

1. `cargo check -p keeper` — proves the re-export resolves and that no call site
   lost its import. This is the only gate that can fail because of my change.
2. `cargo clippy -p keeper --all-targets -- -D warnings` — a `pub(crate) use`
   that ends up unused would be `unused_imports`; it will not, but check.
3. `cargo test -p keeper --lib sync::` — the shell's sync tests, minus the one I
   moved. Expect one fewer test than before and no failures.
4. `cargo test -p keeper-core --lib` — already green here (see below), re-run
   there for the platform-conditional code paths.
5. `bun run check:core-tauri-free` and `check:core-sync-free` — `toml` is new to
   `keeper-core` and drags in neither tauri nor gix (it is already resolved in
   `Cargo.lock` as `keeper-syncd`'s parser, so `Cargo.lock` should not change
   beyond the manifest edit), but this is exactly the check that proves it.

Also not run here, by instruction: the formatter, the linter, and the workspace
suite. Main runs those once.

## Green here

```
cargo test -p keeper-core --lib config::      42 passed; 0 failed; 1 ignored
cargo test -p keeper-core --lib registry::    62 passed; 0 failed
```

The one ignored test is W2Coverage's `docs::regenerate`, a generator rather than
a check.
