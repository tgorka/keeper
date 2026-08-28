---
title: '56.1 A policy that says which files may stay away'
type: 'feature'
created: '2026-08-25'
status: 'done' # draft | ready-for-dev | in-progress | in-review | done | blocked
baseline_revision: '2730e51cf4efc0b49b347d9eca6a85881f773f07'
final_revision: 'e48057ec3d3b06e48960b229cda2214e0b7ed9f0'
review_loop_iteration: 0
followup_review_recommended: true
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Roughly 80% of keeper's virtual-file mechanism already ships and is unreachable, because nothing can name *which* paths are allowed to stay unmaterialized. Epic 56 needs a per-path selector before any of the four stories downstream of it (56.2 → 56.5) has a type to consume.

**Approach:** Add a `VirtualPolicy` type to `keeper-sync` that compiles a committed root-level pattern file plus the resolved profile's own configuration into two `GlobSet`s and a size floor, and answers `Virtual` / `Materialize` for a repository-relative path with no network and no bytes read. Refuse a malformed glob loudly, naming the source and quoting the pattern. Change no arrival behaviour, delete nothing, and leave `lfs_never` untouched.

## Boundaries & Constraints

**Always:**
- `VirtualPolicy` is a **new type**, not a fourth `LfsMode` variant and not an extension of `lfs_never` (AD-122). `MediaPolicy` (`profile/mod.rs:371`) is the in-tree precedent that a differently-scoped answer gets its own type.
- Every policy term is answerable **from the pointer, never from the bytes** (AD-122): paths plus a size floor. No boolean language, no MIME matching.
- Gitignore dialect, taken from the existing helpers, not re-derived: a pattern with no `/` becomes `**/pattern`, otherwise it is root-anchored; `literal_separator(true)`; blank lines and `#` comments say nothing; `!` negates; `\!` / `\#` escape a literal first character.
- A malformed glob is a hard `SyncError::Config` naming **which source** it came from and quoting the pattern the user typed. Never silently dropped (FR-329).
- The committed pattern file is read from the **worktree** (`profile.local_path`), never from `HEAD`, and read **once** at compile time.
- `resolve()` performs no I/O of any kind (FR-328).
- Nothing in this story deletes, prunes, dehydrates or rewrites a byte of worktree content (FR-330, AD-123).
- `lfs_never`, `LfsMode`, `LfsPolicy` and `materialize_pending` keep their present meaning and behaviour, byte for byte.
- New `SyncProfile` fields use `#[serde(default)]` — that **is** the migration (`profile/mod.rs:855-861`); an old JSON row loads with an empty list and a zero floor.

**Block If:**
- The committed pattern file's name must change from `.keepervirtual` for a reason not derivable here.
- Making the refusal fail `Engine::open` literally is judged mandatory (see Design Notes: it is argued against, on two in-tree rules).

**Never:**
- **Not** an arrival-behaviour change: `engine.rs:5385 materialize_pending` is untouched, and no call site consults the policy yet. 56.2 is the consumer, per the epic's strict 56.1 → 56.2 chain.
- **Not** an IPC/UI change. No `SyncProfileReq` field, no view model, no bindings, no TypeScript. There is no form control to render yet, and an `Option` field with no producer would be an untestable stub on this host (the `keeper` shell crate does not link on Linux). Deferred to the story that renders the control (56.7/56.9), where the DW-116 `Option` rule and a `saving_an_edit_does_not_reset_…`-shaped regression test become assertable.
- **Not** a dehydrate/release verb, a TTL, a ledger column, a pin, or an `EntrySyncStatus`/`FilesSyncStatusVm` variant (56.2/56.4/56.5/56.7).
- No new crate and no new `SyncError` variant.
- No glob validation added to `SyncProfile::validate()` — that would downgrade the refusal to a non-fatal `FolderFault` (`folder.rs:279`, `:406`), which FR-329 forbids.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Nothing configured | no pattern file, empty `virtual_patterns` | every path `Materialize`; `tier() == Unset` | No error expected |
| Pattern file only | `.keepervirtual` = `40-media/**` | `40-media/a.mp4` → `Virtual`; `10-notes/a.md` → `Materialize`; `tier() == PatternFile` | No error expected |
| Basename pattern | `.keepervirtual` = `*.mp4` | `a/b/c.mp4` → `Virtual` (any depth) | No error expected |
| Root anchoring | `.keepervirtual` = `40-media/**` | `30-work/40-media/a.mp4` → `Materialize` | No error expected |
| Negation | file = `40-media/**` and `!40-media/keep.mp4` | `keep.mp4` → `Materialize`; sibling → `Virtual` | No error expected |
| Comments and blanks | file = `# note`, ``, `  `, `*.iso` | ignored, never "match everything"; `*.iso` still in force | No error expected |
| Size floor | `virtual_over_bytes = 1024`, patterns `*.bin` | 1023 bytes → `Materialize`; 1024 → `Virtual` (inclusive, as `LfsPolicy`'s threshold is) | No error expected |
| Profile overrides the file | file = `40-media/**`; profile `virtual_patterns = ["50-iso/**"]` | `40-media/**` → `Materialize`; `50-iso/**` → `Virtual`; `tier() == Profile` | No error expected |
| An empty profile list is silence | file = `40-media/**`; profile list empty | the file is still in force | No error expected |
| `.keeper/keeper.toml` layer | real temp folder, real `[folder] virtualPatterns = ["70-vm/**"]`, file = `40-media/**` | `70-vm/**` → `Virtual`; `40-media/**` → `Materialize` | No error expected |
| `keeper.<host>.toml` layer | both real folder files set `virtualPatterns` | the host file's list wins; the shared file still wins on keys it alone sets | No error expected |
| Stored row is the base | profile row sets `virtual_patterns`; `.keeper/keeper.toml` also sets it | the folder file wins (AD-98: the file keeps winning) | No error expected |
| Malformed glob in the file | `.keepervirtual` = `[unclosed` | `Err` | `SyncError::Config`, message contains `.keepervirtual` and `"[unclosed"` |
| Malformed glob in the profile | `virtual_patterns = ["[unclosed"]` | `Err` | `SyncError::Config`, message contains `virtualPatterns` and `"[unclosed"` |
| Malformed negation | file = `![unclosed` | `Err` | `SyncError::Config`, quoting `"![unclosed"` as typed |
| No pattern file | `.keepervirtual` absent | compiles; absence is silence | `NotFound` is not an error |
| Unreadable pattern file | a **directory** named `.keepervirtual` | `Err` | `SyncError::Config` naming the file and the OS reason — a policy we cannot read is not a policy |
| `resolve` does no I/O | compile, then delete the entire worktree directory | `resolve` answers identically afterwards | No error expected |
| Editing the policy deletes nothing | real 4 KiB file on disk; rewrite `.keepervirtual` to match it; recompile and resolve `Virtual` | the file's bytes and length are byte-for-byte unchanged | No error expected |
| Clean `git status` (FR-331) | real git repo whose worktree holds a committed LFS pointer | after compile + `resolve` → `Virtual`, `git status --porcelain` is empty and the worktree bytes still equal `pointer.render()` | No error expected |
| `lfs_never` is not a virtualization list | `lfs_never = ["*.mp4"]`, no virtual patterns | `*.mp4` → `Materialize`; `LfsPolicy::from_profile(&p).applies("a.mp4", big)` still `false` | No error expected |
| Old profile row | JSON row with neither new key | loads with `virtual_patterns == []`, `virtual_over_bytes == 0` | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/lfs/virtual_policy.rs` -- **new.** `VirtualPolicy`, `Virtualization`, `VirtualPolicyTier`, `VIRTUAL_PATTERN_FILE`, and the unit tests.
- `src-tauri/crates/keeper-sync/src/lfs/mod.rs` -- the flat `pub mod` list (no re-exports) plus the `//! # Modules` doc table at `:27-34`; a new module belongs in both.
- `src-tauri/crates/keeper-sync/src/exclude.rs` -- owns the dialect. `PatternSet` `:394-428`, `PatternSet::new` `:403`, `matches` `:415`, `add_pattern` `:435-448`, `add_glob` `:458-465` (`SyncError::Config` at `:462`), `match_string` `:475` (the cross-platform separator normalizer). `ExcludeSet::new` calls `add_pattern`/`add_glob` at `:274`, `:280`, `:283`, `:285`.
- `src-tauri/crates/keeper-sync/src/lfs/stage.rs` -- the structural precedent. `LfsPolicy` `:134-139`, `from_profile` `:153-183` (`SyncError::Config` at `:167`, `:172`), `applies` `:191-198`, and the compile-before-the-walk comment at `:1191-1193`. **Not modified.**
- `src-tauri/crates/keeper-sync/src/profile/mod.rs` -- `SyncProfile` `:730` (`camelCase`, no `Default` derive); `lfs_never` `:828-829`; `lfs_threshold_bytes` `:790-791`; the serde-default-is-the-migration doc `:855-861`; `SyncProfile::new` `:913-950` (every default lives here); `accepted_profile_keys` `:1105`.
- `src-tauri/crates/keeper-sync/src/profile/folder.rs` -- `FOLDER_FIELD_RULES` `:104-152` (hand-listed, camelCase keys); `folder_field_rule` `:160`; `FolderTier::layer_paths` `:232-240`; `FolderTier::apply` `:258-287`; the exhaustiveness test `folder_field_rules_cover_every_profile_field` `:680-696`; the temp-folder idiom `:660-667`; the per-layer precedence test to copy `:995-1020`.
- `src-tauri/crates/keeper-sync/src/error.rs` -- `SyncError::Config(String)` `:181-184`, `Retriability::Permanent` `:237`, `code() == "config"` `:288`.
- `src-tauri/crates/keeper-sync/tests/virtual_policy.rs` -- **new.** The FR-331 real-git-fixture test.
- `src-tauri/crates/keeper-sync/tests/lfs_roundtrip.rs` -- `init_repo` `:26-34`, `commit` `:42-58`, and the committed-pointer-in-the-worktree fixture `:110-159`, which the new integration test copies.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `materialize_pending` `:5385`, its `subpaths` cone `:5394-5403` and the two `LfsMode::PointerOnly` arms `:5416`, `:5429`, where 56.2 will add a third arm. **Not modified.**

## Tasks & Acceptance

**Execution:**

- [x] `src-tauri/crates/keeper-sync/src/exclude.rs` -- add `PatternSet::named(source: &str, patterns: &[String]) -> Result<Self>`, make `new` delegate with `"exclude pattern"`, and thread `source` through `add_pattern` and `add_glob` so the refusal names the source as well as quoting the pattern -- a third inlined copy of the dialect is how the three copies drift; and "invalid exclude pattern" is the wrong noun for a line in `.keepervirtual`.
- [x] `src-tauri/crates/keeper-sync/src/profile/mod.rs` -- add `#[serde(default)] pub virtual_patterns: Vec<String>` and `#[serde(default)] pub virtual_over_bytes: u64` with doc comments stating the dialect and that `0` means no floor, and initialize both in `SyncProfile::new` -- the profile record and its folder-TOML layers are three of the four policy sources, and `#[serde(default)]` is the whole migration.
- [x] `src-tauri/crates/keeper-sync/src/profile/folder.rs` -- add `("virtualPatterns", FolderFieldRule::Allowed)` and `("virtualOverBytes", FolderFieldRule::Allowed)` under the repository-policy banner -- AD-132 makes the folder TOML tier the canonical home for anything both hosts must honour, and `folder_field_rules_cover_every_profile_field` fails until every field is classified.
- [x] `src-tauri/crates/keeper-sync/src/lfs/virtual_policy.rs` -- new module: `VIRTUAL_PATTERN_FILE`, `Virtualization`, `VirtualPolicyTier`, `VirtualPolicy { patterns, never, over_bytes, tier }` with `compile(&SyncProfile) -> Result<Self>`, `resolve(&Path, u64) -> Virtualization` and `tier()` -- the story's whole deliverable.
- [x] `src-tauri/crates/keeper-sync/src/lfs/mod.rs` -- declare `pub mod virtual_policy;` and add its row to the `//! # Modules` doc table -- the module list is flat and documented in two places.
- [x] `src-tauri/crates/keeper-sync/src/lfs/virtual_policy.rs` -- `#[cfg(test)] mod tests` covering every I/O-matrix row except the git-fixture one, including the two folder-TOML layers driven through a **real** temp folder and `FolderTier::apply`, the delete-the-worktree proof that `resolve` does no I/O, the bytes-unchanged proof for FR-330, and the `lfs_never`-is-untouched assertion -- the epic's stated failure mode is a story that asserts its central claim through a hand-mocked input.
- [x] `src-tauri/crates/keeper-sync/tests/virtual_policy.rs` -- new integration test: a real repo whose worktree holds a committed pointer, compile a policy that makes it `Virtual`, then assert `git status --porcelain` is empty and the worktree bytes still equal the pointer -- FR-331, and the one claim only a real index and a real `git` binary can settle.

**Acceptance Criteria:**

- Given a repository whose policy leaves 10 000 paths virtual, when the policy is compiled once and every path resolved, then no network call and no worktree read happens after compilation, and no path's content changes.
- Given a policy in force, when it is edited to cover a path that is already materialized, then recompiling and resolving changes only the answer — never a byte on disk (FR-330, AD-123).
- Given `lfs_never` and `virtual_patterns` both set on one profile, when `LfsPolicy::from_profile` and `VirtualPolicy::compile` are both built from it, then each honours only its own field and neither changes the other's answer.
- Given a `SyncProfile` JSON row written before this story, when it is deserialized, then it loads with an empty pattern list and a zero floor and re-serializes with the new keys present — no SQL change, no schema bump.
- Given a new `SyncProfile` field, when `folder_field_rules_cover_every_profile_field` runs, then both new fields are classified `Allowed` and the test passes.

## Spec Change Log

## Review Triage Log

### 2026-08-25 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 12: (high 4, medium 3, low 5)
- defer: 5
- reject: 4
- addressed_findings:
  - `[high]` `[patch]` Gitignore's leading `/` — the first spelling anyone coming from `.gitignore` reaches for — compiled to a glob that matched nothing, silently. On a protection line that authorized the one file the user explicitly named. `anchor_line` now resolves all three gitignore anchoring spellings, and a rooted pattern is not re-run through the basename rule, so `/keep.mp4` means the root's own file. Fences: `a_leading_slash_anchors_at_the_root_instead_of_matching_nothing`, `a_root_anchored_single_segment_does_not_become_a_basename_rule`.
  - `[high]` `[patch]` A negated directory protected nothing in any of its three spellings, because a directory pattern only ever matched the directory entry and `resolve` is never asked about one — so "virtualize the zone but not my scans folder" left every file inside it authorized to leave. A protection now also covers its subtree; the permissive half deliberately gets no such expansion, since widening a protection can only keep more bytes. Fences: `negating_a_directory_protects_the_files_inside_it_in_every_spelling`, `a_trailing_slash_covers_the_subtree_at_any_depth`, `naming_a_folder_without_a_slash_does_not_authorize_its_contents`.
  - `[high]` `[patch]` A profile tier that restated the repository's zone discarded the repository's `!` protections along with the list — git-lfs#3092's shape exactly, in the module written to prevent it. Protections now union across every source while only the permissive list is either-or (AD-123: an edit may widen what may leave, never narrow what is kept). Fences: `a_profile_list_cannot_drop_a_protection_the_committed_file_states`, `a_machine_may_add_a_protection_of_its_own`.
  - `[high]` `[patch]` The policy had no carve-out for the files that carry the rules: under `**` it answered `Virtual` for `.keepervirtual` itself, `.gitattributes`, `.lfsconfig`, `.keeper/**` and `.git/**`. A self-erasing policy file would be re-read as pointer text on the next compile. `resolve` now refuses those unconditionally, reusing `stage::is_git_control_file` (raised to `pub(crate)`) rather than copying its list. Fence: `keepers_own_control_files_are_never_authorized_to_stay_away`.
  - `[medium]` `[patch]` A UTF-8 BOM is not whitespace, so it killed the committed file's first line — and turned `!keep.mp4` into a line that no longer started with `!`, filing a protection as an authorization. Windows editors write one by default and this file is committed, so one clone's editor changed every clone's policy. Stripped once at read. Fence: `a_byte_order_mark_does_not_swallow_the_first_line_or_invert_it`.
  - `[medium]` `[patch]` `resolve`'s repository-relative precondition was documented and unenforced: `match_string` drops a root component, so an absolute path was matched as a relative one and a basename pattern answered `Virtual` for a file outside the repository. A `..` did the same. Now answered `Materialize`, and the old test — which passed only by accident of a root-anchored pattern's segments not lining up — was replaced. Fence: `a_path_outside_the_repository_is_never_authorized_to_stay_away`.
  - `[medium]` `[patch]` `tier()` reported dishonestly in two ways: the profile branch could never report `Unset`, so one stray whitespace entry in a folder TOML array silently disabled a repository's whole committed policy while claiming the profile was in force; and a bare `!` counted as the source having spoken. Override is now judged on what the list parses to, and punctuation with no pattern behind it says nothing. Fences: `a_profile_list_of_nothing_but_noise_does_not_mute_the_committed_file`, `punctuation_with_no_pattern_behind_it_is_not_a_policy`.
  - `[low]` `[patch]` A `#` at the start of a TOML array entry was read as a comment, making a path named `#drafts` unexpressible from the tier AD-132 calls canonical. A TOML array has no comments — the format has its own — so `Comments::Literal` now applies there and `Comments::Recognized` only to the line file. Fence: `a_hash_in_a_toml_entry_is_a_path_and_not_a_comment`.
  - `[low]` `[patch]` The refusal claimed to distinguish four AD-132 sources and could distinguish two: by the time a profile reaches this module the folder tier has folded both TOML layers into it. Documented as the honest limit on `FILE_SOURCE`/`PROFILE_SOURCE` and on `VirtualPolicyTier`, pointing at `FolderOutcome::owned` as the thing that can name the file. Also recorded there that the size floor is always the profile's and the protections are a union, so no caller reads more into `tier()` than it knows.
  - `[low]` `[patch]` The deliberate double-compile of the never list (once as typed for the message, once stripped for matching) was replaced: `PatternSet::anchored` now carries `(source, effective, original)` per entry, so one compile quotes the line as typed while matching the anchored form — and a union of two lists can still name the list a bad line is in. Fence: `a_malformed_protection_in_a_profile_entry_names_that_list`, which also asserts the message never quotes a glob keeper derived.
  - `[low]` `[patch]` Refusal wording had three spellings of the same noun across one module and rendered `could not compile virtualPatterns entry set`. Source labels are now the lists' own names (`exclude`, `.keepervirtual`, `virtualPatterns`) with the messages supplying the word "pattern"; the per-pattern exclude message is byte-identical to before, and `ExcludeSet`'s own set-level message was aligned.
  - `[low]` `[patch]` The module header claimed both of its guarantees were "asserted in this module's tests"; the delete-the-worktree test proves only the worktree half of the no-I/O claim, and the `git status` assertion is close to a tautology while `compile`'s only filesystem call is a read. Both now say what they settle and why they are worth keeping as 56.2 and 56.4 add verbs beside them. Also documented in `resolve` that a `Virtual` answer is an authorization about a path that already holds pointer text, which is why `lfs_mode`/`lfs_threshold_bytes`/`lfs_never` are deliberately not consulted (AD-122) — the reviewers' proposal to fold them in was rejected on that ground.
  - `[low]` `[patch]` The two new keys were accepted by `keeper-syncd`'s `[[profile]]` tables the moment `SyncProfile` carried them, while the annotated template the daemon writes documented every neighbouring key including the one they must not be confused with. Both added to `keeper-syncd/src/config.rs`, stating plainly that this is not the opposite of `lfsNever`.

Every fix above was mutation-checked: reverting the five behavioural changes (leading-slash anchoring, subtree protection, cross-tier union, control-file carve-out, BOM strip) fails 7 of the new tests; restoring returns the file to its exact checksum. The four rejected findings were: folding `lfs_mode`/`lfs_threshold_bytes`/`lfs_never` into the policy (AD-122 forbids it, and a path with no pointer has nothing for the answer to act on); a symlinked, FIFO or oversized `.keepervirtual` (`profile/folder.rs` reads its own config with the same bare `read_to_string`, and a hostile repository could commit the content directly); a non-absolute `profile.local_path` (the same precondition every engine path already carries); and a claimed dependent on the changed set-level message (grep found none).

## Design Notes

**The committed file is `.keepervirtual`, root-level, gitignore dialect.** Nothing by that name (or `.keeperignore`) exists anywhere under `src-tauri/crates` today. `.keeperignore` was rejected: "ignore" is the wrong verb — these paths are tracked, committed and wanted; only their *bytes* may stay away — and the name would collide with a future exclude file. It plays `.lfsconfig`'s role: the *repository's* intent, overridden by the *machine's* answer.

**Negation is how `never` gets populated.** AD-122's shape carries both `patterns` and `never`; gitignore dialect already spells the second one `!`. So `never` is not a separate config key — it is the negated lines of the sources. Unlike gitignore's last-match-wins, `never` wins unconditionally over `patterns`, matching `LfsPolicy::applies` (`stage.rs:196-198`) and erring toward keeping bytes. Review hardened this half three times, each in the same direction (the protective side may only ever be wider): a protection also covers its subtree, protections **union across every tier** while only the permissive list is either-or, and the files that carry the rules — `.keepervirtual`, `.gitattributes`, `.lfsconfig`, `.keeper/**`, `.git/**` — are never authorized at all.

```rust
pub fn resolve(&self, rela: &Path, size: u64) -> Virtualization {
    if !is_inside_the_repository(rela) || is_control_file(rela) {
        return Virtualization::Materialize;
    }
    if size < self.over_bytes || self.never.matches(rela) {
        return Virtualization::Materialize;
    }
    if self.patterns.matches(rela) { Virtualization::Virtual } else { Virtualization::Materialize }
}
```

All three gitignore anchoring spellings are resolved in `anchor_line` before compilation, because two of them previously produced a glob that matched nothing — silently, which on a protection line means the bytes it named were authorized to leave. A leading `/` anchors at the root and skips the basename rule; a trailing `/` names a directory and covers what is under it without anchoring; anything else takes `exclude::anchor`'s rule.

**Precedence, reconciled with what the tree actually does.** AD-132 lists four ascending sources ending in "the host's own profile record". Read literally against `folder.rs`, that inverts AD-98: the folder tier is applied on *every* profile read (`db.rs:539`, `:550`) and a folder file **outranks** the stored row — "which is what lets the file keep winning". The epic context states the operative rule plainly: *"a TOML layer outranks the form and keeps winning on every read."* So `VirtualPolicy` adds exactly one tier **below** everything the profile says, and re-implements none of the rest:

    .keepervirtual  <  stored profile row  <  .keeper/keeper.toml  <  keeper.<host>.toml
    └── read here ──┘  └──── already resolved by profile::in_force before compile() ────┘

`compile()` therefore does one read and one override decision; the three upper layers arrive already folded into the `SyncProfile`. A profile list that **says something** replaces the file's permissive list wholesale (AD-122: the profile "overrides" the file), judged on what it parses to rather than on whether the key exists, so a stray blank or comment-shaped entry cannot silently mute a repository's committed policy. Protections are the exception and accumulate across every tier (AD-123). `virtual_over_bytes` has no pattern-file spelling at all — a size floor is not expressible in gitignore dialect — so it comes only from the profile tiers, which is why `tier()` documents itself as answering for the pattern list alone. `tier()` exists because AD-132 requires a surface to show which tier is in force, and because the refusal message must name the source; its honest limit is two sources, since the folder tier has already folded both TOML layers into the profile by the time `compile` sees it, and `FolderOutcome::owned` is the thing that can name the file.

**Why the refusal is not literally `Engine::open`.** The epic's prose says a malformed glob "fails `Engine::open`". `Engine::open` (`engine.rs:731`) is a process constructor that never sees a profile's globs, and there is no once-per-process policy seam in this crate: `LfsPolicy` is compiled at the top of the pass that uses it (`stage.rs:1193`, with the comment "built before the walk so a malformed opt-out glob fails the run outright"), as are `PatternSet` (`engine.rs:3903`) and `ExcludeSet` (`engine.rs:6111`). Failing `Engine::open` on one profile's typo would also break keeper-core's stated rule that "nothing about a config file may keep keeper from starting" (`config/mod.rs:47-55`). `SyncError::Config` is already `Retriability::Permanent`, so the pass fails permanently and the profile parks with the pattern quoted — which is what FR-329 actually asks for. This is a deliberate, argued deviation from the epic's wording, not a relaxation of the requirement.

## Verification

**Commands:**
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` -- expected: clean, no warnings (baseline was clean).
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync` -- expected: green, including the new unit tests and `tests/virtual_policy.rs`.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all` -- expected: applied, no manual formatting fights.
- `bun run lint` -- expected: exactly the 4 warnings + 1 info of the recorded baseline, no more.
- `bun run typecheck` -- expected: clean.
- `bun run test` -- expected: unchanged, since this story touches no TypeScript. Measured on this commit: 296 files / 4717 tests passing. The baseline figure carried into this run (297 / 4869) does not match this commit and was measured elsewhere; `git status --porcelain -- src/` is empty, so vitest's input is byte-identical to `baseline_revision` and this story cannot have moved the count.

Note: `cargo nextest` is not installed on this host, so `bun run test:rust` and `bun run bindings:check` cannot run; `src/lib/ipc/gen/` must simply stay untouched, which it does — this story adds no IPC type.

## Auto Run Result

Status: done

### What was implemented

A `VirtualPolicy` type in `keeper-sync` that resolves a repository-relative path to `Virtual` or `Materialize` by policy alone — no network, no bytes read, no I/O at all after compilation. It compiles a committed root-level `.keepervirtual` (gitignore dialect) together with the resolved profile's own `virtual_patterns`/`virtual_over_bytes`, refuses a malformed glob as a hard `SyncError::Config` naming the list and quoting the line as typed, and deletes nothing. It is a new type, not a fourth `LfsMode` variant and not an extension of `lfs_never`, which is untouched and still means what it meant. No arrival behaviour changed: `materialize_pending` is deliberately unmodified and 56.2 is the consumer.

### Files changed

- `src-tauri/crates/keeper-sync/src/lfs/virtual_policy.rs` — **new.** The policy: `VIRTUAL_PATTERN_FILE`, `Virtualization`, `VirtualPolicyTier`, `VirtualPolicy::{compile, resolve, tier}`, the line parser, gitignore anchoring for all three spellings, the outside-the-repository and control-file guards, and 34 unit tests.
- `src-tauri/crates/keeper-sync/tests/virtual_policy.rs` — **new.** FR-331 against a real repository, a real index and the real `git` binary.
- `src-tauri/crates/keeper-sync/src/exclude.rs` — the gitignore dialect gained a per-entry source label so a refusal names the list, `PatternSet::anchored` for callers that must anchor before compiling, and `anchor` exposed to the crate. Existing exclude messages are byte-identical.
- `src-tauri/crates/keeper-sync/src/profile/mod.rs` — `virtual_patterns` and `virtual_over_bytes` on `SyncProfile`, both `#[serde(default)]`, initialized in `SyncProfile::new`, with the old-row migration test.
- `src-tauri/crates/keeper-sync/src/profile/folder.rs` — both keys classified `FolderFieldRule::Allowed`, the tier AD-132 calls canonical.
- `src-tauri/crates/keeper-sync/src/lfs/mod.rs` — the new module in the list and the doc table.
- `src-tauri/crates/keeper-sync/src/lfs/stage.rs` — `is_git_control_file` raised to `pub(crate)` so the carve-out reuses its list rather than copying it.
- `src-tauri/crates/keeper-syncd/src/config.rs` — both keys documented in the annotated `[[profile]]` template the daemon writes.

### Review findings

12 patches applied (4 high, 3 medium, 5 low), 5 deferred, 4 rejected, 0 intent gaps, 0 spec loopbacks. The four high-severity findings were all in the protective half of the dialect, and all pointed the same way — a protection that silently protected nothing. Every behavioural fix was mutation-checked: reverting the five of them fails 7 of the new tests, and restoring returned the file to its exact checksum. Details in the Review Triage Log.

### Verification performed

- `cargo fmt --manifest-path src-tauri/Cargo.toml --all` — applied; `--check` clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` — clean. The one remaining `warning:` line is cargo's pre-existing future-incompat note about the third-party `proc-macro-error2`, not a lint on this code.
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync` — 844 lib tests pass (up from 831 pre-review, 810 pre-story) and every integration binary passes, including `tests/virtual_policy.rs`. The single exception is `tests/hooks_never_run.rs`, which fails for an environmental reason proved unrelated: this host's `/home/dev/.gitconfig` sets `core.hooksPath`, making repo-local hooks inert, and the test passes under `GIT_CONFIG_GLOBAL=/dev/null`. Deferred.
- `bun run lint` — 4 warnings + 1 info, exactly the recorded baseline. `bun run typecheck` — clean. `bun run test` — 296 files / 4717 tests passing; no TypeScript changed, so vitest's input is byte-identical to `baseline_revision`.
- Guard tests still pass by name: `folder_field_rules_cover_every_profile_field`, `the_accepted_key_set_is_every_serialized_profile_field`.

### Residual risks

- **Nothing consults the policy yet.** That is the story's declared non-goal and the epic's strict 56.1 → 56.2 chain, but it means FR-329's "refuses at startup" is reachable only through `compile`, which has no production caller. Deferred with the instruction that 56.2's review confirm its consumer compiles the policy before any materialization decision.
- **No withdrawal spelling.** A host can replace the repository's list but cannot say "materialize everything here", because an empty list is silence. Deferred to the story that renders the control, where the `Option`-vs-empty distinction and the DW-116 rule are both in scope.
- **`tier()` has no consumer.** It answers honestly and narrowly now, but its requirements will be set by the surface 56.7 builds, which may want a shape that can name the actual TOML file.
- **No user-facing documentation.** Only the daemon's template describes the keys; the `docs/sync.md` chapter is story 56.8's by the epic's own ordering. Deferred.
