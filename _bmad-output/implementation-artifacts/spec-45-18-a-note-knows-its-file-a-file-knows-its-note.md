# Spec 45.18 — A Note Knows Its File, a File Knows Its Note

story: 45.18
bindings: FR-196, UX-DR79, AD-65, AD-90
depends on: 45.1 (panel targets), 45.2 (the viewer registry), 45.4 (raw/rendered, and the CSV it deferred), 45.10 (link rendering)
author: W3NoteFile

## What shipped

One resolution rule, authored in Rust and mirrored in TypeScript against a
shared vector table, and the four things it makes possible.

| file | what it is |
| --- | --- |
| `src-tauri/crates/keeper-core/src/vault_link.rs` | **new.** `note_path_for_file` / `file_path_for_note`, both total, both pure |
| `src-tauri/crates/keeper-core/src/vault-link-vectors.json` | **new.** 6 vaults, 20 `to_note`, 12 `to_file` vectors; both suites load it |
| `src/lib/vault-link/rule.ts` | **new.** the mirror |
| `src/lib/vault-link/actions.ts` | **new.** `showNoteInFiles`, `openNoteForFile` |
| `src/lib/vault-link/index.ts` | **new.** the barrel, matching `@/lib/viewers` |
| `src/lib/notes/follow-link.ts` | **new.** `resolveWikilink`, `followExternalUrl` |
| `src/components/viewers/text-file-viewer.tsx` | resolves its vault: CSV tables, preview gets a vault, **Open in Notes** |
| `src/components/notes/note-editor.tsx` | **Show in Files**; `onFollowLink` deleted and following actually wired |
| `src/components/notes/editor/live-preview.ts` | `LINK_ATTR` on external links; `onOpenUrl`; the pointer is gated on a follower |
| `src/components/notes/editor/wikilink.ts` | `LINK_ATTR` |
| `src/components/viewers/markdown-preview.ts` | passes both followers down; stops fabricating `() => {}` |
| `keeper-core/src/notes/vm.rs`, `keeper/src/notes_ipc.rs`, `stores/notes-editor.ts` | `NoteBodyBatch::Reset` gains `path` |
| `keeper/src/notes_ipc.rs`, `keeper/src/lib.rs`, `ipc/client.ts` | **new command** `notes_resolve_link` |

## The rule, and why it runs in two languages

A Files panel holds a **sync profile id** plus a profile-relative path; every
notes command holds a **notes vault id** plus a vault-relative path. They differ
by exactly the vault's `subfolder`, so converting is stripping or restoring a
prefix — and doing that in the webview is the path arithmetic AD-65 forbids,
because it is the frontend deciding which folders are vaults.

The consumers need it **synchronously**, because they use it to decide whether
an action EXISTS. An IPC round trip per file would make "Open in Notes" appear a
frame late on every row and would make a CSV panel flash "not in a notes vault"
before replacing itself with a table.

So: authored in `keeper_core::vault_link`, mirrored in `rule.ts`, pinned by
`vault-link-vectors.json` which both suites load — the treatment
`keeper_core::size`, `keeper_core::file_asset` and `keeper_core::notes::attach`
already have, and for the same reason. Nothing here has a root; every input and
output is a relative path, and `keeper_sync::browse` re-contains whatever this
produced on every real read (AD-59).

### I/O matrix — `note_path_for_file(vaults, profileId, relativePath)`

Vaults: `v-merope`/`p-merope`/`notes`, `v-merope-journal`/`p-merope`/`notes/journal`,
`v-atlas`/`p-atlas`/`Second Brain`, `v-taygeta`/`p-taygeta`/`Notes\Daily/`,
`v-unflagged`/`p-plain`/``, `v-kelvin`/`p-kelvin`/`KELVIN` (U+212A).

| input | answer | why |
| --- | --- | --- |
| `p-merope` `notes/inbox/idea.md` | `v-merope` `inbox/idea.md` dir `inbox` | the ordinary case |
| `p-merope` `notes/idea.md` | `v-merope` `idea.md` dir `""` | at the vault root; `notes_tree` lists the root |
| `p-merope` `notes/journal/2026-01-01.md` | **`v-merope-journal`** `2026-01-01.md` | innermost wins; first-match would name a note id the outer vault has not got |
| `p-merope` `notes` / `notes/` | none | the vault directory is a folder |
| `p-merope` `notesy/x.md` | none | components, never a string prefix |
| `p-atlas` `Second Brain/a b/Meeting Notes.MD` | `v-atlas` `a b/Meeting Notes.MD` dir `a b` | spaces and capitals survive |
| `p-atlas` `second brain/lower.md` | `v-atlas` `lower.md` | APFS case-folds the compare; the ANSWER keeps the dirent's case |
| `p-taygeta` `notes/daily/x.md` | `v-taygeta` `x.md` | `Notes\Daily/` is normalised on both separators and both edges |
| `p-taygeta` `notes/x.md` | none | only `notes/daily` is that profile's vault |
| `p-plain` `anything.md` | none | an empty subfolder is no vault, not a vault at the root |
| `p-atlas` `notes/x.md` | none | a path is only relative to its own profile |
| `p-kelvin` `KELVIN/x.md` (U+212A) | `v-kelvin` `x.md` | exact bytes still match |
| `p-kelvin` `kelvin/x.md` (ASCII) | **none** | Rust folds ASCII only; `toLowerCase` would fold these together |
| `notes/../secrets.md`, `/notes/x.md`, `C:/x`, `\\srv\s`, `` | none | refused, not collapsed |
| `p-merope` `notes/a\b.md` | `v-merope` `a\b.md` | a backslash in a FILE name is a character (legal on Linux); in a configured SUBFOLDER it is a separator |

`file_path_for_note` is the inverse: it keeps the stored subfolder's own case
(the only spelling it has), normalises separators, and refuses the same paths.
Both suites round-trip every resolved row back through it.

## The four surfaces

1. **Note → file.** `Show in Files` in the `NoteEditor` header, present only when
   `filePathForNote` resolves. Absent — never disabled — for a vault list not yet
   read, a profile with no subfolder, or a note with no path.
2. **File → note.** `Open in Notes` in `TextFileViewer`, only for
   `entry.format === "markdown"` **inside** a vault. The registry's format, never
   an extension (AD-87). The note **id** comes from `notes_tree` on the file's own
   vault directory, matched exactly on path — a note id survives a rename and a
   path does not, so the index is the only thing that can say which note this is.
3. **Inherited item 1 — the CSV.** 45.4's `CSV_NEEDS_A_VAULT = null` and
   `PREVIEW_WITHOUT_A_VAULT` are gone. A CSV inside the vault now tables from a
   panel, addressed by the vault it resolved to. The assertion 45.4 named
   (`text-file-viewer.test.tsx`, "inside a notes vault") **is changed**, and its
   two other halves are kept: a CSV outside every vault still says why, and a CSV
   whose vault list has not been read yet still says why rather than guessing.
4. **Inherited item 2 — the dead prop.** `NoteEditor.onFollowLink` is deleted, not
   re-plumbed: no caller ever passed it, so a wikilink click reached `?.()` on
   `undefined` and did nothing since 37.6.

## Both halves of "look before you wire"

**`onFollowLink` was hiding an unexposed resolver.**
`NoteIndexSnapshot::resolve_link` has answered "which note does this link name"
since epic 37 — it is what the backlink map is built from, it folds through
`link_key`, and it answers to a note's id, its path, that path without `.md`, its
filename stem and its title, breaking ties by path order. **No command exposed
it.** So the honest wiring was one new command, `notes_resolve_link`, and not an
exact-match filter over `notes_link_targets` — that is a substring search for a
completion popup, and `index.rs:254` says in as many words that two definitions
of "what names this note" is a bug waiting to happen.

**And the external URL: keeper already had the capability.** `openUrl` from
`@tauri-apps/plugin-opener`, granted by `opener:default` in
`capabilities/default.json`, already used by `login-screen.tsx` and
`about-section.tsx`. The plugin's `allow-default-urls` scope allows exactly
`http`, `https`, `mailto`, `tel`, so `javascript:`, `file:` and `data:` are
refused by Tauri rather than by a guard of ours. **Decision: activate it.** The
frontend also names the refused scheme, because a note is agent-writable and a
rejected promise carrying the plugin's words about a scope the reader has never
seen is not an explanation. A rejection is surfaced as a sentence, never
swallowed — which matters because `capabilities/quick-capture.json` grants no
opener, so the identical press in a capture window is refused. (Main has since
ruled that grant be added; W3Capture/W3CaptureWindow own it.)

**And the affordance is now honest.** `.cm-lp-link` carried `cursor: pointer`
in every host while no host had a follower. Colour (`this is a link`) is split
from the pointer (`this one goes somewhere`); the pointer arrives with
`cm-lp-followable`, only when `onOpenUrl` was supplied. `markdown-preview.ts`
stopped turning a missing `onOpenLink` into `() => {}` — a fabricated value
standing in for a missing one.

## The defect this story found, and why it is the story's own shape

**`NoteBodyBatch::Reset` carried no `path`.** Only `Renamed` and a completed save
ever set `notesEditorStore.path`, so **a freshly opened note had none until its
first autosave.** Consequences already in main: the note header's path caption
(`{path ?? ""}`) has rendered empty on open since the frontmatter split, and this
story's headline control would have been **absent for every note anyone actually
opened**, appearing a second later. It passed the first test run as "absent,
correctly".

Fixed: `Reset` gains `path`, `notes_ipc.rs` sends the value it already had,
the store adopts it with `?? null` (a new required field on an existing variant
does not fail an old fixture — it delivers `undefined`, which passes every
`path !== null` gate while composing `"undefined/note.md"`). Binding regenerated
by the ts-rs export step, never hand-written.

Shape, contributed to the channel: **a field only ever written by a rare event is
indistinguishable from a field that is never written.** `Renamed` and save are
both real and both tested, so every test of the mechanism passed while the
ordinary path was never exercised. Ask who writes the field on the BORING path.

W2Attach then found the mirror symptom: **nine of eleven `reset` fixtures in the
tree were already passing `path`**, tolerated only because each literal ended
`} as NoteBodyBatch` and an assertion suppresses the excess-property check. Same
missing field, two symptoms, one cast hiding both. All nine casts dropped; those
literals are checked now rather than asserted past.

## Verification

`cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib vault_link`
— **EXIT=0, 12 passed**, re-run after every change.

`cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib` (the named
acceptance command) — **1712 passed, 1 failed, EXIT=101** at last reading (down
from 4 as siblings' sweep windows shut). The remaining red is not mine.
Earlier reading, kept because it names more owners: **1705 passed, 4 failed.** None mine, all
`notes::seed::tests::*` capture-tag assertions belonging to W3CaptureTag, who was
mid-sweep in `seed.rs`. `notes::vm` (which this story edits) and `vault_link` are
green. Reported as red rather than filtered down to a green subset.

`bun run test src/components/layout/ src/components/notes/ src/components/viewers/`
(the named acceptance command) — **EXIT=0, 1496/1496, zero unhandled errors** at
best observed, after the two fixture repairs below. Two of three repeats then
showed **2 failures each**, and I was stood down before attributing them; my own
files were 31/31 in isolation across those runs and 1496/1496 on the clean one,
so I am recording the repeats as **unattributed rather than as mine or as
somebody else's**. That is the honest state and it is not a green.

Two real fixture faults of my own were found by running this command rather than
a scope of my choosing, and both were load-dependent in a way that made them
look like the box:

**A fixture whose answer depended on a race.** The editor places the caret at the
END of the body absent a template hint, and `livePreview` gives the caret's own
line its source back — so my links, written on the last line, were decorated
only when the view was constructed before the opening `Reset` landed. Fast runs
built over an empty document (caret at 0, link decorated); loaded runs built over
the full text (caret on the link's line, no decoration, nothing to press). Four
tests passed for a hundred runs and failed together the first time the box was
busy. Every fixture now parks its link in the middle of the document. **A fixture
whose answer depends on a race is not a fixture.**

**And a raised timeout applied by a script, which missed one test.** The per-test
third argument is precise and forgettable; the one it missed then failed alone
under load and read as a defect in the code it was testing. Replaced with
`vi.setConfig({ testTimeout: CHUNK_TIMEOUT_MS })` at file scope — **a budget that
cannot be missed is worth more than one that is precise.** Same class of mistake
as the sliced file below: something a script produced and I did not re-read.

My scope, after the import cycle above was closed —
`src/components/notes/editor/` + `src/components/viewers/` +
`note-file-links.test.tsx` + `src/lib/vault-link/`: **610/610, EXIT=0, and
`Unhandled Rejection` count ZERO**, on two of three repeats. The single
intermittent on the third is `file-embed.test.tsx > turns a data embed into a
panel and leaves an ordinary wikilink alone` at 5004 ms — W2Embeds' known
load-dependent `waitFor`, zero of my symbols in it. Reported rather than
replaced with the best run.

Test-level timeouts in my own two files were raised to a named
`CHUNK_TIMEOUT_MS = 20_000` **because what is being waited for is the editor's
lazily imported CodeMirror chunk, not logic** — under eight concurrent suites
that import has been measured past `waitFor`'s 5 s default, which turns a red
into a measurement of the box.

`bunx tsc --noEmit` — zero errors in any file this story touches. The remainder
in the tree are siblings' (`sidebar-pane`, `leading-drawer`, `space-icons`,
`capture-document.test`, `notes-pane.test`, `settings-dialog.test`).

Sentinels: `MUTNF` returns **zero** across `src` and `src-tauri/crates`, by
literal substring, including the five new files a `git diff` cannot see
(W3Export's finding). Scoped diff read line by line on every shared file
(`notes-editor.ts`, `wikilink.ts`, `markdown-preview.ts`, `live-preview.ts`,
`note-editor.tsx`, `NoteBodyBatch.ts`): every changed line intended.

### Mutation table — 25 mutations, 25 caught (24 sweep, 1 audit; 3 closed after a repair)

| # | mutation | caught by |
| --- | --- | --- |
| M01 | `prefix.len() >= parts.len()` → `>` | `the_vault_directory_itself_is_not_a_note` |
| M02 | drop the longest-match guard | `the_innermost_vault_holding_a_file_is_the_one_that_answers` |
| M03 | drop the profile filter | `a_path_is_only_resolved_against_its_own_profiles_vaults` |
| M04 | `vault_dir` = the whole remainder | the vector table |
| M05 | stop refusing `.` / `..` | `neither_direction_composes_a_path_that_climbs_or_is_absolute` |
| M06 | drop the empty-subfolder guard | `an_empty_subfolder_is_no_vault_in_either_direction` |
| M07 | `asciiLower` → `toLowerCase` | the Kelvin-sign vector pair |
| M08 | mirror's `>=` → `>` | `refuses the vault directory itself` |
| M09 | mirror's `vaultDir` | the vector table |
| M10 | drop the empty-`profileId` guard | **SURVIVED**, closed — see below |
| M11 | `notes.find(path)` → `notes[0]` | `opens the note it names` (two notes in the fixture) |
| M12 | `notesTree(vaultId, relativePath)` | `toHaveBeenCalledWith("vault-1", "")` |
| M13 | `setActiveVault(profileId)` | `toHaveBeenCalledWith("vault-1")` |
| M14 | drop `setView("files")` | `opens the note's own file in the Files pane` |
| M15 | `openPanel` → `setActiveTarget` | `keeps the note open rather than replacing it` |
| M16 | offer Open in Notes outside a vault | `offers nothing for a markdown file outside the vault` |
| M17 | CSV target = the profile path | `toHaveBeenCalledWith("vault-1", "rows.csv")` |
| M18 | preview `vaultId` → null | **SURVIVED**, closed — see below |
| M19 | drop the `filePathForNote` guard | `offers nothing for a note whose profile carries no vault subfolder` |
| M20 | add `javascript:` to the grant | `refuses a scheme the opener grant does not carry` |
| M21 | swallow the opener's rejection | `says so when the OS refuses` |
| M22 | drop the "no such note" branch | `says so when nothing in the vault answers to the link` |
| M23 | drop `LINK_ATTR` from the mark | `hands a web link to the OS opener` |
| M24 | `path: batch.path` → `null` | `opens the note's own file in the Files pane` |
| M25 | report success without checking the vault switch landed | `refuses to navigate when the vault switch was refused` (audit probe, found a real defect — see below) |

**M10 survived** — the mirror's empty-`profileId` guard had no test (Rust's did).
A `file` target with an empty profile id is refused by `isRestorableTarget`, so
composing one is an action that works until a restart and then silently does not.
Closed with a test; re-run, caught.

**M18 survived, and the fix was a seam rather than a test.** `TextFileViewer`
composed the vault id **twice** — once for the wikilink follower and once for the
markdown preview — so blanking the preview's copy alone changed nothing any test
read. That is Main's prop-boundary shape at a boundary with no mock in it. There
is now **one** `const vaultId`, shared, so the two cannot be given different
answers; the mutation re-run against the shared const is caught by the wikilink
test. Honest narrowing: what is now enforced is that the preview and the follower
receive the SAME id, not that the preview's own consumer (a `![[…]]` embed inside
a `.md` file opened from Files) resolves against it. That consumer is 45.12's and
is not exercised here.

## Shape audit — 8 shapes, run after the sweep was green

1. **What composes the input?** `TextFileViewer` builds the `preview` and `csv`
   objects in production; the tests build `ViewerFile` by hand. Probed both →
   M17 (caught) and **M18 (survived)**.
2. **Did anything press the button?** Every offer here is pressed: Show in Files,
   Open in Notes, a wikilink, an external link, and a refused external link.
3. **A contract stated in a doc comment and enforced nowhere.** Ran W3Chrome's
   shape over four claims I inherited or wrote — `FilesFolderRoles::role_of`'s
   case-insensitive normalisation (**true**, `same_folder_path` lowercases),
   `panels.ts`'s `isRestorableTarget` refusal shape (**true**, four spellings),
   `notes_vault.rs` composing `format!("{subfolder}/{rel}")` (**true**, five
   sites), and 45.4's claim that its CSV assertion is the one 45.18 changes
   (**true**, and changed). All read, not assumed.
4. **A fallback for a case that cannot happen.** Found one and removed it:
   `markdown-preview.ts`'s `onOpenLink: options.onOpenLink ?? (() => {})`
   fabricated a follower so the decoration layer could not tell "no host" from
   "a host that does nothing". Removing it is what made the honest pointer
   possible. Re-probed the line after removal.
5. **Assert the fixture is what you think before asserting what it produces.**
   The vector table asserts its own minimum sizes AND that it holds ≥2 vaults on
   ≥2 profiles, ≥4 unresolved and ≥6 resolved rows — a one-vault table cannot
   tell a per-profile filter from an unconditional match.
6. **A branch reachable only from a second host.** The decoration layer has two
   hosts and this story wires both. Wikilink-following in a `.md` file opened
   from **Files** is tested in `text-file-viewer.test.tsx`, separately from the
   editor's — including the no-vault case, where the lookup must not happen at
   all rather than run against an empty vault id.
7. **Assert what you handed on, not only what came back.** Every action asserts
   the CALL and the state: `notesCsvRead("vault-1", "rows.csv")`,
   `notesTree("vault-1", "")`, `setActiveVault("vault-1")`,
   `notesResolveLink("v1", "Meeting")`, `openUrl("https://example.org/a%20b?q=1")`
   verbatim. Every collection fixture carries **two** items (two vaults on two
   profiles, two notes in every folder listing), so a `slice(0, 1)` or a wrong-row
   match has something to fail against.
8. **Count the doors.** Show in Files: 2 hosts (`NotesPane`, `PanelStrip`) — tested
   through the component both mount, 5 tests. Open in Notes: 1 host, 5 tests.
   Wikilink: 2 hosts, 2 + 2 tests. External link: 2 hosts — **3 tests, all through
   the editor, zero through the Files panel.** Named as a hole rather than
   papered over; the code path is identical (`markdown-preview` passes
   `onOpenUrl` through) but nothing presses it from the second door.

### Two more found after the sweep, both from peers' shapes

**A real defect, from W2Media's *two sequenced producers cannot share one slot*.**
`openNoteForFile` awaited `setActiveVault` — which **swallows a rejected
`notes_vault_set_active` into the mirror's error slot and returns normally**, so
awaiting it proves nothing. The sequence was: the switch fails and writes a
sentence, then this function navigates and returns `null` for success. The later
producer wins. The reader pressed **Open in Notes**, was moved to the Notes tab,
and found **no note open** — the pane only shows one while its vault is active —
with nothing on screen saying why. Now the outcome is read back from the mirror
rather than inferred from the absence of a throw, nothing navigates, and the
reason is shown where they pressed. Probed as M25: caught.

Generalised for the channel: **`await` is not a success check when the callee
catches its own failure.** A function whose contract is "never throws, records
the reason somewhere" turns every `await` on it into a no-op assertion, and the
compiler cannot tell you. `setActiveVault` is not the only function in this repo
shaped like that.

**A real import cycle, surfaced by this story and closed.** Every run of
`src/components/notes/editor/` and `src/components/viewers/` was carrying

    Unhandled Rejection: ReferenceError: Cannot access '__vite_ssr_import_6__'
      before initialization — Module.livePreview live-preview.ts:957 mermaidLayer(),

**`mermaidLayer` had nothing to do with it.** `live-preview.ts` imports
`tableLayer` from `markdown-table.ts`, and `markdown-table.ts` imported
`spliceBetween` back out of `live-preview.ts`; whichever Vite evaluated second
found the first one's bindings in the temporal dead zone, and `mermaidLayer` is
simply the next binding in the array `livePreview()` returns. It was
intermittent because which module loads first depends on which surface the
process reached first — and **this story made it common** by giving the markdown
preview a second host. Closed by moving `spliceBetween`/`TextSplice` into
`editor/text-splice.ts`, which imports nothing; `live-preview.ts` re-exports both
so every existing importer is untouched, and only `markdown-table.ts` changed its
import. `Unhandled Rejection` count across that scope went **1 per run → 0**.

### Absence-witness audit (W3Recording / W3Chrome / W3TagsDelete / W3Capture)

*An absence with no positive in the same representation is one assertion, not
two.* Ran mechanically over all three test files, comment-stripped. Every
load-bearing absence is witnessed by a positive using the **same literal and the
same query**: `queryByRole("button", { name: "Open in Notes" })` against
`getByRole` on the same name (and likewise "Show in Files");
`notesCsvRead`/`notesResolveLink`/`openUrl` `.not.toHaveBeenCalled()` against
`toHaveBeenCalledWith(...)`; `noticeText()` null against `toContain`;
`opened` empty against `toContain("note-7")`; the new
`not.toContainEqual({kind:"note",…})` against the success test's
`toContainEqual` on the same object. In the rule's own tests every refusal list
sits in a test that also asserts an accepted input (`notes/inner/a.md` beside the
three refusals), so the fixture cannot go hollow silently.

**One weak spot named rather than fixed**, and it is 45.4's rather than mine:
`queryByRole("tablist")` in the binary-file test has no positive on the same
literal anywhere. Deleting the toggle outright would pass it. Owed.

## Deliberately NOT done

- **No note is created for an unresolved wikilink.** Obsidian does; the `[[`
  completion source already offers "create and link". Following a link that
  names nothing says so instead, and creating a file as a side effect of a click
  on rendered text is a different decision.
- **No transclusion.** `![[note.md]]` stays excluded from the embed registry, for
  `file-embed.ts`'s own stated reason.
- **No per-row "Open in Notes" in the Files tree.** The panel header has only a
  path, so deciding "is this markdown" there would mean switching on an extension
  (AD-87). It lives in the viewer the registry already routed the file to.
- **No `NOTE_PANEL_LIMIT` change.** Per Main's ruling.
- **No case-insensitive fallback** when matching the vault path against the
  index's path. Both come from walking the same filesystem, so their case already
  agrees; a fallback would be a guess dressed as robustness and on a
  case-sensitive volume it would open a different note.
- **No panel-strip edit.** W3Export owns that header. But see below.

## What I could not verify here, and why

- **The `keeper` shell does not build on Linux (AD-55/AD-56).**
  `notes_resolve_link` has **never been compiled**. What is proved is the shape
  of the call and that the core function it delegates to (`resolve_link`) is
  covered by `keeper-core`'s own tests, which this story left untouched. Same for
  the one-line `NoteBodyBatch::Reset { path: entry.path.clone() }` in
  `notes_ipc.rs` and the `lib.rs` registration. **First check on the gate:**
  `cargo check -p keeper`, then open a note and confirm the header's path caption
  is filled in *immediately* rather than after the first autosave — that is the
  whole of the defect above, visible in one glance.
- **`openUrl` has never been called for real.** jsdom has no OS. What is proved
  is that the correct URL reaches the plugin verbatim, that a scheme outside the
  grant never reaches it, and that a rejection becomes a sentence. **Second check
  on the gate:** click a `https://` link in a note and confirm the browser opens;
  then click one in a quick-capture window and confirm you get a sentence rather
  than silence (until the `opener:default` grant lands there).
- **A real notes vault.** `notes_tree`, `notes_csv_read` and `notes_resolve_link`
  are exercised through injected doubles. The byte-level claims are
  `keeper-core`'s.
- ~~**An intermittent unhandled rejection I surfaced and did not cause.**~~
  **CLOSED** — it was the `live-preview` ↔ `markdown-table` cycle above, not
  mermaid, and it is fixed rather than deferred. The original characterisation
  is kept below because the misleading stack line will mislead the next person
  the same way. Under
  repeat runs of `src/components/viewers/`, roughly one run in two reports
  `ReferenceError: Cannot access '__vite_ssr_import_6__' before initialization`
  from `livePreview` → `mermaidLayer` (`live-preview.ts:957`), reached through
  `mountMarkdownPreview`. That is a **circular-import TDZ between
  `live-preview.ts` and `mermaid-widget.ts`**, pre-existing and independent of
  this story's logic — but this story is what makes it reachable often, because a
  markdown file opened from Files now mounts the preview where it previously
  refused. I moved `follow-link.ts` out of `components/notes/editor/` into
  `src/lib/notes/` to stop *adding* to that graph, which reduced but did not
  remove it. **This is the one thing in this story I would not let ship
  unlooked-at**, and it belongs to whoever owns `mermaid-widget.ts`: the fix is to
  break the cycle (a lazy `mermaidLayer` or a shared module holding what both
  need), not to raise a timeout. My three repeats therefore read 244/244, then
  242/244, then 243/244 — I am reporting the reds, not the best run.
- **No pixel, no compositor.** Whether the two new header controls fit beside the
  five already there at a narrow pane width is a browser fact.

## Files a reviewer should read first

`src-tauri/crates/keeper-core/src/vault_link.rs` (the rule and why it is here),
then `vault-link-vectors.json` (the contract), then
`src/lib/vault-link/actions.ts` (why both actions open *beside* rather than
replace — `setActiveTarget` on "Show in Files" would close the note you pressed
it from).
