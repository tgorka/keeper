---
title: 'Story 43.6: Two Videos, One Transport'
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
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-5-one-attachment-vocabulary.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-4-the-note-stub.md'
---

<intent-contract>

## Intent

**Problem:** a session that recorded a screen and a camera writes two `.mov` files, and a note that
embeds both gets two players. Story 43.5 made each of them render; nothing made them agree. Two
native `<video>` transports side by side hand the reader two clocks and no way to keep them in step,
and the first thing two independent decoders do is disagree. Scrubbing to 4:12 in the screen track
tells you nothing about where the camera track is.

The controls are the easy half. The hard half is that a *pair* can be in states no single element
can report, and the obvious implementation — ask whichever element we touched last — reports every
one of them wrong:

- one track stalls on a seek while the other lands it, and the bar says "playing";
- `play()` returns a promise that can reject (an autoplay policy, a detached element, a resource
  that went away), and a pair where one `play()` was refused is not playing — but a button stuck on
  "Pause" blames the reader for the next click;
- the two clocks drift apart with neither element raising a single event about it.

**Approach:** one transport object per session per editor, holding a list of tracks. Every state it
publishes is a fold over ALL of them — `some` for the bad news, `every` for the good — so there is
no code path by which it can report one element's opinion as the pair's. Time is centralised
(play/pause, scrub, `±10s`, the readout); volume and mute are deliberately not, because how loud the
camera sits under the screen is a mixing decision and not a time decision (UX-DR53). Drift past a
stated threshold is corrected toward the scrub position, and a seek that only half-lands stays
visible until every track confirms it.

## Boundaries & Constraints

**Always:**
- Every reported state is a fold over every track. A stall, a refusal or an unconfirmed seek in ONE
  track is a fact about the pair.
- The drift threshold is a named constant with a comment saying why that number, and the tests
  assert it as a literal rather than deriving their offsets from it.
- One video keeps its native `controls`, exactly as Story 42.6 shipped it. The transport engages at
  two and disengages back to one.
- Volume and mute stay per track, beside their own video.
- The transport is scoped to one `EditorView`, so two editors open on one note are two readers.
- A departing track hands the shared bar down before its host is emptied; the bar node is moved, not
  rebuilt, so the reader's position and play state survive.

**Never:**
- No second embed module and no second embed syntax: this is a behaviour added below
  `recording-embed.ts`'s existing branch, and the markdown a note holds is unchanged. Obsidian still
  renders `![[a.mov]]` and `![[b.mov]]` as two of its own players; keeper simply agrees with itself.
- No absolute path anywhere in the surface (FR-145); the transport never touches a path at all.
- No new dependency, and no timer. Everything is driven by the media events the platform already
  raises.
- No autoplay. Engaging the transport starts nothing.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| One video | a note embedding one `.mov` | native `controls`, no bar, no mixer | none |
| Two videos | two `.mov` of one `session:` | both lose `controls`; one bar under the FIRST embed; one mixer each | none |
| Three or more | three `.mov` of one session | the same, over three; a fold is not arity-2 | none |
| Play | the bar's Play | `play()` on every track; button reads Pause | none |
| Pause | the bar's Pause | `pause()` on every track; button reads Play | none |
| Scrub | drag to 0:37.5 | every track's `currentTime` is 37.5 | none |
| `+10s` / `-10s` | pressed | every track moves to `position ± 10`, from the SHARED position, not either clock | none |
| Skip past either end | `-10s` at 0:05, `+10s` at 1:59 of 2:00 | clamped to 0 and to the span | none |
| Span | tracks of 120 s and 119 s | 120 s — the LONGEST, so the screen recording's last second is reachable | none |
| No span yet | `preload="metadata"` has not answered | scrub DISABLED (a `max="0"` range silently clamps every drag to 0); `±10s` still live | none |
| Volume | one track's slider | that track only; no clock moves | none |
| Mute | one track's button | that track only; the button then offers Unmute, named for its file | none |
| Stalled track | `waiting` / `stalled` from one track | the PAIR is `buffering`, not `playing`; status says so | none |
| Stall clears | `playing` / `canplay` | back to `playing` | none |
| Refused `play()` | one track rejects | intent drops to paused, EVERY track is paused, status says Playback was refused, title carries the platform's words | reported, never thrown |
| Refusal after a pause | reader paused while promises were in flight | the refusal is dropped — noise about a dead question | none |
| Refusal after a scrub | reader scrubbed while promises were in flight | still reported: scrubbing does not withdraw "play" | none |
| Half-landed seek | one track never raises `seeked` | `seeking` stays true, status says Seeking | none |
| Swallowed seek | one track ignores `currentTime` and raises nothing | still `seeking`: the request was recorded before it was made | none |
| Seek in flight | `timeupdate` arrives from the reference | position is NOT updated — the thumb does not jump out from under the reader | none |
| Track errors mid-seek | `error` from an unconfirmed track | it stops holding the pair on Seeking, and counts as stalled | none |
| Track leaves mid-seek | widget destroyed | dropped from the unconfirmed set | none |
| A track pauses itself | `pause` while intent is playing | the whole pair pauses — a paused pair is recoverable, a growing gap is not | none |
| Shorter track ends | `ended` from one of two | the pair keeps running to the longer one's end | none |
| Every track ends | `ended` from all | the pair pauses | none |
| Drift 0.8 s ahead | follower ahead of the scrub position | corrected to the scrub position | none |
| Drift 0.9 s behind | follower behind it | corrected to the scrub position | none |
| Drift 0.45 s | inside the threshold | left alone: `timeupdate` sampling jitter is not drift | none |
| Drift on an ended track | follower at its own end, pair still running | not corrected — a seek it can never win | none |
| Correction in flight | the follower has not confirmed the correction | reported as `seeking`, because that is what it is | none |
| Leader leaves | the top embed scrolls out or is deleted | the bar MOVES to the next track's host; position preserved | none |
| Falls to one track | the second embed is deleted | native `controls` return; bar and mixers gone | none |
| Two editors, one note | same session id, two views | two transports; one reader's Play does not move the other's video | none |
| Slider inside CodeMirror | a drag on the scrub or a volume | the event is claimed, so the line is not revealed and the control survives the gesture | none |

</intent-contract>

## Code Map

- `src/components/notes/editor/recording-transport.ts` — **new, and the whole story.**
  `RecordingTransport` (the fold, the intent, the seek bookkeeping, the drift correction), the bar
  and the per-track mixer, `MAX_DRIFT_SECONDS`, `SKIP_SECONDS`, `transportFor` (per view, per
  session) and `releaseTrack`.
- `src/components/notes/editor/recording-embed.ts` — three seams and nothing else:
  `RecordingEmbedOptions.transport`; a `join` after the mount in `renderRecordingEmbedInto` (after,
  because the leader is decided by document position and a detached element has none); a
  `releaseTrack` at the head of `releaseRecordingMedia`; and `input` added to `ignoreEvent`'s
  selector for the two new sliders.
- `src/components/notes/editor/live-preview.ts` — `transportFor(view, sessionId)` passed to the
  widget, and additive rules at the end of the existing recording block in the base theme.

## Tasks & Acceptance

**Execution:**
- [x] One transport per session per view, folding over a list of tracks.
- [x] Play, pause, scrub, `±10s` and the readout centralised; volume and mute left per track.
- [x] `MAX_DRIFT_SECONDS` named, justified in a comment, and asserted as a literal.
- [x] The pair reports buffering for a stall, paused for a refusal, seeking for a half-landed seek.
- [x] Engagement at two, disengagement at one, and the bar handed down when the leader leaves.
- [x] Tests: 35 in `recording-transport.test.ts`, 36 in `recording-embed.test.ts` (7 of them driving
      real `<video>` elements inside a real `EditorView`).
- [x] Reverts proved: 17 mutations, each applied, run and watched to fail (see Design Notes).

**Acceptance Criteria:**
- `bun run test src/components/notes/editor/recording-embed.test.ts` passes — 36 tests.
- `bun run test src/components/notes/editor/recording-transport.test.ts` passes — 35 tests.
- Play, pause, scrub and `±10s` move both, asserted on fakes and on real `<video>` elements.
- Volume and mute move one, asserted on fakes and on real `<video>` elements.
- A stalled track reports the pair as buffering, not playing.
- A rejected `play()` leaves the transport paused, every track paused, and a stated reason.
- Drift past the threshold is corrected toward the scrub position; drift inside it is not.

## Design Notes

**One video gets the native controls, and that is the considered answer, not the lazy one.** A lone
`<video>` has no second clock to keep it honest, so the entire reason this module exists is absent —
and a hand-built bar for one track is a strictly worse `<video controls>`: no fullscreen, no
picture-in-picture, no captions menu, no platform keyboard conventions, no playback-rate menu, and a
fresh set of accessibility bugs the platform already fixed. So the transport engages at two and
disengages back to one, and Story 42.6's single-video path ships untouched. The cost is a visible
transition — a second embed appearing changes the first video's furniture — and that is the right
trade, because the alternative is either a worse control for the common case or two clocks for the
case this story exists to fix.

**Three or more is the same claim as two, and the code never says "2".** The invariant is *one
session, one moment, one clock*, and the arity is incidental: a three-angle session buffers when any
one angle stalls and scrubs all three together. Everything is `some` / `every` over a list. The only
special number is one, above.

**`MAX_DRIFT_SECONDS = 0.5`, bounded from both sides.** Below it the *measurement* is noise:
`timeupdate` is only required to fire about four times a second, so two tracks sampled at different
moments legitimately read up to a quarter second apart while being perfectly in step — a tighter
threshold would re-seek forever against its own sampling jitter. Above it two views of one moment
visibly disagree: a cursor that moves before the click, a mouth that moves after the word. And a
correction is not free — assigning `currentTime` forces a real seek, and on a screen recording with
sparse keyframes that seek can cost longer than the drift it repaired, so the number has to be one
worth paying a seek for. Both bounds are asserted: raising it to 5 fails three tests, lowering it to
0.1 fails two others, and one test asserts the literal.

**Correction is toward the scrub position, not toward the other track.** The position is what the
bar shows and what the reader asked for. A pair that agreed with each other while disagreeing with
the readout would be a third kind of lie. The consequence is deliberate and worth stating: if the
LEADING track is the one that stalls, the position stops advancing and the follower gets pulled back
to it — the pair moves at the speed of its slowest member. That is the correct behaviour for two
views of one moment, and it is why a stall is surfaced as `buffering` rather than hidden.

**The generation guard distinguishes a withdrawal from a continuation, and the distinction is the
interesting part.** `play()` awaits `allSettled` over every track, so a refusal can land after the
reader has done something else. If they *paused*, the refusal is dropped — telling someone a play
they abandoned was refused is noise about a dead question. If they *scrubbed*, the refusal is still
reported, because scrubbing does not withdraw "play": they still want the pair running, and one
track refusing is news whichever second they land on. `pause()` bumps the generation; `seekTo` does
not. Two tests hold the two halves apart.

**`allSettled`, never `all`.** `all` rejects on the first failure and leaves the other track playing
alone — precisely the half-state this transport exists to prevent, and a solo view of a two-view
moment drifting further from its partner every second. One refusal pauses everything.

**The seek is written down before it is made.** Every track is added to the unconfirmed set before
any `currentTime` is assigned, rather than relying on the `seeking` event. An element that swallows
the assignment — detached, errored, mid-teardown — raises no `seeking` at all, and a transport that
learned only from events would settle the bar on a position half the pair never reached. The
`seeking` event is still listened to, because a seek the transport did not order (the platform's
own, or a drift correction) is also a seek the pair is waiting on.

**A real bug the real-editor test found, which no unit test would have.** The scrub is an
`<input type="range">`, and `preload="metadata"` means `duration` is `NaN` until the file answers.
The first version set `max = "0"` in that window — and a range input silently clamps every value the
reader drags to back to 0. The control looked live and moved nothing. It is now `disabled` until the
pair has a span, which says the true thing (not yet) instead of the false one; the `±10s` buttons
stay live, because a relative move needs no span. This surfaced only because the integration test
drives real jsdom `<video>` elements, whose `duration` really is `NaN` — the fakes had durations and
sailed past it.

**What the reverts proved.** Seventeen mutations, each applied to the shipped source, run, and
watched to fail:

| Mutation | Caught by |
|---|---|
| A refusal no longer pauses the pair | 2: `does not claim to be playing when one track's play() was refused`, `still reports a refusal that lands after a scrub` |
| A stalled track no longer buffers the pair | 3, including `reports the pair as buffering while one track is stalled, never as playing` |
| Drift is never corrected | 4, including the real-`<video>` `corrects a real player that drifted past the threshold` |
| `MAX_DRIFT_SECONDS` 0.5 → 5 | 3: `names half a second`, and both `0.8s ahead` / `0.9s behind` |
| `MAX_DRIFT_SECONDS` 0.5 → 0.1 | 2: `names half a second`, `leaves a follower 0.45s out alone` |
| Seek moves only the leading track | 7, across both suites |
| `play()` asks only the leading track | 4, including the real-`<video>` play assertion |
| Volume is shared across the pair | 1: `turns one real player down and leaves the other where it was` |
| Mute is shared across the pair | 1: `mutes one real player and leaves the other audible` |
| The transport engages for a single video too | 3, including `leaves a lone video its own native controls` |
| A half-landed seek is not marked unconfirmed | 1: `keeps a seek a track swallowed without a word visible too` |
| The bar is not handed down on teardown | 1: `gives the remaining player its own controls back when an embed is deleted` |
| `ignoreEvent` stops claiming slider events | 1: `claims the transport's slider events` |
| `live-preview` stops handing the widget a transport | 4 — every real-editor test in the pair suite |
| The scrub is left inert instead of disabled | 1: `disables the scrub bar until the pair has a span` |
| A track that pauses itself no longer stops the pair | 1: `stops the pair when one track pauses itself out from under it` |
| The leader is chosen by join order, not document order | 1: `leads with the first embed in the note however the two resolve` |

Four of those (the threshold raise, shared volume, the unmarked seek, and the slider claim) survived
the first pass. Each survivor was a real hole and each was closed rather than argued away: the drift
tests had derived their offsets from the constant they were meant to defend, and there was no
real-element volume test and no `ignoreEvent` test for the sliders at all.

## What is proved against a fake, and what is proved on no machine at all

The parent asked for this section plainly, so here it is plainly. **jsdom implements no media
playback.** Measured on this machine, not assumed: `play()` returns `undefined` after logging "not
implemented", `pause()` and `load()` do the same, `readyState` never leaves 0, `duration` is `NaN`
forever, and `timeupdate`, `waiting`, `stalled`, `seeking` and `seeked` never fire. `currentTime`,
`volume`, `muted` and `controls` are ordinary settable properties and do work.

**Proved against a fake track (`FakeTrack` in `recording-transport.test.ts`).** Everything that
needs an event no jsdom element will ever raise: the stall fold, the refusal rollback and its
generation guard, the half-landed and swallowed seeks, the drift measurement and correction, the
`ended` handling, the self-pause, the leader handover, and the engage/disengage transition. The fake
is a faithful model of the *interface* — an `EventTarget` with the same properties and the same
event names — and it is an honest model of nothing else. What it cannot tell us is whether a real
pair of `.mov` files produces those events in the order and at the times assumed.

**Proved against real `<video>` elements in a real `EditorView`** (7 tests in
`recording-embed.test.ts`): that two embeds of one session find each other through the real
decoration layer at all; that both real elements lose `controls`; that exactly one bar is mounted,
in the leading widget's own host; that two mixers exist; that the bar's Play calls `play()` on both
real elements; that the scrub and `±10s` write both real elements' `currentTime`; that a real
element's volume and mute move alone; that a real drifted element is corrected; that deleting one
embed returns the survivor's native controls; and that a slider's events are claimed from
CodeMirror. Three of those need help jsdom cannot give: `play`/`pause` are stubbed, `duration` is
defined on the instances, and `seeked` is dispatched by hand. Each of those three is commented at
the call site.

**Proved on no machine here.** Stated so the reviewer does not have to infer it:

- **That two real `.mov` files actually stay in step.** Nothing in this suite decodes a frame. The
  transport is proved to *react* correctly to drift; whether the drift it will meet in a WKWebView
  is 0.05 s or 3 s, and therefore whether 0.5 s is the right threshold in practice, is a question
  only the owner's machine with the owner's recordings can answer. If the correction turns out to
  fire constantly, the number is wrong and this is the first place to look.
- **Whether a `currentTime` correction on a sparse-keyframe screen recording is cheap enough.** The
  comment on `MAX_DRIFT_SECONDS` claims a seek can cost more than the drift it repaired. That is the
  reasoning behind the number; it is not measured.
- **Whether the real autoplay policy rejects `play()` in a Tauri webview at all.** The refusal path
  is proved to be handled; that it ever fires in production is not.
- **Layout.** jsdom performs no layout, so nothing here says the bar looks right, that a
  `flex` row of six controls fits a narrow note pane, or that the readout does not reflow the scrub
  bar once a second. The `fontVariantNumeric: tabular-nums` rule is a precaution, not a measurement.
- **Focus and pointer behaviour during a real drag.** `ignoreEvent` is asserted to claim slider
  events, which is the mechanism; that a real pointer drag across a real scrub bar survives a
  CodeMirror re-render is not exercised.

## Deliberately NOT done

- **No "wait for the slow one" sync-pause.** A more sophisticated multi-track player pauses the
  healthy tracks while one buffers and resumes them together. That is a better experience and a
  bigger story: it needs a resume-arbitration policy, a guard against a track that never recovers,
  and a way to distinguish a stall from an end. The epic asked for correction toward the scrub
  position, and that is what is here; the stall is surfaced rather than smoothed over.
- **No fullscreen, picture-in-picture, playback rate or captions on the shared bar.** These are the
  native affordances the two grouped videos give up, and reimplementing them is a UI story of its
  own. It is a real cost of engaging the transport and it is recorded here as one.
- **No keyboard shortcuts** (space to play, arrows to seek). The bar's controls are real focusable
  `<button>`s and a real `<input type="range">`, so it is keyboard-operable; it has no global
  bindings, because a note editor's keymap is not a media player's and Story 43.1 has just finished
  arguing about what Tab means in this editor.
- **No grouping of `<audio>` with the videos.** A session's audio sidecar is a separate recording of
  the same moment and the argument for one clock applies to it too, but nothing in the field report
  asked for it and the epic says two videos. The transport folds over a list of `TransportTrack`,
  which `HTMLAudioElement` satisfies, so admitting audio later is a change to the join condition in
  `renderRecordingEmbedInto` and nothing else.
- **No persistence of position across a reload.** Closing the note and reopening it starts at 0:00,
  as it did before this story.
- **No change to the note's bytes.** The markdown is two ordinary `![[…]]` embeds, unchanged, and
  Obsidian renders them as its own two players. The one clock is keeper's reading of them, not a
  claim written into the vault.
