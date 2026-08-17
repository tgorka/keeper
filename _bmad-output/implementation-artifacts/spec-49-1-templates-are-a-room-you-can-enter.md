# Spec 49.1 — Templates are a room you can enter

story: 49.1
status: implemented; reviewed twice and re-gated; the shell crate's own tests are owed to hesperia
branch: `feat/49-1-templates-you-can-enter` (off `main` @ f5846d6; bottom of the epic-49 stack)
binds: FR-269, FR-270, FR-271, FR-272; AD-65 (Rust composes every path), AD-121
sentinel: `MUT49-1`

<intent-contract>

**The ask, verbatim.** *"w menu sessions oprucz all active, archive stworz mowa opcje templates -
zobaczyc i edytowac templaty (oraz zmienic nazwe) - dodaj button new template obok new session"*

**Problem.** Everything except rename already exists in Rust and reaches no one. `sessions_patterns`
returns the zone template and every named template with `kind: "template"`
(`sessions_ipc.rs:709-762`); the whole render path is the create row's `<Select>`, read into pane-local
state only while `creating` is true (`sessions-pane.tsx:194-219`). `sessions_template_install` has
taken `name: Option<String>` since it was written (`sessions_ipc.rs:1793`, doc `:1780-1782`) and its
one caller drops it (`sessions-pane.tsx:235`) — behind a control that hides itself as soon as a zone
has any template (`session-pattern-picker.tsx:110-111`). Four tests are green over shapes the
application cannot produce (`session-pattern-picker.test.tsx:111,170`; `sessions-pane.test.tsx:217`;
`pattern.rs:496,508`).

**Approach.** A `Templates` mode on the board, a list that shows each template and the files inside
it, the existing file target to open one, the existing install command with its name passed, and one
genuinely new command to rename.

**Always.**
- The template list comes from `sessions_patterns` filtered on `kind === "template"` — one source of
  truth with the picker, so a template that appears in one appears in the other.
- Every path is composed in Rust. `sessions_template_entries` returns profile-relative `subpath`s;
  TypeScript joins nothing (AD-65 — the rule `session-pattern-picker.tsx:38-46` already states).
- A write (create, rename) re-reads the list through the nonce the pane already owns.
- Rename validates through `keeper_core::notes::naming::slug`, exactly as install does
  (`sessions_ipc.rs:1803-1816`), and goes through `sessions_exec::run` with a journal row.

**Block if.**
- The rename target is the zone's own `_template/` → refused with a sentence naming why: the
  directory name is the contract (`model::TEMPLATE_DIR`), and there is exactly one of it per zone.
- The new name slugs to nothing → the install command's existing sentence, reused verbatim.
- A template already exists under the new name → refused rather than merged or trashed. Install may
  trash-then-write because the operator asked for keeper's skeleton; a rename must not eat a
  neighbour.

**Never.**
- Never a fourth `STATUS_CHOICES` entry. `SessionsStatusFilter` values are matched against
  `row.status` in `filterRows` (`sessions-list.ts:15,112-135`); a template has no status. The chip
  reads as a peer and drives a separate `mode`.
- Never a fourth button in the pane header. The group is `shrink-0` beside a `min-w-0` title block
  (`sessions-pane.tsx:282-306`): at a 560px window the header has ≤512px, today's two buttons take
  ~173px, and a third takes ~290px — the 96-character subtitle then wraps to four lines and pushes
  the filter row under the fold, which is what UX-DR85 exists to prevent
  (`sessions-pane.tsx:373-374`). `New template` goes on the templates list's own header, the way
  `New space`/`Restore default spaces` sit on the Spaces heading (`session-spaces.tsx:212-240`).
- Never `_template/` scanned as a session (FR-225). Nothing here touches the indexer.
- Never a template row that opens a directory: the panel strip addresses files.

**I/O and edge-case matrix.** Every row is a test.

| # | input | expected |
|---|---|---|
| 1 | `sessions_template_entries(root, None)` on a zone whose `_template/` holds `AGENTS.md` + `about.md` | both, profile-relative subpaths, newest-first by mtime |
| 2 | `sessions_template_entries(root, Some("interview"))` | only `_template/interview/`'s files |
| 3 | entries for a name that is not a directory | `Ok(vec![])`, not an error — a template someone deleted under us is an empty room, not a fault |
| 4 | entries for an unknown root | `internal`, the existing `root_error` sentence |
| 5 | `sessions_template_rename(root, "interview", "Kick Off")` | `_template/kick-off` exists, `_template/interview` gone, journaled, rescan fired |
| 6 | rename to a name that slugs to `""` (`"###"`) | refused, install's existing sentence, nothing moved |
| 7 | rename onto an existing named template | refused naming the collision, both directories untouched |
| 8 | rename with `name = None` (the zone's own `_template/`) | refused: the zone template's name is the contract |
| 9 | rename on a root with no such template | refused, nothing created |
| 10 | board with mode `templates` | the templates list renders; the search box, the status chips' selection, Pinned and Unread do not filter it and the session rows are not shown |
| 11 | mode `templates`, zone with no template at all | the empty state offers `New template`, and the zone-template install path stays reachable |
| 12 | pressing a file row in the templates list | `setActiveTarget({ kind: "file", profileId: rootId, relativePath: <the subpath Rust returned> })` — asserted as the exact string Rust gave, never one TypeScript joined |
| 13 | `New template` with a name | `sessionsTemplateInstall(rootId, name)` — the name argument present, and the list re-read afterwards |
| 14 | `New template` with an empty name | the confirm is inert; no call is made |
| 15 | a rename that rejects | the sentence renders in the list's live region and the list is unchanged |
| 16 | switching root while in `templates` mode | the list re-reads for the new root; no cross-root rows survive |

</intent-contract>

## Code Map

### Rust (shell only — the domain gets nothing; there is no rule here, only disk)

| file:line | change |
|---|---|
| `src-tauri/crates/keeper/src/sessions_ipc.rs:581-590` | `named_templates(zone)` already reads `_template/*`; reuse, do not re-walk |
| `src-tauri/crates/keeper/src/sessions_ipc.rs` — new, beside `sessions_template_install` (`:1793`) | `sessions_template_entries(root_id: String, name: Option<String>) -> Result<Vec<SessionTemplateEntryVm>, IpcError>`: `read_dir` of `<zone>/_template[/<slug>]`, files only, `_*`/`.*` skipped, `subpath` = `format!("{}/{}", zone.subfolder-relative dir, file_name)` composed here, sorted newest-first, `Ok(vec![])` when the directory is absent |
| `src-tauri/crates/keeper/src/sessions_ipc.rs` — new, after `sessions_template_install` | `sessions_template_rename(root_id: String, name: String, new_name: String) -> Result<String, IpcError>`: slug-validate via `keeper_core::notes::naming::slug`, refuse empty slug / collision / missing source, compile a move plan, `sessions_exec::run`, `sessions_root::rescan`, return the new id (`_template/<slug>`) |
| `src-tauri/crates/keeper/src/lib.rs:947-950` | register both commands in `generate_handler!` — the hop no TS test can cover |
| `src-tauri/crates/keeper-core/src/sessions/vm.rs` (beside `SessionSpaceFileVm`) | `SessionTemplateEntryVm { subpath: String, name: String, mtime_ms: i64 }`, `#[derive(TS)] #[ts(export)] #[serde(rename_all = "camelCase")]` — bindings regenerate into `src/lib/ipc/gen/` |
| non-desktop | `#[cfg(not(desktop))]` stubs returning `unsupported()`, mirroring `:1853-1862` |

### TypeScript

| file:line | change |
|---|---|
| `src/lib/ipc/client.ts:5220-5224` | `sessionsTemplateInstall` unchanged — it already takes the name |
| `src/lib/ipc/client.ts` beside `:5347` | `sessionsTemplateEntries(rootId, name?)`, `sessionsTemplateRename(rootId, name, newName)` |
| `src/lib/stores/sessions-list.ts:15,29-56` | add `mode: "sessions" \| "templates"` + `setMode`. `SessionsStatusFilter` and `filterRows` are NOT touched |
| `src/components/sessions/sessions-pane.tsx:85-89,373-417` | the chip row gains a `Templates` chip driven by `mode`; a status chip press sets `mode: "sessions"`. The row wraps rather than clipping. Search/Pinned/Unread are hidden in templates mode — they filter session rows |
| `src/components/sessions/sessions-pane.tsx:194-219` | the patterns read fires for `creating \|\| mode === "templates"`; `installNonce` is the existing re-read signal and covers create and rename |
| `src/components/sessions/sessions-pane.tsx:229-241` | `installTemplate(name?: string)` — pass the argument through |
| `src/components/sessions/sessions-pane.tsx:419-453` | in templates mode the list region renders `<SessionTemplates>` instead of the session rows |
| `src/components/sessions/session-templates.tsx` — new | the list: one section per template (label from `pattern.label`, files from `sessionsTemplateEntries`), a header carrying `New template` and the name field, a per-row rename control for named templates only, a live region for what those verbs said |
| `src/components/sessions/session-templates.test.tsx` — new | matrix rows 10–16 |
| `src/components/sessions/sessions-pane.test.tsx` | mode switching, and that the create row still behaves |

## Tasks & Acceptance

- [x] `SessionTemplateEntryVm` in `keeper-core/src/sessions/vm.rs`; bindings regenerated by `cargo test -p keeper-core`
- [x] `sessions_template_entries` + `sessions_template_rename` in `sessions_ipc.rs`, both registered in `lib.rs`, both with non-desktop stubs
- [x] Rust tests for rows 6 and 8 (`sessions_ipc.rs`'s new `mod tests` over the two pure namers) — **written, never executed here**; rows 1–4, 7 and 9 need a live zone and are owed as the hesperia smoke test, not claimed
- [x] `client.ts` wrappers, doc comments in house voice — corrected in review where they promised refusals the commands could not emit
- [x] `mode` in `sessions-list.ts`; `Templates` chip (a toggle, `aria-pressed`); templates room; `New template`; rename
- [x] Vitest coverage for matrix rows 10–16, row 12 asserting the exact Rust-composed subpath, plus the four regression tests the review pass earned
- [x] `docs/sessions.md` gains the templates verbs in the operator's language

**Acceptance.** From the Sessions board a person can press `Templates`, see every template the zone
has with the files inside it, open one and edit it in the same editor a session's README opens in,
create a second template by name, and rename a named one — without touching the create row, and
without a fourth button in the pane header.

## Design Notes

**Deviations from the plan above, each decided on evidence.**

1. **`compile_rename(from, to)` takes no ulid.** The pinned contract had a third parameter; `Plan`
   has no id field and `compile_unarchive` (`plan.rs:388`) is the precedent — a MoveDir-only verb
   taking no id. The journal row gets its id from the exec path, so the parameter had no consumer.
2. **`nameable` is a domain predicate, not an alphanumeric test in the shell.** The blocking review
   finding was that the "slugs to nothing" refusal could never fire: `notes::naming::slug` falls
   back to `"untitled"` (`naming.rs:121-131`), so a guard reading its output is dead code and
   `sessions_template_rename(root, name, "###")` moved the template to `_template/untitled`. The
   obvious fix — "does the input contain an alphanumeric?" — is wrong: `search::fold_str` drops
   combining marks, several of which `char::is_alphanumeric` accepts, so that test would still let a
   name through that slugs to nothing. `template::nameable` asks `slug_stem` itself, so the guard
   cannot drift from the slugger. This bug was pre-existing in `sessions_template_install`; this
   story is the first to advertise it as a load-bearing refusal, and the first to have a test for it.
3. **The Templates room reads a template with the walk a create uses.** The first implementation
   walked the directory itself: non-recursive, `_`-prefixed files dropped, symlinked files dropped.
   All three disagree with `pattern_files`, which is what `sessions_create` actually copies — so a
   folder-shaped template said "Nothing in this template yet." over files a create copies in full,
   and the room's count differed from the picker's Copies list for the same template. It now reuses
   `pattern::without_dirs(&pattern_files(…), &named_templates(zone))`, one walk for both surfaces.
   Consequence worth knowing: `SessionTemplateEntryVm.name` is the file's path **relative to the
   template** (`prompts/hand-off.md`), not a basename, because two nested files would otherwise
   render as two rows with the same label.
4. **`same_directory` lives in `sessions_exec.rs` and is shared.** A case-only rename
   (`_template/Interview` → `interview`) refused itself on case-insensitive APFS: the destination
   "exists" because it IS the source. Fixing only the IPC guard moved the same refusal one layer
   down into `MoveDir`, which re-checks `target.exists()`. So the identity carve-out is defined once
   (`sessions_exec.rs`, canonicalise both sides) and used by both. **Blast radius: `MoveDir` is
   shared by archive, unarchive and the promote path**, which now traverse the new branch. The
   "target exists and is a different directory" refusal is byte-identical, and Linux behaviour is
   unchanged — the comparison is by canonical path, never a lowercase string compare.
5. **A create onto an occupied name is refused in the room, not in Rust.**
   `sessions_template_install` trash-then-writes `AGENTS.md`/`about.md` when the destination holds
   them, and that is deliberate for the verb's original purpose: adopting keeper's skeleton is a
   thing an operator asked for. Under a button called `New template` it is a silent overwrite, so
   the room refuses the collision before the wire, in the same live region the rename collision uses.
   The Rust verb keeps its adopt semantics; `docs/sessions.md` now says which is which.
6. **The `Templates` chip is a toggle.** Its neighbours Pinned and Unread are `aria-pressed`
   toggles; the chip now announces its state and a second press returns to the rows, rather than
   leaving a status chip as the only undo — a rule that was stated in a comment and nowhere in the UI.

**Two review passes ran over the frozen commit** (`agent://Review491Spec` spec-conformance,
`agent://Review491Defects` defect-hunting). Both returned `incorrect`; 19 findings were fixed at the
cause, 0 disputed. The two blocking ones were the dead refusal (2 above) and an untagged async
mirror: `patterns` was the only one of the board's three mirrors carrying no root stamp, so a root
switch rendered the previous zone's template headings — with **live Rename buttons addressing the new
root's id** — for the whole duration of the new read.

## Verification

**Ran here, on Linux, in this worktree.**

| command | result |
|---|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all` | clean |
| `cargo clippy … -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` | clean |
| `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core` | 2075 + 30 across the integration suites, 0 failed; bindings regenerated and stable on a second run |
| `bun run typecheck` | clean |
| `bun run lint` | exit 0; 4 warnings, all pre-existing `suppressions/unused` |
| `bun run test` | 273 files, 4145 tests, 0 failed (this is the union with story 49.2, which shares the worktree) |
| `bun run test session-templates.test.tsx sessions-pane.test.tsx` | 37 passed — this story's own two files |
| `check:core-tauri-free`, `check:core-sync-free`, `check:syncd-lean`, `check:design` | all clean |

**Mutations run, each pinned to exactly one test** (the reviewer specified them; the coordinator
performed them, restored the file, and re-verified the restore by probe):

| mutation | tests killed |
|---|---|
| `relativePath: entry.subpath` → `entry.name` | 1 — *opens a file row at the EXACT subpath Rust returned* (the AD-65 guard) |
| delete the `name === ""` guard in `create` | 1 — *is inert with an empty name — Rust is never asked what an empty name means* |
| `patternList` root tag → `patterns?.list ?? null` | 1 — the new root-switch test, which asserts the previous zone's heading is gone and its Rename button with it |

**Not run here, and owed to hesperia.** The `keeper` shell crate does not link on this box: there is
no `pkg-config`/glib, so `cargo clippy --workspace` fails in the `glib-sys` build script and
`cargo test --workspace` cannot be reached. Therefore:

1. `sessions_ipc.rs`'s new `mod tests` (6 tests over `template_mint` and `template_at`, including
   matrix row 6 — the one that catches the dead refusal — and the `..`/separator/leading-underscore
   traversal refusals) has **never executed**. It is compile-read only. Same for
   `sessions_exec.rs`'s `same_directory` tests.
2. Matrix rows 1, 2, 3, 4, 7 and 9 describe command behaviour over a live zone and have no automated
   coverage on either side. They are owed as the manual smoke test below, not claimed.
3. `bun run check:rust:macos` and `bun run install:macos` run over the whole epic-49 stack, not per
   story.

**Manual smoke test on hesperia** (rows 1–4, 7, 9): with a zone flagged, press `Templates`; the
zone's own template lists its files including any nested ones; `New template` with a name creates a
second and it appears; open `AGENTS.md` from the room and edit it; rename the named template and the
heading follows; rename it onto the first name and read the collision refusal; rename with `###` and
read the "needs letters or digits" refusal rather than finding a `_template/untitled`.
