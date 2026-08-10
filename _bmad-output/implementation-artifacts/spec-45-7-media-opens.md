---
title: 'Story 45.7: Media Opens'
type: 'story'
created: '2026-08-10'
status: 'review'
blocking_condition: ''
baseline_revision: ''
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-45-open-it-change-it-put-it-back.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-44-1-two-videos-properly.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-5-one-attachment-vocabulary.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-6-two-videos-one-transport.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-45-2-one-viewer-registry.md'
---

<intent-contract>

## Intent

**Problem:** the Files pane lists a video, an image and an audio file and hands you their names. The
epic says: open them in a panel, "served over `keeper-recording://` with its range support (43.5),
using the transport 43.6/44.1 already built rather than a second player".

Two of those three instructions turn out to be wrong about the machine, and getting them right is
most of this story.

**The coordinates do not match, and the epic's sentence had to be corrected.**
`keeper-recording://<session_id>/<rel>` is rooted at the *effective recordings destination* and
resolves by listing what a **session** has and picking a member by name. A Files-pane file is a
**sync profile id** plus a profile-relative path — a different address space over a different root.
And AD-74 says in as many words that the Files tab must not reach for that scheme; `sync_open_entry`
already refused the identical shortcut and wrote down why ("pointed at a note in a vault it would
refuse, correctly, and a browser whose Open works for one folder in five is worse than one with no
Open at all"). `keeper-note://` does not fit either: its root is `vault.root`, which is
`local_path/subfolder`, narrower than the tree the Files pane browses.

So the range support is reused — it is `note_protocol`'s, called rather than copied, by all four
handlers now — and the root is new: a **fourth scheme, `keeper-file://<profile_id>/<rel>`**, rooted
at the sync profile's own `local_path` and contained by `keeper_sync::browse::resolve`, the same
function `sync_browse`, `sync_open_entry`, `sync_read_text` and `files_write::resolve_existing`
already call. Not a second containment rule; the first one, with a second caller.

**The transport does not engage, and that is 43.6's rule rather than an omission.**
`RecordingTransport` exists because two tracks of one session must agree about one clock. A panel
shows one file. 43.6 states the ONE case explicitly: a lone video keeps native `controls`, because a
hand-built bar for one track is a worse `<video controls>` — no fullscreen, no picture-in-picture,
no captions menu, no platform keyboard conventions. What a panel inherits from 43.6/44.1 is not the
bar. It is **44.1's frame prime**, which was measured on the lone video too.

**44.1 is why there is a `primeFirstFrame` call in a React component.** `preload="metadata"` settles
a `<video>` at `readyState` 1 (HAVE_METADATA), which the HTML spec defines as having obtained no
video data and representing *transparent black* — measured in a real WKWebView as **zero lit
pixels**, for the lone video with native controls exactly as for the pair. A panel showing one video
is precisely that case. Without the prime this story would have shipped 44.1's defect into a new
surface on the day after its fix landed.

**Approach:** one viewer component branching at the element (43.5's shape); a URL composed from two
Rust-supplied halves; a fourth protocol handler whose every decision lives in a crate that compiles
on Linux; and the unknown viewer reused, with one new optional prop, as the honest answer for a file
the platform will not decode.

## Boundaries & Constraints

**Always:**
- Both halves of every URL arrive from Rust. The frontend escapes them and puts a `/` between them;
  it joins nothing onto a root (AD-65).
- The kind comes from the registry row, never from an extension read at a surface (45.2).
- Every video gets the frame prime; audio never does, because there is no frame to buy.
- Every media element is released on unmount and on retarget: `pause()`, drop `src`, `load()`. The
  third call is the one people leave out and the only one that aborts the selected resource.
- An unknown profile id is refused before any path work happens — no join, no `canonicalize`, no
  `stat`.
- Every protocol refusal collapses to one 404, so a probe cannot tell "outside the folder" from
  "not there" from "no such profile". The distinction survives in the log, without the path.
- A file keeper cannot show says so with its name, its size, its format and its actions — AD-91's
  placeholder, reused, never reimplemented.

**Never:**
- No second player, no second transport, no second byte formatter, no second containment rule.
- No `preload="auto"`. The prime buys one keyframe; `auto` would download a multi-hundred-megabyte
  screen recording to show a thumbnail.
- No absolute path rendered anywhere (FR-145), and no element pointed at one — that would go around
  the containment check AD-65 exists to keep.
- No widening of `keeper-recording://` (AD-74) and no widening of `keeper-note://`'s root (AD-59).
- No new dependency. Nothing was added to `package.json` or `Cargo.lock`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Video in a profile | `kind: video`, `profileId` set | `<video controls preload=metadata>` at `keeper-file://<id>/<rel>` | none |
| Image in a profile | `kind: image` | `<img decoding=async>` with `alt` = the file name | none |
| Audio in a profile | `kind: audio` | `<audio controls preload=metadata>`, no prime | none |
| Frame, ordinary | `loadedmetadata`, `readyState` 1, `currentTime` 0 | `currentTime` becomes 0.001; the platform decodes and presents frame 0 | none |
| Frame, already has one | `readyState` >= 2 | nothing; the range request would be spent for nothing | none |
| Frame, reader moved it | scrubbed to 37.5 before metadata landed | left at 37.5 — a look must not become a move | none |
| Frame, second event | a source change raises `loadedmetadata` again | nothing, even for a reader back at exactly 0 (`{ once: true }`) | none |
| Frame, audio | a `.wav` | not primed | none |
| Intrinsic size, image | `load`, `naturalWidth` 2560 | the facts line reads `2560 × 1440` | none |
| Intrinsic size, video | `loadedmetadata`, `videoWidth` 1440 | the facts line reads `1440 × 900` | none |
| Intrinsic size, unknown | zero width, or audio | no dimensions stated — a `0 × 0` would read as a fact about the file | none |
| Undecodable, code 3 | `MediaError` DECODE | the placeholder, naming the file, "damaged, or its codec is one this machine cannot play", with Size and Open With | reported in place |
| Undecodable, code 4 | SRC_NOT_SUPPORTED | "This machine has no decoder for this format." | reported in place |
| Unreachable volume | `MediaError` NETWORK | "The volume it is on may have gone away." | reported in place |
| Aborted | `MediaError` ABORTED | "Loading … was aborted before it finished." | reported in place |
| Image failure | `<img>` `error`, no `MediaError` | "keeper could not open …, and the platform did not say why." | reported in place |
| Outside every profile | `profileId: null` | the placeholder with its own sentence and Open With; no element, no URL | none |
| Panel closed | unmount | `pause()`, `src` removed, `load()` — the resource is released, listeners removed | none |
| Panel retargeted | `src` changes | old resource released BEFORE the new one loads; the old file's failure sentence is forgotten | none |
| Late failure after close | `error` fires on a torn-down element | nothing; no state set on an unmounted tree | none |
| Unknown profile id | `keeper-file://01NOBODY/clip.mov` | 404, `ServeRefusal::UnknownProfile`, before any path work | 404 |
| `..` in the path | `keeper-file://01P/%2E%2E/secrets.mov` | refused at the parser, and again at `browse::resolve` | 404 |
| Absolute path | `keeper-file://01P//etc/passwd` | refused: an absolute subpath REPLACES a root under a naive join | 404 |
| Symlink out of the root | `escape.mov -> ~/.ssh/id_rsa` | refused after canonicalisation — no string test can see this | 404 |
| Symlink inside the root | `alias.mov -> clip.mov` | SERVED; containment is not a blanket refusal of links | none |
| Directory / fifo | `2026`, a named pipe | refused; a fifo would block forever and a device would lie about its length | 404 |
| Format not served | `notes.md`, `a.docx` | refused before the engine is asked — a name test, no disk | 404 |
| PDF | `reports/2026 Q3.pdf` | served (Story 45.8's opt-in, `application/pdf`) | none |
| Range, ordinary | `bytes=2-5` | 206, four bytes, `Content-Range: bytes 2-5/10` | none |
| Range, past the end | `bytes=10-`, `bytes=-0` | 416 with `Content-Range: bytes */10` and an empty body | 416 |
| Range, malformed | `bytes=abc`, `bytes=1-2,4-5`, `items=0-1` | 200, whole body, bounded by the 8 MiB cap | none |
| Range, absurd | `bytes=0-99999999999999` on an 8 MiB+ file | 206 clamped to exactly 8 MiB, not an unbounded read | none |
| No Range | a 200 MB file | 200/206 capped at 8 MiB; the element re-requests forward | none |

</intent-contract>

## Code Map

**New, and compiled and tested on this machine:**

- `src-tauri/crates/keeper-core/src/file_asset.rs` — the `keeper-file://` grammar. `SCHEME`,
  `parse_file_url`, `is_servable_kind`, `is_servable_path`, `SERVABLE_EXTENSIONS`. 14 tests.
  Homed here rather than beside the handler because `parse_note_url` and `parse_recording_url` live
  in the shell, which does not build on Linux — so the most testable and most dangerous part of a
  protocol handler is currently proved on one platform only. This one is not.
- `src-tauri/crates/keeper-core/src/file-asset-url-vectors.json` — the shared table pinning the Rust
  parser to the TypeScript composer, `file-size-vectors.json`'s pattern exactly.
- `src-tauri/crates/keeper-sync/src/file_serve.rs` — `resolve_served_path`, `ServeRefusal`,
  `served_file_name`, `is_inside`. 12 tests over real temp directories, real symlinks and a real
  fifo. The profile gate runs first, then `browse::resolve`, then the regular-file check.
- `src/lib/viewers/file-asset-url.ts` — `FILE_ASSET_SCHEME`, `fileAssetUrl`. 8 tests.
- `src/components/viewers/media-viewer.tsx` — the viewer. 22 tests.

**New, written and NOT compiled here (the shell crate does not build on Linux — AD-55, AD-56):**

- `src-tauri/crates/keeper/src/file_protocol.rs` — the handler. Four lines of decision, each a call
  into a function tested above: parse, format allow-list, resolve, `mime_for` + `read_response`.
  There is no path arithmetic in the file. Its `mod tests` drives `serve` against a real temp
  directory and covers the whole Range matrix; those nine tests run on the macOS gate.
- One registration in `src-tauri/crates/keeper/src/lib.rs`, and one `mod` line.

**Edited:**

- `src/components/notes/editor/recording-transport.ts` — gained `FRAME_PRIME_SECONDS`,
  `HAVE_CURRENT_DATA`, `primeFirstFrame` and `releaseMediaElement`, moved down from
  `recording-embed.ts`. See "Where the two media facts live now".
- `src/components/notes/editor/recording-embed.ts` — imports both from the transport;
  `releaseRecordingMedia`'s three inline calls became one call to `releaseMediaElement`.
- `src/components/notes/editor/gallery-block.ts` — its `primeFirstFrame` import repointed.
- `src/components/notes/editor/recording-embed.test.ts` — **the import statement only.** Every
  assertion is Story 44.1's, unchanged; 46/46 still green.
- `src/lib/viewers/unknown-viewer.tsx` — one optional `reason` prop, defaulting to
  `unknownViewerSentence(entry)`. Every existing caller is unchanged.
- `src/lib/viewers/components.tsx` — three lines: `video`, `image` and `audio`, all bound to
  `MediaViewer`.
- `src/lib/viewers/index.ts` — exports `fileAssetUrl` and `FILE_ASSET_SCHEME`.

## Tasks & Acceptance

**Execution:**
- [x] Read 44.1 before mounting anything, and applied its prime to the lone-video case a panel is.
- [x] Established that a Files-pane file's coordinates do not reach `keeper-recording://`, said so,
      and solved it in Rust with a fourth root rather than a join in TypeScript.
- [x] Put the grammar, the allow-list and the resolution in crates that compile on Linux; left only
      wiring in the shell.
- [x] Reused the transport module for the two media-element facts; did NOT engage the transport bar
      for one track, per 43.6.
- [x] Reused AD-91's placeholder for the undecodable case, with a named reason per `MediaError` code.
- [x] Released the element on unmount and on retarget.
- [x] 33 mutations applied, run and watched to fail (26 in the sweep; 7 more from auditing this
      code for four shapes peers were bitten by AFTER their own sweeps were clean). Two survived:
      one a real hole in a test, one a genuinely uncovered seam. Both closed and re-probed.
- [x] No new dependency in `package.json` or `Cargo.lock`.

**Acceptance Criteria:**
- `bun run test src/lib/viewers src/components/viewers/media-viewer.test.tsx
  src/components/layout/panel-strip.test.tsx src/components/notes/editor/recording-embed.test.ts
  src/components/notes/editor/recording-transport.test.ts
  src/components/notes/editor/gallery-block.test.ts src/test/no-user-agent-gating.test.ts` —
  **EXIT=0**, 233 tests over three consecutive repeats, including 44.1's 46 and 43.6's 39 unchanged.
- `cargo test -p keeper-core --lib file_asset` — **EXIT=0**, 14 tests.
- `cargo test -p keeper-sync --lib file_serve` — **EXIT=0**, 12 tests.
- `bunx tsc --noEmit` clean over every file this story touches.

## Design Notes

### Why a fourth scheme, in the order the alternatives were eliminated

1. **Widen `keeper-recording://`.** Refused by AD-74 in text, and by its resolution model in fact:
   it does not join a path onto a root at all, it lists a session's composed targets and matches by
   name. There is no session to list for a Files-pane file.
2. **Widen `keeper-note://`.** Its root is `vault.root` = `local_path/subfolder`. A profile's media
   mostly sits outside the notes subfolder. Widening the root to `local_path` would delete the
   containment that makes AD-59 true for every note in the app.
3. **Tauri's built-in asset protocol.** Takes absolute paths and a config-file scope. That is the
   frontend naming a location the engine has not agreed to, which is the whole of AD-65.
4. **Base64 the bytes over IPC.** No Range, so no seeking; and a 200 MB screen recording costs
   200 MB of webview heap. Story 45.8 started here and dropped it for this scheme.
5. **A fourth scheme rooted at the profile.** Chosen. It reuses `note_protocol`'s entire half below
   resolution and `browse::resolve`'s entire containment rule, and adds a root.

### Where the two media facts live now, and why they moved

`primeFirstFrame` and the three-call release arrived in 44.1 inside `recording-embed.ts`, which is a
CodeMirror widget module: it imports `@codemirror/view` and the Tauri IPC client. By this story
there are **three** consumers — the note embed, 44.15's gallery, and a React viewer that has no
business importing an editor widget to learn how to let go of a `<video>`.

They now live in `recording-transport.ts`, which **imports nothing at all**. That makes it the
honest home for two facts about the platform rather than about any surface, and it is the module the
story was told to reuse. The alternative was a fourth module for two functions, or three surfaces
each spelling `pause(); removeAttribute("src"); load();` — and a release that three places implement
is a release two of them will get wrong, which is the leak `releaseRecordingMedia` exists to prevent.

The move is behaviour-preserving and the proof is that 44.1's 46 tests and 43.6's 39 pass with only
their import line changed.

### What the transport contributes, and what it does not

Nothing on the panel is a hand-built control. 43.6's own rule is that the transport engages at two
tracks and disengages at one, because a lone element with the platform's controls is strictly better
than anything this repo would draw for it. A panel holds one target, so the bar never engages —
and asserting that it does not is more useful than asserting that it does.

What 43.6/44.1 genuinely gave this story is the two element-level facts above, and the measurement
that produced the first of them.

### The reason became a prop rather than a second placeholder

AD-91 makes "unknown" a first-class kind whose viewer names the extension, states the size and
offers Reveal and Open With. A media file the platform will not decode wants exactly that
placeholder and a *different sentence*: keeper knows what the format is and has a viewer for it, and
the decoder said no. Rendering the stock sentence would tell the reader their `.mkv` is an unknown
file — a small lie that costs them a bug report. Building a second placeholder beside this one would
give the two of them different facts within a release. One optional prop, defaulting to the existing
sentence, was the smallest change that keeps one placeholder.

The four `MediaError` codes get four sentences because "the volume it is on may have gone away" and
"this machine has no decoder for this format" are different problems and only one of them is worth
unplugging a drive over. Read by number rather than off `MediaError`'s constants, because jsdom does
not define that class and a failure path that only exists in a browser is a failure path nobody
tests.

### `src` is assigned last, in an effect

Assigning `src` is what starts the load, and every listener — the failure handler, the measurement
and the frame prime — must already be registered when it does. React sets props during commit and
runs effects after, so a `src` JSX prop would invert that order. The effect assigns it after
registering, exactly as `recording-embed.ts`'s `elementFor` does, and the cleanup releases before the
next effect loads the new source.

### The URL encoding is stricter than `encodeURIComponent`, on purpose

`encodeURIComponent` leaves `!`, `'`, `(`, `)` and `*` unescaped. They are legal sub-delims in a path
segment, and therefore a question about what a webview normalises before the request reaches the
handler. Escaping them costs nothing and removes the question. What survives is RFC 3986's unreserved
set — alphanumeric plus `-._~` — which is `notes_vault::asset_url`'s set exactly, so keeper has one
spelling for an asset URL rather than one per scheme.

A `.` or `..` segment is still **composed**, encoded whole as `%2E`/`%2E%2E` rather than dropped, so
a traversal attempt reaches the log as visible text instead of as a path that already collapsed
before anyone could see it. The parser refuses every one of them, and the shared vector table
asserts both ends of that decision.

### What Main asked to be proved, and where each proof runs

Main required three things as tests rather than comments, once this became a general Range-capable
read reachable from anywhere in the webview — including from text that arrived in a note over sync.

1. **An unregistered profile id is refused before any path work.** `file_serve.rs`, executed on
   Linux. The discriminating test passes an id that does not exist together with a subpath that
   would itself be refused, and asserts the answer is `UnknownProfile` and not `Escapes`: if the
   path were consulted first, a handler could be induced to canonicalize an attacker's string before
   deciding it had any business doing so.
2. **An escaping path is refused, and the second caller genuinely goes THROUGH `browse::resolve`.**
   `file_serve.rs`, executed on Linux, over real temp directories: `..` in four shapes, `.`, three
   absolute-path forms, a symlink pointing out of the root, and — the other half, without which the
   rule would be a blanket refusal rather than containment — a symlink that stays inside and IS
   served. The mutation that replaces `browse::resolve` with `profile.local_path.join(subpath)`
   fails four of them, which is the "second caller genuinely goes through it" claim as a measurement
   rather than an assertion. macOS case-insensitivity is the one form NOT exercised here; see below.
3. **A malformed or absurd Range is bounded and honest.** `file_protocol.rs`'s own tests, over a real
   8 MiB+ file: `bytes=10-`/`bytes=-0`/past-the-end → 416 with `bytes */total` and an empty body;
   `bytes=abc`/multi-range/`items=` → 200 capped; `bytes=0-99999999999999` → 206 clamped to exactly
   8 MiB. **These are written and run on the macOS gate, not here**, because a 416 is a
   `tauri::http::Response` and `tauri` does not build on Linux. Correcting Main's framing on one
   point, because the difference matters to a `<video>`: a *malformed* Range is not a 416 — it is
   treated as no Range at all and answered 200, bounded by the cap, which is `note_protocol`'s stated
   I/O matrix and `keeper-media://`'s before it. A 416 is for a *well-formed but unsatisfiable*
   range. An empty 200 would tell the element the file is empty and it would stop.

### What the reverts proved

26 mutations, each applied to the shipped source, run, and watched to fail. Baseline green before
and after at exactly the verdict's scope, judged by **exit code** rather than the summary line.

| Mutation | Caught by |
|---|---|
| The parser accepts a `..` segment | 3, incl. `refuses_a_dot_segment_in_either_spelling` |
| The parser accepts a NUL in the id | `refuses_a_nul_in_either_half` |
| The parser collapses an empty segment | `refuses_an_empty_id_an_empty_path_and_an_empty_segment` |
| Segments are not percent-decoded | 4, incl. `every_shared_vector_takes_apart_into_the_two_halves…` |
| The PDF opt-in is dropped | `serves_a_pdf_and_no_other_document` |
| The kind gate lets a plain `File` through | 4, incl. `refuses_everything_that_is_not_opted_in` |
| The FIRST path segment decides the format | 3, incl. `the_last_extension_decides…` |
| Any profile answers for any id | 3, incl. `an_unregistered_profile_id_is_refused_before_any_path_work` |
| A non-regular file is served | `a_directory_is_not_a_file_to_serve`, the fifo test |
| The resolver joins instead of calling `browse::resolve` | 4, incl. both symlink tests |
| Absent media is reported as the wrong fact | `a_path_that_is_not_on_disk_is_missing_rather_than_an_escape` |
| The composer leaves sub-delims unescaped | 2, incl. the shared vector table |
| The composer drops a dot segment's encoding | 2, incl. the shared vector table |
| The profile id is not escaped | `escapes the profile id too` |
| The video is never primed for a frame | `moves an untouched element off zero once the metadata lands` |
| The element is never released | 3 release tests |
| A failed decode still shows the player | 4 placeholder tests |
| A file outside every profile gets the stock sentence | `shows the placeholder rather than a player…` |
| A retarget keeps the last file's failure | `forgets the previous file's failure when the panel is retargeted` |
| Every failure gets one sentence | `names each of the platform's four reasons, and never as the unknown one` |
| The media ids are not bound | 6, incl. `resolves a video, an image and an audio file…` |
| The placeholder ignores the caller's reason | 4 placeholder tests |
| The release forgets to abort the load | 4, incl. 44.1's `releases the media element on teardown…` |
| The release forgets to drop the source | 4, incl. 44.1's |
| The prime fires on every `loadedmetadata` | 2, incl. 44.1's `buys one frame and not one per event` |
| The prime yanks an element the reader moved | 2, incl. 44.1's `leaves an element the reader… already moved` |

**One survived the first pass, and it was a real hole.** Replacing `MediaError` code 1's sentence
with the "the platform did not say why" default left four *distinct* strings, so a test asserting
only distinctness passed while the reader was being told keeper had no idea when it had been told
exactly. The test now also asserts that a code the platform DID give is never reported as the
unknown one. Re-mutated: caught.

**I hit the deletion trap the brief warns about, on the first mutation.** The inverse
`replace("", anchor)` counted 16 315 occurrences of the empty string, replaced at the wrong place,
and left the dot-segment refusal DELETED from `file_asset.rs` — a live security mutant in a shared
crate for about two minutes, found because the run raised loudly rather than silently. Restored by
hand and re-verified. The harness now refuses an empty replacement outright and uses a unique
sentinel for deletions.

**Both directions verified at the end**, taking W2Documents' finding: every anchor asserted PRESENT
exactly once with plain Python `.count()` — no matcher semantics in the way — as well as every mutant
and sentinel asserted absent. The presence direction is the robust one; a missing anchor cannot hide
behind a bad pattern the way a present mutant can.

**One fixture in this story is sized to reach the path it tests, and the discipline deserves its
name.** Story 45.8 put it best, having been bitten by it: *if your fixture is supposed to be opaque,
assert that it is opaque before you assert what reading it produces* — their "compressed" PDF object
stream was 37 bytes, below the size at which `flate2` bothers to deflate, so it was emitted as a
STORED block, stayed legible as plain ASCII, and the inflation path they were testing was never
entered. Generalised: a fixture meant to exercise a path must be PROVED to reach it, not assumed to.
The instance here is the Range cap. `big.mov` is `8 MiB + 4096` bytes precisely because a file below
the cap would satisfy the clamp assertion trivially without any clamping happening, and the
assertion pins `Content-Range: bytes 0-8388607/8392704` so the total is visibly larger than the
slice. Same discipline, different threshold — and the one to watch for in this repo is any test
whose fixture is small enough that a size-triggered behaviour quietly does nothing.

**Every absence-shaped assertion in this story now has a mutation behind it, and one did not until
it was audited.** Story 45.13's generalisation of the above: a fixture whose expected value is an
ABSENCE is the dangerous case, because the absence is also exactly what a broken fixture produces.
This story has fourteen such assertions — the two `refused` vectors, and the parser tests for a dot
segment, a NUL, an empty id / path / segment, non-UTF-8 bytes and a foreign scheme, plus
`file_serve`'s four `Err` variants. Thirteen already had a mutation that turns the `None` or the
`Err` into a `Some` or an `Ok` — which is the only thing that proves the vector is green for the
reason claimed. `refuses_another_scheme_entirely` did not, so it was probed: replacing the
three-spelling prefix strip with a generic `raw.split_once("://")` makes `keeper-recording://…` and
`keeper-note://…` parse happily, and it fails that test plus
`accepts_the_windows_and_android_spelling_tauri_rewrites_to`. Caught. Total for the story: **27
mutations, 27 caught**, of which 26 were the sweep and one this audit.

### A real defect found after the sweep, by a peer's generalisation

Story 45.13's finding — *a contract stated in a view model's doc comment and enforced nowhere is a
silent-failure path at every call site that trusts it; where the consumer can be shaped so the bad
case is unrepresentable, do that instead of asserting the good one* — was checked against this
story's code rather than acknowledged, and it found something.

`MediaViewer` chose its element with `entry.viewer === "image" ? <img> : <player video={entry.viewer
=== "video"}>`. **`audio` was the else-of-an-else.** The contract that only three ids ever reach this
component lives in `VIEWER_COMPONENTS`, in a different file, and was enforced nowhere here — so
binding a fourth id to this component would have drawn a permanently empty `<audio>` bar and said
nothing. A silent empty control for a file keeper can see perfectly well is the exact defect this
epic exists against, one level up from where the epic looked for it.

The fix partitions on the field that decides rather than falling through to it: the three ids are
named, and anything else renders AD-91's placeholder with a sentence that blames keeper's wiring
rather than the file, because the file is fine. Below the guard `entry.viewer` is exactly the three.

Proved by reverting: replacing the guard's condition with `false` fails
`says so when a non-media row is routed here, rather than drawing a silent audio bar`, **and only
that test** — so the guard is load-bearing today rather than defensive. Its input is built by hand,
following `text-file-viewer.test.tsx`'s precedent for the same situation and for the same reason:
the registry cannot produce a non-media row for this component, and a guard that can only run on an
input the registry cannot produce is exactly the guard nothing else will ever exercise.

Running total at this point: 28 mutations, 28 caught — 26 in the sweep, one from the
absence-assertion audit, one from this. One more pair follows.

### A second one, from the same audit habit: a type that lied

Story 45.13's next finding was an unreachable ternary arm justified by an invented reason. Auditing
for that shape here turned up its inverse, which is worse because the compiler is on the wrong side
of it.

`PlayerElement` mounts BOTH a `<video>` and an `<audio>`, and its ref was typed
`useRef<HTMLVideoElement>`. Two live consequences the type hid:

- `node.videoWidth ?? 0` read as pointless defence against a non-nullable `number`, while being the
  only thing stopping `undefined` reaching the facts line for an audio element. **A guard the
  compiler believes is redundant is a guard the next reader deletes.**
- `node instanceof HTMLVideoElement` read as always-true — TS narrowing `HTMLVideoElement` to
  `HTMLVideoElement` — while being genuinely false half the time. The prime's correctness rested on
  a check the compiler thought was noise.

The ref is now `HTMLMediaElement`, attached through a callback ref because `<video>` wants
`Ref<HTMLVideoElement>` and `<audio>` wants `Ref<HTMLAudioElement>` and one `RefObject` of the
supertype satisfies neither, while a function taking the supertype satisfies both — the honest type
reaching both elements with no cast. `videoWidth` is now read only inside a narrow the compiler
agrees is meaningful, and the audio branch reports zeroes explicitly instead of relying on
`undefined ?? 0`.

Both branches probed: reporting a size audio does not have fails `says nothing about dimensions for
audio, which has none`; dropping the video branch's report fails `exposes a video's intrinsic size
from the same metadata that primes it`. Each fails alone.

Worth stating because it changes what "covered" means here: **the `instanceof` narrow is now
enforced by the compiler rather than by a test, and that is stronger.** Removing it no longer
type-checks, where before it was a runtime-only guard that no test in this suite could distinguish —
`undefined > 0` is false, so deleting it would have produced identical output and survived every
mutation.

Running total at this point: 30 mutations, 30 caught. One more follows.

### The seam nothing pressed, and the mutation that survived it

Story 45.13's third handback was the sharpest: *their whole suite stopped at the offer and nothing
ever pressed the button, so a total feature break was invisible.* Probed for it here rather than
acknowledged, and the analogue was real.

**Every media test in this story builds its own `ViewerFile` and asks the registry directly.** None
of them exercises `FilePanelBody`, which is the function that turns a `sync_browse` listing into
that `ViewerFile` — so all 23 would still pass if it put `entry.name` where `relativePath` belongs.
Measured, not supposed: **that mutation survived**, silently, across the whole suite.

The consequence is not cosmetic. A file in a subfolder would be addressed as though it sat at the
profile root, `browse::resolve` would find nothing there, and every video, image and audio inside a
folder — which is where recordings actually live, `2026/08/…` — would 404. Files at the root would
work, which is exactly the pattern that makes a bug look like a mystery.

Closed with one test in `panel-strip.test.tsx`, asserting over the composed URL, which is the only
place the two spellings differ. Re-probed: `entry.name` now fails that test and only that test, and
`profileId: null` fails it plus the strip's own routing test.

The corollary Story 45.13 drew from it is the one to carry: **a mutation sweep certifies the lines
it was pointed at.** Mine was 26 lines I had already reasoned about. Six defects and gaps in this
story were found after it was clean — one absence assertion with no mutation behind it, one
silent-audio-bar fallthrough, two branches of a type that lied, and this seam — and every one came
from auditing for a SHAPE a peer had just been bitten by, not from extending the sweep.

Story total: **33 mutations, 33 caught** — 26 sweep, 7 audit.

## What is proved on which machine, and what is not proved anywhere

**Proved on this machine (Linux), executed:**

- The URL grammar in both directions, including the Windows/Android `http://<scheme>.localhost/`
  spelling, cache-busters, fragments, non-UTF-8 bytes, NULs, empty segments and dot segments
  (14 Rust tests).
- That the Rust parser and the TypeScript composer agree, over twelve names including spaces, `#`,
  `?`, a literal `%`, sub-delims, CJK and a trailing space — the shared vector table, loaded by both
  suites (14 Rust + 8 TS).
- The format allow-list, driven through `kind_for_file_name`'s own extension tables rather than a
  list retyped beside it.
- The whole resolution and containment story over real temp directories, real symlinks in both
  directions and a real fifo (12 Rust tests).
- Every branch of the viewer: the three elements, the prime's five cases, the intrinsic-size
  reporting, all five failure sentences, the no-profile case, the retarget reset and the release
  (22 TS tests).
- That the move of `primeFirstFrame` and `releaseMediaElement` changed no behaviour: 44.1's 46 and
  43.6's 39 tests pass with only an import line edited.

**Written here, and only executed on the macOS gate** (the `keeper` shell crate does not build on
Linux):

- `file_protocol.rs` in its entirety — including all nine of its own tests, which are the Range
  matrix Main asked for, the "every refusal is the same 404" assertion, and the cross-crate check
  that every format this scheme serves has a `Content-Type`.
- The scheme's registration in `lib.rs`.

**Not settled anywhere, and it should be said rather than glossed:**

- **No pixel has been counted for this surface.** jsdom decodes nothing: it never fires
  `loadedmetadata`, never raises `readyState` above 0 and never gives an `<img>` a `naturalWidth`.
  Every assertion here drives the element the way the platform would and asserts what this code does
  with it. 44.1 counted 988 lit pixels out of 1024 in a real WKWebView for a lone `<video>` after
  exactly this prime, on the owner's own machine; that measurement is what this story is standing
  on, and it has not been repeated for a panel. **The first thing to check on macOS is that a video
  opened from the Files pane shows a frame before you press play.**
- **A macOS case variation is not exercised.** Main named it. APFS is case-insensitive by default, so
  `CLIP.MOV` and `clip.mov` are one file there and two on the Linux box these tests run on — a test
  written here would assert the wrong thing. `browse::resolve` canonicalizes both sides, which is
  the mechanism that handles it, and `canonicalize` on APFS returns the on-disk spelling. Not
  measured. It is a case-folding question about the containment rule rather than about this story's
  second caller, and the honest place for it is a `#[cfg(target_os = "macos")]` test beside
  `browse::resolve`.
- **The `Content-Type` a real WKWebView accepts for each extension.** `mime_for`'s table is shared
  with two shipped schemes and is not new here, but `avif`, `flac` and `oga` have never been served
  to a real webview by any of them as far as this story could establish.
- **What the prime costs on a slow volume**, unchanged from 44.1: still reasoning, not measurement.
  A panel pays it once per open rather than once per note, which is cheaper than the note case, and
  is the same drive touch.
- **Whether `keeper-file://` needs a CSP entry.** `tauri.conf.json` sets `"csp": null`, so there is
  no allow-list to add to and nothing to get wrong. If a CSP is ever introduced, this scheme must be
  added to `media-src` and `img-src` alongside the other three.
- **Concurrency of the handler under many simultaneous requests.** Each request gets its own
  `spawn_blocking` and shares no state, as `recording_protocol` does; not load-tested.

## Deliberately NOT done

- **The transport bar is not offered for a single file.** 43.6's rule, restated above. A panel that
  could hold two videos of one session is a different story and would want `transportFor`.
- **No poster, no thumbnail cache.** 44.1's reasoning unchanged: a poster means generating and
  storing an image per file, with an invalidation rule and a place to put it. The prime buys the
  real frame instead.
- **No fullscreen, picture-in-picture, playback rate or captions control.** The platform's own
  `controls` already offer all four; drawing our own would remove them.
- **No `loading="lazy"` on the panel's image**, unlike a note embed's. A panel's media is the thing
  the reader just clicked; deferring it would be a deliberate delay in front of the only content on
  screen.
- **DOCX, PPTX and XLSX are not served over this scheme.** Story 45.8 parses those in Rust and ships
  a bounded view model, so their bytes never cross into the webview. Only PDF is opted in, because
  it is the one document format whose pixels the webview draws for itself.
- **No write path for media.** `entry.writable` is `false` for all three rows and the epic's "what is
  NOT in this epic" says why: a lossy round trip through a media container is how people lose work.
- **`recording-embed.ts` was not made to use `keeper-file://`.** A recording note's embed resolves by
  session id through the index, which survives a Story 40.4 retitle; re-addressing it by profile
  would break that and is not what this story was asked for.
- **`media_protocol.rs`'s and `note_protocol.rs`'s duplicate `parse_range` implementations were not
  unified.** There are two copies with subtly different `bytes=-N` clamping. Merging them into
  `keeper-core` would make the Range grammar provable on Linux for all four handlers and is the
  right next step — but it is a refactor of two shipped modules used by four schemes, with tests
  this machine cannot run, and it belongs to a story that can compile them. Recorded here rather
  than done quietly.
- **The epic file is not edited.** The correction to "served over `keeper-recording://`" is recorded
  in this spec; the epic author owns the epic text.
