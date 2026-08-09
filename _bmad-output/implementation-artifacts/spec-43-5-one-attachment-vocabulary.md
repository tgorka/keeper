---
title: 'Story 43.5: One Attachment Vocabulary'
type: 'feature'
created: '2026-08-09'
status: 'review'
blocking_condition: ''
baseline_revision: ''
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-43-a-note-can-show-you-the-file.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-42-the-recordings-archive.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-4-the-note-stub.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-5-one-tag-vocabulary.md'
---

<intent-contract>

## Intent

**Problem:** a recording note lists four files and keeper can render exactly one kind of them. The
session folder holds a screen track, a camera track, a whiteboard photo, an audio sidecar and a
manifest; `![[…mov]]` becomes a player and every other embed stays a line of blue text. The reason
is that `RecordingNoteTargetKind` answers a narrower question than the surfaces need — it says
`video | file`, which was enough when the only consumer was a Preview menu item and is not enough
for a note body.

The obvious repair is a photo widget and an audio widget beside `recording-embed.ts`. That is the
wrong shape and it is the whole reason this story is written down: three widgets is three parsers,
three resolution paths, three degrade behaviours and three places to fix the next bug. There is ONE
question — *what is this file and how should it be shown* — and Rust already half-answers it.

**Approach:** widen the kind so it names what a file IS, decide it by extension in exactly one
function, and branch the one existing widget at the element. Widen `keeper-recording://`'s
allow-list to the kinds a note now renders inline and nothing else. This is a convergence story in
42.5's sense: it makes one vocabulary carry more, rather than adding a second surface that would
need its own extension table.

## Boundaries & Constraints

**Always:**
- The kind is decided in exactly one place, by extension, never by reading the file. Every surface
  — the note body, the properties panel, the protocol, and 43.8's files tab — reads that one answer.
- A file keeper cannot render inline is still an attachment. It gets a chip carrying Reveal and Copy
  path, because 42.6's rule is that a dead player is worse than a plain link and a bare line of text
  is worse than either.
- The protocol's root, its resolution-by-membership and its canonicalizing containment check do not
  move. What widened is the set of KINDS it will serve.
- No absolute path is rendered, ever (FR-145). The chip shows the file name, its tooltip is the
  note's own relative path, and the absolute path is only ever the argument of an action.
- The frontend joins no path onto any root (AD-65). Both halves of a `keeper-recording://` URL
  arrive from Rust.

**Never:**
- No second embed module, and no second embed syntax. `![[…]]` stays the one form, so a note keeper
  renders is a note Obsidian renders.
- No sniffing of file contents. Targets are composed for a whole folder at once and that folder may
  be a network share.
- No hand-written `src/lib/ipc/gen/*`.
- No transcription, no thumbnails, no derived media.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Video | `screen-0000.mov` | `kind: video`; a `<video controls preload=metadata>`, no other element | none |
| Image | `whiteboard.png` | `kind: image`; an `<img loading=lazy alt="whiteboard.png">`, no other element | none |
| Audio | `room-tone.wav` | `kind: audio`; an `<audio controls preload=metadata>`, no other element | none |
| A file nothing renders | `manifest.json` | `kind: file`; a chip with Reveal and Copy path, no player | none |
| Unknown extension | `board.sketchpad` | `kind: file`; a chip, never a broken player | none |
| Uppercase extension | `camera-0000.MOV` | `kind: video` — case does not distinguish | none |
| Double extension | `clip.mov.bak` | `kind: file` — the LAST extension decides | none |
| Dotless name | `notes`, `Makefile`, `.gitignore` | `kind: file`, never a guess | none |
| A path, not a name | `2026/a.mov/notes.txt` | `kind: file` — only the last component decides | none |
| Folder | the `recording:` line's target | `kind: folder`; the embed stays the link — no element for a directory | none |
| Chip actions | Reveal / Copy path pressed | reveals and copies the ABSOLUTE path composed in Rust | best effort, swallowed |
| No file manager | `revealInFileManager: false` | Reveal absent (not disabled); Copy path still offered | none |
| Chip in CodeMirror | a chip action clicked | the event is claimed, so the caret does not move and the chip survives the click | none |
| Chip name clicked | the chip's own label | the event is NOT claimed — behaves like the wikilink it stands for | none |
| Image/audio fails to load | volume ejected between resolve and request | the link comes back; no broken element left behind | none |
| Session unknown / IPC rejects | `null`, or a throw | the link, unchanged; never a throw out of the render | none |
| Protocol, servable kind | a `.png` inside the root | 200/206 with the allow-list's `Content-Type` | none |
| Protocol, chip's file | `manifest.json` | 404 — nothing renders it, so nothing may fetch it | 404 |
| Protocol, folder | the session folder | 404 — not a servable file | 404 |
| Protocol, escape after canonicalisation | a `.png` symlinked out of the root | 404, asserted | 404 |
| Kind table vs `Content-Type` table | every servable extension | has a `Content-Type`, asserted across the two crates | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` — `RecordingNoteTargetKind` becomes
  `Video | Image | Audio | File | Folder`. `File` keeps its spelling and its wire form, so every
  existing `kind !== "folder"` and `kind === "video"` read survives untouched.
- `src-tauri/crates/keeper-core/src/archive/recordings_fts.rs` — **the whole point of the story.**
  `is_video_name` becomes `pub fn kind_for_file_name`, and the single `VIDEO_EXTENSIONS` table
  becomes three public tables. If a fourth table appears anywhere in the repo, the story failed.
- `src-tauri/crates/keeper/src/recording_protocol.rs` — `resolve_video` becomes `resolve_servable`
  and the kind filter becomes `is_servable`. The root, the URL grammar, `contained_read` and the
  `Range`/404 machinery are untouched.
- `src/components/notes/editor/recording-embed.ts` — one widget, one new `elementFor` branch, and a
  chip. `videoTargetFor` becomes `attachmentTargetFor`; `releaseRecordingVideo` becomes
  `releaseRecordingMedia`.
- `src/components/notes/editor/live-preview.ts` — additive rules on the base theme only.
- `src/lib/ipc/client.ts` — the `recordingNoteTargets` doc, which said "and whether it is a video".
- `src/lib/ipc/gen/RecordingNoteTargetKind.ts` — **regenerated by ts-rs on macOS, not hand-written.**

## Tasks & Acceptance

**Execution:**
- [x] The kind widened, documented, with the epic's vocabulary and Main's naming correction.
- [x] One public classifier plus three public extension tables in `recordings_fts`.
- [x] The protocol serves `Video | Image | Audio` and nothing else; root unchanged.
- [x] One widget branching at the element: `<video>`, `<img>`, `<audio>`, chip.
- [x] The chip's Reveal / Copy path, gated on `revealInFileManager`, reachable inside CodeMirror.
- [x] Tests: every matrix row that is reachable on Linux, plus the two shell tests for macOS.
- [x] Reverts proved: five mutations, each caught by named tests (see Design Notes).

**Acceptance Criteria:**
- `cargo test -p keeper-core --lib` passes, covering each kind including a dotless name, an
  uppercase extension and a double extension.
- `bun run test src/components/notes/editor/recording-embed.test.ts` passes, covering each kind
  rendering its element and no other, an unknown extension being a chip, and a chip's actions
  working.
- The protocol still refuses a path that escapes the root after canonicalisation, asserted.
- The bindings regenerate with no drift.

## Design Notes

**The variant is `File`, not `Document`, and that was a correction worth taking.** The epic prose
says `document`; the enum answers "how should this be shown", and the last variant covers a `.zip`,
a `manifest.json`, a `.partial` from a rotation in flight and an extensionless dotfile. Calling any
of those a document is a misnomer that reads as a bug to the next person. `File` says exactly what
keeper is claiming — that it is a file — and keeping the spelling also shrank the change to zero in
`keeper/src/ipc.rs` and zero in `properties-panel.test.tsx`, both of which already read `file`.

**The three tables agree with `note_protocol::mime_for` by test, because they cannot by
construction.** The kind tables live in `keeper-core` and the `Content-Type` allow-list lives in the
`keeper` shell; there is no type that can hold both. `every_servable_kind_has_a_content_type` walks
all eighteen servable extensions and asserts each has a MIME. A disagreement there is precisely the
dead player 42.6 refuses — an element rendered for a target the scheme then 404s.

**The protocol's allow-list is stated as an opt-in, not as an exclusion.** `is_servable` matches
`Video | Image | Audio` rather than testing "not `File`, not `Folder`", so a sixth kind added later
must be deliberately admitted rather than served because nobody remembered to exclude it. `.pdf` is
the live proof the asymmetry is intentional: it HAS a `Content-Type` and is still refused, because a
PDF renders as a chip and no element ever asks for its bytes.

**The chip fetches nothing, and that is why the protocol did not widen further.** Reveal and Copy
path go through the system, so the note never requests a byte of a file it cannot show. The
narrowest hole the callers need is the one AD-59 asks for.

**One widget, and the branch is nine lines.** Everything expensive — the wikilink that renders
first, the resolve by session id, the four degrade paths, the teardown, the `ignoreEvent` rule — is
shared above the branch. `attachmentTargetFor` changed from `kind === "video"` to
`kind !== "folder"`: the kind now decides WHICH element there is rather than WHETHER there is one,
and a folder is still not an embed because there is no element for a directory.

**`ignoreEvent` widened from `video` to `video, audio, button` for one concrete reason.** Returning
`true` makes CodeMirror skip every handler, which keeps the caret off the line; a revealed line
drops its decorations. Without the `button` term, pressing Copy path would un-render the chip
instead of copying from it. The chip's own NAME is deliberately not in the selector — it is not a
control, and it should behave like the wikilink it stands for.

**Reveal is read from the capabilities mirror, not passed in.** `capabilitiesStore.getState()` is a
vanilla zustand store usable outside React, which is what a CodeMirror widget needs; threading a
`canReveal` option through `live-preview.ts` and `note-editor.tsx` would have touched two files this
story has no business in. Absent rather than disabled on a platform with no file manager, matching
the panel and the recordings browser.

**The two labels are spelled again rather than imported.** `properties-panel.tsx` is React and this
module is in the editor's lazy chunk; pulling React in to share a string would be the more expensive
mistake. This is the same duplication `NOTE_REVEAL_LABEL` already documents against
`RECORDINGS_REVEAL_LABEL` and `REVEAL_IN_FINDER_LABEL` — one affordance, one wording, said in the
places that cannot reach each other.

**What the reverts proved.** Five mutations, each run and each watched to fail:

| Mutation | Caught by |
|---|---|
| Drop the `Image` arm of `kind_for_file_name` | `a_session_types_every_file_it_holds_with_the_one_attachment_vocabulary` (`whiteboard.png` → `File`), `every_extension_resolves_to_exactly_one_kind_and_the_rest_are_plain_files` (`png is on the image table`) |
| `eq_ignore_ascii_case` → `==` | the same two — `camera-0000.MOV` and `room-tone.WAV` fell to `File`, and `mov uppercase is the same video` |
| `attachmentTargetFor` back to `kind === "video"` | 11 of the 29 frontend tests |
| Disable the `image` arm of `elementFor` | 3 tests: the `img` row of the per-kind table, the lazy/alt assertions, the image half of the failed-load degrade |
| Delete the `file` arm of `elementFor` | 6 tests, including the real-editor one |
| `ignoreEvent` back to `closest("video")` | exactly 1: `keeps a chip's actions clickable by claiming their events from CodeMirror` |

**What could NOT be verified on Linux, stated plainly.** The `keeper` shell crate does not build
here — `cargo check -p keeper` fails in `gobject-sys`' build script for want of `pkg-config` and the
GTK development headers, before any of this story's code is compiled. So
`every_servable_kind_has_a_content_type` and `a_widened_kind_did_not_widen_the_root` are **written
but never executed**; they must be run on the macOS host, and a compile error in either is possible.
`a_widened_kind_did_not_widen_the_root` is the assertion the epic's AC names: it builds a real
`archive.db`, symlinks `escape.png` out of the recordings root, and drives `resolve_servable` — the
path the widening actually opened — rather than testing `contained_read` in isolation, which was
already covered and says nothing about whether the new kind path still reaches the check.

Nothing here was verified in a real webview either: jsdom renders no video, decodes no image and
plays no audio, so what the frontend suite proves is which element is constructed with which
attributes, not that a `.wav` is audible. The `<video>` path is 42.6's, unchanged, and shipped.
