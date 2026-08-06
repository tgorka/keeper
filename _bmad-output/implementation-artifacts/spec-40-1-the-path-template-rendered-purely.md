---
title: 'Story 40.1: The Path Template, Rendered Purely'
type: 'feature'
created: '2026-08-05'
status: 'blocked'
blocking_condition: 'intent gap in intent contract'
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
- [x] `keeper-core/src/notes/naming.rs` -- extract `slug_stem` and widen `RESERVED_DEVICE_NAMES` to
      `pub(crate)` -- one slug algorithm in the crate, so recording folders and note filenames can
      never fold a title two different ways.
- [x] `keeper-core/src/recording/path_template.rs` -- module doc with the token table (`{yyyy} {yy}
      {mm} {dd} {HH} {MM} {SS} {title} {slug} {seq}`), stating that `{mm}` is month and `{MM}` is
      minute, and that this is `journal_template`'s vocabulary (AD-65) -- the doc is the contract
      40.2's UI copy is written from.
- [x] `keeper-core/src/recording/path_template.rs` -- `TemplateError` (thiserror, one variant per
      rejection reason in the matrix, lowercase messages naming the offending token/char) and
      `RelativePath` (newtype over `String`; `as_str`, `components()`, `Display`) -- 40.2 renders
      the error text inline, so each reason must stand alone as a sentence.
- [x] `keeper-core/src/recording/path_template.rs` -- `PathTemplate::parse`, `DEFAULT_TEMPLATE`,
      `PathTemplate::as_str`, and a `Segment`/`Token` IR -- parse once at edit time, render per
      recording.
- [x] `keeper-core/src/recording/path_template.rs` -- `RenderCtx` (civil `year/month/day/hour/
      minute/second`, `title: Option<String>`, `seq: u32`) and `PathTemplate::render` implementing
      the collapse, sanitise-title, and collision-suffix rules -- the shell owns the clock and the
      retry loop, so both arrive as plain data.
- [x] `keeper-core/src/recording/path_template.rs` -- `#[cfg(test)] mod tests` covering every row of
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

## Review Triage Log

### 2026-08-06 — Review pass

- intent_gap: 2: (high 2, medium 0, low 0)
- bad_spec: 5: (high 0, medium 4, low 1)
- patch: 8: (high 0, medium 0, low 8)
- defer: 3: (high 0, medium 1, low 2)
- reject: 6
- addressed_findings:
  - none

Cascading order: an intent gap exists, so every lower finding is moot and none were
addressed. `review_loop_iteration` was not incremented — no bad_spec loopback was
triggered.

#### The intent gap (both findings, one root cause)

The `<intent-contract>` permits a template whose **final** component is built only from
optional tokens, and specifies no behaviour for the case where that component renders to
nothing. Two rows require this to be possible:

- the *Hostile title* row fixes `{yyyy}/{title}` as a template that must parse and render;
- the *Default, untitled* row establishes an untitled recording as a first-class case.

Together they describe a state the contract never resolves. Reproduced by executing the
committed code (ctx = 2026-08-05T14:32:07, `title: None`):

1. **A fully collapsed leaf makes a parent directory the session folder, and the collision
   ordinal renames that parent.** `{yyyy}/{title}` renders `2026`, then `2026 (2)`,
   `2026 (3)`. `{yyyy}/{mm}/{slug}` renders `2026/08`, then `2026/08 (2)`. Under story 40.3
   an untitled recording would write its manifest and media directly into the year
   directory that is meant to *contain* sessions; the next titled recording would then
   create a session folder inside it; and because the year directory always already exists,
   the retry loop fires on every untitled recording and grows `2026 (2)`, `2026 (3)`, …
2. **With an explicit `{seq}` in a collapsible leaf, seq 2 renders a child of seq 1.**
   `{yyyy}/{slug}{seq}` renders `2026` at seq 1 and `2026/(2)` at seq 2 —
   `"2026/(2)".starts_with("2026/")` is `true`. A collision ordinal exists to produce a
   *sibling*; here session 2 is created inside session 1, so their media interleave and
   deleting session 1 deletes session 2. `{yyyy}/{slug} {seq}` behaves identically.

Finding 2 is wrong under every reading of the epic, and neither can be fixed without a
decision that belongs to the intent contract. The three candidate resolutions are mutually
exclusive and the contract supports none of them as written:

- **(a) Reject leaf-optional templates at parse time.** Coherent, and reuses the existing
  `MayRenderEmpty` vocabulary — but it makes `{yyyy}/{title}` invalid, directly
  contradicting the *Hostile title* row.
- **(b) Keep the current behaviour and let story 40.3 refuse the path at pre-flight.**
  Requires the contract to state that a rendered path is not necessarily a session folder,
  which defeats the epic's premise that a recording lands somewhere findable.
- **(c) Substitute a placeholder for a vanished leaf.** Explicitly refused by the epic and
  by this spec's own Design Notes ("inventing an 'Untitled' placeholder is exactly what
  this epic refuses").

Choosing between (a), (b) and (c) — and amending the I/O matrix to match — is a human
decision. It is not inferable: there is more than one defensible reading.

#### Findings held moot by the cascade (recorded so nothing is lost)

Not written to the deferred-work ledger, because the story will be re-derived once the
intent gap is resolved and these would then be duplicates.

**bad_spec (5)**

- `[medium]` The 80-**character** title cap does not bound the rendered component in
  bytes. Measured on `{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {title}`: an 80-character Japanese
  title yields a 256-byte leaf, 260 bytes at seq 9, against a 255-byte `NAME_MAX` on APFS,
  ext4 and exFAT. Because the retry loop only *lengthens* the name, 40.3 could never
  recover from the resulting `ENAMETOOLONG`. The Design Notes fix a char cap and the
  guarantee list says nothing about bytes.
- `[medium]` `render` silently rewrites template *literals*, contradicting the module doc's
  "never quietly rewritten into a path the user did not ask for". Measured: `{yyyy}/..a` →
  `2026/a`; `{yyyy}//{mm}` → `2026/08`; `{yyyy}/` → `2026`; `{yyyy}/  ..  ` → `2026`.
  `ParentComponent` is an exact `.`/`..` match, so everything else falls to silent edge
  trimming. Story 40.2 shows a live preview beside the raw field; the two will disagree
  with no error to explain why.
- `[medium]` Unicode **format** characters (`Cf`) pass both the template validator and the
  title filter: `is_illegal` covers `Cc` via `is_control()` only. Measured:
  `PathTemplate::parse("\u{feff}")` succeeds and renders a folder whose entire name is an
  invisible character; the title `annual\u{202e}gpj.review` renders verbatim, which Finder
  displays as `annualweiver.jpg` — the standard RTL-override spoof. The property sweep's
  `FRAGMENTS` already contains U+200B, so the test suite *certifies* the acceptance. The
  contract's illegal set is defined as filesystem-refused characters, which `Cf` are not —
  widening it is a spec decision.
- `[medium]` The property sweep never renders an untitled recording: all 14 000 renders pass
  `Some(title)`, so `title: None` is exercised only by four hand-written examples. And
  `assert_legal` asserts only the *upper* bound on component count, no byte bound, and
  nothing about one render being a path-prefix of another — which is exactly why both
  intent-gap findings survived it. The acceptance criterion enumerates only properties that
  already hold.
- `[low]` A collapsing token can eat a separator run belonging to a neighbouring
  non-collapsing token, and two adjacent optional tokens eat both runs. Measured:
  `{dd} {slug}{title} {HH}` untitled → `0514`, not `05 14`. This follows the Design Notes'
  rule to the letter ("the preceding one if there is one, otherwise the following one"),
  which is what makes it a spec-level issue rather than a coding slip.

**patch (8, all low)**

- `RenderCtx`'s fields are `pub`, unvalidated, with no constructor and no `debug_assert`;
  `year: -5` renders a component starting with `-` (read as a flag by `ls`, `git add`,
  `rsync`), and any year outside `0..=9999` breaks the fixed-width chronological sort that
  justifies the epic.
- `seq: 0` renders byte-identical to `seq: 1`, so a 0-based caller silently reuses the
  first path. `debug_assert!(seq >= 1)` would pin the documented 1-based contract.
- The non-empty-path invariant is accidental: it holds only because `is_edge_noise` is a
  strict subset of `is_filler`, a coupling neither predicate documents and no test pins.
  Adding `'-'` to `is_edge_noise` alone would let `-{slug}` render `""`.
- `de_reserve` guards on a trimmed stem but rebuilds from the untrimmed one, so the title
  `"nul .txt"` renders `nul -rec.txt`.
- No test ties this token vocabulary to `journal_path`, though the doc's stated purpose is
  that the two "cannot drift silently". One assertion that the shared tokens render
  identically in both is the only cheap thing that would actually catch the drift.
- The brace grammar is asymmetric and unescapable, and its error strings are 40.2's inline
  UI copy: `{{yyyy}}` reports `{{yyyy} is not one of the tokens…` (mismatched braces inside
  the message), a bare `}` is a silent literal, `{ yyyy }` is rejected with no hint that
  the spaces are the problem, and there is no escape for a literal `{`.
- No doctest anywhere in the module, so every prose claim in the token table and the
  seven-bullet guarantee list is unverified by `cargo test` — including the two this review
  showed to be false.
- The story introduces one new rustdoc warning: `notes/naming.rs:96` — "public
  documentation for `slug` links to private item `slug_stem`". (The crate already emits 61
  rustdoc warnings, so this is not a gate; it is still new.)

**defer (3)** — pre-existing or downstream, not caused by this story

- `[medium]` `{title}` is neither case-folded nor NFC-folded. On case-insensitive APFS
  `2026/Standup` and `2026/standup` are one directory, so story 40.3's existence probe must
  compare case- and normalisation-insensitively, as `notes::naming::note_filename` already
  does, or it will judge an occupied path free.
- `[low]` Nothing bounds total path length or depth. `{yyyy}/{mm}/{dd}/{HH}/{MM}/{SS}
  {title}` with a long title passes Windows' 260-character `MAX_PATH` without long-path
  opt-in; a template of 5 000 separators is accepted and would have 40.3 create 5 000
  nested directories.
- `[low]` `notes/templates.rs:164` documents `MM` as month and `mm` as minute for its
  `{{date:FMT}}` field — inverted relative to this module, and each doc flags itself as the
  pair to read twice without mentioning the other. They are genuinely different dialects
  (moment-style format codes vs keeper placeholder tokens), so this predates the story, but
  two inverted conventions in one settings surface will be read as a bug.

**reject (6)** — duplicate ordinal on a parent (subsumed by intent-gap finding 2);
duplicated `{seq}` accepted; the title cap yielding 79 characters when the 80th is a space;
two vacuous tests; `'\0'` listed redundantly beside `is_control()`; the claim that
`RESERVED_DEVICE_NAMES` should contain `com0`/`lpt0` (Windows reserves only 1–9).

## Auto Run Result

Status: **blocked** — intent gap in intent contract.

### What was implemented

Story 40.1's code was already written and committed by a previous, interrupted run
(`590df2a`, "the path template, rendered purely (story 40.1, salvaged)"). This session
verified it rather than re-deriving it: all six Execution tasks are genuinely done, all
sixteen I/O-matrix rows are covered by named tests, and all five acceptance criteria hold
as written. The module is a pure `keeper-core` module — `PathTemplate::parse` /
`render`, `TemplateError`, `RelativePath`, `RenderCtx`, `DEFAULT_TEMPLATE`, a
`Segment`/`Token` IR, 24 tests including a seeded 1 000-title property sweep — with no
caller, no new dependency, and no change outside `keeper-core`.

Review then found that the intent contract itself is incomplete, in a way the acceptance
criteria could not detect because they enumerate only properties that already hold. The
code is not wrong against the spec; the spec is under-determined, and the resulting
behaviour breaks the epic's premise for any template whose last folder is optional.

### Files changed

- `src-tauri/crates/keeper-core/src/recording/path_template.rs` — new, 978 lines: the whole
  story. This session changed only two doc comments in it (see below).
- `src-tauri/crates/keeper-core/src/recording.rs` — `pub mod path_template;`, one line.
- `src-tauri/crates/keeper-core/src/notes/naming.rs` — `slug_stem` extracted from `slug`,
  `RESERVED_DEVICE_NAMES` widened to `pub(crate)`; `slug`'s behaviour and its 14 tests
  unchanged.

### Code changes made by this session

Two doc-comment corrections, no behaviour change:

1. `TITLE_MAX_CHARS`'s doc claimed the 80-character cap "clears the 255-byte name limit
   even when every character is a 4-byte codepoint" — false (80 × 4 = 320 > 255). Corrected
   to state that it is a character cap, not a byte cap. The residual defect is recorded as a
   moot bad_spec finding above.
2. Removed a redundant explicit rustdoc link target that emitted a `cargo doc` warning; the
   AD-65 sibling statement and the named `journal_path` path are preserved verbatim.

### Why the code was not reverted

The intent_gap branch calls for reverting code changes. The story's code is already
published in `590df2a` on this branch, and `docs/project-context.md` reserves branch
topology and history for the orchestrator and the human coordinator. Reverting an
already-published commit would destroy work the coordinator explicitly salvaged and which
is ~95% correct — the resolve session will want it in front of them, since resolving the
gap changes `parse`'s acceptance rule and the matrix, not the module's shape. The two
doc-comment corrections were also kept deliberately: reverting them would restore a
statement this review proved false. Nothing else from this pass touches code.

### Verification performed

Run under this container's constraints (3 GiB cap → `CARGO_BUILD_JOBS=1
CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0`, or rustc is
OOM-killed and misreports it as "could not compile"):

- `cargo test -p keeper-core --lib` — **1034 passed, 0 failed**.
- `cargo test -p keeper-core --lib path_template` — **24 passed**.
- `cargo test -p keeper-core --lib notes::naming` — **14 passed**, and
  `git diff 7615e30 HEAD -- notes/naming.rs` shows no change inside `mod tests`.
- `cargo clippy -p keeper-core --all-targets -- -D warnings` — clean (exit 0; the only
  output is a pre-existing `proc-macro-error2` future-incompat notice).
- `cargo clippy -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` — clean.
- `cargo fmt --all --check` — clean.
- `bun run check:core-tauri-free`, `bun run check:core-sync-free` — both exit 0.
- `git diff --stat 7615e30 HEAD -- src-tauri/Cargo.toml src-tauri/Cargo.lock src/` — empty.
- Every intent-gap and bad_spec finding above was **reproduced by executing the committed
  code** in a throwaway integration test, not inferred from reading it. The probe was
  deleted afterwards.

### Not verifiable in this container

- `bun run test:rust` and the `--workspace` arm of `bun run check:rust`: `cargo-nextest` is
  not installed, and the `keeper` tauri shell crate cannot be built here (glib/pkg-config
  absent) without the `/tmp/lxenv.sh` sysroot, which changes `RUSTFLAGS` and invalidates the
  whole target directory. Scoped to `-p keeper-core -p keeper-sync -p keeper-syncd` instead.
  Workspace risk is confined to the one-line `pub mod path_template;`, which compiles, and
  no crate outside `keeper-core` references `slug`, `slug_stem` or `RESERVED_DEVICE_NAMES`.
- Real filesystem legality on APFS / exFAT / NTFS is asserted only as string properties. The
  module is pure by design; `mkdir` is story 40.3's surface.

### Residual risks

- The two intent-gap findings are latent, not active: nothing calls this module yet. They
  become live defects the moment story 40.3 wires `recording_start` to it, and resolving
  them will change `parse`'s acceptance rule — so 40.2's settings UI should not be built
  against the current acceptance behaviour until the gap is closed.
- The byte-length finding has the same shape: it turns into a recording that cannot start,
  for an ordinary CJK or emoji title, on the default template.
