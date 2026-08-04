# Epic 36 — Capture in two seconds

status: draft
created: 2026-08-02
altitude: epic
parent: Epic 35 (the vault is a folder you already sync)
source: `product-inputs-notes-2026-08-02.md` (the numbering spine), the divergent session in
`brainstorm-keeper-notes-2026-08-02/`, and a full read of `tray.rs`, `hotkey.rs`,
`lifecycle.rs`, `menu.rs`, `keeper-core/src/palette.rs` and the capability files
binds: FR-98–FR-102, FR-117, FR-120, NFR-27, AD-60, AD-61, UX-DR35, UX-DR42, UX-DR43

## Why this epic exists

"Catch a thought in under two seconds without leaving the context I am in" outranked every
other job in the brainstorm. Not by a little — it was the job that made every other one worth
building, because a note system you do not reach for is a folder of files.

Two seconds is not a slogan; it decomposes into things that are either true or not:

- The panel is up and focused within 300 ms of the hotkey (NFR-27).
- The first keystroke lands in the buffer, even if it is typed 50 ms after the hotkey.
- Nothing is asked. No title, no folder, no tag, no save button — anywhere in the feature
  (UX-DR35). Every question is a decision, and a decision at capture time is why people stop
  capturing.
- Dismissing the panel does not lose the buffer, and neither does `kill -9`.

Everything in this epic is in service of those four sentences, plus the surface they hang off:
the tray. The brainstorm's position is that the main window is optional for a whole day of use,
which promotes the tray from a convenience to *the* primary surface — and that promotion is
what turns two known Linux defects from cosmetic into blocking.

### The Linux tray is a blocker, and it is measured

Story 36.1 comes first because two facts in the current tray make a tray-primary feature
impossible on Linux:

1. **All ten tray glyphs are macOS template images.** `keeper/icons/tray-*.png` — idle, error,
   recording, and the seven sync states — are 44x44 RGBA8, pure black plus alpha. On macOS
   `set_icon_as_template` tells the system to invert them against the menu bar. On Linux that
   call is a documented no-op, so on an XFCE or ayatana panel with a dark theme keeper draws
   black on black. The user does not see a wrong glyph; they see nothing.
2. **A Linux tray menu cannot be replaced once set.** The current design has one global
   `Mutex<Option<TrayState>>` and one `on_menu_event` router registered on the tray itself, so
   the router survives a `set_menu` — but on Linux there is no second `set_menu` to survive.
   Any affordance not present in the menu built at tray creation can never appear.

That second fact dictates the shape of every later tray story: notes items are **built once,
at tray creation**, and thereafter mutate only through `set_text` and `set_enabled`. It is also
why the capability gate matters more here than elsewhere — an item omitted at build time on a
build where notes is absent is omitted forever, which is correct, and an item present but
inapplicable is disabled with a legible label, not missing.

Related and separate: `TrayIconEvent` clicks and `show_menu_on_left_click` are Linux-
unsupported, so no notes affordance may be reachable only by clicking the icon. Every one of
them is a menu item or a hotkey.

### The capture window is a second window, and the capability file is not optional

There is no `WebviewWindowBuilder` anywhere in the codebase today; `tauri.conf.json:19-28`
declares exactly one static window and every capability file scopes to `"windows": ["main"]`
(`capabilities/default.json:5`). A second window that inherits nothing can invoke no command
at all — it will render and then sit there inert, which is a failure mode that looks like a
frontend bug and is not. So AD-60's window is declared statically alongside `main`, and it
ships with its own least-privilege capability file in the same story that creates it.

## Stories

### Story 36.1: Linux Gets a Tray It Can See and a Menu It Can Keep
**Rust-only (`keeper` shell) + assets.** Bindings: no.

Add a Linux-visible glyph variant for all ten tray images under `keeper/icons/`, selected by
target at build time, and stop calling `set_icon_as_template` where it is a no-op. Restructure
`tray.rs`'s menu construction so the full item set — including the notes items stories 36.7 and
38.3 will need — is decided once at tray creation from the capability snapshot, and every later
change goes through `set_text`/`set_enabled` on held item handles. Keep the last-pushed-glyph
guard from story 34.1 intact: the variant switch must not reintroduce a write per tick.
AC: on an XFCE panel, in both the dark and the light default themes, all ten glyphs are legible
and distinguishable by shape; no code path calls `set_menu` after tray creation on Linux, and a
test asserts it; the 60-second idle watch from story 34.1 still performs zero icon writes.

### Story 36.2: The Notes Actions Exist Once
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 35.2.

`keeper-core/src/palette.rs` gains `requires_notes` on `PaletteActionVm` and
`registry_sections(recording, notes)` (`:690`), following the `requires_recording` precedent
exactly. Six actions are declared: New Note, Quick Capture, Today's Journal, Open Note…,
Search Notes, Switch Vault. Because the native menu builder (`keeper/src/menu.rs`) and the
`cheat_sheet_sections` command both read `registry_sections`, declaring them once gives the
palette, the ⌘? cheat sheet and the native menu bar the same six actions with the same
shortcuts, and makes drift impossible rather than unlikely (UX-DR42, the UX-DR15 rule).
AC: `bun run bindings:check` passes; with the notes capability off, all six actions are absent
from all three surfaces and the existing exhaustive registry tests still pass; with it on, each
action's title and accelerator are identical in the palette, the cheat sheet and the native
menu, asserted by the existing cross-surface consistency test extended to the new actions.

### Story 36.3: The Capture Window Exists, and Answers in 300 ms
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 36.2.

`tauri.conf.json` gains a second static window — `label: "quick-capture"`, `visible: false`,
`alwaysOnTop: true`, `decorations: false`, `skipTaskbar: true`, `resizable: false` — plus
`keeper/capabilities/quick-capture.json` scoped to `"windows": ["quick-capture"]` carrying only
the permissions the panel needs. A third global hotkey `hotkey.capture` clones the existing
triple: a key in `keeper-core/src/registry.rs` beside `hotkey.global` (`:913`) and
`hotkey.recording` (`:940`), storing the accelerator opaquely, plus registration in
`keeper/src/hotkey.rs`. Rust owns show, position and focus (`keeper/src/lifecycle.rs`); the
webview asks for nothing and renders a single `<textarea>` and no chrome (AD-60). The window is
created hidden at startup so the hotkey path is a show, not a construction.
AC: measured on a warm process on both macOS and Linux, hotkey to focused caret is under
300 ms across 20 samples with the slowest recorded; a character typed 50 ms after the hotkey
appears in the textarea; the panel floats above a fullscreen window and never takes a taskbar
slot; removing the capability file makes the panel's own commands fail — the test that proves
the file is load-bearing.

### Story 36.4: The Capture Buffer Survives Everything
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 36.3.

The buffer's source of truth is Rust, not the webview: the panel pushes its text on a ~500 ms
idle debounce and on hide, and Rust persists it under a `notes.capture_buffer` key in the
`keeper-core/src/registry.rs` settings table. Escape saves and hides; there is no save button
and no discard prompt (UX-DR35). On show, Rust seeds the textarea from the stored buffer before
the window is made visible, so the panel never paints empty and then fills.
AC: type, Escape, re-summon — the text and the caret position are there; type, `kill -9` the
process, relaunch, summon — the text is there; the panel never renders an empty textarea that
subsequently populates, asserted by a first-paint assertion rather than a screenshot.

### Story 36.5: The Vault Writer — First Line Is the Title, No Dialog
**Rust-only (`keeper` shell + `keeper-core`).** Bindings: no. Depends on 35.5, 35.3, 36.4.

`notes_vault::write`: compose frontmatter with a fresh ULID via `keeper_core::notes`, resolve
the filename through `name.rs`, and write through the engine's existing `.keeper.*.tmp`
staging prefix followed by an atomic rename — which is the whole reason a half-written note
never reaches a commit, since that prefix is already a tier-0 exclusion and the rename is
already handled as a modification by the stability gate. The capture panel's Escape flushes the
buffer through this writer into the vault's capture destination and clears the buffer only
after the rename returns.
AC: creating 200 notes titled "Meeting" produces 200 files with counter suffixes and 200
distinct ULIDs; no dialog is presented at any point; a `kill -9` between write and rename
leaves a `.keeper.*.tmp` file that the next scan excludes and no partial note in the vault; the
capture buffer is still present after a flush that failed on a read-only volume, with the
failure surfaced as a non-blocking notice (NFR-30).

### Story 36.6: Journal, Templates, and the Per-Vault Settings
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 36.5.

`keeper-core/src/notes/template.rs` — pure placeholder expansion for date, time, title and
cursor position — plus `notes_vault` applying a template at creation from `templates/*.md`.
The journal lands at `journal/<YYYY>/<YYYY-MM-DD>.md` from the journal template, and the
Today's Journal action from 36.2 opens it or creates it. `NotesConfig` (story 35.1) gains
`journal_path_template`, `default_template` and `capture_destination`, each `#[serde(default)]`
so existing vaults keep working, and each surfaced in the profile's notes settings showing the
value actually in force. The cadence knob is story 39.1's; the other three land here.
AC: invoking Today's Journal twice in one day opens the same file and does not append a second
template body; a template containing every documented placeholder expands with the cursor
landing where the marker was; a vault whose journal template path points at a missing file
creates a plain dated note and raises a legible notice rather than failing; `bun run
bindings:check` passes.

### Story 36.7: The Tray Is a Note Taker
**Rust-only (`keeper` shell).** Bindings: no. Depends on 36.1, 36.5, 36.6.

`tray.rs` gains, in the first-built menu, New Note, Today's Journal and five last-touched note
slots, all routed through the tray's surviving `on_menu_event` router. Recency is maintained in
`notes_vault::registry` and pushed to the held item handles with `set_text`/`set_enabled` only
— never a rebuild, per 36.1. With the notes capability off the items are omitted at build time;
with it on but no vault flagged they are present and disabled with a label that says why. The
unread indicator on the glyph is story 38.3; this story is the menu.
AC: on Linux, touching five notes updates five menu labels with no `set_menu` call and an open
menu is not closed by the update; clicking a recent item raises the main window with that note
selected; on a build with notes absent the items do not exist in the menu; with no vault they
are visibly disabled and their label states the reason.

## Out of scope

- The unread dot on the tray glyph, and everything that computes unreadness (Epic 38).
- Capture-from-a-chat-message. Declared Should in the brainstorm and not scheduled this phase;
  the capture path here takes plain text and imposes no source-link format, which keeps it
  forward-compatible without paying for it now.
- Any editing surface. The panel is a textarea; live preview is story 37.6.
- Rich capture targets (append to today's journal, capture into a chosen space). Capture files
  nothing at creation, by decision; retro-filing is the list's job (Epic 37).
