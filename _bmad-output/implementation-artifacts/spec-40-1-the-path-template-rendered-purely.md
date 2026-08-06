---
title: 'Story 40.1: The Path Template, Rendered Purely'
type: 'feature'
created: '2026-08-05'
status: 'in-progress'
baseline_revision: '7615e3065968543d746e8fb58e18e39e28198f1d'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-40-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** A recording session's folder name is a fixed `keeper-rec <ts>` (or `<title> <ts>`)
string, flat under the destination root — it does not sort, it does not nest, and it is formatted in
the Tauri shell where nothing can test it. Everything epic 40 wants (year nesting, chronological
sort, retitle-as-move) needs that name to become a user-editable relative *path*, rendered by a
function with no clock and no filesystem under it.

**Approach:** Add a pure `keeper-core` module that parses a path template over the shared
`{yyyy}`-style token vocabulary into a validated `PathTemplate`, and renders it against a caller-
supplied `RenderCtx` (civil datetime, optional title, collision ordinal) into a `RelativePath` that
is legal on the APFS ∩ exFAT ∩ NTFS intersection. Nothing calls it yet — stories 40.2 and 40.3 are
its first consumers.

## Boundaries & Constraints

**Always:**
- The module is pure: no clock, no filesystem, no `tauri`, no `keeper-sync`, no new dependency in
  any `Cargo.toml`. Every input arrives through `RenderCtx`.
- Rendering guarantees, unconditionally, for every template that parsed and every title: no `:`
  anywhere; never absolute; no component that is empty, `.` or `..`; no leading/trailing separator;
  no leading/trailing space or `.` in any component; and a title can never introduce more path
  components than the template's own `/` separators.
- Validation, not sanitisation: `parse` rejects with a typed reason and never rewrites. `render` is
  infallible — every decision was made at parse time.
- The token table is documented in the module doc, and the doc states in prose that it is the same
  vocabulary `journal_template` publishes (AD-65), naming
  `keeper-core/src/notes/naming.rs::journal_path` as the sibling so the two cannot drift silently.
- Existing `notes` behaviour is unchanged: `slug()`, `note_filename()` and `journal_path()` keep
  their current outputs, guarded by their existing tests.

**Block If:**
- Delivering the rendering guarantees would require a new crate dependency (a slug/unicode/path
  crate). AD-55 and the epic both refuse one.

**Never:**
- No caller: `recording_start`, `SessionParams`, `session_folder_name`, `keeper/src/ipc.rs` and the
  frontend are untouched. Wiring is 40.2 and 40.3.
- No settings key, no IPC surface, no TypeScript bindings, no `ts-rs` derive.
- No migration of existing session folders, and no removal of the shell's current naming path.

## I/O & Edge-Case Matrix

Ctx below = 2026-08-05T14:32:07 unless stated. Template = `DEFAULT_TEMPLATE`
(`{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}`) unless stated.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Default, titled | title `"Standup"`, seq 1 | `2026/2026-08-05 1432 standup` | none |
| Default, untitled | title `None`, seq 1 | `2026/2026-08-05 1432` — collapsed `{slug}` takes the preceding space with it, no trailing space | none |
| Title slugs to nothing | title `"!!!"` | same as untitled: the token collapses with its separator | none |
| Collision, no `{seq}` in template | title `"Standup"`, seq 3 | `2026/2026-08-05 1432 standup (3)` — suffix on the final component only | none |
| Collision, explicit `{seq}` | `{yyyy}-{mm}-{dd} {slug}{seq}`, seq 1 then 2 | `2026-08-05 standup` then `2026-08-05 standup (2)` | none |
| Hostile title | template `{yyyy}/{title}`, title `"a/b:c"` | exactly 2 components; separators and `:` are stripped from the title, never rendered | none |
| `{title}` vs `{slug}` | template `{title}` / `{slug}`, title `"Café Déjà Vu"` | `Café Déjà Vu` / `cafe-deja-vu` | none |
| Reserved device name | template `{slug}`, title `"NUL"` | a non-reserved component (suffixed), never bare `nul` | none |
| Parse: traversal | `../{yyyy}` | rejected | `TemplateError::ParentComponent` |
| Parse: absolute | `/Users/x/{yyyy}` | rejected | `TemplateError::Absolute` |
| Parse: illegal char | `{HH}:{MM}` | rejected | `TemplateError::IllegalCharacter { ch: ':' }` |
| Parse: unknown token | `{week}` | rejected, reason names the token | `TemplateError::UnknownToken(String)` |
| Parse: unterminated | `{yyyy` | rejected | `TemplateError::Unterminated` |
| Parse: empty / blank | `""`, `"   "` | rejected | `TemplateError::Empty` |
| Parse: nothing guaranteed | `{slug}`, `{slug}/{title}` | rejected — could render to nothing at all | `TemplateError::MayRenderEmpty` |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/recording/path_template.rs` -- **new**; the whole story.
- `src-tauri/crates/keeper-core/src/recording.rs` -- add `pub mod path_template;` (Rust 2018 puts
  the submodule in the sibling `recording/` directory; the 5 800-line file is **not** moved to
  `mod.rs`). Nothing else in it changes.
- `src-tauri/crates/keeper-core/src/notes/naming.rs` -- `slug()`'s folding loop is extracted to a
  `pub(crate) fn slug_stem(&str) -> String` (no `"untitled"` fallback, no `-note` suffix);
  `slug()` becomes `slug_stem` + those two steps and keeps its exact current behaviour.
  `RESERVED_DEVICE_NAMES` becomes `pub(crate)`. Prior art for the token vocabulary and the
  Windows-legality rules lives here.
- `src-tauri/crates/keeper-core/src/notes/templates.rs` -- read-only precedent: the hand-rolled
  `{{token}}` scanner, and the `Stamp` struct that `RenderCtx`'s civil-datetime fields mirror.
- `keeper/src/ipc.rs:4465-4521` -- read-only: the shell naming this eventually replaces
  (`sanitize_session_title`, the ` (2)` collision loop). Do not edit — that is story 40.3.

## Tasks & Acceptance

**Execution:**
- [ ] `keeper-core/src/notes/naming.rs` -- extract `slug_stem` and widen `RESERVED_DEVICE_NAMES` to
      `pub(crate)` -- one slug algorithm in the crate, so recording folders and note filenames can
      never fold a title two different ways.
- [ ] `keeper-core/src/recording/path_template.rs` -- module doc with the token table (`{yyyy} {yy}
      {mm} {dd} {HH} {MM} {SS} {title} {slug} {seq}`), stating that `{mm}` is month and `{MM}` is
      minute, and that this is `journal_template`'s vocabulary (AD-65) -- the doc is the contract
      40.2's UI copy is written from.
- [ ] `keeper-core/src/recording/path_template.rs` -- `TemplateError` (thiserror, one variant per
      rejection reason in the matrix, lowercase messages naming the offending token/char) and
      `RelativePath` (newtype over `String`; `as_str`, `components()`, `Display`) -- 40.2 renders
      the error text inline, so each reason must stand alone as a sentence.
- [ ] `keeper-core/src/recording/path_template.rs` -- `PathTemplate::parse`, `DEFAULT_TEMPLATE`,
      `PathTemplate::as_str`, and a `Segment`/`Token` IR -- parse once at edit time, render per
      recording.
- [ ] `keeper-core/src/recording/path_template.rs` -- `RenderCtx` (civil `year/month/day/hour/
      minute/second`, `title: Option<String>`, `seq: u32`) and `PathTemplate::render` implementing
      the collapse, sanitise-title, and collision-suffix rules -- the shell owns the clock and the
      retry loop, so both arrive as plain data.
- [ ] `keeper-core/src/recording/path_template.rs` -- `#[cfg(test)] mod tests` covering every row of
      the I/O matrix plus the property test below -- the matrix is the acceptance surface.

**Acceptance Criteria:**
- Given the default template and no `{seq}` in it, when the same title is rendered at seq 1 and
  seq 2, then the two `RelativePath`s differ, differ only in the final component, and both satisfy
  every legality rule.
- Given a deterministic generator (seeded, in-test, no new dependency) of **1 000** titles drawn
  from separators, `:`, control characters, dots, spaces, emoji, CJK, combining marks, reserved
  device names and 300-character strings, when each is rendered against every token-bearing test
  template at seq 1 and seq 7, then every component is non-empty, contains no character from the
  illegal set, has no leading/trailing space or dot, is not `.`/`..`/a reserved device name, the
  path is relative, and the component count never exceeds the template's own separator count + 1.
- Given `bun run test:rust`, when it runs, then the new tests pass and every pre-existing
  `notes::naming` test still passes unmodified.
- Given `bun run check:core-tauri-free` and `bun run check:core-sync-free`, when they run after this
  change, then both exit 0, and `git diff` shows no `Cargo.toml` or `Cargo.lock` change.
- Given `cargo clippy --workspace --all-targets -- -D warnings`, when it runs, then it is clean with
  no `.unwrap()`/`.expect()` outside `#[cfg(test)]`.

## Design Notes

**Why `seq` is 1-based.** `seq` is the ordinal of this folder among siblings that rendered to the
same path — `1` is the first and adds nothing. It keeps the existing user-visible ` (2)`-for-the-
second-folder convention, and lets 40.3 write `for seq in 1..` with no off-by-one.

**Collapse rule, stated once.** `{title}`, `{slug}` and `{seq}` are *optional* tokens. When one
renders empty, it removes itself **and one adjacent literal separator run** — the preceding one if
there is one, otherwise the following one. This is what turns `…{HH}{MM} {slug}` into
`2026/2026-08-05 1432` rather than `2026/2026-08-05 1432 `. Applying it during segment assembly
(rather than trimming the finished string) is what makes `{yyyy}/{slug}/x` → `2026/x` instead of an
empty component. `MayRenderEmpty` at parse time then closes the only remaining hole: a template
whose every component is optional-only has no rendering left to fall back on, and inventing an
"Untitled" placeholder is exactly what the epic refuses.

**Sanitising a title is not sanitising a template.** The template is the user's *specification* and
is rejected when illegal; a title is *data* that arrives from anywhere (a bridge, an agent, a paste)
and is filtered — illegal characters and control codes out, whitespace runs collapsed to one space,
trimmed, capped at 80 characters on a char boundary. Rejecting a recording because its title
contains a slash would be absurd; rejecting a template that contains one is the point.

**Illegal set:** `< > : " / \ | ? *`, `\0`, and control characters — the union of what APFS, exFAT
and NTFS refuse, so a FAT pendrive stays a legal destination. `/` is legal in a *template* (it is
the separator) and never survives inside a rendered component.

## Verification

**Commands:**
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-core path_template` -- expected:
  all new tests pass.
- `bun run test:rust` -- expected: whole workspace green, `notes::naming` tests unchanged.
- `bun run check:rust` -- expected: rustfmt clean, clippy clean under `-D warnings`.
- `bun run check:core-tauri-free && bun run check:core-sync-free` -- expected: both exit 0.
- `git status --porcelain -- src-tauri/Cargo.toml src-tauri/Cargo.lock src/` -- expected: empty (no
  dependency change, no frontend change).
