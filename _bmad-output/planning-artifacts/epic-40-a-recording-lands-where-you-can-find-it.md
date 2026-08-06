# Epic 40 — A recording lands where you can find it

status: draft
created: 2026-08-05
altitude: epic
parent: Phase 6 (Recording × Sync), Epic 19 (destination chooser), Epic 21/22 (session metadata)
source: `product-inputs-recording-sync-2026-08-05.md` (the numbering spine), the divergent session in
`brainstorm-recording-sync-archive-2026-08-05/`, and a read-only survey of `keeper/src/ipc.rs`'s
recording commands, `keeper-core/src/recording.rs` and the React recording settings surface
binds: FR-125–FR-129, FR-144, FR-145, AD-64, AD-65, AD-73, UX-DR45, UX-DR46

## Why this epic exists

Story 19.5 gave the owner a destination folder, and it has been enough to record into and not
enough to live in. Everything a session becomes afterwards — sorted, findable, syncable, prunable —
is decided by the one string nobody can currently change: the folder name. Today it is
`keeper-rec <ts>` (or `<title> <ts>`), flat under the root, forever.

Flat is a countdown. A daily recorder passes a thousand folders inside four years, and both Finder
and `git status` start paying for it long before the human does. Date-first is the other half:
`2026-08-05 1432 standup` sorts chronologically in Finder, in `ls`, in the Files app and in a
`git log --name-only` — with no metadata read anywhere. Sorting is not a nicety here; it is the
cheapest search index in existence, and it works on machines this app will never run on.

Three facts make this epic small:

1. **The path is chosen host-side, in Rust.** `recording_start` (`keeper/src/ipc.rs`) creates the
   session folder and hands the Swift sidecar one absolute path; the sidecar only derives sibling
   segment names from it. A new naming scheme touches no Swift and no sidecar protocol.
2. **The root already exists end to end.** `recording.destination_dir` lives in the `keeper.db`
   `settings` k/v table, surfaces as `RecordingSettingsVm.destinationDir`, and is edited by
   `RecordingDestinationControls`. This epic adds one sibling key and reuses every wire it travels.
3. **The token vocabulary already exists too.** `keeper-sync`'s `DEFAULT_JOURNAL_TEMPLATE` is
   `journal/{yyyy}/{yyyy}-{mm}-{dd}.md`. Recordings adopt that vocabulary verbatim; a user who has
   learned one has learned both.

### Where we take a position

**The template renders a path, not a name.** The obvious design is a name pattern plus a
"nest by year" checkbox, and it is wrong in a way that compounds: the next request is month
nesting, then per-client folders, then a checkbox matrix nobody can predict the output of. A
template that may contain `/` answers all of them and is *less* code — year nesting stops being a
feature and becomes the default template's opinion (FR-126). The preview under the field is then
the entire documentation (UX-DR45).

**The template is validated, not sanitised.** Silently rewriting a user's template into something
legal produces a path they did not ask for and cannot predict. An invalid template is rejected
inline, at edit time, with the reason — and record time never has to make a decision (FR-129).
Legality here means the union of what APFS, exFAT and NTFS accept, because removable-volume sync
makes a FAT pendrive a genuine destination: no `:`, which rules out the obvious `HH:MM`.

**Identity is not the folder name.** The moment a title can be edited, the folder name becomes a
label rather than an identity, so the session gets a device-scoped id that never changes (FR-145,
AD-73) and the folder becomes free to move. That is what makes story 40.4's retitle a `git mv`
instead of a data migration, and it is why the id lands in this epic rather than in the archive
epic that consumes it.

## Stories

### Story 40.1: The Path Template, Rendered Purely
**Rust-only (`keeper-core`), pure.** Bindings: no.

New `keeper-core/src/recording/path_template.rs`: `PathTemplate::parse(&str) -> Result<PathTemplate,
TemplateError>` over the tokens `{yyyy} {yy} {mm} {dd} {HH} {MM} {SS} {title} {slug} {seq}`, and
`PathTemplate::render(&RenderCtx) -> RelativePath` where `RenderCtx` carries the civil datetime, the
optional title and a collision sequence — no clock, no filesystem, no `tauri`, no `keeper-sync`
(NFR-35). `{slug}` is the title slugified; `{title}` is the title with only illegal characters
removed. Rendering guarantees FR-127: no `:` anywhere, no component that is empty, `.` or `..`, no
leading or trailing separator or space in any component, and a collapsed `{slug}` takes its
adjacent separator with it. `DEFAULT_TEMPLATE` is `{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}`.
A collapsed *interior* component vanishes with its separator; a collapsed **leaf** is refused at
parse (`TemplateError::OptionalLeaf`), because the rendered path is the session folder and a
vanishing leaf silently promotes the year directory into one — the collision ordinal then renames
that parent, and an explicit `{seq}` in a collapsible leaf makes session 2 a child of session 1.
The leaf must carry one always-rendering token or one literal character, and the rendered leaf
including its ordinal is capped at 255 bytes with the title truncated at a character boundary.
(Decided 2026-08-06 resolving the story 40.1 review escalation; FR-127 carries the same words.)
Document the token table in the module doc — it is the same table `keeper-sync` publishes for
`journal_template`, and the doc says so (AD-65).
AC: the default template with title "Standup" at 2026-08-05T14:32 renders exactly
`2026/2026-08-05 1432 standup`; the same template with no title renders `2026/2026-08-05 1432` with
no trailing space; a template containing `..`, an absolute prefix, or `{HH}:{MM}` is rejected at
parse with a distinct `TemplateError` variant each; a title of `"a/b:c"` never produces more path
components than the template's own separators; property test over 1 000 generated titles asserts
every rendered component is non-empty and free of the illegal set; `check:core-tauri-free` and
`check:core-sync-free` stay green.

### Story 40.2: The Template Is a Setting, and the Preview Is the Manual
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 40.1.

`recording.path_template` joins `recording.destination_dir` in the `keeper.db` `settings` k/v table
(and therefore inherits story 22.6's `config.json` override with no extra plumbing).
`RecordingSettingsVm` gains `pathTemplate: String`, and the settings command validates a submitted
template through `PathTemplate::parse`, returning a typed `IpcError` with the parse reason rather
than a boolean. `RecordingDestinationControls` gains the template field plus a live preview line
rendering *now* against the current template and the current title box (UX-DR45), and shows the
resolved absolute path the next recording would use (UX-DR46). An invalid template disables the
save and names the fault inline.
AC: `bun run bindings:check` passes with the regenerated `src/lib/ipc/gen/` tree; typing
`{yyyy}/{mm}/{dd} {slug}` updates the preview within one render and never writes the setting until
save; submitting `../{yyyy}` is rejected with the parse reason visible in the form and the stored
value unchanged; setting `recording.path_template` through `config.json` and restarting shows the
override in the form; clearing the field restores the documented default rather than an empty
template.

### Story 40.3: `recording_start` Uses It, and the Session Gets an Identity
**Rust-only (`keeper` shell).** Bindings: no. Depends on 40.1.

`recording_start` (`keeper/src/ipc.rs`) renders the template instead of formatting a name,
`create_dir_all`s the rendered relative path under the destination root, and retries with an
incremented `{seq}` on collision (FR-128). The session gains `session_id` — device id plus ULID
(AD-73, FR-145) — minted here and carried in `SessionMeta`, and the manifest records only paths
relative to the session folder (FR-145). The pre-flight failure modes stay honest: an unwritable
rendered path fails before the sidecar is launched, naming the path it tried.
AC: two sessions started inside the same minute with the same title produce two folders, the second
carrying the collision suffix, and two distinct session ids; the manifest of a session moved to
another machine contains no absolute path, asserted by a test that greps the serialized form for
the destination root; a destination root that is read-only fails `recording_start` with a typed
error naming the rendered path, and no partial folder is left behind; recording into the default
template produces `<root>/2026/…` and the year directory is created on demand.

### Story 40.4: Retitle Moves the Folder, Not the Identity
**Rust + frontend.** Bindings: **yes**. Depends on 40.3. Binds FR-144.

A completed session can be retitled from the session's metadata card. The rename renders the
template again with the new title, moves the folder (`fs::rename` within the same root), rewrites
`manifest.json`'s title while leaving `session_id` untouched, and — when the session lives inside a
synced profile — performs the move so that the next commit records a rename rather than a delete
plus an add. A rename that would collide gains the same `{seq}` suffix as a fresh session; a rename
attempted while that session is still recording is refused with a reason, because the driver holds
absolute paths.
AC: retitling a session moves the folder and leaves `session_id` byte-identical in the manifest;
`git log --follow` over the moved media reaches the pre-rename commits; retitling to a name that
renders to an existing path produces a suffixed folder and no data loss; retitling the active
session is refused with a typed error and the folder is untouched; retitling a session inside a
paused or offline profile succeeds locally and is picked up on the next sync.

## Out of scope

- Choosing the destination *profile* or any sync behaviour — that is epic 41 in full.
- Any archive row, search index or browser — epic 42.
- Per-tag routing to a different root, and month-depth defaults: both are template edits the user
  can already make once this epic ships, and neither earns a switch.
- Migrating existing session folders into the new layout. Old sessions stay exactly where they are;
  the template governs new sessions only, and nothing in the app resolves a session by its folder
  name once 40.3 lands.
