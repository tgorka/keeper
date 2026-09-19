---
status: done
baseline_revision: 16f7912
final_revision: '6745464f6bd9b423e991215fed39acd65bbc32e0'
---

# Story 75.1 — A lock freezes the size you chose

<intent-contract>

## Problem

The owner resizes the capture window, presses the padlock, and the window jumps back to the size it had before. The triage ruled out the obvious cause: the lock command reads the live geometry and **persists the new size** (`notes_ipc.rs:5419-5421`) before it re-applies anything, and the row still holds it afterwards — unlocking brings it back.

The snap comes from one arm of one pure function:

```rust
let wanted = match (self.locked, self.size) {
    (true, _) => CAPTURE_DEFAULT_SIZE,      // capture.rs:532
    (false, Some(size)) => size,
    (false, None) => return None,
};
```

`(true, _)` — whatever the user just did. The rule it encodes is "a locked window is keeper's to place and size", recorded in three places and pinned by a test and by mutation M9. It is a defensible reading of *lock* and it is not the one a person pressing a padlock has: they mean **stay exactly as you are**.

## Approach

Change the arm, not the mechanism.

## Always

Locked with a remembered size answers that size. The clamp to the work area still applies — a remembered size from a larger monitor must not put the window off this one.

## Block If

Nothing new is refused.

## Never

Never touch the `(false, None)` arm: "never resized" is not "resized to 560×340", and `window_size`'s own doc says why — the live window may have been dragged seconds ago by a user whose blur has not yet been written, and re-asserting a size would undo the gesture in front of them. Never change what the lock does to **position**: a locked window still cannot be dragged.

## I/O and edge-case matrix

| Input | Answer |
| --- | --- |
| locked, size `Some((900, 600))` | `Some((900, 600))`, clamped |
| locked, size `None` | `Some(CAPTURE_DEFAULT_SIZE)` — a window that was never resized still gets keeper's size when it is locked; there is nothing else to honour |
| unlocked, size `Some(..)` | unchanged |
| unlocked, size `None` | `None` — unchanged, and the caller touches nothing |
| locked, remembered size larger than the work area | clamped, as before |

</intent-contract>

## Code Map

- `keeper-core/src/capture.rs:530-538` — `window_size`, the one arm; `:1556` — `a_locked_window_is_normalised_and_an_unsized_one_is_left_alone`, the test that pins today's rule and must state the new one.
- `crates/keeper/src/notes_ipc.rs:5392-5398` — the shell comment that repeats the old rule (shell crate: macOS CI).
- `src/components/capture/capture-window.tsx:71-75` and `CAPTURE_LOCK_LABEL` — the words the button uses; they say *position* today, which is now also the whole truth.
- `spec-46-15`'s edge-case table line 148 — the recorded decision this rescinds.

## Tasks & Acceptance

Acceptance, verbatim from the epic: *a `Placement` that is locked with a remembered size answers that size; locked with no remembered size still answers `None`… the clamp to the work area still applies to both; the flipped test fails if the arm is reverted (mutation-proved). The end-to-end sequence — resize, lock, and the window stays — is a macOS-gate step.*

Note the matrix above refines one line of that sentence: locked-and-never-resized answers `CAPTURE_DEFAULT_SIZE`, not `None`, because a locked window must have a size to be locked at; the `None` case the epic means is the **unlocked** one, which is the arm that must not move.
