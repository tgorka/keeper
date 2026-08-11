# Spec 45.1 — A Panel Shows a Target

status: implemented
story: Epic 45, Story 45.1
bindings: FR-173, AD-90, UX-DR65
also touches: AD-58 (why there is one note panel), AD-65 / FR-145 (why no absolute path)

## Is this a new model, or the old one made addressable?

**New model, and the distinction is worth stating because the epic has been wrong about it before.**

What existed before this story:

- `src/lib/stores/primary-view.ts` — one enum (`inbox | archive | … | files | notes | settings`)
  saying which *surface* is on screen. There is no room for a thing in it and never was.
- `notesListStore.selected` — a single `{vaultId, noteId}` cursor, private to the Notes pane.
- The Files pane — **no selection at all**. It listed files and offered Reveal / Copy path / Open.
- The recordings browser — its own row cursor, keyed by `sessionId`.

Three surfaces each holding one slot, in three different shapes, and "open this beside that" was
inexpressible in all three. A panel list is genuinely new. What is *not* new is the fact those slots
held: the degenerate one-panel case. `notesListStore.selected` has therefore been **deleted rather
than left beside the panel list** — two places that both know which note is open is precisely how
they come to disagree.

**What did I find already there and dead?** Nothing panel-shaped: a search of `src/` for
`panel` / `openBeside` / `PanelTarget` found only `bbctl-panel`, `DetailPanel` and prose. The nearest
neighbour is `keeper_core::vm::NavState` (Story 14.4), which persists the phone stack's nav level in
Rust — deliberately ephemeral, phone-only, explicitly not a target vocabulary. Left alone. The thing
that *was* already there and unapplied is `FilesListingVm.detail`: Rust has composed "this volume is
not attached" / "something else is mounted there" / "this folder is not on disk" since 43.8, and this
story renders those verbatim in a panel rather than writing new sentences for the same facts.

## What was built

| Layer | File | What it owns |
| --- | --- | --- |
| Vocabulary | `keeper-core/src/panels.rs` (new) | `PanelTargetVm` — three variants, ts-rs exported |
| Model | `src/lib/stores/panels.ts` (new) | the panel list, its five operations, the cookie |
| Host | `src/components/layout/panel-strip.tsx` (new) | renders panels, resolves targets, says why not |
| Wiring | `src/components/layout/app-shell.tsx` | the restore effect; the strip beside the Files tree |
| Producer | `src/components/layout/files-pane.tsx` | single click / double click on a file row |
| Producer | `notes-pane.tsx`, `use-notes-actions.ts`, `use-notes-open-note.ts` | the notes cursor migrated onto the panel list |

### The target vocabulary

```rust
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PanelTargetVm {
    Note      { vault_id: String, note_id: String },
    File      { profile_id: String, relative_path: String },
    Recording { session_id: String },
}
```

Every field is one a producer supplies today, taken field-for-field from the row that supplies it —
`NoteRefVm` for a note (a note id is only unique within its vault, which is why `vault_id` is not
optional), `FilesEntryVm.relative_path` for a file, `RecordingHitVm.session_id` for a recording,
which is already the recordings browser's own row key.

Three deliberate absences:

- **No absolute path.** A panel is restored after a restart, and a stored absolute path comes back
  pointing at a volume that is not mounted there any more — or, worse, at a different one that is
  (AD-65, FR-145). Rust resolves a target when it is opened; an absolute path exists only as long as
  one action needs it, and it reaches a viewer only from a freshly resolved `FilesEntryVm`.
- **No name, size, kind or title.** Those describe what the target resolves to *right now*. A panel
  that cached them would render a stale name over a file that had been replaced.
- **No string address.** AD-90 spells the vocabulary `note:<id>` / `file:<vault>/<rel>`; it is not
  materialised, because a slash-joined address is ambiguous the moment a profile id ends in `/` or a
  path begins with one. `file_targets_with_a_shifted_separator_are_not_equal` is the test that keeps
  that restraint deliberate rather than accidental.

### The model

`panels: {id, target, replaced}[]` + `activeId`. Invariant: never empty.

| Operation | Rule |
| --- | --- |
| `setActiveTarget(t)` | the active panel now shows `t`; the list does not grow |
| `openPanel(t)` | append beside the active panel and focus it — with three exceptions below |
| `focusPanel(id)` | focus, changing nothing |
| `closePanel(id)` | refuses the last; focus moves to the panel that slides into the closed one's place, else the one on its left |
| `closeTarget(t)` | stop showing `t` anywhere; the last panel is *emptied* rather than kept |

`openPanel` does not append when: a panel already holds that exact target (focus it — two identical
panels are two views that can never differ); the target is a note and a note panel exists (below); or
the active panel is showing nothing (fill it rather than leave an empty frame beside the new one).

**`replaced`, and why a double click is not two clicks.** The DOM fires `click` before `dblclick`.
Without a memory of what the first click displaced, double-clicking B while reading A yields
`[B, B]` and loses A. So `setActiveTarget` records what it displaced and `openPanel` puts it back
before appending. A timer that swallowed the first click would do the same job and would make every
test of it a race. `replaced: {was: null}` is a third state and not the same as `replaced: null`: the
panel was previewing over *nothing*, so pinning keeps the target where it is instead of appending an
empty frame. A run of previews keeps the first `was`, so previewing three files and pinning the third
still restores the original document.

**`NOTE_PANEL_LIMIT = 1`, and it is not tidiness.** `notesEditorStore` is a module singleton holding
one buffer, one base and one `notes_open` subscription (AD-58). Two mounted `NoteEditor`s would take
turns owning it, and each would show the other's text under its own title — data loss, not a cosmetic
bug. The *model* refuses the second note panel rather than a surface declining to draw it, so no
surface can reach the state. Lifting it means making the mirror per-document, which **Story 45.15
needs anyway** (several capture windows, each holding its own note): one job, not two.

### Persistence

`document.cookie`, following `src/lib/column-widths.ts` — `localStorage` is refused across this
codebase and a panel arrangement is a lens the viewer arranged, not a fact Rust has a use for. Only
targets travel; ids are regenerated and `replaced` is transient. Restore is a single idempotent
`hydratePanels(document.cookie)` **mounted in `AppShell`** — not at module load (untestable) and not
inside the strip (the notes list retargets panels while the Notes surface is up and the strip has
never mounted, which would overwrite the remembered arrangement with one panel).

The budget is enforced at 3500 bytes and the shortfall reported at `console.info`, because a browser
drops an oversized cookie *silently*: the assignment succeeds, the value is not stored, and the next
launch comes up blank with no explanation.

## I/O and edge-case matrix

| Input | Result | Test |
| --- | --- | --- |
| single click on a file row | active panel retargeted, list length unchanged | `sets the active panel's target on a single click, without growing the list` |
| double click on a file row (with its preceding click) | the displaced file returns, the new one opens beside it | `appends a panel on a double click, keeping what was open`; `puts back what the click displaced` |
| ⌘ / ⌃ / ⇧ click | panel untouched — that gesture is 45.3's selection | `leaves the panel alone for a modifier click` |
| click on Copy path / Reveal / Open | panel untouched (the click bubbles to the row) | `leaves the panel alone when the click was on one of the row's own controls` |
| click on a folder row | expand/collapse only; never a target | `does not make a folder a panel target` |
| preview A, preview B, preview C, pin C | `[A, C]` | `keeps the panel that started a run of previews` |
| preview over an empty panel, then pin | that panel keeps it; no empty frame appended | `pins a preview in place when there was nothing under it` |
| double click a target already open elsewhere | that panel is focused; no twin | `focuses the panel that already holds a target` |
| a second note opened | the one note panel is retargeted | `retargets the one note panel instead of opening a second` |
| close the only panel | refused; the control is absent, not disabled | `refuses the last one`; `is absent on the last panel` |
| close a middle panel | the panel that slides into its place takes focus | `moves focus to the panel that slides into a closed middle one's place` |
| close the rightmost | focus falls to its left neighbour | `falls back to the left neighbour` |
| close a panel that was not focused | focus unchanged | `leaves focus alone when a panel other than the focused one closes` |
| note deleted | every panel holding it closes; the last is emptied | `stops being shown anywhere`; `empties the last panel rather than refusing` |
| restart | targets and the focused panel round-trip | `round-trips the arrangement and the focused panel through a cookie` |
| restart with the drive out | the panel is restored, *then* renders the reason | `restores an unresolvable target rather than dropping it` |
| restart, React double-invokes the effect | restores once, does not overwrite a click | `hydrates once` |
| corrupt / wrong-version / absent cookie | one empty panel; never a throw at boot | `comes up clean from a corrupt cookie` |
| cookie holds `../../.ssh/id_rsa`, `/etc/passwd`, `C:\…`, `\\server\…` | refused on the way in | `refuses a file path that is absolute or climbs out of its profile` |
| cookie holds `a/b..c/d.md`, `..hidden.md` | allowed — a dot is not a climb | `allows an ordinary relative path` |
| cookie holds a kind this build does not know | that entry dropped, the rest restore | `drops an entry whose kind this build does not know` |
| cookie's focused index out of range | clamped | `clamps a focused index that no longer names a panel` |
| arrangement too big for a cookie | the longest prefix that fits, reported at INFO | `remembers what fits and says how many it could not` |
| nothing open | the cookie is forgotten, not written empty | `forgets the arrangement when nothing is open` |
| file target resolves | drawn by 45.2's registry, from the file's own folder listing | `draws a resolved file through the registry rather than deciding itself` |
| drive out / volume unexpected / folder moved | Rust's own sentence, verbatim; the panel keeps its place | `renders Rust's own reason when the drive is out, and keeps the panel` |
| folder listed, file not in it | the sentence names the file | `names the file when its folder listed and it was not in it` |
| a non-`listed` state carrying `entries: []` | the state wins; never "this file is missing" | `trusts the state over the entry list` |
| `sync_browse` rejects | the IpcError message, composed in Rust | `shows the message Rust composed when the listing call is refused` |
| pointer lands in a panel | that panel takes focus, so the next single click replaces it | `focuses a panel the pointer lands in` |
| recording target | says keeper cannot show one yet, keeps its place | `says so for a target no viewer has been built for yet` |
| note target, vault gone | `PANEL_NO_VAULT_SENTENCE` | rendered path; see "could not verify" |

Resolution lists the file's **own folder** through `sync_browse` rather than asking for the file:
that command is the one directory reader (AD-74) and already carries the containment rule, the volume
check and the Rust-composed sentence for every way a folder can fail to be readable. A second command
that stat'ed one path would be a second place those rules live.

## Mutation table

Harness: `~/.W1Panels/mutate.py` (private to this agent — never `/tmp`), one mutation per
invocation, restore verified by comparison before writing.

Baselines, taken at exactly the scope of each verdict, before **and** after:

| Scope | Before | After |
| --- | --- | --- |
| `cargo test -p keeper-core --lib panels` | 5 passed | 5 passed |
| `panels.test.ts` + `notes-list.test.ts` + `panel-strip.test.tsx` | 49 → 50 passed | 50 passed |
| `files-pane.test.tsx -t "a row opens a panel"` | 5 passed | 5 passed |
| `app-shell.test.tsx` | 22 passed | 22 passed |

| # | File | Mutation | Verdict | Caught by |
| --- | --- | --- | --- | --- |
| M1 | `panels.rs` | fields serialise `snake_case` | CAUGHT | `panel_target_wire_shape_is_kind_tagged_and_camel_case` |
| M2 | `panels.rs` | tag renamed `kind` → `type` | CAUGHT | same |
| M3 | `panels.ts` | `setActiveTarget` appends instead of replacing | CAUGHT | `…does not grow` (+22 others) |
| M4 | `panels.ts` | pinning does not restore what it displaced | CAUGHT | `puts back what the click displaced` |
| M5 | `panels.ts` | `closePanel` allows closing the last | CAUGHT | `refuses the last one` |
| M6 | `panels.ts` | close never moves focus | CAUGHT | `moves focus to the panel that slides…` |
| M7 | `panels.ts` | `NOTE_PANEL_LIMIT = 2` | CAUGHT | `retargets the one note panel` |
| M8 | `panels.ts` | drop the `..` containment check | CAUGHT | `refuses a file path that is absolute or climbs out` |
| M9 | `panels.ts` | `hydratePanels` no longer idempotent | CAUGHT | `hydrates once` |
| M10 | `panels.ts` | cookie always records index 0 | CAUGHT | `round-trips the arrangement and the focused panel` |
| M11 | `panels.ts` | `closeTarget` returns without acting | CAUGHT | `stops being shown anywhere` |
| M12 | `panel-strip.tsx` | unresolved renders `null` | CAUGHT | the three reason tests |
| M13 | `panel-strip.tsx` | check `entries === null` only, not the state | **SURVIVED, then CAUGHT** | see below |
| M14 | `panel-strip.tsx` | `parentOf` always returns `""` | CAUGHT | `draws a resolved file through the registry` |
| M15 | `panel-strip.tsx` | close control always rendered | CAUGHT | `is absent on the last panel` |
| M16 | `files-pane.tsx` | modifier guard requires all three modifiers at once | CAUGHT | `leaves the panel alone for a modifier click` |
| M17 | `files-pane.tsx` | drop the "click landed on a button" guard | CAUGHT | `leaves the panel alone when the click was on one of the row's own controls` |
| M18 | `files-pane.tsx` | a folder becomes a target | CAUGHT | `does not make a folder a panel target` |
| M19 | `files-pane.tsx` | double click calls `setActiveTarget` | CAUGHT | `appends a panel on a double click` |
| M20 | `app-shell.tsx` | the restore effect never calls `hydratePanels` | CAUGHT | `restores the panels the last run left open` |
| M21 | `app-shell.tsx` | the strip is not mounted beside `FilesPane` | CAUGHT | same |

**M13 was a genuine survivor and is reported as one.** `resolveFrom` guards
`listing.state !== "listed" || listing.entries === null`, and every fixture for an unreadable folder
sent `entries: null` — so the second half alone satisfied them all and the state check was never
reached. Exactly the shape the epic describes: a fixture that cannot reach the boundary the code
branches on. The fix was a test, not a code change — a listing with `state: "missing"` and
`entries: []`, which is the case `FilesListingVm`'s own doc says the two fields exist to keep apart
("an empty array and a null are different in TypeScript"). Re-run: CAUGHT.

**Two incidents worth recording, because both are the failure mode the sweep guidance warns about.**

1. During the first (wide-scope) sweep a sibling agent wrote `files-pane.tsx` while the M17 mutant was
   in it. Their edit merged on top of the mutant, so the checksum restore correctly refused to
   clobber their work — and left `if (false) { return null; }` **live in the shipped file**. It was
   caught minutes later by its own test. The harness now un-mutates *surgically* over a concurrent
   edit (removing the mutant, keeping the sibling's work), and every mutant anchor is grepped for by
   name afterwards.
2. The M2 Rust mutant **regenerated `src/lib/ipc/gen/PanelTargetVm.ts` with `"type"` tags**, and
   restoring `panels.rs` did not restore the generated file — the binding stayed wrong until
   W1FilesWrite reported a `tsc` error against it. Regenerating bindings is now the last step of any
   sweep that mutates a ts-rs type. **A mutation harness must clean up a run's side effects, not only
   the file it edited.**

The three verdicts taken while `files-pane.test.tsx` had a failure that was not mine (a sibling's new
row control made an unnamed `getByRole("button")` ambiguous in a 43.8 test) were **discarded, not
reported**; M16–M19 were re-run at the narrowed scope above, with the baseline taken at that same
scope before and after.

## Deliberately NOT done

- **The strip is mounted for the Files surface only.** The Notes surface keeps its own single editor
  column, which now renders *the active note panel* rather than a cursor of its own. The model is
  shared and singular; the second host is not wired, because the note editor's singleton mirror means
  a strip in Notes could hold exactly one note anyway.
- **No recording viewer.** `recording` is in the vocabulary because AD-90 says so and because the
  recordings browser can already produce one; nothing in wave 1 opens one, and the panel says so in a
  rendered sentence rather than an empty frame. Story 45.19 is where the Recording surface starts
  producing them.
- **No keyboard model for the strip** — no ⌘1..⌘9 to focus a panel, no chord to close one. The story
  asked for the click behaviour and the close rules; a chord set belongs with 45.20's chrome work,
  where the cheat sheet and the native menu are edited together.
- **No drag to reorder, no per-panel resize, no split direction.** `flex-1` with a 280 px floor and
  horizontal overflow. 44.12's resizable-columns machinery is the obvious home for per-panel widths.
- **No Rust consumer of `PanelTargetVm`.** No command takes or returns one yet; the enum's job in
  wave 1 is to be the one vocabulary, generated into TypeScript. Story 45.18 is a resolution rule in
  Rust and is where the first Rust producer appears.
- **`closeTarget` is not called for a deleted *file*.** Story 45.3 owns the file delete path;
  `closeTarget` exists and is tested, and W1FilesWrite has been told it is theirs to call.

## What I could not verify here, and why

- **The packaged app.** The `keeper` shell crate does not build on this Linux box (glib-sys, no
  pkg-config), so nothing here was exercised through Tauri. Everything in this story is
  `keeper-core` (which builds and is tested) plus the webview (jsdom). No shell code was written —
  deliberately, per AD-55/AD-56.
- **The note panel body.** `NotePanelBody` mounts the real `NoteEditor`, and its vault-missing and
  vault-unhydrated branches have no rendering test: `NoteEditor` dynamically imports the whole of
  CodeMirror, and standing that up inside the panel suite would make it the note editor's suite. The
  model half is covered in `panels.test.ts` and `notes-list.test.ts`; the surface half is covered
  where it already lives — `notes-pane.test.tsx`'s "keeps the open note open when a filter excludes
  its row" and "does not clear the open note when the vault changes" now both run through the panel
  store, and both pass.
- **Real cookie eviction.** The 3500-byte budget is asserted against the value this code produces,
  not against a browser refusing a 4 KB cookie. jsdom's cookie jar enforces no size limit, so the
  *reason* for the budget is documented rather than demonstrated.
- **Two panels genuinely side by side on a real screen.** jsdom measures zero, so the layout is
  asserted only as classes. What is asserted for real is that two frames mount, that each renders its
  own resolved body, and that closing one leaves the other — the behaviour, not the pixels.
- **Modifier clicks on a real Mac.** My guard reads `event.metaKey || event.ctrlKey ||
  event.shiftKey` off a `MouseEvent` directly — no CodeMirror `Mod` resolution is involved, so the
  jsdom platform trap Main raised does not apply here. M16 (requiring all three modifiers at once)
  is CAUGHT, which is what proves the assertion is not vacuous.
