# Spec 46.15 — A Capture Window You Can Resize

status: implemented
story: Epic 46, Story 46.15
bindings: the owner's report — *"I want to be able to resize the quick-capture window when it is
unlocked"*; FR-192, UX-DR77, UX-DR43, NFR-27, AD-55/AD-56, AD-104
crates: `keeper-core` (`capture.rs`, `registry.rs`) — compiles and tests here;
`keeper` (`notes_window.rs`, `notes_ipc.rs`, `lib.rs`, `tauri.conf.json`) — **never compiled here**
frontend: `src/test/capture-capability.test.ts` (new size pins),
`src/components/capture/capture-window.tsx` (the lock's accessible name),
`src/components/notes/note-editor.tsx` (one comment, three lines, naming a constant that no longer
exists)
ledger: DW-198, DW-199

---

## What the lock actually was, and what it is now

`Placement { locked, position }` carried no size, and `locked` reached the window through exactly
one channel: the webview toggled `data-tauri-drag-region` on the chrome strip. Nothing in Rust ever
called `set_resizable`, and `resizable` was hard-`false` in the two places a capture window is
born — `tauri.conf.json:40` for the prewarmed one, `notes_window.rs:206` for every other. So
"unlocked" meant **movable and nothing else**, and the word on the button ("so it can be moved")
was the honest description of a one-verb lock.

There was a third obstacle behind those two, and it is the one that would have made a naive
`resizable(true)` look fixed and behave broken: `notes_window.rs:220-221` re-applied
`set_size(CAPTURE_SIZE)` on **every** `open()`, unconditionally. A user's size would have survived
until the next time they opened that window, and then vanished — the worst shape of bug, because
the gesture works and the loss arrives later, detached from its cause.

The lock now has two verbs. Unlocked is movable **and** resizable; locked is neither, and a locked
window is normalised back to keeper's own 560×340. The size is remembered under the same settings
key as the position, with the same "locking is not a discard button" rule.

## Was this entangled with epic 45's Wayland finding? No — verified, not assumed.

The story said to check this rather than take it, so it was checked from tao's source
(0.35.3, the version in `Cargo.lock`), per platform, before anything was written.

UX-DR43's finding is specifically that **`set_position` is the call a Wayland compositor refuses**:
a request to put a surface at a coordinate is the compositor's business, not the client's.
`set_resizable` is a different kind of thing — a window *attribute* — and the relevant question is
not whether the compositor honours it but whether tao's own edge hit-testing for an **undecorated**
window reads that attribute **live** or captures it when the window is built. If it captured it,
`set_resizable(true)` after creation would set a flag nothing consults, and this whole story would
ship a lock that toggles a boolean into a void.

It reads it live, on all three:

| platform | verdict | why |
| --- | --- | --- |
| macOS | yes | a borderless window is created *with* `NSWindowStyleMask::Resizable`; `set_resizable` ORs it back onto the live styleMask (`platform_impl/macos/window.rs:216-232, 795-811`) |
| Windows | yes | `WS_SIZEBOX` follows `WindowFlags::RESIZABLE` and is not stripped for undecorated windows; `WM_NCHITTEST` synthesises resize borders reading the flags per message (`windows/event_loop.rs:2178-2229`) |
| Linux/GTK | yes | the motion/button/touch handlers are wired unconditionally at creation and re-query `!window.is_decorated() && window.is_resizable()` on **every event**; `set_resizable` routes to `gtk_window_set_resizable`, the same property (`linux/window.rs:236-244, 640-642`, `linux/event_loop.rs:316`). No `Rc<AtomicBool>`/`Cell` snapshot exists |
| X11 vs Wayland | same for the undecorated case | `begin_resize_drag` → `xdg_toplevel.resize` / `_NET_WM_MOVERESIZE`; Wayland additionally hit-tests decorated windows, which does not apply here |
| tauri 2.11 | pass-through | `WebviewWindow::set_resizable` → `WindowMessage::SetResizable` → tao; on Windows it also re-attaches tauri's own undecorated-resize handler, which helps |

**No frontend `startResizeDragging` is needed, and no capability permission is added.** Resizing is
a native edge drag against an attribute Rust sets — it is not a plugin command the webview invokes,
so `capabilities/quick-capture.json` is byte-for-byte unchanged. That is stated in the module doc
so the next reader does not go looking for a fourth grant.

One platform caveat came out of the same reading and is recorded as **DW-199**: on GTK the resize
strip is 5 px × scale *inside* the surface and swallows webview clicks, which lands on the chrome's
close button in the top-right corner. It is Linux-only, it needs a real window to measure, and
guessing an inset is worse than naming the hazard.

## Where each decision lives, and why

Everything that can be wrong went into `keeper-core`, where it runs on Linux. The shell converts
units and makes calls.

| decision | lives in | testable here? |
| --- | --- | --- |
| what a persisted placement means | `Placement::encode`/`decode` | yes |
| whether a size is readable at all | `Placement::decode` | yes |
| locked → normalise; unlocked+size → restore; unlocked+no size → **touch nothing** | `Placement::window_size` | yes |
| the clamp (screen ceiling, usability floor, who wins) | `clamp_size` | yes |
| the two numbers | `CAPTURE_DEFAULT_SIZE`, `CAPTURE_MIN_SIZE` | yes, and pinned to `tauri.conf.json` from TypeScript |
| calling `set_resizable`/`set_size`, and converting physical↔logical | `notes_window.rs` | **no** |

`window_size` returning `Option` is the part that is easy to get wrong and the reason the third row
exists as its own case. "Unlocked and never resized" is **not** "unlocked at the default size": the
live window may hold a size the user chose seconds ago that no blur has written down yet, and
re-asserting a default on the next open would undo the gesture in front of them. `None` means *do
not call `set_size`*.

## The encoding, and the row this story did not write

`locked` / `free`, optionally two integers, optionally `size` and two more:

| placement | persisted |
| --- | --- |
| default | `locked` |
| dragged, unlocked | `free 120 -40` |
| resized but never moved | `free size 900 600` |
| both | `free 120 -40 size 900 600` |
| resized, then locked | `locked -15 900 size 1280 800` |

Tagged rather than positional, for one reason that is not tidiness: **a size must be storable
without a position** — a window resized but never moved is ordinary — and three optional trailing
integers cannot say which pair is which. The tag also makes the pre-46.15 spelling read verbatim:
`free 120 -40` still decodes to exactly the placement it always did, with no size, and
`a_placement_written_before_this_story_still_reads_as_itself` asserts that specifically. Every
capture window on every machine that has ever been unlocked already has such a row.

Units are asymmetric on purpose and it is written down in the type: **position physical, size
logical**. A position is a point on a desktop that may span monitors of different scale factors and
only the physical coordinate names one unambiguously. A size is a statement about how much content
fits — restore a physical 1120 on a monitor that went from 2× to 1× and the person gets a window
twice the size they left. `logical_work_area` exists so the clamp cannot mix them: clamping a
logical 900 against a physical 2880 would silently permit a window twice as wide as the screen.

## I/O matrix

`Placement::decode` → `window_size(work_area)`. `work_area` is logical; `None` is a headless session
or a compositor that will not name a monitor.

| stored row | decode | `window_size(Some((1920,1080)))` | what the shell does |
| --- | --- | --- | --- |
| *(absent)* | `{locked, -, -}` | `Some((560,340))` | normalise, not resizable |
| `locked` | `{locked, -, -}` | `Some((560,340))` | same |
| `free` | `{free, -, -}` | `None` | resizable; **size untouched** |
| `free 120 -40` | `{free, (120,-40), -}` | `None` | resizable, positioned; size untouched |
| `free size 900 600` | `{free, -, (900,600)}` | `Some((900,600))` | resizable, sized, auto-placed |
| `locked 10 10 size 900 600` | `{locked, (10,10), (900,600)}` | `Some((560,340))` | **normalised to 560×340**, kept at (10,10) |
| `free size 3000 2000` on a 1440×900 screen | `{free, -, (3000,2000)}` | `Some((1440,900))` | clamped to the display |
| `free size 1 1` | `{free, -, (1,1)}` | `Some((320,240))` | raised to the floor |
| `free size 0 340` | `{free, -, **-**}` | `None` | size refused; window untouched |
| `free 1 2 size 560 tall` | `{free, (1,2), **-**}` | `None` | half-readable size is no size |
| `written by a later build` | `Placement::default()` | `Some((560,340))` | the whole row degrades |

## Edge cases

| case | behaviour |
| --- | --- |
| a size of `0` on either axis | refused at decode. It parses, and it describes a window that cannot be seen, focused or closed |
| a negative or `> u32::MAX` size | fails `parse::<u32>()` before the zero check; no size |
| half-readable size (`size 560 tall`) | no size, same rule the position has had since 45.15 — never a fabricated axis |
| a readable size followed by junk | **honoured.** `free size 560 340 1 2` is a readable size; the two-token spelling already ignored trailing words, and refusing would discard what the row plainly says |
| `size` where a position was expected | the position scan peeks for the tag first, so `free size 900 600` is not read as coordinates |
| remembered 3000 px wide on a 1440 px display | clamped. The close button would otherwise be past the far edge of a window in no dock and no task switcher — nothing left to click |
| the monitor it was sized on has gone away | same path; the clamp is against whatever display it is restored on |
| the display is *smaller* than the floor | **the display wins.** Floor first, then ceiling. Reachable beats comfortable |
| a work area reported as `0` | ignored, not clamped to — that is a monitor mid-reconfiguration, and clamping to it builds the invisible window `decode` refuses to build |
| no monitor at all (headless) | floor applies, no ceiling; nothing is invented |
| lock pressed after a resize | window snaps to 560×340 **keeping its position**, and the size is kept in the row. Deliberate: the alternative is the same surprise delivered on the next open, unattached to the click that caused it. Unlocking restores it |
| unlock pressed | `set_resizable(true)` on the live window — no reopen needed |
| the prewarmed window after a restart | `lib.rs` setup adopts the stored placement once, off the hotkey path |
| the hotkey raises the draft window | position only. Resizability and size were already set at boot; `reveal(None)` no longer re-asserts a default over them |
| creating a second capture window while unlocked | built `resizable(!locked)` from the first frame, not built `false` and flipped |
| the user drags the window smaller than the floor | the compositor refuses it — `minWidth`/`minHeight` and `.min_inner_size` — so the clamp never has to argue with a size that already happened |
| AD-104's header at a narrow width | see below |

## AD-104: does a resizable window reintroduce what 46.4 fixed?

No, and this was checked against `spec-46-4-*.md` rather than assumed.

46.4's fix is **structural, not width-dependent**: three groups, where identity is
`min-w-0 flex-1` (so it contributes nothing to the row's content width and gives all the ground),
status is `shrink-0` with a box measured from `SAVE_CAPTION_SIZERS`, and actions are last. A save
cannot move the toolbar at *any* width, because the caption's box is a constant and the actions
therefore start at a constant offset. Its own edge-case table already lists **"very narrow window
(below 560, restorable via `notes.capture_placement`)"** and answers it: group 1 → 0, group 2 holds
its box, group 3 absorbs the squeeze. This story makes that row of the table reachable by a gesture
instead of by hand-editing a settings row; it does not create the case.

What this story adds is a floor where there was none. `CAPTURE_MIN_SIZE = (320, 240)` is derived
from what the header has to hold, not chosen for looks: status reserves ≈100 px for
`Saved · HH:MM` in a 12-hour locale, actions carry an icon button plus a word-labelled menu
(≈112 px after 46.5), and the row's padding and gaps are ≈30 px — so below roughly 250 px the
actions begin leaving the right-hand edge, which *is* 46.5's defect. 320 keeps a margin over that
and still leaves the title something to truncate into. Without the story, a person could already
drive the window below that number by hand; with it, two mechanisms refuse.

## Mutation table

Sentinel `MUT46-15`, applied one at a time to `capture.rs`, `cargo test -p keeper-core --lib
capture::` between each, restored from bytes captured before the sweep and **verified by sha256
after every single mutant** — not from memory. **9 applied, 9 caught, 0 survived.** Final digest
`c1d2835d…09501`, identical to pristine; `grep -rn MUT46-15 src-tauri/` returns nothing.

| # | mutation | caught by |
| --- | --- | --- |
| M1 | `decode`: drop the `width > 0 && height > 0` guard — a zero is a size | `an_unreadable_size_costs_the_size_and_never_the_window` |
| M2 | `decode`: an unreadable size falls back to `CAPTURE_DEFAULT_SIZE` instead of `None` | same |
| M3 | `decode`: drop the `peek != SIZE_TAG` guard, so the position scan eats the tag | `a_placement_round_trips_…` + `an_unreadable_size_…` |
| M4 | `clamp_size`: no ceiling — restore 3000 px on a 1440 px screen | `a_remembered_size_is_cut_down_to_the_screen_…`, `a_display_smaller_than_the_floor_…`, `an_unknown_display_…` |
| M5 | `clamp_size`: no floor | `a_remembered_size_is_never_smaller_than_the_window_can_hold`, `an_unknown_display_…` |
| M6 | `clamp_size`: floor applied **after** the ceiling, so the floor beats the screen | `a_display_smaller_than_the_floor_still_gets_a_window_it_can_show` |
| M7 | `clamp_size`: a `0` work area treated as a measurement | `an_unknown_display_clamps_what_it_can_and_invents_nothing` |
| M8 | `window_size`: unlocked + never resized returns the default instead of `None` | `a_locked_window_is_normalised_and_an_unsized_one_is_left_alone`, `an_unreadable_size_…` |
| M9 | `window_size`: a locked window keeps the remembered size | `a_locked_window_is_normalised_and_an_unsized_one_is_left_alone` |

M4/M5/M6 together are the clamp proof the story asked for: the ceiling, the floor, and the order
between them are each independently defended. M1/M2/M3 are the decode-degradation proof.

## Deliberately NOT done

- **`tauri.conf.json`'s `resizable` stays `false`.** The prewarmed window is created before anything
  has read a setting, so it must boot in the state a person who never touched the lock expects.
  `lib.rs`'s setup flips it immediately afterwards from the stored placement. A test pins the
  `false` so a later "tidy-up" cannot flip it and hand a resizable window to someone who never
  asked for one.
- **No settings read on the hotkey path.** `notes_window::show` still takes no placement.
  NFR-27's 300 ms is `set_position` → `show` → `set_focus`, and a sqlite read in front of it would
  be paid on every press to answer a question that only changes when the user presses the lock.
  Adopting the placement once at boot costs that path nothing.
- **The hotkey path's re-centring is not fixed** — pre-existing, out of scope, and now more visible
  because the size survives what the position does not. **DW-198**, with the shape of the fix and
  the trade it costs.
- **No `Resized` event handler.** Blur is when geometry is written down, exactly as 45.15 chose for
  the position: a drag emits one event per compositor frame and a settings write per frame would
  put a sqlite transaction inside a gesture.
- **No per-window size override for the *locked* case.** A locked window is keeper's size, full
  stop; "locked at a size I chose" would be a third state with no control to express it.
- **No frontend resize affordance** (grip, `startResizeDragging`, cursor). Every backend already
  hit-tests the edges of an undecorated resizable window. The GTK inset question is DW-199.
- **No capability change.** Resizing invokes no plugin permission.
- **`sprint-status.yaml` not flipped** — several agents share it this wave; the ledger is Main's.

## What I could not verify here, and why

**The `keeper` shell crate does not build on Linux** (no GTK/webkit), so every line in
`notes_window.rs`, `notes_ipc.rs`, `lib.rs` and `tauri.conf.json` is unexecuted and untypechecked
by anything I ran. What I *did* do to bound that:

- `rustfmt --edition 2021 --check` parses each edited shell file — it **parses**, which rules out
  syntax errors but says nothing about types. All four are format-clean; the only diffs rustfmt
  reports in that module tree belong to `ipc.rs` and `sync_ipc.rs`, which are other agents'
  in-flight edits.
- Every tauri API used was checked against the 2.11.5 rustdoc for its real signature —
  `Monitor::work_area() -> &PhysicalRect<i32, u32>`, `Monitor::scale_factor() -> f64`,
  `PhysicalSize::to_logical::<u32>`, `WebviewWindow::set_resizable`,
  `WebviewWindowBuilder::min_inner_size`.
- The platform behaviour was read out of tao 0.35.3's source, not inferred from documentation.
- `src/test/capture-capability.test.ts` pins the three shell facts that a Linux machine *can*
  check: the two sizes against `capture.rs`'s constants, the `resizable: false`, and the presence of
  `set_resizable(!placement.locked)` and `.min_inner_size(CAPTURE_MIN_SIZE.0.into()`. Those are text
  assertions on source, which is exactly as strong as they sound — they catch a deletion or a
  drifted number, not a type error.

Specifically **not** proven by anything here, and needing the macOS host and eyes:

1. that the shell crate compiles at all;
2. that an unlocked, undecorated capture window is actually edge-draggable on the real machine
   (the tao reading says yes on all three; a reading is not a window);
3. that the remembered size comes back after a real quit-and-relaunch;
4. that `to_logical`/`set_size` round-trips cleanly on a HiDPI display — the unit asymmetry is
   argued and typed, never executed;
5. that the header holds at 320 px (46.4's structure says it must; 46.4 could not measure layout
   either, and said so).

### Ordered gate checks

Run on the macOS host, in the built app.

1. **`cargo test -p keeper-core --lib capture::`** → EXIT=0. *(Done here: 22 passed, and again
   after the mutation sweep restored the file.)*
2. **`cargo test -p keeper-core --lib registry::tests::two_capture_windows_remember_two_placements_independently`**
   → EXIT=0. *(Done here: 1 passed.)*
3. **`bun run test src/components/capture/ src/test/capture-capability.test.ts`** → EXIT=0.
   *(Done here: 54 passed.)*
4. **`cargo build -p keeper`** → the first thing that has ever compiled any of this. If it fails,
   it fails in `notes_window.rs`'s geometry helpers or in the `Geometry` threading through
   `notes_ipc.rs`/`lib.rs`; nothing else moved.
5. **`cargo clippy -p keeper --all-targets`** → clean. The workspace runs `clippy::all` +
   `unwrap_used`; there are no unwraps in the new code.
6. **`cargo test -p keeper --lib notes_window::`** → the four pre-existing geometry tests, unmoved.
7. Launch. Press the capture hotkey. The panel appears as it always has, **locked, 560×340, not
   resizable** — drag its edges and nothing happens. This is the no-regression check and it must
   pass before any of the rest means anything.
8. Press the padlock. Its name is now *"Unlock this window so it can be moved and resized"*. Drag
   an edge: **the window resizes.** Drag the strip: it still moves. (Linux only: also click the
   top-right two pixels of the close button — DW-199.)
9. Type in the resized window, wait for the autosave, and watch the `⋯`. **Nothing may move**, and
   nothing may leave the right-hand edge — 46.4's gate, at a width 46.4 could not reach.
10. Drag the window as small as it will go. It must stop at **320×240** and refuse to go further.
11. Press the padlock again to lock. The window **snaps back to 560×340 and keeps its position**.
    Unlock: the size you chose comes back.
12. With the window unlocked and resized, dismiss it (Escape) and press the hotkey again. **Same
    size.** Then quit keeper entirely, relaunch, and press the hotkey: **still the same size**, and
    the padlock still shows unlocked. This is the acceptance the whole `Placement::size` half
    exists for.
13. Open a note as a capture window (not the draft). Repeat 8, 11 and 12 for it — a different key,
    a different row, and it must not move or resize the draft window.
14. Resize a window very wide, quit, unplug the external display, relaunch and raise it. It must
    come back **fitting the remaining screen**, with its close button reachable.
15. `sqlite3 <data>/keeper.db "select key, value from settings where key like
    'notes.capture_placement.%'"` → rows read like `free 120 -40 size 900 600`. Hand-edit one to
    `free 1 2 size 0 600`, relaunch: the window comes back at 560×340 at (1,2), not invisible.
