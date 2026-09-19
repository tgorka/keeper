---
status: in-progress
baseline_revision: 16f7912
final_revision: ''
---

# Story 75.2 — The lock is off when it opens, and nothing floats until asked

<intent-contract>

## Problem

Both flags are remembered, and both ship on:

- `locked` defaults to `true` and is stored per window in `notes.capture_placement.<key>` (`config/keys.rs:532-543`, `Scope::SessionState`), read back at boot (`lib.rs:517-548`) and at every open. Nothing resets it.
- `top` rides in the same row, also defaults `true`, and the draft window is born `alwaysOnTop: true` (`tauri.conf.json:31-45`).

So a window that was locked once opens locked forever, and a fresh machine gets a panel that floats over everything and cannot be moved — which, with story 75.1 unfixed, also resized itself. The owner asked for both off.

## Approach

Two different answers, because they are two different kinds of thing.

## Always

**The lock is off at every open.** It is a gesture for the capture in front of you. The reset is exactly one field: position and size live in the same row and must survive untouched.

**Floating is off by default and stays off until asked.** `Placement::default()` and the draft window's birth state both become `false` — but a pin the owner sets **persists**, because it is a preference about this window, not a gesture, and throwing it away on every open would be the same disrespect in the other direction.

**The persisted spelling had to invert with the default, and that is the one-way upgrade cost.** The row carries `always_on_top` as the tag `top 1`, written **only when the flag is on**; an absent tag reads as off. This is the inverse of Story 48.4's rule, and it had to invert: 48.4 wrote the tag only when the flag was OFF, precisely so that every row written before it stayed byte-identical — which means "on" is *unrepresentable* in a pre-48.4 row, and there is no way to tell a window the owner pinned from one that was pinned because keeper pinned everything by default. Leaving the absent-tag fallback at `true` would therefore have shipped a new default that no existing machine — including the one the complaint came from — ever sees. Both spellings are still *read*: `top 0`, the row 48.4 wrote for an un-pinned window, still means un-pinned, so no row changes meaning. What does change is the pre-75.2 row with no tag at all — written when the default was `true` and an explicit pin could not be expressed — which therefore comes back un-pinned exactly once. That un-pinning is the deliverable AD-259 asks for rather than a regression of it, and it is accepted as the one-way upgrade cost: there was no persisted spelling that could both flip the default and preserve it. The argument and the back-compat tests live in `keeper-core/src/capture.rs` — the `TOP_TAG` and `Placement` docs, and the tests `a_row_from_before_the_toggle_no_longer_floats_unasked`, `reading_the_flag_costs_the_row_none_of_its_other_facts` and `an_unreadable_flag_never_floats_a_window_nobody_floated`.

## Block If

Nothing.

## Never

Never reset by writing a fresh `Placement` — that would take the position and size with it. Never reset in only one of the two paths: the hotkey **reveals a hidden window** (`hotkey.rs:186-198`) rather than building one, and boot adopts the stored row separately (`lib.rs:517-548`); which one opened the window must not change the answer.

## I/O and edge-case matrix

| Input | Result |
| --- | --- |
| stored row: locked, position `Some`, size `Some` | opens unlocked, at that position, at that size |
| stored row: locked, pinned | opens unlocked, still pinned |
| no stored row (fresh machine) | unlocked, unpinned, keeper's placement and size |
| user pins, dismisses, reopens | still pinned |
| user locks, dismisses, reopens | unlocked |

**An accepted consequence of AD-258, recorded so it is not mistaken for a defect:** since every capture window is unlocked at every open, `plan_show_position` answers "leave it where it is" for every window that can say whether it is resizable — so keeper's own pointer-following placement (DW-198: "a fifth of the way down the monitor holding the pointer") is no longer reached by a window that answers. A fresh machine gets the window centred on the monitor it was created on, and the first blur freezes that position in the stored row. That is deliberate: pointer-following was the compensation for a window nobody could move — keeper placed it because the user was not allowed to (Story 47.5) — and with no window arriving locked there is nothing left to compensate for, while a panel that jumps to the pointer's monitor on every press is exactly the "keeps its own preferences and discards yours" the epic is named after (`notes_window.rs`'s module doc argues this in full). The placement path remains reachable for a window whose backend refuses to report resizability: a window that will not answer must still be placed, exactly as it always was, and the `ShowPosition::Place` arm stays for exactly that caller.

</intent-contract>

## Code Map

- `keeper-core/src/capture.rs` — `Placement::default()` (the `top` default), and the pure function that produces the open-time placement, which is where the lock reset belongs so both shell paths inherit it.
- `keeper-core/src/registry.rs:1450-1486` — `get/set_capture_placement`, the store and read-back.
- `crates/keeper/src/notes_window.rs:241-287` (`open`), `:549-563` (`reveal`), `:616-655` (`adopt_placement`); `crates/keeper/src/lib.rs:517-548` (boot adoption); `crates/keeper/tauri.conf.json:31-45` (birth state). Shell crate: macOS CI only.
- `spec-46-15` acceptance step 12 and `spec-48-4` §2 + step 6 — the decisions this overrules, quoted in the epic.

## Tasks & Acceptance

Acceptance, verbatim from the epic: *a stored row that says locked comes back unlocked and keeps its position and size — the reset is one field, not a wipe; a fresh machine opens unpinned and unlocked; a pin the owner sets is still there on the next open; the boot adoption path and the hotkey reveal path both go through the same reset, so which one opened the window cannot change the answer.*
