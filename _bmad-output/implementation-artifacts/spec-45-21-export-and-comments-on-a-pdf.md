# Story 45.21 — Export, and Comments on a PDF

status: implemented (export) / **deliberately not built, with a measurement** (annotations)
epic: 45 (Open it, change it, put it back), wave 3
binds: FR-199, UX-DR83, AD-65, AD-90
agent: W3Export

---

## The two halves, and the verdict on each

| Half | Verdict |
|---|---|
| **Export a note or a file to a location the user picks** | Built, end to end, three crates plus two surfaces. |
| **Comments anchored to parts of a PDF, with a show/hide toggle** | **Not built.** Not "not finished" — the anchor the criterion needs cannot exist in the engine keeper ships, and I measured that rather than assuming it. Section "The annotation half" below, with the evidence and what it would take. |

44.8 is the precedent the brief named and it is the one I followed: find the
criterion unmeetable as written, say so with evidence, build the smaller honest
thing well.

---

## Check-whether-it-already-exists

Asked before building, per the epic's standing instruction.

**Found, and reused rather than rebuilt:**

- `@tauri-apps/plugin-dialog` — already a dependency, already used by six
  non-test surfaces. **No new dependency was added by this story.**
- `keeper_sync::files_write::collides` — the case-insensitive collision check.
  Reused verbatim, including its refusal, rather than writing a second one.
  APFS and NTFS are case-insensitive by default, so an exact-match check passes
  on this Linux box and destroys a file on the Mac it ships to; that argument is
  already made in that function's doc and it is the same argument here.
- `keeper_sync::browse::resolve` — the one containment rule. Every source path
  an export reads is re-resolved through it (AD-65).
- `keeper_core::notes::embed::candidates` — the embed viewer's own candidate
  order. An export that resolved embeds differently would carry a *different*
  file from the one the note renders.
- `keeper_core::notes::links::extract` — the one link grammar. It skips fenced
  code, drops anchors and percent-decodes; a private scanner here would get one
  of those wrong.
- `saveOpenNote` (`@/hooks/use-notes-body`) — lifted out of the hook by 45.14
  half an hour before I needed it, for a caller with no editor of its own.
  Exactly the flush an export needs.
- `syncErrorMessage` (`@/lib/stores/sync`) — the house helper for an `IpcError`
  message. This repo already had three copies of a structural `isIpcError`
  guard; it did not need a fourth.
- `revealPath`, the Sonner toast, and the `capabilities.revealInFileManager`
  gate — the archive export's (Story 5.5) idiom, followed rather than reinvented.

**Found and deliberately NOT used:** `keeper_core::archive::export`. Different
verb. That module *renders* a chat archive into markdown or JSON; this one
copies bytes and changes none of them. The name collision is real and both new
modules' docs say so.

**Searched and found nothing:** no `@tauri-apps/plugin-fs` anywhere in
`package.json` or `bun.lock`, and no existing note/file export command in Rust
(`grep` over `notes_ipc.rs`, `sync_ipc.rs`, `keeper-core/src/notes`). Every
"write where the user says" path keeper has — the archive export, the recordings
destination — picks a path in the webview and writes it in Rust.

---

## The epic labels this story "Frontend". It cannot be one.

`plugin-dialog` returns a string. **Nothing in the webview can write a byte
outside the app's own storage**, because `plugin-fs` is not a dependency and
adding it would be a new dependency for a capability Rust already has, with a
much wider blast radius than one command.

So export is a Rust feature with a frontend on it. The split follows the epic's
own rules about where a decision lives:

| Crate | What it decides | Compiles here? |
|---|---|---|
| `keeper_core::notes::export` | Which files a note needs. Pure, no IO. | Yes — 12 tests |
| `keeper_core::vm::ExportReceiptVm` | What the receipt says, word for word. | Yes — 5 tests |
| `keeper_sync::export` | Whether and where bytes may be copied, and what happens when a copy fails. | Yes — 17 tests over real temp directories |
| `keeper/src/{notes,sync}_ipc.rs` | Two thin command shims. | **No** — see "what I could not verify" |

`keeper-sync` rather than `keeper-core` for the copier, for `files_write`'s
stated reason: the shell does not build on Linux, so a containment rule written
there is a rule proved on no machine this is developed on — and `keeper-sync` is
deliberately `keeper-core`-free (AD-40), so the attachment list crosses that
boundary as `&[String]` rather than as a `keeper-core` type.

---

## The decision: what "export a note" means

The brief said to decide and say which. **A note exports as its own bytes,
unchanged, plus every file it embeds, at the same vault-relative paths, inside a
folder named after the note.**

```
~/Desktop/
└── Meeting/                      <- named after the note, never overwriting
    ├── Meeting.md                <- byte-identical to the vault's copy
    ├── attachments/photo.png     <- the path the note already names
    └── data/rows.csv
```

The two obvious readings both lose something:

- **The markdown alone** lands somewhere `![[attachments/photo.png]]` means
  nothing. That is precisely the failure this epic exists to end — keeper
  handing somebody the *name* of a thing instead of the thing.
- **The markdown with its links rewritten** to wherever the copies landed means
  the exported file is no longer the note. A note is a synced artefact people
  diff, review and copy back; an export whose bytes keeper silently edited
  cannot be compared against the vault.

So keeper does neither: it **reproduces the neighbourhood the links name**
rather than rewriting the links. Byte-identical *and* live, instead of one or
the other. The export folder is a miniature vault root — the note at its own
name, every attachment at the vault-relative path the note spells — which is
what makes the untouched link still resolve.

Three consequences, each deliberate:

- **A note always exports into a folder**, even with no attachments. The shape
  must not depend on whether the note happens to embed a picture today, or
  somebody's muscle memory breaks the day they add one.
- **A file exports as the file**, directly into the picked folder, no wrapper.
  keeper does not read a PDF's references or a spreadsheet's links, so there is
  no neighbourhood to reproduce and the export *is* the file.
- **An embedded note is not followed.** `![[Other Note]]` is an edge in the
  vault graph; following it makes an export of one note an export of an
  unbounded set. It is named in the receipt so the reader knows to export it
  separately — not silently dropped, and not reported as "missing".

---

## I/O and edge-case matrix

### `keeper_core::notes::export::plan` — which files a note needs

| Input | Output | Test |
|---|---|---|
| `![[photo.png]]`, `attachments/photo.png` on disk | carried as `attachments/photo.png` | `a_bare_embed_resolves_in_the_attachments_folder` |
| `![[photo.png]]`, BOTH `photo.png` and `attachments/photo.png` on disk | carried as `photo.png` — the viewer's own order | `a_bare_embed_prefers_the_path_as_written_over_the_attachments_copy` |
| `![[data/people.csv]]`, only `attachments/data/people.csv` on disk | **missing** — a slashed target is literal | `a_pathed_embed_is_never_looked_for_in_the_attachments_folder` |
| `![[b.png]] … ![[a.png]] … ![[B.PNG]]` | `[attachments/b.png, attachments/a.png]` — document order, folded dedup | `document_order_is_kept_and_a_repeat_is_carried_once` |
| `![alt](attachments/one.png)` and `[[attachments/two.png]]` | only the embed is carried; a mention is not a copy | `a_markdown_embed_counts_and_a_plain_link_does_not` |
| An embed inside a fence | not carried — documentation about embeds | `an_embed_inside_a_fence_is_documentation_about_embeds` |
| `![[Other Note]]`, `![[daily.MD]]` | reported as notes, not carried, not "missing" | `an_embedded_note_is_named_rather_than_carried_or_missed` |
| `![[.gitignore]]` | carried — a leading dot is a file name, never a title | `a_dotfile_embed_is_a_file_and_not_a_note` |
| `![[photo.png/index]]` | a note — the extension is read off the LAST segment | `the_extension_is_read_off_the_last_segment_only` |
| `![[gone.png]] ![[vanished.pdf]]` beside two real ones | two carried, two reported missing, export still happens | `a_missing_embed_is_reported_and_the_rest_still_go` |
| Vault configured with `media/` | resolves in `media/`, not `attachments/` | `the_attachments_folder_is_the_one_the_vault_configured` |
| A note with no embeds | an empty plan | `a_note_with_no_embeds_carries_nothing` |

### `keeper_sync::export` — whether and where the bytes may go

| Input | Output | Test |
|---|---|---|
| A note with two attachments, an empty destination | folder + 3 files; **bytes identical** including BOM, CRLF, NUL and invalid UTF-8; vault untouched | `a_note_exports_byte_identical_with_its_attachments_beside_it` |
| A note with no attachments | still its own folder, no `attachments/` created | `a_note_with_no_attachments_still_exports_into_its_own_folder` |
| One file | the file itself, no folder | `one_file_exports_as_itself_and_not_into_a_folder` |
| Destination already holds `meeting/` (note is `Meeting.md`) | refused, case-insensitively, **before** anything is written | `a_name_already_in_the_destination_is_refused_case_insensitively` |
| Destination already holds `PHOTO.PNG` | refused; the collider's bytes are untouched | `a_file_whose_name_is_already_in_the_destination_is_refused` |
| Destination gone | `DestinationMissing`, nothing created | `a_destination_that_is_gone_says_so_and_writes_nothing` |
| Destination is a file | `DestinationNotAFolder`, the file untouched | `a_destination_that_is_a_file_says_so` |
| **Destination is `chmod 0o555`** | `FolderFailed` / `CopyFailed` carrying the OS's own words; nothing left in it | `a_destination_that_cannot_be_written_says_so_in_the_os_words` |
| Destination inside the vault | refused — that copy would sync back as a duplicate | `exporting_into_the_folder_being_exported_from_is_refused` |
| Subpath `../out/secret.txt` | `Escapes` | `a_subpath_that_escapes_the_root_is_refused` |
| File gone between listing and export | `Missing`, destination empty | `a_file_that_is_gone_is_missing_rather_than_exported_empty` |
| Subpath names a folder | `IsDirectory` | `a_folder_is_not_a_file_to_export` |
| An attachment moved between plan and copy | refused, **and no folder left behind** | `an_attachment_that_is_gone_refuses_and_leaves_no_folder_behind` |
| An attachment readable at check time, `chmod 000` at copy time | `CopyFailed`, folder removed with the note already inside it | `a_copy_that_fails_halfway_removes_the_folder_it_made` |
| Two carried paths landing on one name | `Collision`, nothing left behind | `two_carried_paths_that_land_on_one_name_refuse_rather_than_overwrite` |
| `notes/Meeting.md` / `Meeting.md` / `a.b.md` / `.hidden` / `plain` | `Meeting` / `Meeting` / `a.b` / `.hidden` / `plain` | `the_folder_name_is_the_note_name_without_its_extension` |
| Every refusal variant | names its subject and finishes its sentence | `every_refusal_names_what_it_is_about_and_finishes_its_sentence` |

### `ExportReceiptVm` — what the person is told

| Input | Summary | Test |
|---|---|---|
| One file | `Exported clip.mov to /Users/alice/Desktop.` | `a_file_export_receipt_names_the_file_and_the_folder` |
| Note + 2 that landed | `Exported Meeting.md and 2 attachments to /out/Meeting.` | `a_note_export_receipt_counts_the_files_that_actually_landed` |
| Note + 1 / note + 0 | `and 1 attachment` / no clause at all | `a_note_export_receipt_is_singular_about_one_attachment_and_silent_about_none` |
| 2 missing + 1 embedded note | both caveats, both naming names | `a_note_export_receipt_names_what_it_could_not_find_and_what_it_would_not_follow` |
| 1 missing + 2 notes | singular / plural both correct | `a_single_missing_file_reads_as_one_rather_than_as_a_list` |
| 0 / 1 / 3 / 5 names | `""` / `a` / `a, b, c` / `a, b, c and 2 more` | `a_long_list_names_three_and_counts_the_rest` |

The count comes from **what landed** (`written.len() - 1`), never from the plan.
A receipt that counted the plan would say "and 2 attachments" about an export
that copied one.

### The frontend — `exportTarget` and the two controls

| Input | Output | Test |
|---|---|---|
| Note target, folder picked | `notesExport(vaultId, noteId, destination)` — asserted by value | `sends the vault, the note and the folder that was picked` |
| Any target | picker asked with `directory: true, multiple: false` and a title | `asks for a folder, one of them, under a title that names the act` |
| Dirty buffer | `notesSave(sub, buffer, rev)` **then** the export, in that order | `writes the buffer before Rust reads the file, and sends what the save left` |
| Dirty buffer whose save fails | refused; **no picker, no command** | `refuses rather than exporting a copy missing the edits it could not save` |
| A DIFFERENT note is open and dirty | no flush, no refusal, export proceeds | `does not flush, or refuse, for a note that is not the one in the editor` |
| Clean buffer | no save at all | `does not flush a clean buffer` |
| File target | `syncExportEntry(profileId, relativePath, destination)` — the WHOLE subpath | `sends the profile, the listing's own path and the folder that was picked`, `sends the whole subpath, not the file's own name` |
| Dialog cancelled | `{status:"cancelled"}`, **neither command called** | `calls no command at all when the dialog is cancelled` |
| Picker answers an array | treated as a cancel, not unwrapped to `[0]` | `treats an unexpected array answer as a cancel rather than unwrapping it` |
| Recording target | refused with a sentence, no picker | `has no export path for a recording, and says so instead of throwing` |
| Rust rejects | Rust's sentence, verbatim | `shows Rust's sentence verbatim and adds no words`, `surfaces a destination that cannot be written, in the OS's own words` |
| A rejection with no message | a finished fallback sentence, never `[object Object]` | `says something finished when the rejection carries no sentence` |

### The two doors

| Door | Host it is driven through | Tests |
|---|---|---|
| Note — the editor's Actions menu | a real Radix menu, and separately **the real `NoteEditor`** | 4 + 1 |
| File — a panel header | **the real `PanelStrip`**, on a file two folders deep | 5 |
| Both | one label; a file export does not flush an unrelated dirty note | 2 |

`export-in-the-note-editor.test.tsx` exists for one reason: epic 44 shipped
three tray listeners that were declared and never mounted, because `renderHook`
mounts the hook itself and can never see that `App` does not. That file mounts
the whole real `NoteEditor`, opens its own Actions menu through its own trigger,
and presses the item. Remove the child from the header and it fails.

---

## The annotation half

### What the criterion needs

"Comments anchored to parts of a PDF, with a show/hide toggle" needs three
things from the renderer: a coordinate space, a way to know which page is on
screen, and an anchor that survives the document being replaced.

### What the renderer gives — measured, not assumed

45.8 renders a PDF as `<embed type="application/pdf">` over `keeper-file://`,
routed to the platform's own renderer (PDFKit under WKWebView). I checked what
the page can reach inside it, in the engine keeper actually ships on rather than
in a mental model of it.

**WebKit's own IDL** (`Source/WebCore/html/HTMLEmbedElement.idl`, read from
WebKit `main`):

```
[Plugin, Exposed=Window, EnabledBySetting=EmbedElementEnabled]
interface HTMLEmbedElement : HTMLElement {
    attribute DOMString align;
    attribute DOMString height;
    attribute DOMString name;
    attribute USVString src;
    attribute DOMString type;
    attribute DOMString width;
    [CheckSecurityForNodeWithFrameOwner] Document? getSVGDocument();
};
```

Six reflected attributes and `getSVGDocument()`. **There is no
`contentDocument` and no `contentWindow`** — unlike `<iframe>` and `<object>`,
which have both. Confirmed independently against a spec-conformant DOM (jsdom,
via `"contentDocument" in element`): `embed` → `false`, `iframe` → `true`,
`object` → `true`. `getSVGDocument()` is same-origin-guarded and returns a
Document only for SVG content; a PDF is not SVG, and `keeper-file://` is a
different origin from the app document in any case.

So the chain is short and it is not a matter of effort:

1. **No document inside the embed is reachable from JavaScript.** Not
   restricted — absent from the interface.
2. Therefore **no text selection**: a `Selection` belongs to a document, and
   there is no document to select in.
3. Therefore **no coordinates and no page events**: scrolling, page turns and
   zoom happen inside the plugin, and the host page is never told.
4. Therefore **an overlay rectangle anchors to nothing.** A `<div>` over the
   embed can capture a drag, but keeper cannot read the embed's scroll offset or
   current page, so the rectangle would be pinned to *viewport* coordinates of a
   document whose position keeper cannot observe. It would drift the instant the
   reader scrolls — and it would drift silently, which is the worst version.

The epic's own fallback — "annotations live beside the file and anchor to a page
and a rectangle" — is therefore **half impossible**. The rectangle cannot be
drawn meaningfully. The page number could only ever be one the reader TYPES,
because keeper cannot know which page is on screen.

### So what would be left, and why I did not build it

A comment list beside the file, each comment carrying a page number the reader
types in. That is buildable. I did not build it, for a reason that is about
keeper and not about effort:

**A paragraph of prose about a document, anchored by a number a person typed, is
a note.** keeper already has notes; a note can embed the file, and 45.18 links a
file to its note in both directions. Building a second prose store beside the
file would mean a second write path for prose, with its own sidecar format, its
own sync and conflict semantics, and an anchor keeper cannot verify — inside an
epic whose AD-88 and AD-89 exist specifically to refuse a second path for
something that already has one.

And the failure mode is the one the brief named: a sidecar that is moved,
renamed or synced apart from its file loses the comments, silently. **A
half-built annotation layer that loses comments is worse than an export button
and a written-down reason.**

### What it would take

An epic of its own, and the first item is the expensive one:

1. **Replace the PDF renderer with one that runs in JavaScript** (pdf.js or
   equivalent). That is a new dependency of substantial size, it re-opens the
   zero-egress review, it means keeper renders pages itself — and the whole
   reason 45.8 chose `<embed>` was that a 400-page PDF then costs one element
   and no marshalling. A JS renderer costs a canvas and a text layer per visible
   page and a virtualiser to bound it.
2. **An anchor format that survives the document being replaced.** A page number
   plus a rectangle does not: re-exporting a document re-paginates it. The state
   of the art is a text-quote anchor (prefix / exact / suffix, à la W3C Web
   Annotation) with a page+rect fallback, which needs the text layer from (1).
3. **A store with sync semantics.** Where the comments live (a sidecar in the
   vault, or notes with structured frontmatter), what happens on a conflict, and
   what happens when the file is renamed — the same three questions
   `keeper_sync` answers for everything else, answered again for this.
4. **Only then the UI**: the overlay, the gutter, the show/hide toggle.

Recorded as a ledger entry so it is findable from the code rather than only from
this file.

### One thing I could NOT measure here, stated plainly

Everything above is about what the DOM exposes, and that part is conclusive on
WebKit's own source. What I could not run is the embed itself: the `keeper`
shell does not build on Linux (it fails in `glib-sys`' build script before
compiling a line of keeper's code) and jsdom renders nothing. 45.8's spec
already owes the macOS gate the check that a PDF's pages appear at all. If they
do not, this analysis is unaffected — the API surface is the same either way.

---

## Deliberately NOT done

- **An Export on a Files-pane row.** The row already opens a panel in one click,
  and the panel header exports. A second entry point is a second place to
  compose the target, which is exactly how 45.7's panel seam 404'd every media
  file in a subfolder while working at the profile root.
- **Exporting a folder.** `IsDirectory` refuses it with a sentence pointing at
  the file manager. keeper does not walk a tree it did not list.
- **Exporting several files at once.** The Files pane has a multiselection
  (45.3) and this does not consume it. One target, one export; a batch needs a
  progress surface and a partial-failure story, and it is a story.
- **A markdown file exported from the Files pane does not carry its embeds.**
  The Files surface addresses a file as a file. Export it from the note surface
  to get the neighbourhood.
- **Following an embedded note.** Named in the receipt, not carried. See the
  decision above.
- **Overwriting anything.** Never, under any flag. The destination is somebody's
  Desktop, not a folder keeper manages.
- **Exporting a recording.** `PanelTargetVm` allows it and no control offers it;
  `exportTarget` answers with a sentence rather than throwing, because a
  component crashing on a case the store's own type permits is worse.
- **A persistent failure dialog.** The archive export (5.5) uses one because its
  job is long and may leave partial output. This one is instant, leaves nothing
  behind by construction, and the person is looking at the control they just
  pressed — so a `toast.error` carrying Rust's sentence is the whole surface.
- **Any annotation code whatsoever.** Not a stub, not a disabled button, not a
  constant. The brief said build nothing and write down why; this file is that.

---

## What I could not verify here, and why

**1. Neither `#[tauri::command]` has ever been compiled.** `notes_export` and
`sync_export_entry` live in `keeper/src/notes_ipc.rs` and
`keeper/src/sync_ipc.rs`. The `keeper` crate does not build on this box —
`cargo check -p keeper --lib` fails in `glib-sys`/`gobject-sys` build scripts
(no `pkg-config`/GTK), before reaching keeper's own source. So the registration
in `lib.rs`, the argument names, the camelCase serialisation round trip and the
`#[cfg(desktop)]` gating are **unproved**. Both bodies are short and shaped
directly on their neighbours (`sync_read_text`, `notes_body_read`), and every
decision they delegate to is tested in `keeper-core` or `keeper-sync`.

*First check on the macOS gate:* `cargo check -p keeper`. Then open a note with
an image in it, Actions → Export…, pick the Desktop, and confirm a folder
appears with the note and the image inside it — and that the exported `.md` is
`diff`-identical to the vault's copy.

**2. No byte has ever crossed IPC for this feature.** The frontend tests mock
`notesExport` and `syncExportEntry`; the Rust tests call the engine directly.
The two halves are asserted against the same argument order by hand — the
command signature is `(vaultId, noteId, destination)` and the client sends
`{ vaultId, noteId, destination }` — but nothing has executed the join.

**3. The `<embed>` measurement is an API-surface measurement, not a render.** As
stated above: WebKit's IDL is conclusive about what JavaScript can reach, and no
PDF has been rendered on this machine by anything.

**4. macOS case-folding is untested.** `files_write::collides` folds with
`eq_ignore_ascii_case`, and my collision tests run on ext4 where `Meeting/` and
`meeting/` are two directories. On APFS the *filesystem* also folds, so the
refusal fires for a second reason. Both paths refuse; only one is exercised
here. This is `files_write`'s pre-existing property, not new.

**5. A destination on a different filesystem, a network share or a full disk.**
`std::fs::copy` handles cross-device copies, and a failure becomes `CopyFailed`
with the OS's words — but the only failures exercised here are permission
denied and unreadable source. ENOSPC mid-copy takes the same path by
construction and has not been run.

**6. The export has never been performed on a note that is actually open in the
editor with unsaved text**, end to end. The flush ordering is asserted against a
mocked `notesSave`; the real `saveOpenNote` writing through `notes_save` and
Rust then reading the new bytes off disk is two verified halves and one unrun
join.

---

## Verification

| Gate | Result |
|---|---|
| `cargo test -p keeper-core --lib notes::export` | EXIT=0, 12/12 |
| `cargo test -p keeper-core --lib vm::tests` | EXIT=0, 116/116 (5 of them this story's) |
| `cargo test -p keeper-sync --lib export::` | EXIT=0, 17/17 |
| `bun run test src/components/export src/lib/export` | EXIT=0, 35/35 (4 files; one is 5.5's pre-existing archive dialog) |
| `bunx tsc --noEmit` | zero errors in any file this story touches |
| new dependency | none |

Exit codes read from the process, never from a summary line: the first version
of this harness piped `cargo test` into `tail` and reported `EXIT=0` over
`test result: FAILED`, which is the exact failure the rule exists for.

---

## Verification, final

| Gate | Result |
|---|---|
| `cargo test -p keeper-core --lib notes::export` | **EXIT=0, 14/14** |
| `cargo test -p keeper-core --lib vm::tests` | **EXIT=0, 116/116** |
| `cargo test -p keeper-sync --lib export::` | **EXIT=0, 20/20** |
| `bun run test src/components/export src/lib/export` | **EXIT=0, 35/35** (4 files; one is 5.5's pre-existing archive dialog) |
| `bunx tsc --noEmit` | zero errors in any file this story touches |
| sentinel scan | zero `MUT45210` across `src` and `src-tauri/crates` |
| new dependency | none |

Exit codes read from the process, never from a summary line. The first version
of this harness piped `cargo test` into `tail` and reported `EXIT=0` over
`test result: FAILED` — the exact failure the rule exists for, hit on the first
try.

---

## Mutation table

Harness in `~/.W3Export/`, sentinel `MUT45210_NN`, unique in both directions,
every swap count-checked at exactly 1 in **both** directions, and the tree
scanned for the literal sentinel after every stop.

**36 mutations run, 35 caught, 1 accepted survivor.** Six survived their first
probe; five were real gaps and are closed, and each is written up below.

| # | Gate | Mutation | Verdict |
|---|---|---|---|
| 01 | core | `if name.starts_with('.')` → `if false` | **SURVIVED → the line was removed** |
| 02 | core | `eq_ignore_ascii_case("md")` → `== "md"` | caught |
| 03 | core | `None => true` → `None => false` | caught |
| 04 | core | `if !link.embed` → `if false` | caught |
| 05 | core | `if seen.contains(&folded)` → `if false` | **SURVIVED → closed, then caught** |
| 06 | core | `target.to_lowercase()` → `to_owned()` | **SURVIVED → closed, then caught** |
| 07 | core | `candidates(target, attachments_dir)` → hardcode `"attachments"` | caught |
| 08 | core | reverse the candidate order | caught |
| 09 | core | drop the `out.attachments.contains` guard | **SURVIVED → closed, then caught** |
| 10 | core | `link.target.trim()` → `as_str()` | **SURVIVED → the `trim()` was removed** |
| 11 | core | `if target.is_empty()` → `if false` | **SURVIVED → the guard was removed** |
| 12 | vm | `written.len().saturating_sub(1)` → `written.len()` | caught |
| 13 | vm | `" and 1 attachment"` → `" and 1 attachments"` | caught |
| 14 | vm | `.take(NAMED_IN_A_RECEIPT)` → `.take(1)` | caught |
| 15 | vm | `rest > 0` → `rest > 1` | caught |
| 16 | sync | `if !canonical.is_dir()` → `if false` | caught |
| 17 | sync | invert the destination-inside-source test | caught |
| 18 | sync | delete the note folder's collision check | caught |
| 19 | sync | `files_write::collides` → a case-SENSITIVE `exists()` | caught |
| 20 | sync | skip a carried path that will not resolve instead of refusing | caught |
| 21 | sync | delete the `remove_dir_all` cleanup | caught |
| 22 | sync | `if target.exists()` → `if false` | caught |
| 23 | sync | delete the `remove_file` cleanup in `export_entry` | **SURVIVED — accepted, see below** |
| 24 | sync | drop the `!stem.is_empty()` guard in `note_folder_name` | caught |
| 25 | sync | write the receipt path without the folder prefix | caught |
| 26 | sync | return the destination instead of the folder created | caught |
| 27 | ts | `typeof destination !== "string"` → `destination === null` | caught |
| 28 | ts | swap `vaultId` and `noteId` in the note command | caught |
| 29 | ts | send the file's name instead of its subpath | caught |
| 30 | ts | flush, then report success regardless | caught |
| 31 | ts | flush whatever note is open, not the one being exported | caught |
| 32 | ts | offer Export on every panel target, not only a file | caught |
| 33 | ts | delete `<ExportNoteItem>` from the NoteEditor header | caught |
| 34 | ts | make a cancelled dialog fall through to the error toast | caught |
| 35 | ts | offer Reveal regardless of the capability | caught |
| 36 | ts | reveal the first written file instead of the export root | caught |

### The six survivors

**01 — a special case that was doing nothing, and doing it wrong.** The
leading-dot early return in `names_a_note` changed no answer for `.gitignore`
(the ordinary arm already reads an empty stem and a non-`md` extension as a
file). Its *only* effect was on `.hidden.md`, which it called a **file** — and a
`.md` file is a note by every other rule in this codebase. Untested and wrong.
Removed; `a_dotfile_embed_is_a_file_but_a_dotted_note_is_still_a_note` now pins
both halves.

**05, 06, 09 — three guards, one untested promise.** *An embed is looked at once
however many times, and however many ways, a note names it.* Every duplicate
test in the file used embeds that **resolve**, and the resolved list
deduplicates itself — so the `seen` set, its case folding, and the second guard
beside the push were all invisible. The lists that push unconditionally are
`missing` and `notes`, and a receipt naming `gone.png` three times is a receipt
nobody reads twice. Closed by one test that names one absent file two ways, one
note two ways, and **two different targets that resolve to one path** — which is
the only input `seen` cannot cover, because `seen` folds the *target* and
resolution happens after it. All three now caught, each by that test.

**10, 11 — two lines with no input.** `link.target.trim()` and the emptiness
guard. `links::extract` is the one link grammar and it already trims both
spellings and drops a wikilink with no target, so neither line could be reached.
Proved by writing the test that would have exercised them
(`a_padded_or_empty_embed_target_is_handled_by_the_one_link_grammar`) and
watching it pass without them. Both removed; a guard with no input is a second
opinion about a rule that already has an owner.

**23 — accepted, and stated rather than closed.** Deleting the `remove_file`
after a failed single-file copy survives, because every copy failure this suite
can produce (unwritable destination, unreadable source) fails **before**
`fs::copy` creates the target. The case it defends is a copy that fails partway
— ENOSPC, a disconnected volume — which leaves a truncated file that the next
reader cannot tell from a short one. That is real and I cannot reproduce it on
this box without a loopback filesystem. Kept, with the survivor recorded here
rather than closed with a test that would only prove the mock.

---

## Shape audit

Run after the sweep was green. Nine of these are other people's shapes, traded
in channel during the wave; the sweep found the floor and this list is what was
above it.

| # | Shape (whose) | Applied to 45.21 | Outcome |
|---|---|---|---|
| 1 | **What composes the input?** (brief) | The note door's first tests built a Radix menu by hand. | **Real gap.** Added `export-in-the-note-editor.test.tsx`, which mounts the whole real `NoteEditor` and drives its own Actions menu. Mutation 33 (delete the child from the header) is caught only by it. |
| 2 | **Did anything press the button?** (brief) | Every door test clicks, and the Reveal action's `onClick` is **invoked**, not merely asserted present. | Clean. |
| 3 | **A contract stated in a doc comment and enforced nowhere** (brief) | Three claims checked. `export_note`'s "carried is deduplicated" → enforced by `Collision`. The module doc's "every source is re-resolved through `browse::resolve`" → the note's path was covered, a **carried** path was not. | **Real gap.** `a_carried_path_that_escapes_the_vault_is_refused_like_any_other`. |
| 4 | **A fallback for a case that cannot happen** (brief) | Five candidates probed. | **Three removed** (01, 10, 11 above); one kept and tested (`From<WriteRefusal>`'s unreachable arm, now asserted to keep its words); one kept and reported (23). |
| 5 | **Assert a fixture is opaque before asserting what reading it produces** (brief) | "Byte-identical" is only a claim about bytes if the fixture could catch a re-encode. | **Real gap.** `the_fixtures_can_actually_catch_a_re_encode` asserts `BINARY` is invalid UTF-8, holds a NUL and has no final newline, and that `AWKWARD_NOTE` opens with a BOM, holds a CRLF and ends in trailing whitespace. Without it a "binary" fixture of plain ASCII would make every byte assertion vacuous — wave 2's 37-byte deflate. |
| 6 | **A branch reachable only from a second host** (brief) | Two doors, counted: note = 4 hand-built + 1 real editor; file = 5, all through the real `PanelStrip`. Zero doors untested. | Clean by construction. |
| 7 | **Assert what you handed on, not only what came back** (Main / W2Media) | Every command asserted with `toHaveBeenCalledWith`; the file door driven through the panel so the assertion is about what `PanelFrame` composes, not what a test handed a button. | Clean — and mutation 29 (send the name instead of the subpath) is caught only by the panel test. |
| 8 | **The same shape at an uncompilable boundary** (mine, from 7) | `ExportReceiptVm::note` took `missing` and `notes` as two `Vec<String>` — **swappable, from a call site in `keeper/src/notes_ipc.rs` that compiles for nobody.** A swap would have produced a grammatical sentence about the wrong files. | **Real hazard, closed structurally.** It now takes the `NoteExportPlan` whole: one argument cannot be swapped with itself. The test asserts both caveats in one string so their order is pinned. |
| 9 | **Two-item collections** (Main / W2Attach) | `carried()` has two; the panel listing has two rows; the receipt fixture has two written entries; `named_list` is asserted at 0/1/3/**4**/5. | **One real gap:** the 4-item case. Mutation 15 (`rest > 1`) passes every 5-item test and is caught only by the 4. |
| 10 | **Count tests per entry point** (Main / W2Attach) | note 5, file 5, orchestrator 14. | Clean; shape 1 is what filled the note door's real-host hole. |
| 11 | **Mock completeness is a function of how long your tests run** (W2Attach) | The real `NoteEditor` mounts `TemplateUpdateOffer`, which calls `notesTemplateUpdatePreview` after a **four-second** idle timer. My factory did not define it. | **Real defect in the test.** On a busy box it throws an unhandled rejection *inside a passing test*. Added, with a comment saying it is reached only on a slow run so nobody trims it. |
| 12 | **`as T` on a fixture asserts it instead of checking it** (W3NoteFile) | `entry()` ended `as FilesEntryVm`. | **Real gap.** Dropping the cast revealed the fixture was missing `size`, `folderRole` and `write` — three fields the real listing always sends. Filled in; the compiler now reads the fixture. |
| 13 | **`mockRejectedValue` builds its rejection when CONFIGURED, not when called** (W3NoteFile) | Four of them. | Switched to `mockImplementation(async () => { throw … })`. No dangling rejection can outlive a test. |
| 14 | **A doc comment that overclaims** (W2Media / W2Attach) | `exportTarget`'s doc said "Rust words every sentence. Nothing here composes one." Three constants in the same file are keeper's own words. | **Narrowed**, naming the three and why each is a case Rust never sees because the command is not called. |
| 15 | **Who writes the field on the boring path?** (W3NoteFile) | `ExportReceiptVm.path` and `Exported.written` — every path. `plan.notes` — only when a note embeds a note, and both states are tested. | Clean, checked rather than assumed. |
| 16 | **An interrupted sweep is a crashed sweep** (mine, learned twice) | Stranded a live mutant **twice** — once when `hub stop` killed the runner between apply and revert, once when a tool timeout did. A `try/finally` cannot help; a SIGKILL does not run it. | Both caught within seconds by a literal sentinel scan. **`git diff` found neither the first time** — `notes/export.rs` is untracked, so a diff-based restore check is blind to it. Broadcast to the wave. |

### What the audit cost and returned

Ten TS mutations and twenty-six Rust ones is the sweep. The audit added six
probes and **five of the six survivors above came from it or from the shapes
that prompted it** — none from extending my own list. The two I would not have
found any other way are **08** (an argument swap at a boundary that compiles on
no machine I have) and **11** (a mock gap that only fires when the box is busy,
and presents as a timeout in an unrelated assertion).
