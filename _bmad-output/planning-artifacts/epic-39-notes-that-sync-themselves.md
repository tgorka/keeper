# Epic 39 — Notes that sync themselves

status: draft
created: 2026-08-02
altitude: epic
parent: Epic 36 (capture in two seconds), Epic 37 (a place to read and write),
Epic 38 (your agent writes here too)
source: `product-inputs-notes-2026-08-02.md` (the numbering spine), the divergent session in
`brainstorm-keeper-notes-2026-08-02/`, and a full read of the supervisor tick in
`keeper-sync/src/engine.rs`, `keeper/src/lifecycle.rs` and the window/capability surface
binds: FR-115, FR-120 (cadence knob), FR-124, AD-62, plus `docs/notes.md` and phase acceptance

## Why this epic exists

A note vault that syncs on the same 15-second-ish cadence as a photo folder is a note vault
that loses a thought when the laptop lid closes. Notes have a different rhythm from the rest of
sync: many tiny writes, seconds apart, from a human who expects the machine on the other desk
to have them by the time they walk over to it.

The temptation is a notes scheduler. There will not be one. AD-62 says cadence is a **profile
knob** consumed by the supervisor that already exists — the 1 Hz tick in `keeper-sync`'s engine
that already paces scans against `poll_interval_ms` with a 2-second floor. A notes profile
defaults to the short end of that existing range; nothing new schedules anything.

Two other things close the phase here. The sticky note (FR-124) is the phase's second Should-
tier item and is scheduled last for a reason beyond priority: it is the only story in the phase
that needs a **dynamically created** window, and the codebase has never built one. And then the
phase has to be provable — `docs/notes.md` and a cross-host run against every number in the
spine.

### The window that does not exist yet

There is no `WebviewWindowBuilder` anywhere in the codebase. Epic 36 added a second *static*
window in `tauri.conf.json`, which is the easy case: it is declared, so a capability file can
name its label. A sticky note cannot be declared, because there are N of them and their labels
carry a note ULID.

That matters more than it sounds. Capability files scope by window label
(`capabilities/default.json:5` is `"windows": ["main"]`), so a dynamically created window whose
label matches no capability file can invoke **nothing** — it renders and sits inert. Story 39.3
therefore ships a label pattern and a capability file matching it in the same change, and its
acceptance asserts the window can actually call a command rather than merely appearing.

### What "force-flush" has to mean

FR-115's cadence contract has four legs and they are not the same mechanism:

- **Idle-debounced local commit** — a short quiescence after typing stops, expressed as the
  profile's existing settle window, not a new timer.
- **Interval or on-blur push** — the supervisor's existing paced work, with a notes default.
- **Force-flush on window hide** — including the quick-capture panel's hide, which is the most
  common way a captured thought ends.
- **Force-flush on quit** — reusing the bounded graceful-finalize path story 30.5 already
  defined and the app's quit path already runs.

Story 39.1 owns the first two, story 39.2 the last two, because the first pair is a knob the
engine reads and the second pair is a lifecycle call site.

## Stories

### Story 39.1: Cadence Is a Profile Knob
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 35.1, 36.6.

`NotesConfig` (story 35.1) gains the cadence fields — idle-commit debounce and push interval —
each `#[serde(default)]`, and the supervisor consumes them as the per-profile cadence it
already consumes `poll_interval_ms` as, honouring the existing 2-second floor. Notes profiles
default on with the short cadence; a non-notes profile is unaffected. The knob is surfaced in
the per-vault settings from story 36.6, showing the value actually in force including the
floor's substitution (AD-34-8), which completes FR-120.
AC: no second scheduler, timer or task is introduced, asserted by a convention test over the
supervisor module; a notes profile commits an idle-settled note within its configured debounce
plus one tick, measured; a cadence below the floor is clamped and the form shows the clamped
value rather than the requested one; a non-notes profile's cadence is byte-identical to its
0.6.5 behaviour; `bun run bindings:check` passes.

### Story 39.2: Flush on Hide, and on Quit
**Rust-only (`keeper` shell).** Bindings: no. Depends on 39.1.

`keeper/src/lifecycle.rs`: hiding the main window and hiding the quick-capture panel each force
a flush of the affected notes profile through the supervisor rather than waiting for its next
paced turn, and the quit path runs the same flush inside the existing bounded graceful-finalize
before it tears the engine down. A flush that cannot complete inside the bound leaves the work
journaled — it is queued, not dropped, which is what makes this safe to bound at all.
AC: type into a note, hide the window, and the commit exists in `git log` within the flush
bound; capture text, press Escape, and the note is committed without waiting for the push
interval; quit mid-typing and relaunch — the content is in the vault and in a commit; a flush
that exceeds the bound leaves a journal entry that the next start drains, and loses nothing
(NFR-30).

### Story 39.3: The Sticky Note
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 37.6, 39.2.

The app's first `WebviewWindowBuilder`: a note tears off into a `note-sticky-<ulid>` window —
small, always-on-top, undecorated, skip-taskbar — and several may live at once. A
`keeper/capabilities/note-sticky.json` scopes to the label pattern with the least-privilege set
the sticky needs (the body channel from story 37.6, the write command, nothing else). Per-note
geometry persists so a sticky reopens where it was. A sticky and the main window editing the
same note are two buffers over one file, and story 38.2's merge is what makes that safe — the
sticky is not a special case in the write path.
AC: three stickies open at once, are individually movable, and reopen in place after a restart;
a sticky can invoke its commands — the assertion that proves the capability file matches the
label pattern; typing in a sticky while the same note is open in the main window converges with
no prompt and no lost characters; closing a sticky does not close or unload the note in the
main window; on Linux the stickies float above other applications and take no taskbar slots.

### Story 39.4: `docs/notes.md`
**Documentation.** Bindings: no. Depends on 39.3, and on every epic's measured output.

The durable operator document, in the shape `docs/sync.md` set: the vault layout and what each
directory means; the frontmatter keys keeper claims and the guarantee that it preserves every
key it does not; the Obsidian coexistence rules and their limits; the space query grammar with
a worked example an agent can copy; the cadence contract's four legs; the conflict model; the
Linux tray notes; and the measured envelopes from stories 35.6, 37.2, 37.5, 38.1 and 39.1 with
the vault sizes they were measured at.
AC: a reader can hand-author a vault, a template, a journal path and a space from this document
alone, verified by doing it against a clean install; every limitation is stated, including the
scan's measured ceiling and the fact that overlapping concurrent edits become conflict rows
rather than merges; no number in the document is unsourced.

### Story 39.5: Phase Acceptance
**Field validation.** Bindings: no. Depends on every prior story. **Human-in-the-loop:**
requires two physical hosts, one macOS and one Linux, against one remote.

The cross-host run: one vault synced between both machines, an agent writing on one side while
a human writes on the other, a capture on each, a conflict deliberately provoked, a rename
followed by a history read. Then the sweep — every FR-94 through FR-124 and every NFR-27
through NFR-30 checked off against a shipped story, with NFR-27, NFR-28 and NFR-29 re-measured
on both platforms and the numbers written back into `docs/notes.md`.
AC: both hosts converge on identical vault content with no conflict storm; the agent's writes
appear as unread with correct device and origin on the other host; NFR-27 (300 ms), NFR-28
(10 000-note cold index under 5 s, list under 100 ms) and NFR-29 (1 s) hold on both platforms
and the measurements are recorded; every FR and NFR in the phase maps to a shipped story, and
any that does not is raised as a gap rather than quietly marked done.

## Out of scope

- A notes-specific scheduler, queue or worker. AD-62 is a knob, and story 39.1's convention
  test enforces it.
- Sticky notes on iOS, or any notes surface there. The capability gate (FR-122, story 35.2)
  omits the whole surface.
- Capture-from-a-chat-message, table/board refinements beyond story 37.9, and the Could tier
  (calendar lens, transclusion, graph view, publish-to-room).
- Changing any non-notes profile's cadence behaviour. The knob is per profile and defaults to
  today's behaviour everywhere it is not set.
