# spec-47-5 — the small tail (DW-196, DW-197, DW-198, DW-199, DW-200)

Branch `work/epic-47`, worktree `quick-donkey`. Sentinel `MUT47-5`. Owner of
`keeper/src/ipc.rs`, `keeper/src/notes_window.rs`, `keeper/src/sync_ipc.rs`,
`keeper-sync/src/files_write.rs`, `keeper-core/src/capture.rs` — plus, granted
mid-story by Main: `keeper/src/lib.rs` (one call),
`src/components/capture/capture-window.tsx`, the Problems pane
(`src/components/layout/sync-pane.tsx`) for DW-200, and the test files forced by
two new VM fields.

**Four of these five items live in code that never compiles on Linux.** The
"what I could not verify here" section is not boilerplate; read it before
trusting anything below it.

---

## 1. What shipped, per DW

### DW-197 — `WriteScope::file` deleted (keeper-sync, PROVEN HERE)

`WriteScope::file` had no production caller after AD-102 and answered
`Err(OutsideVault)` for exactly the paths AD-102 routes to the second writer —
so it was the pre-fork mental model, still public, still plausible.

Deleted. Two rustdoc links to `[`Self::file`]` retargeted: `directory`'s now
points at `Self::owner`, and `route`'s ordering note names `vault_relative`
without a link (a link to a private item is a rustdoc warning).

The seven tests that reached it were re-expressed rather than repointed, per
DW-197's own instruction:

| test | was | now | why that target |
| --- | --- | --- | --- |
| `a_profile_with_no_vault_refuses_every_path_and_names_itself` | `file` → `NoVault` | `directory` → `NoVault`, **plus** `owner` → `Unmanaged` | `directory` is the create path and still refuses; the added `owner` line pins AD-102's actual truth — no vault is no longer a refusal for an *existing* file |
| `a_folder_whose_name_extends_the_vaults_is_not_inside_the_vault` | `file` | `directory` + `owner` both arms | post-AD-102 a raw `starts_with` would send a *neighbour's* file down the **vault** writer, so the component match is asserted through both surfaces that depend on it |
| `a_nested_vault_subfolder_is_matched_one_component_at_a_time` | `file` | `directory` | component-by-component vault match |
| `the_configured_subfolder_is_normalised_however_it_was_typed` | `file` ×3 | `directory` ×3 | same |
| `traversal_is_refused_wherever_it_is_aimed` | `file` | `owner` | escape refusal |
| `the_vault_directory_itself_cannot_be_deleted` | `file` | `owner` | vault-root refusal |
| `a_directory_is_refused_as_a_folder_whatever_it_is_named` | `file` ×2 | `owner` ×2 | is-a-directory refusal; the "writable" half becomes `Ok(WriteOwner::Vault)` |

No production line changed. `vault_relative` stays — it is the shared core of
`directory` and `classify`.

**Verified by compilation, not by grepping**: `cargo test -p keeper-sync --lib
files_write::` → **33 passed, EXIT=0**. The grep is the corroboration, not the
proof: `pub fn file` and `.file(` return nothing in `keeper-sync/src` or
`keeper/src`.

### DW-196 — `DestinationProfileRow`'s two `Option`s collapsed (keeper shell, NOT COMPILED HERE)

New private type in `ipc.rs`:

```rust
struct RecordingsPlace { root: PathBuf, subfolder: String }
```

and the row carries `recordings: Option<RecordingsPlace>` in place of
`recordings_root: Option<PathBuf>` + `recordings_subfolder: Option<String>`.
A root with no head is now unrepresentable rather than merely unwritten.

An accessor `DestinationProfileRow::recordings_root(&self) -> Option<&Path>`
keeps the readers that only ask *where* at one line, and borrows, so asking
cannot hand a caller a root separated from its head.

The builder does **not** reimplement the join, which was the standing rule:

```rust
recordings: profile.recordings_root()
    .zip(profile.recordings.as_ref())
    .map(|(root, recordings)| RecordingsPlace {
        root, subfolder: recordings.subfolder.trim().to_owned(),
    }),
```

`keeper_sync::SyncProfile::recordings_root()` stays the one definition of the
join; `zip` is what makes "both or neither" a fact about one expression.

### DW-198 — the hotkey stops re-centring a window the user placed (shell, NOT COMPILED HERE)

Two halves, and **either alone changes nothing a person would notice**:

1. **At boot** — `notes_window::adopt_position`, called from `lib.rs`'s setup
   beside the existing `adopt_placement`. Deliberately a separate function
   rather than a line inside `adopt_placement`, because that one *also* runs on
   the lock toggle, and applying a stored position there would make *unlocking*
   teleport a window the person is looking at. A padlock click is not a request
   to move a window.
2. **On every `show`** — `reveal(None)` no longer calls `position()`
   unconditionally. It reads `window.is_resizable()` and asks
   `plan_show_position`.

DW-198's own text said "on every `show`, do what `reveal(None)` does today".
**That is the one place this story departs from the DW, and it has to**: boot
adoption alone is undone by the first hotkey press, so the fix would have been
invisible. The DW's *reasoning* — "locked follows the pointer, unlocked stays
put" — is what is implemented.

`is_resizable()` rather than a settings read is what keeps NFR-27: it is the
same window attribute `apply_resizability` writes at boot and on every lock
toggle, read back off the window. One source of truth, no sqlite on the hot
path. The path is now `is_resizable` → (`set_position`) → `show` → `set_focus`,
and for an unlocked window it is one call *shorter* than before.

**The promise did not move.** Applying a stored position is `set_position`, the
call UX-DR43 says a Wayland compositor may refuse. So the restore is *attempted*
and still not *promised*: `CAPTURE_UNLOCK_LABEL` still reads "so it can be moved
and resized" and never "remembers", and no UI copy changed. The module doc that
said "what is still not promised is the restore" was updated to say the restore
is now attempted, best-effort, and still not promised — because a doc that
contradicts the code is worse than either.

### DW-199 — the resize border no longer eats the close button (fix in keeper-core + TSX)

**The finding, which is the whole justification for pushing a layout number
through Rust.** From tao 0.35.3, `src/platform_impl/linux/event_loop.rs` lines
501 (motion) and 531 (button-press):

```rust
if !window.is_decorated() && window.is_resizable() && !window.is_maximized() {
    let border = window.scale_factor() * 5;
    let edge = crate::window::hit_test((left, top, right, bottom), cx, cy, border, border);
```

Three consequences, and a fix that ignored any one of them would be wrong in a
way that looks right in a screenshot:

* The border is `scale_factor() * 5` against **GDK window coordinates**, which
  are logical on GTK3 — so it is **10 logical px on a 2× display**. A CSS
  constant of `5` would be exactly half the border on the owner's own hardware:
  the close button would *look* fixed and still miss.
* It is guarded by `is_resizable()`, so a **locked** capture window has no
  border at all — an inset there is a permanent gutter over nothing.
* It is guarded by `!is_maximized()`, and maximized is the state a person is
  most likely to be in when they reach for close.

**Why the number cannot be decided in the webview.** `capture-window.tsx` and
`src/test/no-user-agent-gating.test.ts` say this app reads the platform nowhere.
A `navigator`-sniffing inset in TSX would have passed every test this story ran
and broken the one the repo wrote to stop exactly that. Rust reads
`is_resizable`, `is_maximized` and `scale_factor`, hands the webview a **number**,
and the frontend still knows nothing about GTK.

Shipped:

* `keeper_core::capture::{TAO_EDGE_BORDER, EdgeResize, chrome_edge_inset}` — the
  arithmetic and its four zero-states, tested on this box.
* `CaptureWindowVm.chrome_inset` (wire `chromeInset`), filled by
  `notes_window::edge_inset`, which is the one platform test:
  `inside_client_area: cfg!(all(unix, not(target_os = "macos")))`, a
  compile-time fact and never a runtime probe.
* `capture-window.tsx` insets the chrome strip's `paddingTop` and adds the inset
  to its existing `px-1` via `calc`, and applies **no `style` attribute at all**
  when the inset is zero.

Every best-effort read errs toward the reachable control: a window that will not
say whether it is resizable is treated as locked (no gap over nothing), and one
that will not name a scale factor gets one border's worth rather than none.

### DW-200 — a file keeper knows about and never mentions (added mid-story by Main)

Not on the original list. L2Names (story 47.2) proved that a folder holding
`a\xFF.txt` and a legitimate `a\u{FFFD}.txt` renders both to one string; the
pane hands that string back, `resolve` **succeeds at the other file**, and
`files_write` shares the join — so **a delete confirmed against one row removed
the other**. They closed it at the `browse::plain_segments` choke point (see §7
for the one line that reaches this file). That fix makes the file *unreachable*
from the app; it does not make it *visible*. `ProblemReport.unspellable` was
being produced and reaching no surface, which is the shape this epic exists to
kill: keeper does the work and does not show you. Main ruled it must finish here
rather than become deferred work.

Shipped, all of it above L2's layer where the engine type exists:

* `SyncUnspellableVm { display, escaped }` in `sync_ipc.rs`, beside
  `SyncParkedVm` and for the same reason: `keeper_sync::UnspellableName` is an
  engine type and does not derive `TS`.
* `SyncProblemsVm.unspellable: Vec<SyncUnspellableVm>`, filled in the
  hand-written projection at the `sync_problems` command — hand-projected like
  every field beside it, so a field added to `ProblemReport` reaches the pane by
  someone deciding it should rather than by inheriting a derive.
* A section in `SyncProblemsSection` (`sync-pane.tsx`), between conflict copies
  and parked work, and folded into the "renders nothing when nothing is wrong"
  guard.

**Both renderings per row, which is Main's requirement and the right one.**
`display` is what a person recognises; it is lossy and non-injective, so two
different files produce one `display` — that IS the defect. `escaped` is
byte-exact ASCII and is the line a person can paste into a shell to go and find
the file. A row carrying only `display` names a file the reader cannot then
locate. So the escaped line is `select-all break-all` and deliberately **not**
truncated: half of a byte-exact name is worse than none.

Rows are keyed on `escaped`, never `display` — see §4, where that decision
started as an unproven comment and a survivor turned it into an assertion.

The copy says what keeper **will not** do, so the reader stops waiting for it:
the bytes sync perfectly (git stores the raw name), but keeper will not open,
edit or delete the file from the app, because the only name it can show is not
the name on disk.

---

## 2. I/O matrix

### `WriteScope` after DW-197 (keeper-sync — every row asserted here)

| scope | subpath | is_dir | `directory` | `owner` |
| --- | --- | --- | --- | --- |
| no vault | `clip.mov` | false | `Err(NoVault)` | `Ok(Unmanaged)` |
| `10-notes` | `10-notes/a.md` | false | `Ok("a.md")` | `Ok(Vault)` |
| `10-notes` | `10-notes-archive/a.md` | false | `Err(OutsideVault)` | `Ok(Unmanaged)` |
| `10-notes` | `10-notes` | true | `Ok("")` (create here) | `Err(VaultRoot)` |
| `10-notes` | `10-notes/notes.md` | true | `Ok("notes.md")` | `Err(IsDirectory)` |
| `10-notes` | `10-notes/notes.md` | false | `Ok("notes.md")` | `Ok(Vault)` |
| `10-notes` | `..`, `../etc`, `/etc/passwd`, `.`, `10-notes/./a.md`, `10-notes//a.md`, `10-notes/a.md/` | false | `Err(Escapes)` | `Err(Escapes)` |
| `a/b` (any of `a//b`, `a\b`, `/a/b/`) | `a/b/c.md` | false | `Ok("c.md")` | `Ok(Vault)` |
| any | `BrowseRefusal::Unspellable` from `plain_segments` | — | `Err(Escapes)` | `Err(Escapes)` |

### `Placement::adopted_position` (keeper-core — asserted here)

| locked | stored position | answer | why |
| --- | --- | --- | --- |
| true | `None` | `None` | keeper places it, as always |
| true | `Some((120,-40))` | `None` | locking is not a discard button (the row keeps it) but a locked panel follows the pointer |
| false | `None` | `None` | never moved ≠ moved to the default |
| false | `Some((-1400,220))` | `Some((-1400,220))` | a monitor left of the primary is an ordinary desk |

### `plan_show_position` (keeper-core — asserted here)

| `unlocked` (= live `is_resizable()`) | answer |
| --- | --- |
| `true` | `Leave` — the window stays where the person put it |
| `false` | `Place` — keeper centres it on the pointer's monitor, exactly as before |
| read failed (`Err`) | shell passes `false` → `Place` — today's behaviour |

### `chrome_edge_inset` (keeper-core — asserted here)

| inside_client_area | resizable | maximized | scale | inset |
| --- | --- | --- | --- | --- |
| true (GTK) | true | false | 1 | **5** |
| true (GTK) | true | false | 2 | **10** |
| true (GTK) | true | false | 0 (unknown) | **5** |
| true (GTK) | **false** (locked) | false | 1 | 0 |
| true (GTK) | true | **true** | 1 | 0 |
| **false** (macOS/Windows) | true | false | 1 | 0 |
| **false** (retina Mac) | true | false | 2 | 0 |

---

## 3. Edge cases considered

* **DW-197 / no-vault profile.** `owner` returning `Unmanaged` where `file`
  returned `NoVault` is not a regression the deletion introduced — it is AD-102,
  and the migrated test now states it so a future reader cannot mistake the
  change for one.
* **DW-197 / `directory("")` still refuses.** The create path in a vault-less
  profile is unchanged; only the *delete/edit* path forks.
* **DW-196 / six stale fixtures.** All six `unflagged.recordings_root = None`
  became `unflagged.recordings = None`, which now also clears the head. That was
  DW-196's named hazard: six fixtures left a stale `recordings_subfolder` behind
  and it changed no verdict only because every read site checked the root first.
* **DW-196 / partial move.** `resolve_recording_destination` and
  `default_recording_destination` both *move* the root out of a consumed row.
  Both take the whole `RecordingsPlace` (`row.recordings.filter(..)?.root`), so
  the borrow checker sees the same shape it saw before.
* **DW-198 / a locked window is untouched.** No behaviour change at all for the
  default configuration; a person who never touches the lock sees exactly
  today's panel.
* **DW-198 / unlock does not teleport.** The lock toggle path
  (`notes_ipc.rs:4557` → `adopt_placement`) is deliberately not given position
  adoption.
* **DW-198 / a new capture window.** `open` calls `reveal(Some(placement))` →
  `apply_placement`, untouched.
* **DW-198 / hidden window.** `is_resizable()` reads a stored attribute
  (`gtk_window_get_resizable`, a macOS style mask), not a mapped surface. Listed
  as a gate check anyway.
* **DW-199 / unknown VM.** The frontend reads `chromeInset ?? 0`, matching
  `locked ?? true`: a gap arriving a frame late is invisible; a gap on a window
  with no border is permanent.
* **DW-199 / no style attribute at zero.** `style` is `undefined` rather than
  `{}` when the inset is zero, so nothing about the strip changes on macOS,
  Windows, or a locked window — asserted with `not.toHaveAttribute("style")`.
* **DW-199 / fractional scale.** GTK3's scale factor is an integer, but the
  shell rounds **up** through `f64::ceil().max(1.0)`: under-insetting leaves part
  of the close button on a resize handle, over-insetting costs a pixel.
* **DW-200 / two files, one `display`.** The list's own worked example. Both
  rows render, both are distinguishable (by `escaped`), and the React key is the
  byte-exact form so the two do not collide.
* **DW-200 / the section is open for another reason.** A folder with a warning
  and no unspellable names must show the warning and say nothing about names.
  This is now the second test, and it exists because the first version of it —
  written against a folder with *nothing* wrong — could not fail (see §4).
* **DW-200 / an empty list.** `unspellable` joins the "renders nothing at all"
  guard, so it can open the Problems section on its own and cannot leave an
  empty heading behind.

---

## 4. Mutation table

Every row below was actually run: mutation applied with a `// MUT47-5 IN-FLIGHT
MUTATION:` header line, the named test run, the file restored from a byte
snapshot, and the restore verified by **reading `git diff`** (not by memory) plus
a repo-wide grep for `MUT47-5`, which now returns nothing. The two files this
story CREATED — this spec and `src/lib/ipc/gen/SyncUnspellableVm.ts` — are
invisible to `git diff` and were checked by name.

**Baselines were established green in the exact scope each sweep used**, per the
standing rule Main issued after L2Names found a kill verdict scored against an
already-red test. That mattered here: the TSX sweeps ran under a `-t` filter
*narrower* than the file-level run I had been using, so the filtered baseline was
re-run on its own and confirmed green (`1 passed`, EXIT=0) before any mutant.

### keeper-sync — `files_write.rs` (proves the migrated DW-197 tests still bite)

| # | mutation | test | verdict |
| --- | --- | --- | --- |
| M1 | `vault_relative`: component loop → raw `starts_with(subfolder)` | `a_folder_whose_name_extends_the_vaults_is_not_inside_the_vault` | KILLED |
| M2 | `classify`: drop the `VaultRoot` refusal | `the_vault_directory_itself_cannot_be_deleted` | KILLED |
| M3 | `classify`: drop the `IsDirectory` refusal | `a_directory_is_refused_as_a_folder_whatever_it_is_named` | KILLED |
| M4 | `vault_relative`: drop `browse::plain_segments` | `traversal_is_refused_wherever_it_is_aimed` | KILLED |
| M5 | `WriteScope::new`: drop subfolder normalisation | `the_configured_subfolder_is_normalised_however_it_was_typed` | KILLED |
| M6 | `vault_relative`: `NoVault` → `OutsideVault` | `a_profile_with_no_vault_refuses_every_path_and_names_itself` | KILLED |

### keeper-core — `capture.rs`

| # | mutation | test | verdict |
| --- | --- | --- | --- |
| N1 | `adopted_position`: ignore the lock (return `self.position`) | `an_unlocked_window_is_put_back_and_a_locked_one_still_follows_the_pointer` | KILLED |
| N2 | `plan_show_position`: always `Place` | `the_hotkey_leaves_an_unlocked_window_alone_and_still_places_a_locked_one` | KILLED |
| N3 | `chrome_edge_inset`: drop the `maximized` guard | `the_chrome_is_inset_only_where_the_resize_border_actually_is` | KILLED |
| N4 | `chrome_edge_inset`: drop the `resizable` guard | same | KILLED |
| N5 | `chrome_edge_inset`: drop the platform guard | same | KILLED |
| N6 | `chrome_edge_inset`: ignore the scale factor (return the bare constant) | same | KILLED |
| N7 | `CaptureWindowVm`: `#[serde(skip)]` on `chrome_inset` | `the_window_view_model_wire_shape_is_camel_case` | KILLED |

### frontend — `capture-window.tsx`

| # | mutation | test | verdict |
| --- | --- | --- | --- |
| T1 | inset the strip unconditionally (drop the `> 0` guard) | `keeps the buttons out of the window's own resize border…` | KILLED |
| T2 | hard-code tao's `5` instead of the measured number | same | KILLED |
| T3 | ignore the number Rust sent (`chromeInset = 0`) | same | KILLED |

### frontend — `sync-pane.tsx` (DW-200)

| # | mutation | test | verdict |
| --- | --- | --- | --- |
| U1 | key the rows on the LOSSY `display` instead of the byte-exact name | `reports a name that is not text…` | **survived first, then KILLED** |
| U2 | drop the byte-exact line, leaving only the lossy rendering | same | KILLED |
| U3 | drop the readable line, leaving only the escaped form | same | KILLED |
| U4 | remove `unspellable` from the "nothing is wrong" guard | same | KILLED |
| U5 | render the name list unconditionally | `says nothing about names…` | **survived first, then KILLED** |

21 mutations, 21 kills, 0 survivors — **after two rounds.** The first round of
the DW-200 sweep produced two survivors, and both were tests that read as if
they proved something they did not:

* **U1.** The code comment said the rows must be keyed on `escaped` because
  `display` collides; the test counted two rows and called that the proof. It is
  not: React renders duplicate-keyed siblings and only *warns*, so the count
  passes either way. Fixed by capturing `console.error` across the render and
  asserting no `same key` complaint. The hazard is real for this list
  specifically — it is the one place in the app where two entries routinely
  share a `display`, so a duplicate key is a live reconciliation bug the moment
  the list changes.
* **U5.** "Says nothing about names" was asked of a folder with *nothing* wrong,
  so the whole Problems section early-returned `null` and the test passed
  without the inner conditional ever running. It could not fail. Fixed by giving
  the folder a *different* problem — a warning — so the section is open and the
  name list is the only thing that must be absent.

Both are the same failure Main named: **a test that cannot fail reports success
forever.** Neither would have been found by reading the test.

**Not mutation-tested, and cannot be:** every line in `ipc.rs`,
`notes_window.rs`, `lib.rs` and `sync_ipc.rs`. They do not build on this
machine, so a mutation there produces no observable verdict. Claiming a kill for
them would be a lie. That specifically includes the DW-200 projection: nothing
here proves `display` and `escaped` are not transposed on their way out of
`ProblemReport` — see the gate check in §6.

---

## 5. Deliberately NOT done

* **Nothing else in `sync_ipc.rs`.** DW-196's field pair turned out to live
  entirely in `ipc.rs`; the `recordings_subfolder` matches in `sync_ipc.rs`
  belong to `SyncProfileVm` and `SyncProfileReq`, which are different types with
  a different contract, and they were left alone. The only `sync_ipc.rs` hunks
  are DW-200's.
* **No fold on the unspellable list.** Conflicts do not fold either, and parked
  work folds because it is unbounded by design. A folder with enough
  non-UTF-8 names to need a fold has a different problem and should say so
  differently; guessing that shape now would be inventing a requirement.
* **No "rename it for me" action.** keeper cannot address the file — that is the
  whole finding — so a button would be the dead button AD-27 forbids. The copy
  points at the terminal instead, which is where the fix actually is.
* **No `strip_prefix` recovery of the recordings head.** `RecordingsPlace`
  carries the stored string. `20-media//sessions` and `20-media/sessions` join
  to one root and are two different stored values, and only the stored one may
  be echoed back to `sync_profile_save`.
* **No UI copy change for DW-198.** The restore is attempted and still not
  promised — see §1. Upgrading the label to "remembers" would be promising a
  `set_position` a compositor may refuse.
* **Position adoption is not in `adopt_placement`.** See §1; a padlock click
  must not move a window.
* **No source-reading test for DW-196.** The repo's source-scan trick
  (`command-registration.test.ts`) fits facts a *string* can state. "A root
  without a head is unrepresentable" is a fact about a type, proved by the
  compiler at the gate; a text test asserting the field's spelling would pin an
  implementation detail and prove nothing.
* **`WriteScope::directory` was not renamed** even though it now also carries the
  no-vault refusal that `file` used to. It is the create path's name and the
  create path is what it is for.
* **The `BrowseRefusal::Unspellable` arm in `files_write.rs` is not mine.** It is
  one match arm added at L2Names' request to unbreak the shared crate — see §7.

---

## 6. What I could not verify here, and why — ordered gate checks

`keeper` (the Tauri shell crate) does not build on Linux: no GTK, no webkit. I
never ran `cargo build/check/clippy/test -p keeper`. **`ipc.rs`,
`notes_window.rs`, `lib.rs` and `sync_ipc.rs` were never compiled by this story,
by anything, anywhere.**

What I *did* run on them: `rustfmt --edition 2021 --check` on each, EXIT=0 — that
proves the files **parse** and are already rustfmt-clean. It proves nothing about
types, borrows, trait bounds or API existence.

**The DW-200 TypeScript bindings are HAND-WRITTEN, not generated, and this is
the one thing in this story I would check first.** `SyncProblemsVm` and
`SyncUnspellableVm` live in the `keeper` crate, so `cargo test -p keeper --lib
export_bindings` — the thing that emits them — cannot run on Linux. I wrote
`src/lib/ipc/gen/SyncUnspellableVm.ts` and edited
`src/lib/ipc/gen/SyncProblemsVm.ts` by hand, in ts-rs's exact emitted style
(copied from `SyncParkedVm.ts` and the file's own previous contents, trailing
spaces included). `tsc --noEmit` passes against them and the pane's tests pass,
so they are *consistent*; only the gate can prove they are what ts-rs
*generates*. If the gate reports a binding diff, the generated file wins —
nothing in the frontend depends on my formatting, only on the field names and
types. By contrast `CaptureWindowVm.ts` IS generated: it lives in `keeper-core`,
which builds here, and `export_bindings_capturewindowvm` passed.

What I established by reading rather than by compiling:

* `WebviewWindow::is_resizable() -> crate::Result<bool>`,
  `is_maximized() -> crate::Result<bool>` and `scale_factor() -> crate::Result<f64>`
  all exist in tauri 2.11.5 (`src/webview/webview_window.rs:1754`, `:1739`,
  `:1700`), which is the version in `Cargo.lock`.
* `Option::zip` is stable and is what makes the `RecordingsPlace` build total.
* `keeper_sync::names::UnspellableName` has public `display: String` and
  `escaped: String` (`names.rs:87`, `:93`) and `ProblemReport.unspellable:
  Vec<UnspellableName>` exists (`engine.rs:262`) — read from L2Names' code, not
  compiled against.
* tao's border arithmetic and its guards — quoted verbatim in §1.

### Gate checks, in the order they should be run on macOS

1. **`cargo check -p keeper`.** This is the real DW-196 gate. If I missed one of
   the sites in §8 it fails here, loudly, with a line number. That is the honest
   outcome the story was scoped for.
2. **`cargo clippy -p keeper -- -D warnings`.** Two things to watch that a
   `check` will not catch: the rustdoc intra-doc link I retargeted to
   `[`Self::owner`]`, and whether `RecordingsPlace` earns a `dead_code` warning
   on the iOS target (it should not — its fields are read by
   `destination_profile_vms`, which is not `cfg`-gated).
3. **`cargo test -p keeper --lib ipc::`.** The `destination_profile_*` tests and
   the six rewritten fixtures.
4. **`cargo test -p keeper --lib notes_window::`** — there is no new test there;
   this is a "did I break the existing ones" check.
5. **`cargo test -p keeper-core --lib capture::`** and
   **`cargo test -p keeper-sync --lib files_write::`** — both already EXIT=0 on
   Linux; re-running them on the gate costs nothing.
6. **Confirm `src/lib/ipc/gen/CaptureWindowVm.ts` is committed.** It is
   regenerated and in the worktree (`export_bindings_capturewindowvm` passed).
   The gate fails on *uncommitted* bindings.
7. **`cargo test -p keeper --lib export_bindings`, then `git diff -- src/lib/ipc/gen/`.**
   This is the DW-200 binding gate and the one described above. An empty diff
   means my hand-written files match ts-rs byte for byte; a non-empty one means
   take the generated version. Either way the result must be **committed**.
8. **`cargo test -p keeper --lib sync_ipc::`** — a "did I break the existing
   ones" check; DW-200 added no Rust test there, because there is no Rust test
   on this box that could have run.

### Behaviour that needs a human at a real window

9. **DW-198, macOS or GTK.** Unlock the draft panel, drag it somewhere, quit,
   relaunch, press the hotkey. It should appear where you left it. Press the
   hotkey again — still there. Lock it and press the hotkey: it should snap back
   to a fifth of the way down the *pointer's* monitor. Move the pointer to a
   second monitor and press again: a locked panel follows, an unlocked one does
   not. **The unlocked case is the one that is genuinely untested** — everything
   about it below `plan_show_position` is uncompiled shell code.
10. **DW-198 on Wayland specifically.** The restore is `set_position` and may be
    refused. A refusal must log at debug and leave a usable window — it must not
    look like a hang or an invisible panel. Nothing promises the restore in the
    UI, so a refusal is a non-event by design; confirm it actually is.
11. **DW-198, hidden-window attribute read.** `is_resizable()` on the *hidden*
    prewarmed window must answer truthfully. If a backend answers `Err` or
    `false` for a hidden-but-resizable window, the unlocked panel silently keeps
    re-centring — the bug would look unfixed rather than broken. Worth one
    `tracing` glance at the first hotkey press after an unlock.
12. **DW-199, GTK, the actual measurement.** This is what DW-199 asked for and
    what I could not run. Unlock a capture window and click the top-right two
    pixels of the close button. It must close, not resize. Then maximize it and
    click the same pixels — still closes, and there should be **no** gutter.
    Then lock it — no gutter. If 5 × scale turns out to be the wrong number on
    a real compositor, `TAO_EDGE_BORDER` is one constant with one test.
13. **DW-199 on macOS.** Look at the capture chrome. If there is a visible gap
    above or right of the close button, `inside_client_area` is being computed
    wrong and the `cfg!` in `notes_window::edge_inset` is the single place to
    look.
14. **DW-200, end to end, and this is the check the pane test cannot make.**
    In a synced folder, `touch $'weird\xff.txt'`, let a sync carry it, and open
    the folder's Problems section. A row must appear with `weird<U+FFFD>.txt`
    above `weird\xffepuap`-style escaped bytes. **Copy the escaped line and
    paste it into a shell** — it must name the real file. If the two lines are
    transposed, or the escaped one is truncated, that is the hand-written
    projection at `sync_problems` being wrong, and nothing on Linux could have
    caught it.
15. **DW-200, the file stays unreachable.** With that row on screen, try to
    delete the file from the Files pane. It must refuse — L2Names' `Unspellable`
    refusal, rendered through `WriteRefusal::Escapes`. A delete that *succeeds*
    here is the original data-loss defect, and the Problems row would have made
    it easier to reach rather than safer.

---

## 7. Cross-layer coupling — read this before splitting

`files_write.rs` contains **one match arm that is not this story's**:

```rust
BrowseRefusal::Unspellable { subpath } => Self::Escapes { subpath },
```

L2Names (story 47.2) added `BrowseRefusal::Unspellable` to `browse.rs` to close a
real defect — a lossy `U+FFFD` rendering of a non-UTF-8 filename can be joined
back to a *different real file*, so a delete confirmed against one row removes
another. That made `keeper-sync` non-exhaustive and red for the whole worktree.
I added the arm at their request and it unblocked the crate.

**Main ruled: keep it where it is.** The stack is L1 → L2 → L3 → L4 → L5, so
L2Names' layer sits **below** mine and the variant already exists beneath this
arm when it compiles. Do not move it into L2. (The matching arm in
`keeper-sync/src/export.rs` is L2Names', not mine — I did not touch that file.)

**DW-200 depends on L2 the same way and for the same reason.** Everything in
`sync_ipc.rs` here reads `keeper_sync::names::UnspellableName` and
`ProblemReport.unspellable`, both of which L2Names created. Same stack order,
same conclusion: fine as long as L2 lands first. This layer's PR must say so in
its description, because a reader who sees a Problems-pane row for a type the
diff never introduces will go looking for it.

Files carrying hunks that are forced consequences of this story's two new VM
fields, all confirmed as mine by Main:

| file | why |
| --- | --- |
| `src/components/capture/capture-window.test.tsx` | `chromeInset` fixtures + the DW-199 test |
| `src/lib/stores/capture-windows.test.ts` | three fixtures gain `chromeInset: 0` |
| `src/components/settings/sync-section.test.tsx` | its `syncProblems` mock gains `unspellable: []` |
| `src/components/sync/add-folder-form.test.tsx` | same |
| `src/lib/ipc/client.ts` | one re-export line for `SyncUnspellableVm` |

Each is a type error without the change, so leaving any of them would have left
the shared worktree with a red `tsc` that reads to nine other agents as their own
breakage.

---

## 8. DW-196 — the sites, by pre-change `ipc.rs` line

Line numbers are against `ipc.rs` as it stood at the start of this story, so the
gate's error output can be checked against them. Twenty-five touched lines in
thirteen hunks; DW-196 counted the hunks.

**Declaration (hunk 1)**

| line | was |
| --- | --- |
| 7225–7227 | doc for `recordings_root` |
| 7228 | `recordings_root: Option<PathBuf>,` |
| 7229–7237 | doc for `recordings_subfolder`, incl. `[`Self::recordings_root`]` |
| 7238 | `recordings_subfolder: Option<String>,` |

→ one field `recordings: Option<RecordingsPlace>`, plus the new `RecordingsPlace`
struct and the `impl DestinationProfileRow` accessor inserted after line 7252.

**Readers (hunks 2–8)**

| line | was | now |
| --- | --- | --- |
| 7488 | `let Some(root) = row.recordings_root else {` | `let Some(place) = row.recordings else {` |
| 7503 | `root,` | `root: place.root,` |
| 7539 | `row.recordings_root.filter(\|_\| row.enabled)?` | `row.recordings.filter(\|_\| row.enabled)?.root` |
| 9762 | `row.recordings_root.is_none()` | `row.recordings.is_none()` |
| 9791 | `row.recordings_root.as_deref() == Some(folder)` | `row.recordings_root() == Some(folder)` |
| 9800 | `offers_recordings: row.recordings_root.is_some()` | `offers_recordings: row.recordings.is_some()` |
| 9856 | `row.recordings_root.as_ref()?.to_string_lossy()` | `let place = row.recordings.as_ref()?;` … `place.root.to_string_lossy()` |
| 9861 | `row.recordings_subfolder.clone()?` | `place.subfolder.clone()` (one `?`, not two) |

**Builder (hunk 9)**

| line | was |
| --- | --- |
| 7631 | `recordings_root: profile.recordings_root(),` |
| 7632–7635 | the pairing comment |
| 7636–7639 | `recordings_subfolder: profile.recordings.as_ref().map(…)` |

→ the single `zip` expression in §1.

**Test fixtures (hunks 10–13)**

| line | was | now |
| --- | --- | --- |
| 12247 | `recordings_root: Some(PathBuf::from(local).join("recordings")),` | `recordings: Some(RecordingsPlace { root, subfolder })` |
| 12248–12250 | `recordings_subfolder: Some("recordings".to_owned()),` | (folded into the above) |
| 12372 | `unflagged.recordings_root = None;` | `unflagged.recordings = None;` |
| 12463 | same | same |
| 12570 | same | same |
| 12714 | same | same |
| 13022 | same | same |
| 13209 | same | same |
| 13213 | `nested.recordings_subfolder = Some("40-media/recordings".to_owned());` | `nested.recordings = Some(RecordingsPlace { … })` |
| 13214 | `nested.recordings_root = Some(PathBuf::from("/Volumes/nest/40-media/recordings"));` | (folded into the above) |
| 13287 | `destination_profile_row(&profile).recordings_root` | `.recordings` |
| 13298 | `row.recordings_root,` | `row.recordings_root().map(Path::to_path_buf),` |

`removable_row` (12263) needed no change — it builds on `flagged_row` with
struct-update syntax.

**Audited after the change**: a grep of `ipc.rs` for `recordings_root` /
`recordings_subfolder` returns only `SessionSync.recordings_root` (a different
struct, out of scope), `RecordingProfileVm.recordings_root` (the VM, unchanged by
design), `SyncProfile::recordings_root()` (keeper-sync's method) and one test
local. No `DestinationProfileRow` site remains.

---

## 9. Gates run on this machine

| gate | result |
| --- | --- |
| `cargo test -p keeper-sync --lib files_write::` | **EXIT=0**, 33 passed |
| `cargo test -p keeper-core --lib capture::` | **EXIT=0**, 25 passed (incl. both `export_bindings`) |
| `npx vitest run` on the five owned/forced test files | **EXIT=0**, 165 passed |
| `npx tsc --noEmit -p tsconfig.json` | **EXIT=0**, whole project |
| `rustfmt --edition 2021 --check` on all six owned Rust files | EXIT=0 each — **parse only, not a typecheck** |
| repo-wide grep for `MUT47-5` | no matches |
| mutation sweep | 21 mutants, 21 kills, 0 survivors (after fixing the two tests that let round one survive) |

The five test files in the vitest row:
`src/components/capture/capture-window.test.tsx`,
`src/lib/stores/capture-windows.test.ts`,
`src/components/layout/sync-pane.test.tsx`,
`src/components/settings/sync-section.test.tsx`,
`src/components/sync/add-folder-form.test.tsx`.

Not run, per the batch rule: the full suite, the formatter, the linter. Never
run, per the platform: anything that compiles the `keeper` crate.

---

## 10. DW ledger — for Main, who is the only one who may write it

| DW | state | note |
| --- | --- | --- |
| DW-196 | **closed** | collapsed; every site listed in §8; proof is the macOS `cargo check` |
| DW-197 | **closed** | deleted and proven by compilation here |
| DW-198 | **closed** | fixed in both halves; departs from the DW's literal fix text — see §1 |
| DW-199 | **closed** | fixed with tao's own arithmetic rather than a measured guess; gate check 12 is the confirmation, not a condition |
| DW-200 | **closed** | reaches the Problems pane with both renderings |

**Opened: none.** Nothing in this story was deferred.

Two things that are NOT ledger entries but need a decision from someone:

* The hand-written `sync_ipc` bindings (§6). If the gate regenerates them
  differently, that is a commit, not a defect.
* Gate checks 12 and 14 are the two behaviours no machine available to me could
  exercise. If either fails, the fix is small and local — one constant for
  DW-199, one projection for DW-200 — but they should be run before this is
  called done in front of the owner.
