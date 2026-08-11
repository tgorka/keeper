# Spec 45.5 — A File Says What It Is and How Big

status: implemented
story: Epic 45, Story 45.5
bindings: FR-178, UX-DR68, AD-87 (one viewer registry), AD-65 (Rust joins paths), AD-40 (keeper-sync is keeper-core-free)
crates: `keeper-core`, `keeper-sync`, `keeper` (shell, uncompilable on Linux)
frontend: `src/lib/file-size.ts`, `src/components/layout/files-pane.tsx`, two chat surfaces

---

## What this story turned out to be about

Two sentences in the epic, and both of them were about **a value that already existed and
nobody applied**, which is now the seventh time this epic family has found that.

1. The glyph table the story asks for **was already in `files-pane.tsx`**, added by 43.8 — and it
   was keyed on the five-value attachment vocabulary, so a CSV, a Rust source file, a PDF and an
   executable all drew the same blank page. The pane could tell a video from an image and could
   not tell a spreadsheet from a binary. Retiring it into 45.2's registry is the story.
2. The size was **not** on the wire — see "What I found on the wire" — but the *bytes* were
   already being `stat`ed on every listed entry and thrown away.

And one thing the story did not ask for but could not be done without: keeper had **six**
byte formatters and they disagreed.

---

## What I found on the wire (the 44.11 check)

The brief asked whether the size was already shipping unread, the way `truncated` was from 43.8
until 44.11. **It was not.** Reported here as a negative finding with the evidence, because
"we checked" is only worth something if it says what was checked.

| Carrier | Field | Verdict |
| --- | --- | --- |
| `keeper_sync::browse::BrowseEntry` | `name`, `relative_path`, `absolute_path`, `is_dir`, `sync` | **No size.** Added by this story. |
| `keeper_core::vm::FilesEntryVm` | `name`, `relativePath`, `absolutePath`, `kind`, `sync` | **No size, no role.** Both added by this story. |
| `keeper_core::vm::FilesListingVm` | `truncated` | Present since 43.8, unread until 44.11, read now. |
| `sync_ipc.rs` delivery rows | `size_bytes: Option<u64>` | **Present and read** — but this is a git-LFS *delivery* row, a different object about a different question. Not reusable. |
| `RecordingSummaryVm` | `total_bytes` | Present and read, summed per recording session. Not a dirent size. |

What *was* already being paid for and discarded: `browse::list_resolved` called
`std::fs::metadata(&absolute_path)` on every entry to answer `is_dir` and dropped the
`Metadata` immediately. The size now comes off that same call — **the listing makes exactly the
same number of syscalls it made before this story.**

Also already present and now applied: `SyncProfile.notes.subfolder` and
`SyncProfile.recordings.subfolder`, both stored in the sync profile JSON since epics 35 and 41,
both reachable from `files_listing_vm`'s `&SyncProfile`, and neither previously consulted by any
listing. Nothing new had to be plumbed to answer "which of these folders is my vault".

---

## The six formatters, by name and location

The story says "computed once in Rust so every surface agrees on whether 1 kB is 1000 or 1024".
It was worse than that: nobody agreed, and two surfaces were printing wrong numbers.

| # | Location | Base | Prints | Status |
| --- | --- | --- | --- | --- |
| 1 | `keeper-core/src/error.rs::format_gb` | 1000 | always `GB`, 1 dp, **rounds** | **Left. DW below.** |
| 2 | `keeper/src/tray.rs::format_size` | 1000 | `MB`/`GB` only, truncates | **Left. DW below.** |
| 3 | `keeper-sync/src/progress.rs::format_bytes` | **1024** | `B`/`KB`/`MB`/`GB` | **Left. DW below (AD-40).** |
| 4 | `keeper-syncd/src/commands.rs::format_bytes` | **1024** | `B`/`KB`/`MB`/`GB` | **Left. DW below (AD-40).** |
| 5 | `src/components/chat/composer.tsx::formatSize` | **1024** | `B`/`KB`/`MB`/`GB`/`TB` | **Deleted; now calls the mirror.** |
| 6 | `src/components/chat/media-attachment.tsx::formatSize` | **1024** | same | **Deleted; now calls the mirror.** |

Six became two. The two that remain — `keeper_core::size::format_file_size` and its TypeScript
mirror — **cannot disagree silently**, because both test suites load the same checked-in vector
table.

### The base, and why

**Decimal. 1 kB = 1000 bytes.** macOS Finder has reported decimal sizes since 10.6, keeper is a
macOS-first application, and the number keeper puts beside a file must be the number the
operating system puts beside the same file — otherwise the user concludes one of them is lying,
and they are right. Units are SI: `bytes`, `kB`, `MB`, `GB`, `TB`, `PB`, `EB`. Lowercase `k`,
because `KB` is not a unit and `KiB` is the one this module deliberately never produces.

Stated in the UI, because the choice is visible in the number: `FILES_SIZE_BASE_NOTE` —
*"Sizes are decimal: 1 kB is 1000 bytes, the same as Finder."* — is the `title` on every size
cell, alongside the exact byte count.

### Integer, truncating, no floats

`u64` above 2^53 loses precision as `f64`, and `{:.1}` rounds — which is how a 999 999-byte file
becomes `"1000.0 kB"`, a string whose figure cannot occur in its stated unit. The divisor is
divided before dividing by it (`bytes / (divisor / 10)`, never `bytes * 10 / divisor`) so a count
near `u64::MAX` cannot overflow. `u64::MAX` renders `"18 EB"`.

---

## I/O matrix — `keeper_core::size::format_file_size`

Every row below is in `src-tauri/crates/keeper-core/src/file-size-vectors.json` and is asserted by
**both** the Rust unit test and the Vitest mirror test.

| Bytes | Renders | Why this row is in the table |
| --- | --- | --- |
| 0 | `0 bytes` | An empty **file** exists and is worth saying so about. |
| 1 | `1 byte` | The singular. |
| 2 | `2 bytes` | The plural resumes immediately. |
| 999 | `999 bytes` | One below the base: still exact, still a word. |
| 1 000 | `1.0 kB` | The base itself. |
| 1 023 | `1.0 kB` | One below the *binary* boundary, which is not a boundary here. |
| 1 024 | `1.0 kB` | 1024 bytes is 1.024 kB, not 1 KiB. This row is the whole decision. |
| 1 500 | `1.5 kB` | One decimal place below ten. |
| 1 999 | `1.9 kB` | Truncation: 1.999 is 1.9, never 2.0. |
| 9 999 | `9.9 kB` | Last value that keeps a decimal. |
| 10 000 | `10 kB` | At ten the decimal is dropped. |
| 999 999 | `999 kB` | A rounding implementation prints the impossible `1000.0 kB` here. |
| 1 000 000 | `1.0 MB` | The MB step. |
| 1 048 576 | `1.0 MB` | One mebibyte — the old chat value; right string, wrong reason. |
| 1 500 000 | `1.5 MB` | **The discriminator.** A 1024-based formatter says `1.4 MB`. |
| 999 999 999 | `999 MB` | One below the GB step. |
| 1 000 000 000 | `1.0 GB` | The GB step. |
| 2 500 000 000 | `2.5 GB` | Big enough to need GB. |
| 5 000 000 000 | `5.0 GB` | Recording-sized. |
| 12 300 000 000 | `12 GB` | At and above ten, no decimal, in a unit above kB. |
| 999 999 999 999 | `999 GB` | One below the TB step. |
| 10^12 / 10^15 / 10^18 | `1.0 TB` / `1.0 PB` / `1.0 EB` | Each rung of the ladder. |
| 18 446 744 073 709 551 615 | `18 EB` | `u64::MAX`. Overflow guard, and the top rung's reason. |

### Edge cases handled outside the table

| Input | Behaviour | Why |
| --- | --- | --- |
| A **directory** | Never reaches the formatter. `FilesEntryVm.size` is `None`, the pane renders nothing. | A folder showing `0 bytes` is false for every folder with anything in it, and an honest total needs a recursive walk a listing must never do. `FilesEntryVm::new` DISCARDS a size offered for a directory, so a caller cannot leak one. |
| Metadata unreadable (broken symlink, file removed between `read_dir` and `stat`) | `size: null`, renders nothing | Unknown and absent render identically, which is correct: in both cases keeper does not know. |
| A fifo / socket / device node | `size: null` | `is_file()`, not `!is_dir()`: its length is not a number of bytes anyone can read out of it. |
| Negative / `NaN` / `Infinity` (TypeScript only) | `0 bytes` | `media.size` comes off a Matrix event's `info` block — remote data another client wrote. A `RangeError` out of `BigInt()` would blank a timeline. |
| Fractional (TypeScript only) | Truncated to an integer | Same reason. |

---

## I/O matrix — folder roles

`FilesFolderRoles { notes_subfolder, recordings_subfolder }` is borrowed for a whole listing and
resolved per entry by `role_of(relative_path, is_dir)`.

| Configured | Entry | `is_dir` | Role | Why |
| --- | --- | --- | --- | --- |
| notes = `Second Brain` | `Second Brain` | yes | `notesVault` | The configured folder. |
| recordings = `Clips` | `Clips` | yes | `recordings` | The configured folder. |
| notes = `Second Brain` | `10-notes` | yes | *none* | **keeper's own default vault name is not evidence of anything.** |
| notes = `notes` | `Notes` | yes | `notesVault` | APFS/HFS+ are case-insensitive; a case-sensitive compare drops the marker with no way for the user to see why. |
| notes = `/notes` `notes/` `\notes` `NOTES` | `Notes` | yes | `notesVault` | The stored value is whatever the settings form accepted; normalised once, in one function. |
| notes = `work/notes` | `work/notes` | yes | `notesVault` | A nested vault is still the vault. |
| notes = `notes` | `notes/daily` | yes | *none* | A folder **inside** the vault is an ordinary folder — marking every descendant makes the marker useless at the depth people scan. |
| notes = `notes` | `archive/notes` | yes | *none* | Exact whole-path match, not a name match. |
| notes = `Second Brain` | `Second Brain` | **no** | *none* | A **file** named like the vault is not the vault. |
| notes = `""` | `""` | yes | *none* | The profile root is not a vault. (`NotesConfig::validate` refuses an empty subfolder; this is defence at the second layer.) |
| nothing configured | anything | yes | *none* | Not a vault, not a recordings root. |
| both configured identically | that folder | yes | `notesVault` | A misconfiguration the settings form should refuse; if one reaches here it must give **one deterministic answer** rather than whichever branch was written first. |

### Why the recordings role reads the profile's `recordings.subfolder` and not the settings key

The brief says "the recordings destination is in settings". It is — but the setting
(`recording.destination_profile_id`) chooses **which profile** receives new recordings, while
`RecordingsConfig` on a profile means "this synced folder holds recordings". The icon answers
*this folder holds recordings*, which is true of a flagged profile's subfolder whether or not it
is today's chosen destination (it holds the ones already there, and the ones another device
wrote). Reading the settings key instead would un-mark a folder full of recordings the moment the
user pointed new captures elsewhere. Both are configuration; this is the one that answers the
question the icon asks. The plain-folder alternative (`recording.destination_dir` pointing inside
a synced tree) cannot arise — the settings command already refuses it.

---

## The icon seam

`resolveViewer({ name, kind }).icon` → `IconName` → `VIEWER_ICON: Record<IconName, LucideIcon>`.

- The pane **never switches on an extension**. It cannot: `resolveViewer` requires `kind`, and
  `kind` comes from Rust.
- `VIEWER_ICON` is keyed on the registry's `IconName` union, which preserves the property 43.8's
  doc comment was written for: a name added to the registry fails **this file** to compile rather
  than rendering an empty cell.
- A configured folder role **overrides** the glyph, including while the folder is open. The
  obvious implementation branches on open/closed first and loses the marker at exactly the moment
  someone is inside the vault wondering whether they are in it; there is a test for that.
- I asked W1Registry to narrow `resolveViewer` to `ViewerSubject = Pick<ViewerFile, "name"|"kind">`
  and they did. The alternative was fabricating a full `ViewerFile` per row per render including
  an `openWith` closure never called — and an `openWith: null` written "just for the icon" is a
  lie the next reader takes literally.

### A trap worth writing down

`lucide-react` v1 **aliases** several icon names, and the alias renders a class that does not
match the identifier: `FileVideo` draws `lucide-file-play`, `FileAudio` draws
`lucide-file-headphone`, `FileJson` draws `lucide-file-braces`, `FileQuestion` draws
`lucide-file-question-mark`. The table now imports the canonical names, so what a test sees
matches what the code says.

---

## Accessibility

`aria-label` on a `treeitem` **replaces** its subtree's contribution to the name, so a size or a
vault marker rendered only as a child is visible and unspeakable — 44.11 found this for the count.
`aria-describedby` is now a **list**: count, size, role, in that order. The role's words
(`"Your notes vault"`, `"Where recordings are saved"`) are `sr-only`, because the glyph is already
the visible form of the same fact. `countOf()` in the test file was reading `aria-describedby` as
a single id; it now resolves the list and selects by `data-slot`.

---

## Tests, and the mutation table

Baseline green **before and after** at exactly the verdict's scope, keyed per command.

- Rust core: `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib -- size:: <8 named vm tests>`
- Rust sync: `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync --lib browse::`
- TS (pane): `bun run test src/components/layout/files-pane.test.tsx -t "what it is and how big"`
- TS (own): `bun run test src/lib/file-size.test.ts src/components/chat/media-attachment.test.tsx`

The pane command is filtered to this story's describe block because `files-pane.test.tsx` is
shared with 45.1 and 45.3, whose assertions were in flight. **A baseline that is not green at the
scope of the verdict licenses nothing**, so the verdict scope is exactly what was baselined.

**18 mutations, 18 caught, 0 survived, 0 unproved.**

| # | Mutation | Caught by |
| --- | --- | --- |
| R1 | Ladder base `1_000` → `1_024` | `the_base_is_1000_and_the_boundary_is_where_it_says`; `every_shared_vector_matches…`; `precision_drops_at_ten…` |
| R2 | Drop the singular (`1 byte` → `1 bytes`) | `small_counts_are_exact_and_the_singular_is_singular`; `every_shared_vector_matches…` |
| R3 | Round instead of truncate | `the_figure_never_rounds_up_past_its_own_unit`; `a_bigger_file_never_renders_in_a_smaller_unit`; +2 |
| R4 | `bytes * 10 / divisor` (overflow form) | `the_largest_possible_count_still_renders_as_a_size`; `every_shared_vector_matches…` |
| R5 | `whole < 10` → `whole < 100` (decimal never drops) | `precision_drops_at_ten…`; `large_files_reach_the_units_they_need`; +2 |
| R6 | Remove the `EB` rung | suite red (`the_largest_possible_count…`) |
| R7 | Directory keeps its `size_bytes` | `a_directory_has_no_size_even_when_one_is_offered` |
| R8 | `role_of` ignores the dirent | `the_vault_and_the_recordings_folder_come_from_configuration_not_from_a_name` |
| R9 | Role compare becomes case-sensitive | `the_role_normalises_the_configured_subfolder…` |
| R10 | Empty configured subfolder matches | `the_role_normalises_the_configured_subfolder…` |
| R11 | Role matches by prefix instead of exactly | `the_role_normalises_the_configured_subfolder…` |
| R12 | `browse` gives directories a size | `a_file_carries_its_byte_count_and_a_folder_carries_none` |
| T1 | Mirror base `1_000n` → `1_024n` | `matches keeper-core on every shared vector`; `is decimal, and never spells a binary unit`; `shows the size Rust computed…` |
| T2 | Mirror drops the singular | `spells small counts out, with a singular byte`; `matches keeper-core on every shared vector` |
| T3 | Pane renders a size for a folder | `gives a folder no size, and never renders it as zero` |
| T4 | Pane takes the icon from `kind` instead of the registry | `takes a file's glyph from the viewer registry rather than from its kind` |
| T5 | Pane ignores `folderRole` | `marks the vault and the recordings folder from configuration, not from a name`; `keeps the vault's marker while the vault is open` |
| T6 | Pane derives the role from the literal name `10-notes` | same two |

T1 and T2 being caught by **both** suites is the pinning working: a change to the mirror alone
fails against the shared vector table.

### Two harness findings worth passing on

1. **My harness had a gate bug that the sweep itself exposed.** It recorded one baseline result
   per *scope*; a scope running two commands let the second overwrite a red first, so "refuse if
   the baseline is not green" saw green and ran anyway. The first sweep's TS verdicts were
   therefore unlicensed and were re-run. Keyed per command now.
2. **A mutant is indistinguishable from a bug to everyone else in a shared worktree.**
   W1FilesWrite reported one of my `role_of` tests failing; it was mutant R10, live for ninety
   seconds. I now broadcast before and after a sweep. Post-sweep I re-grepped every anchor by
   name and re-ran the ts-rs export tests — `FileSizeVm.ts` and `FilesFolderRoleVm.ts` are
   correct and no `KiB`/`1_024` reached `src/lib/ipc/gen/`.

---

## Deliberately NOT done

- **The four remaining formatters were not migrated.** Each is a DW entry below with the reason.
  Migrating any of them silently changes user-visible text in a subsystem this story is nowhere
  near.
- **`kind_for_file_name` was not widened.** 45.2 established that the extension refinement lives
  inside kind `file` and proved the tables disjoint. A sixth kind for "notes vault folder" would
  have put a *machine-local* fact into a classifier that answers the same on every machine.
- **The folder role is not a `RecordingNoteTargetKind` variant**, for the same reason, and it is
  not in 45.2's registry table either — agreed with W1Registry. It is an overlay.
- **No recursive folder size.** A listing must never walk a tree; a folder's size is absent, not
  slow.
- **No sorting by size, no size column header, no toggle to binary units.** One base, stated.
- **`entry.size.bytes` is carried but only used for the tooltip.** It is there for a sort or a
  threshold a later viewer applies; it is not dead, but nothing computes with it today.
- **The `as FilesEntryVm` cast in the test file's `entry()` helper was left.** It is the file's
  existing convention (`profile()` does the same) and removing it is a shared-file change that
  belongs to whoever owns that helper next. It does mean a future required field can be forgotten
  in the fixture without a type error — noted rather than fixed by me mid-wave.
- **No logging.** Nothing in this story declines to act. A directory with no size and a file with
  unreadable metadata are *designed absences* rendered as nothing, not refusals; an unreadable
  dirent is already `continue`d silently by `browse` and logging one per row would be a
  high-cardinality line in the hot listing path. The one candidate — "your vault is configured as
  `X` but no such folder is here" — is not reportable honestly from a listing, because the vault
  is created lazily and the listing may be of a different subdirectory.

## Deferred work (DW) entries owed

- **DW-45.5-a — `keeper-sync/src/progress.rs::format_bytes`.** Divides by 1024 and prints
  `KB`/`MB`/`GB`. A 1 500 000-byte transfer shows `1.4 MB` in the tray where Finder shows
  `1.5 MB`. Cannot call `keeper_core::size` — **AD-40 makes `keeper-sync` `keeper-core`-free**, and
  a crate-independence rule that survives only until something small wants to cross it is not a
  rule. Fix is to move the formatter to a shared leaf crate or duplicate the decimal arithmetic
  with a pinned vector test.
- **DW-45.5-b — `keeper-syncd/src/commands.rs::format_bytes`.** Identical defect, identical
  AD-40 blocker.
- **DW-45.5-c — `keeper/src/tray.rs::format_size`.** Already decimal, but floors at MB
  (`format_size(0)` is `"0 MB"`, and 400 bytes written shows as `"0 MB"`). Migrating would change
  asserted tray copy, and `keeper` **does not compile on Linux**, so the change could not be
  verified here at all.
- **DW-45.5-d — `keeper-core/src/error.rs::format_gb`.** Decimal, but **rounds** where this story
  truncates (9 GiB → `"9.7 GB"` there, `"9.6 GB"` here) and deliberately says `GB` even for
  500 MB, because it words a disk-space rejection. Migrating would silently change asserted
  message text in an unrelated subsystem. Whoever owns those messages should decide.

## Behaviour changes outside this epic's area

**Two chat surfaces now display different numbers than before this story**, and this belongs in a
spec rather than in a bug report:

- `src/components/chat/composer.tsx` — the pending-attachment chip's size.
- `src/components/chat/media-attachment.tsx` — the file-bubble's `mimetype · size` line.

Both divided by 1024 and printed `MB`. A 1 500 000-byte attachment read `1.4 MB` and now reads
`1.5 MB`. `media-attachment.test.tsx`'s fixture was 1 048 576 bytes with the comment
*"1 MiB rendered as a human size"* — a case that produced the right string for the wrong reason
and so could never have caught the defect. It is now 1 500 000 bytes, which distinguishes the two
bases, and the comment says why.

`src/lib/file-size.ts` exists **only** for bytes that never pass through Rust — the composer
formats a `File` the user just picked in a dialog. Anything with a view model reads
`size.label`.

---

## What I could not verify here, and why

- **The shell crate does not build on Linux** (`glib-sys`, no `pkg-config`). So
  `keeper/src/sync_ipc.rs::files_listing_vm` — the eleven lines that read `profile.notes.subfolder`
  and `profile.recordings.subfolder` into `FilesFolderRoles` and pass `entry.size_bytes` through —
  **was never compiled by me.** Every decision it carries is in `keeper-core` and is tested there
  (AD-55/AD-56): the role rule is `FilesFolderRoles::role_of`, the directory rule is inside
  `FilesEntryVm::new`, the formatting is `keeper_core::size`. The shell is a field read and a call.
  It needs a macOS compile before it can be believed.
- **No real macOS filesystem was exercised.** The case-insensitive role compare is justified by
  APFS/HFS+ behaviour I did not observe here; the Linux test asserts the *comparison* is
  case-insensitive, not that macOS resolves the two spellings to one directory.
- **The icon glyphs were verified by class name in jsdom, not by eye.** That a `.csv` now draws a
  spreadsheet rather than a blank page is proved; that the chosen glyphs read well at 16px beside
  each other in the real pane is not. `file-document` for a PDF (lucide `FileType`) is the one I
  am least sure of visually.
- **DW-45.5-c and DW-45.5-d could not be attempted**, not merely deferred: `tray.rs` is in the
  uncompilable crate, and `error.rs`'s rounding difference means the migration is a behaviour
  change requiring its owner's judgement rather than a mechanical edit.
- **The size cell's `title` was not checked against a screen reader or a real hover.** The
  `sr-only` role text and the `aria-describedby` list are asserted in jsdom, which is a claim
  about the accessibility *tree*, not about what VoiceOver actually says.
- **`bun run lint`, `bun run typecheck` (project-wide), `cargo clippy` and `cargo fmt` were not
  run** — the wave gates once at the end. I did run `bunx tsc --noEmit` filtered to my four files:
  clean. The only error in `files-pane.test.tsx` is an unused `FILES_CANCEL_LABEL` import
  belonging to 45.3.
