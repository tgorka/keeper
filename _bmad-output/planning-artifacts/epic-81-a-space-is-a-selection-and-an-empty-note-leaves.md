# Epic 81 — A space is a selection, and an empty note leaves

created: '2026-09-23'
source: the owner's five notes on keeper **0.8.32** (`main` at `f95905b`, `v0.8.32-4`, the build he is using; epics 79 and 80 shipped in it). The notes cover space definitions he wants treated as service files, a new note he wrote nothing in and wants gone, Back/Forward that ignores the active space, a space he cannot pick from each drive at once, and a caret that jumps on every save. Five read-only scout lanes triaged the notes one claim at a time (`local://triage-*.md`). Two deep-exploration digests turned the triage into designs (`local://bp-spaces.md`, `local://bp-closenav.md`). The coordinator pinned the decisions and ruled on each digest's pushback (`local://coordinator-decisions.md`, `local://impl-contract.md`). Two items were put to the owner before any design was pinned, and his answers are quoted below.
binds: FR-661…FR-676 and NFR-88…NFR-91 (allocated here, defined in *Requirements allocated here*; FR-677…FR-680 of this epic's block stay unallocated); AD-303…AD-307 (pinned by the coordinator, written out below in Binds / Prevents / Rule form); UX-DR111 (defined here, in *Decisions this epic takes*; UX-DR112…UX-DR115 of the block epic 79 reserved stay unallocated); DW-283…DW-289 (allocated in *What stays out*); D-24 (drafted at the end, for `docs/decisions.md`). **FR-660, NFR-87, AD-302, UX-DR110, DW-282 and D-23 were the previous ceilings** (epic 80's binds line; D-23 is `docs/decisions.md:1172`). No id here collides with epic 79 (AD-287…AD-296, FR-616…FR-645, NFR-80…NFR-84, UX-DR106…UX-DR110, DW-276…DW-278) or epic 80 (AD-297…AD-302, FR-646…FR-660, NFR-85…NFR-87, DW-279…DW-282, D-23). **Amends** AD-294's dispatch sentence, FR-629 (generalised, not reversed), AD-241 (a history entry carries a scope), AD-267's "the file name in any folder" (a second rule beside the names) and NFR-30 (one more named exception, D-24). It also closes the flush/close race that spec-46-12 named and left open. See *What earlier epics decided, and what this epic amends*.
see-also: epic 79 (AD-294, FR-629, Story 79.5, the rail per drive; Story 79.2, drift and reset; Story 79.9, the spaces folder as a setting; Story 79.11, the create chooser); epic 77 (spaces as saved searches, *Save as space*, DW-268); epic 76 (AD-267, FR-570…FR-572, Story 76.4, service files); epic 72 (AD-241, spec-72-5, per-panel history); epic 46 (spec-46-12, one subscription per note, and its named race); epic 45 (Story 45.14, capture's pristine pointer); epic 44 (Story 44.11, `keeper.limit` caps selection; DW-163); epic 42 (Story 42.4, the untouched stub is unlinked); `docs/notes.md` §"Service files" and §"Spaces and the query language".

## The owner's ask

Verbatim, in Polish, as sent, against build 0.8.32:

> uwagi ktore mam podczas uzywania keepera (uzyj bmad i pr ze stack pr do implementacji):
> - service files - by default dodaj jeszcze definicje space
> - jezeli stworze nowa notatke, ale nic w niej nie zapisze - usun ja
> - jak chodzisz wstecz/naprzod po notatkach - uwzglednij aktywne space
> - space daj mozliwosc zaznaczenia wielu drive i space z kazdego drive (to chyba wypadlo z implementacji ostatniej)
> - bug: jak notatka sie zapisuje to kursor wraca na poczatek lini (poprzedniej lini) a nie na miejscu

The parenthesis is the process: plan with BMAD and ship as a stacked PR set, which is the rung layout under *Stories*. Two items could be read two ways. The owner chose on 2026-09-23:

- **Item 4 → „Zaznacz kilka space'ów naraz"** ("select several spaces at once"). Multi-select happens in the rail: one or several spaces from each drive. The result is a union: drive A through space X, drive B through space Y. This is a wire change, a list of (drive, space) pairs in place of one `space_id`. The other reading, "the rail should show every selected drive", already shipped in 0.8.32 as Story 79.5, so his "it probably fell out of the last implementation" is answered: nothing fell out. The composite scope was never planned (FR-629 is deliberately single-space).
- **Item 3 → „Space podąża za historią"** ("the space follows history"). History stays per panel and keeps its shape. Going back or forward to a note that was opened under a different scope switches the active scope to that one. He did not choose "skip the notes outside the active space".

## What the triage found

In the owner's order. Each verdict uses the triage vocabulary: broken / unreachable / deliberate / absent.

| # | The owner's ask | Verdict | What the code says |
| --- | --- | --- | --- |
| 1 | service files should hide space definitions by default | **absent** | The service filter in `project_list` (`notes_ipc.rs:711-715` at `f95905b`) is a basename list (`keeper-core/src/notes/service_files.rs:4-15`, AD-267: "matched case-insensitively on the file name in any folder"). Space files have arbitrary names under the per-vault, configurable `spaces_prefix()` (`notes_vault.rs:146-152`, Story 79.9), and they pass `matches_filter`, whose default lens hides only conflict, archived and private (`notes_ipc.rs:439-501`). No recorded decision keeps them visible: `docs/notes.md:78-79` is a storage decision, not a list decision, and spec-76-4's stays-out waives only non-list surfaces. **Adding names to `DEFAULT_SERVICE_FILE_NAMES` would be wrong twice.** `inbox.md` would hide unrelated notes in any folder, and a persisted custom list never receives a new default (`registry.rs:1357-1369`). |
| 2 | delete a new note nothing was written into | **absent** | `create_note` writes eagerly (`notes_ipc.rs:3486-3487`). `notes_close` only unregisters (`:4016-4019`). No pristine record exists for ordinary notes. spec-44-6 §7 records no decision on this either way. Two precedents exist: `CaptureDraft::is_untouched` (`registry.rs:1463-1498`, Story 45.14: an untouched capture page is reused) and `dismiss_stub`'s byte-proven unlink (`ipc.rs:9213-9268`, spec-42-4:27-29: "an archive full of empty notes is worse than one with none"). |
| 3 | back/forward should respect the active space | **absent**, never considered | `navigate()` and `setActiveTarget` (`panels.ts:581-641`) never read the scope, which lives in `notesFiltersStore` (`notes-filters.ts:381-438`). AD-241 and spec-72-5 define history per panel, transient, and scope-blind. Spaces appear nowhere in them. |
| 4 | several spaces at once, one from each drive | **absent** (never planned) | `NoteQueryReq` carries one `space_id` (`keeper-core/src/notes/vm.rs:1252`). `project_vaults` resolves one lens against the active vault and applies it to every selected drive (`notes_ipc.rs:1581-1665`, `:551-571`). FR-629 plans exactly that. DW-268 (a space does not store drives) and DW-277 are neighbours. Neither is this. |
| 5 | the caret jumps to the start of the previous line when a note saves | **broken — the headline** | The reconcile effect keyed on `[base, openingCursor]` (`note-editor.tsx:857-863`) placed the template's `{{cursor}}` caret again on **every** save acknowledgement, because `markSaved` moves `base` (`notes-editor.ts:461-473`). `NoteDocument.cursor` was documented "consumed once" (`:150-153`) and nothing ever nulled it. All three shipped templates declare `{{cursor}}` (`templates.rs:594-643`). For the inbox template the hint is the start of the blank line after the heading, which is the owner's "start of the previous line". The same effect had a second defect: `applyExternal(base)` after `markSaved` spliced away keystrokes typed during the save round trip. The splice is remote-annotated, so it was never reported back. Rust was correct: `take_caret` is once per open (`a_caret_hint_is_taken_exactly_once`). |

## The one sentence

**A caret that jumps back on every save, space definitions listed as notes, empty new notes that pile up, a Back button that leaves the list in a space the note is not in, and one space where the owner wants one from each drive. The fix: a save stops reaching the editor, a space's definition becomes a service file, a new note nobody wrote in leaves with its editor, history carries its scope, and a scope becomes a set of spaces, each searched in its own drive.**

## What earlier epics decided, and what this epic amends

Nothing here reverses an owner decision. Each amendment below extends or narrows an earlier rule. The table quotes the earlier rule and gives the reason for the change.

| The earlier decision | What it said, verbatim | What the owner found | The amendment |
| --- | --- | --- | --- |
| **AD-294's dispatch** (epic 79) | "the list command is called with the scope's vault (the lens) and the selection (the search set)" | A scope that names spaces on two drives has no single "scope's vault". | **AD-306.** Every lens names its own drive on the wire (`NoteSpaceRefReq`). `notes_list` is always called with the active drive. The drives searched are `search_drives(home)`, which keeps 79.5's acceptance: with an empty selection, a Work space searches Work alone. |
| **FR-629** (epic 79) | "A space entered from any drive's group is searched in that drive (and across the selected drives when several are selected) through that space's query" | He wants drive A through X *and* drive B through Y. | **AD-306, generalised.** One space still does exactly this, byte for byte. With several spaces, a drive is narrowed by the spaces that live on it, or by all of them when none does. |
| **AD-241** (epic 72) | "Navigation history is per panel, and it belongs to the viewer … the stacks are capped … and are not persisted" | Back lands on a note outside the list's scope while the rail still marks another space. | **AD-305.** Each entry also remembers the scope it was followed under, and a step onto a note restores that scope. The rule stays per panel, capped at 50, and never persisted. |
| **AD-267** (epic 76) | "matched **case-insensitively on the file name in any folder**" | The files he means have arbitrary names in a configurable folder. | **AD-303.** A second rule sits beside the name list: the entries the index flags `space`, in the default lens only. The names, the keys and their defaults are untouched. |
| **spec-46-12's named race** | "If Rust ever processes `notes_close` ahead of the `notes_save` that preceded it, the last keystrokes before a panel closes are lost … Named rather than fixed: fixing it means awaiting a write on a teardown path, which is a different decision." | Deleting an untouched note on close makes that ordering a data-loss question. It is no longer a lost-keystroke nuisance. | **AD-304** closes the race without an await. The last words ride inside the close, and a per-subscription gate orders every write. |
| **NFR-30** (phase 5) | "No keeper code path deletes or overwrites a note body without leaving a recoverable copy" | A trashed empty note is still an empty note, under another name. | **D-24.** A second named exception, on Story 42.4's reasoning and with 42.4's discipline: byte proof, and every uncertainty keeps the file. |
| **DW-N1's caret machinery** | the store "applies it both at EditorState.create and in the reconcile effect" | The caret went back to the hint on every autosave. | **AD-307.** The hint is placed once and consumed. The editor reconciles on content that did not come from it, never on a save acknowledgement. |

The coordinator's own pinned texts for AD-304, AD-305 and AD-306 were refined by the rulings on the two digests. Each AD below names the sentence it refines and why.

## Decisions this epic takes

The rules below are the plan. The review wave changed AD-303, AD-304, AD-306, AD-307 and UX-DR111, and refined AD-305's restore, in the code that shipped. Read *Review-wave amendments* (A1–A11) with them: where a rule here and an amendment disagree, the amendment is what shipped.

- **AD-303 — A space's definition is a service file of the notes list.** **Binds:** FR-663, FR-664, NFR-90; Story 81.2. **Prevents:** space definitions listed among notes in the plain list; a basename entry that hides unrelated notes in any folder; a new default that persisted `notes.service_file_names` lists never receive; a literal `spaces/` when the folder is a per-drive setting (79.9); a space whose own query selects space definitions coming back empty; a rail left stale because the edit it should reload on moved no row and no count (bp-spaces D4). **Rule:** When `hide_service_files` is on (the default), the **default lens** drops every entry the index flags `space` unless the query asks for `is:space` by name. The flag is what `parse_note` stamps against the drive's own `spaces_prefix()`, and it already leaves out the reserved `index.md` and `log.md`. Asking by name is the rule `conflict` and `archived` already follow. The pinned text said "a path-prefix rule next to the basename list". The shipped rule reads the flag that the prefix produced. That is the same set, with no second path test that could drift from the index (`service_files.rs` `hides_space_definition(flags, asked)`). The rule applies to the default lens only. Inside a space lens nothing changes, and neither does a folder scope, which is a `notesTree` listing and not a lens. The Files tree, ⌘⇧F, links and the rail are also unchanged, so FR-571's contract holds. Hidden definitions are counted in the existing `hidden` (FR-572). `DEFAULT_SERVICE_FILE_NAMES`, `notes.service_file_names` and `notes.hide_service_files` are untouched. There is no wire change and no migration. A hidden edit moves no row and no count, so the changes stream keeps a revision of the drive's space definitions. `space_definitions_revision` hashes the `space`-flagged entries' path, size, mtime, inode and flags, and lives in process memory only. `stream_changes` sends a batch when the revision moves, even with no ops and the same counts. The webview already reloads the rail on every batch (`use-notes-changes.ts` `requestSpacesReload`), so no wire field is added. Sessions' `_spaces/` are out (DW-288).

- **AD-304 — An untouched new note is removed when its editor lets it go.** **Binds:** FR-665, FR-666, FR-667, FR-668, NFR-89; Story 81.3; D-24. **Prevents:**
  - an archive of `YYYY-MM-DD-untitled.md` files nobody wrote in;
  - a removal that races the last save. Flush and close are two unawaited IPCs today, and [INFERENCE: bp-closenav Q5] Tauri 2 does not order separate async commands;
  - a save that lands after the release and recreates the file;
  - a wikilink *create and link* stub deleted behind the link that was just written;
  - a different note deleted because it reused a freed untitled filename;
  - a pinned, tagged or propertied note deleted because its body is empty;
  - a capture page or a journal entry removed;
  - a trash copy that leaves the empty note behind under another name.

  **Rule:**
  - **Which notes are eligible.** Rust records a pristine pointer, `registry::PristineNote { note_id, path, document }`. `document` is the whole file creation wrote, read back from disk, never assembled: capture's rule, because a `{{now}}` template makes the bytes unrepeatable. A pointer is recorded for a create that `NoteCreateReq::writes_nothing()` (no title and no body, where blank counts as none) made through `notes_create` or `tray_new_note`. That covers:
    - the pane's and the phone pane's *New note*;
    - the palette;
    - ⌘⌥N;
    - a space row's *New note in {space}*;
    - the tray;
    - *New note from search* with an empty prompt.

    `create_note` itself records nothing, so capture (`resolve_capture_draft`, which reuses) and the journal (`create_journal`) are excluded by construction.
  - **Where the pointer lives.** In memory in `PRISTINE`, keyed by note id, and in one registry row per drive, `notes.pristine.<vault_id>` (a `KeySpec` family, `Scope::SessionState`, `Settable::Never`).
  - **What untouched means.** `PristineNote::is_untouched(rel, disk)` holds when:
    - the path is the one creation wrote;
    - the frontmatter block equals creation's once `updated` is removed from both, so a pin, a tag, a property, or another note's `id` at the same path all differ;
    - the body is equal apart from surrounding whitespace (CaptureDraft's rule).

    Typing and then deleting back to nothing is still nothing written, and the pointer lives until the release.
  - **The release.** `notes_close(subscription_id, release: Option<NoteReleaseReq { text: Option<String>, base_rev }>) -> Result<bool, IpcError>` carries the last editor's unsaved words and the decision in one command. In order:
    1. It takes the subscription's `released` gate. The gate is held across every write through that subscription, so a `notes_save` either completes before the release or writes nothing.
    2. It flushes `text`. The flush is skipped when the disk body already equals `text`, so an autosave that has already landed is not written a second time and turned into a conflict copy.
    3. It removes the subscription and counts the note's remaining body subscriptions in one critical section.
    4. Only when it was the last subscription and carried a release does it run `discard_if_untouched`. An orphan close (`release: None`) never removes. `discard_if_untouched` reads at the subscription's current `rel` (renames are tracked by id) and compares. On a match it unlinks the file: `unlink_untouched` in `notes_ipc.rs` runs `remove_file` on the contained path, then `touch` + `mark_dirty` exactly as `trash_note` does. The row then leaves the list through the ordinary index path, and the removal commits. Since A1 the release must also say `discard: true`.

    Every other outcome keeps the file and forgets the pointer: any difference, a read error, a rename, or a failed flush (logged at WARN, returned as `Err`). The answer `true` means the note was removed. The webview then calls `closeTarget`, and the note leaves every panel's history (R3, see 81.3 and 81.5).
  - **Crash and quit.** At every vault registration, `sweep_pristine` handles the rows a previous run left, meaning ids not in `PRISTINE`. If a restored panel has already subscribed to the note, the sweep adopts it and that editor's release decides. Otherwise an untouched note is removed. The row entry is dropped either way. The sweep does no vault IO under the `PRISTINE` lock.
  - **Unlink, not trash** (D-24).

  **Refinements of the pinned text** (each is a coordinator ruling on bp-closenav):
  - *"with no caller-supplied body"* becomes *no title and no body* (P3). The wikilink stub sends a title and is never opened, so it would hold a pointer that no release clears, and the next sweep would delete the note behind the new link.
  - *"on-disk body trimmed equals the pristine snapshot trimmed"* becomes *path equal, block equal minus `updated`, body equal trimmed* (P4). The untitled filename is reused after a removal, and pins, tags and properties never touch the body.
  - *"after the frontend's final save has resolved"* is met in Rust (P5). The final save travels inside the close, so no teardown path awaits anything, and spec-46-12's race closes as a side effect.
  - *"keyed by note id"* stays true inside one row per drive (P8). The settings table cannot enumerate a prefix, and a row per note would leave one cleared row for every note ever created.
  - *"session space create"* is not a notes-vault create (P2). `sessions_file_new_kind` writes a session file that opens as a `file` target with no body subscription, so it is out (DW-285).
  - A folder that a space's seed created stays when its only note is removed (Q1), because the folder may be the person's.

- **AD-305 — History remembers the scope a note was opened under, and going back restores it.** **Binds:** FR-675, FR-676, NFR-91; Story 81.5; UX-DR111 (the rail rows it validates against). **Prevents:** Back landing on a note that the list's scope does not contain while the rail marks another space; a Rust membership query per history step (membership is a Rust-evaluated DSL predicate, and skipping entries would need a new command, triage option b, which the owner did not choose); a restore that ANDs the previous space's restored chips onto the new scope (X's query ∩ Y's chips, usually empty); a search the person typed wiped by a step back; a scope restored onto a space that no longer exists (the list would answer `NotFound`); a stamp written into the panels cookie. **Rule:** `panels.ts` history entries become `PanelHistoryEntry { target: PanelTargetVm; scope: NoteScope | null }`, and `Panel` gains `scope`, the stamp of its current target. The stamp is `notesFiltersStore.getState().scope`, taken inside `setActiveTarget` (AD-241's one funnel), in `appendBeside`, and in `openPanel`'s fill-empty branch. It is `null` for a target restored from the cookie and for an empty panel. The stamp is an opaque `NoteScope` value, so AD-306's list-shaped space arm costs nothing here. `navigate()` (back, forward, a direct jump) moves entries as today. After the state write, if the landed target is a note and its stamp is not `null`, it calls `restoreScope(stamp)`. That is the merged rule from the impl contract:
  1. If `sameScope(current, stamp)` holds, do nothing. `sameScope` is AD-306's exported, ordered `(vaultId, id)` equality.
  2. Every space in the stamp must be a row of `railSpaces` (matched by `vaultId` and `id`) with `error === null`. Otherwise do nothing. `railSpaces === null` also means do nothing for a space stamp; that covers the phone and a rail not yet read.
  3. If the bar is untouched (an entered space that has not drifted, or an empty bar with nothing entered):
     - exactly one space: `enterSpace(row)`, as a rail click does, with no park and no `notesSpaceTouch`;
     - any other stamp: apply the stamp with a cleared bar.
  4. If the bar is touched: apply the stamp with `enteredSpace: null` and `sort: null`, and strip the outgoing entered space's restored terms (`withoutRestored`). The person's own chips and text stay.

  Non-note targets carry stamps but never apply them. Stacks stay per panel, capped at 50, and transient (AD-241). `PanelTargetVm`, Rust, the cookie and `PANELS_VERSION` are untouched. `railSpaces` holds the rail's rows, lifted from `SpaceList`'s component state into `notesFiltersStore` through `setRailSpaces`. It is not a filter, so `clearAll` leaves it. **Refinement of the pinned text:** *"scope only — the bar text/chips are not re-restored"* gives the wrong list in the common case when held literally. Entering a space writes its restore into the bar, so changing only the scope leaves the previous space's chips ANDed on, and `spaceDrift` reports a drift the person did not cause (bp-closenav P1; bp-spaces P6). The merged rule therefore enters the space only when nothing in the bar is the person's, and otherwise strips only what the outgoing space itself put there. "Space follows" holds, and nothing the person typed is ever removed. That is the owner's "not search resets".

- **AD-306 — A scope is a set of spaces, each searched in its own drive.** **Binds:** FR-669, FR-670, FR-671, FR-672, FR-673, FR-674, NFR-88; Story 81.4; UX-DR111. **Prevents:**
  - a second space from another drive that is reachable only by replacing the first;
  - a space from drive B resolved against drive A's snapshot. This becomes structural: each lens is resolved once, on its own drive;
  - TypeScript evaluating a space query (AD-20/AD-58: Rust owns membership);
  - a union that lists more of X than X lists alone, which happens if caps are ignored;
  - a union ranked by comparing scores computed from different texts;
  - one saved space pretending to carry per-drive meaning (DW-268 stands);
  - a synthetic id (`keeper:all`, `keeper:temporary`, `keeper:group:*`) inside a selection.

  **Rule:**
  - **The wire** (clean cutover, every caller). `NoteQueryReq.space_id: Option<String>` becomes `spaces: Vec<NoteSpaceRefReq { vault_id, space_id }>`, `#[serde(default)]`, in selection order. `space_terms` keeps its name and default. It is false only when the scope is exactly one entered, non-opaque space whose restore is on the bar; the comparison tightens from id only to id plus drive. `NoteScope`'s space arm becomes `{ kind: "space"; spaces: readonly [NoteScopeSpace, ...NoteScopeSpace[]] }` with `NoteScopeSpace { id, name, vaultId, defaultKey }`.
  - **Which drives are searched.** `NoteQueryReq::search_drives(home)`: the selection; with nothing selected, the drives the spaces live on; with no spaces, home. The list is deduplicated, in first-mention order.
  - **What narrows each drive.** Drive `d` is narrowed by `merge::for_drive(scope, d)`: the members that live on `d`, or all members when none does. So one space still narrows every selected drive exactly as FR-629 says, and 79.5's acceptance holds. Within a drive, members combine by OR, and the bar narrows further by AND.
  - **Resolution.** The shell resolves every member lens once per list read, on its own drive's snapshot (`scope_lenses`). A lens that cannot be resolved fails the list, just as a missing single space fails it today (P7). Examples are a deleted space and an unmounted drive holding a member. *A2 superseded this for unions:* a member is resolved only when a searched drive needs it, and a failed member makes only that drive a notice. `keeper:all` and empty ids name no lens. `default_lens(req)` ("no real space") replaces both default-lens tests, the one in `matches_filter` and AD-303's.
  - **Prompts and caps inside a union.** A space's saved prompt selects by its words through the lexical pool, falling back to `IndexEntry::matches_text` when the index is not open. It does not rank (P2): ranking would need an embedding per prompt per refresh, and `merge_rows` would compare scores computed from different texts (DW-283). Only a sole space's prompt ranks, as today. Each space keeps its own `keeper.limit` through `counts::union_keep`: a note stays when some space that admits it would keep it on its own (P3). Caps are the common case, because every space saved from the bar carries one (DW-289). When no sort is explicit, the first-selected space's ordering orders the union.
  - **Dispatch** (replaces AD-294's dispatch sentence). `notes_list` is always called with the active drive (P8). The stream wakes on `other_drives(home)`: every drive searched or read from, home excluded. This also closes a pre-existing gap: a single selected drive other than home never woke the home stream.
  - **Interaction.**
    - A plain click on a rail space replaces the selection. This is today's `enterSpace`, and its restore into the bar is unchanged.
    - ⌘/Ctrl-click toggles a space in or out through `toggleSpace`, which never restores a bar. Leaving a single entered space for a union strips that space's restored terms (`withoutRestored`); otherwise the union would read X ∩ (X ∪ Y) = X.
    - `enteredSpace`, drift and reset (79.2) apply only to a single entered space. The invariant: `enteredSpace !== null` implies the scope is exactly that space.
    - `keeper:uncategorized` is a member like any space, and each drive's Uncategorized is its own (P1). `keeper:all`, Temporary and groups are never members.
    - Leaving a union by a plain click parks nothing (P5), because a park needs one base and one drive.
    - *Save as space* is refused for two or more members (P4, DW-284). Saving a lens-applied single space that lives on another drive saves into that space's drive (bp-spaces D2, which was a `NotFound` before).
    - `setVaultIds` prunes nothing (P10). A member whose drive leaves the selection narrows the searched drives that have none of their own.
    - `mergeSpace` (*Add to current search*, AND) is untouched.
    - Uncategorized's synthetic rail row becomes `restore.opaque` (bp-spaces D1). Entering it then sends `spaceTerms: true`, and it lists what it names instead of every note including conflicts and archived ones.

  **Refinement of the pinned text:** the pinned AD left "a visible affordance — a checkbox lane in the row on hover/phone long-press" to UX-DR111, and UX-DR111 declines it.

- **AD-307 — The caret hint is consumed once and a save acknowledgement never touches the editor.** **Binds:** FR-661, FR-662; Story 81.1. **Prevents:** the caret sent back to the template's `{{cursor}}` on every autosave; keystrokes typed during the save round trip spliced away by `applyExternal(base)` after `markSaved`; a store field documented "consumed once" that nothing nulls; a fix that also stops writes from outside, or an accepted revision, from reaching CodeMirror. **Rule:** `consumeCaretHint` nulls `NoteDocument.cursor` right after the editor places the hint. That happens in the boot closure or in the reconcile effect, whichever the arrival order makes first. The reconcile effect fires on `NoteDocument.externalEdition`, a monotonic counter bumped by the opening `reset`, by a live `external` apply on a clean buffer, and by `acceptPending`. It never fires on `base`. So `markSaved` changes nothing in the CodeMirror document or selection, and every save path inherits that: autosave, blur, ⌘S, close. Rust is unchanged. **Done in the tree** (Story 81.1).

- **UX-DR111 — A selection of spaces.** **Binds:** AD-306, AD-305; Story 81.4 (`epic81-spaces`), and 81.5 reads the rows it paints. **Prevents:** a selection only a ⌘-click can reach; a new verb confused with *Add to current search* (which ANDs); `aria-current` claimed by several rows; two chips with the same name and no way to tell their drives apart; a platform sniff for the modifier; hover buttons coming back into the rail. **Rule:**
  - **Rail row.** A plain click opens the space and replaces the selection, as today. ⌘/Ctrl-click adds the space to the selection or removes it. The test is `metaKey || ctrlKey`, the house pair at `files-pane.tsx:2258`; `src/test/no-user-agent-gating.test.ts` forbids a platform test. ⌘/Ctrl+Space and ⌘/Ctrl+Enter on a focused row do the same, with no second activation from a synthesized click. Group rows and Temporary fold on any click. All notes means All notes on any click.
  - **Context menu** on a real space row and on Uncategorized: *Add to selection* when the row is not a member, *Remove from selection* when it is. The item goes directly after *Open space*. *Add to current search* stays, unchanged. UX-DR108's order becomes: Open space, Add to/Remove from selection, Add to current search, separator, Pin/Unpin, Edit space…, Duplicate space, separator, New note in this space, Add sub-space, New space…, separator, Delete space….
  - **No hover checkbox lane.** UX-DR104 removed the rail's hover buttons. The menu verb is the visible door that keyboard and assistive technology can reach.
  - **Member rows** keep the existing selected paint and gain `aria-pressed="true"`. `aria-current` is set only when the scope is exactly that one space.
  - **Scope chips.** One chip per member, in scope order. Each × is labelled `Clear scope {name}` and removes only its member. When the members span two or more drives, each chip's accessible description and hover hint read `{name} — {drive name}`. The folder chip is unchanged. A single entered space keeps AD-292's "Space" label rule.
  - ***Save as space*** with two or more members is disabled, with the description `A selection of several spaces can't be saved as one space — open one of them to save it.`
  - ***New note from search*** with two or more members: the chooser lists each member's own drive, `— in {space}`, in scope order, then every other selected drive, `— outside {scopeLabel}`. For two or more spaces, `scopeLabel` is the names joined with ` or `. An "outside" row sends the first member, so Rust still words the cross-drive notice.
  - **Phone.** No multi-select gesture, because there is no rail. The chips and their 44 px × still shrink a union made on the desktop.
  - **Tokens.** None new.

Also in this epic, without an AD, as the coordinator ruled:
- The Uncategorized row's opaque restore (D1, 81.4).
- Saving a lens-applied single space into its own drive (D2, 81.4).
- Removed notes pruned from every panel's back and forward stacks (R3, 81.3's answer applied by `closeTarget`, 81.5's stack shape).
- The home stream now wakes for a single selected drive other than home (81.4).

`spaceDrift` and `resetToEnteredSpace` still have no callers (D3), and they are left alone. The misleading wording of an "outside" create from Uncategorized (D5) is pre-existing and not widened here, so it is also left.

## Requirements allocated here

| id | statement | story | AD |
| --- | --- | --- | --- |
| FR-661 | After an autosave, a blur save or ⌘S, the caret stays where typing left it. Every character typed during the write's round trip stays in the editor, and a buffer holding such characters stays unsaved until they are written. A save acknowledgement changes nothing in the editor's document or selection. | 81.1 | AD-307 |
| FR-662 | A template's `{{cursor}}` hint places the caret once, on open, whichever of the editor and the opening snapshot arrives first. A write from outside applied live, or an accepted revision, still reaches the editor and does not move the caret back to the hint. | 81.1 | AD-307 |
| FR-663 | With service files hidden (the default), the plain list hides every space definition in the drive's spaces folder, whatever that folder is called, and counts it in `N hidden`. Turning the toggle off, or asking for `is:space`, shows them. Inside a space, and in a folder scope, nothing is hidden on their account. The rail, links, ⌘⇧F and the Files tree still find them. | 81.2 | AD-303 |
| FR-664 | An edit to a space definition made outside keeper's rail (an agent, sync, another editor) reloads the rail even though the plain list's rows and counts do not change. | 81.2 | AD-303 |
| FR-665 | Some new notes are created without a title or words: *New note* in the pane or on the phone, the palette, ⌘⌥N, *New note in {space}*, the tray, and *New note from search* with an empty prompt. If nobody writes in such a note, it is removed from its drive when the last editor showing it lets it go. Its row leaves the list, and no panel's Back, Forward or history menu offers it. | 81.3 | AD-304 |
| FR-666 | A new note is kept when anything was written in it: a word that stays, a title, a tag, a pin, a property, a rename. Typing and deleting back to nothing counts as nothing written. A wikilink's *create and link* note, a capture page and a journal entry are never removed, and a note keeper cannot read is kept. | 81.3 | AD-304 |
| FR-667 | The words typed into a note before its last editor closes reach disk in the same command that lets the note go. A save that arrives after that command writes nothing and never recreates a removed note. | 81.3 | AD-304 |
| FR-668 | A new note that a quit or a crash left untouched is removed when its drive is next opened. If an editor has already opened it again, that editor's release decides. | 81.3 | AD-304 |
| FR-669 | Any of these adds a rail space to the scope or removes it: ⌘/Ctrl-click on the space; ⌘/Ctrl+Space or ⌘/Ctrl+Enter on the focused row; *Add to selection* / *Remove from selection* in its menu. A plain click still replaces the scope with that one space and restores its search into the bar. A drive's Uncategorized can be selected; All notes, Temporary and groups cannot. | 81.4 | AD-306, UX-DR111 |
| FR-670 | A scope of several spaces searches each selected drive through the spaces that live on it, combined by OR, or through all of them when none lives on it. With no drive selected, it searches the drives its spaces live on. One space behaves exactly as FR-629 says. The bar's chips and text narrow the result further. | 81.4 | AD-306 |
| FR-671 | Inside a union each space keeps its own cap: a note stays when some space that admits it would keep it on its own. A space's saved prompt narrows by its words and does not rank. | 81.4 | AD-306 |
| FR-672 | The scope shows one chip per selected space, in the order chosen, and each × removes only its own space. When the spaces span two or more drives, each chip's hint names its drive. The Escape walk drops the last-chosen space first, one at a time. | 81.4 | AD-306, UX-DR111 |
| FR-673 | With two or more spaces selected: *Save as space* is disabled with a sentence that says why; *New note from search* offers each space's own drive (`— in {space}`) and each other selected drive (`— outside {A or B}`); leaving the selection by a plain click on one space parks nothing; deleting a selected space removes only that space from the selection. | 81.4 | AD-306, UX-DR111 |
| FR-674 | The list of a selection is live. A change on any drive it searches, or reads a space from, refreshes it, including a drive outside the drive selection. | 81.4 | AD-306 |
| FR-675 | Back, Forward or a jump in a panel's history onto a note restores the scope that note was opened under, when it differs from the current scope. If nothing in the bar is the person's, one space is entered as a rail click enters it, and any other scope replaces the bar's restored terms. If the bar holds a search of their own, the scope changes and their own chips and text stay. | 81.5 | AD-305 |
| FR-676 | Some history steps leave the scope alone: a step onto a file, a recording or a run; onto a target restored at launch; or onto a note whose remembered scope names a space the rail does not list (deleted, its drive not selected, its query broken, the rail not yet read). The history controls and menus are unchanged. | 81.5 | AD-305 |
| NFR-88 | A union costs what its members cost, once. Every member lens is resolved once per list read, on its own drive. Each candidate is tested against its drive's lenses, and the first lens that admits it decides. A body is read at most once while matching (the memoised `body_reader`), and at most once more per capped lens in the cap pass. The cap pass runs only with two or more lenses and at least one cap, and only over entries already matched. A union makes no embedding call beyond the bar text's one, so NFR-72's "exactly one embedding round trip" holds. It opens the search index for prompt words only when a member has a prompt. A single-space scope takes the unchanged path. Target: a two-space union over two drives of 10 000 notes each answers in p95 ≤ 250 ms end to end (NFR-72's bar). **To be measured on hesperia.** | 81.4 | AD-306 |
| NFR-89 | Letting a note go costs one IPC. The last release is a single `notes_close` whether or not it carries words; today a dirty close is `notes_save` followed by `notes_close`. A clean release carries no text, and no teardown path awaits anything. The pristine bookkeeping is registry IO only: one read-modify-write of the drive's row at an eligible create and one at its release, under one lock. It never walks a vault and never does vault IO under that lock. The start-up sweep reads exactly the files its row names. A create that supplies a title or words, a capture and a journal entry cost nothing new. | 81.3 | AD-304 |
| NFR-90 | Hiding space definitions adds one flag test per candidate to the existing service-file pass. `notes.service_file_names`, its default and `notes.hide_service_files` are unchanged, so a stored custom list keeps working and needs no migration. The spaces revision is one pass over the in-memory index entries per stream wake, with no disk read and no new wire field, and a batch it causes carries no rows. After an external edit to a space file, the rail shows the change within NFR-29's 1 s. **The last clause is to be observed on hesperia.** | 81.2 | AD-303 |
| NFR-91 | History stamps are never persisted. The panels cookie and `PANELS_VERSION` are byte-identical for the same panels. A step whose stamp equals the current scope writes nothing to the filters store. A step that restores a scope makes no IPC of its own; the one list read that any scope change makes is its only cost. | 81.5 | AD-305 |

## Stories

Every story names its rung in the three-rung stack: `epic81-plan` → `epic81-fixes` (81.1, 81.2, 81.3) → `epic81-spaces` (81.4, 81.5). Each rung must pass CI alone, and `bun run lint` (`biome check .`) is part of the gate. **Everything under `src-tauri/crates/keeper/**` is by inspection, awaiting CI's macOS job**: the shell crate does not build on this host. Every `keeper-core` signature a story changes is grepped for callers in `crates/keeper/src` and `crates/keeper-syncd/src` in the same change. `NoteQueryReq.space_id` has no reader in keeper-syncd, bots, `palette.rs`, tasks or sessions; Matrix `SpaceVm.space_id` and sessions `space_id` are different concepts and stay untouched. Generated bindings (`src/lib/ipc/gen/*.ts`) are regenerated with `cargo test -p keeper-core`, never written by hand. `notes_ipc.rs` moves under this wave by hundreds of lines, so anchors there are function names; line numbers are from `f95905b` where given.

Several files carry hunks for both rungs, and the coordinator splits them at stack time:

| File | `epic81-fixes` | `epic81-spaces` |
| --- | --- | --- |
| `keeper-core/src/notes/vm.rs` | `NoteCreateReq::writes_nothing`, `NoteReleaseReq` | `NoteSpaceRefReq`, `NoteQueryReq.spaces`, `search_drives`/`other_drives` |
| `keeper/src/notes_ipc.rs` | AD-303's site, `stream_changes`'s `sent_spaces`, every AD-304 region | the AD-306 regions, including AD-303's site rewritten to `default_lens(req)` and `stream_changes`'s secondaries |
| `keeper/src/notes_vault.rs` | the `refresh` sweep hook, the creation-test migration, `ScratchPlatform` (A8) | the `space_id` → `spaces` test migrations, the union desktop test, and 81.2's service-files desktop test. That test builds its query with `spaces`, so on `epic81-fixes` it needs the `space_id` form. |
| `src/lib/ipc/client.ts` | `notesClose` + `NoteReleaseReq` | the `NoteSpaceRefReq` re-export |
| `dev/mock-shell.ts` | `notes_close` | `notes_list`/`notes_spaces` |
| generated files | `NoteReleaseReq.ts`, `docs/settings-keys.md` | `NoteQueryReq.ts`, `NoteSpaceRefReq.ts` |

`bindings:check` must be green on each rung alone.

### 81.1 — The caret stays where you type
**Intent:** "jak notatka sie zapisuje to kursor wraca na poczatek lini (poprzedniej lini) a nie na miejscu". A save acknowledgement is Rust agreeing with text the editor already shows, so it reaches nothing in CodeMirror. **Claim:** 5. **Rung:** **epic81-fixes**. AD-307. **Status: done in the tree** (lane FixCursor). A9 (lane FixFront) amended it after the review found a second view of the same note left behind. The change is frontend only, with no shell file and no binding.
**Files (as shipped):**
- `src/lib/stores/notes-editor.ts`:
  - `NoteDocument.externalEdition: number` (doc at `:156-168`; `0` in `EMPTY_NOTE_DOCUMENT` `:227`; never decreases). It is bumped by `applyBodyBatch`'s `reset` arm (`:390`), by its `external` arm only on a clean buffer (`:412`; a dirty buffer still raises the bar and does not bump), and by `acceptPending` when a revision is pending (`:455`). It is never bumped by `markSaved` (`:486-498`), `editBuffer`, `keepMine`, `beginSave`, `markSaveFailed` or `consumeCaretHint`.
  - `consumeCaretHint(vaultId, noteId)` (`:460-464`) nulls `cursor`.
  - The `cursor` doc (`:146-155`) now says what is true: set only by the opening reset, placed once, then nulled.
- `src/components/notes/note-editor.tsx`:
  - The boot closure consumes the hint right after placing it (`:835-844`).
  - The reconcile effect (`:857-879`) is keyed on `[vaultId, noteId, externalEdition]`. It does nothing at edition `0` or without a runtime. Otherwise it reads the document from the store, applies `base`, and places and consumes the hint when one is still set.
  - The `base` selector that only the old effect read is deleted.
  - *A9:* `applyExternal` is now `adopt(text, flash)`. The editor's store subscription mirrors the store's `text` into its view, with no flash, when `text` moved and `externalEdition` did not. A sibling view of the same note therefore follows. A save acknowledgement moves neither, so FR-661 holds.
- `src/components/notes/new-note-caret.test.tsx`:
  - The `notesSave` mock resolves a real `NoteWriteVm` (`WRITTEN`).
  - New helpers: `openHeldBack` (the reset arrives after the editor chunk, which is the usual order in the running app), `openTemplated`, `ORDERS` (both arrival orders), `type` (an unannotated user transaction) and `idleUntilAutosave` (fakes only `setTimeout`/`clearTimeout`).

**Acceptance (shipped, green):**
- *The two regressions*, `describe("a save acknowledgement")`:
  - (a) In both `ORDERS`: a note opened on `# Standup\n\n## Agenda\n` with the hint at 10, typed at the end of the body and autosaved (`notesSave` called once), keeps `selection.main.head` at the typed position, and the store is clean.
  - (b) With the write held: `first` is autosaved, ` second` is typed during the round trip, then the write lands. The editor and the store both read `# Standup\nfirst second`, and the store is still dirty.

  Both failed on the old code with the owner's two symptoms: `expected 10 to be 30`, and ` second` spliced away.
- *The other half of the contract*, `describe("a revision from outside the editor")`:
  - In both orders, an `external` batch on a clean buffer lands in CodeMirror, and a caret the user moved to 0 stays at 0.
  - An `external` batch on a dirty buffer leaves the editor untouched until `acceptPending`, which puts their text in.
- *Mutation record* (lane report). Each mutant was restored and the restore verified with `cmp`; the source was then re-read with `git diff -U0`.
  - M2, the shipped defect (the effect on `[base, openingCursor]`, no consume anywhere): 5 failures.
  - M1, a base-keyed effect that keeps the boot consume: caught by (a) chunk-first and by (b).
  - M3, no consume on the reconcile path: caught by the live-apply case chunk-first.
  - M4, no boot consume: caught by live-apply reset-first.
  - M5, the `external` arm not bumping: caught by live-apply in both orders.
  - M6, `acceptPending` not bumping: caught by the accept case.
  - M7, `markSaved` bumping: caught by (b).

  M1 and M3 survived the first sweep until (a) and the live-apply case were parametrised over both arrival orders.
- *Runs:*
  - `bunx vitest run src/components/notes/new-note-caret.test.tsx src/components/notes/note-editor.test.tsx src/lib/stores/`: 48 files, 575 tests.
  - The notes/capture/export/hooks neighbourhood: 106 files, 1571 tests.
  - `bunx tsc --noEmit -p .`: clean.
- *Two views of one note (A9)*, `two-notes-at-once.test.tsx` `describe("two panels showing the same note")`:
  - *what one view types and saves the other shows, and typing there keeps it*: the store and view A keep A's words after B types.
  - *keystrokes typed in one view while its save is in flight survive the ack in both*: the text survives in A, in B and in the store, and the store is still dirty.

  Disabling the mirror turns both red (FixFront). FixFront's run: vitest over `src/components/notes`, `src/lib/stores`, `src/hooks` and `capture-document.test.tsx`, 146 files, 2105 tests; `tsc` clean.
- *On hesperia:* a note from the inbox template, three lines typed, a 2 s wait. The caret is where it was.

**Retargeted tests:** none re-pinned. The three open-caret cases (`new-note-caret.test.tsx:231-285`) stand unchanged. So do `note-editor.test.tsx`'s resets seeded with `cursor: null`, and `notes-editor.test.ts`, which tests isolation between notes.
**Not in this story:**
- Rust. `take_caret`/`PENDING_CARET` were already once per open.
- `placeCaret`, `spliceBetween` and the external apply (`adopt` since A9), which are correct and covered.
- `refreshMarks` re-sending the search-marks effect when `base` or `rev` moves. That changes neither text nor selection.

Two leftovers the lane named, outside its files:
- `UseNotesBody.cursor` (`use-notes-body.ts:158-161`, `:174`, `:264`) has no consumer any more; the editor reads the store at `note-editor.tsx:581`. 81.3 owns that file this wave and deletes it.
- The header of `note-editor.tsx` (`:19-22`, "Only `base` flows the other way") gains "…when the external edition moves".

**binds:** FR-661, FR-662, AD-307

### 81.2 — Space definitions are service files
**Intent:** "service files - by default dodaj jeszcze definicje space". The plain list stops listing the notes that define spaces, and the rail still hears about edits to them. **Claim:** 1. **Rung:** **epic81-fixes**. AD-303. **Status:** the rule and the spaces revision are in the tree, amended by A3 and A8. The two keeper-core tests are mutation-proved per the lane. The shell call sites and the shell desktop test await CI's macOS job.
**Files:**
- `src-tauri/crates/keeper-core/src/notes/service_files.rs`:
  - The module doc reads "configured basenames, never a path or glob, plus every space definition the index flags".
  - `pub const SPACE_FLAG: &str = "space"`.
  - `pub fn hides_space_definition(flags: &[String], asked: &[String]) -> bool`: the entry carries `space`, and the query did not ask for `is:space`.
  - `pub fn space_definitions_revision(entries: &[IndexEntry]) -> u64`: path, size, mtime, inode and flags of the `space`-flagged entries, hashed. In process only, never persisted or sent.
- `src-tauri/crates/keeper/src/notes_ipc.rs` (by inspection, awaiting CI macOS):
  - `project_list`'s service-file retain gains `&& !(default_lens && hides_space_definition(&entry.flags, &req.flags))` beside `is_service_file`, inside the existing `req.hide_service_files` block, so the entries count in `hidden`. `default_lens` is the same "no real space" test `matches_filter` uses; on `epic81-spaces` both read `default_lens(req)` (81.4).
  - `stream_changes` keeps `sent_spaces: Option<u64>` beside `sent` and sends a `NoteChangeBatch` when the revision moves, even with no ops and the same counts. Since A3 the revision is taken before `current_window`. When the window read fails after a batch has already gone out, a moved revision still sends an empty-ops batch with the last counts.
- `docs/notes.md:195-202` ("Service files": space definitions in each drive's spaces folder; only the plain list hides them; a hidden note still opens, matches ⌘⇧F and is in the Files tree; a space definition is still in the rail).
- `src/components/notes/search-settings.tsx:149-153` (the Service files caption names space definitions and the spaces folder).
- No frontend logic changes: `use-notes-changes.ts:106`'s `requestSpacesReload()` on every batch is the reload.

**Acceptance:**
- *keeper-core, in the tree:*
  - `hides_space_definitions_unless_asked_for_by_name`: `hides_space_definition(["space"], [])` holds; `(["pinned","space"], ["pinned"])` holds; `(["pinned","space"], ["space"])` does not; `(["pinned"], [])` does not; a flag spelled `spaces` does not.
  - `space_revision_moves_with_space_definitions_only`: the revision is equal for a clone and after an ordinary note's mtime/size change. It moves when a space's mtime/size changes, when its inode changes (replaced), when it gains `temporary`, and when a space is removed.

  Both are mutation-proved per the lane.
- *Shell, by inspection, awaiting CI macOS:* `a_space_definition_is_a_service_file_of_the_plain_list_only` in `notes_vault.rs`. It runs over a `ScratchPlatform` whose data dir is a temp dir (A8), so it reads the default service-file names and never the owner's registry. The vault holds `plain.md` and `spaces/defs.md`, and the space's own query (`tag:def`) admits itself.
  - The default query lists `Plain` only, with `hidden == 1`.
  - `hide_service_files: false` lists both, with `hidden == 0`.
  - Scoped to the space, with the toggle on, it lists both with `hidden == 0`: inside a space, its query decides.

  Mutation to run on the gate: dropping `default_lens &&` turns the scoped case red. Two cases the plan named are not in the shipped test:
  - asking for `is:space` by name (the keeper-core test covers the predicate);
  - a renamed spaces folder (the rule reads the index's `space` flag, which Story 79.9 stamps from `spaces_prefix()`).

  **Rung hazard:** the test builds its query with `spaces: Vec<NoteSpaceRefReq>`, which is 81.4's wire. On `epic81-fixes` alone it must use `space_id` or move up a rung (see the hunk table).
- *A3 has no test.* Taking the revision before `current_window`, and sending the empty-ops batch on `Err`, are by inspection only.
- *On hesperia:*
  - The count line reads `N notes · M hidden`, with M including the drive's space definitions.
  - The eye toggle shows them.
  - Renaming a space file's title in another editor changes the rail within 1 s (NFR-90) while the list does not move.
  - Delete the file of the space you are in from another editor. The rail drops the space and the list shows its error (A3).

**Retargeted tests:** none (lane report). `keys.rs`'s `service_file_default_matches_registry_fallback` and `registry.rs`'s service-file round trips stay green unchanged, which is the proof that no default moved.
**Not in this story:**
- Sessions' `_spaces/` (DW-288).
- The rail's read path (`notes_spaces` reads definitions directly and is unaffected).
- A Settings row for the rule, which rides the existing toggle.
- A `NoteChangeBatch` field.
- AD-306's rewrite of the same site to `default_lens(req)` (81.4).

**binds:** FR-663, FR-664, NFR-90, AD-303, AD-267 (amended)

### 81.3 — An untouched new note leaves
**Intent:** "jezeli stworze nowa notatke, ale nic w niej nie zapisze - usun ja". A new note nobody wrote in is removed when its editor lets it go, and nothing anybody wrote can be removed by that rule. **Claim:** 2. **Rung:** **epic81-fixes**. AD-304; D-24.
**Files (keeper-core, testable here):**
- `notes/vm.rs`:
  - `impl NoteCreateReq { pub fn writes_nothing(&self) -> bool }`: true when the title is none or blank and the body is none or blank.
  - `NoteReleaseReq { text: Option<String>, base_rev: String, discard: bool }` (`discard` since A1), `TS` and camelCase. It generates `src/lib/ipc/gen/NoteReleaseReq.ts`.
- `registry.rs` (beside `CaptureDraft`):
  - `NOTES_PRISTINE_PREFIX = "notes.pristine."` and `pristine_notes_key(vault_id)`.
  - `PristineNote { note_id, path, document }` with `is_untouched(path, on_disk)`: path equal; block equal after `Frontmatter::remove_in(.., "updated")` on both; body equal after trimming.
  - `get_pristine_notes(data_dir, vault_id)`: absent, blank or malformed reads as `[]`, and malformed warns.
  - `set_pristine_notes(data_dir, vault_id, &[..])`: an empty slice writes `""`.
- `config/keys.rs`: a `KeySpec` for `notes.pristine.` (`family: true`, `Scope::SessionState`, `Settable::Never(..)`, `Shape::Json`), between `notes.capture_placement.` and `notes.read.`.
- `docs/settings-keys.md`: regenerated with `cargo test -p keeper-core --lib config::keys::tests::docs::regenerate -- --ignored`.

**Files (shell, by inspection, awaiting CI macOS):**
- `keeper/src/notes_ipc.rs`:
  - Imports `NoteReleaseReq` and `PristineNote`.
  - `BodySub` gains `released: Mutex<bool>`, initialised `false` in `notes_open`, with `lock_released` tolerating poison like `lock_body`.
  - `static PRISTINE: LazyLock<Mutex<HashMap<String, (String, PristineNote)>>>` sits beside `PENDING_CARET` and guards map operations only (A7). `PRISTINE_HELD: AtomicUsize` lets a close skip the lock while nothing is pristine, and `notes.pristine.` row IO runs under a separate `PRISTINE_ROWS` mutex. Lock order: a `released` gate, then `PRISTINE`, then `SUBSCRIPTIONS` (A5); never `PRISTINE` while holding `SUBSCRIPTIONS`; no vault IO under `PRISTINE`.
  - `write_through(sub, vault, text, base_rev, frontmatter)` is extracted from `notes_save`. Its behaviour is unchanged, except that it records the revision it wrote in `BodyState.written` (A6).
  - `notes_save` holds the gate across `write_through` and answers `NotFound` once released. The function has no `.await`, so the future stays `Send`.
  - `notes_close(state, subscription_id, release: Option<NoteReleaseReq>) -> Result<bool, IpcError>` sits over a synchronous, testable `release_subscription(data_dir, subscription_id, release)`, as in AD-304. It removes a note only when `release.discard` is true (A1). With `false`, the pointer stays in `PRISTINE` for a later release.
  - `flush_on_release` skips a body already on disk. It takes the disk revision as its base when that equals `BodyState.written`, so a release after the person's own autosave writes no conflict copy (A6).
  - `remember_pristine`, `forget_pristine`, `edit_pristine_row` and `discard_if_untouched`. `discard_if_untouched` logs INFO when it removes and DEBUG when it keeps, and runs over `reads_as_created` and `remove_untouched` (A7).
  - `pub(crate) sweep_pristine(data_dir, vault)`. It decides each note under `PRISTINE` right before its unlink, and adopts a note an editor holds at that moment (A5).
  - `notes_create` gains `State<'_, AppState>` and records after `create_for_space` when `req.writes_nothing()`. `create_for_space` becomes `pub(crate)`.
  - `tray_new_note` records in its `Ok` arm before `emit_open`.
  - `create_note`, `resolve_capture_draft` and `create_journal` are unchanged.
- `keeper/src/notes_vault.rs`:
  - No unlink function lives here. The unlink shipped as `unlink_untouched` in `notes_ipc.rs`: `remove_file` on `notes_vault::contained(..)`, then `touch` and `mark_dirty`, as `trash_note` does. Its doc carries NFR-30's exception.
  - `refresh` takes `data_dir` once and calls `sweep_pristine` per registered vault inside the existing `spawn_blocking`.
  - `cross_drive_creation_names_the_source_space_without_inheriting_it` calls `create_for_space`, because a unit test cannot build `State`.
- `keeper/src/lib.rs`: registration unchanged; the macro adapts.

**Files (surface):**
- `src/lib/ipc/client.ts`: `notesClose(subscriptionId: string, release: NoteReleaseReq | null = null): Promise<boolean>`, re-export and import, and the doc (a release carries the unsaved words; `true` means Rust removed an untouched new note).
- `src/hooks/use-notes-body.ts`:
  - The last-view cleanup becomes one `notesClose(sub, { text: dirty ? text : null, baseRev: rev, discard: mayDiscard(vaultId, noteId) })`. `mayDiscard` is true only when no panel targets the note (A1). On `true` it calls `panelsStore.getState().closeTarget({ kind: "note", vaultId, noteId })`, and `.catch(() => {})` handles failure.
  - The orphan branch sends `{ text: null, baseRev: "", discard: <the same rule> }` when `readNoteDocument(vaultId, noteId).views === 0`, and `null` otherwise (A1). The build first shipped `null` in every case.
  - The header bullets and the `saveNote` doc are updated.
  - The dead `UseNotesBody.cursor` (81.1's leftover) is deleted.
  - *A10:* when `notes_open` fails NotFound, that note's target is closed with `closeTarget`. `isNoteNotFound` tests `code === "internal"` and a message beginning `no such note:`. Any other failure keeps the panel and shows why.
- `dev/mock-shell.ts`: the `notes_close` handler answers `false`.
- `docs/notes.md`: one sentence under the new-note paragraph.
- The `notes_close` row of `ARCHITECTURE-NOTES-PHASE5.md`'s IPC table.

**Acceptance:**
- *keeper-core* (mutation-proved; comparing the body only must turn the pin, tag and different-id cases red):
  - `a_create_that_supplies_nothing_writes_nothing`: none or blank title and body gives true; title `Foo` (the wikilink case) gives false; body `x` gives false; tags, space and template do not matter.
  - The pristine round trip: two drives' rows stay independent; an empty slice reads back `[]`; a malformed row reads `[]`.
  - `is_untouched`:
    - identical bytes: true;
    - `updated` restamped plus a trailing newline: true;
    - a word in the body: false;
    - `tags:` added through `set_in`: false;
    - another path: false;
    - another `id` at the same path: false.

  The keys coverage and docs tests stay green after regeneration.
- *Shell, `notes_ipc.rs` `mod tests`* (by inspection, awaiting CI macOS). Each test uses a per-test temp `data_dir` and `test_vault`, and registers a `BodySub` with `Channel::new(|_| Ok(()))`:
  1. `an_untouched_new_note_is_removed_when_its_editor_lets_it_go` → `Ok(true)`, the file is gone, no `.keeper/trash` exists (unlinked, not trashed), and the row is empty.
  2. `a_note_typed_back_to_nothing_is_still_removed` → `true`.
  3. `the_release_carries_the_last_words_and_the_note_is_kept` (`text: Some("hello")`) → `false`, the disk body is `hello`, and the pointer is gone.
  4. `a_save_after_the_release_writes_nothing` → `NotFound`, and the file is still absent. No resurrection; mutation-proved on the gate by dropping the gate check.
  5. `only_the_last_of_two_editors_decides` → `false` then `true`.
  6. `an_orphan_close_never_removes_a_note` (`release: None`) → `false`, and the pointer is kept.
  7. `a_flush_already_on_disk_is_not_written_twice` → no `*.sync-conflict-*` file.
  8. `a_templated_note_is_untouched_as_created` (a template with `{{now}}`) → `true`.
  9. `a_renamed_or_pinned_new_note_is_kept` → `false` both ways.
  10. `the_sweep_removes_what_a_previous_run_left`: a row seeded directly is swept and cleared, and a note held in `PRISTINE` is left alone.
  11. `a_release_after_its_own_autosave_leaves_no_conflict_copy` (A6): an in-flight autosave, then a release against the older revision → 0 conflict copies. Another writer's edit still → 1.
  12. `a_note_kept_on_screen_is_not_discarded_by_its_last_close` (A1): a last close with `discard: false` keeps the note and its row. A later close with `discard: true` removes it.
  13. `the_sweep_adopts_a_leftover_an_editor_holds` (A5): the sweep adopts a held note, and that editor's release then removes it.

  Tests 11–13 have not run. The mutations to run on the macOS gate:
  - the gate ignoring `discard` → 12 red;
  - `flush_on_release` always passing `base_rev` → 11 red (one copy instead of none);
  - `held_open` forced false → 13 red.
- *Shell, `notes_vault.rs`* (by inspection, awaiting CI macOS): the migrated cross-drive creation test. The planned `removing_an_untouched_note_leaves_no_trash_copy` became test 1's trash assertion, because the unlink lives in `notes_ipc.rs`.
- *Frontend:*
  - `two-notes-at-once.test.tsx`, `describe("closing one of two panels")`:
    - `notesClose` is called exactly once with `("sub-one", { text: "${ONE_BODY}unflushed\n", baseRev: "rev-${ONE}", discard: true })`, and `notesSave` is not called for `sub-one`;
    - *releases a clean editor with no words* sends `text: null`.
  - `describe("letting go of a new note nobody wrote in (AD-304)")`, over a `Strip` harness driven by the real panel store (A1):
    - *folding the only panel on it flushes without discarding, and the panel keeps it* (`discard: false`);
    - *moving the panel to another note discards it, and the removal leaves history* (`discard: true`; a `true` answer prunes history);
    - *left before its open resolved, it is still released and may go* (`{ text: null, baseRev: "", discard: true }`).
  - `describe("a note panel whose note is not there")` (A10): *closes when opening it answers that the note does not exist*; *stays, showing why, when opening it failed for another reason*.
  - `capture-document.test.tsx:469` asserts `("sub-1", expect.objectContaining({ text: null }))`, and `:483`'s one-write assertion stays.
  - Mutation-proved. The old `notesSave` + `notesClose(sub, null)` pair turns the retargeted case red. Each of these turns a named case red (FixFront): `discard` forced true, `discard` forced false, the orphan always sending `null`, the NotFound check disabled, and any error closing the panel.
- *On hesperia:*
  - ⌘⌥N, then click another note: the untitled file is gone from Finder, its row from the list, and Back does not offer it. `git log` on the drive shows the add and the removal, or neither if the cadence never committed the add.
  - The same with one word typed: kept.
  - A title set, a pin, a wikilink stub: kept.
  - Fold the only panel on a new note, or switch Notes → Files: the note stays, and so does its panel (A1).
  - Quit with an untouched new note open, then relaunch. The note is gone, and the panel restored onto it closes (A10). The sweep runs before any webview opens a note, so its adopt branch (A5) serves later refreshes.

**Retargeted tests:**
- `two-notes-at-once.test.tsx:27`, `:41` and `capture-document.test.tsx:51`, `:75`: the mock wrappers forward the second argument, since they drop it today.
- `two-notes-at-once.test.tsx:262-289`: the unmount flush's words now ride the close, and every release expectation carries `discard`. The earlier "Rust removed" case assumed a removal while another panel still showed the note, which A1 rules out. The `Strip` cases above replace it. The assertion changes route but keeps what it protects.
- The other roughly 15 `notesClose` mocks are `vi.fn(async () => {})` and stay compatible.
- `panels.test.ts:430-455` ("a target that was deliberately deleted") gains "and leaves no step back to it", and its identity case still holds.

**Rung note (R3), for the coordinator:** the prune of a removed note from `back`, `forward` and `replaced` lives in `closeTarget` in `panels.ts` (`withoutTarget`, A11). It is written with 81.5's `PanelHistoryEntry`, in the `epic81-spaces` rung. On `epic81-fixes` alone, Back can land on a removed note. Since A10 that no longer drops typed words silently: `notes_open` fails NotFound and `closeTarget` closes the panel. That is abrupt, but nothing is lost. The recommendation stands: cut a four-line prune hunk into `epic81-fixes` over today's `PanelTargetVm[]` stacks (filter each by `!sameTarget`), and let 81.5 retype it to `entry.target`. The alternative is to state that `epic81-fixes` is never released without `epic81-spaces`.
**Not in this story:**
- A quit flush (DW-286).
- Session-space create (DW-285).
- Removing an emptied folder (Q1).
- Trash instead of unlink (D-24).
- Capture's reuse, journal creation and wikilink stubs, all unchanged by construction.

**binds:** FR-665, FR-666, FR-667, FR-668, NFR-89, AD-304, D-24

### 81.4 — A scope is a selection of spaces
**Intent:** "space daj mozliwosc zaznaczenia wielu drive i space z kazdego drive", as the owner resolved it: select several spaces at once, one or more from each drive, and get their union. **Claim:** 4. **Rung:** **epic81-spaces**. AD-306; UX-DR111.
**Files (keeper-core, testable here):**
- `notes/vm.rs`:
  - `NoteSpaceRefReq { vault_id, space_id }`, `ts(export)`.
  - `NoteQueryReq.spaces: Vec<NoteSpaceRefReq>` (`#[serde(default)]`) replaces `space_id`, and the `space_terms` doc is rewritten.
  - `impl NoteQueryReq { search_drives(&self, home) -> Vec<&str>, other_drives(&self, home) -> Vec<&str> }`.
- `notes/merge.rs`: `for_drive(scope, drive, drive_of)`. Since A2 it also has `DriveScope` (`new`, `for_drive(drive, resolve)`, `first()`), which resolves a member only when a searched drive needs it, at most once per list.
- `notes/counts.rs`: `union_keep(candidates, lenses) -> Vec<bool>`.
- `notes/rail.rs`: the `keeper:uncategorized` synthetic row gets `restore.opaque = true` (D1).
- Generated: `NoteQueryReq.ts` (`spaces: Array<NoteSpaceRefReq>`) and `NoteSpaceRefReq.ts`. `src/lib/ipc/client.ts` re-exports `NoteSpaceRefReq`.

**Files (shell, by inspection, awaiting CI macOS):**
- `keeper/src/notes_ipc.rs`:
  - Since A2: `member_lens`, `scope_members`, `home_of` and `unreadable_member` over `merge::DriveScope`. The planned `ScopedLens`/`scope_lenses` are gone. `default_lens(req)` is used in `matches_filter` and at AD-303's site.
  - `LensTest { query, words: Option<PromptWords> }`, with `PromptWords::{Ids, Text}` and `admits(&IndexEntry)`. `read` uses `search_index` `LEXICAL_POOL`, and `Text` is the fallback through `IndexEntry::matches_text`.
  - `space_matches(req, &[LensTest], entry, body, now)`.
  - `project_list(platform, vault, req, lenses: &[&SpaceLens], whole, embedding)`:
    - The `scope_vault` parameter and the per-drive lens resolution are gone.
    - With no lenses, today's `None` arm runs verbatim. With lenses, the union arm runs, and it yields the cap only for a single lens.
    - When `capped_union` holds (A4), the first pass records a per-entry lens-admission mask over one memoised `body_reader`. The cap step reads that mask and feeds `counts::union_keep`. Each capped lens logs its decline with `counts::select(..).report(&lens.name)` at the existing warn/info split.
  - `project_vaults`:
    - `search_drives` runs at the top.
    - The single-drive path runs when `vault_ids` is empty and one drive is searched, keeping today's paged dispatch. It resolves only the members that drive needs, and it fails as before when one of them fails.
    - Otherwise it fans out over a `BTreeSet` of drives, each through `DriveScope::for_drive`. A failed member makes that drive a notice (A2).
    - After the loop, `sole_prompt` and the re-sort by `explicit.or(ordering)` read `scope.first()`, the first member that resolved.
    - `merge_rows` is unchanged.
  - `stream_changes`'s secondaries come from `other_drives(&vault.id)`, with owned ids collected before the guard drops.
  - `wait_for_list_change`'s `multiple` is `!other_drives(..).is_empty()`.
  - `default_query` has `spaces: Vec::new()`.
  - `UNCATEGORIZED_SPACE_ID` and `ALL_SPACE_ID` stay where they are, because `uncategorized-id.test.ts` regex-reads the file.
- `keeper/src/notes_vault.rs` tests: `space_id: None` → `spaces: Vec::new()`; `req.space_id = Some(id)` → `req.spaces = vec![NoteSpaceRefReq { vault_id, space_id }]`; plus the new desktop test below.
- `lib.rs`: unchanged.

**Files (surface):**
- `src/lib/stores/notes-filters.ts`:
  - The `NoteScopeSpace` type and the space arm.
  - Exported `spaceKey`, `scopeSpaces`, `scopeHas`, and `sameScope` (ordered `spaceKey` equality).
  - `scopeLabel` joins names with ` or `.
  - `enterSpace` builds a one-member scope from a four-field projection, never the whole VM.
  - `toggleSpace(space)`, plus a private `withoutRestored`. `toggleSpace` never restores the bar. The last toggle-out gives All with the bar kept, `enteredSpace` and `sort` null, like today's ×.
  - `spaceDrift` and `dropLastChip`: with two or more members, drop the last member only.
  - `noteQueryFor` sends `spaces`, and `spaceTerms` uses the id-plus-drive rule. The docs at `:588-598` and the `enteredSpace` invariant are updated.
- `src/hooks/use-notes-changes.ts`:
  - `readWindow` always passes the active drive.
  - `scopeDrivesKey` replaces `scopeVaultId`.
  - Subscription ids are `{vaultId, ...selected, ...scopeDrives}`.
  - A batch refreshes when nothing is selected, when its drive is selected, or when it is a scope drive.
- `src/hooks/use-notes-actions.ts`:
  - `openNotesSpace`: the re-select guard is `outgoing.length === 1 && spaceKey` equal, or All on All. The baseline is `enteredSpace` only for a single matching member. It parks only when `outgoing.length <= 1`, and the park's id, drive, name and base come from `outgoing[0]`.
  - A new `toggleNotesSpace(space)` on the same serialized chain and generation guard. `keeper:all` delegates to `openNotesSpace`. Removing a member is allowed even with `space.error`. Adding one throws on `space.error` and touches non-`keeper:` ids.
  - `captureSpaceDraft` returns `null` for two or more members. `baseSpaceId` is `members[0].id` when `spaceTerms`. The draft drive is `members[0].vaultId` when a base is sent (D2).
  - *A11:* a queued plain click records `queryGeneration` at click time, and toggles re-base when they start. A queued entry's own scope change runs inside `applyEntry`, which the generation counter ignores.
- `src/components/notes/space-list.tsx`:
  - Delete `scopedSpaceId`, `activeSpaceId` and `scopedVaultId`.
  - `active` becomes pending → key match; All → `scope.kind === "all"` on its drive; else `selectedKeys.has(key)`.
  - `aria-pressed` on members, and `aria-current` only when the scope is that sole space.
  - Row `onClick`: fold for groups; `metaKey || ctrlKey` on a non-All row → `pick(space)`; else `open(space)`.
  - `onKeyDown`: ⌘/Ctrl+Space and ⌘/Ctrl+Enter call `preventDefault()` then `pick`. An auto-repeated key is prevented and ignored (`event.repeat`, A11).
  - The menu items per UX-DR111.
  - `onDeleted` toggles out only the deleted member.
- `src/components/notes/note-filter-bar.tsx`:
  - One chip per member, keyed `spaceKey`. The label follows AD-292's rule for a single entered space. The × is `Clear scope {name}` → `toggleSpace(m)`. When the members span drives, the × is `Clear scope {name} — {drive}` (A11), and the description and hint name the drive too.
  - A local pure `createChoices(scope, selectedIds)`. The button direct-creates iff there is exactly one choice.
  - `createFromSearch(choice)` cites `choice.space ?? members[0]`.
  - Save is disabled for two or more members, with UX-DR111's sentence.
- `src/components/notes/notes-pane.tsx`: the `no-recordings` empty state requires exactly one member with `defaultKey === "recordings"`. ⌘⇧S on a union shows `SAVE_UNION_REFUSED` through the pane's action error (A11).
- `notes-phone-pane.tsx`, `recordings-space.ts`, `search-field.tsx`, `physical-tree.tsx`, `tag-tree.tsx` and palette `actions.ts`: unchanged.
- `dev/mock-shell.ts`: the `notes_list`/`notes_spaces` emulation reads `spaces`, if the harness needs it.

**Acceptance:**
- *keeper-core* (each test mutation-proved; e.g. `for_drive` ignoring "own" gives B `[X,Y,Z]` and turns red; `union_keep` ignoring caps keeps index 2 of a cap-1 lens and turns red):
  - `a_query_names_each_space_with_its_drive`: frontend-shaped JSON `{"spaces":[{"vaultId":"a","spaceId":"x"}],…}` deserialises, and a missing `spaces` gives `[]`. This defends the camelCase contract for a shell crate that cannot build here.
  - `search_drives_prefer_the_selection_then_the_spaces_drives_then_home`: the selection wins; with an empty selection, spaces on `[B, A, B]` give `[B, A]`; unscoped gives `[home]`.
  - `other_drives_watch_a_lens_drive_outside_the_selection`: home is excluded; a space on C with the selection `[A]` gives `[C]`.
  - `a_drive_is_narrowed_by_its_own_spaces_or_by_all_of_them`: the scope `[X@A, Y@B, Z@A]` gives A → `[X, Z]`, B → `[Y]`, C → `[X, Y, Z]`; an empty scope gives nothing; order is kept.
  - `a_union_keeps_what_some_space_would_keep_alone`:
    - A capped at 2 admitting `[0,1,2]`, with B uncapped admitting `[2,3]`: keeps `{0,1,2,3}`.
    - A cap 1 over `[0,1]` and B cap 1 over `[1,0]`: keeps `{0,1}`.
    - A note beyond every admitting cap is dropped.
  - `rail.rs`: the uncategorized row's restore is opaque. Mutation: `false` turns it red.
  - Since A2, `merge.rs`: `a_member_fails_only_the_drives_that_need_it_and_is_resolved_once`, `a_member_no_searched_drive_needs_is_never_resolved`, `a_union_is_ordered_by_its_first_member_that_resolved`. Mutation (FixRust, restore checked with `cmp`) killed each of these: resolving every member up front, ignoring a failed picked member, removing the no-own-members fallback, and `first()` returning the last resolved member.
- *Shell* (by inspection, awaiting CI macOS):
  - A new `notes_vault.rs` desktop test, `a_scope_of_two_spaces_searches_each_drive_through_its_own`. Vault A has X = `tag:a`, vault B has Y = `tag:b`, and each holds one note tagged `a` and one tagged `b`.
    - `vault_ids = [A,B]`, `spaces = [X@A, Y@B]` → A's `a` and B's `b` only.
    - `spaces = [X@A]` → A's `a` and B's `a` (FR-629 unchanged).
    - `spaces = [X@A, Z@A = tag:c]` → both drives through X ∪ Z.
    - X with `limit: 1` and two `a` notes, in a union with Y → exactly one `a` note plus Y's.
    - (A2) `vault_ids = [A,B]`, `spaces = [X@A, gone@B]` → A's `a` notes, plus a notice naming the drive (`Archive`) and the space (`gone`). A member on a drive nobody searches narrows nothing and cannot fail the list. Restoring the hard `?` must turn this red on the gate.
    - (A4) The two capped-union cases, `[cap@A, Y@B]` → `A a1`, `B b` and `[cap@A, Z@A]` over A → `A a1`, `A c`, are the correctness check for the single-pass mask.
  - The stream test at the old `:6752` is extended: `vault_ids: []` with `spaces: [secondary]` wakes, and clearing `spaces` stops the waking.
  - `restored_chips_can_widen_a_space_without_changing_its_identity` is migrated to the slice form, plus two cases: a note admitted only by the second lens passes, and a note no lens admits fails.
- *TS:*
  - `notes-filters.test.ts`:
    - *toggling a space from another drive names both on the wire*: enter X@v1 (restore `{work: include}`, text `budget`), add `draft` excluded, toggle Y@v2. Expected: `spaces` = both with their drives, `spaceTerms: true`, `tags: {draft: exclude}`, `text: null`, `enteredSpace: null`.
    - *toggling back to one space applies its lens without re-entering it*.
    - *the last toggle-out clears the scope and keeps the bar*.
    - *one id on two drives is two members* (Uncategorized@v1 and @v2).
    - *Esc walks a union down one space at a time*.
  - `space-list.test.tsx`:
    - *⌘-click adds, a plain click replaces*: a plain click on X, `metaKey` on Y (both `aria-pressed`), `ctrlKey` on Z, then a plain click on Y gives `[Y]` with its restore applied.
    - *⌘-click on All notes clears; on a group it folds*.
    - *leaving a union by plain click parks nothing* (`notesSpacePark` not called).
    - *deleting a member removes only it*.
    - *⌘+Space toggles from the keyboard*, and `fireEvent.keyDown` returns `false` (default prevented, A11); *a held chord toggles once, not on every key repeat*.
    - *the menu offers Add to selection after Open space, and Remove from selection on a member*.
    - A11, queued clicks: *still lands: the ⌘-click's own change is not an edit that cancels it*; *yields to a search typed after the click*.
  - `note-filter-bar.test.tsx`:
    - *one chip per space, each × removes only its own*: the test clicks `Clear scope Inbox — Personal` and only that member goes; the hints name the drives when drives differ.
    - *the chooser offers each space's own drive and marks the rest outside*: "in" rows send that space and drive; an outside row sends the first member.
    - *Save as space is refused for a union and says why*.
  - `notes-pane.test.tsx` (A11): *says why ⌘⇧S cannot save a selection of several spaces*: an alert with `SAVE_UNION_REFUSED`, and no naming popover.
  - `use-notes-changes.test.ts`: *a union is read from the active drive and refreshed by any of its drives*. `notesList` is called with `("v1", { spaces: [… v2 …] })`, the subscription covers v2, and a v2 batch triggers a refetch.
- *Fix-wave mutation record (FixFront):* each of these turns a named test red: the repeat guard removed; `preventDefault` removed; a plain click taking its generation at start; the queue's own changes counted as edits; the ⌘⇧S union check removed; the chip × label without the drive.
- *On hesperia:* two drives. ⌘-click `Bali` under Work and `Journal` under Personal. The list shows Work's Bali notes and Personal's Journal notes, and each chip's hint names its drive. A plain click on `Bali` narrows to Bali with its search in the bar.
- *On hesperia (A2):* eject Personal's drive. Work's Bali notes still list, with a notice naming Journal and Personal.

**Retargeted tests:**
- `notes-filters.test.ts:114`, `:122`: `query.spaceId` → `query.spaces`.
- `notes-filters.test.ts:133-159`: `spaceId: "B"` → `spaces: [{ vaultId: "vault-1", spaceId: "B" }]`.
- `notes-filters.test.ts:164`, `:180`, `:195-196`, `:210`: → `spaces: []`.
- `recordings-space.test.ts:73`, `:95` and `recording-pane.test.tsx:1220-1223`: `toMatchObject({ id })` → `{ spaces: [expect.objectContaining({ id })] }`.
- `space-list.test.tsx`: `:353`, `:374`, `:378`, `:384`, `:393`, `:992` take the same `spaces` form; `:465-472`, `:490-497` take the full `{ kind: "space", spaces: [{…}] }`; `:979-984` also migrates.
- `note-filter-bar.test.tsx:267`, `:281`: state literals. `:280-318` (79.11's chooser) passes unchanged, which proves one space is byte-identical.
- `notes-pane.test.tsx`: the `evaluate` fake at `:196-230` reads `query.spaces[0]?.spaceId`, and the comment at `:974` is updated.

No test is deleted: each one pins behaviour that survives in the new shape.
**Not in this story:**
- History following the scope (81.5).
- Saving a union (DW-284).
- Meaning for prompts inside a union (DW-283).
- A space storing its drives (DW-268).
- A phone multi-select gesture.
- Pruning members on drive deselection (P10).
- `mergeSpace`.
- The Uncategorized "outside" wording (D5).

**binds:** FR-669, FR-670, FR-671, FR-672, FR-673, FR-674, NFR-88, AD-306, UX-DR111, FR-629 (generalised), AD-294 (amended)

### 81.5 — History takes its space with it
**Intent:** "jak chodzisz wstecz/naprzod po notatkach - uwzglednij aktywne space", as the owner resolved it: the space follows history. **Claim:** 3. **Rung:** **epic81-spaces**, above 81.4, because the stamp and `sameScope` are the multi-space shapes. AD-305.
**Files:**
- `src/lib/stores/panels.ts`:
  - `export interface PanelHistoryEntry { target; scope: NoteScope | null }`. `Panel.scope`, `back`/`forward: PanelHistoryEntry[]`, and `replaced.was: PanelHistoryEntry | null` with the history in the same shape.
  - `makePanel(target, folded = false, scope = null)`. `initialPanels`, `hydratePanels` and `closeTarget`'s empty panel stamp `null`.
  - `setActiveTarget` stamps the current scope and pushes `{ target: panel.target, scope: panel.scope }` only when there is a target.
  - `navigate` moves entries, sets `target`/`scope` from the landed entry, and after `setState` and `persist` calls `restoreScope(entry.scope)` when the target is a note and the stamp is not null.
  - `openPanel`'s restore branch puts `was.target`/`was.scope` back, and its fill-empty branch stamps the current scope.
  - `appendBeside` stamps the current scope.
  - `retargetPanels` keeps `scope`, since a rename is the same note.
  - `closeTarget` also removes the target from every panel's `back`, `forward` and `replaced` (R3), through `withoutTarget` (A11). It drops the target, collapses adjacent duplicates (keeping the later), drops a top step equal to what the panel shows, prunes `replaced.back`/`replaced.forward`, and nulls `replaced.was` when it was the target. The early return becomes "nothing shows it and no stack holds it".
  - The import `notesFiltersStore, type NoteScope` from `notes-filters.ts` is acyclic, because `notes-filters.ts` imports only zustand, the IPC client and `all-spaces`.
- `src/lib/stores/notes-filters.ts`:
  - `railSpaces: readonly NoteSpaceVm[] | null`, `setRailSpaces(rows)`, and `restoreScope(stamp)` per AD-305's four steps. Since A11 the restored scope is built from the validated rail rows (`scopeSpaceOf`), so a renamed space shows its current name.
  - `resetNotesFiltersStoreForTest` resets `railSpaces` to `null`.
- `src/components/notes/space-list.tsx`: `setRailSpaces(rows)` after `setSpaces(rows)`. `setRailSpaces(null)` runs when there is no vault, when the vault changes, and when the rail unmounts (A11): no rail means no space restore, as on the phone.
- `src/components/notes/note-editor.tsx`: `NavigationButton`'s `entries: readonly PanelHistoryEntry[]` renders `entries[len - 1 - index].target`. No UI change.

**Acceptance:**
- `panels.test.ts`, a new `describe("history follows the notes scope (AD-305)")`, with `resetNotesFiltersStoreForTest()` in `beforeEach`:
  1. *going back restores the scope the note was opened under, and forward returns*: `setRailSpaces([X,Y])`, enter X, open N1, enter Y, open N2. `back()` gives scope X with X's restore in the bar, and `forward()` gives Y.
  2. *a search the person typed survives the scope following history*: the same, with `setText("tax")` before opening N2. `back()` gives scope X, text `tax`, `enteredSpace: null`, `noteQueryFor(..).spaceTerms === true`, and none of Y's restored chips.
  3. *a stamp naming a space the rail no longer lists leaves the scope alone*.
  4. *stepping onto a file leaves the scope alone*: the file is opened under X and stepped back onto under Y, so a wrongly applied stamp would show as X. (A11: the first version opened it under Y and could not fail.)
  5. *a direct jump applies the landed entry's stamp*: `back(undefined, 2)`.
  6. *a target restored from the cookie carries no stamp*.
  7. *the double-click restore keeps the displaced note's stamp*.
  8. *a union stamp is restored whole*: toggle X and Y, open N1, enter Z, open N2, then `back()` gives `[X, Y]` with the bar cleared of Z's restore.
- `notes-filters.test.ts`, `restoreScope`:
  - same scope → no store write;
  - untouched bar → enters the space, with `enteredSpace` set and its restore applied;
  - a drifted bar is kept and `enteredSpace` cleared;
  - `railSpaces === null` → nothing for a space stamp;
  - a row with `error` → nothing;
  - an `all` stamp with an untouched bar clears the bar;
  - a folder stamp is applied.

  Mutation-proved: skipping `withoutRestored` turns case 2 red; skipping the rail check turns case 3 red.
- *Stamps stay transient (NFR-91)*: after navigation the cookie written by `persist` is byte-identical to the cookie written without stamps.
- `panels.test.ts`, the prune (A11): *leaves no step that goes nowhere: a doubled neighbour or a top step onto what is shown*; *and a double click cannot bring it back from the preview it displaced* (`replaced.forward`).
- `notes-filters.test.ts`, multi-member stamps (A11): *an untouched bar takes the union with an empty bar, under the names the rail reads now*, and its typed-bar twin: the search stays, the outgoing space's chips go, and the names are the rail's.
- `space-list.test.tsx` (A11, replacing the store-plumbing test "hands the rows it read to history…"): *Back restores the space while the rail lists it*; Back leaves the scope alone after a drive change; *with the rail unmounted, Back leaves the scope alone*.
- Mutation (FixFront): each of these turns a test red: restoring the stamp's names, removing the collapse, removing the top-drop, not pruning `replaced.forward`, not clearing the rail on unmount, and removing the file-step note guard.
- *On hesperia:* enter Bali, open a note; enter Journal, open another; press Back. The rail marks Bali, the list is Bali's, and the note is in it. Type `tax` in the bar, press Forward: Journal with `tax` still in the bar.

**Retargeted tests:**
- The panel literals at `panels.test.ts:482`, `:493`, `:533`, `:546-552`, `:857-863`, `:887-889` and `app-shell.test.tsx:240-246`, `:581-587` gain `scope: null`.
- `note-navigation.test.tsx` is unchanged: it seeds through verbs and asserts targets.
- `panels.test.ts:50-123` (independence, cap, truncation, jump, non-note targets, never persisted) is unchanged, and that is the point.

**Not in this story:**
- Skipping out-of-scope entries, or a Rust membership check (the owner chose "follows").
- A scope chip in the history menu.
- Persisting stamps (AD-241).
- The phone, which has no rail: space stamps are never applied there, and that is honest.
- Validating a folder stamp whose folder was deleted (bp-closenav Q4: applied as given; the list then shows the folder's empty state).

**binds:** FR-675, FR-676, NFR-91, AD-305, AD-241 (amended)

## Review-wave amendments

Three adversarial reviews read the wave's tree:
- RevFront reviewed `src/**` and `dev/mock-shell.ts`.
- RevShell reviewed the shell hunks for compile, clippy and logic. It found no compile or clippy failure: it ran `rustfmt --check` and linted a probe crate over the least readable constructs.
- RevCore reviewed the Rust logic of AD-303, AD-304 and AD-306.

The coordinator amended the frozen contract with A1–A11, and fix lanes FixRust and FixFront shipped them. They are written here because the code shipped with them, and a reader of the ADs alone would be told the wrong thing. Where a bullet and an AD disagree, the bullet wins. Every shell mechanism below is **by inspection, awaiting CI macOS**.

- **A1 — A note still on screen is never discarded (AD-304; amends Story 81.3).**
  - *Answers:* RevFront [major]. Folding a panel, or switching the primary view (Notes → Files), unmounts the editor while the panel keeps its target. The release then removed a new untouched note the person could still see, and `closeTarget` closed or blanked its panel. It also answers RevCore [minor] and a RevFront observation: the orphan close always sent `null`, so a new note left before its `notes_open` resolved (⌘⌥N twice, quickly) outlived the session.
  - *Shipped:* `NoteReleaseReq` gains `discard: bool` (`keeper-core/src/notes/vm.rs`; TS `discard: boolean`, regenerated). `release_subscription` removes only when `discard` is true. It still requires the last subscription, a pristine note and a byte-proof match. With `false`, the pointer stays in `PRISTINE`, so a later release still decides. The webview computes `discard` in `mayDiscard` (`use-notes-body.ts`): true only when no panel targets the note (`sameTarget`). The orphan path sends `{ text: null, baseRev: "", discard: <the same rule> }` when `readNoteDocument(..).views === 0`, and `null` otherwise, as bp-closenav §3.5 designed. This is what FR-665's "lets it go" means: a fold or a view switch keeps the note.
- **A2 — One unreadable space costs its drive, not the list (AD-306; amends Story 81.4; supersedes ruling P7 for unions).**
  - *Answers:* RevCore [major] and RevShell [minor]. `project_vaults` resolved every member before choosing a drive. A deselected, ejected or remotely deleted member therefore failed the whole list, including drives it did not narrow, and the stream's `Err` branch then left stale rows.
  - *Shipped:* keeper-core `merge::DriveScope` (`new`, `for_drive(drive, resolve)`, `first()`) resolves a member only when a searched drive needs it, at most once per list. `merge::for_drive` stays the public rule. In the shell, `member_lens`, `scope_members`, `home_of` and `unreadable_member` replace `ScopedLens`/`scope_lenses`. In the multi-drive loop, a failed member turns that drive into a notice, `Drive {drive} could not be listed: the space {title or id} on {home drive} could not be read — {message}`, and the other drives still list. The single-drive path resolves only the members its drive needs and fails as before, so a single space behaves as it did. The union's prompt and ordering come from `first()`, the first member that resolved. This tightens NFR-88's "resolved once" to "at most once, and only when needed".
- **A3 — The spaces revision is taken before the list (AD-303; amends Story 81.2).**
  - *Answers:* RevCore [minor]. The revision was computed only after `current_window` succeeded. So the one space edit it exists for sent nothing: deleting the space you are in, or breaking its query. The rail kept the stale row.
  - *Shipped:* `stream_changes` computes `space_definitions_revision` before `current_window`. On `Err`, when a batch has already gone out and the revision moved, it sends an empty-ops `NoteChangeBatch` with the last counts and records the revision. The webview's refetch then shows the list's error, and the rail reloads.
- **A4 — A capped union evaluates each lens once (AD-306, NFR-88; amends Story 81.4).**
  - *Answers:* RevCore [minor, perf] and RevShell [minor, cost]. The cap pass re-tested every matched entry per lens with a fresh `body_reader`. Body-reading queries therefore re-read each candidate once per lens, on every read and every stream wake. That is the common case under DW-289.
  - *Shipped:* when `capped_union` holds (two or more lenses, one of them capped), the first pass applies `matches_filter` first. It then evaluates every lens over one memoised `body_reader` and records a flat admission mask (`admissions[admitted_at[path] + k]`). The per-space cap step reads the mask. NFR-88's "at most once more per capped lens in the cap pass" is now zero.
- **A5 — The sweep decides each note at its unlink (AD-304; amends Story 81.3).**
  - *Answers:* RevCore [minor]. `held_open` was a snapshot taken before the `PRISTINE` lock. An editor releasing in between left a note adopted with nobody holding it, and a panel that registered after the snapshot had its note unlinked under it.
  - *Shipped:*
    - `sweep_pristine` skips a note already in `PRISTINE` and reads the bytes outside the lock.
    - Then, under `PRISTINE`, it checks again and asks `subscriptions()`. A held note is adopted, inserted and counted before the `SUBSCRIPTIONS` guard drops, and it stays in the row. Otherwise the note is unlinked if untouched and dropped from the row.
    - The lock order is written in the doc: gate → `PRISTINE` → `SUBSCRIPTIONS`.
    - Vault IO stays outside the lock, so a window between the decision and the unlink remains. The doc names it: an editor that opens the note in that window finds it gone, and its next save writes it back.
- **A6 — A release after one's own autosave leaves no conflict copy (AD-304; amends Story 81.3; closes DW-287).**
  - *Answers:* RevShell [minor]. An in-flight autosave wrote "abc" from r1 to r2, and then the release carried "abcd" against r1. `flush_on_release` saw a stale base and wrote a conflict copy of the person's own earlier words.
  - *Shipped:* `BodyState.written: Option<String>`, set only in `write_through`, is the revision this subscription last wrote. `flush_on_release` takes the disk revision as its base when it equals `written`, and otherwise keeps `base_rev`, so another writer's edit still yields a conflict copy.
- **A7 — An ordinary close does not wait on the settings database (AD-304, NFR-89; amends Story 81.3).**
  - *Answers:* RevCore [minor, perf]. Every last close took the global `PRISTINE` lock, which was also held across registry round trips, and the tray's New Note runs on the macOS main thread.
  - *Shipped:*
    - `PRISTINE_HELD: AtomicUsize`, stored under the lock after every change, lets `forget_pristine` return without locking while nothing is pristine.
    - Row read-modify-write moved to a separate `PRISTINE_ROWS` mutex (`edit_pristine_row`, the sweep's row read), so `PRISTINE` guards map operations only.
    - The old removal helper is split into `reads_as_created` and `remove_untouched`.

    One deviation, recorded by the lane: while any note is pristine, an ordinary close still takes `PRISTINE` briefly, for a map operation with no IO.
- **A8 — The service-files desktop test never reads the user's registry (AD-303; amends Story 81.2).**
  - *Answers:* RevShell [minor, test hygiene]. With `hide_service_files: true`, the test reached `registry::get_service_file_names` through `DesktopPlatform`, which means the owner's real settings on hesperia.
  - *Shipped:* a `ScratchPlatform` test platform whose data dir is a temp dir (`notes_vault.rs`). The test's `#[cfg(desktop)]` was removed along with `DesktopPlatform`, the only reason it had.
- **A9 — Two views of one note stay one text (AD-307; amends Story 81.1).**
  - *Answers:* RevFront [major]. With the reconcile keyed on `externalEdition`, a second view of the same note never caught up after the first view's save, and one keystroke in it wrote a buffer without the first view's words. The old `base`-keyed effect had resynced it by accident.
  - *Shipped:* `applyExternal` becomes `adopt(text, flash)`. The editor's store subscription mirrors the store's `text` into its view, with no flash, when `text` changed, `externalEdition` did not, and the document still has views. Anything that moves `externalEdition` stays with the reconcile effect (the flash, the caret hint). A save acknowledgement does not change `text`, so FR-661 holds.
  - *Not added:* the contract's "guard against a store text that lags this view's own typing". `editBuffer` runs inside the keystroke's own CodeMirror update, and the subscription fires in the same call, so the typing view already equals `text`. A guard keyed on "the last text this view reported" would wrongly skip a sibling that types the buffer back to that same text.
- **A10 — A panel whose note is not there closes (AD-304; amends Story 81.3).**
  - *Answers:* RevShell [minor]. At startup the sweep runs before any webview calls `notes_open`, so its adopt branch cannot fire then. A panel restored from the cookie onto a swept note opened a deleted file and stayed on it.
  - *Shipped:* `use-notes-body.ts` closes the target through `closeTarget` when `notes_open` fails NotFound. Any other failure keeps the panel and shows the error. `isNoteNotFound` reads `code === "internal"` and a message beginning `no such note:`, which is the Display of `NotesError::NotFound` through `notes_error`. It is the one place to change if the IPC error codes ever gain a not-found code.
- **A11 — Frontend minors (UX-DR111, AD-305; amends Stories 81.4 and 81.5).** Each item answers one RevFront minor.
  - *81.5:* `restoreScope` builds the restored scope from the validated rail rows (`scopeSpaceOf`). A space renamed since the note was opened shows its current name, in both the untouched and the typed-bar branches.
  - *81.5:* `closeTarget`'s prune (`withoutTarget`) removes the target, collapses adjacent duplicates, drops a top step equal to what the panel shows, prunes `replaced.back`/`replaced.forward`, and nulls `replaced.was` when it was the target.
  - *81.5:* `setRailSpaces(null)` also runs when `SpaceList` unmounts. No rail means no space restore, as on the phone.
  - *81.5, test quality:* "stepping onto a file leaves the scope alone" now opens the file under X and steps back under Y, so it can fail. The store-plumbing rail test is replaced by three behaviour tests.
  - *81.4:* a held ⌘/Ctrl+Enter or ⌘/Ctrl+Space toggles once, because `event.repeat` is prevented and ignored. The keyboard test asserts `fireEvent.keyDown` returns `false` (default prevented).
  - *81.4:* a queued plain click records its query generation at click time, so a search typed after the click cancels it. Toggles re-base when they start. A queued entry's own scope change runs inside `applyEntry`, which the generation counter ignores, so a ⌘-click cannot cancel the plain click queued behind it.
  - *81.4:* ⌘⇧S on a union shows `SAVE_UNION_REFUSED` through the pane's action error instead of doing nothing.
  - *81.4, amends UX-DR111's chip pin:* when the members span drives, the chip's × is labelled `Clear scope {name} — {drive}`. Two chips with the same name no longer give assistive technology two identical buttons.

**Verification of the fix wave.**
- keeper-core (FixRust): `cargo test -p keeper-core` passed (2797 lib tests and every integration binary), `cargo clippy -p keeper-core -p keeper-sync --all-targets -- -D warnings` was clean, and `cargo fmt --check` passed. `DriveScope` mutants M1–M4 were killed.
- Frontend (FixFront): vitest passed 146 files and 2105 tests, `tsc` was clean, and all 19 mutants were caught.
- Shell: A1, A2, A3, A5, A6, A7 and A8 are by inspection. A probe crate over the new shell constructs is clippy-clean. The four new or extended shell tests (A1 kept-on-screen, A6 in-flight, A5 adopt, A2 union notice) have **not run and are not mutation-proved**. Their mutations are listed in 81.3 and 81.4 for the macOS gate.

**Not amended in this wave, for the coordinator:**
- RevFront [minor]: a ⌘-click is dropped silently when a query edit or a history restore lands during `notesSpaceTouch`, and the temporary space's lifetime has already been reset. FixFront left it to be fixed or ledgered.
- Pre-existing, named by FixFront: words typed into a brand-new note before its `notes_open` resolves are dropped when the note is left. The cleanup sees `subscriptionId === null`. The release now lets Rust remove the untouched file, but those keystrokes were never in the release.
- Pre-existing, named by FixFront: a keystroke in the old view between a `noteId` prop change and the effect cleanup goes to the new document.
- Pre-existing, named by RevCore: a failed release flush returns `Err`, and the webview drops it silently.

None of these has a DW number here, because the ledger is the coordinator's.

## What stays out

- **Skipping history entries outside the active space.** The owner chose "the space follows". Skipping would also need a Rust membership check per step, because TypeScript cannot evaluate a space.
- **A space that remembers several drives, or a union saved as one space.** DW-268 stands: a file naming drives by id stops meaning the same thing on another machine. A same-drive union has a query shape waiting in DW-284.
- **Hiding space definitions anywhere but the plain list.** The rail, the Files tree, ⌘⇧F and links keep them (FR-571). A space's own query decides what it shows.
- **New names in `DEFAULT_SERVICE_FILE_NAMES`.** Arbitrary names, and lists that never receive a default (triage row 1).
- **Trashing an untouched note.** Trashing would leave the empty note behind under another name (D-24).
- **Removing a folder a removed note leaves empty** (Q1). The folder may be the person's.
- **A quit flush for notes.** A real gap, and older than this epic (DW-286).
- **Pruning union members when their drive leaves the selection** (P10). The per-drive rule stays total.
- **A visible checkbox lane in the rail.** UX-DR111 declines it: the menu verb is the visible door, and UX-DR104 removed hover buttons.
- **`spaceDrift`/`resetToEnteredSpace`'s missing callers (D3) and the Uncategorized "outside" wording (D5).** Both are pre-existing and not widened here.
- **The add-and-remove commit pair a removed note leaves in git** (bp-closenav Q2). The pinned text accepts it; the bytes in both commits are keeper's own.
- **Recording surfaces.** Untouched, by the coordinator's guard.

Deferred, with the ledger entries allocated here so a later planner finds them:

```markdown
### DW-283: Saved prompts inside a union narrow by words and do not rank or use meaning.

origin: epic 81's plan, 2026-09-23 (AD-306, bp-spaces P2)
location: `src-tauri/crates/keeper/src/notes_ipc.rs` (`project_list`'s `LensTest.words` / `PromptWords`, `sole_prompt`, `LAST_EMBEDDING`), `src-tauri/crates/keeper-core/src/notes/merge.rs` (`merge_rows`)
reason: inside a union, a space's saved prompt selects through the lexical pool and does not rank. A single space is unchanged and still ranks by its prompt with meaning. Ranking a union would cost one embedding per distinct prompt per list refresh. `LAST_EMBEDDING` caches one, so every keystroke and every stream wake would call the provider once per prompt. `merge_rows` would then compare scores computed from different texts across drives. Only unions holding text-bearing spaces (parked and bar-saved searches) are affected. Revisit when a union is reported missing a note that one of its spaces finds alone. The shape is a per-prompt embedding cache keyed by text, plus a rank rule that never compares scores across texts.
status: open

### DW-284: Save a same-drive selection of spaces as one space, `(a) | (b)`.

origin: epic 81's plan, 2026-09-23 (AD-306, bp-spaces P4, UX-DR111)
location: `src/hooks/use-notes-actions.ts` (`captureSpaceDraft`), `src-tauri/crates/keeper/src/notes_ipc.rs` (`compose_space_query`, `notes_space_save`), `src-tauri/crates/keeper-core/src/notes/query.rs` (the `|` operator)
reason: *Save as space* is disabled for two or more members, because one saved space cannot carry per-drive meaning (DW-268). When every member lives on one drive, the DSL can express the union as `(a) | (b)` over the members' composed queries. Prompts (DW-283), per-member caps (`counts::union_keep`) and per-member orderings do not compose into one query, so the saved space would not always list what the union lists. Decide after the owner has used selections. One option composes queries only and refuses when a member has a prompt or a cap. The other saves anyway and says in a sentence what differs.
status: open

### DW-285: A new session file created from a session space is not removed when nobody writes in it.

origin: epic 81's plan, 2026-09-23 (AD-304, bp-closenav P2)
location: `src/components/sessions/session-spaces.tsx:741-753` (the create, `sessionsFileNewKind`) and `:484-488` (opened as a `file` panel target), `src-tauri/crates/keeper/src/sessions_ipc.rs` (`sessions_file_new_kind`)
reason: the pinned AD-304 listed session-space create among the eligible creates. It is not a notes-vault create. It writes a session file with no note id and opens it as a `file` panel target (AD-109) with no body subscription, so neither the pristine pointer nor the release reaches it. Covering it needs a pointer of its own and a release on the file viewer's close, with the same byte proof.
status: open

### DW-286: Quit or crash within the autosave window loses the text, and now the pristine file with it.

origin: epic 81's plan, 2026-09-23 (AD-304, bp-closenav P7)
location: `src/hooks/use-notes-body.ts` (`NOTE_AUTOSAVE_IDLE_MS`), `src-tauri/crates/keeper/src/lib.rs` (main-window close hides; `RunEvent::ExitRequested` flushes no notes), `src-tauri/crates/keeper/src/notes_ipc.rs` (`sweep_pristine`)
reason: no notes flush exists on quit and there is no `beforeunload` in the notes code, so up to 1.5 s of typing that never reached disk is lost, as before this epic. What is new: if that note was an untouched new note, its file is still pristine on disk, and the next start's sweep removes it. Nothing on disk is lost, because the words were never in the file. But the note the person had just started is gone, where before it would have been left empty. A bounded quit flush would close both: Rust asks each webview for its dirty buffers and waits a fixed time.
status: open

### DW-287: A stale unmount flush writes a conflict copy of the person's own words.

origin: epic 81's plan, 2026-09-23 (AD-304, bp-closenav Q3)
location: `src-tauri/crates/keeper/src/notes_ipc.rs` (`flush_on_release`, `write_through`, `BodySub.state.rev`), `src/hooks/use-notes-body.ts` (the last-view release)
reason: an in-flight save moves disk from R0 to R1 with text X, then the release carries Y against R0. The flush sees `base_rev != disk` and writes a conflict copy holding X, the person's own earlier words, before writing Y. This is pre-existing `notes_save` behaviour. `flush_on_release` skips only the identical-text case. A fix needs a per-subscription "last revision this subscription wrote". `state.rev` is not that, because `Diverged` sets it to the foreign revision.
status: done (2026-09-23, epic 81 review fix wave A6)
resolution: `BodyState.written` records the revision this subscription last wrote (set only in `write_through`); `flush_on_release` takes the disk revision as its base when it equals `written`, so a release after the person's own in-flight autosave writes no conflict copy. By inspection, awaits CI macOS.

### DW-288: Session zones' `_spaces/` definitions are listed as notes.

origin: epic 81's plan, 2026-09-23 (AD-303; adjacent finding in the service-files triage)
location: `src-tauri/crates/keeper-core/src/sessions/spaces.rs:58`, `src-tauri/crates/keeper/src/notes_vault.rs` (`parse_note`, `is_internal`), `src-tauri/crates/keeper/src/notes_ipc.rs` (`project_list`'s service-file pass)
reason: session zones keep their space definitions at `<zone>/_spaces/*.md` inside the vault tree (e.g. `60-sessions/…`). The index stamps `space` only against the notes spaces folder, so these definitions still appear in the plain notes list. Hiding them needs a second flag, or a sessions-zone rule in `parse_note`. It also needs a decision on whether session zones belong in the notes list at all.
status: open

### DW-289: Every space saved from the bar carries `keeper.limit` equal to the list window.

origin: epic 81's plan, 2026-09-23 (bp-spaces P3, aside)
location: `src/hooks/use-notes-actions.ts:234-250` (`captureSpaceDraft` sends `limit` from `notesListStore`: `NOTES_PAGE_SIZE = 200`, grown by `growWindow`), `src-tauri/crates/keeper/src/notes_ipc.rs` (`notes_space_save` writes `limit` when it is above 0)
reason: the window is a rendering page. `keeper.limit` caps what a space *selects* (Story 44.11, DW-163's resolution). A search saved while the window was 200, or 400 after scrolling, becomes a space that silently selects at most that many notes. In a union (AD-306) that cap is honoured per space. So caps, meant as a deliberate property of a space, are the common case by accident. The draft should send no limit (uncapped) unless the person set one. Spaces saved already keep what they have.
status: open
```

## The failure shape this epic must not repeat

Three, and the first one is the headline bug's own history.

**A test that watched the wrong moment.** The caret tests covered opening only: three cases in `new-note-caret.test.tsx`, none after a save. The defect lived one save later, on every note keeper creates. The fix's first regression survived one mutant because its fixture delivered the snapshot before the editor chunk, which is the opposite of the running app's usual order. It went red only when parametrised over both orders. Every new test in this epic crosses a boundary in time or in order: a save acknowledgement, a release after a save, a save after a release, a Back after a scope change, a stream wake after a hidden edit. Where two orders exist, the test runs both.

**A deletion decided on a guess.** AD-304 is the one change here that could lose words a person wrote. The design answers with the discipline of Story 42.4:
- the decision reads bytes from disk, never what the webview claims;
- the last words travel in the same command as the decision;
- one gate orders every write through the subscription;
- every uncertainty keeps the file.

The thirteen shell tests that prove this run only on CI's macOS job, because the shell crate does not build here. Ten were planned, and three came from the review wave (A1, A5, A6). **81.3 is not done until they have run there, whatever Linux reports.** The frontend tests prove only that the words ride the close and that `discard` follows the panels, not what Rust does with them.

**A stack green only at the tip.** Both rungs touch `notes_ipc.rs`, `notes_vault.rs`, `keeper-core/src/notes/vm.rs`, `client.ts` and `dev/mock-shell.ts`, and both regenerate bindings. The hunk table under *Stories* is the split. One hunk (R3's history prune) is written against the upper rung's type and is wanted by the lower rung's behaviour (81.3's rung note). The shell half of every story is **by inspection, awaiting CI macOS**:
- AD-303's retain and the spaces revision in `stream_changes`;
- the release, the gate, the pointer helpers and the sweep in `notes_ipc.rs` and `notes_vault.rs`;
- `scope_lenses`, `project_list`, `project_vaults` and the stream secondaries.

`epic81-fixes` has to build and pass on the macOS job alone before `epic81-spaces` is stacked on it.

## Sprint-status entry

Paste under `development_status:` above the epic-80 block. The coordinator owns the ledger; this is the text:

```yaml
development_status:
  # Epic 81: the owner's five notes on 0.8.32 — the caret that jumps on save, space definitions as service files, the empty new note that leaves, a scope of several spaces (one or more per drive, a union), history that takes its space with it — triaged, then two items clarified with the owner, before planning.
  # Stack rungs: epic81-plan, then epic81-fixes (81.1 cursor, 81.2 service files + spaces revision, 81.3 empty note), then epic81-spaces (81.4 multi-space, 81.5 history follows scope). Shell crate by inspection; CI macOS is the gate.
  # 81.1 is implemented in the wave tree, and so are 81.2's rule and spaces revision; the coordinator sets statuses at the gate.
  # Stack publication is reserved for the coordinator by this checkout's rules.
  epic-81: backlog
  81-1-the-caret-stays-where-you-type: backlog
  81-2-space-definitions-are-service-files: backlog
  81-3-an-untouched-new-note-leaves: backlog
  81-4-a-scope-is-a-selection-of-spaces: backlog
  81-5-history-takes-its-space-with-it: backlog
  # DW-283…DW-289 are opened by this plan; none closes here.
```

## docs/decisions.md entry

Draft for `docs/decisions.md`, to follow D-23. The number is **D-24**, the next free after epic 80's D-23.

```markdown
## D-24 — keeper removes a new note nobody wrote in, and it unlinks rather than trashes

NFR-30 is the notes phase's one unacceptable failure: no keeper code path deletes or
overwrites a note body without a recoverable copy, and every delete goes to
`<vault>/.keeper/trash/`. Story 42.4 made the first exception. A recording stub proved byte
for byte to be what keeper composed is unlinked, because "trashing it would leave the
empty note behind under another name, which is precisely the litter dismissing exists to
prevent" (`ipc.rs`, `dismiss_stub`). Epic 81 makes the second, for the same reason and
under the same discipline.

- **What changes:** a note keeper created on this device with no title and no words
  (the pane, the phone, the palette, ⌘⌥N, a space row, the tray, an empty *New note
  from search*) is removed from disk when the last editor holding it lets it go and no
  panel still shows it, if its file is still exactly what creation wrote. A quit or a
  crash is caught at the drive's next registration. (AD-304; FR-665…FR-668; NFR-89)
- **The proof it requires:** a pointer recorded at creation, read back from disk,
  never assembled. At release, the file at the path creation wrote must have the same
  frontmatter block once `updated` is set aside, which proves the id and that no tag,
  pin or property was written. Its body must be the same up to surrounding whitespace.
  The last words the editor held are written in the same command before the check, and a
  per-subscription gate means no save can land after it.
- **Every uncertainty keeps the file:** a read error, a rename, a different block, a
  word, a failed flush, a second editor still open, a panel that still shows the note,
  an orphan close whose note was opened again. Leaving an empty note behind is untidy.
  Deleting a note somebody wrote in is the mistake this rule exists to make impossible.
- **Why unlink and not trash:** a trashed empty note is still an empty note, in a folder
  a person has to empty. The removal is staged and committed like any other, so if the
  cadence already committed the creation, history holds it. Those bytes are keeper's
  own by construction.
- **What it is not:** a general empty-note cleaner. It never touches a note that existed
  before this device created it, a wikilink's *create and link* note (it has a title),
  a capture page (reused, Story 45.14), a journal entry, or anything created with words.
  It is not a trash bypass for any other path.
- **Revisit triggers:** a third unlink exception (then fold the three into one guarded
  helper with one proof contract); any report of a removed note that held words (the
  proof is wrong, so stop removing); a notes quit flush (DW-286), which would change what
  a crash can leave behind.
- **Status / owner:** decided. Owner is the architect. Epic 81 (AD-304, Story 81.3)
  implements it; `unlink_untouched` in `notes_ipc.rs` is its only unlink.
```
