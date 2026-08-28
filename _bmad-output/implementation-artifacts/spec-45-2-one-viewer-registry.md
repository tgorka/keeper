---
title: 'Story 45.2: One Viewer Registry'
type: 'feature'
created: '2026-08-10'
status: 'review'
blocking_condition: ''
baseline_revision: ''
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-45-open-it-change-it-put-it-back.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-5-one-attachment-vocabulary.md'
---

<intent-contract>

## Intent

**Problem:** keeper shows you the name of a thing where it should show you the thing, and the reason
it cannot is that nothing in the frontend knows *which component renders what*. Every surface that
has ever needed that answer has answered it privately: `recording-embed.ts` branches on a kind,
`files-pane.tsx` keeps its own `KIND_ICON` table, and each viewer this epic adds would grow a third
and a fourth. Three CSV widgets that disagree about a ragged row is where that path ends, and 44.16
already refused the equivalent in Rust by putting the CSV decision in `keeper-core::notes::csv`.

**Approach:** one table, consulted by every surface, keyed on the vocabulary 43.5 already
established. A surface asks the registry; a surface never switches on an extension. Adding a format
is a row — extensions, viewer, icon, rendered half, editor syntax, writability — and no surface
changes. Unknown is a row like any other (AD-91): a format keeper cannot render still names the
file, names its extension, states its size, and offers the two actions that leave keeper.

## The decision this story exists to make: what does the registry key on?

**Both, with the 43.5 kind strictly dominant, in three cases.**

1. `folder` — the folder row. The extension is never consulted, because a directory is known from
   the dirent that listed it and no extension table can tell `2026.08` from `notes.zip`.
2. `video`, `image`, `audio` — that row, and **the extension is never consulted**. Rust decided;
   TypeScript does not second-guess it. A `.heic` added to `IMAGE_EXTENSIONS` in `keeper-core`
   renders as an image the day it lands, with no change in the frontend.
3. `file` — and only here, the lowercased last extension refines the answer against `FILE_FORMATS`.
   A miss, a name with no extension, an empty name: the unknown row.

**Why case 3 is not the second classifier this story exists to prevent.** `file` is 43.5's
*declared* catch-all. `kind_for_file_name`'s own doc comment calls it "every extension not named
above" — the bucket that means *keeper has no element for this*. A `.csv` and a `.md` are both
`file` there and always will be, because neither is a `<video>`, an `<img>` or an `<audio>`.
Refining inside that bucket answers a question 43.5 deliberately did not ask, and it cannot
contradict the classifier, because the kind decides first: the extension table never gets a chance
to disagree.

**That is an argument, and an argument is not a guarantee.** So `classifier-agreement.test.ts` reads
`recordings_fts.rs`, parses `VIDEO_EXTENSIONS`, `IMAGE_EXTENSIONS` and `AUDIO_EXTENSIONS`, and
asserts `FILE_FORMATS` is disjoint from all three. The two vocabularies live in different languages
in different crates and no type can hold both — the situation 43.5 met between the kind tables and
`note_protocol::mime_for`, solved the same way, by walking both and asserting. A parse that finds
nothing must FAIL rather than pass vacuously, so each table is checked against the arity Rust
declares in its own type (`[&str; 8]`) and against a member known to be there.

**The kind is a required field, and that is the enforcement mechanism.** `ViewerSubject.kind` comes
from Rust — `FilesEntryVm.kind`, `RecordingNoteTargetVm.kind`. A caller cannot ask this registry a
question without having Rust's answer in hand. There is no name-only overload and there must never
be one: the first one added makes the extension the primary key at exactly one call site, and then
at two.

**Was the classifier widened? No, and it did not need to be.** The kinds this epic renders are the
five 43.5 already ships, and the finer distinctions the epic needs — `.md` from `.csv` from `.rs` —
are *inside* `file`, where a Rust enum variant buys nothing: a `Text` variant would still not tell
45.4 which rendered half to draw or 45.6 which syntax to load, so the extension would have to
participate anyway and the epic would have paid a wire-format change for it.

## Boundaries & Constraints

**Always:**
- One table. A surface asks `resolveViewer` or `viewerComponentFor`; it never switches on a kind, a
  format or an extension of its own.
- Resolution is total. Every input yields a frozen row; nothing returns `undefined`, nothing throws.
- Rows are module singletons compared by identity, so two surfaces resolving one file get the very
  same object.
- `registry.ts` imports nothing but its own types — no React, no IPC, no store — so 45.5 can call it
  once per row of a virtualised tree.
- `resolveViewer` takes only what it reads (`ViewerSubject` = name + kind), so an icon cell
  fabricates nothing.
- No absolute path is rendered, ever (FR-145). `absolutePath` is an action's argument and nothing
  else; the frontend never composes one (AD-65).
- Writability is two questions and both must say yes: `entry.writable` is "can this FORMAT be
  written", 45.3's `FilesWriteVm.writable` is "can this LOCATION be written".

**Never:**
- No second extension table anywhere in the frontend.
- No `register()` call. Registration by import side effect makes what renders depend on which module
  the bundler evaluated first, and a viewer that fails because a side effect did not run is a bug
  with no stack trace.
- No byte formatting in TypeScript here. The size arrives as `sizeLabel`, formatted once by
  `keeper_core::size::format_file_size`.
- No reading of file contents. A viewer loads its own bytes through 45.3's path; the registry never
  becomes a place that reads the disk.
- No new dependencies. This story added none.

## I/O & Edge-Case Matrix

| Scenario | Input (`name`, `kind`) | Expected | Error |
|---|---|---|---|
| Registered kind, video | `screen-0000.mov`, `video` | `viewer: "video"`, `format: "video"` | none |
| Registered kind, image | `whiteboard.png`, `image` | `viewer: "image"` | none |
| Registered kind, audio | `room-tone.wav`, `audio` | `viewer: "audio"` | none |
| Registered kind, folder | `2026-08`, `folder` | `viewer: "folder"` | none |
| **Kind beats extension** | `chart.md`, `image` | `viewer: "image"` — extension not consulted | none |
| **Kind beats extension** | `chart.pdf`, `video` | `viewer: "video"` | none |
| Refinement, markdown | `notes.md`, `file` | `text` / `markdown`, rendered `markdown`, writable | none |
| Refinement, CSV | `budget.csv`, `file` | `text` / `csv`, rendered `table`, writable | none |
| Refinement, JSON | `manifest.json`, `file` | `text` / `json`, rendered `structure` | none |
| Refinement, JSONL | `events.jsonl`, `events.ndjson`, `file` | `text` / `jsonl` — one row, two spellings | none |
| Refinement, source | `main.rs`, `file` | `text` / `source`, language `rust`, rendered `null` | none |
| Refinement, document | `report.pdf`, `deck.pptx`, `file` | `document`, `writable: false` | none |
| Unregistered format | `board.sketchpad`, `file` | the unknown row — **not** an error | none |
| Binary | `installer.exe`, `archive.zip`, `file` | unknown row; `language === null`, so 45.6 never sees it | none |
| Double extension | `clip.mov.bak`, `file` | unknown — the LAST extension decides, as in Rust | none |
| Dotless name | `Makefile`, `notes`, `file` | unknown; the viewer says the extension is "None" | none |
| Leading-dot name | `.gitignore`, `file` | unknown — a file called `.gitignore`, not a `gitignore` file | none |
| Leading dot plus one | `.env.local`, `file` | extension `local` — matches `Path::extension` | none |
| Trailing dot | `trailing.`, `file` | unknown (Rust yields `Some("")`; both miss every table) | none |
| Uppercase | `NOTES.MD`, `Budget.CsV`, `file` | markdown, csv — case does not distinguish | none |
| A path, not a name | `2026/a.mov/notes.txt`, `file` | `txt` — only the last component decides | none |
| **Prototype key** | `payload.constructor`, `x.__proto__`, `file` | the unknown row — a `Map`, so no prototype to fall through | none |
| Empty / `.` / `..` | `""`, `"."`, `".."`, any kind | a complete row, never `undefined` | none |
| Kind from a newer Rust | `thing.qqq`, `"hologram"` | the unknown row — a `Record` index is `undefined` at runtime | none |
| Bare subject | `{ name, kind }` only | the identical row a full descriptor gets | none |
| Unbound viewer id | a `.pdf` before 45.8 lands | the unknown viewer, naming "PDF", **plus one `console.info`** | none |
| Unbound, re-rendered | the same id again | no second log line — once, not once a frame | none |
| Two surfaces, one file | Files descriptor vs note descriptor | the identical frozen row, and identical rendered DOM | none |
| Unknown viewer, size | `sizeLabel: "1.2 MB"` / `null` | "1.2 MB" / "Unknown" — never "0 B" | none |
| Unknown viewer, reveal | capability true + `absolutePath` | Reveal calls `revealPath` with the Rust-composed path | swallowed |
| Unknown viewer, no file manager | `revealInFileManager: false` | Reveal **absent**, not disabled | none |
| Unknown viewer, no absolute path | `absolutePath: null` | Reveal absent even with the capability | none |
| Unknown viewer, open | `openWith` supplied / `null` | present and calls that thunk / absent | swallowed |
| Unknown viewer, opener rejects | the thunk throws | the pane survives; no unhandled rejection out of the click | swallowed |
| Unknown viewer, FR-145 | any file | the absolute path appears nowhere in the DOM | none |
| Cross-crate guard | `png` added to a `FILE_FORMATS` row | the agreement test fails and names the overlap | fails |
| Cross-crate guard | the Rust parser matches nothing | the arity assertions fail — never a vacuous pass | fails |

</intent-contract>

## Code Map

All new, all under `src/lib/viewers/`. **Nothing existing was modified** — no Rust, no
`files-pane.tsx`, no generated bindings.

- `types.ts` — the vocabulary. `ViewerId`, `ViewerFormat`, `RenderedView`, `LanguageId`, `IconName`,
  `ViewerEntry` (a row), `ViewerSubject` (what resolution reads), `ViewerFile` (what a viewer is
  handed), `ViewerProps`, `ViewerComponent`. Split from the table so a viewer imports its props
  without importing the table, and so the table stays React-free.
- `registry.ts` — `KIND_ENTRIES` (a total `Record` over the wire kind), `FILE_FORMAT_ROWS` →
  `FILE_FORMATS`, `UNKNOWN_ENTRY`, `extensionOf`, `resolveViewer`, `registeredViewerIds`.
- `components.tsx` — `VIEWER_COMPONENTS` (the static bindings; wave-2 stories add one line each),
  `resolveViewerComponent`, `viewerComponentFor`.
- `unknown-viewer.tsx` — the AD-91 placeholder and its labels.
- `actions.ts` — `openWithForProfileEntry`, the only place that decides `sync_open_entry` is the
  legal opener for a profile entry. Separate from `registry.ts` so an icon lookup does not drag the
  Tauri client into its import graph.
- `index.ts` — the public surface. Everyone imports `@/lib/viewers`.
- `registry.test.ts`, `classifier-agreement.test.ts`, `unknown-viewer.test.tsx`,
  `components.test.tsx`.

## Tasks & Acceptance

**Execution:**
- [x] The key decision made and written down: kind dominant, extension refining only inside `file`.
- [x] One table, frozen rows, total resolution, no prototype chain to fall through.
- [x] The unknown viewer: extension, size, format, and Reveal / Open in default app.
- [x] The bindings table, with an unbound id reported at INFO rather than blanking the pane.
- [x] The cross-crate guard that makes a second classifier impossible rather than merely unintended.
- [x] The interface published over `hub` and consumed by 45.1, 45.4, 45.5 and 45.6.
- [x] Thirteen mutations, each run and each caught (below).

**Acceptance Criteria:**
- `bun run test src/lib/viewers/` — **72 tests, 4 files, green** before and after the sweep.
- `cargo test -p keeper-core --lib archive::` — **not run and not required: the classifier was not
  touched.** No file under `src-tauri/` was modified by this story.

## Design Notes

**`ViewerId` is coarse on purpose, and it is the AD-88 decision showing through.** A `.md`, a `.csv`
and a `.rs` all resolve to `text`, because raw and rendered are one component: the raw half is a text
editor over the real bytes in every case, and which rendered half sits beside it is `entry.rendered`,
not a different viewer. Three ids would be three components that must agree about saving, and "the
read path and the write path disagree about what the file says" is exactly what AD-88 forbids.

**`icon` is a string and `language` is an id — not a component and not a grammar.** The registry must
stay cheap: 45.5 calls it once per row of a virtualised tree, and a `ComponentType` in the row would
put React — and eventually a document renderer's bundle — in the path of an icon lookup. Only
`@codemirror/lang-markdown` was in `package.json` when this landed, so most language ids have no
grammar and 45.6 degrades them to plain text. That is deliberate: the table states what the file IS,
and listing only the languages the editor can currently highlight would make the table a record of an
implementation gap and guarantee somebody re-derives the real answer elsewhere.

**`resolveViewer` takes `ViewerSubject`, not `ViewerFile`, and W1TypeSize was right to ask.** It
reads a name and a kind and nothing else. Requiring the full descriptor would make an icon cell
allocate a `relativePath`, an `absolutePath` and an `openWith` closure per row per render purely to
satisfy a type — and the `openWith: null` written there "just for the icon" is a lie the next reader
takes literally as "this file cannot be opened". `ViewerFile extends ViewerSubject`, so no caller
changed, and a test resolves from a bare subject and asserts it is the identical row a full
descriptor gets, so nobody widens the parameter back on a tidying pass. The enforcement is untouched:
`kind` is still required and still comes from Rust.

**`FILE_FORMATS` is a `Map`, and that is a correctness decision rather than a style one.** The house
rule prefers a `Record` for a small static string-keyed table, and it is the right default — but this
table is indexed with **untrusted input**: the extension of a file name a user can create with
`touch`. `formats["constructor"]` on an object literal returns `Object`'s constructor, a function
wearing a row's type, and the viewer crashes reading `.label`. "Resolution is total" would then be
false for `payload.constructor`. A `Map` has no prototype chain to fall through. Mutation M6 turns
the `Map` back into an object, and the hostile-name test catches it.

**An unbound viewer id logs at `console.info`, once.** Three tray listeners shipped in epic 44
declared and never mounted, and what let them ship was that nothing said so (DW-172). A wave-2 story
that forgets its binding gets a visible line naming the id, and its files fall back to a placeholder
that says "keeper recognises this as PDF but cannot show it here yet" — a different sentence from the
one an unrecognised format gets, because telling somebody their PDF is an unknown file is a small lie
that costs a bug report. The dedupe is not cosmetic: this runs in a render path, and a line per frame
is a line nobody reads.

**`resolveViewerComponent` takes its bindings as a parameter.** Not dependency injection for its own
sake: it is the only way to exercise the unbound path forever. A test that picked whichever id
happens to be unbound today would quietly stop testing anything the day the last viewer is bound.

**"Open in default app", not the epic's "Open With".** The command behind it hands the file to the
system's DEFAULT handler and offers no chooser. A button labelled "Open With…" that never asks which
is a button that lies the first time somebody wants the other application. The wording is otherwise
the Files pane's, spelled again rather than imported on 43.5's terms — one affordance, one wording,
said in the places that cannot reach each other.

**The opener is a thunk the surface supplies, not a call the viewer makes.** Which opener is legal
depends on provenance: a profile entry goes through `sync_open_entry` (profile id plus a
profile-relative subpath, so the command cannot be pointed at an arbitrary location), and a recording
goes through `recording_open_path`, whose root is the recordings destination and which would refuse a
vault note (AD-74). The registry knows formats, not provenance. `null` means the action is absent,
never disabled — 43.5's rule for Reveal on a platform with no file manager.

**`profileId` is on `ViewerFile` because a viewer cannot read bytes without it.** Added mid-flight at
W1RawRendered's request, and he was right: every 45.3 read/write command is scoped to a profile id
plus a profile-relative subpath so Rust re-resolves through `keeper_sync::browse`'s containment on
every call. Reading through `absolutePath` instead would go around that check, which is the one thing
AD-65 exists to keep. `null` is a fact, not a gap: a panel can view a file outside every profile and
cannot write it, and the viewer says so rather than offering a save that will fail.

**What was already there.** Two findings, both worth having looked for:

- **`files-pane.tsx:159` already has `KIND_ICON`** — a `Record<FilesEntryVm["kind"], LucideIcon>`
  added by 43.8. Not dead, but precisely the surface-local table 45.5 is meant to retire; W1TypeSize
  was told the line and is deleting it in favour of `resolveViewer(...).icon`. Its doc comment
  explains it is keyed on the wire type so a new kind fails compilation — a property worth keeping in
  the replacement, and it is being kept.
- **The kind was already there AND already applied.** `FilesEntryVm.kind` and
  `RecordingNoteTargetVm.kind` have carried the full `Video | Image | Audio | File | Folder`
  vocabulary since 43.5/43.8, and both are read today. That is why this story needed no Rust: the
  field the registry keys on was already reaching the frontend on every listing.

## Deliberately NOT done

- **No widening of `kind_for_file_name`, and no Rust at all.** Argued above. If a later story needs a
  kind that does not exist, it widens the classifier — not this table.
- **No component for `text`, `document`, `video`, `image`, `audio` or `folder`.** Those are 45.4,
  45.6, 45.7 and 45.8, one binding line each. Until then their files render the unknown viewer and
  log which id was missing. This is not a stub: it is the AD-91 behaviour, and it is what a `.pdf`
  will do forever if a renderer turns out not to be honestly buildable — the epic's own stated
  fallback for 45.8.
- **No changes to `files-pane.tsx`.** Consuming the registry there is 45.5's row, and three agents
  were already editing that file.
- **No `.tsv`.** 44.16's parser is a CSV parser, and this story does not get to claim a delimiter it
  has not read the code for.
- **No plugin API.** The epic says a third-party viewer is a different epic.
- **No size arithmetic.** `sizeLabel` arrives formatted; see 45.5.

## What the reverts proved

Thirteen mutations, each applied to the shipped source, each run, each watched to fail. Baseline
green **before and after** the sweep at exactly the verdict's scope (`bun run test src/lib/viewers/`,
72 passed both times). The whole sweep was re-run after the late `ViewerSubject` narrowing rather
than assuming a signature change is behaviour-free. Harness at `~/.W1Registry/mutate.py`; it restores
in a `finally`, and the post-sweep baseline confirms nothing was left mutated. **No survivors and
nothing unproved.**

| Mutation | Caught by |
|---|---|
| M1 kind no longer dominant (refine by extension first) | 5 tests, incl. `takes the kind's answer even when the extension says otherwise` and every per-kind row |
| M2 `extensionOf` takes the FIRST extension | `clip.mov.bak has extension bak`, `.env.local has extension local` |
| M3 `extensionOf` stops lowercasing | `NOTES.MD has extension md`, `is case-insensitive about the extension` |
| M4 `lastDot <= 0` → `=== -1` (a leading dot becomes an extension) | `.gitignore has no extension` |
| M5 drop the `?? UNKNOWN_ENTRY` on the kind index | `survives a kind this build's bindings do not know` |
| M6 `FILE_FORMATS` becomes a plain object | `does not throw and does not return undefined for a hostile name` |
| M7 an unbound viewer id resolves to `undefined` | 5 tests, incl. both host-parity rows and `viewerComponentFor is total` |
| M8 the unbound report is not deduplicated | `falls back to the unknown viewer and says which id was unbound` |
| M9 the unknown viewer renders the absolute path | `never renders the absolute path (FR-145)` |
| M10 Reveal ignores the platform capability | `omits Reveal where the platform has no file manager` |
| M11 an unknown size renders as "0 B" | `says the size is unknown rather than inventing one` |
| M12 the registry claims `png`, which keeper-core calls an image | `claims no extension that keeper-core already classifies as media` |
| M13 the Rust-table parser silently matches nothing | all three `parsed to its declared arity and contains …` rows |

M12 and M13 are the pair that matters. M12 proves the cross-crate guard is live; M13 proves it cannot
pass vacuously, which is the failure mode a source-parsing test is prone to and the reason the
declared arity is asserted rather than only the members.

## What I could not verify here, and why

- **The registry has not been rendered inside a real panel, a real Files row or a real note embed**,
  because none of those exist yet — 45.1's panel host, 45.5's icon cell and 45.12's embed land in
  parallel with this story or after it. What IS verified is host parity in the strong form: two
  *different* real host components mount `viewerComponentFor`'s answer for the same file, and their
  rendered DOM is asserted identical (`components.test.tsx`). That is a real assembly, not a hook
  test — but it is my host, not the shipped one. **The first surface to consume the registry should
  assert its own parity against a second surface rather than treating this as covering it.**
- **jsdom is not a webview.** The unknown viewer's layout, truncation and focus order are unverified;
  what is verified is which elements exist, with which text, and which actions are absent.
- **`console.info` is asserted; its visibility in the packaged app is not.** DW-162 is about
  `tracing::debug!` never reaching the packaged log, and the frontend equivalent is that a webview
  console line is only visible to somebody with the inspector open. If the unbound-viewer report must
  reach the app's own log, that is a wiring story that does not exist yet — flagged rather than
  quietly assumed adequate.
- **No Rust was compiled, because none was written.** The `keeper` shell crate does not build on this
  Linux box (`gobject-sys` wants `pkg-config` and the GTK headers), but nothing here needed it.
  `classifier-agreement.test.ts` reads `recordings_fts.rs` as **text**, so it verifies the two
  vocabularies agree without compiling either crate — which also means it would not notice a Rust
  file that no longer compiles. That is `cargo`'s job and the macOS gate's.
- **Every `language` id other than `markdown` and `plain` is an unverified claim** about what 45.6
  will do with it. The table states what the file is; whether the editor honours it is 45.6's
  acceptance, not this story's.
- **`FILE_FORMATS` is disjoint from the Rust media tables as they are on this branch.** The guard
  re-derives that on every run, so it stays true — but it proves agreement with the source file on
  disk, not with a version of `keeper-core` somebody has vendored or patched elsewhere.
