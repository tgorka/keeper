# Spec 46.16 — Start a recording from the tray

story: 46.16
status: implemented in code; **no line of `tray.rs` was compiled** (see gate checks)
branch: `feat/epic-46-config-and-the-gaps`
binds: FR-48, FR-66, UX-DR42, AD-35, AD-61 (respected, not changed); continues 46.5

## What the absence actually was

The report — *"in the menu I don't see New recording — it opens the Recordings tab"* — has two
halves and wave 1 fixed one of them. Story 46.5 retitled `recording-start` to **New Recording**
and gave it the keywords `new` / `start`, which reached the ⌘K palette, the ⌘? cheat sheet and
the native menu bar in a single edit, because all three are renderings of
`keeper_core::palette::registry_sections()`.

It reached the menu-bar tray not at all, and the reason is the story:

**The tray had no start item in any state, and could not have had one renamed into it.** It is
the one discovery surface that does not project the registry — it hand-builds every label from a
string literal. `build_idle_menu` was the notes section + Show keeper + Quit; `build_recording_menu`
offered Stop + Open Recordings Folder over a live session; `build_error_menu` offered Show
Recording / Open Folder / Dismiss Error. The id list carried no start id. So from the menu-bar
icon there was no way to begin a recording, which is very plausibly the "menu" the owner meant:
the menu bar's own menu is the surface you reach when the window is hidden, and hiding the window
is what the tray is for.

## The decision, and why it is the bigger one

The brief offered two answers: one hand-built item consistent with the rest of `tray.rs`, or make
the tray project the registry like its three siblings. **The tray now projects the registry** —
for the verbs it offers, which is the honest reading of "like its siblings".

`keeper_core::palette` gained:

- `RECORDING_START_ID` / `RECORDING_STOP_ID` / `RECORDING_OPEN_FOLDER_ID` — the three dispatch ids,
  now spelled **once** and referenced by the registry entries themselves. Titles are allowed to
  change (46.5 proved it); ids are what `actions.ts` dispatches on and never do. The shell has to
  name one of them in Rust, so it names the constant.
- `TrayMenu { Idle, Sync, Recording, Error }` — which rendering is being built.
- `tray_recording_verbs(menu, recording) -> Vec<MenuItemVm>` — membership per rendering
  (`tray_verb_ids`, private), **order and words from `registry_sections()`** by filtering that
  projection in place. Pure, and it compiles and tests on Linux.

`tray.rs` keeps only glue: build the items from the projection (`build_recording_verbs`), map a
registry id to the tray's own item id where the shell dispatches the click
(`tray_verb_item_id`), and route the one forwarded id (`start_recording`).

Why not project the *whole* registry into the tray — the literal reading — is recorded under
"deliberately NOT done": most of the tray is not registry verbs at all, and thirty items in a
menu-bar dropdown is a worse surface than the curated one. The half that IS still hand-typed —
the notes section's labels — is real and is now **DW-195**, with the shape of the fix and the one
property (slot order) that widening must not inherit.

### How the click is dispatched, and why it is not a second implementation

The item's id **is** the registry id `recording-start`, and clicking it runs
`show_main_window(app)` then `crate::menu::handle_menu_event(app, RECORDING_START_ID)` — the
identical function the native menu bar's own registry items call. That emits
`keeper://menu-action`, which `use-menu-actions.ts` routes through `dispatchPaletteAction`, the
single frontend dispatch table. `resolveMenuActionId` passes a non-toggle id through unchanged
(`use-menu-actions.ts:40-44`), so `recording-start` lands on the handler at `actions.ts:80` —
`setView("recording")` then `startRecordingWithCurrentSelections()`, the same shared
recording-control module the global hotkey uses.

A Rust-side start beside `stop_recording` was rejected on merit, not effort: a start carries the
**current** capture selections (screen/window, mic, camera), which live in frontend stores. A
shell start would have to invent a default selection set — a silent revert to defaults for the
user, and precisely the parallel implementation this story exists to prevent. The stop stays
shell-side for the opposite reason: it is the panic button on a live capture and must not depend
on a responsive webview.

**The id namespace now says who dispatches**: `tray-*` is handled in the shell, a registry id is
forwarded verbatim. That is written on `STOP_ID`, `OPEN_FOLDER_ID` and `tray_verb_item_id`, and
asserted by the one new test in `tray.rs`.

## I/O matrix

### `tray_recording_verbs(menu, recording)` (`keeper-core`, tested on this box)

| `menu` | `recording = true` | `recording = false` |
|---|---|---|
| `Idle` | `[recording-start]` → "New Recording" | `[]` |
| `Sync` | `[recording-start]` → "New Recording" | `[]` |
| `Recording` | `[recording-stop, recording-open-folder]` → "Stop Recording", "Open Recordings Folder" | `[]` |
| `Error` | `[recording-open-folder]` → "Open Recordings Folder" | `[]` |

Titles are never spelled in the function or in its tests — they are compared against
`palette_actions()`, so the table above is what the registry currently says, not what the code
asserts.

### The four tray menus (`tray.rs`, **not compiled**)

| Rendering | Menu, in order | Start item |
|---|---|---|
| idle (Story 10.3/36.7) | notes section, ─, **New Recording**, ─, Show keeper, Quit | present |
| sync (Story 29.2) | notes section, ─, **New Recording**, ─, sync status line, Show keeper, Quit | present |
| recording (Story 18.1) | status line, ─, Stop Recording, Open Recordings Folder, ─, Show keeper, Quit | absent |
| error hold (Story 18.4) | `Recording failed — <reason>`, ─, Show Recording, Open Recordings Folder, Dismiss Error, ─, Quit | absent |
| any rendering, `recording_supported() == false` | exactly the menu that shipped before this story, no stray separator | absent |

### Click routing (`tray.rs`, **not compiled**)

| Item id | Handler | Path |
|---|---|---|
| `recording-start` | `start_recording` | `show_main_window` → `menu::handle_menu_event` → `keeper://menu-action` → `dispatchPaletteAction` |
| `tray-stop-recording` | `stop_recording` | shell: `ipc::stop_active_recording` |
| `tray-open-recordings-folder` | `open_recordings_folder` | shell: reveals the **live session's** `output_path` |
| `tray-show` / `tray-show-recording` / `tray-quit` / `tray-dismiss-error` / `tray-note-*` | unchanged | unchanged |

## Edge cases

- **The sync menu replaces the idle menu.** This is the one that would have shipped broken. On the
  first sync tick `build_sync_menu`'s menu is installed over the idle one — the existing docblock
  says so about the notes section — so a start verb added only to `build_idle_menu` would vanish
  the moment a folder started syncing, which is most of the time. `TrayMenu::Sync` exists for that,
  and it is a tested row (M1), not a comment.
- **The verb sits in the same slot in both.** Idle and sync both place it after the notes section
  and before the tail, so "New Recording" does not move when a folder starts syncing.
- **Linux cannot swap a tray menu** (AD-61), so an affordance absent from the first menu can never
  appear. The first menu is the idle one, which now carries the verb — and on Linux
  `recording_supported()` is `false`, so the projection is empty there anyway and the menu is
  byte-identical to what shipped.
- **No lingering start item.** Every builder asks the projection for its own rendering; the live
  and error renderings cannot grow one without changing `tray_verb_ids`, which fails M2.
- **No stray separator.** `add_recording_verbs` appends nothing — not even the grouping separator —
  when the projection is empty, so a build without the recording capability renders the previous
  menu exactly.
- **The error hold gets no start verb.** Story 18.4's rule is that the tray never restarts a
  session; the one-click Restart lives on the window's banner, reached by Show Recording. A start
  item there would be a second restart path with different words.
- **The error menu's folder reveal is mid-group.** It sits between Show Recording and Dismiss
  Error, so those verbs are appended by hand rather than through `add_recording_verbs` (which
  groups with a trailing separator). Order is preserved exactly as shipped.
- **Two folder reveals with the same words, deliberately.** The tray's
  `tray-open-recordings-folder` reveals the **live session's** folder (`output_path`); the
  registry's `recording-open-folder` reveals the **configured destination**. Same label, two
  folders — which is why that id stays shell-dispatched. Forwarding it would quietly change which
  folder opens mid-session. This is a pre-existing divergence the story names rather than creates.
- **A click before the webview mounts** (tray built at startup, window never shown) emits into no
  listener and is dropped. Identical to every native menu-bar registry item today; not introduced
  here.
- **`show_main_window` first.** `dispatchPaletteAction("recording-start")` sets the view but
  cannot raise a hidden window; the notes tray verbs already all raise first
  (`notes_ipc.rs:4273`), and this follows them.
- **One listener.** `useMenuActions` is mounted only in `app-shell.tsx`, so the emit dispatches
  once; the quick-capture webview does not mount it.

## Mutation table

Rust rows: `cargo test -p keeper-core --lib palette::`. Each mutant was applied from a byte
backup by `/tmp/mut4616.sh`, run, and restored from that backup with a `sha256` comparison per
row; the script's `trap … EXIT INT TERM` restores on a kill as well, and every mutant carried the
sentinel `MUT46-16`. **An earlier attempt at this sweep was interrupted mid-mutation** (an inbound
message stopped the cell while M1 was live) — the residue was caught by the sha mismatch, restored,
and the sweep was rewritten to be interrupt-proof before being re-run. Post-sweep,
`grep -rn MUT46-16 src-tauri/ src/` returns nothing and `palette.rs` hashes to its pre-sweep
`a04a6b6e…`.

| # | Mutation | Caught by | Result |
|---|---|---|---|
| M1 | `TrayMenu::Sync` stops carrying the start verb (the "vanishes while a folder syncs" bug) | `the_tray_offers_the_start_verb_exactly_where_a_session_can_begin` | ✅ 1 failed |
| M2 | the start verb added to `TrayMenu::Recording` (lingers into the live menu) | same test | ✅ 1 failed |
| M3 | the projection overwrites the title with a hand-typed `"Start Recording"` | `the_tray_shows_the_registrys_own_words_for_every_verb_it_projects` | ✅ 1 failed |
| M4 | `registry_sections(recording, …)` → `registry_sections(true, …)` (capability gate lost) | `no_tray_rendering_projects_a_recording_verb_when_the_capability_is_off` | ✅ 1 failed |
| M5 | `TrayMenu::Error` names `"recording-reveal-folder"`, an id the registry does not ship | `every_verb_a_tray_rendering_names_reaches_the_projection` | ✅ 1 failed |
| M6 | the registry entry stops using the shared const (`id: "recording-begin"`) | `every_verb_a_tray_rendering_names_reaches_the_projection` **and** 46.5's `open_recording_present_iff_recording_capability_on` | ✅ 2+ failed |

**6 mutants, 6 caught, 0 survived.** M6 is the one worth reading: it proves the id const and the
registry cannot drift apart, which is what lets the shell name a verb in Rust at all.

Not mutated, and why: `tray_verb_item_id`'s map and the four menu builders live in the shell crate.
Its test (`the_id_namespace_says_who_dispatches_a_projected_verb`) is written and will run on the
macOS gate; it could not be executed, let alone mutated, on this box.

## What I could not verify here, and why

**Nothing in `src-tauri/crates/keeper/src/tray.rs` was compiled.** The `keeper` shell crate does
not link on Linux (no GTK/webkit), so `cargo build`/`check`/`clippy`/`test -p keeper` were never
run — deliberately, per the wave's constraint. Two compile-free checks were run on it and both
pass: `rustfmt --edition 2021 --check crates/keeper/src/tray.rs` exits **0**, which also proves
the file still parses; and every API it calls (`MenuBuilder::item`/`items`/`separator` with an
`'a`-bound item ref, `menu::handle_menu_event<R: Runtime>`, `macos_version::recording_supported`)
is used exactly as an existing call site in the same file or in `menu.rs` uses it. That is not a
build. Treat every claim about the four menus as unbuilt until gate 1 passes.

`keeper_core::palette` **was** compiled and tested here: `cargo test -p keeper-core --lib palette::`
→ **30 passed, 0 failed, EXIT=0**, including 46.5's three-verb section-count assertion.

Ordered gate checks:

1. **`cargo build -p keeper` on macOS.** First and blocking; nothing below can run until it links.
   Expect no warnings from `tray.rs`: the five new `keeper_core::palette` imports are all used, and
   `add_recording_verbs` takes `mut builder` (no shadowing `let mut`).
2. **`cargo clippy -p keeper --all-targets` on macOS.** Workspace lints are `clippy::all = warn` +
   `unwrap_used = warn`; the new code contains no `unwrap` and no `unsafe`.
3. **`cargo test -p keeper --lib tray::` on macOS.** The new
   `the_id_namespace_says_who_dispatches_a_projected_verb` must pass, along with the 18.x/29.x
   tray tests, which were not touched.
4. **`cargo fmt --check`** — expect two pre-existing deviations in `palette.rs` (lines ~537 and
   ~1292) that belong to wave 1's 46.5 edit, not to this story; `tray.rs` is already clean.
5. **Idle tray, macOS ≥ 13, recording available.** Settings → menu-bar presence on, no session,
   no folder syncing. The menu reads: notes section, ─, **New Recording**, ─, Show keeper, Quit.
   The words are "New Recording", not "Start Recording" — if they differ, the projection is not
   being read.
6. **Press New Recording with the window hidden.** The main window raises and focuses, the
   Recording view is on screen, and a session starts **with the selections currently configured**
   (not defaults) — pick a specific window + mic first, hide keeper, then start from the tray and
   confirm the banner names those.
7. **The live menu.** With that session recording, open the tray: status line, ─, Stop Recording,
   Open Recordings Folder, ─, Show keeper, Quit. **No New Recording item.** This is the lingering-
   verb check on the real menu.
8. **The sync menu — the check most likely to catch a real bug.** Stop the session, start a folder
   sync so the sync rendering installs (the icon changes and a sync status line appears), then open
   the tray: **New Recording is still there**, in the same slot as in gate 5, above the sync status
   line.
9. **The error hold.** Force a session failure (e.g. kill `keeper-rec`). The tray badges, and the
   menu reads `Recording failed — …`, ─, Show Recording, Open Recordings Folder, Dismiss Error, ─,
   Quit — **no New Recording**, and the folder reveal still opens the failed session's own folder,
   not the configured destination.
10. **Return to idle.** Dismiss Error (or let a session finalize) and confirm the idle menu comes
    back **with** New Recording — `restore_idle` rebuilds it, so a regression here shows as the
    verb never returning.
11. **Forced presence (Story 18.2).** With menu-bar presence OFF, start a recording from the app.
    The tray force-builds in the recording rendering (no start verb); on the terminal tick it
    disappears entirely. Unchanged by this story, and the check that proves it.
12. **Linux/other desktop.** Presence on: the menu is exactly what shipped — no New Recording, and
    **no stray separator** where it would have been.
13. **Not checkable anywhere:** whether the owner's "the menu" meant the menu bar's menu or the
    tray's. Both now offer the verb with the same words, so the question no longer has two answers.

## Deliberately NOT done

- **The tray does not project the whole registry.** The literal version of "project it like its
  three siblings" would put ~30 items across seven categories into a menu-bar dropdown, and most
  of the tray is not registry verbs at all (Show keeper, Quit, two status lines, five recent-note
  slots, Dismiss Error, Show Recording). "Project the registry" here means the verbs the tray
  offers come from the registry, which is what landed.
- **The notes section's labels are still hand-typed** — the same defect shape one level down,
  recorded as **DW-195** with the concrete fix and the reason it is not a copy of this one (the
  notes labels are composed, and their slot order must stay the tray's own under AD-61).
- **No new Tauri command**, so `generate_handler!` and `src/test/command-registration.test.ts` are
  untouched. The verb reaches the already-registered `recording_start` command through the
  frontend handler that already existed.
- **`recording-stop` and `recording-open-folder` keep their shell dispatch and their `tray-*` ids.**
  Only their words now come from the registry. Reasons are on the constants: a stop must survive a
  wedged webview, and the tray's reveal opens a different folder from the registry's.
- **The registry is otherwise unchanged.** No title, category, keyword, shortcut or gate moved; the
  three `id:` lines now reference the constants that hold the same strings. 46.5's rename and its
  section-count assertion stand.
- **No icon change.** The idle glyph still means "presence"; a start verb is not a state.
- **No `set_menu` refresh loop.** Nothing here mutates a live menu — the verb is built into each
  rendering, which is what AD-61 requires.

## Files changed

- `src-tauri/crates/keeper-core/src/palette.rs` — `RECORDING_START_ID` / `RECORDING_STOP_ID` /
  `RECORDING_OPEN_FOLDER_ID` (referenced by the three registry entries), `TrayMenu`,
  `tray_verb_ids`, `tray_recording_verbs`; four new tests.
- `src-tauri/crates/keeper/src/tray.rs` — module docblock records the projection; `start_recording`
  (raise + shared dispatch) and the `RECORDING_START_ID` router arm; `tray_verb_item_id`,
  `build_recording_verbs`, `add_recording_verbs`; all four menu builders take their recording verbs
  from the projection; `STOP_ID` / `OPEN_FOLDER_ID` docs record the id-namespace rule; one new test.
- `_bmad-output/implementation-artifacts/deferred-work.md` — DW-195.

## Gate results on this box

- `cargo test -p keeper-core --lib palette::` — **30 passed, 0 failed, EXIT=0** (re-run after the
  sweep restore).
- Mutation sweep — 6 mutants, 6 caught, `palette.rs` restored to `a04a6b6e…`, `grep MUT46-16`
  empty.
- `rustfmt --edition 2021 --check crates/keeper/src/tray.rs` — **EXIT=0** (parse + format).
- `cargo build/check/clippy/test -p keeper` — **not run, cannot run here.**
- Full suite, formatter and linter deliberately not run (Main runs them once at the end).
