# Epic 34 — Sync you can see, and a window that behaves

status: draft
created: 2026-07-28
altitude: epic
parent: Epic 32 (sync visibility and control), Epic 33 (add a folder, verified copy)
source: field report from the 0.6.2 build on hesperia (macOS 26.5, arm64), plus a
full read of the tray, shell, sync-UI and sync-engine paths

## Why this epic exists

A user ran 0.6.2 on their own machine, dropped real files into a real synced folder, and
reported ten things. Every one of them reproduces, and the read that followed found that most
of them are not ten problems but four:

1. **The tray glyph repaints itself once a second.** Not an animation — the same bytes,
   pushed again, forever.
2. **The window steals ~40 px at the top and cannot give the drawer's bottom back.** The
   empty band reads as a black strip, and the account footer is clipped with no way to scroll
   to it.
3. **The sync engine has no filesystem watcher wired at all.** `watch.rs` is written, tested,
   exported — and never constructed outside its own tests. Change detection is a 15 s poll, a
   file needs two polls to settle, and the status line says "up to date" the whole time.
4. **Sync's UI throws away what the backend already tells it.** `Sync now` returns a full
   outcome and discards it; activity rows carry a change kind; progress carries bytes. None of
   it is shown, or it is shown without the one number that makes it legible.

The rest are genuine gaps: no reveal for the access token, no way to name the machine or shape
the commit subject, no size on an activity row, no discard on the add form, no remove in the
Sync view, no transfer rate.

### What the user asked for, and where we disagree

The report proposed *turning the settle window off* and putting cache extensions in
`.gitignore` instead. We are not doing that, because the settle window is not what is slow.
Evidence: `StabilityGate.entries` is a `HashMap<PathBuf, Entry>` (`stability.rs:220`) and
`observe` restarts the timer for **one path only** (`stability.rs:271-289`) — a new write does
not restart the batch. The 5 s window is not the floor; the **15 s poll is**, twice over,
because a never-observed path is `Settling` by definition (`stability.rs:298-301`) and paths
are only observed inside `collect_stable_changes`. Switching the window off would remove the
one guard that stops a half-written file being committed, and leave the 15 s cadence — the
actual cause — untouched.

What the report was right about: the built-in exclusion list is too short. `BUILTIN_EXCLUDES`
(`exclude.rs:44-107`) covers partial downloads and lock files but not `node_modules`, build
output, or `__pycache__`. That is story 34.9's second half.

## Decisions

**AD-34-1 — A glyph write happens on a transition, never on a tick.**
Binds: the tray. Prevents: the flicker. Rule: the sync renderer remembers the last glyph and
the last status string it actually pushed, and returns early when the next tick would push the
same bytes. The recording renderer already works this way (`tray.rs:545-567` guards on
`status_item`/`error_rendered`); the sync renderer at `tray.rs:1125-1130` is the outlier and
becomes consistent with it. Corollary: a state that flips and un-flips inside one supervisor
tick must not reach the tray at all.

**AD-34-2 — One inset reserves the traffic lights, and it is the drag region.**
Binds: window chrome. Prevents: paying twice for the same 78x12 px. Rule: exactly one element
reserves space for the macOS traffic lights. Today two do — the 28 px band at
`app-shell.tsx:141` and a second `pt-3` at `sidebar-pane.tsx:106`. The second goes. The
survivor carries `data-tauri-drag-region`, and it is rendered only where the platform actually
floats traffic lights over the webview.

**AD-34-3 — The band is never a colour the app does not otherwise use.**
Binds: the same band. Prevents: the "black strip". Rule: the band spans two panes with two
different backgrounds (`bg-sidebar` on the left, `bg-background` on the right), so it must be
painted per-column to match what is beneath it. A single full-width `bg-background` strip above
a `bg-sidebar` drawer is a visible seam in light mode and a black bar in dark mode, and that is
the entire bug.

**AD-34-4 — Every pane can reach its own bottom.**
Binds: layout. Prevents: unreachable UI. Rule: any pane whose content grows with user data
pairs `min-h-0 flex-1` with a scroll container. Every pane does this except the sidebar
(`sidebar-pane.tsx:100-104`), whose Spaces and Networks lists are unbounded and whose
`mt-auto` footer is therefore clipped by the `overflow-hidden` root. The sidebar joins the
rule.

**AD-34-5 — No `#[tauri::command]` does filesystem work on the main thread.**
Binds: responsiveness. Prevents: a window that will not move. Rule: Tauri v2 runs non-`async`
commands on the main thread, and on macOS `startDragging` resolves to
`performWindowDragWithEvent` on that same thread. So a command that reads a directory or loads
a manifest is `async`. Today `recovered_sessions_list` (`ipc.rs:4911`, a `read_dir` plus a
manifest load per subfolder), `recording_session_summary` (`ipc.rs:4880`),
`recording_settings_get` (`ipc.rs:5063`, mounted three times per pane) and `recording_status`
(`ipc.rs:4757`, polled at 1 Hz) are all synchronous — which is why the Recording tab is the
only tab where the window will not drag.

**AD-34-6 — Focus is not a reason to spawn a process.**
Binds: the same symptom. Prevents: two sidecar launches at the exact moment a drag begins.
Rule: window `focus` may invalidate cached data, but it may not unconditionally spawn a child.
`recording-source-picker.tsx:143-147` and `use-recording-permission.ts:191-193` both spawn a
`keeper-rec` on every focus event; clicking an unfocused titlebar to drag it therefore costs
two process launches. They coalesce behind a minimum interval.

**AD-34-7 — A secret is revealable, never silently readable.**
Binds: the access token. Prevents: turning a write-only credential into an ambient one. Rule:
the field gets an eye toggle for what the user is typing, and revealing a *stored* token is a
separate, explicit act that fetches from the keychain on demand and is never part of loading
the form. The token is not sent to the frontend when the edit form opens.

**AD-34-8 — A field that has a default shows that default.**
Binds: the sync form. Prevents: a blank box that silently means 5 000. Rule: every numeric
knob shows the value that is actually in force, including the ones the backend substitutes.
`settleSeconds` renders blank today while `effective_settle_ms` (`profile.rs:232-238`) may be
applying 10 000 for removable media, and `pollIntervalMs` is not in the form at all.

**AD-34-9 — Nothing the backend knows about a profile is lost by saving it.**
Binds: `parse_req`. Prevents: the bug class that has now bitten twice. Rule: `parse_req`
(`sync_ipc.rs:378-451`) clones the prior profile as its base and overwrites only what the
request expresses, instead of building from `SyncProfile::new` and re-adding survivors one at a
time. Today it restores `enabled`, `author_override` and `volume_id` and drops
`poll_interval_ms`, which the engine started consuming on 2026-07-28 (DW-116) — so every save
from the app resets the scan cadence to 15 s.

**AD-34-10 — "Up to date" is a claim, and it must be true.**
Binds: `status_line`. Prevents: the most misleading string in the app. Rule: the line may only
say "up to date" when nothing is waiting for any reason. `progress.rs:403` is guarded solely by
`status.pending > 0`, and `pending` counts **journal rows** (`db.rs:471-478`) — a folder with
five thousand files inside their settle window has no journal rows, reports `pending = 0`, and
prints "up to date". The count of settling files is already computed (`engine.rs:1508`) and
thrown into a `debug!` (`engine.rs:1539-1541`); it becomes a field.

**AD-34-11 — The engine reacts to the filesystem, and polls only as a backstop.**
Binds: change detection. Prevents: the 15 s floor. Rule: `watch::FolderWatcher`
(`watch.rs:360-528`) is constructed for each enabled profile and drives the engine; the paced
scan stays as the backstop for what a watcher cannot see (missed events, network volumes,
watcher failure). A watcher-delivered close-write reaches `note_close_write`
(`stability.rs:257`) and takes the 1 s `CLOSE_WRITE_SETTLE_MS` path that exists today and is
unreachable.

**AD-34-12 — A user-initiated sync is recorded as user-initiated.**
Binds: provenance and feedback. Prevents: a lie in the commit trailer and a button that looks
broken. Rule: `SyncSource` threads from `sync_once` into `Provenance`, instead of being consumed
by one log line (`engine.rs:1935`) while the commit hard-codes `SyncSource::Watch`
(`engine.rs:1559`, `engine.rs:1184`). And `Sync now` reports its outcome: `SyncOutcomeVm`
(`sync_ipc.rs:158-163`) is returned by the command and discarded by the store
(`sync.ts:276-280`).

**AD-34-13 — Progress that cannot be rated is not progress.**
Binds: the progress payload. Prevents: a bar that moves with no sense of how fast. Rule:
`SyncProgress` carries a byte rate derived from a start instant and `bytes_done`, and the commit
leg advances `files_done` and `current` per file instead of jumping 0 -> total
(`engine.rs:1316-1319`) with `current` set once (`engine.rs:1310`).

**AD-34-14 — Removing a profile takes its secret with it.**
Binds: removal. Prevents: an orphaned keychain item keyed on an id that can never be re-derived.
Rule: `sync_profile_remove` deletes `sync/<id>/credential` (`profile.rs:242`). The existing
contract that the folder and its `.git` are left on disk (`sync_ipc.rs:499-500`) does not
change.

**AD-34-15 — An excluded path is invisible everywhere.**
Binds: the pending list. Prevents: a `.DS_Store` listed as pending forever. Rule: `Engine::pending`
(`engine.rs:2128-2148`) filters through the same `ExcludeSet` the commit path uses. Today it does
not, which contradicts the module's own contract (`exclude.rs:8-11`).

## Stories

**34.1 — The tray glyph stops repainting itself.** `keeper/src/tray.rs`: add the last-pushed
glyph and status text to `TrayState`, and make `apply_sync_state` (`:1069`) return before
`set_icon`/`set_icon_as_template` (`:1125-1130`) and before `set_text` (`:1105`) when neither
changed. Then stop the churn upstream: `Engine::publish` promotes state to `Syncing`
(`engine.rs:419-420`) on the transient `Scanning` published at the top of every
`collect_stable_changes` (`engine.rs:1437`), and `refresh_pending` (`engine.rs:687-694`) clears
it back inside the same tick, so the tray's independent 1 Hz sampler catches a state that never
really happened and the 3-tick `SYNC_DWELL` (`tray.rs:990`) pins it for ~4 s. A scan that
stages nothing must not publish a busy phase at all. Tests: the existing `sync_tray_tests`
(`tray.rs:1148-1276`) plus a new one asserting a repeated identical state performs no icon
write. Verify on hesperia: watch the menu bar for 60 s with folders configured and idle.

**34.2 — Give the window back its 96 px, and let the drawer reach its own bottom.**

MEASURED ON HESPERIA, 2026-07-28, keeper 0.6.2, window pinned to (200, 200, 1280x800) and read
out of the accessibility tree. These are the numbers the fix has to satisfy; do not re-derive
them from CSS, because CSS is only half of it:

| Element | Reported frame | What it means |
|---|---|---|
| window (`AXWindow`) | `(200, 200) 1280x800` | the whole window |
| traffic lights (close button) | `(208, 208) 16x16` | native, 8 px below the window top |
| **`HTML content`** | **`(200, 256) 1280x800`** | **the web viewport starts 56 px down and is a FULL 800 tall** |
| drag band (`app-shell.tsx:141`) | `(200, 256) 1280x28` | first 28 px of the viewport |
| verify banner | `(212, 284) 1256x46` | |
| nav (`Views`) | `(200, 338) 260x718` | ends at y=1056 |
| `Add account` button | `(208, 1012) 243x36` | **below the window bottom at y=1000 — unreachable** |

**The root cause is not CSS.** The web viewport is offset 56 px down inside the window *and*
sized to the window's full 800 px height, so `100vh` is 56 px taller than the visible area and
everything anchored to the bottom is pushed out of sight by exactly that much. `mt-auto` is not
collapsing — it is doing its job in a viewport whose bottom is off-screen. This is why the
account footer is clipped with no scrollbar, and it is a strictly separate defect from the two
stacked insets.

So the top of the window wastes **96 px** in three independent layers: 56 px of native titlebar
the webview is pushed below, the 28 px band at `app-shell.tsx:141`, and the 12 px `pt-3` at
`sidebar-pane.tsx:106`. The user sees ~90 px of nothing and calls it a black strip.

Work, in this order:

(a) **Fix the 56 px offset first, and confirm it with a measurement, not a screenshot.**
`titleBarStyle: "Overlay"` + `hiddenTitle: true` (`tauri.conf.json:26-27`) are supposed to give
a full-size content view with the traffic lights floating over it — the measurement says that is
not what is happening. Establish which it is before changing anything: either the setting is not
taking effect (check the value Tauri v2 actually accepts for `titleBarStyle`, and whether
anything overrides it at runtime), or it takes effect and the 56 px is a unified-titlebar area
macOS reserves anyway. If Overlay cannot be made to give a full-height viewport, **drop
`titleBarStyle`/`hiddenTitle` and take the native title bar** — it is the boring, correct
outcome, and then (b) and (c) below become deletions rather than rewrites. Do not "fix" this by
subtracting a magic 56 px in CSS; a hard-coded native-chrome constant is how this bug comes back
on the next macOS release.

(b) **One inset, painted per column (AD-34-2, AD-34-3).** If a band survives (a), it replaces
the full-width one at `app-shell.tsx:141` and paints `bg-sidebar` across the drawer's width and
`bg-background` across the rest, both carrying `data-tauri-drag-region` — a single
`bg-background` strip above a `bg-sidebar` drawer is a seam in light mode and a black bar in
dark mode, which is the whole of the reported "black strip". Render it only where traffic lights
actually float over the webview; `titleBarStyle`/`hiddenTitle` are macOS-only, so elsewhere it
is waste under a real title bar. Take the platform from the existing capability/platform
surface, not a user-agent sniff. Delete the duplicate inset at `sidebar-pane.tsx:106` either
way.

(c) **The drawer scrolls (AD-34-4).** Give the `<nav>` (`sidebar-pane.tsx:100-104`) `min-h-0`
and put the view list plus `SpacesGroup` plus `NetworksGroup` in a scroll container, leaving the
`mt-auto` footer (`:246`) pinned — the pattern every other pane already uses
(`sync-pane.tsx:516`, `bridges-pane.tsx:43`, `recording-pane.tsx:264`). This is needed even
after (a): eight Spaces in a 600 px window overflow on their own.

Acceptance, measured on hesperia the same way: with the window at 1280x800, the web viewport's
reported height equals the window's visible height (no bottom overflow), `Add account` reports a
frame fully inside the window, and the top of the window is one continuous colour per column in
both light and dark. Then at the 600 px minimum height with eight Spaces the footer is still
visible and the lists scroll.

**34.3 — The window stays draggable on the Recording tab.** Make every recording command that
touches the filesystem `async` (AD-34-5): `recording_status` (`ipc.rs:4757`),
`recording_session_summary` (`:4880`), `recovered_sessions_list` (`:4911`, including
`scan_recovered_sessions` `:4930-4980`), `recording_settings_get` (`:5063`), plus
`recording_stop` (`:4749`) and `recording_acknowledge` (`:4770`). Coalesce the focus-driven
refreshes behind a minimum interval (AD-34-6) in `recording-source-picker.tsx:143-147` and
`use-recording-permission.ts:191-193`. Harden `src/components/ui/select.tsx:6-8` by defaulting
Radix `Select` to `modal={false}`: five Selects live in the Recording pane and their option
lists mutate on the 3 s poll (`recording-audio-controls.tsx:132-136`,
`recording-webcam-controls.tsx:116-120`), and a Select whose content unmounts while open leaves
`pointer-events: none` on `document.body`, which would defeat Tauri's drag-region hit test
outright. Acceptance, on hesperia: with the Recording tab active, the window drags by its
titlebar band as readily as it does on the Sync tab, and repeated open/close of the fps and mic
Selects does not change that.

**34.4 — The access token can be revealed.** `src/components/sync/add-folder-form.tsx:685-697`:
add an eye toggle that flips the field between `password` and `text`, labelled for screen
readers and reflecting state via `aria-pressed`. Add `sync_get_credential` to
`keeper/src/sync_ipc.rs` beside `sync_set_credential` (`:661`) reading
`platform.secret_get(&profile.secret_key())`, bind it in `client.ts`, and wire a "Show stored
token" action that fetches on demand only — never on form load (AD-34-7). Acceptance: typing a
token can be read back before saving; an existing profile's stored token is shown only after an
explicit reveal; the form still opens with the field blank.

**34.5 — The machine name, the commit subject, and the knobs that were hiding.** Four related
gaps in one story because they share `SyncProfileReq` and one form.
(a) Device label: `db::set_device_label` (`db.rs:167`) exists with no callers — add
`Engine::set_device_label`, an IPC command, and a field in Settings. The label is minted once
from `hostname` at first open (`engine.rs:232`, `keeper/src/sync.rs:137-150`) and reaches every
commit as `Keeper-Device` (`provenance.rs:67`).
(b) Commit subject: add a template field to `SyncProfile` and `SyncProfileReq`, consumed by
`change_subject` (`provenance.rs:243-258`), with today's mechanical
`sync(<profile>): 3 added, 1 modified` as the documented default and a fixed, documented set of
placeholders. Keep `commit_message`'s trailer block (`provenance.rs:225-238`) untemplatable —
provenance is not decoration.
(c) `pollIntervalMs` becomes a form field, since it is the knob that actually governs latency.
(d) Fix `parse_req` per AD-34-9 — clone `prior` as the base — and show real defaults per
AD-34-8, including `effective_settle_ms`'s removable-media substitution. Acceptance: saving a
profile from the app never changes a field the form did not show; the device label round-trips
into a commit trailer; a custom subject template appears in `git log`.

**34.6 — Activity says what changed, and how big.** Add `size` to the `activity` table
(`db.rs:100-106`), `ActivityRow` (`db.rs:722-728`), `record_activity` (`:737-771`) and
`SyncActivityVm` (`sync_ipc.rs:175-181`) — a migration, not a schema edit in place. The size is
available and then thrown away: `is_stable` takes a `FileSample` carrying `size`
(`stability.rs:66-71`) and then `forget`s the entry on `Stable` (`:384-386`), and
`save_file_state` replaces the whole table (`db.rs:646`) — so capture it while it is still in
hand and carry it to `record_commit_activity` (`engine.rs:1638-1658`). Deleted rows record the
size that was removed. In `sync-pane.tsx:731-777` render a distinct icon per `kind` (the
`ActivityKind` set is `added|modified|deleted|conflict`, `db.rs:683-691`) and the size beside
the timestamp. Acceptance: three files of visibly different sizes, one of each kind, are
distinguishable at a glance in the ACTIVITY list.

**34.7 — Discard an add, remove a folder, take the secret with it.**
(a) `sync-pane.tsx:521-527` passes no `onCancel`, so the add form has no cancel button while the
edit form does (`add-folder-form.tsx:762-766`) — pass one, and have it reset the draft.
(b) The Sync view has no remove affordance at all; the only one lives in Settings
(`sync-section.tsx:238-246`) behind an `AlertDialog`. Bring the same confirm into the Sync view
and delete the header comment (`sync-pane.tsx:14-16`) that declares its absence deliberate.
(c) Clear the keychain credential on removal per AD-34-14.
Acceptance: opening Add a folder and clicking Discard leaves no draft and no profile; removing a
folder from the Sync view leaves the folder and its `.git` on disk and leaves no keychain item.

**34.8 — Progress you can watch.** Add a byte rate to `SyncProgress` (`progress.rs:78-92`) and
`SyncProgressVm` (`sync_ipc.rs:729-757`), derived in `TransferTally` (`progress.rs:148-152`)
from a start instant and `bytes_done` — the only `Instant` in the transfer path today
(`git/fetch.rs:327`) is a throttle and is never read for a rate. Advance the commit leg per file
by threading a sink into `git::commit::stage_and_commit` (`git/commit.rs:86-246`) so `files_done`
and `current` move instead of jumping (`engine.rs:1316-1319`). Add `bytes` to `SyncOutcomeVm` —
`SyncOutcome.bytes` is computed (`engine.rs:1978-1980`) and dropped at the VM boundary. Render
the rate and the current file in `sync-pane.tsx:697-715`. Acceptance: pushing a folder with one
large file shows a moving rate and a file counter that climbs.

**34.9 — Files sync when they land, not when a timer says so.** The root cause of the report's
"waiting for writes to stop takes ages".
(a) Wire `watch::FolderWatcher` (`watch.rs:360-528`, already written and tested, exported at
`lib.rs:52`, constructed only in its own tests at `:847/:862/:871`) into the supervisor for each
enabled profile, feeding `note_close_write` (`stability.rs:257`) so the 1 s
`CLOSE_WRITE_SETTLE_MS` path (`profile.rs:114`) becomes reachable. Keep `scan_is_due`
(`engine.rs:564-583`) as the backstop and say so in the code. Watcher failure degrades to
polling, loudly, never silently.
(b) Make the wait honest per AD-34-10: carry the settling count from `engine.rs:1508` into
`SyncStatus` (`progress.rs:233-259`) and add an arm to `status_line` (`:359-405`) ahead of the
"up to date" arm at `:403`.
(c) Apply tier-0 excludes in `Engine::pending` per AD-34-15.
(d) Extend `BUILTIN_EXCLUDES` (`exclude.rs:44-107`) with the conventions the report named:
`node_modules`, `__pycache__`, `.venv`, `target`, `dist`, `build`, `.next`, `.cache` — each
needing both a name rule and a `**/name/**` subtree rule per the module doc (`:21-27`). These are
tier-0 name shapes, not a `.gitignore`: keeper still authors no `.gitignore` (only
`.gitattributes`, `engine.rs:1576-1584`) and git remains the authority on ignore rules
(`repo.rs:383-384`).
(e) `db::save_file_state` (`db.rs:641-666`) is a full `DELETE` plus one `INSERT` per row with no
transaction, run up to four times per pass and largest exactly when the tree is busiest — wrap it
in one transaction.
Acceptance, on hesperia: drop 500 files into a synced folder and the first commit starts within a
few seconds, not a quarter of a minute; while they settle the status line and the tray say how
many are waiting, never "up to date".

**34.10 — Sync now proves it ran.** Thread `SyncSource` from `sync_once` (`engine.rs:1923-1985`)
into `Provenance` at both construction sites (`engine.rs:1559`, `engine.rs:1184`) per AD-34-12,
so `Keeper-Source` stops saying `watch` for a manual sync. Surface `SyncOutcomeVm` in
`syncProfileNow` (`sync.ts:276-280`) and render a result — committed, pushed, pulled, files
changed, conflicts, bytes — at both call sites (`sync-pane.tsx:636-645`,
`sync-section.tsx:205-214`), including the honest "nothing to do" case. Stamp `last_sync_ms`
outside `do_push` (`engine.rs:1384-1386`) so a pull-only or push-nothing sync still records that
it succeeded. Acceptance: clicking Sync now always produces a visible statement of what happened,
and `git log` on a manually synced folder shows `Keeper-Source: manual`.

## Out of scope

- Deleting a profile's `.git` or working tree. `remove_profile` is a configuration change by
  explicit design (`sync_ipc.rs:499-500`) and stays that way.
- Templating the provenance trailer block.
- `sync_open_path` (specified in epic 32 story 32.4, never shipped). Real gap, not this epic.
- Per-file byte progress inside a single file's transfer. The rate in 34.8 is aggregate.
