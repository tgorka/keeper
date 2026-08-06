# Epic 40 Context: A recording lands where you can find it

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Today a recording session gets a flat, unchangeable folder name under the destination root, which
does not scale (a daily recorder passes a thousand sibling folders within a few years) and does not
sort. This epic replaces that fixed name with a user-editable **path template** that renders the
whole relative path under the root — date-first and nested by year by default — so sessions sort
chronologically in Finder, `ls`, the Files app and `git log` with no metadata read anywhere. It also
gives every session an immutable identity separate from its folder name, so a session can later be
retitled and moved without becoming a different thing. Sorting is the cheapest search index that
exists and it works on machines this app will never run on; the identity is what every downstream
epic in this phase keys its data on.

## Stories

- Story 40.1: The path template, rendered purely
- Story 40.2: The template is a setting, and the preview is the manual
- Story 40.3: `recording_start` uses it, and the session gets an identity
- Story 40.4: Retitle moves the folder, not the identity

## Requirements & Constraints

- The session folder name becomes a path template over the tokens
  `{yyyy} {yy} {mm} {dd} {HH} {MM} {SS} {title} {slug} {seq}`, rendering a *relative path* (it may
  contain `/`) beneath the destination root. Year nesting is not a switch — it is what the default
  template happens to say, and month nesting or per-client folders are template edits the user can
  already make. Resist re-introducing checkboxes for any of these.
- Default template: `{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}`.
- The rendered path must be legal on the intersection of APFS, exFAT and NTFS, because a FAT
  pendrive is a genuine destination once removable-volume sync exists: no `:` anywhere (which rules
  out `HH:MM`), never absolute, no `..` or `.` component, no empty component, no leading or trailing
  separator or space in any component. An empty title makes `{slug}` collapse *together with its
  adjacent separator* — no dangling separator, no "Untitled" placeholder.
- Templates are **validated, never sanitised**. An invalid template is rejected at edit time with
  the reason shown inline; record time never has to make a naming decision or silently rewrite the
  user's intent into a path they did not ask for.
- A rendered path that already exists gains a numeric collision suffix. Two sessions started inside
  the same minute with the same title must never share a folder.
- Every session carries an immutable, device-scoped identity (device id + ULID) that survives
  retitles and moves — device scoping is what stops two machines recording into one synced folder in
  the same minute from colliding. The manifest holds **no absolute paths**; everything it records is
  relative to the session folder, so a session folder is portable between machines.
- A session can be retitled after recording completes: the folder moves, the manifest's title is
  rewritten, the identity is untouched, and inside a synced tree the move is done such that git
  records a rename rather than a delete plus an add.
- Purity gate: nothing here may make `keeper-core` depend on `tauri` or on `keeper-sync` — the
  `check:core-tauri-free` and `check:core-sync-free` checks must stay green.
- No new dependencies. Everything needed is already vendored.

## Technical Decisions

- **The renderer is pure and lives in `keeper-core`**: a function of (template, civil datetime,
  optional title, collision sequence). No clock, no filesystem, no ambient state. The shell supplies
  the clock, the filesystem and the retry loop.
- **The token vocabulary is shared, not duplicated.** It is the same vocabulary `keeper-sync`
  already publishes for its journal template — one convention across notes and recordings, one
  documented token table, and the recording-side doc should say so explicitly so the two never drift.
- **The template is a setting alongside the existing destination root**, stored in the same settings
  key/value table and travelling the same wires — which means it inherits the existing file-based
  config override with no extra plumbing. Adding a sibling key should require no new storage,
  no new command family and no parallel validator.
- **Validation surfaces as a typed error carrying the parse reason**, not a boolean — the UI must be
  able to name the specific fault.
- `{slug}` is the title slugified; `{title}` is the title with only illegal characters removed.
  Neither may ever introduce more path components than the template's own separators — a hostile
  title cannot deepen or escape the path.
- **Path choice is host-side and Rust-only.** The session folder is created by the shell and one
  absolute path is handed to the capture sidecar, which only derives sibling segment names from it —
  so this epic touches no Swift and no sidecar protocol.
- Pre-flight failure honesty: an unwritable rendered path fails *before* the sidecar is launched, the
  error names the path that was tried, and no partial folder is left behind.
- A retitle is refused while that session is still recording, because the running driver holds
  absolute paths.

## UX & Interaction Patterns

- The live rendered preview **is the documentation**. The template field shows what path would be
  used right now, rendered against the current template and the current title box, updating as the
  user types and without writing the setting until save. Do not compensate with a help panel.
- The destination surface resolves to **one line of truth**: the absolute path the next recording
  will actually use. Paths render in the mono face.
- An invalid template disables save and names the fault inline, in place — never a modal, never a
  failure deferred to record time.
- Clearing the field restores the documented default rather than storing an empty template.
- Retitling is initiated from the session's metadata card, and only for completed sessions.

## Cross-Story Dependencies

- 40.1 is the foundation: 40.2 (settings + preview) and 40.3 (`recording_start`) both depend on it,
  and can proceed in parallel once it lands.
- 40.4 depends on 40.3 for the identity and the rendered-path plumbing it re-renders against.
- 40.2 and 40.4 cross the IPC boundary and regenerate TypeScript bindings; 40.1 and 40.3 do not.
- Downstream: this epic gates the rest of the phase — the sync-destination work resolves a
  destination that only exists once the template setting does, and every archive row is keyed by the
  session identity minted in 40.3.
- Deliberately out of scope: choosing a sync profile or any sync behaviour, any archive row, search
  index or browser, per-tag routing, and migrating existing session folders. Old sessions stay
  exactly where they are; the template governs new sessions only, and nothing resolves a session by
  its folder name once the identity lands.
