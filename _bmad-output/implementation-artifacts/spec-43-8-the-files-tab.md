---
title: 'Story 43.8: The Files Tab'
type: 'feature'
created: '2026-08-09'
status: 'review'
blocking_condition: ''
baseline_revision: '9f7150d'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-43-a-note-can-show-you-the-file.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-3-the-recordings-browser.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-4-the-note-stub.md'
---

<intent-contract>

## Intent

**Problem:** keeper syncs a folder, records into it, keeps a notes vault in it, and has no surface
that will show you what is in it. The Sync pane answers "is this folder working"; nothing answers
"what is in it". So the owner's answer to every "where is that file" is still Finder.

**Approach:** one shell command that lists ONE directory of ONE profile, and a tree that asks for a
folder's children the first time it is opened. Listing is Rust's job end to end: the frontend hands
back a `subpath` Rust produced, Rust joins it onto the profile's own root, refuses anything that is
not a plain descendant, filters through the tier-0 exclusions sync already has, and answers with a
`state` that keeps "the drive is out" apart from "this folder is empty". The surface reveals, copies
a path and opens with the system handler, and does nothing else, on purpose.

## Boundaries & Constraints

**Always:**
- **Read-only twice over (AD-75).** No control writes, renames, moves or deletes a file — and
  nothing in the listing path touches `Engine`, the stability gate, `file_state`, the journal, the
  scan clock or the watcher. Looking is not an event. `browse()` takes a `&SyncProfile`, never an
  `&Engine`, so it *cannot* spend a gate verdict the way `scan_and_enqueue` and the notes cadence
  each once did.
- **No frontend surface joins a root and a subpath (AD-65).** The join happens in
  `keeper_sync::browse` and nowhere else; the shell only stringifies the result.
- **FR-145:** the only path a person ever sees is profile-relative. `absolutePath` crosses the wire
  as an action argument and is never rendered, and no absolute path is written anywhere.
- **`..` cannot escape the profile root, and the refusal is lexical first.** The lexical test runs
  before the disk is consulted, so an escape is refused identically whether the drive is in, out, or
  replaced — otherwise the refusal could be probed for by unplugging the media.
- **Both halves of the AD-59 containment idiom.** Lexical (every component exactly one
  `Component::Normal`) plus canonicalisation of both sides, which is the only thing that catches a
  symlink planted inside the folder.
- **Lazy.** One directory per call, children on expand, never a tree walk. These folders hold
  100 000 files and one of them is on a pendrive.
- **One exclusion corpus.** `keeper_sync::exclude::ExcludeSet`, not a second ignore rule.
- **One attachment vocabulary (AD-73).** `RecordingNoteTargetKind`, classified by 43.5's
  `kind_for_file_name`, with folder-ness taken from the dirent.
- Capability gating is **absence**, at the nav entry and at the render chain, on the same `sync`
  flag the Sync pane uses.

**Block If:**
- `bun run bindings:check` cannot run. It gates this story, and it is one of the gates that cannot
  run on Linux; the macOS host is the arbiter.

**Never:**
- No write path of any kind (AD-75).
- No bytes served. The Files tab lists and reveals; it does not stream, and it does not reach for
  `keeper-recording://`, whose root stays the recordings destination (AD-74).
- No second ignore rule, no second kind enum, no second containment rule.
- No engine mutation, including "just a rescan to notice now".

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Mount | any state | profiles listed, **zero** `sync_browse` calls | none |
| Enabled profiles | three profiles, one paused | the two enabled appear; the paused one does not | none |
| No profiles at all | fresh install | "No folders are set up yet…" | none |
| Every profile paused | all disabled | "Every folder is paused…" — a different sentence | none |
| Expand a profile | first click | one `sync_browse(id, "")` | none |
| Collapse and re-open | second and third click | no further call; the loaded listing is reused | none |
| Expand a child folder | click a folder row | `sync_browse(id, "2026")` — the subpath Rust handed back | none |
| Empty folder, fixed disk | `read_dir` returns nothing | `state: listed`, `entries: []`, "This folder is empty." | none |
| Removable profile, no marker | drive unplugged | `state: mediaAbsent`, `entries: null`, the reattach sentence | none |
| Removable profile, foreign marker | a different stick mounted there | `state: mediaUnexpected`, never listed | none |
| Folder moved outside keeper | root gone on a fixed disk | `state: missing`, the re-point sentence | none |
| Subfolder gone | child deleted between listings | `state: missing`, named by its own relative path | none |
| `..` in the subpath | `..`, `../..`, `a/..`, `/etc`, `.`, `a//b` | refused before the disk is read | `internal`, non-retriable |
| `..` with the drive out | removable profile, unplugged, subpath `../..` | refused as an escape, NOT reported as absent media | `internal` |
| Symlink out of the root | `escape -> /tmp/x` planted in the folder | refused after canonicalisation | `internal` |
| Symlink to a folder inside the root | ordinary case | expands like the folder it points at | none |
| Tier-0 noise | `.git`, `.keeper-sync`, `.keeper`, `node_modules`, `.DS_Store`, `*.partial` | absent from the listing | none |
| Profile exclude pattern | `*.tmp` on the profile | absent on top of the builtin corpus | none |
| Malformed profile pattern | a bad glob in `config.json` | refused with the pattern named | `internal` |
| Directory over the cap | 1005 entries | first 1000 plus a sentence saying it was cut short | none |
| Expanding a file | subpath names a file | `state: missing` — a file has no children | none |
| Unreadable directory | permissions | the OS's own words, verbatim; NOT "empty" | `internal` |
| Non-UTF-8 file name | Linux only | rendered lossily; every action re-resolves and refuses honestly | `internal` on action |
| Reveal | a row | `reveal_path(absolutePath)` | swallowed |
| Reveal where unsupported | `revealInFileManager` off | the action is **absent**, not disabled | none |
| Copy path | a row | the absolute path on the clipboard, transient confirmation | swallowed |
| Open | a file row | `sync_open_entry(id, relativePath)` — re-resolved in Rust | `internal` |
| Open on a folder row | — | the action is absent; expanding is what a folder does | none |
| Refresh | one folder open of two | only the open one is re-read | none |
| No sync capability | no usable `git` | the pane and its nav entry are ABSENT from the DOM | none |
| Any write control | `Rename`/`Delete`/`Move`/`New folder`/… | none exists, asserted by name | n/a |
| Tab into the tree | any state | exactly ONE row in the tab order, never one per row | none |
| Up / Down | focus on a row | one visible row at a time; the ends do not wrap | none |
| Home / End | focus on a row | first / last visible row | none |
| Right on a closed folder | folder row | expands and STAYS put — it does not also move | none |
| Right on an open folder | folder row | descends to its first child | none |
| Right on an open EMPTY folder | only a note below it | focus does not move — the next row is a sibling | none |
| Left on an open folder | folder row | collapses it | none |
| Left on a closed / leaf row | any child row | climbs to its parent | none |
| Enter / Space | folder row | toggles | none |
| Tab from the focused row | a file row with actions | walks that row's actions, then leaves the tree | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/browse.rs` (new) — `resolve`, `browse`, `BrowseEntry`,
  `BrowseDirectory`, `BrowseListing`, `BrowseRefusal`, `LISTING_CAP`. **Decision:** here, not in
  `keeper-core` and not in the shell. See Design Notes.
- `src-tauri/crates/keeper-sync/src/exclude.rs` — `ExcludeSet::is_excluded_directory`, plus the
  derived `dir_set` and the `add_glob` split that keeps a stripped pattern's anchoring.
- `src-tauri/crates/keeper-sync/src/lib.rs` — `pub mod browse;`.
- `src-tauri/crates/keeper-core/src/vm.rs` — `FilesListingState`, `FilesEntryVm` (+ `::new`, which
  is where the dirent meets 43.5's classifier), `FilesListingVm`.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — `sync_browse`, `sync_open_entry`, `files_listing_vm`,
  `missing_sentence`.
- `src-tauri/crates/keeper/src/lib.rs` — both commands in the single `invoke_handler` literal.
- `src/lib/ipc/client.ts` — `syncBrowse`, `syncOpenEntry`, and the three type re-exports.
- `src/lib/stores/primary-view.ts` — a `files` member.
- `src/components/layout/files-pane.tsx` (new) + its test.
- `src/components/layout/sidebar-pane.tsx` — the gated `Files` entry, and `SidebarView.view`.
- `src/components/layout/app-shell.tsx` — the gated render.

## Tasks & Acceptance

**Execution:**
- [x] `keeper-sync::browse` with both halves of the containment rule and the four listing states.
- [x] `ExcludeSet::is_excluded_directory`, derived from the one corpus.
- [x] The three VMs and the `FilesEntryVm::new` projection through 43.5's classifier.
- [x] `sync_browse` and `sync_open_entry`, registered, off the async runtime.
- [x] Client wrappers; bindings generated by ts-rs.
- [x] The pane: a lazy tree, four states kept apart, three read-only actions.
- [x] Capability gating at nav and render.
- [x] A roving-tabindex keyboard model (WAI-ARIA APG, Tree View) over a flat DOM.
- [x] Tests, and a revert proof for each central claim.

**Acceptance Criteria:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib` — **1259 passed**.
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync --lib` — **573 passed**.
- `bun run test src/components/layout/files-pane.test.tsx` — **23 passed**.
- `./node_modules/.bin/biome check src/components/layout/files-pane.tsx` — clean.
- `bun run test src/components/layout/sidebar-pane.test.tsx` — **40 passed**.
- `bun run test src/components/layout/app-shell.test.tsx` — **21 passed**.
- `bun run bindings:check`, `cargo clippy`, `cargo fmt --check` and `bun run lint`/`typecheck` —
  **not run here**; the shell crate does not build on this Linux box (see Verification).

## Verification

Every claim below was proved by removing the fix and watching the named test fail, then restoring
it. Files were byte-compared against their pre-mutation copies afterwards.

| Reverted | Test that caught it | Observed failure |
|---|---|---|
| Lexical containment in `resolve` (push each segment raw) | `parent_traversal_is_refused_at_every_position`, `absolute_and_degenerate_subpaths_are_refused`, `traversal_is_refused_even_when_the_media_is_absent` | `..` resolved; with the drive out it returned `Ok(MediaAbsent)` instead of refusing |
| The post-canonicalisation `starts_with` check | `a_symlink_out_of_the_root_is_refused_after_resolution` | `Ok(Some("/tmp/.tmpKu9jSU"))` — the planted symlink resolved outside the root |
| The removable-volume scan (`if false && profile.removable`) | `a_removable_profile_with_no_marker_reports_absent_media`, `a_foreign_marker_is_neither_listed_nor_reported_as_absent` | `Listed(entries: [])` — exactly the "your recordings are gone" lie the story exists to prevent |
| `is_excluded_directory` → `is_excluded` for directories | `tier_zero_noise_is_filtered_by_the_one_exclude_set`, `a_removable_profile_whose_volume_is_attached_lists_normally` | `.git` and `.keeper-sync` appeared in the listing |
| The derived `dir_set` (never compiled) | `a_directory_whose_whole_subtree_is_excluded_is_excluded_as_a_directory`, `a_profile_subtree_pattern_also_answers_the_directory_question` (+ both browse tests above) | the directory question answered `false` for `.git` and for a profile's `scratch/**` |
| The dirent deciding folder-ness (classify by name only) | `a_directory_is_a_folder_whatever_it_is_named` | a directory called `notes.md` classified as `File` |
| `entries: Option<Vec<_>>` → `Vec<_>` | `an_empty_listing_and_an_absent_drive_are_different_on_the_wire` | `mediaAbsent` serialized `"entries":[]`, indistinguishable from an empty folder |
| The pane's `entries === null` branch | `says the drive is out rather than showing an empty folder`, `keeps a foreign volume and a folder that moved apart from both` | the absent drive rendered "This folder is empty." |
| Laziness (load every profile root on mount) | `asks for a folder's children only when it is expanded, and only once`, `re-reads only the folders that are open when Refresh is pressed` | `sync_browse` fired before any expansion |
| Read-only (a `Delete` button added) | `offers no control that could write, rename, move or delete` | the AD-75 assertion named `Delete` |
| The `enabled` filter on the profile list | `lists every enabled profile and no paused one`, `distinguishes no folders at all from every folder paused` | a paused folder appeared as browsable |
| The `Files` nav entry | `places Files immediately after Sync, before Settings`, `switches the primary view to files, leaving Sync unmarked` | no entry, no route |
| The `sync &&` gate on the files view | `does not render the Files pane when the sync capability is off` | the pane rendered where sync cannot run |
| The row `onKeyDown` handler | all eight `FilesPane keyboard navigation` tests | every arrow, Home/End, Enter and Space became inert; focus never moved |
| The roving tabindex (`tabIndex={0}` on every row) | `puts exactly one row in the tab order and moves it with focus` | both rows reported `tabindex="0"` — a two-hundred-file folder becomes two hundred Tab presses |
| The child guard on Right (`nodes[index+1]?.parentKey === node.key`) | `does not let Right on an empty open folder look like it descended` | focus jumped to the SIBLING profile, so the tree lied about the hierarchy |

**What could NOT be verified on this machine, and why.**

`cargo check -p keeper` fails on Linux before it reaches any of my code: `gobject-sys` cannot find
`pkg-config`/GTK. So **`sync_browse`, `sync_open_entry`, `files_listing_vm` and `missing_sentence`
are the only code in this story that has never been compiled**, and their behaviour is asserted only
indirectly — through `keeper_sync::browse`, which they are a thin projection over, and through the
frontend test, which exercises the VM shape against a mock rather than against the real command.
The macOS gate is the arbiter for those four functions, for `cargo clippy`, for `cargo fmt --check`
and for `bindings:check`.

Two consequences worth naming rather than hiding:

- **The `..` escape assertion does run here.** It lives in `keeper-sync`, which builds on Linux, and
  it is asserted over a real temp directory with a real planted symlink. That was the reason for
  choosing `keeper-sync` over the shell as its home.
- **The frontend test proves the pane, not the shell.** "The absent-drive state is distinct from the
  empty-folder state" is proved twice — once on the wire in `keeper-core`
  (`"entries":null` vs `"entries":[]`) and once in the pane against a mocked client. What is *not*
  proved anywhere runnable is that the real `sync_browse` produces that wire shape; that is
  `files_listing_vm`, and it is unbuilt code.

The generated bindings (`FilesEntryVm.ts`, `FilesListingState.ts`, `FilesListingVm.ts`) were emitted
by ts-rs during `cargo test -p keeper-core` — never hand-written — and will be regenerated by the
macOS gate.

## Design Notes

**The listing lives in `keeper-sync`, and that is the load-bearing decision.** The obvious home was
the Tauri shell: the story calls listing "the shell's job", and the profile lookup is there. But the
shell does not compile on Linux, so a containment rule written there is a security rule proved on no
machine any of this was developed on. `keeper-sync` already owns both halves of the question —
`ExcludeSet` and `volume::scan` — and it builds everywhere, so `..`, the planted symlink, the absent
drive and the tier-0 filter are all asserted over real temp directories. `keeper-core` was not an
option: it is deliberately `keeper-sync`-free (AD-40, and `bun run check:core-sync-free` enforces
it), so it cannot see `ExcludeSet`. The shell keeps exactly the part only it can do — find the
profile, project the VM, register the command.

**Browsing is read-only with respect to the ENGINE, not only with respect to the disk.** This
codebase has already fixed two bugs whose whole shape was "a caller that only meant to look also
made the engine forget": `scan_and_enqueue` spending the stability gate's verdict, and the notes
cadence calling `rescan` to mean "notice now". A listing that stats a directory is the third
opportunity, and it is the easy one to take by reaching for a convenient helper. `browse()` is
therefore typed against `&SyncProfile`, not `&Engine`; the only non-`std::fs` call it makes is
`volume::scan`, which was read before it was called and only reads a marker file — minting one is
`VolumeMarker::ensure`, and browsing must never be what binds a profile to a disk.

**Four listing states, not a boolean and an empty list.** An unplugged pendrive and an empty folder
are the same thing to `read_dir` — the directory is simply gone. Telling someone their recordings
folder is empty when their stick is in a drawer is the fastest possible way to destroy trust in a
file browser, so `MediaAbsent`, `MediaUnexpected` and `Missing` are separate variants with separate
next steps: reattach the drive, work out whose disk is mounted there, re-point the profile. A
foreign marker is deliberately not folded into "absent": something genuinely is mounted there, and
listing a stranger's files under this profile's name would be a different lie.

**`entries: Option<Vec<_>>` makes the mistake unrepresentable rather than merely discouraged.** In
TypeScript `[]` and `null` are different, so a pane that wants to render "this folder is empty" from
`entries.length === 0` has to unwrap first, and unwrapping is exactly where it meets the state that
says otherwise. Carrying `[]` for an unreadable folder would have made the wrong rendering the path
of least resistance. The `keeper-core` test asserts the two spellings on the wire.

**`ExcludeSet` grew a second question, not a second rule set.** `.git/**` and `.keeper-sync/**` are
in the corpus only as subtree rules, so `is_excluded(".git")` is `false` — correctly, for the scan
path, which asks about files and never enumerates `.git` (git's own status walk does that). A
browser asks a different question: *is this a directory that can only ever open onto nothing?*
`is_excluded_directory` answers it from the same list, by taking every pattern that ends in `/**`
and compiling its prefix. Writing `[".git", ".keeper-sync"]` into a new array in `browse.rs` would
have been three lines shorter and would have been the second ignore rule the story forbids — and it
would not have covered a profile's own `scratch/**`. The `add_glob` split exists because a stripped
`scratch/**` loses its `/`, and re-running it through the basename rule would silently promote a
root-anchored pattern to "any `scratch` at any depth".

**Open is a new command, and `recording_open_path` is deliberately not reused.** That command's
containment root is the *recordings destination*, permanently and correctly, and AD-74 says the
Files tab must not reach for the recordings protocol to browse folders that are not recordings
roots. Pointed at a note in a vault it would refuse — so reusing it would have shipped an Open that
works in one folder out of five, which is worse than no Open at all. `sync_open_entry` is rooted at
the profile instead and shares `browse::resolve`, so it is a second *call site* of the one
containment rule, not a second rule. Reveal *does* reuse `reveal_path`, which is already the
codebase's command for a path the frontend legitimately holds, exactly as the recordings browser
uses it.

**The cap is reported, not hidden.** Lazy expansion bounds the depth of what a person asks for; it
does nothing about one flat directory with fifty thousand entries in it, which is still one call and
one serialisation. `LISTING_CAP` is 1000 and the surface says when it bit. A browser that silently
drops entries is worse than one that admits it stopped.

**`SidebarView` gained a `view` field and lost a ten-deep ternary.** The label-to-primary-view
mapping was a nested ternary duplicated once for the click handler and once for `aria-current`,
so every new surface had to be spelled in three places and a miss produced a row that highlighted
the wrong entry. Adding an eleventh rung twice was not a change worth making; one field and one
lookup was. The existing 40 sidebar assertions are what made that safe to do in the same commit.

**Files is a sibling of Sync, not a tab inside it,** for the reason 42.3 gave for Recordings being a
sibling of Recording: Sync answers "is this folder working" and Files answers "what is in it", and a
browser folded into a diagnostics pane is a browser nobody finds. Both ride the one `sync`
capability — where no folder can be synced there is nothing to browse, so the entry is absent rather
than empty.

**The tree is flat in the DOM, and that is what makes the keyboard model
honest.** WAI-ARIA permits a tree whose depth is carried by `aria-level` alone,
and choosing it over nested `role="group"` containers means "the next visible
row" is one array index rather than a walk — so what a person sees and what the
arrows move through are the same list by construction, not by two pieces of code
agreeing. It also removed the `<ul role="tree">` / `<ul role="group">` pair that
`noNoninteractiveElementToInteractiveRole` and `useSemanticElements` were right
to object to.

**A roving tabindex, and it was a design decision rather than a lint fix.**
Biome's `useFocusableInteractive` is satisfied by putting `tabIndex={0}` on every
`treeitem`, and that would ship a folder of two hundred files as two hundred Tab
presses — worse than the error. Exactly one row is in the tab order; Tab reaches
the tree once and the arrows move inside it. The active row is remembered as a
*hint*: if its branch is collapsed away or a refresh drops it, `activeKey` falls
back to the first row, so the tree can never end up with no tab stop at all.
Focus is moved for real through a ref map — a roving tabindex that marks the
right element and does not move focus is a tree that reads correctly in the
markup and traps a keyboard user in practice, which is why every keyboard test
asserts `document.activeElement` and none of them reads a `tabindex` attribute
off a row to decide whether navigation works.

**Right does not descend into a folder that has no children.** An open folder
whose only content is "this folder is empty" has a *sibling* on the row below
it; stepping there would look exactly like descending and would be the tree
telling a lie about the hierarchy. The guard checks that the next row's
`parentKey` really is this node's key, and the revert proof above is what shows
the lie is otherwise reachable.

**A row's actions join the tab order only while that row is focused.** Otherwise
Tab walks Open/Reveal/Copy for every row in the tree, which is the same problem
the roving tabindex exists to solve one level down. Arrow keys choose the row;
Tab then moves into that row's actions and out of the tree.

**Notes are rows but not `treeitem`s.** "Reading…", the empty-folder sentence,
the absent-drive sentence and the truncation notice are prose about a folder,
not things you can select or expand — making them focusable would put dead stops
in the arrow path.

**`FILES_WRITE_CONTROL_LABELS` turns AD-75 from a promise into a gate.** A promise kept only by
everyone remembering it has a date on it. The constant names ten labels this surface must never
grow, and the test asserts each one is absent — so the next person who adds "just a rename" fails a
test that says why, rather than depending on a reviewer noticing.

## Deliberately NOT done

- **No write, rename, move, delete, drag target or new-folder control** (AD-75). Not deferred —
  refused.
- **No bytes served from a synced folder** (AD-74). `keeper-recording://`'s root does not move, and
  this surface never asks it for anything. Files are handed to the system's default application or
  revealed; nothing is streamed into the webview.
- **No search or filter over the tree.** A tree of 100 000 files wants one, and it wants a Rust-side
  index to back it, which is a story rather than a control. Scoped out rather than shipped as a
  frontend `filter()` over the 1000 entries that happen to be loaded — that would look like search
  and lie about its scope.
- **No file sizes, modification times or previews.** Every one of them costs a `stat` or a read per
  entry; the listing already pays one `metadata` call per entry for folder-ness, and the moment
  anyone wants a size column the right answer is to widen `BrowseEntry` once, not to add a second
  pass.
- **No sparse-checkout awareness.** A profile's `subpaths` shape what git materialises; the browser
  shows what is on disk, which is the honest answer to "what is in this folder".
- **No type-ahead** (jump to the row whose name starts with the typed letters). The APG lists it as
  optional, and it wants a debounce, a search buffer and a wrap-around scan; the eight navigation
  keys that are implemented are the ones without which the tree is unusable, and they are asserted
  by where focus lands.
- **No multi-select and no `aria-selected`.** Nothing here acts on a set of files, so a selection
  model would be state with no consumer.
- **No in-flight indicator for a re-read.** A folder with no listing yet says "Reading…", which is
  the whole of the in-flight state and needs no second flag beside it. A folder being *re-read* by
  Refresh shows nothing, because its previous listing is still on screen and still true. Taken
  deliberately: the alternative is a spinner that appears for the tens of milliseconds a local
  `read_dir` takes, which is flicker rather than feedback. If Refresh over a slow pendrive turns out
  to feel dead, the honest fix is a pending mark on the Refresh control itself, not a second
  per-node state to keep in step with the first.
- **No `keeper-syncd` exposure.** Browsing is a UI question; the daemon has no webview to serve.
