# Epic 88 — A meeting you can play inside a note

created: '2026-09-29'
status: built 2026-09-29 with the owner's amendments (*Build amendments*, below); stories 88.1–88.5 are in `review`. Planned as `keeper-meeting`; the block is `keeper-media` everywhere below (N1).
source: the owner's message of 2026-09-29, in Polish (its closing paragraph, verbatim below; the rest of that message is story 87.10). Other inputs:
- the coordinator's scope, `local://epic87-fr3.md` § *Epic 88 — plan only* (binding: the design points to pin, the files to read, the ledgers to draft);
- a reading of keeper's notes editor, its embeds and its recording note stub, and of the transcript viewer's player, with every claim below cited `path:line`;
- the public record of how Obsidian and its plugins render named fenced blocks and time links, and the W3C Media Fragments recommendation. Each is cited by URL, read on 2026-09-29, in *Verified facts, with sources*.

Line numbers are in the current worktree: `dfa843e3` (origin/main, PR #410) plus the uncommitted Epic 87 tip, rungs 1–7. `src/components/transcription/**` was being edited by story 87.10 while this was written. Those files are cited by symbol, and by line only where the line was read.
binds: FR-759…FR-766 and NFR-110…NFR-111 (allocated here); AD-351…AD-359; UX-DR124…UX-DR126; DW-347…DW-354 (allocated in *What stays out*); D-30 (drafted at the end, for `docs/decisions.md`).
- **The previous ceilings:**
  - FR-758, NFR-109, AD-350 and UX-DR123 (epic 87's `binds:` line, `epic-87-…:10`);
  - DW-346. Story 87.10 opened it (epic 87's `binds:` line, `epic-87-…:10`; `sprint-status.yaml:69`), and it is the last entry in the ledger (`deferred-work.md:6932`);
  - D-29 (`docs/decisions.md:1429`).
- **No earlier allocation.** On 2026-09-29 a grep found no allocation of any of these ids.
  - **What was searched:** `_bmad-output`, `docs`, `src`, `src-tauri/crates`, `tools`, `dev`, `AGENTS.md`, `README.md` and `CLAUDE.md`, for `epic-88`, `epic88`, FR-759…FR-779, NFR-110…NFR-119, AD-351…AD-369, UX-DR124…UX-DR139, DW-346…DW-369 and `D-3x`.
  - **What it found:** only three things. There were the range statements in two earlier headers, epic-86:12 and epic-87:16. And there was DW-346 itself, which is story 87.10's (`deferred-work.md:6932`, `sprint-status.yaml:69`, `epic-87-…:10`, `:646`, `:967`, `docs/transcription.md:398`), so this epic starts at DW-347.
  - **An older numbering.** `DW-992` (`recording.rs:1771`) and `DW-1272` (`sync-section.tsx:131`) belong to a numbering the ledger used before. Neither falls in this range.
see-also:
- D-21 (a derived index is disposable), D-29 (a recording is transcribed on this Mac; the stub carries no transcription);
- AD-27 (absent rather than disabled), AD-55/AD-56 (decisions in keeper-core, IO in the shell), AD-65 (Rust composes every path; `keeper-sync/src/browse.rs:722-746`), AD-74 (`keeper-file://` serves only synced folders), AD-343 (one file per fact, and why), AD-344 (a transcript is a file beside its media);
- FR-109 (a note's file link may point anywhere in its drive), FR-121 (the vault is the person's), FR-145 (no absolute path in a synced file), NFR-27 (the editor's chunk is React-free), UX-DR44 (a block that cannot draw shows its source and the reason), UX-DR53 (one transport for one moment);
- `docs/notes.md` § *Widgets in a note*, `docs/transcription.md` § *What you see*, `docs/recording.md` § *Transcription*.

## Build amendments (2026-09-29)

The owner asked for the plan to be built, in Polish, with these changes; the coordinator's rulings N1–N5 fix them (`local://epic88-build.md`). Where this document still describes the plan, these win.

- **N1 — the name is `keeper-media`.** "keeper-meeting moze nie najlepsza nazwa moze keeper-media zeby bylo bardziej uniwersalnie (autio, video, meeting, podcast, video etc)". A block plays any recording, transcript or media file, so the name names the content, not one kind of it; the `keeper-` prefix still keeps it clear of plugins. Q1 is closed by it. The grammar is otherwise unchanged: a plain audio or video file is a `[[part]]` with no transcript, and the block plays it and offers *Transcribe* where that is possible. Code: `keeper_core::notes::media_block` (grammar, times, edits, clips, the stub's block, finding blocks, the old-stub rewrite) and `keeper_core::transcription::media::resolve_block` (resolution over facts the shell answers through `MediaLookup`); the shell's `media_block_ipc.rs`.
- **N2 — the old media rendering goes.** "obecne renderowanie video - usun - zle dziala a nowy didget jest lepszy i bardziej intencjonalny". `![[x.mov]]` in a note no longer mounts a player: it is a chip with the file's name and kind, *Reveal*, *Copy path* and **Play in a player**, which asks Rust for the replacement (`media_block_for_embed`): in a recording note every embed of the recording's media collapses into one `session` block; any other media file becomes a one-part block naming it relative to the drive. The editor applies the line edits in one transaction. `NoteEmbedPathVm` gains `absolutePath` for the chip's actions.
- **N3 — inserting a widget.** The notes toolbar's *Insert widget* menu and the `/` menu list keeper's note widgets, *Media player* first. Picking a recording or a file in the drive asks Rust for the block (`media_block_compose`): a recording by its identity; a transcript by its path, or by its recording when it is one; a recording's own file by the recording; any other media file as one part.
- **N4 — a block before its transcript** plays the media under "Not transcribed yet.", with *Transcribe* and the job's progress, and shows the lines when `keeper://transcript-written` names its expected path (AD-357).
- **N5 — old stubs, on request.** `recording_notes_adopt_media_block({profileId, dryRun})` rewrites every recording stub whose body still holds exactly the per-file embeds the stub composer wrote into the one block (`media_block::adopt`), leaves every other byte, is idempotent, and names the notes whose embeds were edited by hand and so were left alone. It is the person's action (the notes options menu, with a count and a confirmation) and the coordinator's migration on hesperia; keeper still rewrites nothing by itself (AD-357 holds).

**Shapes, as built.** Commands: `media_block_resolve({profileId, source}) → MediaBlockVm`, `media_block_edit({source, edit: MarkerEditReq}) → string`, `media_block_clip({profileId, source, from, to, words}) → MediaClipVm`, `transcript_clip({path, from, to, words}) → MediaClipVm`, `media_block_sources({body}) → string[]`, `media_block_find_marker({profileId, body, name}) → MediaMarkerHitVm | null`, `media_block_for_embed({profileId, body, line, target}) → LineEditVm[]`, `media_block_compose({profileId, pick: MediaPickReq}) → string`, `recording_notes_adopt_media_block({profileId, dryRun}) → MediaAdoptionVm`. `source` is the fence's body, verbatim. Every refusal is the `IpcError`'s sentence. `MediaRef` is a union, `{via: "file", profileId, relativePath, kind} | {via: "recording", sessionId, relativePath, kind}`, and a part carries `here`. The event is `keeper://transcript-written` with `{path}`, emitted after every transcript JSON the shell writes.

**Front, as built.** The editor layer `src/components/notes/editor/media-block.ts` and its mount `media-block-host.tsx`; the panel `src/components/notes/media-block-panel.tsx`, over the viewer's player and lines; the embed chip `editor/media-chip.ts` and the shared playback vocabulary `editor/media-playback.ts` (`recording-transport.ts` is gone); *Insert widget* in `format-toolbar.tsx` from `src/lib/notes/widgets.ts`, the picker `media-picker-dialog.tsx`, the `/` source `editor/widget-slash.ts`; the old-stub action `adopt-media-blocks.tsx`; marker links in `src/lib/notes/follow-link.ts` and `note-editor.tsx`. The viewer's *Copy as note embed* and *Copy clip from here…* call `transcript_clip`.

**Deviations from the plan, as built.**
- A part's `duration` is `0` when nothing measured it: a recording not transcribed yet takes each segment's length from its capture sample bounds (`ptsStart`/`ptsEnd`) when the manifest has them, and a `[[part]]` without a transcript has none; the player reads it off the media. A `[[part]]` with no `offset` after such a part starts where that part starts.
- `media_block_sources` names the recordings of `session` blocks only; a block naming its recording through `src` is not read for the attachments panel (it would need the file).
- A clip of a `src` block checks the new window against the block's own `from`/`to`, not the file's.
- "Not here" is a Git LFS pointer standing in for the file, or a file that is missing; a virtual file the sync has not materialised reads as whatever is on disk.

## The owner's ask

Verbatim, as the coordinator's scope records it (`local://epic87-fr3.md:11-14`):

> Reszta jest super chcialem jeszcze zeby bylo zaplanowane mozliwosci emedu (plugina itp) do notatek w md (patrz obsidian) zeby moc wyrenderowac ten ekran dla notatek (tych autmatycznych notatek po renderowaniu) - tak zebym mow widziec ten embeded widok i odtwarzac wideo wewnatrz notatki - ten wbudowany plugin moze w konfiguracji miec tylko referencje do wlasciwych plikow (jak wideo/audio liste po kolei, liste kamer/mikrofonow desktop/kamer)
> moze jak bedzie w postaci nazwanego code block: ```NAZWA konfiguracyjny pseudo jezyk z parametrami jak toml lub json, albo link to takiego konfigu w drive```
> Daj mozliwosc zaznaczania znacznikow czasowych (i time-window) - nazwanych tak zeby mozna bylo przewijac do tych momentu w notatce.
> Daj mozliwosc przekopiowania tego oznaczenia pluginu do innej notatki z wysnaczona tylko (np ze znacznika) czasem - lub czescia czasu

"Ten ekran" is the transcript viewer the rest of that message is about: its player, and the transcript's lines under it.

## The verdict, ask by ask

The epic is a plan, so the verdict is what the plan does, not what exists.

| # | The ask (verbatim) | Verdict | How it is met | Mechanism |
| --- | --- | --- | --- | --- |
| 1 | "zaplanowane mozliwosci emedu (plugina itp) do notatek w md (patrz obsidian)" | **planned** | A block type built into keeper's note renderer. It uses the convention Obsidian gives plugins for this job: a fenced code block with a name, claimed by a renderer. | AD-351; 88.2 |
| 2 | "zeby moc wyrenderowac ten ekran dla notatek" | **planned, read-only** | The block draws the viewer's player and the transcript's lines, in read mode, following playback. Corrections stay in the viewer, which the block opens at its current time. | AD-358, UX-DR124; 88.2 |
| 3 | "(tych autmatycznych notatek po renderowaniu)" | **planned** | The note keeper writes when a recording ends carries the block, naming the session by its identity. It plays at once, and shows the lines once the transcript is written, with no rewrite of the note. "po renderowaniu" is read as "after recording" (Q7). | AD-357; 88.5 |
| 4 | "zebym mow widziec ten embeded widok i odtwarzac wideo wewnatrz notatki" | **planned** | The block plays the meeting in the note as one timeline across its segments, with the Picture and Sound choices. | AD-353, AD-359; 88.2 |
| 5 | "w konfiguracji miec tylko referencje do wlasciwych plikow (jak wideo/audio liste po kolei, liste kamer/mikrofonow desktop/kamer)" | **planned** | The body holds references and nothing else. It is either a recording's identity, a transcript's path, or an ordered `[[part]]` list whose entries name the main file, the camera, and which audio track is the call and which the microphone. | AD-352 |
| 6 | "nazwanego code block: ```NAZWA konfiguracyjny pseudo jezyk z parametrami jak toml lub json" | **planned: `keeper-media`, TOML** | TOML is keeper's configuration language. JSON is rejected (AD-352). | AD-351, AD-352; D-30 |
| 7 | "albo link to takiego konfigu w drive" | **planned** | `src = "<path in the drive>.toml"` names a file holding the same grammar. | AD-352, AD-353 |
| 8 | "zaznaczania znacznikow czasowych (i time-window) - nazwanych" | **planned** | `[[marker]]` tables in the block: a `name`, and either `at` or `from` with `to`. They are added, renamed and removed from the block. | AD-354, UX-DR125; 88.3 |
| 9 | "tak zeby mozna bylo przewijac do tych momentu w notatce" | **planned** | A marker's chip seeks the block's player. `[[note#name]]`, or `[[#name]]` inside the same note, opens the note, brings the block into view and moves its player to the moment. | AD-354; 88.3 |
| 10 | "przekopiowania tego oznaczenia pluginu do innej notatki z wysnaczona tylko (np ze znacznika) czasem - lub czescia czasu" | **planned** | *Copy clip* works from a block, a marker or the transcript viewer. It puts on the clipboard a new block for the same meeting, holding the chosen window and the markers inside it. It can also carry the window's lines, for readers without keeper. | AD-355, AD-356, UX-DR126; 88.4 |

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| A renderer for a named fenced block in a note | **present, for one name** | ` ```mermaid ` is found in the parse tree by `FencedCode` and its `CodeInfo`, and replaced by a block widget from a `StateField` (`mermaid-widget.ts:170-208`, `:210-265`). A `ViewPlugin` cannot supply that block decoration: CodeMirror throws (DW-165, `:214-221`). The layer is composed at `live-preview.ts:1852`. |
| keeper's own blocks in a note | **present, as callouts, with a rule against fences** | `> [!board]`, `> [!log]`, `> [!refs]` (`note-widget.ts:12-24`) and `> [!gallery]` (`gallery-block.ts:11-30`) chose Obsidian's callout because "a fence would have made it a wall of grey source everywhere but here" (`docs/notes.md:141-144`). AD-351 answers that rule. |
| A React panel inside a note, without React in the editor's chunk | **present** | `note-widget.ts:216-233`: a dynamic import of the host, a synchronous mount, and a microtask unmount on `destroy` (`:236-250`), for NFR-27 (`:45-48`). |
| A recording played in a note | **present, one embed per file** | `![[…]]` in a recording note resolves by the session's identity (`recording-embed.ts:18-30`) and plays over `keeper-recording://` (`recording_protocol.rs:1-33`). The screen and camera of one session share one transport (`recording-transport.ts:1-37`). |
| A transcript's parts played as one timeline, with its lines | **present, in the transcript viewer only** | `TranscriptPlayer` (`src/components/transcription/transcript-player.tsx`), `locate` and `currentUtterance` (`session-timeline.ts:14-49`), and `transcript_media` (`transcription/media.rs:17-75`). A part plays only from a synced folder (`media.rs:95-122`); anything else says "This part is not in a synced folder, so keeper cannot play it here." (`PART_UNAVAILABLE_SENTENCE`). |
| The recording note stub | **present; embeds each video** | `compose` writes `# Title`, then one `![[<file>]]` per video, then a blank line (`recording_note.rs:301`, `:361-393`). Nothing else writes to a stub, and a re-finalize leaves an existing one alone (`ipc.rs:9117-9135`). |
| Named moments or windows in a media timeline | **absent** | Searching `src`, `src-tauri/crates`, `docs` and `dev` for `keeper-media`, `[[marker]]`, "time marker" and `#t=` found only `#t=30` in two URL parsers' tests. Those tests assert that the fragment is discarded, not read as a path (`file_asset.rs:264-274`, `recording_protocol.rs:276-279`). |
| Following `[[note#anchor]]` to a place in the note | **absent** | The link graph drops the anchor (`notes/links.rs:478-481`, `notes/index.rs:916-919`, `attach.ts:563-567`). A click opens the note it names and nothing more (`note-editor.tsx:481-499`, `follow-link.ts:92-109`). |
| TOML in keeper-core | **present, read only** | `toml` (`keeper-core/Cargo.toml:76`; workspace `toml = "0.9"`, `src-tauri/Cargo.toml:304`). An edit that leaves every other byte alone needs `toml_edit`, which is in the lock at 0.25.12 only through `proc-macro-crate` (`Cargo.lock:8359-8360`). There is no YAML crate in the workspace. |
| A signal that a transcript changed | **absent** | `transcriptionStore.jobs` holds only the jobs this window started (`src/lib/stores/transcription.ts:22`, `:107-119`). There is no `transcript-written` event anywhere. The after-recording job is started by the shell. |
| Honouring `#t=` on an ordinary `![[video.mp4#t=10]]` | **absent** | `embed::candidates` tries the whole target, fragment included, as a path (`notes/embed.rs:48-54`). DW-350. |

## The one sentence

**keeper can play a meeting and read it back in the transcript viewer, and can play a recording's files in its note, but a note cannot show the meeting, cannot remember a moment in it, and cannot hand a piece of it to another note.** The fix is one fenced block, ` ```keeper-media `, with five parts:
- **The block.** Its TOML body names the meeting by reference: a recording's identity, a transcript, a list of parts, or a config file in the drive.
- **Resolution.** Rust reads the body and resolves every name.
- **The panel.** The note draws the viewer's player and the transcript's lines inside the block.
- **Markers.** Named moments and windows live in the block, and are linkable as `[[note#name]]`.
- **Clips.** A clip is a new block for the same meeting, with a window.

## What earlier decisions said, and what this epic amends

| The earlier decision | What it said | What this epic needs | The amendment |
| --- | --- | --- | --- |
| **`docs/notes.md:141-144`**, `note-widget.ts:12-24` and `gallery-block.ts:11-30` | keeper's blocks are callouts, not fences, because a fence is "a wall of grey source everywhere but here" | A block whose content is configuration: an ordered list of parts, files, offsets and timed markers. The owner asked for a named code block. | **Scoped (AD-351, D-30).** The callout rule holds for a block whose content is prose or links a reader can use elsewhere. A block whose content is configuration is a fence. |
| **`recording_note.rs:25-43`, `:361-393`** (Story 42.4, Story 43.5) | the stub's body embeds each video, `![[<file>]]`, below the heading | The meeting in the note, not one player per file | **Superseded for new stubs (AD-357).** The body carries one `keeper-media` block naming the session. `files:` and the heading rule stay. Stubs already written keep their embeds, untouched. |
| **`recording_note.rs:13-17`**, and D-29's last bullet | "No transcription, no summarisation, no inference"; the transcript is its own file | — | **Held.** The block names the session. The stub gains no transcript text. |
| **`docs/recording.md:316-319`** | "The session's note stub stays as it was; the transcript is its own file." | The stub references the meeting | **Reworded (88.5).** The transcript is still its own file, and the stub's block plays it. |
| **Epic 87's *What stays out*** (`epic-87-…:940`) | "A transcript in the session's note stub" is out | — | **Held.** A reference is not a transcript. |
| **AD-65, FR-145** | Rust composes every path, and no synced file carries an absolute one | Paths in a note's block | **Held (AD-353).** Paths are relative to the drive, and every one is joined by `browse::resolve`. |
| **AD-74**, applied in `transcription/media.rs:95-122` | the player plays only a part inside a synced folder | A stub's block must play what its embeds played, which includes a session in a plain recordings folder | **Extended (AD-353).** A part of an indexed session outside every synced folder plays over `keeper-recording://`, by session id. The viewer's player gains the same. |
| **The attachments panel** (Story 43.7; `attachments-panel.tsx:403-443`) | a session file is "in the note" only when the body embeds it; otherwise it offers *Insert* | A stub whose videos play through its block | **Amended (88.5).** A file of a session that a block in the body names counts as in the note. |
| **`recording-transport.ts:1039-1058`**, and `recording-embed.ts:50-56` | a media element is released when its widget goes away; metadata only; no autoplay | Many blocks in one note | **Held and tightened (AD-359).** Media mounts only near the screen, and one block plays at a time. |
| **The link graph** (`notes/links.rs:478-481`) | an anchor is dropped: a link to `note#x` is a link to the note | `[[note#marker]]` | **Held for the graph.** Following the link now also seeks, when a block in that note has a marker of that name (AD-354). |

## Decisions this epic takes

- **AD-351: A meeting in a note is a fenced code block named `keeper-media`, and only Rust reads its body.**

  **Binds:** FR-759, FR-760, FR-766; NFR-111; Stories 88.1, 88.2; D-30.

  **Decision:**
  - **The block.** A CommonMark fenced code block whose info string's first word is `keeper-media`. Backticks or tildes, at any indent CommonMark allows, including inside a list item.
    - **How it is found.** From the parse tree's `FencedCode` and its `CodeInfo`, exactly as the mermaid fence is found, and for the reason written there (`mermaid-widget.ts:170-178`). It is replaced by a block widget supplied from a `StateField`, because a `ViewPlugin` cannot supply one (DW-165, `:210-229`). The layer is composed beside `mermaidLayer()` in `livePreview`'s extension list (`live-preview.ts:1843-1852`).
    - **Revealing the source.** With the caret inside the fence, the source comes back (`mermaid-widget.ts:226-228`).
  - **The body is handed to Rust verbatim.** TypeScript never reads a key, never splits a line and never joins a path. That is the widget rule, "Nothing here composes a query" (`note-widget.ts:33-37`), applied to a meeting.
  - **Why a fence when keeper's other blocks are callouts.**
    - **The callout rule protects what these blocks do not have.** `docs/notes.md:141-144`, `note-widget.ts:12-24` and `gallery-block.ts:21-30` chose callouts because their content is a query or a list of links, and those stay useful in Obsidian: a titled quote with working links. A meeting's content is configuration: an ordered list of parts, each with files, an offset and track roles, and markers with times. The owner asked for exactly that ("nazwanego code block … konfiguracyjny pseudo jezyk"). Put in a callout, the data is still TOML, now behind `> ` on every line, and none of it becomes a working link.
    - **The fence is Obsidian's own extension point for this job.** `registerMarkdownCodeBlockProcessor(language, handler)` "handles fenced code given a language" (`obsidian.d.ts:4993-5001`). Obsidian's first-party ` ```base ` (a YAML body) and Dataview's ` ```dataview ` are named fences. An Obsidian-side renderer (DW-347) can claim a fence. It cannot claim a callout.
  - **The name is prefixed.** Code-block languages are one namespace in a vault: the Timestamp Notes plugin claims the bare `timestamp` and `timestamp-url` (its `main.ts:30`, `:58`). `keeper-media` cannot collide with a plugin that did not choose it.

  **Why not the obvious alternative:**
  - **A callout, `> [!meeting] …`, the house pattern.** It fits prose and links. This content is neither (above).
  - **`![[kelly-sync.meeting.toml]]`, an embed of a config file.** Every note, and every clip, would need a second file. Obsidian would show a link to a file it cannot draw, and copying a clip would create files. It remains available as `src` (AD-352), for a configuration a person wants to share between notes.
  - **An HTML element, `<keeper-media …>`.** Obsidian and GitHub sanitise or hide unknown HTML, so the reader outside keeper would see nothing, which is worse than code.

- **AD-352: The body is TOML, version 1, and its grammar is closed.**

  **Binds:** FR-760, FR-761; Stories 88.1, 88.3; D-30.

  **Decision:** the whole grammar of version 1:

  ````markdown
  ```keeper-media
  # A comment is the person's, and keeper keeps it.
  session = "01J8…-01J8…"          # exactly one source: session | transcript | [[part]] | src
  title = "Pricing, with Kelly"    # optional
  from = "00:12:00"                # optional window on the meeting's own clock, [from, to)
  to = "00:15:30"
  picture = "both"                 # optional: screen | camera | both
  sound = "both"                   # optional: system | microphone | both

  [[marker]]
  name = "The price we agreed"
  at = "00:13:05"

  [[marker]]
  name = "Demo"
  from = "00:14:10"
  to = "00:15:00"
  ```
  ````

  - **Exactly one source:**
    - `session = "<device ULID>-<session ULID>"` names a recording by the identity keeper mints once and never rewrites (`recording_note.rs:47-51`). Its transcript is `transcript.json` in the session's folder (AD-344), when there is one.
    - `transcript = "<path>"` names any transcript file, `transcript.json` or `<name.ext>.transcript.json`. Its media are its `source.parts` (`transcription/model.rs:45-74`).
    - One or more `[[part]]` tables, in play order, name media with no transcript. Each has:
      - `file`, the main file, which carries the picture and the sound;
      - `camera`, optional, a video filmed beside it;
      - `offset`, optional, where the part starts on the block's clock. It defaults to where the previous part ends;
      - `system` and `microphone`, optional, the audio-track numbers that are the call and the microphone. They count from 1, as a person counts tracks.
    - `src = "<path>.toml"` names a file in the drive holding a body of this same grammar, without `src`. The block may add `title`, `from`, `to`, `picture`, `sound` and its own `[[marker]]`s. A marker name must be unique across the file's markers and the block's.
  - **Times.** Either a TOML string in W3C Normal Play Time, `"ss[.f]"`, `"mm:ss[.f]"` or `"hh:mm:ss[.f]"` with `mm` and `ss` two digits (Media Fragments §4.2.1), or a TOML number of seconds. Negative values are refused. keeper writes `"hh:mm:ss"`, in whole seconds (AD-354).
  - **Every time is on the source's own clock:** the session's, or the transcript's, or the parts' timeline. `from` and `to` only narrow what plays.
  - **The window.**
    - It is half-open, `[from, to)`. `from` defaults to 0 and `to` to the end.
    - `from ≥ to` is refused, and a `to` past the end plays to the end. These are Media Fragments' temporal rules (§4.2.1, §6.1.1). The block refuses where a user agent would ignore, because a silently ignored window plays the whole meeting.
  - **A marker** has a `name` and either `at`, or `from` and `to`. Its name rules are in AD-354.
  - **`version`** is optional, and absent means 1. A body above 1 shows its source and "This block was written by a newer keeper."
  - **Anything else is refused, naming the key.** A misspelt `form = …` that were ignored would play the whole meeting.
  - **Refusals.** Each is a sentence from Rust, shown above the block's own source text, never an empty box (UX-DR44, `mermaid-widget.ts:17-21`).

  **Why not the obvious alternative:**
  - **JSON,** the owner's other suggestion. It has no comments, and every key and string is quoted, so it reads worse in Obsidian. keeper's configuration files are already TOML: `.keeper/keeper.toml`, `_models/models.toml` and the account's `drives.toml`.
  - **YAML,** which Obsidian's ` ```base ` uses. keeper has no YAML parser; frontmatter is read by keeper's own subset parser. A second YAML dialect would be a second answer to one question.
  - **Accepting unknown keys for forward compatibility.** `version` already carries that, and a typo that plays the wrong thing is the costlier failure.
  - **Times as bare seconds only.** `"00:13:05"` is legible in Obsidian, and `785` is not.
  - **`screen` and `audio` as separate keys in a `[[part]]`,** the coordinator's first sketch. A part has one main file, and it is either a screen video or an audio file. That is how keeper-rec writes segments, `plan_for_session` reads them (87.1's acceptance) and `SourcePart.file` records them (`model.rs:66-74`). Two keys for one role would admit a part with both, or neither. Rust tells a video from an audio file by its extension (`kind_for_file_name`), as it does everywhere else.

- **AD-353: Rust resolves every name. A recording is named by its identity, and a path is relative to the drive.**

  **Binds:** FR-759, FR-760, FR-765; NFR-111; Stories 88.1, 88.5.

  **Decision:**
  - **What the front sends.** Only the drive, and the body: `{profileId, source}`. The drive is the note's; a vault's id is its profile's (`notes_vault.rs:129-131`). The Files preview of a markdown file in a synced folder passes that folder's profile. With no profile, the block says keeper cannot draw it here (the `WIDGET_NO_HOST` pattern, `note-widget.ts:101-105`).
  - **`session`** resolves through the recordings index, the way `recording_note_targets` does (`ipc.rs:2652-2708`). A Story 40.4 retitle therefore cannot break it, and the block and the `keeper-recording://` handler cannot disagree about where a session is (`:2690-2693`). A recording note's embed answers an unknown session with its plain link (`:2670-2674`). The block answers it with its source and the sentence "keeper does not know this recording on this Mac."
  - **`transcript`, `file`, `camera` and `src`** are relative to the root of the drive that holds the note, the frame FR-109 already gives a note's file links (`notes_vault.rs:136-138`).
    - They are joined only by `browse::resolve`, "the whole of AD-65" (`browse.rs:722-746`), and its refusals are the block's: absolute, escaping, missing.
    - A `src` file is read through the same join, capped at 64 KiB, and parsed with the same grammar.
  - **No absolute path is ever written into a block** (FR-145, `recording_note.rs:53-57`).
  - **Media.**
    - A part inside a synced folder is served over `keeper-file://`, as the viewer's are (`media.rs:95-122`).
    - A part of an indexed session outside every synced folder is served over `keeper-recording://`, by session id and the file's target, which is how a stub's embeds play today (`recording_protocol.rs:20-27`). `MediaRef` gains that alternative, so the transcript viewer's player plays such a session too.
    - A part whose bytes are not on this device (an LFS pointer, a virtual file) is reported as not here, with the probe story 87 uses for `media_here` (A14). It is never handed to a `<video>`.
  - **Not across drives (DW-348).** A path names a file in the note's drive. A `session`, being an identity, plays from any drive's note.

  **Why not the obvious alternative:**
  - **Paths relative to the note.** They break when the note moves. Obsidian resolves from the vault root, not from the note ("Folder paths start at the vault root", Obsidian's *Internal links*).
  - **Paths relative to the vault.** Recordings are outside every vault by construction (`RecordingsConfig::validate`; `ipc.rs:8746-8753`).
  - **The session folder's path.** A retitle renames it (`recording_note.rs:47-51`).
  - **Resolving in TypeScript.** That is the second path join AD-65 forbids.

- **AD-354: A marker lives in the block, on the meeting's clock, and keeper changes only the marker it touches.**

  **Binds:** FR-762, FR-763; NFR-111; Story 88.3.

  **Decision:**
  - **Where a marker lives.** In the note's block, as a `[[marker]]` table.
  - **Names.** Free text, trimmed, 1–80 characters, with none of `[`, `]`, `|`, `#`, `^` or a line break. Those are what a wikilink cannot carry (`recording_note.rs:395-410`). A name is unique within the block under Unicode lower-casing, the transcription code's one folding helper (A8).
  - **Links.** `[[<note>#<name>]]`, and `[[#<name>]]` inside the same note, matched case-insensitively.
    - Following one opens the note, scrolls to the first block holding that marker, and moves its player there, paused (Q3).
    - Rust answers which block and what time. The editor scrolls, and the panel seeks once it has mounted.
    - A name no block in the note carries opens the note, with the sentence "No moment called *X* in this note."
    - The link graph is unchanged: a marker link is a link to the note (`notes/links.rs:478-481`).
  - **Writing.**
    - keeper writes a marker only when the person acts: add, rename or remove.
    - Rust returns the block's new text with that one table changed and every other byte as it was, comments and blank lines included. The tool is `toml_edit`, a direct dependency at the 0.25.12 already locked.
    - The editor splices that text over the block's range, found again at the moment of the action, which is the gallery's rule (`gallery-block.ts:705-722`). The note's ordinary save carries it, with its base revision, and the edit is undoable.
    - Where the text cannot be written, as in the Files preview's read-only pane (`markdown-preview.ts:300-351` builds the editing extensions only for the editable branch), the add, rename and remove controls are absent (AD-27). The chips still work there.
  - **Times stay the meeting's.**
    - A new marker's `at` is the player's time rounded down to a whole second. A window from selected lines is the first line's start rounded down to the last line's end rounded up.
    - A redo deletes and rewrites `transcript.json` (`local://epic87-fr3.md:24`) but keeps the parts' offsets, so a marker still names the same moment.
    - Utterance ids (`u1…`, `model.rs:166-168`) are renumbered by a redo and by a split (B3), and are never a marker's anchor.

  **Why not the obvious alternative:**
  - **In `transcript.json`.** Three reasons:
    - a redo deletes it;
    - every correction rewrites it under `TRANSCRIPT_WRITES`, and two devices marking at once would make a `.sync-conflict-` copy of the whole file (AD-343's reasoning);
    - one note's markers would appear in every note.
  - **A sidecar `markers.toml` beside the transcript.** It is a second writer in the session folder for one note's annotation, and one file per meeting conflicts. A `[[note#marker]]` link would also name a note that does not hold the marker. The gallery settled the same question: "Pins live in the NOTE and there is nowhere else they could go" (`gallery-block.ts:38-44`).
  - **Re-serialising the block** after an edit. It drops the person's comments. `docs/notes.md:52-53` promises that a write leaves every other byte identical.
  - **`^block-id` anchors,** Obsidian's portable form. They take only Latin letters, digits and dashes (Obsidian's *Internal links*), so Polish marker names could not be linked. Inside a fence they would not be block ids anyway.

- **AD-355: A clip is a new block that Rust composes, for the same meeting with a narrower window.**

  **Binds:** FR-764; NFR-111; Story 88.4.

  **Decision:**
  - **Where it is offered.** *Copy clip…* in a block's menu, prefilled from the block's window or the player's time, and in a marker's menu, prefilled from the marker. In the transcript viewer: *Copy as note embed* for the whole transcript, and *Copy clip from here…* in a line's ⋯ menu (R4's menu, story 87.10).
  - **What Rust composes:**
    - the source key or keys verbatim: a `session` stays a `session`, and a `src` stays a `src`;
    - `title`, `picture` and `sound`, carried over;
    - the new `from` and `to`;
    - the markers that lie wholly inside the new window, with their times unchanged.

    Nothing else is carried. A clip of a block that has a window must lie inside that window.
  - **From the viewer**, which knows only a path:
    - `session = …` when the transcript's folder holds a manifest with a session id;
    - otherwise `transcript = …`, relative to the synced folder that holds it;
    - otherwise a refusal: "This transcript is not in a synced folder, so a note cannot name it."
  - **The result goes to the clipboard as Markdown,** with the words when chosen (AD-356). Pasting is ordinary pasting. A `session` clip pasted into a note in another drive still plays.

  **Why not the obvious alternative:**
  - **Cutting a media file for the clip.** It copies hundreds of megabytes to say "minute 12 to 15", and the result stops tracking corrections.
  - **An *Insert into note…* chooser.** It is a second way to do what paste already does. It can be added later without changing the block.
  - **Copying every marker.** A marker outside the window names a moment the clip cannot play.

- **AD-356: Obsidian reads the block as TOML, and a clip may carry its words as a folded callout.**

  **Binds:** FR-766; NFR-111; Stories 88.2, 88.4.

  **Decision:**
  - **Without a plugin, Obsidian shows the fence as a code block,** unhighlighted, because Prism has no `keeper-media` language (Obsidian's *Basic formatting syntax*). The title, the window and the markers' names and times read plainly. A session id does not mean anything to a reader.
  - **The words of a clip,** when copied with them, are a callout on the line directly after the closing fence:

    ````markdown
    ```keeper-media
    session = "01J8…-01J8…"
    from = "00:12:00"
    to = "00:15:30"
    ```
    > [!transcript]- Kelly sync · 00:12:00–00:15:30
    > **[00:12:03] Kelly Chang:** So the price we can live with is…
    > **[00:12:10] You:** For the first year, yes.
    ````

    - **The lines** are the ones overlapping the window. Each is written in the one line format `render::markdown` already writes into `transcript.md` (`docs/transcription.md:241-243`).
    - **Obsidian** shows a collapsed callout titled with the meeting and the window: an unknown callout type takes the default style, and `-` folds it (Obsidian's *Callouts*).
    - **keeper** takes the callout into the block's range. It is hidden with the fence and revealed with it, so a keeper reader never sees the words twice.
    - **A callout after a blank line** is the person's, and keeper leaves it alone.
  - **The words are a snapshot.** keeper writes them once, when the person copies. It never refreshes them: a correction or a redo does not reach them (DW-352).
  - **The stub carries no words.** It has no transcript when it is written, and D-29 keeps transcription text out of it.
  - **The tradeoff.**
    - A reader in Obsidian sees a stub's block as code with an id, and a clip's words as a quote.
    - Nobody in Obsidian sees a player, until an Obsidian renderer exists (DW-347).
    - A reader in keeper sees the live lines, never the stale ones.

  **Why not the obvious alternative:**
  - **Always writing the words, the stub included.** A whole meeting's text would be copied into the note, would go stale on the first correction, and would break the stub's no-transcription rule.
  - **HTML comment delimiters around the words.** They are invisible in Obsidian's reading view but noise in its editor, and no house pattern uses them. A callout's end is already well defined: where the `>` lines end (`note-widget.ts:75-79`).
  - **Keeping `![[screen-0000.mov]]` embeds in the stub for Obsidian.** Those resolve in Obsidian only when its vault root holds the recordings. keeper's layout puts `.obsidian/` in the notes subfolder (`docs/notes.md:26-37`). In keeper they would be a second player for the same moment (Q2).

- **AD-357: The note written when a recording ends carries a `session` block, and learns of its transcript without being rewritten.**

  **Binds:** FR-765; NFR-110, NFR-111; Story 88.5.

  **Decision:**
  - **The stub's body.** `recording_note::compose` writes, below the heading and in place of the per-file video embeds (`recording_note.rs:301`, `:361-393`), a fence of three lines:

    ````markdown
    ```keeper-media
    session = "<session id>"
    ```
    ````

    The id is the same one as `session:` in the frontmatter. `files:`, the heading-first rule (`recording_note.rs:33-38`) and `body_offset` are unchanged. A session with no identity gets no stub today (`ipc.rs:9122-9128`), so no block either.
  - **Before the transcript exists,** the block plays the session's media from its manifest, as the embeds did, under the line "Not transcribed yet."
    - *Transcribe* appears where transcription runs and the session is `transcribable` (AD-27, A14).
    - While a job for it runs, the block shows the job's progress.
  - **When a transcript is written, keeper says so.**
    - The shell emits `keeper://transcript-written`, carrying the transcript's absolute path, after every write or delete it makes under `TRANSCRIPT_WRITES`: a job's result, a correction, a redo's delete.
    - Every mounted block, and every open viewer, whose transcript path matches reads it again. The path is always known: the expected path for a session with no transcript yet.
    - A block that is not mounted resolves afresh when it mounts. No note is rewritten.
  - **Stubs written before this epic keep their embeds, and keep working.** keeper never rewrites a stub (`ipc.rs:9117-9119`).
  - **The attachments panel** counts a session's file as in the note when a block in the body names the note's session. Without that, every video row would offer *Insert* (`attachments-panel.tsx:403-443`): a normal state reported as a fault.

  **Why not the obvious alternative:**
  - **`transcript = "<folder>/transcript.json"` in the stub.** A retitle breaks it.
  - **Writing the block, or the transcript, into the stub when the job ends.** That is a second write into a note the person may be typing in at that minute, and the stub is written once (`ipc.rs:9108-9119`).
  - **A payload-free event,** the `keeper://kebab-case` convention (`notes_window.rs:144-148`, `sessions_ipc.rs:7-10`). Every mounted block would re-read a multi-megabyte JSON on every correction made in the viewer. The path is the one key a block filters on.
  - **Polling.** It costs work on every open note, to find out about a rare event.

- **AD-358: A block reads; the transcript viewer corrects.**

  **Binds:** FR-759, FR-765; Stories 88.2, 88.5.

  **Decision:**
  - **What the block does.** Plays, follows, seeks, and adds, renames or removes markers.
  - **What it does not do.** It does not edit a line, change a speaker, split, add a line, or *Transcribe again*.
  - **Where corrections happen.** *Open transcript* opens the transcript viewer at the block's current time. Every correction there, and a redo, reaches every open block through the event (AD-357).
  - **Reading needs no engine.** The block exists wherever keeper draws notes, whatever `capabilities.transcription` says (the rule F6 set for *Open transcript*). Only *Transcribe* is gated.

  **Why not the obvious alternative:**
  - **Correcting inside the note.** The editor owns the caret and the keys. A widget holding text fields fights it (`note-widget.ts:252-265`), and each block would be a second correction surface over one transcript. The owner's own direction for the viewer is read mode first, with a discreet way to modify (R4).

- **AD-359: A note of many meetings costs one player at a time.**

  **Binds:** NFR-110; Story 88.2.

  **Decision:**
  - **Resolving reads text.** A block's view model carries:
    - the lines inside its window: id, speaker, start, end, text;
    - its speakers: id, name or label, origin;
    - its parts' media references;
    - its markers.

    It never carries word timings, embeddings or candidates.
  - **Media mounts near the screen.** A block creates its `<video>` and `<audio>` elements when an `IntersectionObserver`, with one screen of margin, sees it, and releases them with `releaseMediaElement` when it leaves, unless it is playing.
    - CodeMirror culls only "when that document is big", and then will "only render that plus a margin around it" (CodeMirror's guide, § *Viewport*). A short note with four blocks mounts all four at once, so the editor's culling is not enough.
    - Every video primes its first frame (`primeFirstFrame`, `recording-transport.ts:1005-1037`; R7). That is one seek per video on open, a price the module states (`:984-990`). The observer is what keeps it to the videos near the screen.
  - **`destroy` always releases,** playing or not (`recording-embed.ts:415-418`, `recording-transport.ts:1039-1058`). A playing block that scrolls far enough for the editor to stop drawing it stops (DW-349).
  - **One block plays at a time in a note pane.** Starting one pauses the others, through a scope registry like `transportFor(scope, …)` (`recording-transport.ts:929-944`).
  - **The block's height is fixed.**
    - The lines area is a fixed box, about twelve lines, scrolling inside itself.
    - It is windowed by the arithmetic the gallery and every list use (`gallery-block.ts:52-59`).
    - The widget's `estimatedHeight` is fixed (`note-widget.ts:89-99`, `:189-193`), so a sixty-minute meeting does not stretch the note or shift the height map.
  - **Same text, same block.** The widget's `eq` compares the source (`note-widget.ts:185-187`). A keystroke elsewhere in the note does not resolve it again, and neither does anything but the event.

  **Why not the obvious alternative:**
  - **Mounting every block.** That is one range request per video on open, against files that may sit on a pendrive (`recording-transport.ts:984-990`).
  - **One shared player per note.** It breaks the reason for the block: seeing *this* meeting *here*.
  - **`preload="auto"`.** It downloads whole recordings to show a first frame (`recording-embed.ts:50-56`).

- **UX-DR124: The meeting block.**

  **Binds:** AD-352, AD-353, AD-357, AD-358, AD-359; FR-759, FR-761, FR-765; Stories 88.2, 88.5.

  **Rule** (the design lane owns the details and conformance with DESIGN.md):
  - **A card at the note's content width.**
    - **Header:** the title (the block's `title`, else the session's or the transcript's); the date and the duration, or the window, "12:00–15:30 of 53:15"; a ⋯ button with *Open transcript*, *Copy clip…*, *Mark this moment* and *Mark a window…*.
    - **The player:** the viewer's `TranscriptPlayer`, with story 87.10's icon toggles for Picture and Sound, each shown only when both choices exist. Follow is on. There is no pin: a note scrolls as a whole.
    - **The part line,** "Part 2 of 3 · screen-0001.mov", as in the viewer.
    - **The marker chips** (UX-DR125).
    - **The lines,** in read mode (R4): the speaker's dot and name, the time as quiet text, the words. Clicking a line seeks. The line being said is highlighted.
  - **The player's box** keeps its aspect ratio and never collapses, and every video shows its first frame (R7).
  - **States, each with a sentence and never an empty box:**
    - resolving: the fence's own text, until the panel arrives (`note-widget.ts:195-205`);
    - refused: the source, and Rust's sentence above it (UX-DR44);
    - no drive: "keeper draws meetings only in notes inside a synced folder.";
    - not transcribed yet: the media, and "Not transcribed yet.", with *Transcribe* or the job's progress (AD-357);
    - a part not here or not playable: the viewer's per-part sentence;
    - newer version: "This block was written by a newer keeper."
  - **Keyboard.** Every control is a button in tab order, and the ⋯ menus are keyboard menus. Space plays and pauses when the player has focus. Events inside the panel stay in the panel (`note-widget.ts:252-265`).
  - **`dev/mock-shell.ts`** covers every state, a two-part session with a camera, a window, and three markers.

- **UX-DR125: Markers.**

  **Binds:** AD-354; FR-762, FR-763; Story 88.3.

  **Rule:**
  - **The chips.**
    - A moment is `name · 13:05`. A window is `name · 14:10–15:00`, with a range glyph.
    - Clicking a moment seeks there. Clicking a window seeks to its start, plays, and pauses at its end.
    - Each chip's ⋯ menu offers *Copy link* (`[[<note>#<name>]]`), *Copy clip*, *Rename…* and *Remove*. Remove asks no question, because ⌘Z undoes it: the splice is an editor transaction.
  - **Adding.**
    - *Mark this moment* opens a small popover with *Name* and the player's time. The name is prefilled with the first words of the line being said, cleaned of the characters a name may not carry.
    - *Mark a window…* opens *Name*, *From* and *To*, prefilled from the selected lines or from the line being said.
    - A refused name (a duplicate, a forbidden character, too long) shows Rust's sentence in the popover.
  - **Following a marker link** that no block answers leaves the note open, with "No moment called *X* in this note." in the link-notice place `note-editor.tsx` already uses (`:489-499`).

- **UX-DR126: Copy a clip.**

  **Binds:** AD-355, AD-356; FR-764; Story 88.4.

  **Rule:**
  - **A popover** with *From* and *To* (`hh:mm:ss`, checked by Rust as the person types), a line count ("7 lines"), and the checkbox *Include the words, for Obsidian and other apps*, on by default (Q6).
  - *Copy* puts the Markdown on the clipboard and says "Copied. Paste it into any note."
  - **Entry points:** a block's ⋯ menu, a marker's ⋯ menu, the transcript viewer's header menu (*Copy as note embed*), and a line's ⋯ menu in the viewer (*Copy clip from here…*, prefilled with that line's start and end).

### Alternatives this plan rejected

- **A callout for the block** (AD-351). The content is configuration, not prose or links.
- **An embedded config file as the only form, or an HTML element** (AD-351).
- **JSON or YAML** (AD-352).
- **Ignoring unknown keys** (AD-352).
- **Separate `screen` and `audio` keys in a part** (AD-352).
- **Paths relative to the note or to the vault, or by session folder** (AD-353).
- **Resolving anything in TypeScript** (AD-353).
- **Markers in `transcript.json`, in a sidecar, or anchored to utterance ids** (AD-354).
- **Re-serialising a block on edit** (AD-354).
- **Cutting media files for a clip** (AD-355).
- **Words written into the stub, or refreshed by keeper** (AD-356).
- **A second write into the stub when the transcript arrives, a payload-free event, or polling** (AD-357).
- **Correcting a transcript inside a note** (AD-358).
- **Mounting every block's media, one shared player, or `preload="auto"`** (AD-359).
- **A `speakers` filter in the grammar** (the coordinator's "`speakers` filter?"). It hides one side of a conversation while the player still plays both sides, and the window already narrows a clip.

## Verified facts, with sources

Graded as in epic 87: [SOURCE] is a public document read on 2026-09-29, [REPO] is this repository, [INFERENCE] is reasoned from those, and [UNVERIFIED] was looked for and not established.

- **W3C Media Fragments URI 1.0 (basic),** Recommendation of 2012-09-25 [SOURCE, <https://www.w3.org/TR/2012/REC-media-frags-20120925/>]:
  - §4.2.1: temporal clipping is `t=<begin>,<end>`, and "the interval is half-open". The begin defaults to 0 and the end to the media's duration; `t=,20` is `[0,20)`, and `t=10` is `[10,end)`.
  - Normal Play Time is `npt-sec`, `npt-mmss` or `npt-hhmmss`, and "Minutes and seconds must be specified as exactly two digits, hours … any number of digits". `npt:` is optional.
  - §6.1.1: a `t=a,b` with the end past the media plays to the end.
  - §6.2.1–6.2.2: "only the last valid occurrence of a dimension … is interpreted", and "Invalid temporal fragments SHOULD be ignored". Among the invalid examples §6.2.2 lists `t=10:20`, which sits oddly with the `npt-mmss` production. keeper writes `hh:mm:ss`, which is unambiguous under both readings.
  - §1: "A temporal fragment can also be marked with a name and then addressed through a URI using that name, using the id dimension." That dimension, `#id=`, belongs to the *advanced* specification, not the basic one (§5.1.2's dimension table). No user agent is claimed here to resolve it [UNVERIFIED]. It is prior art for a *named* window, not something keeper can rely on.
- **The HTML standard's media element** [SOURCE, WHATWG, <https://html.spec.whatwg.org/multipage/media.html>, the media data processing steps]: "If either the media resource or the URL of the current media resource indicate a particular start time, then set the initial playback position to that time … For example, with media formats that support media fragment syntax, the fragment can be used to indicate a start position." The standard names only a start position.
- **MDN,** *Audio and video delivery* § *Specifying playback range* [SOURCE, <https://developer.mozilla.org/en-US/docs/Web/Media/Guides/Audio_and_video_delivery>]: "`#t=[starttime][,endtime]` … as a number of seconds … or as an hours/minutes/seconds time separated with colons".
- **keeper's own schemes tolerate a fragment** [REPO]. `keeper-file://01P/clip.mov#t=30` and `keeper-recording://01S/clip.mov#t=30` parse to the file, and the fragment is discarded (`file_asset.rs:264-274`, `recording_protocol.rs:276-279`). keeper's player seeks in code instead, because a window can span two parts, and one URL fragment cannot [INFERENCE].
- **Obsidian's plugin API** [SOURCE, `obsidian.d.ts`, <https://raw.githubusercontent.com/obsidianmd/obsidian-api/master/obsidian.d.ts>]:
  - `registerMarkdownCodeBlockProcessor(language, handler)` (since 0.9.7), a "special post processor that handles fenced code given a language", which replaces the `<pre><code>` with a `<div>` handed to the handler (lines 4993-5001);
  - `MarkdownPostProcessorContext.getSectionInfo(el)`, "may also return null", is how a processor finds its own source range to write back (4017-4023);
  - `addChild(MarkdownRenderChild)`, whose `unload` runs when the element is removed (4009-4016).
- **Obsidian's developer docs,** *Markdown post processing* [SOURCE, <https://docs.obsidian.md/Plugins/Editor/Markdown+post+processing>, source file `en/Plugins/Editor/Markdown post processing.md`]: the mermaid block as the model, and a `csv` code-block processor as the example.
- **Obsidian's own named block** [SOURCE, *Create a base*, <https://help.obsidian.md/bases/create-base>, source `en/Bases/Create a base.md:30-43`]: "Bases can also embedded directly into a note using a `base` code block", with a YAML body.
- **Dataview** [SOURCE, <https://blacksmithgu.github.io/obsidian-dataview/queries/structure/>]: queries are ` ```dataview ` fences holding a query language.
- **Timestamp Notes** [SOURCE, <https://github.com/juliang22/ObsidianTimestampNotes>, `main.ts:30`, `:58`]: registers the bare ` ```timestamp ` and ` ```timestamp-url `, and inserts them as fences.
- **Media Extended** [SOURCE, <https://mx.aidenlx.site/docs/v4/concepts/timestamps>, `/reference/timestamp-format`, `/reference/hash-props`, `/concepts/media-notes`, `/concepts/transcript`; README at <https://github.com/aidenlx/media-extended>]:
  - a timestamp is a link whose target carries `#t=`, `[[lecture.mp4#t=95]]`. `#t=a,b` is a clip that "locks playback to that range", on links and on embeds, `![[lecture.mp4#t=1:10,1:52]]`;
  - its parser follows W3C NPT, requires two-digit `MM`, and adds an `e` end sentinel. Repeated `t=` keeps the last valid one;
  - a media note points at its media from the frontmatter (`media: "[[lecture.mp4]]"`), and `.srt`/`.vtt` files beside a media file are its transcript;
  - v4 is closed source.

  In the pages read, Media Extended configures media through links, hash properties and frontmatter, not a fenced block [INFERENCE: the pages read show none].
- **Obsidian's embeds, links, callouts and formats** [SOURCE, the help vault's source at <https://github.com/obsidianmd/obsidian-help>]:
  - *Embed files*: `![[note#^id]]`, audio embeds, and a PDF's `#page=N`. No `#t=` for media is documented;
  - *Internal links*: `[[note#heading]]`. Block ids use "only … Latin letters, numbers, and dashes", and "Folder paths start at the vault root";
  - *Callouts*: "any unsupported type defaults to the `note` type. The type identifier is case-insensitive", and `-` makes a callout start collapsed;
  - *Accepted file formats*: video `.mkv .mov .mp4 .ogv .webm`;
  - *Basic formatting syntax*: "Obsidian uses Prism for syntax highlighting".
- **CodeMirror 6's viewport** [SOURCE, <https://codemirror.net/docs/guide/> § *Viewport*]: "CodeMirror doesn't render the entire document, when that document is big … it will … only render that plus a margin around it."

### Not established

- whether Obsidian's own video embed honours `#t=` without a plugin [UNVERIFIED]. Nothing here depends on it;
- what Obsidian does with `[[note#name]]` when the note has no such heading. The reading is that it opens the note [UNVERIFIED], and nothing here depends on that either;
- whether two Obsidian plugins can register one code-block language [UNVERIFIED]. The `keeper-` prefix makes it moot;
- where the owner's Obsidian vault root is: the drive or the notes subfolder (Q4);
- how much a note of twenty blocks costs to open in a real WKWebView (owed, 88.2).

## Requirements allocated here

| id | statement | story | AD |
| --- | --- | --- | --- |
| FR-759 | A fenced ` ```keeper-media ` block in a note draws the meeting it names, in the note editor and in the Files preview of a Markdown file inside a synced folder. It shows the meeting's media as one timeline across its parts, with the Picture and Sound choices, and the transcript's lines, read-only, following playback. With the caret inside, the block's text is shown and edited as text. Drawing needs no transcription capability. | 88.1, 88.2 | AD-351, AD-353, AD-358, UX-DR124 |
| FR-760 | A block's body is TOML. It holds exactly one source, `session`, `transcript`, one or more `[[part]]`s, or `src`, plus optional `title`, `from`, `to`, `picture`, `sound`, `[[marker]]`s and `version`. Paths are relative to the drive holding the note. A body with an unknown key, two sources or none, a bad time, an absolute or escaping path, a `src` that names `src`, or a version above 1 shows its own text and a sentence naming what is wrong. | 88.1, 88.2 | AD-352, AD-353 |
| FR-761 | With `from` and `to`, a block plays and lists only the window `[from, to)` of the meeting. Its scrub bar spans the window, and playback pauses at `to`. Times are `hh:mm:ss`, `mm:ss` or seconds, on the meeting's own clock. | 88.1, 88.2 | AD-352 |
| FR-762 | From a block, a person can mark the current moment or a window with a name, and rename or remove a marker. Each marker is a `[[marker]]` table in the block. keeper changes only that table and leaves every other byte of the note as it was. Clicking a marker's chip moves the block's player to it; a window plays and pauses at its end. | 88.1, 88.3 | AD-354, UX-DR125 |
| FR-763 | `[[<note>#<marker name>]]`, and `[[#<marker name>]]` inside the same note, open the note, bring the block holding the marker into view and move its player to the marker, paused. A name no block carries opens the note and says so. The link graph counts such a link as a link to the note. | 88.3 | AD-354, UX-DR125 |
| FR-764 | *Copy clip* works from a block, from a marker, and from the transcript viewer, for the whole transcript or from a line. It puts on the clipboard a new block for the same meeting, with the chosen window and the markers inside it. It can also carry the window's lines as a collapsed `[!transcript]` callout, which keeper hides inside the block. | 88.1, 88.4 | AD-355, AD-356, UX-DR126 |
| FR-765 | The note keeper writes when a recording ends carries a block naming the session by its identity, instead of one embed per video. The block plays the recording at once, and says when there is no transcript yet, offering *Transcribe* where that is possible. It shows the transcript's lines once the transcript is written, without rewriting the note. A correction or a redo shows in every open block. The attachments panel counts the session's files as in the note. | 88.1, 88.5 | AD-353, AD-357 |
| FR-766 | A note carrying a block is valid Markdown. Obsidian, GitHub and a text editor show the block as a code block, and a clip's words as a collapsed callout. keeper never rewrites a block or its words by itself. | 88.2, 88.4 | AD-351, AD-356 |
| NFR-110 | **Many meetings, one player.** In a note of twenty blocks, only the blocks within a screen of the view hold media elements. A block leaving that range releases them unless it is playing, and a block the editor stops drawing always releases them. At most one block plays per note pane. A block's view model carries no word timings, embeddings or candidates. Its height is fixed and its lines are windowed. A keystroke elsewhere in the note resolves no block. | 88.1, 88.2 | AD-359 |
| NFR-111 | **The note stays the person's, and readable without keeper.** A block holds no absolute path. TypeScript never parses a block or joins a path; Rust resolves every name through `browse::resolve` or the recordings index. keeper changes a block's bytes only on the person's action, and each edit changes only the table it touches. The stub's block is written once, with the stub. `.obsidian/` is neither read nor written. | 88.1, 88.3, 88.4, 88.5 | AD-351, AD-353, AD-354, AD-356 |

**Held, not restated:**
- NFR-27: the editor's chunk stays React-free. The panel arrives by dynamic import (`note-widget.ts:45-48`).
- NFR-105: no network host. The block adds no destination: media is served over keeper's custom schemes, and nothing is fetched.
- FR-145, FR-109, FR-121, and AD-65: as cited in the decisions.

## Open questions for the coordinator

Each has the reading this plan builds to, marked as such, so that no lane is blocked. None is resolved silently.

- **Q1. The block's name.**
  - **Gap.** The coordinator proposed `keeper-media`. The block also plays a lecture or any transcribed file.
  - **Plan's reading:** `keeper-media`. It names what the owner thinks of, and the `keeper-` prefix avoids collisions. It becomes a contract with D-30, so it must be settled before any stub carries it.
- **Q2. The stub stops embedding its videos.**
  - **Gap.** An Obsidian vault whose root is the drive would lose the stub's native `![[…mov]]` players.
  - **Plan's reading:** replace them (AD-357, AD-356). The alternative is to keep them and have the block absorb the embeds of its own session, which is a second grammar in one block.
- **Q3. "przewijac do tych momentu": seek, or seek and play?**
  - **Plan's reading:** a link seeks and leaves the player paused. A chip seeks, and keeps playing if the block was playing. A window chip plays to the window's end.
- **Q4. The owner's Obsidian vault root.**
  - **Gap.** If it is the notes subfolder (`docs/notes.md:26-37`), a drive-relative path is a dead link in Obsidian, and the words callout is the only Obsidian-visible content that works.
  - **Plan's reading:** that is the case, and AD-356 is built for it.
- **Q5. A marker belongs to one note.**
  - **Plan's reading:** yes, like a gallery pin. A meeting-wide list, where a moment is marked once and seen in every note, is DW-353.
- **Q6. The words in a copied clip: on or off by default?**
  - **Plan's reading:** on. A clip in another note is usually a quote, and the words are what an Obsidian reader can use. The choice is not remembered, so no settings key is needed.
- **Q7. "tych autmatycznych notatek po renderowaniu".**
  - **Plan's reading:** the automatic notes written after recording: "po nagraniu", not literal rendering.
- **Q8. Track numbers in `[[part]]` count from 1.**
  - **Gap.** The transcript's own `track` index is the engine's.
  - **Plan's reading:** a person counts from 1. Rust converts, and a test pins the conversion.

## Stories

Every story names its rung in the stack (*Stack*, below).
- **The shell is by inspection.** Everything under `src-tauri/crates/keeper/**` awaits CI's macOS job and the Mac gate on hesperia.
- **Generated bindings** (`src/lib/ipc/gen/*.ts`) are regenerated by the ts-export run and never hand-edited.
- **Every new core and front behaviour test is mutation-proved:** mutate, run, restore, and read the diff to confirm the restore.
- **Names are suggestions the lanes agree on.** The command and view-model names below are the plan's. The lanes may rename them, and the behaviour may not change.

### 88.1 — The block's grammar, and what it names

**Intent:** "nazwanego code block … jak toml lub json, albo link to takiego konfigu w drive"; "tylko referencje do wlasciwych plikow". **Rung:** the pure half on **epic88-core**; the commands and the event on **epic88-surface**. AD-352, AD-353, AD-354's edits, AD-355's composition, AD-357's event, AD-359's view model.

**Files:**
- `keeper-core/src/notes/media_block.rs` (new). It holds:
  - the grammar's types and `parse`, and every refusal with its `sentence()`;
  - NPT time parsing and formatting;
  - `edit` (add, rename or remove a marker, through `toml_edit`);
  - `clip` (compose, with or without words);
  - `session_block(session_id)`;
  - `find_marker(sources, name)`;
  - `resolve`, which composes the view model from facts the shell has read: the transcript, the manifest, the session's targets, the synced folders, and whether each part's bytes are here.
- `keeper-core/src/notes/mod.rs`;
- `keeper-core/src/transcription/media.rs` and `vm.rs` (`MediaRef`'s `keeper-recording://` alternative; the slim line and speaker view models);
- `keeper-core/Cargo.toml` and `src-tauri/Cargo.toml` (`toml_edit`, workspace, at the locked 0.25.12);
- `keeper/src/transcribe_ipc.rs`, beside `transcript_media`:
  - the commands `media_block_resolve({profileId, source})`, `media_block_edit({source, edit})`, `media_block_clip({from: block | transcriptPath, window, words})`, `media_block_sources({body})` and `media_block_find_marker({sources, name})`;
  - the emit of `keeper://transcript-written` under `TRANSCRIPT_WRITES`;
- `keeper/src/lib.rs` (registration);
- the new `src/lib/ipc/gen/*.ts` files.

**Acceptance:**
- *Grammar* (mutation-proved):
  - each of the four source forms parses;
  - two sources are refused, and so is none;
  - an unknown key is refused, and the sentence names it: `form` in place of `from`;
  - `version = 2` is refused with the newer-keeper sentence;
  - a `src` file that names `src` is refused;
  - a marker with both `at` and `from` is refused, and so is one with neither;
  - a name holding `[`, `]`, `|`, `#`, `^` or a line break is refused, and so is one of 81 characters;
  - `Umowa` beside `umowa` is a duplicate, and `Ą` beside `ą` is too, by Unicode folding.
- *Times* (mutation-proved):
  - `95`, `95.5`, `"95"`, `"01:35"`, `"1:02:30"` and `"00:01:35.250"` parse;
  - `"1:35"`, `"10:60"`, `"-5"` and `"1:2:3"` are refused;
  - formatting writes `"00:01:35"`;
  - `from = to` is refused, and so is `from > to`;
  - a `to` past the end resolves to the end;
  - `[[part]]` track 1 is the file's first audio track (Q8).
- *Edits* (mutation-proved). Each is checked byte for byte against a body holding a comment line, a blank line and an inline comment:
  - adding a marker appends one `[[marker]]` table and changes no other byte;
  - renaming changes only that `name` value;
  - removing deletes only that table;
  - an edit of a body that does not parse is refused, and nothing is returned to splice.
- *Clips* (mutation-proved):
  - the source key is kept verbatim, for a `session`, a `transcript` and a `src`;
  - the window is the one asked for;
  - a marker wholly inside is kept with its time unchanged, and a marker straddling `from` is dropped;
  - a clip reaching outside the block's own window is refused;
  - with words, the callout's lines are exactly the lines overlapping `[from, to)`, each in `render::markdown`'s line format;
  - from a transcript path: `session` when the folder's manifest has an identity, `transcript` relative to the synced folder otherwise, and the refusal sentence outside every synced folder.
- *Resolution* (pure, over fixtures):
  - a session with `transcript.json` resolves to ready; without one, to not transcribed yet, with the expected transcript path;
  - a part inside a synced folder gives a `keeper-file://` reference. A part outside every synced folder, of an indexed session, gives the `keeper-recording://` one. A part whose bytes are not here gives the not-here state;
  - a path that escapes the drive is refused with `browse::resolve`'s own refusal;
  - a `src` file over 64 KiB is refused;
  - only the lines overlapping the window are carried;
  - the view model has no `words`, `embedding` or `candidates` field at all.
- *The stub's block* is exactly three lines, the fence, `session = "<id>"` and the closing fence, and it parses to that session.
- *The event* (shell, by inspection): emitted with the transcript's path after a job's write, after a correction, and after a redo's delete; not emitted when a write fails.
- *Purity:* `check:core-tauri-free` and `check:core-sync-free` pass. `keeper-core` gains no platform `cfg`. `cargo deny` passes with `toml_edit`, and `Cargo.lock` gains no new crate.
- *Bindings:* `bindings:check` is green on this rung.

**binds:** FR-759, FR-760, FR-761, FR-762, FR-764, FR-765, NFR-110, NFR-111, AD-352, AD-353, AD-354, AD-355, AD-357, AD-359

### 88.2 — A meeting plays inside a note

**Intent:** "zeby moc wyrenderowac ten ekran dla notatek … widziec ten embeded widok i odtwarzac wideo wewnatrz notatki". **Rung:** **epic88-surface** (the front). AD-351, AD-356's absorption, AD-358, AD-359; UX-DR124.

**Files:**
- `src/components/notes/editor/media-block.ts` (new): the `StateField` layer. It finds fences by `CodeInfo` and takes an adjacent `[!transcript]` callout into the range. The widget has a fixed `estimatedHeight`, a dynamic import of the host, `ignoreEvent` inside the body, and a `destroy` that unmounts in a microtask and releases the media;
- `src/components/notes/editor/media-block-host.tsx` (new, the mount);
- `src/components/notes/media-block-panel.tsx` (new): the React panel. It holds the read-only lines list, the chips (88.3) and the menu;
- `src/components/transcription/transcript-player.tsx`:
  - a `window` prop, spanning the scrub bar and pausing at `to`;
  - the one-plays-per-pane scope;
  - the `keeper-recording://` reference, through `recordingAssetUrl`;
  - media mounted on visibility;
- `live-preview.ts` (the layer beside `mermaidLayer()`), `note-editor.tsx` (the profile), and `src/components/viewers/markdown-preview.ts` (the file's profile, for a Markdown file in a synced folder);
- the client wrappers, `dev/mock-shell.ts` and a fixture;
- `docs/notes.md`: a chapter, *A meeting in a note*, after *Widgets in a note*, and that section's "callouts, not fenced blocks" paragraph scoped to widgets (AD-351).

**Acceptance:**
- **Detection.**
  - A `keeper-media` fence is replaced by the block: with backticks, with tildes, indented, or in a list item.
  - A `mermaid`, `toml` or unlabelled fence is untouched.
  - With the caret inside, the source is shown. Clicking inside the panel does not move the caret.
- **The words callout.** An adjacent `> [!transcript]` callout is inside the block's range and is not drawn separately. One after a blank line is drawn as an ordinary callout.
- **Degrading.**
  - A refused body shows its source and Rust's sentence, never an empty box.
  - With no profile, the no-drive sentence shows.
  - With the wrappers failing, the fence's text stays on screen.
- **The window.** The scrub bar spans `[from, to)`. Playback pauses at `to`. No line outside the window is in the list.
- **Media, over a mocked `IntersectionObserver`** (NFR-110):
  - a block off screen has no media element;
  - one that comes near gets its elements, and one that leaves has `releaseMediaElement` called, unless it is playing;
  - `destroy` releases even a playing block;
  - starting a second block pauses the first;
  - `primeFirstFrame` is called for every video.
- **Following.**
  - The line being said is highlighted while playing and on every seek.
  - Clicking a line seeks.
  - A `keeper://transcript-written` event for the block's path re-resolves it, and one for another path does not.
- **Opening the viewer.** *Open transcript* opens the viewer at the block's current time. The block works where `capabilities.transcription` is false.
- **Browser proof.** In a real browser, with the mock shell: a note with a whole meeting and a clip, and the not-transcribed state. A real-WKWebView proof of a four-block note on hesperia is owed.

**binds:** FR-759, FR-760, FR-761, FR-766, NFR-110, AD-351, AD-356, AD-358, AD-359, UX-DR124

### 88.3 — Named moments and windows

**Intent:** "Daj mozliwosc zaznaczania znacznikow czasowych (i time-window) - nazwanych tak zeby mozna bylo przewijac do tych momentu w notatce." **Rung:** **epic88-surface** (the front and the link path); the core's edits and `find_marker` are 88.1's. AD-354; UX-DR125.

**Files:** `media-block-panel.tsx` (the chips and the popovers), `media-block.ts` (the splice at the range found again), `note-editor.tsx` (`openWikilink` carries the fragment and asks where the marker is), `src/lib/notes/follow-link.ts` (`#name` alone means this note), `dev/mock-shell.ts`, and `docs/notes.md`.

**Acceptance:**
- **Adding.**
  - *Mark this moment* at 00:13:05.8 writes `at = "00:13:05"`. The name is prefilled from the line being said and cleaned.
  - *Mark a window…* from two selected lines writes `from` rounded down and `to` rounded up.
  - Both reach the note through an editor transaction that ⌘Z undoes, and the note saves through its normal path.
- **Renaming and removing** change only that table (the byte-for-byte checks are 88.1's). A duplicate or a forbidden name shows Rust's sentence and writes nothing.
- **Chips.** A moment chip seeks. A window chip plays from its start and pauses at its end.
- **Links.**
  - `[[Kelly sync#The price we agreed]]` from another note opens *Kelly sync*, scrolls the block holding that marker into view, and leaves its player at 00:13:05, paused.
  - `[[#the price we agreed]]` in the same note does the same without opening anything.
  - An unknown name leaves the note open, with "No moment called … in this note.".
  - The link still counts as a backlink of the note.
- **A stale range.** A marker added after the text above the block changed lands inside the block, not in the paragraph above.

**binds:** FR-762, FR-763, NFR-111, AD-354, UX-DR125

### 88.4 — A clip of the meeting in another note

**Intent:** "Daj mozliwosc przekopiowania tego oznaczenia pluginu do innej notatki z wysnaczona tylko (np ze znacznika) czasem - lub czescia czasu". **Rung:** **epic88-surface** (the front); the composition is 88.1's. AD-355, AD-356; UX-DR126.

**Files:** `media-block-panel.tsx` (*Copy clip…* and the marker's *Copy clip*), `transcript-viewer.tsx` (*Copy as note embed* in the header menu, and *Copy clip from here…* in a line's ⋯ menu), `dev/mock-shell.ts`, `docs/transcription.md` (§ *A meeting in a note*) and `docs/notes.md`.

**Acceptance:**
- **The clipboard** holds exactly Rust's composition. With *Include the words* on, which is the default, the callout follows the fence. With it off, the fence alone.
- **Prefills.** From a marker, *From* and *To* are the marker's. From a viewer line, they are the line's start and end.
- **Refusals.** An invalid *From* or *To* shows Rust's sentence in the popover, and *Copy* stays unavailable.
- **Pasting.** A clip pasted into another note in the same vault draws the window, with the markers inside it and the words hidden. A `session` clip pasted into a note in another drive plays.
- **Outside keeper.** Opened as plain text, the pasted note reads as a fence and a quote: a snapshot of the Markdown is kept as a test fixture, and Obsidian is checked by hand on the owner's Mac (owed).

**binds:** FR-764, FR-766, NFR-111, AD-355, AD-356, UX-DR126

### 88.5 — The note written when a recording ends carries its meeting

**Intent:** "zeby moc wyrenderowac ten ekran dla notatek (tych autmatycznych notatek po renderowaniu)". **Rung:** **epic88-surface**, core hunk included (*Stack*: a stub whose block nothing draws would be a regression on a rung merged alone). AD-357; UX-DR124's not-transcribed state.

**Files:**
- `keeper-core/src/notes/recording_note.rs`: `compose` writes `session_block(session_id)` in place of `video_embeds`. The module's documentation and its body tests change with the behaviour;
- `src/components/notes/attachments-panel.tsx`, through `media_block_sources`: a session file counts as in the note;
- `media-block-panel.tsx`: not transcribed yet, *Transcribe*, and the job's progress;
- `docs/recording.md` (§ *Transcription*, the stub), `docs/transcription.md`, and this document.

**Acceptance:**
- **The stub.**
  - Its body is `# Title`, a blank line, the three-line block, and a blank line.
  - `files:` is unchanged, and `body_offset` still points at the heading.
  - A session with no video still gets the block, and plays its audio.
  - A retitle (Story 40.4) after the stub is written leaves the block's text unchanged, and the block still plays the session from its new folder.
- **Old stubs.** A stub with the old `![[…]]` embeds draws exactly as before, and keeper writes nothing into it.
- **The attachments panel.** For a stub whose body holds the block, every session file shows as in the note and offers no *Insert*. For an old stub the panel is unchanged.
- **Not transcribed yet.**
  - The block plays the media under "Not transcribed yet.".
  - *Transcribe* is present only where the capability is true and the session is `transcribable`, and absent elsewhere (AD-27).
  - A running job shows its progress.
- **When the transcript lands.** When the after-recording job writes `transcript.json`, the open block shows its lines, and the note's bytes on disk are identical before and after (a byte comparison).
- **A redo.** A correction or a redo in the viewer shows in an open block of the same session.

**binds:** FR-765, NFR-111, AD-357, UX-DR124

## What stays out

- **Correcting a transcript inside a note** (AD-358). DW-354.
- **Searching inside a block.** ⌘F searches the transcript in the viewer, and *Open transcript* is one press away.
- **A `speakers` filter** (*Alternatives this plan rejected*).
- **A new setting.** Nothing in this epic is configured. The words' default is per copy (Q6).
- **Any network destination.** The block reads files and serves them over keeper's own schemes.

Deferred, with the ledger entries allocated here so a later planner finds them. The coordinator applies them to `_bmad-output/implementation-artifacts/deferred-work.md`, after DW-346, and that ledger is then the source of truth. The paste-ready copy is `local://epic88-ledger-blocks.md` (b).

```markdown
### DW-347: No Obsidian plugin draws a keeper-media block; Obsidian shows it as TOML.

origin: epic 88's plan, 2026-09-29 (AD-351, AD-356)
location: `_bmad-output/planning-artifacts/epic-88-a-meeting-you-can-play-inside-a-note.md` (the grammar), `docs/decisions.md` (D-30, the contract a renderer would read)
reason: keeper draws the block; Obsidian, which reads the same vault, shows it as an unhighlighted code block — the title, the window and the markers legible, a session id meaningless — and shows a clip's words as a collapsed callout. The owner asked for the embed inside keeper and pointed at Obsidian as the model, not as a place the meeting must play. Obsidian's API has the exact hook, `registerMarkdownCodeBlockProcessor("keeper-media", …)`, and D-30 is the contract such a plugin would implement; it would still need keeper's resolution — a recordings index for `session`, the drive's root for paths — which only keeper has. Revisit when the owner wants to watch meetings in Obsidian: an Obsidian plugin that resolves `transcript` and `[[part]]` against the vault's adapter and shows `session` blocks as a link into keeper.
status: open

### DW-348: A block names files only in the drive that holds the note.

origin: epic 88's plan, 2026-09-29 (AD-353)
location: `src-tauri/crates/keeper-core/src/notes/media_block.rs` (resolution), `src-tauri/crates/keeper-sync/src/browse.rs` (`resolve`, the one join)
reason: `transcript`, `[[part]]` and `src` paths are relative to the root of the note's drive and joined by `browse::resolve`, so a note in neuradrive cannot name a transcript in tgdrive by path. A `session` block is the exception — it names an identity the recordings index resolves wherever the recording lives — so every automatic note and every clip of a recording works across drives. A path-named transcript (a file's transcript, a hand-written `[[part]]` list) does not. Revisit when the owner pastes such a clip into another drive's note: a `drive = "<name>"` key resolved through the account's `drives.toml`, still joined by `browse::resolve` under that drive's root.
status: open

### DW-349: A playing block stops when the editor stops drawing it; there is no docked player.

origin: epic 88's plan, 2026-09-29 (AD-359)
location: `src/components/notes/editor/media-block.ts` (`destroy`), `src/components/notes/editor/media-playback.ts` (`releaseMediaElement`)
reason: The house rule is that a widget that goes away gives its media back (`recording-embed.ts:50-56`), and CodeMirror destroys a block it stops drawing — in a long note, when the block scrolls out of the part of the note the editor draws (the view plus a margin), or when the text around it is cut. A person listening to a meeting while writing far below it hears it stop. Keeping it would mean a pane-level dock that adopts the playing elements from a widget being destroyed, the handover `recording-transport.ts`'s staging already does between hosts, lifted out of the editor. Revisit when the owner reports playback stopping while writing: a "now playing" strip under the note that holds the one playing block's media until Stop, the note closing, or another block playing.
status: open

### DW-350: Media Extended's timestamp links and embeds (`[[video.mp4#t=95]]`, `![[video.mp4#t=10,20]]`) are not honoured by keeper's ordinary links and embeds.

origin: epic 88's plan, 2026-09-29 (research: W3C Media Fragments §4.2.1; Media Extended's timestamp format)
location: `src-tauri/crates/keeper-core/src/notes/embed.rs` (`candidates` tries the target, fragment included, as a path), `src-tauri/crates/keeper-core/src/notes/links.rs` (`strip_anchor`), `src/components/notes/editor/vault-embed.ts`
reason: Obsidian's Media Extended plugin writes timestamps as a link whose target carries a W3C temporal fragment, and clips as an embed with `#t=a,b`. keeper resolves `![[video.mp4#t=10,20]]` as a file literally named `video.mp4#t=10,20`, finds none, and shows the link; a `[[video.mp4#t=95]]` link is followed as a link to `video.mp4`, and the time is lost with the dropped anchor (`notes/index.rs:916-919`). The meeting block does not need either — it seeks in code, across parts, with its own `from`/`to` — so this epic leaves them. Revisit when the owner opens notes written with Media Extended in keeper: split a `#t=` fragment off media targets in `candidates`, parse it with the block's NPT parser, and start the element there (the schemes already discard the fragment, `file_asset.rs:264-274`).
status: open

### DW-351: No keeper:// link opens a marker from outside keeper.

origin: epic 88's plan, 2026-09-29 (AD-354)
location: `src-tauri/crates/keeper/src/voice_reach.rs` (`install_deep_link`, the one handler), `src-tauri/crates/keeper/tauri.conf.json` (`plugins.deep-link`)
reason: A marker is reached from a note, by `[[note#name]]`. From a browser, a chat or Obsidian there is no link: keeper's deep-link handler routes `keeper://voice/…`, `keeper://setup` and OAuth callbacks only. A `keeper://note/<vault>/<note>#<marker>` route would need a vault-and-note identity that is stable across devices and a decision about what opening a note from outside does to the window the person is in. Revisit when the owner wants to send a moment of a meeting to someone or to another app: add the route to the one handler and compose the link in Rust.
status: open

### DW-352: The words under a copied clip are a snapshot; corrections and a redo do not reach them.

origin: epic 88's plan, 2026-09-29 (AD-356)
location: `src-tauri/crates/keeper-core/src/notes/media_block.rs` (`clip`, the `[!transcript]` callout), `src-tauri/crates/keeper-core/src/transcription/render.rs` (the line format)
reason: A clip copied with its words carries the window's lines as they were at the moment of copying, for readers without keeper. keeper hides them inside the block and draws the live lines instead, so a keeper reader never sees them go stale — but an Obsidian reader of the same note does, after any correction or a *Transcribe again*. keeper does not rewrite them, because a note is written by its person and a refresh keeper decided on is a write they did not make. Revisit if the owner finds stale quotes in Obsidian: a *Refresh the words* action on the block that re-renders the callout from the current transcript, on the person's press.
status: open

### DW-353: A marker belongs to one note; there is no meeting-wide list of markers.

origin: epic 88's plan, 2026-09-29 (AD-354)
location: `src-tauri/crates/keeper-core/src/notes/media_block.rs` (`[[marker]]` tables live in the note's block)
reason: A marker is written into the block of the note it was made in, for the reason a gallery pin is: two notes about one meeting care about different moments, and a marker filed with the meeting would be one note editing every other note's view of it. So a moment marked in the automatic note is not in the weekly summary that also embeds that meeting, unless copied there as a clip. A shared list would have to live beside the transcript, one file per marker so two devices never conflict (AD-343's shape), and survive a redo. Revisit when the owner wants to mark a moment once and see it in every note: `markers/<ulid>.toml` in the session folder, shown beside each block's own markers.
status: open

### DW-354: A meeting block is read-only; corrections and Transcribe again happen in the transcript viewer.

origin: epic 88's plan, 2026-09-29 (AD-358)
location: `src/components/notes/media-block-panel.tsx` (no correction controls), `src/components/transcription/transcript-viewer.tsx` (every correction)
reason: The block plays, follows and marks; editing a line, changing a speaker, splitting, adding a line and *Transcribe again* stay in the viewer, which the block opens at its current time. Inside the note editor a widget with text fields fights the editor for the caret and the keys, and every block would be one more correction surface over the same `transcript.json`. The corrections still reach every open block through `keeper://transcript-written`. Revisit if the owner asks to fix a word without leaving the note: a per-line ⋯ menu in the block that opens the viewer's line editor in a popover, writing through the same commands.
status: open
```

## The failure shape this epic must not repeat

**A note held hostage.** The callout rule exists because "a note that only makes sense inside keeper is a note keeper has taken hostage" (`docs/notes.md:141-144`). This epic chooses a fence for good reasons, and it inherits that duty. A review that finds any of the following is a blocker:
- an absolute path in a block, or in the words under a clip;
- a block that keeper writes, rewrites or refreshes without the person's action;
- an edit that re-serialises the block and drops a comment;
- TypeScript reading a key of the body, or joining a root and a path;
- anything written to `.obsidian/`.

**A reference that breaks on rename.** A recording is renamed by Story 40.4, and its note keeps working only because it names an identity. A review that finds any of the following is a blocker:
- the stub's block naming the session's folder;
- a marker anchored to an utterance id;
- a clip from the viewer naming a session's transcript by path when the session has an identity.

**A note that eats the machine.** Each video costs a seek on open (`recording-transport.ts:984-990`). A review that finds any of the following is a blocker:
- a media element for a block that is off screen;
- two blocks playing in one pane;
- a `destroy` that does not call `releaseMediaElement`;
- a view model carrying word timings or embeddings;
- a block that resolves again on a keystroke elsewhere in the note.

**A normal state reported as a fault.** A stub with a block and no embeds is the new normal. A review that finds any of the following is a blocker:
- the attachments panel offering *Insert* for a session file the block plays;
- a marker link reported as a broken link;
- a note with a block flagged by any check that counted embeds.

## Sprint-status entry

The coordinator applies this under `development_status:`, above the epic-87 block, in `_bmad-output/implementation-artifacts/sprint-status.yaml`. The paste-ready copy is `local://epic88-ledger-blocks.md` (a).

```yaml
  # Epic 88: the owner's message of 2026-09-29 (Polish, verbatim in the epic; its closing paragraph — the rest is story 87.10) — plan an embed ("plugin") for markdown notes, as in Obsidian, that renders the transcript view in a note (the automatic notes after recording) and plays the video inside the note; its configuration holds only references to the files (video/audio parts in order, cameras and microphones), as a named code block with a TOML/JSON-like body or a link to such a config in the drive; named time markers and time windows to scroll to in the note; copy the block to another note with only a marker's or a part of the time. PLAN ONLY — the owner asked for it to be planned.
  # Stack rungs (planned): epic88-plan (the epic file, this entry, DW-347…DW-354, D-30 in docs/decisions.md) on top of epic87-reading; later epic88-core (keeper-core notes/media_block.rs: grammar, NPT times, toml_edit edits, clip, stub_block, find_marker, resolve; transcription/media.rs's keeper-recording MediaRef; bindings) and epic88-surface (transcribe_ipc.rs media_block_* commands and keeper://transcript-written; the media-block.ts layer and the meeting panel; the player's window, visibility mount and one-per-pane; markers and [[note#name]] links; Copy clip in the block and the viewer; recording_note.rs's stub block and the attachments panel, riding here so no rung writes a block nothing draws; docs/notes.md, docs/transcription.md, docs/recording.md).
  # Decisions: AD-351…AD-359, UX-DR124…UX-DR126, FR-759…FR-766, NFR-110…NFR-111. Open questions Q1–Q8 in the epic (the block's name; the stub drops its video embeds; a marker link seeks paused; the owner's Obsidian vault root; markers per note; words on by default in a clip; "po renderowaniu" read as after recording; track numbers count from 1).
  # Owed when built: a four-block note in a real WKWebView on hesperia (first frames, one player, release on scroll); a stub from a real recording drawing its block before and after the transcript lands; a clip pasted into a note in the other drive; the pasted clip opened in the owner's Obsidian.
  epic-88: backlog
  88-1-the-blocks-grammar-and-what-it-names: backlog
  88-2-a-meeting-plays-inside-a-note: backlog
  88-3-named-moments-and-windows: backlog
  88-4-a-clip-of-the-meeting-in-another-note: backlog
  88-5-the-automatic-note-carries-its-meeting: backlog
  # DW-347…DW-354 are opened by this plan.

```

## docs/decisions.md entry

The coordinator applies this to `docs/decisions.md` after D-29. The paste-ready copy is `local://epic88-ledger-blocks.md` (c). The grammar is a durable contract: every automatic note will carry it, clips copy it between notes and drives, and an Obsidian renderer (DW-347) would implement it. The question "why a fence, when keeper's widgets are callouts?" will be asked again, and this entry answers it.

```markdown
## D-30 — A meeting in a note is a `keeper-media` fence, and its grammar is a contract

The owner asked for the transcript view — the player and the transcript's lines — inside a
note, in the notes keeper writes after a recording and in any note, configured by references
only ("tylko referencje do wlasciwych plikow"), as a named code block with a TOML- or
JSON-like body or a link to such a file in the drive; with named moments and windows to jump
to; and with a way to copy the block into another note for part of the time (2026-09-29).
keeper's other blocks in a note are Obsidian callouts, by a rule written down in
`docs/notes.md` § *Widgets in a note*. This entry records why this one is a fence, and fixes
the grammar so notes written today still play in every later keeper.

keeper will **not** write a block the person did not ask for into an existing note, rewrite
one by itself, put an absolute path in one, or let anything but Rust read one.

- **What it is:** a CommonMark fenced code block whose info string's first word is
  `keeper-media`, with a TOML body (version 1). Exactly one source: `session` (a recording's
  identity, the one its note's `session:` carries), `transcript` (a transcript file's path),
  one or more `[[part]]` tables (`file`, optional `camera`, `offset`, `system`, `microphone`),
  or `src` (a `.toml` file in the drive holding the same grammar). Optional `title`, `from`
  and `to` (a half-open window, W3C Media Fragments' temporal rules), `picture`, `sound`,
  `version`, and `[[marker]]` tables (`name`, then `at`, or `from` and `to`). Times are W3C
  Normal Play Time strings or seconds, on the meeting's own clock; keeper writes `"hh:mm:ss"`.
  Paths are relative to the drive that holds the note. Any other key is refused with a
  sentence, and a version above 1 is shown as source. (AD-351…AD-353; FR-759…FR-761; Epic 88)
- **Why a fence here and callouts elsewhere:** a callout keeps a block legible in Obsidian when
  its content is prose or links — a query, a list of pinned files — and that is the rule's
  purpose. A meeting's content is configuration: an ordered list of files, offsets, track roles
  and timed markers. In a callout it would still be TOML, behind `> ` on every line, with
  nothing in it a working link. A fence is also the hook Obsidian gives plugins for exactly
  this (`registerMarkdownCodeBlockProcessor`), the form of its own ` ```base ` block and of
  Dataview. The callout rule stands for widgets; this entry scopes it. (AD-351)
- **What stays true: the note is the person's.** Rust resolves every name — a `session` through
  the recordings index, so a retitle does not break it; a path through `browse::resolve` under
  the drive's root (AD-65). keeper changes a block's bytes only when the person adds, renames or
  removes a marker, and then only that marker's table, every comment kept. Markers live in the
  note, not in the transcript, which a redo deletes. (AD-353, AD-354; NFR-111)
- **What a reader without keeper sees:** Obsidian, GitHub and `cat` show the block as a code
  block. A clip copied with its words carries them as a collapsed `[!transcript]` callout right
  after the fence, a snapshot keeper never refreshes. The note keeper writes after a recording
  carries a three-line `session` block and no transcript text, as D-29 requires; it replaces
  the per-file video embeds for new notes, and notes already written keep theirs. (AD-356,
  AD-357)
- **What it supersedes:** the stub's per-file `![[…]]` video embeds for notes written from Epic
  88 on (`notes/recording_note.rs`), and `docs/recording.md`'s "the session's note stub stays
  as it was". It scopes, and does not revoke, `docs/notes.md`'s "callouts, not fenced blocks".
- **What is deferred, not refused:** an Obsidian renderer for the block (DW-347), paths into
  another drive (DW-348), a player that keeps playing when its block scrolls away (DW-349),
  Media Extended's `#t=` links in ordinary embeds (DW-350), a `keeper://` link to a marker
  (DW-351), refreshing a clip's words (DW-352), meeting-wide markers (DW-353), and corrections
  inside a note (DW-354).
- **Revisit triggers:** a grammar change is a new `version`, never a reinterpretation of version
  1; a new source kind or key is added only with a version bump or as an optional key an older
  keeper refuses by name. None reopens the second paragraph.
- **Status / owner:** planned 2026-09-29 at the owner's request; the owner is the architect and
  decides Q1–Q8 in the epic before a stub carries the block. Epic 88 implements it:
  `keeper_core::notes::media_block`, the shell's `media_block_*` commands and the
  `keeper://transcript-written` event, the note editor's `media-block.ts` and the meeting
  panel.
```

## Stack

Rungs by layer, as in epics 80–87. Only the first is built now.
1. **`epic88-plan`**, on top of rung 7 `epic87-reading`:
   - this document;
   - the ledgers: the sprint-status entry, `deferred-work.md` DW-347…DW-354, and D-30 in `docs/decisions.md`.
2. **`epic88-core`**, later:
   - keeper-core's `notes/media_block.rs` and `notes/mod.rs`;
   - `transcription/media.rs` and `vm.rs` (the `keeper-recording://` reference and the slim view models);
   - `toml_edit` in `keeper-core/Cargo.toml` and the workspace;
   - the regenerated bindings.

   **Hunks from other layers that ride this rung,** because the new `MediaRef` alternative breaks existing code: every match or literal on `MediaRef` in `keeper/src/transcribe_ipc.rs`, and the TypeScript fixtures (`dev/transcription-fixture.ts`) and the player's URL choice, which must typecheck. `bindings:check` must be green on this rung alone.
3. **`epic88-surface`**, later:
   - the shell: `transcribe_ipc.rs`'s `media_block_*` commands, the `keeper://transcript-written` emit, and the registration in `lib.rs`;
   - the front: `media-block.ts`, `media-block-host.tsx`, `media-block-panel.tsx`, the player's window, visibility and one-per-pane changes, the link path in `note-editor.tsx` and `follow-link.ts`, the Files preview's profile, the viewer's *Copy as note embed* and *Copy clip from here…*, the attachments panel, the client wrappers and the mock shell;
   - **`keeper-core/src/notes/recording_note.rs`'s stub change, by a stack-time decision.** It is core code, and it rides here with the renderer. A stub that writes a block nothing draws would be a regression on a rung merged alone (skill: prove each stack rung stands alone);
   - the docs: `docs/notes.md`, `docs/transcription.md` and `docs/recording.md`.
