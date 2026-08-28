# Spec 49.2 — A space you can write into, and a row that opens as a note

story: 49.2
status: implemented; reviewed twice and re-gated; the shell projection is owed to hesperia
branch: `feat/49-2-a-space-you-can-write-into` (on top of `feat/49-1-templates-you-can-enter`)
binds: FR-273, FR-274; AD-121 (spaces are zone-level saved queries), AD-65, Story 45.18 (the
file↔note bridge), Story 44.x (`createNote(spaceId)`)
sentinel: `MUT49-2`

<intent-contract>

**The ask, verbatim.** *"spaces w sessions moga miec wiele notatek (wydaje sie ze teraz tylko jedna
jest mozliwa) … oraz dodaj new note optcje oraz gdy otwieram notatke ze spaces otworz ja jako
notatke (jak w notes)"*

**Problem, and the part of it that is not what it looks like.** *A space can only hold one note* is
false at every layer and was verified at each: `select` returns `Vec` (`spaces.rs:418-423`),
`SessionSpaceFilesVm.files` is a `Vec` (`vm.rs:487-491`), the UI maps over it
(`session-spaces.tsx:338,426-427`), and a **two-file** listing is already asserted
(`session-spaces.test.tsx:107-117`). There is no cap to lift and this story adds no multiplicity
work; anyone who "fixes" one is fixing nothing.

What actually produces the symptom is the write path. Four of the five default spaces are single
`tag:` queries (`spaces.rs:100-155`), and the only creators a person can reach are `New log` and
`New prompt` on the **Files** heading (`session-file-actions.tsx:140-168`). `New file` writes no kind
tag on purpose (`session-file-actions.tsx:9-18`), so what it makes lands in `Unfiled`
(`session-detail.tsx:392-411`) where no space lists it. About, References and Tasks therefore hold
exactly what the template wrote, forever.

The second half is a one-word difference. A space row opens `kind: "file"`
(`session-spaces.tsx:442-447`); the Notes pane opens `kind: "note"` (`notes-pane.tsx:289-296`). Both
are variants of one `PanelTargetVm` (`panels.ts:45,251-256`), and the converter shipped in Story
45.18: `openNoteForFile` (`vault-link/actions.ts:113-149`) resolves the vault, looks the note id up
by path in `notes_tree` (an id survives a rename; a path does not), makes that vault active, opens
the note target and switches the primary view.

**Approach.** Give a space the create verb the notes rail already has, with the kind derived in Rust;
and route the row click through the bridge that already exists, falling back honestly.

**Always.**
- The kind a space creates is **derived in Rust** and arrives on the VM. TypeScript never parses
  `keeper.space` — the rule `use-notes-actions.ts:67-73` states for notes spaces, restated here.
- A created file is opened immediately, through the same code path a row click uses — so a file made
  in a space that resolves to a note opens as a note.
- `onChanged()` after a create: the definitions and the selections are two payloads
  (`vm.rs:487-497`) and only a re-read makes the new row appear.
- The section's existing `notice` live region (`session-spaces.tsx:242-248`) carries every sentence a
  verb produces, including the two refusal sentences Story 45.18 already worded
  (`vault-link/actions.ts:44-64`).

**Block if.**
- The space's query is not exactly one creatable `tag:` term → `newFileKind` is `None` and the
  button is **absent**, never disabled (the `showNoteInFiles` precedent, `vault-link/actions.ts:66-73`).
- The query names `about` → `None`. `sessions_file_new_kind` refuses it for a real reason: a session
  has one record and a second would give `shape()` two answers (`sessions_ipc.rs:2067-2076`).
- The zone is not inside a registered notes vault → the row keeps today's file target.
  `notePathForFile` returns `null` there (`rule.ts:145-149`) and a file target is the only correct
  answer, not a failure.

**Never.**
- Never a per-space limit, a `take(n)`, or a "show more" — there was never a cap.
- Never a fourth icon button in the space's header row. It holds a dot, a glyph, a truncating name, a
  count, Edit and Delete; at ~208px of card width (a 560px window with the panel strip open) a
  fourth control leaves the name ~36px. `New note` is the third and last; the fold in Story 49.3
  goes on the title, not in this row.
- Never a path joined in TypeScript: `sessions_file_new_kind` returns the subpath it wrote, and that
  string is what gets opened.

**I/O and edge-case matrix.** Every row is a test.

| # | input | expected |
|---|---|---|
| 1 | space `tag:task` | `newFileKind: Some("task")`; the button renders, `aria-label = "New note in Tasks"` |
| 2 | space `tag:about` | `newFileKind: None`; no button |
| 3 | space `tag:log AND date:today` | `None` — one term or nothing |
| 4 | space `tag:ref` | `Some("ref")` — the kind with no button anywhere in the app before this story |
| 5 | space whose query does not parse (`space.error != null`) | `None`; the fault sentence is unchanged |
| 6 | space `tag:project/alpha` (a hierarchical tag, not a kind) | `None` — only `KINDS` members qualify |
| 7 | press `New note in Tasks` | `sessionsFileNewKind(rootId, sessionId, "task", <title>)` once, then `onChanged()`, then the returned subpath is opened |
| 8 | that call rejects | the sentence lands in the section's live region; no navigation happens |
| 9 | a row inside a space, zone inside a vault | `openNoteForFile(...)` → a `kind: "note"` target and the notes view |
| 10 | a row inside a space, zone outside every vault | today's `kind: "file"` target, byte-identical to `session-spaces.test.tsx:124-134`'s assertion |
| 11 | a row whose file has no note in the index | the Story 45.18 sentence in the live region, and the file target as the fallback |
| 12 | a created file in a space that resolves to a note | opens as a note, not as a file — the create and the click share one opener |

</intent-contract>

## Code Map

### Rust

| file:line | change |
|---|---|
| `src-tauri/crates/keeper-core/src/sessions/vm.rs:444-485` | `SessionSpaceVm` gains `new_file_kind: Option<String>` with a doc comment stating that Rust derives it so TypeScript never reads the DSL |
| `src-tauri/crates/keeper-core/src/sessions/spaces.rs` | `pub fn creatable_kind(query: &str) -> Option<KindTag>` — parse the query, accept exactly one `tag:` term, reject `about`, reject anything not in `shape::KINDS`. Pure, in the domain, where the query grammar already lives |
| `src-tauri/crates/keeper/src/sessions_ipc.rs` (the `SessionSpaceVm` projection) | populate `new_file_kind` from `creatable_kind` |
| `src-tauri/crates/keeper-core/src/sessions/spaces.rs` tests | matrix rows 1–6 as domain tests — this is the one piece of new logic and it is pure |

### TypeScript

| file:line | change |
|---|---|
| `src/components/sessions/session-spaces.tsx:120-131` | `SessionSpacesProps` gains `sessionId: string` and `vaults` (from `notesVaultsStore`, the way the text viewer resolves its vault) |
| `src/components/sessions/session-spaces.tsx:313-461` | `SpaceSection` gains a `FilePlus` button when `space.newFileKind !== null`, third in the row, before Edit; and its row `onClick` routes through the opener below |
| `src/components/sessions/session-spaces.tsx` — new local | `openSpaceFile(subpath)`: `notePathForFile(vaults, rootId, subpath) !== null ? openNoteForFile(...) : setActiveTarget({ kind: "file", … })`, the returned sentence into `notice` |
| `src/components/sessions/session-detail.tsx:413-418` | pass `sessionId`; subscribe the vaults mirror |
| `src/components/sessions/session-spaces.test.tsx:124-134` | split into rows 9 and 10 — in-vault becomes a note target, outside-a-vault keeps the file target verbatim |
| `src/components/sessions/session-spaces.test.tsx` | rows 1–8, 11, 12 |

The title for a created file: mirror `session-file-actions.tsx:140-168`'s existing inline flow
verbatim rather than inventing a second one. An empty title is legal —
`sessions_file_new_kind` already names it `untitled` (`sessions_ipc.rs:2081-2087`).

## Tasks & Acceptance

- [x] `creatable_kind` in `keeper-core/src/sessions/spaces.rs` with tests for rows 1–6, plus the fold and the every-`KINDS`-member round trip
- [x] `new_file_kind` on `SessionSpaceVm`, projected in the shell's one construction site, bindings regenerated
- [x] `New note in <space>` in `SpaceSection`, absent when the kind is `None` **or the session is folder-shaped** — see Design Notes 1
- [x] one opener shared by the create and the row click, note-first with the file fallback, one gesture on both arms, with a latest-press guard
- [x] `session-spaces.test.tsx` split at rows 9/10 (row 10's assertion byte-identical) and extended to 1–8, 11, 12 plus the four regression tests review earned
- [x] `docs/sessions.md`: a space can be written into, and what a row opens as — including that a zone outside every vault opening the file is a configuration, not a failure

**Acceptance.** In a session, a person can add a note to References, Tasks, Log or Prompts from the
space itself, watch it appear in that space, and open any row in the full note editor when the zone
lives inside a vault — with the file viewer still the answer when it does not.

## Design Notes

**"Only one note is possible" was the one claim that was false, and nothing was built for it.**
`select` returns a `Vec`, `SessionSpaceFilesVm.files` is a `Vec`, the UI maps over it, and a two-file
listing was already asserted before this story (`session-spaces.test.tsx:107-117`). The symptom came
from the WRITE path, which is what this story fixes. Recorded here so a future reader does not go
hunting for a cap that never existed.

**Deviations and decisions, each from a review finding.**

1. **The create is gated on the flat shape, not only on the kind.** `sessions_file_new_kind` writes a
   stamped file into the session ROOT, and a folder-shaped session's pool is `README.md` plus
   `refs/` and `prompts/` only (`sessions_root.rs:1048-1092`) — root-level markdown is excluded. So
   on a folder-shaped session the file a space just created would be invisible to that space forever.
   `New prompt` is gated the same way for the same reason (`session-file-actions.tsx:196`). Absent,
   never disabled.
2. **`vaults === null` is "keeper has not looked yet", not "no vault holds this zone".** The first
   cut resolved against `vaults ?? []`, so during the hydration window — and permanently if the
   single best-effort hydration failed — a vault-backed zone silently opened the FILE surface, which
   is exactly the behaviour this story exists to remove, down the arm that is deliberately silent.
   The opener now awaits `ensureNotesVaultsHydrated()` and re-reads the mirror, and a list that is
   still unknown opens the file **and says so**: silence belongs to the configuration, never to the
   failure.
3. **One gesture on both arms.** The note arm went through `openPanel` (append-beside) while the file
   arm used `setActiveTarget` (replace), so one click meant two different things depending on where
   the zone sits. `openNoteForFile` gained an additive `gesture` option defaulting to `"beside"`, so
   the text viewer's behaviour is byte-identical, and the space passes `"replace"` — matching
   `notes-pane.tsx:289-296`, which uses replace for a single click and `openPanel` for a double.
4. **A latest-press guard, and it had to reach inside the bridge.** Resolving a row costs one or two
   IPC round trips whose cost depends on the vault directory, so two clicks really can finish out of
   order. The repo's `requestSeq` idiom guards the caller, but the vault switch and the panel target
   both happen *inside* `openNoteForFile` — so the guard is passed in as `stillWanted` and checked
   before any mutation there too.
5. **The same-minute filename collision was real.** Two spaces each hold their own in-flight flag
   now, and `taken_in` reads the directory before either write lands, so two creates in the same
   minute could compile the same stamped name. Fixed; the old single shared flag had made it
   unreachable by accident rather than by design.
6. **The create's failure sentence names its space.** It lands in a zone-level live region, and a
   sentence that does not say which space it is about is a sentence under the wrong heading.

## Verification

**Ran here, on Linux, in this worktree (`/tmp/wt-492`, branch 2 alone — the compile-alone gate a
stacked PR needs).**

| command | result |
|---|---|
| `cargo fmt --all` | clean |
| `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` | clean |
| `cargo test -p keeper-core` | 0 failed, including the eight `creatable_kind` tests |
| `tsc --noEmit` | clean |
| `biome check .` | 4 warnings, all pre-existing |
| `vitest run` | 273 files, **4153** tests, 0 failed |

**Mutations run, each pinned to exactly one test** (specified by the defect review, performed by the
coordinator, each file restored and the restore verified by `git status` being empty afterwards):

| mutation | tests killed |
|---|---|
| drop `asFile()` from the "vault-backed but not indexed" arm | 1 — *row 11: says a vault file has no note yet, and still opens the file* |
| `creatable = kind !== null` (drop the flat-shape half) | 1 — the folder-shape regression test |
| `tags::normalise(&term.value)?` → `term.value.clone()` | 1 — `the_tag_is_folded_the_way_the_index_folds_it` |

**Not run here, and owed to hesperia.** The `keeper` shell crate does not link on this box, so the
`space_vm` projection of `new_file_kind` is read, not compiled. `bun run check:rust:macos` and
`bun run install:macos` run once over the whole epic-49 stack.

**Manual smoke test on hesperia:** in a flat session, press *New note* on Tasks and watch the file
appear in Tasks rather than in Unfiled; press a row and get the note editor with its properties and
backlinks, not the plain file viewer; do the same in a zone outside every vault and get the file
viewer with no sentence; open a folder-shaped session and find no create control on any space.

## Superseded

Story 50.1 removed two of this story's claims. Recorded here rather than dropped, because both
were argued for at the time and the arguments are what changed.

**1. The note arm, and the acceptance sentence that promised it.** This spec said a space row
opens as a vault note when the zone lives inside a registered notes vault, and matrix rows 9, 11
and 12 tested that arm. No configuration reaches it. `notePathForFile` resolves a file only when a
vault CONTAINS it, and `SessionsConfig::validate` (`keeper-sync/src/profile/mod.rs:648-654`)
refuses a sessions zone that overlaps a notes vault in either direction — *"one folder cannot be
both a vault and a sessions zone"*, because two indexers claiming one tree is a state nobody can
reason about afterwards. So the acceptance sentence described a machine that cannot be configured,
and the arm was a promise the product forbids keeping. 50.1 deleted the arm, the
`SESSION_SPACE_VAULTS_UNKNOWN` fallback, the `stillWanted` press guard and the vault-list
hydration that existed only for them, along with rows 9, 11 and 12. Row 10 survives as the one
opener case, now seeded with the impossible vault-contains-zone fixture so that re-adding the arm
turns it red. `src/lib/vault-link/**` is untouched: the text viewer still resolves files that
genuinely are in a vault.

**2. The flat-shape half of the create gate.** Design Notes deviation 1 recorded that the control
is gated on `shape === "flat"` because `sessions_file_new_kind` writes into the session ROOT while
a folder-shaped pool reads `README.md` plus `refs/` and `prompts/`. The premise was exactly true
and the remedy treated the symptom: the fix is to write where the pool reads. 50.1 made the writer
shape-aware through `keeper_core::sessions::shape::kind_dir`, so References and Prompts are
creatable in a folder-shaped session and the blanket gate is gone. What survives is narrower and
now stated where the person is looking: the folder contract has no tasks file, and its log is a
`## Log` heading rather than a file.

The same correction applies to `session-file-actions.tsx`'s `New prompt` gate, whose recorded
reason ("the kind is the directory; a tagged file there would be filed twice") was never true of
the reader — `pool::read_one` derives a kind from tags alone (AD-120).
