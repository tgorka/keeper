# Story 46.2 — Attachments lists what you attached

**Epic 46, wave 1.** Bindings: AD-103. Touches FR-145 (no absolute path in a
synced artefact, and therefore none in this panel), FR-152 / UX-DR56 (43.7's
panel), FR-188 / FR-189 (45.13's one insertion path), FR-199 (45.21's export
plan, whose classification rule this reuses).

Files changed:

- `src/lib/notes/attach.ts` — new `ATTACHMENTS_DIR`, new `bodyAttachments`, one
  private `namesANote`.
- `src/lib/notes/attach.test.ts` — 11 new cases for `bodyAttachments`.
- `src/components/notes/attachments-panel.tsx` — reads both sources; new empty
  state; two captions.
- `src/components/notes/attachments-panel.test.tsx` — 5 new cases, 2 rewritten,
  `notesAttachSources` + `@tauri-apps/plugin-dialog` mocked so the picker is
  pressable here.

**No `note-editor.tsx` hunk.** The panel already receives `frontmatter`,
`body={body.text}` and `onInsert` (`note-editor.tsx:663`), which is everything
this needs. Announced on `hub` and held to, so W1Flicker's save-caption hunk and
W1Delete's action-cluster collapse had that file to themselves.

**No new dependency.** `@tauri-apps/plugin-dialog` was already a dependency and
is only newly *mocked* in a test file that now clicks the picker.

---

## The defect

The panel named "Attachments" could not list an attachment. Not a stale-data
bug: it re-renders on every keystroke because the editor passes it the live
buffer. The data source was wrong, and structurally so.

1. `noteAttachments` read the `files:` frontmatter key and nothing else.
2. The component returned at the top — before reading the body at all — with
   *"This note has no session, so it isn't a recording note"* whenever
   `recordingSessionId(parsed) === null`.
3. An attach writes an embed into the **body** and never touches frontmatter.

So it was a recording-session panel wearing a general name, and the file the
owner had just attached could not appear in it by construction. The banner two
elements up said keeper had copied 1 file into `attachments/`; the panel under it
said the note is not a recording note. One gesture, two surfaces, opposite
answers.

---

## The decision: TypeScript, and not by reimplementing a parser

The story asked me to look hard for existing embed-scanning vocabulary and to
prefer exposing a pure Rust function over reimplementing in TS. I looked at all
three candidates. **TypeScript won, and it is not a close call — but the reason
is not "TS is easier".**

| Candidate | What it answers | Verdict |
| --- | --- | --- |
| `keeper_core::notes::export::plan(body, dir, exists)` (45.21) | "what must an export of this note carry" | **Cannot be used.** Takes a disk `exists` probe. |
| `keeper_core::notes::embed::candidates(target, dir)` (45.12) | "which vault paths may this target mean, in order" | **Rule reused, code not called.** Two lines, and its ordering is the reason a bare name is out of scope (below). |
| `extractLinks` in `attach.ts` (45.13) | "every link in this body, in document order" | **Reused verbatim.** No second parser was written. |

Three reasons Rust cannot answer this question:

1. **The buffer does not exist in Rust.** The panel reads the *unsaved* editor
   buffer — a row must appear on the keystroke after the insert, not after the
   next save. This is the exact argument 45.13's spec already makes for why
   `embeddedAttachmentNames` is mirrored at all (spec-45-13, "The mirror, and why
   there is one"). Nothing has changed about it.
2. **`export::plan` needs a disk.** Its whole shape is `&dyn Fn(&str) -> bool`,
   because containment needs a canonicalising `stat` (AD-55/AD-56). An IPC round
   trip plus a `stat` per embed per keystroke is not a panel, it is a load test.
3. **It answers a different question.** `plan` resolves `![[photo.png]]` to
   `photo.png` at the vault root when that file exists. The panel's question is
   "what did the attach path put in this note", not "what would an export copy".

So `bodyAttachments` is pure TypeScript in `attach.ts`, beside the scanner it
calls. **It adds no new grammar.** `extractLinks` — already pinned to
`keeper_core::notes::links::extract` by the 23-vector shared table — does all the
parsing: both embed spellings, code spans, escapes, anchors, percent-decoding,
external-URL rejection. The only new logic is nine lines of filtering.

### The one mirrored rule, and why it is mirrored rather than invented

`namesANote` mirrors `keeper_core::notes::export::names_a_note` (`export.rs:87`).
It has to be *that* rule and not a fresh one: if the panel called something an
attachment that `export::plan` calls a transclusion, the panel would show a row
for a file the export then refuses to carry, and the two receipts for one note
would disagree. Four lines, cited in the doc comment on both sides of the
question. It is not added to `attach-vectors.json` — see "Deliberately NOT done".

### Why a bare `![[photo.png]]` is *not* listed

This is the one place I narrowed the ask, deliberately, and it follows from
`embed::candidates`' own ordering:

```
candidates("photo.png", "attachments") == ["photo.png", "attachments/photo.png"]
```

The target is tried **where it is written first** and in the attachments folder
second. Which file a bare name means is therefore a question only a `stat` can
answer, and this function runs over an unsaved buffer with no disk in reach.
Listing it would put a row under "Attachments" for a file that may well sit at
the vault root. A prefix, by contrast, is a fact about the text.

And it costs nothing for the reported defect: `notes_attach_sources` writes
`attachments/<collision-free name>` — slash included — for every file it copies
in, which is every file that comes from outside the vault.

---

## I/O matrix

### `bodyAttachments(body) -> string[]`

Document order, deduplicated on the folded target, spelled as the note spells it.

| Body | Result | Why |
| --- | --- | --- |
| `intro\n\n![[attachments/photo.png]]\n` | `["attachments/photo.png"]` | the reported case |
| `![A photo](attachments/photo.png)\n` | `["attachments/photo.png"]` | both embed spellings count; only one is ever written |
| `![[attachments/b.png]]\n![[attachments/scans/a.pdf]]\n` | both, in that order | nested is still inside the folder |
| `see [[attachments/photo.png]]\n` | `[]` | a mention is not an attachment; `!` is the whole difference |
| `![[photo.png]]\n` | `[]` | ambiguous under `embed::candidates`; needs a `stat` |
| `![[recordings/2026/x/screen.mov]]`, `![[data/people.csv]]` | `[]` | a literal path elsewhere; `candidates` never routes it through `attachments/` |
| `![[attachments/notes.md]]`, `![[attachments/Some Note]]` | `[]` | `export::names_a_note` calls both notes |
| `![[attachments/.gitignore]]` | `["attachments/.gitignore"]` | extension `gitignore` is not `md`; no dotfile special case |
| `![[Attachments/photo.png]]` | `["Attachments/photo.png"]` | folder matched folded; the note's own spelling is kept |
| `![[attachments/photo.png]]` + `![[attachments/PHOTO.PNG]]` | one row | folded dedup, as `export::plan` dedups |
| `![[attachments/a/x.png]]` + `![[attachments/b/x.png]]` | two rows | dedup is on the target, not the name |
| ` ```…![[attachments/photo.png]]…``` `, `` `![[…]]` `` | `[]` | code is not a use |
| `# Title\n\nJust words.\n` | `[]` | — |

### `AttachmentsPanel` — which of the two lists renders

`session` = `recordingSessionId(frontmatter)`; `files` = the `files:` key;
`body` = `bodyAttachments`.

| session | files | body | On screen |
| --- | --- | --- | --- |
| yes | 2 | 0 | session list only, no captions. **Byte-identical to 43.7** |
| no | — | 1 | body list only, no captions ← **the reported bug** |
| yes | 2 | 1 | both lists, both captioned |
| yes | 0 | 1 | body list only, no captions, and no "list no files" sentence |
| yes | 0 | 0 | *"This recording note's properties list no files, so there is nothing to insert."* — kept verbatim |
| no | (ignored) | 0 | *"This note has no attachments — nothing in it embeds a file from attachments/. Attaching one adds it here."* |

`files:` is still read **only** when there is a session. A `files:` key in an
ordinary note is somebody else's list: it is relative to the recordings
destination root and an ordinary note has no such root. 43.7's test asserting
that is unchanged and still passes.

### What a body row offers, and the verb I did *not* invent

The story suggested "reveal / open". **The session rows offer neither.** They
offer exactly one control — `Insert` — replaced by the caption `In the note`
once the body holds the file. So:

- **No `Insert`.** The row exists *because* the body embeds it. The one label a
  session row wears once inserted is the only one this row could ever wear, so
  that is what it wears: `ATTACHMENT_PRESENT_LABEL`, the panel's own existing
  word for this exact fact. No new constant, no new verb.
- **No `Reveal` / `Open`.** Both need an absolute path. FR-145 is why this panel
  holds none — 43.7's comment says it does not even *have* one, "since it never
  acts on a file". Adding one means a new prop, a new IPC call and a new promise
  about acting on files outside the note. That is a story, not a row.
- **No kind word.** `kindOf` matches the session index **by name**. A session
  holding its own `photo.png` would label the vault's `attachments/photo.png`
  from a different file entirely. Reading the extension here instead would be the
  second classifier 43.5 exists to prevent. So a body row says nothing about what
  its file is, because keeper has not been asked.

### The two captions

Rendered **only when both lists are on screen**. The two are in different frames
— `files:` is relative to the recordings destination root, a body embed to the
vault — so a reader seeing both at once needs to know which is which. A heading
over a single list distinguishes it from nothing. This is also why they are two
lists and not one merged list: merged, it would be one column of paths with two
meanings.

---

## Edge cases

- **Empty / padded targets.** Not guarded here, deliberately: `extractLinks` is
  the one grammar and it already trims both spellings and drops a wikilink with
  no target. `export::plan` records the same decision, and records that a
  mutation proved its own guard was unreachable.
- **A body attachment whose name collides with a `files:` entry.** Two rows, one
  name. Correct and not deduplicated: the two are in different roots, so they are
  genuinely two files. Reachable only by hand-editing `files:` to name something
  under `attachments/`, which would mean the recordings root's attachments folder.
- **`targets === null` ("keeper can't locate this session").** Still attached to
  the session list only, and now explicitly so — it is a claim about the session,
  and the body's files were never looked for there.
- **The IPC call is still skipped when there is no session.** The `useEffect`
  guard is untouched; the early `return` that was removed was the render's, not
  the effect's. An ordinary note still makes zero IPC calls. Asserted.
- **A recording note whose `files:` list is empty but whose body has an
  attachment** no longer shows the "list no files" sentence, because the panel is
  not empty. The sentence is an empty state.
- **`.md` under `attachments/`** is a transclusion, not an attachment, by
  `export::names_a_note`. Consistent with the export receipt for the same note.

---

## Mutation table

Every mutation applied to the source, suite run, mutation reverted, and the
revert verified by `sha256sum -c` **and** by reading `git diff` — not by
remembering what I changed. A greppable sentinel (`MUTANT-W1ATTACH`) was written
into every mutation and grepped for after every stop.

**Two sweeps were interrupted** (one eval timeout at M9, one kernel death). The
first left M9's mutation live in `attach.ts`; the sentinel grep found it
immediately and it was restored before anything else ran. This is why the
sentinel exists.

| # | Mutation | Caught by |
| --- | --- | --- |
| M1 | `bodyFiles` always `[]` (the fix removed) | `lists an attachment an ordinary note embeds in its body`; `lists both sources for a recording note…`; `shows a recording note's body attachment even when its properties list no files`; `lists what the picker just attached, agreeing with the picker's own receipt` |
| M2 | `sessionFiles` always `[]` (43.7 removed) | **11 tests**, incl. `lists exactly the note's own attachments…`, `says what each file is, in the vocabulary Rust decided`, `hands out the text a user would type…`, `still lists and still inserts when keeper cannot locate the session` |
| M3 | empty state reverted to "isn't a recording note" | `tells an ordinary note it has no attachments, and says nothing about recordings`; `tells an ordinary note it has no attachments`; `lists what the picker just attached…` |
| M4 | `bothLists = false` (captions never render) | `lists both sources for a recording note, without changing the session rows` |
| M5 | body rows render `Insert` instead of the present label | `lists an attachment an ordinary note embeds in its body` |
| M6 | `attachments/` prefix guard removed | `does not list a bare name, because only a disk can say where it is`; `does not list an embed that names a path outside the attachments folder` |
| M7 | `namesANote` guard removed | `does not list a transclusion, by the rule the export plan uses` |
| M8 | folded dedup removed | `lists one row for a file the body embeds twice, in either case` |
| M9 | `!link.embed` skip removed (a mention counts) | `does not mistake a mention for an attachment` |
| M10 | folder match no longer folded | `reads the folder folded…`; `lists one row for a file the body embeds twice…` |
| M11 | `namesANote` calls an extensionless target a file | `does not list a transclusion, by the rule the export plan uses` |

**11 mutations, 11 caught, 0 survived.** M1 and M3 were re-run against the whole
file — including the editor-wired half — once `note-editor.tsx` compiled again;
both kill the end-to-end test, which is the one that presses the real picker.

M2 is the load-bearing one for "a recording note still lists its `files:` rows
unchanged": deleting that list fails eleven tests, none of them mine.

---

## Verification actually performed

```
bun run test src/components/notes/attachments-panel.test.tsx src/lib/notes/attach.test.ts
  → 2 files, 49 passed, EXIT=0 — three consecutive runs
npx tsc --noEmit          → no diagnostics in either changed file
```

`git grep MUTANT-W1ATTACH` → empty. `sha256sum -c` → OK on both files.

Per the wave constraint I did **not** run the full suite, the formatter or the
linter; Main runs those once. I did check every line I wrote against the 100-col
convention and wrapped the one JSX attribute list that exceeded it.

One transient red was **not** mine: `note-editor.tsx:677` threw
`ReferenceError: DropdownMenuItem is not defined` while W1Delete's menu collapse
was mid-edit, which reddened all 5 editor-wired tests in my file. Checked
`git diff` first, messaged them rather than working around it, and they landed the
import. Their fix to `editorWithPanel` (the `Attachments` control is now a
`menuitem`) also landed; the 49-green runs above are with both.

---

## Deliberately NOT done

- **A bare `![[photo.png]]` is not listed.** Argued above. It is the one place I
  narrowed AD-103's "embeds pointing into `attachments/`", and the narrowing is
  `embed::candidates`' own ordering rather than a shortcut.
- **An attach of a file *already inside* the vault, elsewhere, is not listed.**
  `notes_attach_sources` names such a file where it lies — `photos/a.png` — so it
  never acquires the prefix. AD-103 scopes the panel to `attachments/`, which is
  the folder the attach path *writes into*, and covering an arbitrary vault path
  means listing every file embed in the note. That is a wider panel with a wider
  name, and it should be decided rather than acquired. **Named as the one
  residual gap in the reported defect's neighbourhood.**
- **`[[attachments/photo.png]]` — a mention — is not listed.** Three existing
  readers in this feature (`embeddedAttachmentNames`, `export::plan`,
  `attach::embedded_attachment_names`) all draw the line at `!`. A fourth drawing
  it elsewhere is precisely how the two inserters happened.
- **`namesANote` is not added to `attach-vectors.json`.** The table pins one
  function pair, `embedded_attachment_names`, and adding a vector shape for a
  second rule means a new Rust entry point, new Rust test wiring and a second
  schema in one JSON file — inside a defect fix. The rule is four lines, cited by
  name on this side and unchanged on the Rust side. Recorded here as the thing to
  do if `bodyAttachments` grows a second caller.
- **`Reveal` / `Open` on a body row.** New prop, new IPC, new promise about
  acting on files. See above.
- **No repair of notes already written.** Nothing on disk is wrong; the reader
  was. There is no migration.
- **Not merged into one list**, and the session list's rows, wording and ordering
  are untouched.

---

## What I could not verify here, and why

**The shell crate does not build on Linux** (no GTK/webkit), so nothing in
`src-tauri/crates/keeper/` was compiled, run or type-checked by me. Everything
above is asserted against the pure reader in the webview.

**More sharply: no file has ever actually been copied into `attachments/` on any
machine.** 45.13's spec records this — `notes_vault::import_attachment` had no
caller for four epics and `notes_attach_sources` is its first one, and both live
in the uncompilable shell crate. My tests prove the *reader*: given a body that
embeds `attachments/receipt.png`, the panel lists it, and given the picker
answering `relPath: "attachments/receipt.png", copied: true`, the banner and the
panel agree. **That the copy happens, lands in that folder, and produces that
`relPath` is mocked here and has never run anywhere.**

### Gate checks, in order, on macOS

1. `cargo build -p keeper` — the only place `ATTACHMENTS_DIR`, `import_attachment`
   and `notes_attach_sources` are compiled at all.
2. `cargo test -p keeper-core --lib notes::export:: notes::embed:: notes::attach::`
   — the three Rust rules this reader is aligned to. Expected unchanged: I edited
   no Rust.
3. **The real thing, in the app, on an ordinary note with no `session:`:** press
   *Attach a file*, pick a file from `~/Desktop`. Assert **all four** together —
   this is the defect, and any three of them passing is how it shipped:
   a. the file exists at `<vault>/attachments/<name>` on disk;
   b. the body gained `![[attachments/<name>]]`;
   c. the banner says *"…outside the vault, so keeper copied it into
      attachments/…"*;
   d. **the Attachments panel lists it** — before saving, and with a count
      matching the banner's.
4. Repeat with a file **already inside the vault but not in `attachments/`**.
   Expected, and this is the documented gap: the banner reports no copy, the body
   gains `![[photos/a.png]]`, and **the panel does not list it.** Confirm that is
   the behaviour, then decide whether epic 46 wants it widened.
5. On a **recording note**: the `files:` rows still list, still say `video` /
   `file`, still insert, and a body attachment appears as a second captioned
   list. Then unplug the volume and confirm *"keeper can't locate this session"*
   still appears over the session list only.
6. `notes.capture_placement`-restored capture window: the panel is inside the
   editor, so confirm the two lists and their captions do not overflow at 560px.
   jsdom performs no layout, so nothing above measures a width.
