# spec-48-2 — locking a capture window stops discarding your size, and no window can leave the screen

bindings: the owner's report on 0.8.1 — *"po resize gdy lockuje przywraca sie poprzedni size i moze wyjsc
poza monitor"*; FR-192, UX-DR77, UX-DR43, NFR-27, AD-55/AD-56; Story 46.15 (the promise), Story 47.5 /
DW-198 (the position half)
branch `work/epic-48`, worktree `quick-donkey`. Sentinel `MUT48-2`.
crates: `keeper-core` (`capture.rs`) — compiles and tests here;
`keeper` (`notes_window.rs`, `notes_ipc.rs`, `lib.rs`) — **never compiled here** (AD-55/AD-56)
owned files: `keeper-core/src/capture.rs`, `keeper/src/notes_window.rs`,
`keeper/src/notes_ipc.rs` (capture block only), `keeper/src/lib.rs` (capture window-event arm only)

---

## 1. What the report actually is: two defects that happen to share a click

The owner's sentence names one gesture and two independent faults. They were fixed separately because
they are separate, and either one alone still loses you a window.

### Defect A — the lock was a discard button, and 46.15's spec promised it was not

Story 46.15 shipped this sentence in its own edge-case table:

> lock pressed after a resize | window snaps to 560×340 **keeping its position**, and the size is kept in
> the row. […] **Unlocking restores it**

It could not. Both places that write a placement down merged the live window's geometry over the stored
row unconditionally:

```rust
// notes_ipc.rs, notes_capture_set_locked — the pre-48.2 code
let placement = Placement {
    locked,
    position: live.position.or(stored.position),
    size:     live.size.or(stored.size),
};
```

On the **lock** click that is correct: the window is still 900×600 when it is measured, so 900×600 is
what gets stored. On the **unlock** click it is fatal, and the reason is entirely in the ordering: the
*lock* has already called `adopt_placement` → `apply_size` → `window_size(locked)` →
`CAPTURE_DEFAULT_SIZE`, so by the time the user clicks again the live window **is** 560×340. The merge
writes keeper's own size over the user's, and then `adopt_placement` reads the row back and faithfully
restores… 560×340. The 900×600 is gone from sqlite, permanently, and no later unlock can find it.

It does not even need a second click. `remember_placement` (`lib.rs`'s blur arm, and
`notes_capture_close`) has the same merge, so **one click on another application after locking** is
enough. That is the shortest reproduction and almost certainly the owner's.

The same hole ate the **position**, which nobody reported because it is less visible. 46.15's own
`Placement` doc says *"unlock, drag, lock again is a person saying keep it **there**"* — but a locked
panel is re-placed by every hotkey press (Story 47.5, `plan_show_position(false) == Place`), so a locked
window's live coordinate is keeper's, and the first blur after locking overwrote the dragged position
with a fifth-of-the-way-down-the-pointer's-monitor one.

### Defect B — nothing clamped a position, anywhere

`clamp_size` clamps extent and says so at length. There was no counterpart for a coordinate, and
`set_position` was reached from three places with no check on any of them. Two reachable ways off the
screen, both consequences of code that was working as written:

- **The lock grows a small window from the same top-left.** 320×240 parked against the bottom-right
  corner → `apply_size` makes it 560×340 → nothing on that path repositions anything, so 240 px of
  window goes past the edge, *including the corner the close button is in*.
- **A remembered coordinate outlives its monitor.** `apply_placement` and `adopt_position` replayed a
  stored physical coordinate verbatim. Undock the second display and the window is restored onto a
  rectangle of desktop with no pixels behind it.

A capture window is undecorated and `skipTaskbar`. It is in no dock and no task switcher, so a window
off the edge has nothing left to click — which is the exact argument `clamp_size`'s doc already makes
about an oversized window, applied to the axis nobody had covered.

---

## 2. The decision: a guard, not a second remembered size

The brief offered two shapes and asked for an argument. **The guard, and it is not close.**

The alternative is a `user_size` on `Placement` beside `size`, so the applied size and the chosen one are
different fields. It is more code and it is less true, because **there is no moment when both are
informative**: while a window is locked its live size is *always* keeper's and carries no information at
all; while it is unlocked its live size *is* the user's. A second field would therefore be a copy of the
first that is only ever read in one state — plus a token in the persisted spelling, plus a `decode` arm,
plus a question about what old rows mean, plus (this wave, concretely) a tag collision to negotiate with
Story 48.4.

What is actually missing at the merge is not *which size was the user's*. It is *was this window under
the user's control when I measured it* — one boolean, which the shell already holds as `is_resizable()`:
the same attribute `apply_resizability` writes at boot and on every toggle, and the same one
`plan_show_position` reads on the hotkey path. Storing a derived copy in sqlite to avoid reading a fact
you already have in hand is the larger change and the weaker one.

So the rule, in one sentence, and it covers both axes with no special cases:

> **keeper remembers only geometry the user could have produced.**

Concretely: `Observed { position, size, user_controlled }` is what the shell reads off a live window;
`Placement::observing` is the only place it may become a stored `Placement`; `Placement::relocked` is
`observing` with the new lock state on top. `notes_capture_set_locked` and `remember_placement` now have
**no geometry logic in them at all** — which matters, because a struct literal in the crate that does not
compile on every machine is exactly how this got shipped wrong for two releases with no test able to say
so.

### The one asymmetry worth naming

`Observed::user_controlled` reads almost like `Placement::locked` and is deliberately a different
question. `locked` is what is *stored*; `user_controlled` is what the *live window* was when measured.
They disagree at precisely the moment that matters — on the unlock click the placement being written says
`locked: false` while the window being measured is still keeper's normalised one — and that disagreement
is the whole bug.

---

## 3. I/O matrix

| decision | who decides | tested here |
| --- | --- | --- |
| whether a live window's geometry may be remembered at all | `Placement::observing` | **yes** |
| what the lock toggle writes | `Placement::relocked` | **yes** |
| a coordinate pulled back onto the work area | `clamp_position` / `clamp_axis` | **yes** |
| a zero work area is not a measurement | `clamp_axis` | **yes** |
| where an unplaced panel goes (centre, a fifth down, clamped) | `auto_position` | **yes** |
| the top-fraction constant | `TOP_FRACTION` (moved from the shell) | **yes** |
| reading `is_resizable()` off the live window | `notes_window::geometry` | **no** |
| which monitor a coordinate is measured against | `notes_window::physical_work_area` | **no** |
| calling `set_position`, converting physical↔logical | `notes_window::ask_for_position` | **no** |

`TOP_FRACTION`, `centred` and `offset_from_top` **moved out of the shell** into `auto_position`. That is
not tidying: it is the difference between placement arithmetic three developers cannot run and placement
arithmetic every developer runs on every commit. It also means the clamp that keeps a *placed* window on
screen is literally the same code as the clamp that keeps a *restored* one there, so the two cannot come
to different conclusions about what "on screen" means.

`set_position` now appears **exactly once in the whole `keeper` crate**, in `ask_for_position`, and all
three callers reach it through a clamp. There is nowhere left for the clamp to be missing from — which is
how it came to be missing from all of them.

---

## 4. Units, restated because they are the trap

`Placement` already documents the asymmetry: **position physical, size logical**. This story adds a third
quantity and had to pick a side.

- `clamp_size(size, work_area)` — **logical**, fed by `logical_work_area`. Unchanged.
- `clamp_position(position, size, work_area)` — **physical throughout**, fed by the new
  `physical_work_area` and `physical_extent` (i.e. `outer_size`), matching the physical coordinate
  `set_position` takes.

Mixing them would be off by the scale factor on every HiDPI display and exactly right on the developer's
— the failure mode `logical_work_area` was created to prevent, which is why the physical one is named,
documented and placed directly beside it rather than inlined.

`WorkArea` carries an **origin as well as an extent**, and that is why it is a struct rather than
`clamp_size`'s bare `(u32, u32)`. A size is measured from nothing; a position is measured from the
virtual desktop's origin, and the second monitor's work area does not start at zero. Clamping a
coordinate against a bare extent would drag every window on every non-primary monitor onto the primary
one — a worse bug than the one being fixed, and mutant M6 exists to keep it fixed.

---

## 5. Edge cases

| case | behaviour |
| --- | --- |
| resize → lock → unlock | the user's size comes back. **46.15's sentence is now true.** |
| resize → lock → blur → unlock | same. The blur writes nothing, so there is nothing to undo. |
| resize → lock → close → reopen → unlock | same path as the blur; `close` reports `user_controlled` too. |
| unlock → drag → lock → hotkey → blur | the dragged position survives. A locked window's live coordinate is keeper's and is refused. |
| unlock → drag → resize → blur | both remembered. The guard must not over-fire, and `an_unlocked_window_still_writes_down_everything_it_reports` is the test that says so. |
| a platform reporting a size but not a position | the readable half is still remembered — 46.15's rule, unchanged. |
| a window that will not say whether it is resizable | treated as **keeper's**. That direction costs a remembered geometry and can never overwrite one, matching `edge_inset`'s caller. |
| a window that is not open at all | `Observed::default()` — `user_controlled: false`, so nothing is written. |
| a locked window blurring | **no sqlite write at all** now, not even a restatement. Previously a read plus a destructive write; now a read. |
| lock grows a 320×240 corner window to 560×340 | `keep_on_screen` pulls it back to (1360, 740) on a 1920×1080 work area. |
| a window that already fits | `keep_on_screen` calls nothing. Not economy: `set_position` is the call UX-DR43 says a compositor may refuse, and a refusal logged on every toggle of a window that was never off screen buries the one that matters. |
| a stored coordinate on a vanished monitor | `monitor_from_point` claims nothing → falls through to `focused_monitor` → clamped onto the screen the user is looking at. |
| a stored coordinate on a monitor that still exists | measured against **that** monitor, not the pointer's. Otherwise the clamp would drag a happily-placed second-screen window home on every open. |
| a work area reported as `0` | ignored, not clamped to — `clamp_size`'s guard, restated. Guarded **per axis**, so a readable width still clamps while an unreadable height does not. |
| no monitor at all (headless) | position returned untouched. Nothing is invented from nothing. |
| a window larger than the work area | pushed flush to the **near** edge, never shrunk. The size is `clamp_size`'s business and a clamp that changed both would fight it; the near edge is where the drag strip is. |
| the min/max size behaviour | **untouched.** `clamp_size`, `window_size` and both constants are byte-for-byte unchanged; 46.15's four clamp tests still pass unmodified. |
| DW-198 (locked follows the pointer, unlocked stays put) | **untouched.** `plan_show_position` and `adopted_position` are unchanged, and `adopt_placement` still adopts no *stored* position — see below. |

### Why `keep_on_screen` in `adopt_placement` does not reopen DW-198

47.5 was explicit that position adoption must not live in `adopt_placement`, because that function also
runs on the lock toggle and *"a padlock click is not a request to move a window"*. `keep_on_screen` is
not position adoption: it reads no stored coordinate, it is the correction for a resize **this function
itself just performed**, and it moves nothing that already fits. A window the user can no longer reach is
not a window they placed.

---

## 6. Mutation testing

Sentinel `MUT48-2`, applied one at a time to `capture.rs`.

**Baseline established GREEN first, in the same command and the same filter as the sweep**:
`cargo test -p keeper-core --lib capture::` = **45 passed / 0 failed / EXIT=0**. This was not free —
the filter was red for a while under Story 48.4's in-flight `Placement` field, and sweeping then would
have scored kills against tests that were already failing, which proves nothing. The sweep waited.

**9 applied, 9 killed, 0 survived**, each restored and re-verified at 45/0 **between** mutants.

| # | mutation | killed by |
| --- | --- | --- |
| M1 | `observing`: the guard removed entirely — the pre-48.2 unconditional merge | `resizing_then_locking_then_unlocking_gives_the_user_their_size_back`, `a_blur_while_locked_does_not_cost_the_size_or_the_position`, `a_window_that_will_not_say_is_never_taken_for_the_users` (3) |
| M2 | `observing`: the guard **inverted** — only keeper's geometry remembered | the above plus `an_unlocked_window_still_writes_down_everything_it_reports`, `a_half_answer_keeps_the_readable_half_and_invents_nothing` (5) |
| M3 | `relocked`: bypass `observing` and merge live-over-stored — literally the shipped 0.8.1 code | `resizing_then_locking_then_unlocking_…`, `a_blur_while_locked_…` (2) |
| M4 | `clamp_axis`: a zero work area treated as a measurement | `a_zero_work_area_moves_a_window_not_at_all` (1) |
| M5 | `clamp_axis`: the window's extent ignored — only the top-left kept on screen | `locking_a_small_window_in_the_corner_…`, `a_position_from_a_monitor_that_is_gone_…`, `a_second_monitors_origin_…`, `a_window_bigger_than_the_work_area_…`, `a_zero_work_area_…`, and both `an_unplaced_panel_…` tests (7) |
| M6 | `auto_position`: the work area's origin dropped | `an_unplaced_panel_is_centred_on_the_monitor_it_belongs_on` (1) |
| M7 | `auto_position`: the final clamp dropped | `an_unplaced_panel_never_hangs_off_the_monitor_it_is_placed_on`, `an_unplaced_panel_is_centred_a_fifth_of_the_way_down` (2) |
| M8 | `clamp_position`: an unknown display invents `(0, 0)` | `an_unknown_display_leaves_a_position_exactly_where_it_was` (1) |
| M9 | `relocked`: the toggle stops setting the lock | `the_toggle_still_sets_the_lock_whatever_the_window_reports`, `resizing_then_locking_then_unlocking_…` (2) |

**The guard and the clamp are proven separately**, as asked: M1–M3 and M9 touch only `observing`/
`relocked`; M4–M8 touch only `clamp_axis`/`clamp_position`/`auto_position`. No mutant in either group is
killed by a test the other group needs.

M3 is the one worth reading twice. It is not a synthetic mutation — it is the code that shipped in 0.8.1,
reinstated, and two named tests reject it. M2 is its mirror: it proves the fix is a guard and not an
off-switch, because a too-eager version that stopped recording an *unlocked* window's geometry would pass
every test M1 fails.

**Restore verified by reading, not by memory.** `cmp` against a byte copy taken before the sweep →
identical (sha256 `afccad4d…fa5a2`); `grep -rn MUT48-2` over the whole worktree → nothing; and the
`git diff -U0` on `capture.rs` was read line by line. Every removed line in that file belongs to Story
48.4's concurrent `decode` repair — **zero deletions are this story's**, because everything added to
`capture.rs` here is new items. `git diff` is blind to files a story creates; this story created none, so
there is nothing it could not see.

---

## 7. Deliberately NOT done

- **No `user_size` field, no new persisted tag, no `Placement` field at all.** §2 is the argument.
  Coordinated with Story 48.4 on `hub` before writing a line, because both stories wanted the same
  struct; 48.4 took the only new tag (`top`) and the `decode` structural repair that goes with it.
- **No change to `clamp_size`, `window_size`, `CAPTURE_MIN_SIZE` or `CAPTURE_DEFAULT_SIZE`.** The
  acceptance says the min/max size behaviour is unchanged, and the way to guarantee that is not to touch
  it. 46.15's four clamp tests pass unmodified.
- **No position adoption added to `adopt_placement`.** 47.5 argued that out and the argument still holds.
- **No clamping of a *stored* position at write time.** A row that names a monitor you have unplugged is
  still a true record of where you put the window, and clamping on write would silently forget it the
  first time you worked undocked. The clamp is applied on the way to the screen, every time, so
  re-docking restores the real position.
- **No `Moved`/`Resized` listener.** A drag emits one event per compositor frame; the existing
  blur/close cadence is deliberate and unchanged.
- **No shrink-to-fit in `clamp_position`.** Extent is `clamp_size`'s job; two clamps arguing over one
  number is how you get a window that oscillates.
- **No UI copy change.** The restore is still a `set_position` UX-DR43 says a compositor may refuse, so
  nothing in the interface starts promising it. The lock's label is unchanged.

---

## 8. What I could not verify here, and why

**Nothing in `src-tauri/crates/keeper/src/` was compiled.** Not `notes_window.rs`, not `notes_ipc.rs`,
not `lib.rs`. The `keeper` shell crate does not build on this Linux box (AD-55/AD-56) and no `cargo
build`/`check`/`clippy`/`test -p keeper` was run against it at any point. Everything below is
consequently unproven here:

1. **That the shell crate compiles at all.** What *was* checked: all three files were run through
   `rustc --edition 2021 --emit=metadata` and produce **zero syntax errors** — every diagnostic is
   `E0432`/`E0433`/`E0425` name resolution against absent dependencies, which is what a file that parses
   correctly and cannot see its crate graph looks like. That proves the files parse. It does not prove
   they type-check.
2. **The Tauri signatures.** `monitor_from_point`, `work_area`, `outer_size`, `outer_position`,
   `is_resizable` and `label` are all *pre-existing* calls in this file with unchanged argument and field
   usage, so they are proven by the last build that shipped. The genuinely new shapes are
   `WorkArea { position: (area.position.x, area.position.y), size: (area.size.width, area.size.height) }`
   and `label = %window.label()` in a `tracing::debug!`. Both mirror constructs already in the file.
3. **`notes_window::list`'s widened callback**, changed by Story 48.4 in the same file in the same wave.
   Neither of us can compile it. Flagged to that author.
4. **That `is_resizable()` answers truthfully on a *hidden* window.** This is the load-bearing runtime
   assumption of the whole guard, and 47.5 already listed it as unverifiable here for `reveal`. If a
   backend answers `Err` or `false` for a hidden-but-resizable window, the guard over-fires and an
   unlocked window's geometry stops being remembered — the fix would look inert rather than broken, which
   is the worse failure. **Gate check 4 exists for exactly this and must not be skipped.**
5. **That `outer_size()` reflects the size `set_size` just asked for**, which `apply_placement`'s
   size-before-position ordering depends on. That assumption predates this story; the clamp now leans on
   it too.
6. **That a compositor accepts the clamped `set_position`.** Wayland may refuse it (UX-DR43). A refusal
   is a debug log and a window the person can still drag — by design, not by omission.

### Gate checks, in order

Everything from 4 onward is on the Mac. **None of it has been performed; these are instructions, not
observations.**

1. `cargo test -p keeper-core --lib capture::` → EXIT=0. *(Done here: **45 passed / 0 failed**, and again
   after the mutation sweep restored the file.)*
2. `cargo test -p keeper-core --lib registry::tests::two_capture_windows_remember_two_placements_independently`
   → EXIT=0. *(Done here: 1 passed.)*
3. `bun run vitest run src/test/capture-capability.test.ts src/test/command-registration.test.ts` →
   EXIT=0. *(Done here: 16 passed. These are the only tests on this box that read the shell at all —
   they pin the capability glob, the event names and both size constants against `notes_window.rs`.)*
4. **`cargo build`, then `cargo clippy --all-targets -- -D warnings`, on the Mac.** Nothing below this
   line means anything until it passes, and three of the six unverified items above are settled by it.
5. **Unlock the draft panel. Drag an edge to roughly 900×600. Lock it** — it snaps to 560×340 and keeps
   its position, as 46.15 says. **Unlock it: the size you chose comes back.** This is the headline of the
   story and the sentence 46.15 shipped and could not keep.
6. **The same, with a blur in the middle.** Resize, lock, click on another application, click back,
   unlock. **The size must still come back.** This is the owner's shortest reproduction and the reason
   the guard could not live in the lock command.
7. **Resize, lock, quit keeper entirely, relaunch, unlock.** Same size. Proves the row itself was never
   overwritten, not merely that the in-memory path is right.
8. **Unlock, drag the panel somewhere unusual, lock, press the hotkey twice** (it re-places, as DW-198
   says it should), **then unlock and relaunch.** It returns to where you dragged it, not to where the
   hotkey put it. This is the position half of Defect A and it has never worked.
9. **Unlock, resize the panel small (~320×240), drag it hard into the bottom-right corner, lock it.**
   It grows to 560×340 **and stays fully on screen.** Before this it kept its top-left and put its own
   close button past the corner.
10. **Two monitors.** Unlock the panel, drag it onto the second display, blur, quit. Unplug that display.
    Relaunch and press the hotkey: the panel appears **on the remaining monitor, fully visible**. Plug
    the display back in, relaunch: it is back where you left it. The second half is as important as the
    first — the clamp must rescue a lost coordinate without forgetting a good one.
11. **DW-198 unchanged.** Lock the panel and press the hotkey with the pointer on each monitor in turn:
    it follows the pointer, a fifth of the way down, every time. Unlock it and press the hotkey: it stays
    put.
12. **Wayland specifically.** Repeat 9 and 10. A refused `set_position` must log at debug and leave a
    usable window — not a hang, not an invisible panel. Nothing in the UI promises the restore, so a
    refusal is a non-event by design; confirm that it actually is.
13. **A second capture window** (a note, not the draft). Repeat 5, 6 and 9 for it: a different key, a
    different row, and it must not move or resize the draft window.

---

## 9. Deferred work

Two, both discovered while reading rather than while fixing, and neither in scope:

- **DW — `apply_placement` assumes `outer_size()` is up to date immediately after `set_size`.** The
  size-before-position ordering has depended on this since 46.15 and the clamp now depends on it too. On
  a backend where sizing is asynchronous, a window would be clamped against its *previous* extent — the
  clamp would under-fire, not misfire, so the symptom is the old bug reappearing in one corner case
  rather than a new one. Worth confirming on the Mac before deciding whether it needs a
  `request_user_attention`-style settle.
- **DW — `notes_capture_close` and the blur handler both write a placement for the same window.**
  Closing a focused window fires `Focused(false)` and then the close command, so two reads and (before
  this story) two writes happened for one gesture. This story removes the second write for a locked
  window and makes the unlocked case idempotent — `observing` is a pure fold, and the write is now
  skipped when nothing changed — so the duplication is harmless today. It is still two paths that must
  agree, and the next person to add a field to `Placement` will have to notice both.
