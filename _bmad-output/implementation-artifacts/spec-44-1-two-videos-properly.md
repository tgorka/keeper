---
title: 'Story 44.1: Two Videos, Properly'
type: 'bug'
created: '2026-08-09'
status: 'review'
blocking_condition: ''
baseline_revision: ''
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-44-the-vocabulary-is-the-space.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-6-two-videos-one-transport.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-5-one-attachment-vocabulary.md'
---

<intent-contract>

## Intent

**Problem:** the field report says three things about a note embedding two videos. The transport
wraps its labels mid-word, each track's mute slider sits away from the track it governs, and — the
part that is not cosmetic — both videos render as empty grey boxes. The epic asked for the cause of
the third to be established before anything was styled, because it might not be CSS at all.

It is not CSS, and it is not the two-video path either. **The single video was equally frameless and
WebKit's native controls chrome was hiding it.**

`preload="metadata"` settles a media element at `readyState` 1 (HAVE_METADATA). The HTML spec says a
video element at HAVE_METADATA that has obtained no video data represents *transparent black*, and
WebKit obeys that exactly — it decodes no frame and paints nothing. What the reader sees through the
element is `.cm-lp-recording-player`'s own `background-color: var(--muted)`. That is the grey box.
Story 42.6's lone video has the same hole in it; it reads as a player only because WebKit draws its
control chrome on top. Story 43.6's transport takes `controls` away at two tracks, and the emptiness
becomes visible.

So the epic's framing needs correcting, and the correction is the most valuable thing in this story:
this is a one-video defect that two videos exposed, not a two-video regression.

**Approach:** three separate fixes, because they are three separate faults.

*The frame.* Every video this module creates asks for a frame once, on `loadedmetadata`, by
assigning `currentTime`. That is the cheapest thing that makes the platform decode and present frame
zero. It is applied to the lone video too — see "The AC that was overturned".

*The wrap.* Every control on the bar becomes a glyph carrying its phrase in `aria-label`, the
readout loses the spaces around its separator, and the status line moves off the control row onto
its own. The invariant is stated as a property rather than as a style: **the transport's control row
contains no whitespace at all**, so it offers the browser no break opportunity anywhere.

*The mixers.* The transport gathers every track into one stage under the leading embed — a row of
track boxes with the single bar beneath — and each track's mute and volume live inside its own box.
Two `<video>` elements in two different `.cm-line` blocks cannot be put side by side by any rule
this module is allowed to write; CodeMirror owns those lines and they are block-level. Re-parenting
is the only honest way to make the pair read as one player.

## Boundaries & Constraints

**Always:**
- The frame prime happens once, on `loadedmetadata`, and only for an element still at the position
  nobody has moved. A reader who scrubbed, or a transport that seated a late-joining track, has said
  where the element should be.
- An element that already has a frame is asked for nothing.
- Every control's accessible name is unchanged from what Story 43.6 shipped. The glyphs cost a
  screen-reader user nothing.
- A grouped track is handed back to its own host before it is dropped, on every path — teardown, a
  failed load, and falling back to one track.
- A lone video is untouched: native `controls`, no stage, no box, no mixer, in its own host.
- Everything Story 43.6 established still holds. Every state is a fold over all tracks; volume and
  mute stay per track; the transport engages at two and disengages at one.

**Never:**
- No new dependency, no second embed module, no second embed syntax. The markdown a note holds is
  unchanged, and Obsidian still renders `![[a.mov]]` and `![[b.mov]]` as two of its own players.
- No `preload="auto"`. The prime buys one keyframe; `auto` would download a multi-hundred-megabyte
  screen recording to show a thumbnail.
- No absolute path anywhere (FR-145); nothing here touches a path.
- No emoji. The glyphs are U+25BA, U+275A, U+21BA, U+21BB and U+266A, none of which has an emoji
  presentation, so they render as text in the editor's own font.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Frame, ordinary | `loadedmetadata`, `currentTime` 0, `readyState` 1 | `currentTime` becomes 0.001; the platform decodes and presents frame 0 | none |
| Frame, too early | `readyState` 0, no metadata yet | nothing; a seek at HAVE_NOTHING is recorded as a default start position and raises no event | none |
| Frame, already moved | reader scrubbed to 37.5 before metadata landed | left at 37.5 — a look must not become a move | none |
| Frame, already has one | `readyState` ≥ 2 | nothing; the range request would be spent for nothing | none |
| Frame, second event | a source change or reload raises `loadedmetadata` again | nothing, even for a reader who scrubbed back to exactly 0 | none |
| Audio | a `.wav` sidecar | not primed; there is no frame to buy | none |
| One video | a note embedding one `.mov` | native `controls`, no stage, no box, no mixer, element in its own host — and now a frame | none |
| Two videos | two `.mov` of one session | one stage in the leading host; two boxes side by side; a mixer inside each; one bar beneath | none |
| Three or more | three `.mov` of one session | three boxes in the same row, wrapping when the pane is too narrow for three | none |
| Narrow pane | the note pane at 320 px | the control row stays one row; no control's text breaks | none |
| Status shown | a stall, a refusal, a half-landed seek | the message is on its own line below the controls; the row still does not break | none |
| Status empty | the ordinary case | the status line is `display: none`, so the bar is one row high | none |
| Follower deleted | the second embed is edited out | its element is handed back to its host, released, and the survivor gets native controls | none |
| Leader deleted | the first embed is edited out | the group is handed down; the survivor returns to its own host with native controls | none |
| Follower fails to load | `error` from the camera track | the track leaves the group FIRST, then the host is restored to the link — no dead player under it | reported as the link |
| Widget scrolled out | CodeMirror destroys a widget whose element is in the stage | released through the host, not through a search of an empty host | none |
| Two editors, one note | same session, two views | two stages; one reader's group does not contain the other's element | none |

</intent-contract>

## Code Map

- `src/components/notes/editor/recording-embed.ts`
  - `FRAME_PRIME_SECONDS`, `HAVE_CURRENT_DATA` and `primeFirstFrame` — new, and the whole of the
    blank-video fix. Registered in `elementFor` for `kind: "video"` only, before `src` is assigned.
  - `releaseRecordingMedia` now calls `releaseHost(dom)` **before** searching the host, because a
    grouped track is not in its host.
  - The failed-load handler releases before it restores, and the separate `error` listener that used
    to call `releaseTrack` is gone — one handler, one stated order.
- `src/components/notes/editor/recording-transport.ts`
  - `PLAY_GLYPH`, `PAUSE_GLYPH`, `BACK_GLYPH`, `FORWARD_GLYPH`, `MUTE_GLYPH`; `control` takes a
    glyph and a label as separate arguments.
  - `transportBar` gains a `.cm-lp-recording-transport-row` and puts the status outside it.
  - `transportStage` — new. `Member` gains `element` and `box`. `refresh` splits into `engage`,
    `disengage` and `unbox`.
  - `hostOwners` and `releaseHost` replace `owners` and `releaseTrack`: one door, keyed by the one
    thing a widget still has while it is being destroyed.
- `src/components/notes/editor/live-preview.ts` — the recording block of the base theme:
  `-stage`, `-tracks`, `-track`, `-transport-row`, the `:empty` status rule, the muted-glyph rule,
  and a `.cm-lp-recording-track .cm-lp-recording-player` rule that leaves the lone video's own rule
  alone.

## Tasks & Acceptance

**Execution:**
- [x] The blank-video cause established by measurement in a real WKWebView before anything was
      styled, and named: `preload="metadata"` leaves the element frameless per spec.
- [x] The single video found to be equally frameless, and the AC overturned on that evidence.
- [x] Frame primed once, on metadata, only for an untouched element, on every video.
- [x] Glyph controls with unchanged accessible names; the control row carries no whitespace.
- [x] One stage, tracks side by side, each mixer inside its own track's box.
- [x] The teardown path re-keyed to the host, so staging cannot leak a follower.
- [x] Tests: 46 in `recording-embed.test.ts`, 39 in `recording-transport.test.ts`.
- [x] Reverts proved: 15 mutations, each applied, run and watched to fail. Two survived the first
      pass; both were real holes and both were closed.

**Acceptance Criteria:**
- `bun run test src/components/notes/editor/recording-embed.test.ts
  src/components/notes/editor/recording-transport.test.ts` passes — 85 tests.
- The blank-video cause is named, measured, and fixed.
- The transport does not wrap at the pane's minimum width, asserted by the property that prevents
  the wrap, and measured in a real WKWebView at 320 px.
- Each volume control is inside its own track's box, asserted in jsdom and measured in WebKit.
- A single video keeps its native controls, its own host, and no furniture — plus the frame it never
  had.

## Design Notes

### What was measured, and on what

Everything below was measured in a real WKWebView on the owner's own machine, driving the owner's
own two-track session (`screen-0000.mov` 1440×900 and `camera-0000.mov` 1280×720, both HEVC, both
about eight seconds). Three findings the harness had to establish about itself first, because each
one would otherwise have been read as a finding about the code:

- **A WKWebView with no window has no media engine.** Every `<video>` — including a plain H.264
  `.mp4` — ends at `networkState` 3 (NETWORK_NO_SOURCE) with `readyState` 0, while an `<img>` beside
  it loads normally. The probe has to run inside the Aqua session, in a real window.
- **An offscreen or occluded page throttles its timers to roughly one a second**, so a probe that
  polls sixty times outlasts any patience the harness has. Everything waits on events.
- **`drawImage` from a `<video>` reads the frame the compositor is PRESENTING.** A probe that does
  not wait for a real paint — or whose window is not being composited — reads back empty for an
  element with a perfectly good frame. This is the one measurement that is not reproducible on
  demand on a machine somebody else is using, and it is called out again below.

### The cause

| | before | after |
|---|---|---|
| `readyState` | 1 (HAVE_METADATA) | 4 |
| `networkState` | 1 (IDLE) | 1 |
| `videoWidth` / `duration` | 1440 / 8.083 s, 1280 / 8.038 s | unchanged |
| `error` | `null` | `null` |
| box | 312×195 and 312×176 | unchanged |
| painted pixels | **0** | **988 of 1024** |

The first four rows are what eliminate the other candidates, and they eliminate them with
measurements rather than with reasoning:

- **Not the protocol.** Both files loaded. `networkState` is IDLE and `error` is `null` on both;
  `recording_protocol.rs` is re-entrant anyway — every request gets its own `spawn_blocking` with no
  shared state and no lock. A refused second request would in any case have shown as a *link*, not a
  grey box: the widget's `error` handler restores the host to the wikilink.
- **Not two widgets resolving one target.** They report different `videoWidth` (1440 vs 1280) and
  different `duration` (8.083 vs 8.038). Two files.
- **Not a layout collapse.** The boxes measure 312×195 and 312×176. A collapsed box would have been
  invisible; the owner reported a grey box, which is a box.
- **The `controls` candidate, and the twist.** The lone video with `controls: true` measured
  `readyState` 1 and 0 painted pixels as well. Removing `controls` did not break anything; it
  removed the thing that was covering what was already broken.

`currentTime = 0.001` took `readyState` from 1 to 2 and produced 988 lit pixels out of 1024 for the
screen track and, separately, for the lone video.

### The AC that was overturned, and why that is the right call

The epic's fourth acceptance criterion was "a single video is unchanged from 42.6". It was written
before the measurement, to stop this story regressing the case that worked. The measurement says the
case that worked was not working — it was covered. Honouring the sentence would have meant knowingly
shipping a defect in the common path because of a guess made before the evidence. The epic author
overturned it explicitly and will amend the epic text; this spec does not edit the epic file.

So the prime lives in `recording-embed.ts`, on every video the module creates, rather than in the
transport. That is also the better home on its own merits: it is a fact about a media element with
`preload="metadata"`, not a fact about a pair.

What "unchanged" still means, and is asserted: a lone video keeps native `controls`, gets no stage,
no track box and no mixer, and stays in its own widget host.

### The cost, stated plainly

Assigning `currentTime` is a seek, and a seek is a real range request for real bytes against a file
that may be a multi-hundred-megabyte screen recording on a pendrive or a network volume. **Opening a
note therefore touches the drive once per embedded video, on open rather than on play.** It buys one
keyframe and not the file — `preload="auto"` was the alternative and it is not a serious one — and a
video in a note that shows nothing until you press play is not a video in a note. But it is a price,
and the next person should not have to discover it.

A millisecond, and both bounds are real. Non-zero, because a seek to the position the element
already reports is one a user agent may collapse into nothing. Far below one frame's duration —
33 ms at 30 fps — so the frame presented is the frame at the position asked for and not the one
after it.

### Once, and not in the reader's way

Two guards, and they defend different things. `currentTime !== 0` refuses to move an element the
reader or the transport has already placed: by the time metadata lands, the pair may be at 37.5 s,
and dragging it back to the top would turn a cosmetic fix into a control that moves the recording
under somebody's hand. `readyState >= HAVE_CURRENT_DATA` refuses to spend a range request on an
element that already has a frame.

Neither guard covers a reader who scrubbed back to *exactly* zero before a second `loadedmetadata` —
a source change, a reload. That is what `{ once: true }` is for, and it is declared on the listener
rather than unregistered by hand, so the platform enforces it and the element is left holding one
fewer listener. This gap was found by mutation, not by inspection: the first version's manual
`removeEventListener` was mutated away and no test noticed.

### Why the wrap is fixed by text and not by a wrapping rule

Measured at a 320 px pane, before:

| control | text | line boxes | height |
|---|---|---|---|
| Back | `Back 10 seconds` | **2** | 31 px |
| Play | `Play` | 1 | 18 px |
| Forward | `Forward 10 seconds` | **3** | 44 px |
| readout | `0:00 / 2:00` | 3 | 45 px |

Five distinct rows, a 51 px bar. That is the owner's "Back 10 / Pla y / Forward 10 seconds"
exactly. After, at the same 320 px:

| control | text | line boxes |
|---|---|---|
| Back | `↺10` | 1 |
| Play | `►` | 1 |
| Forward | `↻10` | 1 |
| readout | `0:00/0:08` | 1 |

One row, 26 px.

`flex-wrap: nowrap` is in the theme, and it is the belt rather than the braces: **a button wider
than its container breaks its own text whatever the wrapping rule on its parent says.** The thing
that actually prevents the wrap is that no run of text in the row contains a space. That is why the
jsdom assertion is over text and not over a class name — it is the causal property, and it fails the
instant anyone puts a phrase back on a button.

The skip glyphs carry the number (`↺10`, not `↺`), because "how far" is the one thing a triangle
cannot say. The readout drops the spaces around its separator for the same reason as everything
else: a space is a break opportunity.

The status is the one real sentence on the bar — "Playback was refused" — and it moved to its own
line rather than being truncated. A message the reader most needs to read is the worst possible
thing to ellipsise, and it collapses to nothing (`:empty { display: none }`) in the ordinary case,
so the bar is one row high almost always.

### The stage, and the hole it opened

Two embeds are two widgets in two `.cm-line` blocks. CodeMirror owns those lines, they are
block-level, and nothing this module may write puts them side by side. So the transport re-parents:
one stage in the leading embed's host, holding a row of track boxes with the bar beneath. Each box
holds one video and that video's own mixer, which is the whole of the "the slider sits away from the
track it governs" report — a border is a boundary, and a control inside one is unambiguously about
what else is inside it.

Measured: one stage, two boxes at x=7 and x=834 sharing a row, each holding its own video with its
mixer below it.

Re-parenting also makes the leader handover *simpler* than Story 43.6's: handing the group down
moves one node and the tracks travel inside it, so position, play state and focus survive as before.
`append` on an existing child moves it, so re-running `engage` on every join re-sorts the boxes into
document order for free.

**And it put a hole straight through the teardown.** `releaseRecordingMedia(dom)` searches `dom` for
the media element it must release, and a follower's host is empty while the pair is staged — so it
would find nothing, release nothing, and leave the transport holding an element whose widget is
gone. That is the exact leak that function exists to prevent, reintroduced by the fix for the
layout. The repair is to key the release by HOST instead of by element: `hostOwners` and
`releaseHost` replace `owners` and `releaseTrack` entirely, one door rather than two, and `leave`
hands the element home before it drops it. The same ordering question shows up on the failed-load
path, where the track must leave *before* the host is restored — otherwise the restore puts the link
back and the departing element is handed in underneath it. Both are asserted.

### What the reverts proved

Fifteen mutations, each applied to the shipped source, run, and watched to fail:

| Mutation | Caught by |
|---|---|
| The frame is never primed at all | 1: `primes both real players for a frame once the metadata lands` |
| The prime yanks an element the reader already moved | 2, including `leaves an element the reader or the transport already moved exactly where it is` |
| The prime re-buys a frame the element already has | 1: `buys nothing for an element that already has a frame` |
| The prime fires on every `loadedmetadata`, not once | 1: `buys one frame and not one per event, even for a reader back at the top` |
| The teardown looks only inside the host, as before staging | 3, including `releases a follower whose element the stage is holding` |
| A failed load restores the host before the track has left | 1: `puts a failed follower back to a link with no dead player under it` |
| The controls show their phrases again | 4, including `offers no break opportunity anywhere in the row's visible text` |
| The readout gets its spaces back | 4, the same set |
| The status sits back on the control row | 1: `keeps the one real sentence off the control row entirely` |
| The toggle stops swapping its glyph | 1: `plays and pauses both tracks from one button` |
| A leaving track is not handed back to its own host | 3, across both deletion tests and the follower release |
| The mixer goes beside the track instead of inside its box | 2, including `stages the pair as one player, each track boxed with its own mixer` |
| The tracks are never gathered into one stage | 3 |
| The stage is never re-seated under a new leader | 1: `hands the bar down when the leading track scrolls away` |
| A track's element is forgotten, so nothing can be moved | 3 |

Two survived the first pass, and neither was argued away. The `once` semantics had no discriminating
test — the existing one passed on the `currentTime !== 0` guard alone — and the failed-load ordering
had no test at all. Both are now covered by the two tests named above, and the mutation run is clean.

## What is proved against a fake, what against a real engine, and what on no machine at all

**Proved in jsdom** (85 tests): the prime's policy in all five of its branches; the no-break-
opportunity invariant over the control row's real text at every width of clock; the glyph/label
split; the stage's structure, with each mixer inside its own box; the lone video's untouched shape;
the release path through an empty host; the failed-load ordering; and everything Story 43.6 already
held. jsdom performs no layout and decodes no media, so none of this says anything about pixels or
rows.

**Proved in a real WKWebView, on the owner's own two-track session, driving the shipped modules**
(the widget, the transport, the stage and the theme were bundled from source; only the Tauri IPC
client was stubbed, and `keeper-recording://` was served by a real `WKURLSchemeHandler`):

- the cause, as the table above states it;
- that `currentTime = 0.001` moves `readyState` from 1 to ≥ 2, on every element, every run;
- that it produced 988 lit pixels out of 1024 — for the screen track inside the pair, and for the
  lone video;
- that the control row is one row 26 px high at a 320 px width, with every control at one line box;
- that the stage puts two boxes side by side, each holding its own video and its own mixer;
- that the lone video keeps `controls: true`, no stage, no box, no mixer, and its own host.

**Not settled, and it should be said rather than glossed:**

- **The camera track's frame never read back.** In every run, `drawImage` of the camera element
  returned nothing while the screen element in the same run returned 988 lit pixels. Its first frame
  is, independently measured with `ffmpeg`, essentially black (mean luma 1.1 of 255 — the sensor had
  not exposed), but a black frame is opaque and would have read back as such; a magenta canvas
  painted under the draw survived intact, so nothing was drawn. Both files are HEVC 4:2:0 and both
  reach `readyState` 4. Whether WebKit declines to hand that particular frame to a canvas, or
  whether it genuinely presents nothing at 0.001 s, is not resolved here.
- **The pixel readback is not reproducible on demand.** It depends on the probe window actually
  being composited, and the machine has a person using it. It succeeded in the runs where the window
  was being drawn and returned empty in the runs where it was not — including for elements that had
  demonstrably worked minutes earlier. `readyState` is the signal that held across every run, and it
  is the spec's own definition of whether the element has a frame.
- **How much the prime costs on a real slow volume.** The argument that one keyframe range request
  is a fair price for a visible frame is reasoning, not measurement. The files measured here are
  1.7 MB and 4.3 MB on a local disk. A note with six embeds on a USB 2 pendrive is the case to watch,
  and this is the first place to look if opening a note becomes slow.
- **Whether the drift threshold is right in practice.** Unchanged from Story 43.6 and unmeasured
  here for the same reason: nothing in this suite decodes two files in step.
- **A real pointer drag on the scrub bar inside CodeMirror.** `ignoreEvent` is asserted to claim
  slider events, which is the mechanism; the gesture itself is not exercised.

## Deliberately NOT done

- **The follower's line is left blank while the pair is staged.** Its element has moved into the
  leader's stage, so the widget host renders as an empty span. The line is still there, the source
  text is unchanged, and clicking it reveals `![[camera-0000.mov]]` as it always did. Hiding the
  line is a decoration-layer decision and this module does not own the decoration layer.
- **No poster attribute.** A `poster` would show something without a range request, but keeper would
  have to generate and store a thumbnail per track — a cache, an invalidation rule and a place to
  put it, none of which the field report asked for. The prime buys the real frame instead.
- **No fullscreen, picture-in-picture, playback rate or captions on the shared bar.** Still the real
  cost of engaging the transport, still recorded as one, still a UI story of its own.
- **No keyboard shortcuts.** Unchanged from Story 43.6: the controls are real focusable `<button>`s
  and a real `<input type="range">`, and a note editor's keymap is not a media player's.
- **No change to the drift correction, the fold, the refusal handling or the seek bookkeeping.**
  Story 43.6's transport logic is untouched. This story changed what it draws and where it puts it.
- **No `<audio>` in the group.** Unchanged from Story 43.6, and audio is not primed either, because
  there is no frame to buy.
- **The epic file is not edited.** The overturned acceptance criterion is recorded here; the epic
  author owns the epic text.
