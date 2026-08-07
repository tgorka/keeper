---
title: 'Story 40.2: The Template Is a Setting, and the Preview Is the Manual'
type: 'feature'
created: '2026-08-06'
status: 'done'
blocking_condition: ''
baseline_revision: '5e89824bd0d06d0e06baf5f7275a52f49ef3de1c'
final_revision: 'b02765ea0e9de0a2fc5d30a821908a34a10b9324'
review_loop_iteration: 1
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-40-context.md'
---

<intent-contract>

## Intent

**Problem:** Story 40.1 shipped `PathTemplate` — a pure parser and renderer for the session path — and
nothing calls it. The template is not a setting: there is no `recording.path_template` row, no field
on `RecordingSettingsVm`, no way to type a template, and no way to find out what a template would
produce short of starting a recording and looking in Finder. A template language whose only feedback
is the folder it silently creates is not a language the user can learn, and 40.1's thirteen
`TemplateError` sentences — each written as standalone inline UI copy — have nowhere to be printed.

**Approach:** Make the template a setting alongside `recording.destination_dir` (same k/v table, same
VM, same commands, so it inherits story 22.6's `config.json` override with no new plumbing), reject an
unparseable submission with a typed `RecordingError::TemplateInvalid` carrying the parse reason, and
add one new read-only command — `recording_path_preview(template, title)` — that renders the typed
template against the shell's clock and the resolved destination root. The destination card grows the
field, the live preview line (the absolute path the next recording would use, in the mono face) and
the inline fault, with save disabled while the template does not parse. The preview *is* the
documentation: no help panel, no token table in the UI.

## Boundaries & Constraints

**Always:**
- The preview's "now" comes from the BACKEND. `keeper-core` is clock-free by contract, the only clock
  that names a session folder is `chrono::Local::now()` in the `keeper` shell, and a TypeScript
  re-implementation of the render rules would be a second renderer — the exact drift AD-65 forbids —
  that additionally could not produce the `TemplateError` sentences. One IPC round trip per preview.
- Validated, never sanitised. An unparseable submitted template is rejected with the typed reason;
  nothing is silently rewritten, and the stored value does not move.
- A rejected `recording_settings_set` leaves the settings table byte-for-byte as it was. The template
  parse joins the existing "Reject BEFORE any write" guard block, not the write sequence.
- The effective template is always concrete: the getter's `None` (absent, blank) *and* a stored value
  that no longer parses both resolve to `DEFAULT_TEMPLATE` on READ. The settings-get never errors on
  a hand-edited `config.json`, exactly as fps and codec already promise.
- The template field is LOCAL component state keyed off the raw text and the meta title, never a
  store binding — a store binding writes on every keystroke.
- The last keystroke's preview wins, enforced by a monotonic request token (the `writeId` pattern).
- No new dependency in any `Cargo.toml` or `package.json`; `keeper-core` gains no `tauri` and no
  `keeper-sync` edge; no `.unwrap()`/`.expect()` outside `#[cfg(test)]`.

**Block If:**
- Delivering the preview would require re-implementing the render rules, the token vocabulary or the
  rejection sentences in TypeScript. It does not: one command returns all three.

**Never:**
- `recording_start`'s naming block (`sanitize_session_title`, the ` (2)` collision loop) is untouched.
  That is story 40.3; this story's template is read by the settings surfaces only.
- No help panel, no token table, no "Untitled" placeholder, no modal for an invalid template.
- No migration of existing session folders and no change to how a recording is currently named.

## I/O & Edge-Case Matrix

Now = 2026-08-05T14:32:07 local; destination root `/Users/alice/Movies/keeper`; the preview always
renders at `seq: 1`.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh install | no `recording.path_template` row | `recordingSettingsGet()` resolves `pathTemplate: "{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}"` — never `""` | none |
| Registry round trip | `set_recording_path_template(dir, "{yyyy}/x")` | `get_…` returns `Some("{yyyy}/x")` verbatim; no clamp, no normalization | none |
| Cleared vs never set | stored `""` or `"   "` | getter returns `None`; the effective read returns `DEFAULT_TEMPLATE` | none |
| Hand-edited garbage | `config.json` sets `recording.path_template` to `"../x"` | `recording_settings_get` still succeeds; the effective template degrades to `DEFAULT_TEMPLATE` | never an error from the getter |
| `config.json` override | `{"recording.path_template": "{yyyy}/{mm}/{dd} {slug}"}`, restart | the form shows that template, and its preview | none |
| Live preview, titled | field = `DEFAULT_TEMPLATE`, title box `"Standup"` | `relativePath` `2026/2026-08-05 1432 standup`; `absolutePath` `/Users/alice/Movies/keeper/2026/2026-08-05 1432 standup`, mono face | none |
| Live preview, untitled | same field, title box empty | `2026/2026-08-05 1432` — no trailing space (40.1's collapse rule) | none |
| Typing does not write | user types `{yyyy}/{mm}/{dd} {slug}` | the preview line changes; `recording_settings_set` is NOT called | none |
| Empty field previews the default | field = `""` | the preview renders `DEFAULT_TEMPLATE`; save stays enabled — clearing is a save, not a fault | none |
| Invalid: traversal | field = `../{yyyy}` | save disabled; the inline red line reads exactly `a template cannot contain a "." or ".." folder`; nothing written | `TemplateError::ParentComponent`, carried in `RecordingPathPreviewVm.problem` |
| Invalid: colon | `{HH}:{MM}` | save disabled; `the character ':' cannot be used in a folder name` | `IllegalCharacter { ch: ':' }` |
| Invalid: unknown token | `{week}` | save disabled; `{week} is not one of the tokens a template understands` | `UnknownToken("week")` |
| Invalid: optional leaf | `{yyyy}/{slug}` | save disabled; the `OptionalLeaf` sentence naming `{slug}` | `OptionalLeaf` |
| Submit invalid anyway (direct IPC) | `recording_settings_set` with `pathTemplate: "../{yyyy}"` | rejected; NOT one settings row moves, including the six unrelated ones in the same request | `IpcError { code: recordingTemplateInvalid, retriable: false, message: <the parse reason> }` |
| Clear the field and save | field emptied, Save pressed | `""` is stored (getter reads it as unset); the echoed VM carries `DEFAULT_TEMPLATE` and the field repopulates with it | none |
| Preview while destination unset | no `recording.destination_dir` | the absolute line uses the resolved default root (`effective_destination_dir`) | none |
| Preview races a keystroke | three previews in flight, responses out of order | the field shows the response for the LAST typed text; earlier responses are dropped | none |
| Settings dialog surface | template edited there, no meta card mounted | the preview renders untitled — the `{slug}` collapse case, never a stale title | none |
| Sibling-surface save | the other surface persists a template | this surface's field re-seeds from the confirmed value; an in-progress unrelated edit is not clobbered | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` -- `RECORDING_PATH_TEMPLATE_KEY` +
  `get_recording_path_template` / `set_recording_path_template`, beside the `destination_dir` pair
  they copy in shape. Round-trip test beside `recording_destination_dir_defaults_to_none_and_round_trips`.
- `src-tauri/crates/keeper-core/src/error.rs` -- `RecordingError::TemplateInvalid { reason: TemplateError }`
  with a transparent `#[error("{reason}")]`, copying `DestinationInvalid`. The enum's doc header, which
  still claims every variant maps to `Internal`, is corrected.
- `src-tauri/crates/keeper-core/src/vm.rs` -- `RecordingSettingsVm.path_template` (the EFFECTIVE
  template) + its doc block, the new `RecordingPathPreviewVm`, and the new
  `IpcErrorCode::RecordingTemplateInvalid`.
- `src-tauri/crates/keeper/src/ipc.rs` -- the `to_ipc_error` arm above the `Recording(_)` catch-all;
  `effective_path_template` / `resolve_path_template` mirroring the destination pair;
  `read_recording_settings` carrying the field; the pre-write guard in `write_recording_settings`;
  the new `recording_path_preview` command with its pure `compose_path_preview` core.
- `src-tauri/crates/keeper/src/lib.rs` -- register `ipc::recording_path_preview` in `generate_handler!`.
- `src/lib/ipc/client.ts` -- `recordingPathPreview` + the two alphabetical type lists.
- `src/lib/stores/recording-settings.ts` -- `RECORDING_PATH_TEMPLATE_DEFAULT`, and
  `applyRecordingSettings` returning the refusal reason instead of swallowing it.
- `src/components/recording/recording-destination-controls.tsx` -- the field, the preview, the fault,
  the save.
- `src/components/recording/recording-destination-controls.test.tsx` -- the three new cases.
- `src/components/recording/recording-advanced-controls.test.tsx`,
  `src/components/recording/recording-audio-controls.test.tsx`,
  `src/components/settings/recording-settings-controls.test.tsx` -- the `RecordingSettingsVm` literal
  each hard-codes gains `pathTemplate`, or `bun run typecheck` fails.
- `src-tauri/crates/keeper/src/ipc.rs:4497-4519` -- read-only: story 40.3's naming block, and the
  `chrono::Local::now()` call the preview mirrors.

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core/src/registry.rs` -- `RECORDING_PATH_TEMPLATE_KEY` and its getter/setter, blank ⇒
      `None` -- a sibling key in the same table is all the epic's "no new storage, no new command
      family" constraint permits, and it is what buys the `config.json` override for free.
- [x] `keeper-core/src/error.rs` -- `RecordingError::TemplateInvalid { reason: TemplateError }`,
      transparent `#[error("{reason}")]` -- the typed error the epic demands instead of a boolean;
      transparency is what puts 40.1's authored sentence on screen unaltered.
- [x] `keeper-core/src/error.rs` -- correct the `RecordingError` doc header -- it has claimed "all
      variants map to `IpcErrorCode::Internal`" since before `DestinationInvalid` existed, and this
      story adds the second counter-example.
- [x] `keeper-core/src/vm.rs` -- `IpcErrorCode::RecordingTemplateInvalid` -- a template with a broken
      token is the CALLER's fault, so it must not funnel to `internal`; the reasoning is
      `NotesInvalid`'s, verbatim.
- [x] `keeper-core/src/vm.rs` -- `RecordingSettingsVm.path_template` + the struct's doc block -- the
      doc is copied verbatim into the generated `.ts`, so a stale enumeration of the keys ships to
      the frontend as documentation.
- [x] `keeper-core/src/vm.rs` -- `RecordingPathPreviewVm { relative_path, absolute_path, problem }` --
      the `SyncGitVm` summary/problem shape: exactly one side is populated, and the UI renders the
      Rust-composed text verbatim rather than deciding anything.
- [x] `keeper/src/ipc.rs` -- `to_ipc_error` arm for `TemplateInvalid`, above the `Recording(_)`
      catch-all, non-retriable -- retrying the same template never helps; the user edits the field.
- [x] `keeper/src/ipc.rs` -- `effective_path_template` + the pure `resolve_path_template` -- mirrors
      `effective_destination_dir`/`resolve_destination_dir`, and makes the degrade-on-read rule
      testable without a registry.
- [x] `keeper/src/ipc.rs` -- `read_recording_settings` carries the effective template -- one
      definition of "effective settings", so the write path can never echo a value the read path
      would not.
- [x] `keeper/src/ipc.rs` -- the parse guard inside `write_recording_settings`'s existing "Reject
      BEFORE any write" block -- the eight writes are sequential, so a parse failure discovered
      mid-sequence would leave the table half-applied.
- [x] `keeper/src/ipc.rs` -- `recording_path_preview` + the pure `compose_path_preview` -- the clock
      and the destination probe are the shell's; everything below them is a pure function of
      (root, template, ctx) and is unit-tested as one.
- [x] `keeper/src/lib.rs` -- register `ipc::recording_path_preview` -- an unregistered command
      compiles clean and fails only at runtime.
- [x] `src/lib/ipc/client.ts` -- `recordingPathPreview` wrapper + both alphabetical type lists.
- [x] `src/lib/stores/recording-settings.ts` -- `RECORDING_PATH_TEMPLATE_DEFAULT`, and
      `applyRecordingSettings` resolving the refusal reason instead of discarding it -- the bare
      `catch {}` is the single most likely silent failure in this slice.
- [x] `src/components/recording/recording-destination-controls.tsx` -- the field, the token-monotonic
      preview, the inline fault, the disabled save, the re-seed on a confirmed change.
- [x] `src/components/recording/recording-destination-controls.test.tsx` -- keystroke-previews-without-
      writing, `../{yyyy}` names the Rust reason and disables save, clearing restores the default.
- [x] the three sibling test files' `RecordingSettingsVm` literals gain `pathTemplate`.
- [x] Rust unit tests: registry round trip, effective-template fallback, write rejection leaves the
      table unchanged, preview composition (titled / untitled / invalid / destination-rooted).
- [x] `bun run test:rust` regenerates `src/lib/ipc/gen/`, and the regenerated tree is committed so
      `bun run bindings:check` passes.

**Acceptance Criteria:**
- Given no `recording.path_template` row, when `read_recording_settings` runs, then `path_template`
  is `DEFAULT_TEMPLATE` — never `""`, and never the unset sentinel.
- Given a stored template that no longer parses (a hand-edited `config.json`), when
  `read_recording_settings` runs, then it SUCCEEDS and resolves `DEFAULT_TEMPLATE` — the getter never
  errors on data the import path wrote without validating.
- Given `set_recording_path_template` with a template, then `""`, then `"   "`, when each is read
  back, then the first round-trips verbatim and the other two read as `None` — "cleared" and "never
  set" are one state.
- Given a `recording_settings_set` whose `pathTemplate` is `../{yyyy}` and whose `fps` is also
  changed, when it runs, then it is rejected with `IpcErrorCode::RecordingTemplateInvalid`,
  `retriable: false` and the `TemplateError::ParentComponent` sentence as its message, and a
  subsequent read returns exactly the pre-call VM — including the co-edited `fps`.
- Given a blank `pathTemplate` in a `recording_settings_set`, when it runs, then it is accepted and
  the effective VM carries `DEFAULT_TEMPLATE`.
- Given the default template, a destination root and a titled ctx, when `compose_path_preview` runs,
  then `relative_path` is the 40.1 render, `absolute_path` is that path joined component-by-component
  under the root, and `problem` is `None`.
- Given an unparseable template, when `compose_path_preview` runs, then `problem` carries the
  `TemplateError` sentence and both path fields are `None` — the preview never shows a path the
  template could not produce.
- Given an empty or whitespace-only template, when `compose_path_preview` runs, then it previews
  `DEFAULT_TEMPLATE` — the "blank means default" rule has exactly one definition, and it is in Rust.
- Given the destination card mounted and hydrated, when the user types into the template field, then
  `recording_path_preview` is called with the typed text and `recording_settings_set` is NOT called.
- Given the user types `../{yyyy}`, when the preview resolves, then the Rust-authored reason is
  rendered inline and the save affordance is disabled.
- Given the user clears the field and saves, when the effective VM comes back, then the field shows
  `DEFAULT_TEMPLATE` rather than an empty template.
- Given `bun run bindings:check`, when it runs against the committed tree, then it exits 0.
- Given `bun run check:core-tauri-free` and `bun run check:core-sync-free`, when they run, then both
  exit 0 and no `Cargo.toml`/`Cargo.lock` changed.
- Given `cargo clippy --workspace --all-targets -- -D warnings`, when it runs, then it is clean, with
  no `.unwrap()`/`.expect()` outside `#[cfg(test)]`.

## Design Notes

**One renderer, reached over IPC.** The preview could have been rendered in TypeScript from the typed
template and `Date.now()`, and it would have been wrong the first time a rule changed on one side
only. `recording_path_preview` costs one round trip per keystroke and buys three things a mirror
cannot: the same `PathTemplate::render` that will actually name the folder, the shell's
`chrono::Local::now()` (`keeper-core` is clock-free by contract), and 40.1's thirteen authored
rejection sentences — which exist as inline UI copy and have no other home. AD-65 forbids the second
renderer; this is what obeying it looks like at the surface.

**The guard is placement, not validation.** `write_recording_settings` performs eight sequential
registry writes. A template parsed anywhere below the first of them would leave the table holding a
new segment size, a new destination and the OLD template — a half-applied save the UI would then
echo. So the parse joins the existing "Reject BEFORE any write" block beside the destination and
echo-cancellation guards, and the test asserts the whole `RecordingSettingsVm` is byte-identical
after a rejected submission that also carried an fps and a segment-size change.

**Blank is not a refusal.** An empty template clears the key; the effective read then returns
`DEFAULT_TEMPLATE`. That is the one definition of "blank means default", it lives in Rust, and it is
why clearing the field and saving repopulates the field with the default rather than with `""`.
Symmetrically, a stored template that no longer parses (a hand-edited `config.json`, which
`import_config_file` writes verbatim with no allow-list) degrades to the default on READ instead of
failing `recording_settings_get` — the same promise fps and codec already make.

**An unsaved edit outranks every store movement.** The field is local state, and the naive rule
("re-seed whenever the mirror moves") is wrong three times: the optimistic mirror update inside
`applyRecordingSettings` is itself a movement, so it retypes the field under a user who kept typing;
the revert behind a refused write is another, and it would wipe the very text the refusal is about;
and a sibling surface's confirmed save is a third. Hence the `edited` ref — set on the first
keystroke, cleared only when a save is CONFIRMED — and hence `saveTemplate` reading the confirmed
value straight out of the store rather than waiting for an effect whose ordering against the awaited
write is not something a component should depend on.

**The preview is rooted, so the root is a dependency.** `compose_path_preview` resolves the
destination root itself, which made it tempting to key the preview on (text, title) alone. Choosing a
new folder then left the card's one line of truth naming a path under the old root, one click above
the field that had just changed it — the review's blocking finding. The effective `destinationDir`
joins the effect's dependencies, and the monotonic token that already dropped stale keystroke
responses drops stale root responses for free.

**The next-session title is opt-in per surface.** `recordingMetaStore` is a module-level singleton
whose fields outlive the meta card's mount, so "no meta card is mounted" is not the same as "there is
no title". Reading it unconditionally made Settings → Recording preview a title typed on the
pre-record pane, which is exactly the stale title the matrix forbids, and it hides `{slug}`'s collapse
on the surface where the language is being learned. `withNextSessionTitle` is therefore passed by the
pre-record pane only.

**The default is the placeholder, and only the placeholder.** `RECORDING_PATH_TEMPLATE_DEFAULT`
mirrors the Rust constant as UI copy — honest because a blank field genuinely falls back to that same
template in Rust. No fallback that decides anything reads it.

**A settings save materialises the template row.** Because the read hands out the EFFECTIVE template
and every surface persists with `applyRecordingSettings({ ...live, <field> })`, a user who only
changes fps writes `recording.path_template` explicitly for the first time. `destination_dir` has
behaved this way since story 19.5 (the read resolves the effective absolute path, the write persists
it back), so this is the established convention rather than a new defect; noted here so the next
reader knows it was a decision and not an accident.

## Verification

**Commands run (all green unless noted):**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check` — clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings` — clean
  (only the pre-existing `proc-macro-error2` future-incompat note).
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-core` — 1103 tests, 1103 passed.
  Run twice: the second run left `src/lib/ipc/gen` byte-identical, so the committed bindings are the
  generated ones.
- `bun run lint` — clean (two pre-existing suppression warnings in unrelated notes components).
- `bun run typecheck` — clean.
- `bun run test` — 157 files, 1764 tests passed, plus the five review-driven cases added afterwards.
- `bun run check:core-tauri-free`, `bun run check:core-sync-free`, `bun run check:syncd-lean` — all
  exit 0. No `Cargo.toml`, `Cargo.lock` or `package.json` changed.

**Run on the metal, on macOS, because Linux cannot link the shell.** The `keeper` crate's test binary
does not LINK in the Linux container this was written in: GTK/webkit development libraries are absent
(`-lgtk-3`, `-lwebkit2gtk-4.1`, … unresolved) with no privilege to install them, so `cargo check` and
`cargo clippy --all-targets` type-check its `#[cfg(test)]` tests there but cannot run them. So the
gate that matters ran where it can: `bun run check:rust:macos` on `hesperia` (macOS 26.5.2, arm64),
which rsyncs these exact sources, builds the Swift `keeper-rec` sidecar, then runs `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings` INCLUDING the shell crate, and
`cargo test --workspace`. Result: **green**, and the seven tests this story added to `keeper/src/ipc.rs`
all executed —
`ipc::tests::recording_settings_read_carries_the_effective_path_template`,
`ipc::tests::an_unparseable_stored_template_degrades_to_the_default_on_read`,
`ipc::tests::recording_settings_set_rejects_a_bad_template_without_writing_anything`,
`ipc::tests::recording_settings_set_round_trips_a_template_and_clears_a_blank_one`,
`ipc::tests::the_path_preview_composes_the_relative_and_absolute_lines`,
`ipc::tests::the_path_preview_reports_the_parse_reason_and_no_path` and
`ipc::tests::the_preview_context_mirrors_the_local_clock_at_seq_one`. `keeper-core` reported 1074 unit
tests green plus its integration binaries, `keeper-sync` 481, and the script's drift check rsynced the
macOS-generated `src/lib/ipc/gen/` back and found the committed tree identical — which is the substance
of `bindings:check` (`test:rust` itself still cannot run on Linux). Nothing about this story's
verification is owed any more.

**Installed and observed in the field.** `bun run install:macos` built the release bundle from these
sources on hesperia and replaced `/Applications/keeper.app` (0.6.5, built 15:25 local). Read out of the
running app's accessibility tree, the Destination card carries the new row: the field seeded with the
effective template, the Rust-composed absolute line
`/Users/tgorka/Movies/keeper/2026/2026-08-06 1526` — real clock, real destination root, untitled
`{slug}` collapse with no trailing space — and `Save template` disabled while the text equals the
effective value. Minutes later the machine's owner had used it: `keeper.db` holds
`recording.path_template = {yyyy}/{yyyy}-{mm}-{dd} {HH}.{MM} {slug}`, a template only this story can
write, and the card's preview rendered it against a typed session title as
`/Users/tgorka/Movies/keeper/2026/2026-08-06 15.29 test`. Persisted setting → effective read → rendered
preview, end to end on the metal.

**Adversarial review.** Two independent read-only passes (spec-conformance and defect-hunting) over
the uncommitted diff. Six findings were acted on: the preview not re-rooting on a folder change
`[high]`; the Settings-dialog stale title `[medium]`; a write-path refusal never cleared by a later
successful save `[medium]`; the refused-save path discarding newer keystrokes and self-erasing its own
refusal `[medium]`; the `RecordingError` doc header claiming `DestinationInvalid` carries its own code
when it funnels to `Internal` as retriable `[low]`; and the "seven writes" comment that the eighth
write made false `[low]`. The unused-constant finding was resolved by wiring the default in as the
field's placeholder. The "unrelated save materialises the row" finding is recorded above as a
deliberate, precedented behaviour.

## File List

- `src-tauri/crates/keeper-core/src/registry.rs` — `RECORDING_PATH_TEMPLATE_KEY`, getter/setter,
  round-trip test.
- `src-tauri/crates/keeper-core/src/error.rs` — `RecordingError::TemplateInvalid`, corrected doc
  header.
- `src-tauri/crates/keeper-core/src/vm.rs` — `IpcErrorCode::RecordingTemplateInvalid`,
  `RecordingSettingsVm.path_template`, `RecordingPathPreviewVm`.
- `src-tauri/crates/keeper/src/ipc.rs` — `to_ipc_error` arm, `effective_path_template` /
  `resolve_path_template`, the read path, the pre-write parse guard, `recording_path_preview` /
  `compose_path_preview`, six tests.
- `src-tauri/crates/keeper/src/lib.rs` — command registration.
- `src/lib/ipc/gen/RecordingPathPreviewVm.ts` (new), `src/lib/ipc/gen/RecordingSettingsVm.ts`,
  `src/lib/ipc/gen/IpcErrorCode.ts` — regenerated bindings.
- `src/lib/ipc/client.ts` — `recordingPathPreview` wrapper, both type lists, corrected settings docs.
- `src/lib/stores/recording-settings.ts` — `RECORDING_PATH_TEMPLATE_DEFAULT`,
  `RECORDING_SETTINGS_UNKNOWN_ERROR`, `applyRecordingSettings` resolving the refusal.
- `src/components/recording/recording-destination-controls.tsx` — the template row: field,
  Rust-composed preview, inline fault, guarded save, `withNextSessionTitle`.
- `src/components/layout/recording-pane.tsx` — passes `withNextSessionTitle`.
- `src/components/recording/recording-destination-controls.test.tsx` — eight cases.
- `src/components/recording/recording-advanced-controls.test.tsx`,
  `src/components/recording/recording-audio-controls.test.tsx`,
  `src/components/settings/recording-settings-controls.test.tsx` — `pathTemplate` in the VM fixtures.
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — story status.

## Change Log

- 2026-08-06 — Story implemented end to end: the template becomes a setting, `recording_path_preview`
  lands, and the destination card grows the field, the live preview and the inline fault.
- 2026-08-06 — Addressed six adversarial-review findings (one high, three medium, two low) and pinned
  the preview-race, surface-title, folder-refresh and refused-save contracts with five new tests.
- 2026-08-06 — `bun run check:rust:macos` green on hesperia (macOS 26.5.2, arm64): the shell crate's
  clippy and its seven new `ipc.rs` tests ran there, and the macOS-generated bindings match the
  committed tree. The last owed verification is closed.
