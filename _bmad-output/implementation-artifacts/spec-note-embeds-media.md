---
title: "Every note shows the file it embeds"
type: 'feature'
created: '2026-08-20'
status: 'done'
baseline_commit: '45e49eac'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `![[photo.png]]` in an ordinary note is a plain link. Audited, not assumed — `embedEntryFor` answers for every target and its answer today is:

| target | ordinary note | session note |
|---|---|---|
| `.csv` `.json` `.jsonl` | panel, editable | panel |
| `.png` `.mp4` `.m4a` | **plain link** | image / video / audio |
| `.pdf` | **plain link** | chip |
| `.md` | link, deliberately (transclusion is a different feature) | link |

So media renders only in a **recording** note, because the only path that resolves it goes through the recordings index. A photograph attached to an ordinary note is a link to a photograph.

**Approach:** The recording widget already answers the right question — *what is this file, and how should it be shown* — with Rust's `kind` and a branch per answer. What ties it to recordings is one thing: it composes its URL from a session id. Lift the element builder out, give it a URL, and let a vault-resolved embed use the same four branches. `notes_embed_paths` already resolves a target to a vault-relative path through the same candidates the viewer and the export use; it gains the `kind` it already computes internally, so the frontend classifies nothing.

## Boundaries & Constraints

**Always:**
- One classifier, in Rust (AD-87). The frontend never maps an extension to media. `kind` comes from `kind_for_file_name`; refinement *inside* kind `file` is the registry's declared job and is how PDF is reached.
- The link is what renders first and an element only ever replaces it. A missing file, an unreachable volume, an IPC failure and a load error all leave the reader the working link they would have had. A dead player is worse than a link.
- A recording note's own files stay the recording widget's. It is asked first and answers whether it claimed the embed; only an unclaimed target goes to the vault.
- No path arithmetic in the webview (AD-65). Rust returns the path it resolved; the URL is composed from that and the vault id, exactly as `assetUrl` already is.
- `preload="metadata"`, `loading="lazy"`: a note with ten videos must not fetch ten videos.

**Ask First:**
- Rendering a `.docx`, `.pptx` or `.xlsx` inline. They are `documentRow`s with no inline renderer, and what a spreadsheet should look like inside a paragraph is a design question.
- Transclusion of `![[note.md]]`.

**Never:**
- No second extension table, no second resolver, no second element builder. If `recording-embed.ts` and this both build an `<img>`, one of them is wrong.
- No remote fetch. Every URL is `keeper-note://`, so a note cannot become a tracking pixel (NFR-11).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Photo | `![[photo.png]]`, vault holds it | `<img>` with the file name as alt | load error: back to the link |
| Video / audio | `![[clip.mp4]]`, `![[voice.m4a]]` | `<video controls>` / `<audio controls>`, metadata only | as above |
| PDF | `![[doc.pdf]]` | the webview's own PDF view, capped height | as above |
| Bare name | `![[photo.png]]` living in `attachments/` | resolves — Rust returns the path it found | N/A |
| Missing | vault holds no such file | the link, unchanged | never a rejection |
| Recording note | `![[screen.mov]]` of this session | the recording widget's, as today | unchanged |
| Recording note, vault file | `![[people.png]]` beside the note | the recording widget declines, this resolves it | N/A |
| Data file | `![[people.csv]]` | Story 45.12's panel, untouched | N/A |
| Unknown | `![[archive.zip]]` | the link | N/A |

</frozen-after-approval>

## Code Map

- `src-tauri/crates/keeper/src/notes_ipc.rs:4010` -- `notes_embed_paths` returns the kind beside the path; `embed_path_opt` already has the name to compute it from
- `src/lib/ipc/gen/` -- the generated VM; `bindings:check` regenerates
- `src/components/notes/editor/media-element.ts` -- **new**: `mediaElementFor({kind, name, url}, onFailedLoad)`, lifted verbatim from `recording-embed.ts:245`, plus the PDF branch
- `src/components/notes/editor/recording-embed.ts:245` -- `elementFor` becomes a URL and a call
- `src/components/notes/editor/vault-embed.ts` -- **new**: the widget for a vault-resolved media target
- `src/components/notes/editor/live-preview.ts:493-530` -- the `![[…]]` branch gains the vault case, after data and after the session
- `src/components/notes/attachments-panel.tsx:303` -- reads `.relPath` off the richer answer

## Tasks & Acceptance

**Execution:**
- [x] Rust: the VM, the command, its doc, and `cargo nextest` for the kind it now returns
- [x] `media-element.ts` + test: one element per kind, the PDF branch, handlers registered before `src`
- [x] `recording-embed.ts`: call it; its existing tests must pass untouched
- [x] `vault-embed.ts` + test: resolves, replaces, degrades on every failure
- [x] `live-preview.ts` + test: the branch order, and that a session note's own file still wins
- [x] `attachments-panel.tsx` and its test: the field, not the string

**Acceptance Criteria:**
- Given an ordinary note embedding a photo, a video, an audio file or a PDF the vault holds, when the note renders, then the file is shown inline rather than linked.
- Given the vault does not hold it, when the note renders, then the link is what remains, and nothing rejects.
- Given a recording note embedding one of its own files, when the note renders, then the recording widget still owns it.
- Given the same `<img>` builder, when a recording note and an ordinary note each show an image, then they are built by one function.

## Verification

- `bun run typecheck` · `bun run test` · `bun run lint` · `bun run test:rust` · `bun run bindings:check`
- Manual: a note with a photo, a video, an audio file and a PDF beside it.

## What the work turned up

**A `folder` target drew an audio player.** `elementFor` in `recording-embed.ts` branched "video, or else audio", and the kind union includes `folder`. `kind_for_file_name` never returns one, so it was almost certainly unreachable for a recording target — but "almost certainly unreachable" and "silently draws an `<audio controls>`" are a poor pair. Lifting the drawable kinds into a type of their own is what asked the question; `folder` now joins `file` on the chip branch.

**A test asserted the behaviour this story changes.** `recording-embed.test.ts` pinned that an ordinary note leaves an embed alone. Half of it was about this module and still is — a note with no `session:` costs no index round trip — and half of it was the gap. Rewritten to say which half is which, rather than deleted or quietly relaxed.

**The renderer wiring needed its own test.** Three files in this directory already carry DW-172's lesson: a widget a test mounts itself cannot prove `live-preview.ts` mounts it. `vault-embed.test.ts` therefore mounts the real decoration layer for the three cases that matter — a photograph renders, a `.csv` is left to Story 45.12's editable panel, and `[[…]]` without the `!` is still a link.

**No Rust read was added.** The first design read a bounded prefix of every embedded file through `notes_embed_read` purely to learn its kind — a file read per photograph, to answer a question `embed_path_opt` already had the name for. `notes_embed_paths` returns the kind instead.
