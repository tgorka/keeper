# spec-48-4 — always on top, beside the lock

Branch `work/epic-48`, worktree `quick-donkey`. Sentinel `MUT48-4`.

The report, in full: *"chcialbym ikonke obok lock zeby wylaczyc/wlaczyc zawsze
on top"* — "I'd like an icon beside the lock to turn always-on-top off and on."

Owner of `src/components/capture/capture-window.tsx` and its test,
`src/lib/stores/capture-windows.ts` (shared with E48Capture, who released it in
writing), the `Placement`/`CaptureWindowVm` additions in
`keeper-core/src/capture.rs`, and one new command in `keeper/src/notes_ipc.rs`.
Plus, by clearance obtained on the hub during the wave: `notes_window.rs`'s
`list` and `open`, the two handler lists in `lib.rs`, the mobile twin in
`ipc.rs`, `client.ts`, the negative-permission list in
`src/test/capture-capability.test.ts`, and the `CaptureWindowVm` fixtures in
`src/lib/stores/capture-windows.test.ts`.

**Most of the shell half never compiles on Linux.** §7 is not boilerplate.

---

## 1. What was true before this story

Always-on-top was **hard-coded `true` in both birth sites**, and there was no
toggle, no setting, and no view-model field anywhere:

* `src-tauri/crates/keeper/tauri.conf.json:40` — `"alwaysOnTop": true`, the
  statically-declared prewarmed draft window.
* `src-tauri/crates/keeper/src/notes_window.rs:258` — `.always_on_top(true)` on
  the builder, i.e. every note window `open` creates.

`set_always_on_top` appeared nowhere in the crate.

---

## 2. The default is `true`, and that is the whole backward-compatibility story

The one decision in this story that could quietly hurt someone.

A flag added to a persisted record needs a value for the rows that predate it.
`false` is the tidier-looking default and it is **wrong**: every capture window
on every machine is always-on-top today, so an absent tag decoding to `false`
would silently un-pin every one of them at upgrade — a behaviour change nobody
asked for, delivered by a story whose entire content is *adding a toggle*.

So: **absent tag ⇒ `true`**, exactly as an absent row ⇒ `locked`. This is
asserted from the outside against literal pre-48.4 rows
(`a_row_from_before_the_toggle_still_means_on_top`) rather than against
`Placement::default()`, because comparing decode to the default keeps passing
if someone flips both at once — which is precisely the change the test exists
to stop.

The same rule covers unreadable values: `top`, `top off`, `top false` and
`ontop 0` all answer `true`. A row keeper half-understands must cost the user
their geometry, never their pinning.

---

## 3. The encoding, and the latent bug that had to be fixed to add a second tag

46.15's `SIZE_TAG` is the settled pattern and this follows it verbatim:
`const TOP_TAG: &str = "top"`, appended, decoded by peeking for the tag, total,
backward-compatible.

**The tag is written ONLY when the flag is off.** `Placement::default()`
still encodes to exactly `"locked"` and an unlocked-and-moved window still
encodes to exactly `"free 120 -40"` — every row keeper has ever written keeps
its byte-exact spelling, and the four encode assertions in
`the_flag_round_trips_beside_a_position_and_a_size` pin that.

### The bug a second tag could not have been appended over

`decode` could not host a second tag as written, and this is the finding of the
story rather than a detail:

```rust
let size = match (parts.next(), parts.next(), parts.next()) {
    (Some(tag), Some(width), Some(height)) if tag == SIZE_TAG => { … }
    _ => None,
};
```

Three **unconditional** `next()` calls. On a row with no size but a later tag —
`free 1 2 top 0`, a window moved but never resized, which is an ordinary thing —
the size branch eats `top` and `0` looking for a size it never finds. **Both**
facts are lost: the flag, and (on the sibling path) the position, because the
position peek tested only `*word != SIZE_TAG` and would consume any new tag word
as an x-coordinate.

Two behaviour-preserving repairs, and they are the reason the diff in that
function is bigger than one line:

* the position peek now tests `!is_tag(word)` — every tag, not just the size's;
* each tagged group **peeks for its own tag before consuming anything**
  (`parts.next_if(|word| *word == SIZE_TAG)`).

Behaviour-identical on every row that already existed: the truncated
(`free 1 2 size 560`), half-readable (`size 560 tall`), zero (`size 0 340`),
junk-tag (`dimensions 560 340`) and trailing-junk (`size 560 340 1 2`) cases all
still degrade exactly as they did, and their existing tests were not modified.

Mutant 3 in §6 is this repair: reverting it to the old triple-consume kills
three named tests.

---

## 4. The command, and why it is a Rust command

`notes_capture_set_always_on_top(key, alwaysOnTop)`, modelled line-for-line on
`notes_capture_set_locked`: read the stored placement, write the one field back,
apply it to the live window so the toggle takes effect without a reopen.

**No capability was added, and a webview-side call could not have worked.**
`quick-capture.json` grants `core:default`, `allow-hide`, `allow-close`,
`allow-set-focus`, `allow-start-dragging`, `dialog:allow-open` and
`opener:default` — and nothing else. `getCurrentWindow().setAlwaysOnTop()` would
be denied, and denied *quietly*, as a rejected promise inside a click handler
nobody awaits. That is 46.15's argument for `set_resizable` verbatim.

This story makes that decision **enforced rather than merely stated**:
`core:window:allow-set-always-on-top` joins the negative list in
`capture-capability.test.ts`'s "still refuses everything a capture window has no
business doing". The file's own comment two tests further down explains why —
"a contract stated in a comment and enforced nowhere" is the shape that has
already cost this epic an afternoon.

**The geometry is deliberately not touched.** Unlike the lock, pinning changes
nothing about where or how big the window is, so the command must not snapshot
or re-assert either. Reading the live geometry here would re-introduce story
48.2's defect on a brand-new path: the live size is whatever it is at this
instant, and merging it over the stored one is exactly how a remembered size
gets overwritten by a normalised one.

---

## 5. Where the flag is read, and why that source and not the other

`CaptureWindowVm.always_on_top` (wire `alwaysOnTop`) is filled in
`notes_window::list` from **the live window**, falling back to the stored
placement:

```rust
always_on_top: window.is_always_on_top().unwrap_or(stored.always_on_top),
```

I had originally written this as the stored value, justified with "tao offers no
`is_always_on_top`". **That justification was false** — `WebviewWindow::
is_always_on_top` exists in tauri 2.11.5 (`src/webview/webview_window.rs:1768`),
which is the version vendored here, and I only found out by going and reading
it. The comment is corrected and so is the code.

Live is the right source: `set_always_on_top` is a *request* the window manager
may decline — most tiling ones do — so the stored flag is the user's intent and
the live window is the only thing that knows whether it took. Reporting intent
would leave the button pressed above a window that is plainly not on top, which
is the button lying rather than the compositor refusing. `is_visible` and
`chrome_inset` beside it read the live window for the same reason; `locked`
stays stored because it is keeper's own policy and no compositor has a view on
it.

`list`'s callback widened from `&dyn Fn(&str) -> bool` to
`&dyn Fn(&str) -> Placement`. It answered `bool` for the lock alone; a second
persisted flag would have made it two closures and the third three, each doing
its own settings read for one field of one row.

### Per-window, not global

The flag lives beside the size, under the same key, because it is a property of
*this* window in the same sense the size is. 48.3 makes two simultaneous capture
windows reachable, and a person who pins a note beside what they are reading has
said nothing about the next window they open.

### DW-199 is not regressed

tao's undecorated-window hit-test is gated on
`!is_decorated() && is_resizable() && !is_maximized()` — always-on-top is not in
that condition, so un-pinning a window changes neither the resize border nor
`chrome_edge_inset`'s answer. The new button is added to the **left** of the
lock, leaving the close button flush in the top-right corner where 47.5's
`paddingTop`/`paddingRight` inset protects it; the corner geometry is byte-for-
byte what 47.5 left.

---

## 6. The I/O matrix

### `Placement::decode` — the flag

| Persisted row | `always_on_top` | Why |
| --- | --- | --- |
| `` (absent row) | `true` | default; what every window already is |
| `locked` | `true` | pre-48.4 spelling, untouched |
| `free 120 -40` | `true` | pre-48.4 spelling, untouched |
| `free size 900 600` | `true` | 46.15 spelling, untouched |
| `free 120 -40 top 0` | `false` | moved, never resized, un-pinned |
| `free size 900 600 top 0` | `false` | resized, never moved, un-pinned |
| `locked -15 900 size 1280 800 top 0` | `false` | everything at once |
| `free 1 2 top 1` | `true` | explicit on; `encode` never writes it, a hand-edit may |
| `free 1 2 top` | `true` | truncated ⇒ default |
| `free 1 2 top off` / `top false` | `true` | unreadable value ⇒ default |
| `free 1 2 ontop 0` | `true` (and position `(1,2)` kept) | unknown tag is not the tag |
| `banana` / `written by a later build` | `true` | unreadable row ⇒ default |

### `Placement::encode`

| Placement | Encoded |
| --- | --- |
| default | `locked` |
| unlocked, moved, on top | `free 120 -40` |
| unlocked, moved, **not** on top | `free 120 -40 top 0` |
| unlocked, moved, sized, not on top | `free 120 -40 size 900 600 top 0` |

### The button

| Row says | Accessible name | `aria-pressed` | Icon | Click sends |
| --- | --- | --- | --- | --- |
| `alwaysOnTop: true` | "Stop this window floating above other apps" | `true` | `lucide-pin` | `(key, false)` |
| `alwaysOnTop: false` | "Keep this window floating above other apps" | `false` | `lucide-pin-off` | `(key, true)` |
| no row yet | "Stop this window floating…" | `true` | `lucide-pin` | `(key, false)` |

## 7. Mutation table

Baseline established **before** each sweep, in the same command and the same
filter the sweep used, and re-verified **between** every mutant rather than only
at the end. Rust baseline `cargo test -p keeper-core --lib capture::` = 45
passed / 0 failed / EXIT=0 (taken after E48Lock's 48.2 tests had landed).
Frontend baseline `bun run vitest run src/components/capture/` = 52 passed /
EXIT=0.

| # | Mutation | Result |
| --- | --- | --- |
| 1 | `decode`'s absent-tag arm `true` → `false` | **killed** — 6 tests, incl. `a_row_from_before_the_toggle_still_means_on_top`, `a_placement_written_before_this_story_still_reads_as_itself` |
| 2 | `decode`'s unreadable-value fallback `!matches!(…, Some("0"))` → `matches!(…, Some("1"))` | **killed** — `an_unreadable_flag_leaves_the_window_where_it_was` |
| 3 | per-tag peek reverted to the pre-48.4 three-`next()` triple | **killed** — 3 tests, incl. `reading_the_flag_costs_the_row_none_of_its_other_facts` |
| 4 | `Default::always_on_top` `true` → `false` | **killed** — 4 tests |
| A | VM field renamed in Rust (`always_on_top` → `always_on_top_x`) | **no verdict** — see below |
| A2 | VM **wire name** renamed (`#[serde(rename = "alwaysOnTopX")]`) | **killed** — `bun run typecheck` EXIT=2, 6 errors across `capture-window.tsx`, `capture-window.test.tsx`, `capture-windows.test.ts` |
| B | toggle sends the CURRENT value instead of the next | **killed** — `pins the window it belongs to, with the value it is toggling to` |
| C | unknown row reads as NOT on top | **killed** — `behaves as on-top before Rust has answered` |
| D | the two accessible names swapped | **killed** — 4 tests |
| E | the two **icons** swapped | **SURVIVED**, then killed — see below |
| F | pin button moved after close, taking DW-199's corner | **killed** — `keeps the close button last, so DW-199's corner inset still protects it` |
| G | desktop registration removed from `lib.rs` | **SURVIVED** — see below |
| G2 | **both** registrations removed | **killed** — `are each registered on the builder` |

### Mutant A produced no verdict, and that is worth reading

Renaming the Rust field outright does not reach the frontend at all: the rename
breaks `capture.rs`'s own test literals, `export_bindings` never runs, and the
`.ts` file is never regenerated. The typecheck stays green because it is reading
a **stale binding**. So the mutation that looks like the seam test is actually a
test of the Rust compiler. A2 — renaming only the wire name, which compiles —
is the real one, and it kills.

### Mutant E survived, and the fix is in this story

Swapping `<Pin>` and `<PinOff>` changed nothing any test could see. Every
assertion was on the accessible name, and the icons are `aria-hidden` — so a
pinned window showing an "un-pin" glyph would have shipped green. The report
asked for an **ikonke**; the icon is the whole of what a sighted user reads on
this button. `names the always-on-top state it moves to` now asserts
`classList.contains("lucide-pin")` / `("lucide-pin-off")` — `contains` and not a
substring match, because `"lucide-pin-off"` contains `"lucide-pin"` — and E
kills.

### Mutant G survived, and the fix is NOT in this story

`command-registration.test.ts` extracts `path::name` with a regex that captures
only `name`, so `ipc::notes_capture_set_always_on_top` (the iOS twin) satisfies
the check for `notes_ipc::notes_capture_set_always_on_top` (the desktop one).
Deleting the **desktop** registration of my command leaves the suite green;
deleting both fails it. The guard proves a command is registered *somewhere*,
not *on the target that implements it* — a whole feature can be dead on desktop
with a green tree, which is the exact failure class the file was written to
stop. This is pre-existing and applies to all ~200 commands, not to mine.
**Filed as deferred work rather than fixed here**: the fix is a change to a
shared guard that every story on this branch depends on, and landing it inside
a story about a pin button is how a one-line story becomes a merge conflict for
four other agents.

## 8. Deliberately NOT done

* **No capability was added**, and its absence is now asserted (§4).
* **`tauri.conf.json:40` still says `"alwaysOnTop": true`.** With the default
  `true` that line is now *correct* rather than hard-coded — it is the
  prewarmed draft's birth state, and `open` re-asserts the stored flag on it
  every time it shows. Setting it `false` would give an un-pinned draft one
  frame on top of everything, and a pinned one a visible flicker.
* **No global "always on top" setting.** The flag is per window, beside the
  size, because 48.3 makes two capture windows reachable and pinning one says
  nothing about the next.
* **No main-window mirror.** E48Capture established that nothing outside a
  capture window's own document ever hydrates `captureWindowsStore`. The toggle
  lives in the chrome strip, in the same document as the effect that hydrates,
  so it needs none — this is not an omission, and adding the hydration would
  have collided with 48.3's own deferred item.
* **No `is_always_on_top` probe in the webview.** Rust reads it; the webview is
  handed a boolean, exactly as 47.5 hands it a number.

## 9. What I could not verify here, and why

**`cargo build/check/clippy/test -p keeper` never ran, because the shell crate
does not build on Linux (AD-55, AD-56).** Everything in `notes_window.rs`,
`notes_ipc.rs`, `ipc.rs` and `lib.rs` below is **unbuilt**. What I did instead:
read `tauri 2.11.5`'s own source to confirm
`WebviewWindow::set_always_on_top(&self, bool) -> Result<()>`
(`src/webview/webview_window.rs:2049`) and `is_always_on_top(&self) -> Result<bool>`
(`:1768`) exist and have the signatures I call; checked every changed call site
by hand against the helper it calls; and ran the two source-reading suites that
DO execute here (`capture-capability`, `command-registration` — 24 passed).

The highest-risk unbuilt change is **`list`'s callback widening from
`-> bool` to `-> Placement`**, because it is a signature change with one caller
and no compiler on this box to check it.

### Gate checks, in order, on the Mac

1. `bash scripts/check-macos.sh` — the first thing that has ever compiled this
   story's shell half. Expect it to be the step that fails if anything does.
2. `bun run bindings:check` — **`src/lib/ipc/gen/CaptureWindowVm.ts` is
   regenerated and MUST be committed in the same commit as `capture.rs`.** This
   gate fails on an uncommitted binding, not a wrong one.
3. Install, open the quick-capture panel (the global hotkey). **Three buttons in
   the strip: pin, lock, close, in that order, left to right.**
4. Hover the pin: "Stop this window floating above other apps". Click it. The
   icon becomes the struck-through pin. Raise another application over where the
   window is — **the capture window is now covered.** This is the report's ask
   and the one claim that cannot be checked on Linux.
5. Click again → "Keep this window floating…", the window returns above the
   other application. No reopen, no restart.
6. Un-pin, then **quit and relaunch keeper**. Open the panel: still un-pinned.
   (This is `top 0` surviving in the settings table.)
7. **The upgrade check, and the one I would run first if I could run only one.**
   With a placement row written by 0.8.1 — any existing install — open a capture
   window and confirm it is **on top**, i.e. the absent tag still means pinned.
8. Un-pin a window, then **resize it, then lock it, then unlock it**. The pin
   state must survive all three, and the size must survive the lock (48.2's
   fix). The two stories touch one row and this is where they would interfere.
9. Open a **second** capture window on a different note (48.3). Pin one, un-pin
   the other, confirm they disagree and keep disagreeing across a restart.
10. **DW-199 regression check, GTK only.** Unlock a window so its resize border
    is live, then click the close button in the very corner. It must close and
    not start a resize — with a third button now in the strip.
11. On a tiling WM (or GNOME with an always-on-top extension), un-pin and
    confirm the button reports what the compositor did rather than what was
    asked: if the request is refused, the button stays pressed. This is the one
    behaviour I chose deliberately over the alternative and cannot demonstrate.
