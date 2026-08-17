# Spec 53.3 — One title bar, and two folds

story: 53.3
status: review
branch: `work/epic-53-fold-and-merge` (on top of `work/epic-53-tag-chooser-folds`)
baseline_revision: 8c8a3eb
final_revision: ''
binds: FR-316, FR-317, FR-318; AD-102 (narrowed here, explicitly)
sentinel: `MUT53-3`

<intent-contract>

**The ask, verbatim.** *"add option to fold the properties (and the info note at
the top) in the note, source, preview so there will be more space to view the
content, the save bar and the top bar can be the same (title is already repeted)"*

**Three asks, three different states.**

**(a) The properties fold is absent on files and already built on notes.**
`note-editor.tsx:364` holds `showProperties`, defaulting CLOSED, with a header
control at `:793-797` over the region at `:1059-1065`. `TextFileFrame` mounts
`<FileProperties>` unconditionally at `text-file-frame.tsx:539-553`, above the
Preview|Source|Note tabs, so it pushes content down in all three. No reason is
recorded; story 50.4 reused the panel and not the disclosure. The frame already
has a header, and the control's predicate is the panel's own (`savable`), so no
new branch is needed.

**(b) The caveat is protected by AD-102, and this story NARROWS it rather than
deleting it.** `files_write.rs:675-679` — *"Before, and not after. An edit that
quietly does less than the vault path does is strictly worse than the refusal it
replaces: a person who finds out after saving that this file has no history has
already lost the thing history would have given them."* So: the standing fact
stays on screen before the first keystroke, as ONE line composed in Rust beside
`unmanaged_caveat`, expanding to the full four on request. It is never paraphrased
in the webview — `viewers/types.ts:276-282` forbids that, and the short form is
therefore Rust's sentence too.

**(c) The title is rendered twice** — `panel-strip.tsx:559-582` (with the download
icon and the fold/close frame group) and `text-file-frame.tsx:473-500` (with
"Unsaved changes" and Save). The notes surface already proved the merge:
`panel-strip.tsx:452-461` `noteOwnsRow`, `:558` draws no header, `:600` hands the
frame group down into `PaneHeader`'s fourth slot (`pane-header.tsx:226-241`).

**Always**
- Both folds are remembered across a remount. The frame outlives the file it shows
  (`text-file-frame.tsx:305-313`) and a folded panel unmounts its body
  (`panel-strip.tsx:594-601`), so `useState` alone loses the answer.
- The fold state is a standing preference per surface, not per file: a new closed-key
  cookie namespace with two keys, through `readFoldFlags`/`foldFlagsCookie`
  (`fold-cookie.ts:22-25` refuses an open key map, so a per-file key cannot live
  there).
- The merged bar carries the name once, the download control, the save word, Save,
  and the fold/close frame group.
- **The panel keeps its own header row unless the frame is certain to draw one.**
  `TextFileFrame` renders NO header while loading (`:382-386`), for an unreadable
  file (`:394-398`) and for binary bytes (`:400-404`), and
  `text-file-frame.test.tsx:374,384` pin that. A naive port leaves five states with
  no title at all — the asymmetry `panel-strip.test.tsx:701-712` already encodes
  for notes.

**Block if**
- The file is not savable: there is no save bar to merge into, so the panel's own
  row stands, unchanged.

**Never**
- Never remove the standing fact that a file outside the vault has no history. Fold
  it to one Rust-composed line; never to nothing, and never to a webview
  paraphrase.
- Never default the properties panel open on the file surface once the fold exists —
  the notes surface defaults closed, and two surfaces disagreeing is the drift this
  epic is already paying for elsewhere.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src-tauri/crates/keeper-sync/src/files_write.rs:675-703` | a one-line form beside `unmanaged_caveat`, with its own test |
| `src-tauri/crates/keeper/src/sync_ipc.rs:1702-1704` | carries both forms on the row |
| `src/components/viewers/text-file-frame.tsx:473-553` | the merged header (identity + download + save word + Save + frame group), the properties disclosure, the caveat disclosure |
| `src/components/layout/panel-strip.tsx:452-461,558,600` | the file case joins `noteOwnsRow`'s shape, guarded on the frame being certain to draw a row |
| `src/lib/stores/` | a new closed-key fold namespace with `properties` and `caveat` |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | the properties panel folds and unfolds from the pane header, and defaults closed like the notes surface |
| 2 | the fold survives a remount and a panel fold/unfold |
| 3 | the caveat shows one Rust-composed line by default and the full four on request; the short line still names what is missing |
| 4 | the caveat's short form comes from Rust, not from truncation in the webview |
| 5 | a savable file shows ONE title bar carrying the name, download, the save word, Save, and fold/close |
| 6 | a loading, unreadable, binary or non-savable file still has a title bar — the five states a naive merge would strand |
| 7 | `text-file-viewer.test.tsx:467`'s AD-102 guard is re-anchored to the narrowed rule, with a comment naming this story, and `spec-46-14`'s AD-102 section records the override |
| 8 | the measured vertical gain is stated in the spec's Verification from a real render, not estimated |

## Design Notes

**The guard is a promise the frame keeps, not a condition the panel evaluates.**
`savable` is decided inside `TextFileFrame` from `entry.writable`, Rust's
`writeRefusal` and `vm.oversize`, and the last of those only exists after the
read lands — so a panel that gave up its row on a guess about `savable` would
strand five states. Instead: a frame handed `frame` draws a `PaneHeader` in EVERY
branch, loading and unreadable and binary included, and the panel gives its row
up only when the REGISTRY has promised a row exists at all
(`ResolvedViewer.ownsHostRow`, `VIEWERS_OWNING_HOST_ROW` in `components.tsx`). A
`.pdf` resolves to a viewer with no chrome and keeps the panel's row.

**That is why `useFileResolution` moved up into `PanelFrame`** — the same lift
50.1 did for `noteVaultReason`, and the same reason: two decisions turn on the
answer and must not be able to disagree. It carries `folded` now, because the
thing that used to stop a folded panel browsing was the body being unmounted, and
the hook is above the body.

**The fold is a closed two-key cookie namespace** (`file-frame-fold.ts`,
`keeper_file_frame_fold`), hydrated from `TextFileFrame` — the only surface these
two keys belong to, unmounted for whole sessions, and the one place the call can
be forgotten (DW-172).

**The panes take the block back when the form folds away.**
`frontmatterInForm` is `null` while the properties fold is closed: what the form
draws is what the panes hide (52.3), so with no form on screen the document has
to draw the `---` block or a file's `tags:` would be on screen nowhere at all.
The carried block is deliberately not forgotten, so unfolding costs no re-read.

## Verification

### Commands

| command | result |
|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync files_write` | **38 passed**, including `the_short_caveat_still_names_what_is_missing` and the unchanged `the_caveat_names_what_is_missing_without_overstating_it` |
| `cargo test … -p keeper-core vm::tests::a_write_verdict…` | **1 passed** — the three states, now with `caveat_short` |
| `cargo test … -p keeper-core export_bindings_fileswritevm` | **1 passed**; `src/lib/ipc/gen/FilesWriteVm.ts` regenerated with `caveatShort: string \| null` (never hand-edited) |
| `npx tsc --noEmit` | clean for every file this story touches |
| `npx vitest run src/components/viewers/text-file-frame.test.tsx src/components/layout/panel-strip.test.tsx` | **64 passed** (37 + 27) |
| `npx vitest run src/lib/stores/file-frame-fold.test.ts src/components/viewers/text-file-viewer.test.tsx` | **51 passed** (7 + 44) |
| `npx vitest run src/lib/viewers src/components/viewers src/components/export/export-controls.test.tsx` + `files-pane`/`pane-header` | **585 passed** — the registry, the other viewers and every fixture the new `ViewerFile` field reached |

### The vertical gain, summed from a real render

Every number below is read off the class chain a real `TextFileFrame` render
produced (dumped from a throwaway vitest harness, then deleted), against
Tailwind v4's default spacing scale — this project defines no `--spacing`
override, and `text-xs` is not redefined in `index.css`, so it is 12px/16px.

**1. The merged row: −40px, exactly.** The panel's own header was
`<header class="flex h-10 shrink-0 items-center gap-2 border-border border-b px-3">`.
`h-10` is 40px and `pane-header.tsx` measures it as 40px INCLUDING the hairline,
so the whole band and its seam are gone: **40px**.

**2. The properties band, folded: −81px** for a markdown file with no
frontmatter, which is the commonest one. Rendered chain:
`section.flex.flex-col.gap-1.border-b.px-3.py-2` holding the property grid and
the add-a-property row.

    py-2 top                              8
    grid row (tags cell, Button h-6)     24
    gap-1 between the two children        4
    add row: pt-1 (4) + Button h-8 (32)  36
    py-2 bottom                           8
    border-b                              1
    ---------------------------------------
                                         81px

A file WITH frontmatter is larger by 24 + 4 per property row, so 81px is the
floor rather than the typical case.

**3. The caveat band, folded to Rust's one line.** Rendered chain:
`div.flex.items-start.gap-2.border-b.px-3.py-1.5` holding
`p.flex-1.text-muted-foreground.text-xs` and a `size-8` control with `-my-0.5`,
so the control contributes `32 − 2 − 2 = 28px` of content height and the band is
`6 + max(16 × lines, 28) + 6 + 1`:

| lines the sentence wraps to | band height |
|---|---|
| 1 | 6 + 28 + 6 + 1 = **41px** |
| 2 | 6 + 32 + 6 + 1 = **45px** |
| 3 | 6 + 48 + 6 + 1 = **61px** |
| 4 | 6 + 64 + 6 + 1 = **77px** |

The 4-line case is the epic's 77px figure, arrived at independently here. Rust's
two forms are **274** and **94** characters (2.91×), so wherever the full sentence
takes 4 lines the short one takes at most 2: **77 → 45, a 32px gain**; where the
full takes 3, the short takes 1: **61 → 41, 20px**.

**Total for the owner's `AGENTS.md` — a markdown file outside the vault, both
bands folded, one title bar:** 40 + 81 + 32 = **153px** at a 4-line caveat, and
40 + 81 = **121px** for a file keeper manages (no caveat band at all). The exact
line count is a function of the panel's width, and that is the one part of this
arithmetic no measurement here can pin: this container has no Chromium (the
browser tool's daemon fails to launch) and jsdom lays nothing out, so the wrap
is given as the formula above plus its four cases rather than as one number.

### What could not be verified here

**`src-tauri/crates/keeper/src/sync_ipc.rs` is read-not-compiled.** The `keeper`
shell crate does not link on this box (no `pkg-config`/glib), so the one edit
there — `FilesWriteVm::unmanaged` now taking both forms of the caveat — is
unproven by a build. It is three lines inside an existing `match` arm, calling a
`WriteScope` method whose own tests pass in `keeper-sync`; hesperia and CI are the
gates.
