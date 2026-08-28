# Spec 47.2 — A name keeper cannot spell

**Agent:** L2Names · **Sentinel:** `MUT47-2` · **Branch:** `work/epic-47`

The owner, auditing a real repo: *"one file whose name is not valid UTF-8 under
`doc-epuap-common/`, which probably would not make it to the remote anyway."*

---

## 0. The finding that leads, because it is bigger than the one I was sent for

I was sent to fix silence. I found a **wrong-file bug** on the way, and it is the
thing that should be read first.

`browse.rs` rendered every dirent with `to_string_lossy()` and put the result in
`BrowseEntry::relative_path` — the field AD-65 documents as *"feed it straight
back as the `subpath` of the next call"*. Eleven lines above it, a comment
asserted the safety property:

> Lossy rather than skipped. […] every action on it is re-resolved in Rust,
> **where it will honestly refuse rather than open the wrong file.**

That sentence was false, and here is the folder that falsifies it. Two files:

| on disk (raw bytes)   | what it is                                     |
| --------------------- | ---------------------------------------------- |
| `a\xFF.txt`           | not valid UTF-8                                 |
| `a\u{FFFD}.txt`       | ordinary UTF-8 — a name a person can type       |

Both render to the string `a\u{FFFD}.txt`. Measured, before any change:

```
ENTRY rela="a<FFFD>.txt" abs="/tmp/…/a<FFFD>.txt"
ENTRY rela="a<FFFD>.txt" abs="/tmp/…/a\xFF.txt"     <- two rows, one key
FEEDING BACK "a<FFFD>.txt"
RESOLVED Ok(Some("/tmp/…/a<FFFD>.txt"))
READ BACK "the-decoy"   <-- is this the file the user clicked?   SAME? false
```

It did not refuse. It **succeeded, at the other file.** And `files_write.rs`
borrows the very same `browse::lexical_join` — deliberately, so there is one
containment rule and not two — so a *delete* confirmed against one row removed
the other. The comment was doing the work a rule should have been doing.

That is why the fix is at the join and not at the listing, and why the
display/reach separation is a type rather than a convention: the old code
already had the convention, written out in prose, and still had the bug.

---

## 1. What the walk actually does today — measured, not assumed

Every row below was produced by a throwaway probe against real
`OsString::from_vec(b"doc-\xffepuap.txt")` fixtures, then deleted.

| Site | File:line (pre-change) | What it did | Verdict |
| --- | --- | --- | --- |
| Browse dirent | `browse.rs:526` | `to_string_lossy()` | **Silently mangles.** Listed, unmarked, key collides |
| Browse join | `browse.rs:340` `resolve` | joined the rendering | **Reached a different real file** |
| Exclude match | `exclude.rs:431` | `to_string_lossy()` per component | Mangles; user patterns match a name that is not the file's |
| Engine pending | `engine.rs:5043,5099` | `to_string_lossy()` | Mangles; happens to agree with browse because both mangle identically |
| Commit walk | `git/repo.rs:859` `to_path` | `gix::path::from_bstr` | **Byte-exact.** Correct already |
| Index stage | `git/commit.rs:439` | `gix::path::try_into_bstr` | **Infallible on unix.** Correct already |
| Tracked scan | `git/repo.rs:1135` | `BStr::to_string` | **Silently mangles.** Second wrong-file site — see §3.4 |
| `.git` scaffold probe | `git/repo.rs:1034` | `to_str()` → `None` arm | Already honest, already reports |

Nothing aborts a scan. Nothing skips. Everything **mangles**, which is the worst
of the three: a skip is at least a hole you can notice.

## 2. Does the browse listing agree with the commit path?

**Yes — they agree about the file, and that is not the same as being correct.**

* Browse **lists** it (mangled, previously unmarked).
* `Engine::pending` **lists** it (mangled).
* The commit walk **stages** it, by its real bytes.
* `stage_and_commit` **commits** it. The tree entry reads
  `"doc-\xffepuap.txt"` — raw.

They agree because *both surfaces mangle in exactly the same way*, so the join
in `browse::classify` lines up by coincidence. That coincidence breaks the
moment two unspellable names share a folder: two files, one pending key, one
row. So the agreement was real but load-bearing on an accident, and
`the_files_pane_and_the_commit_path_agree_about_a_name_neither_can_spell` now
pins it deliberately — comparing `absolute_path` **bytes**, not renderings.

## 3. What git does, and whether the owner's guess was right

**The owner's guess was wrong, and this is the good news half of the report.**

git stores path *bytes* and never decodes them. `gix::path::try_into_bstr` is
infallible on unix (it is `OsString::into_vec`); the `InvalidPathForRemote`
error at `commit.rs:439` that mentions UTF-8 is a **Windows-only** branch and is
unreachable on macOS and Linux. Measured end to end:

```
COMMIT Ok("ok")
AFTER  untracked=[] added=[]
TREE   entry="doc-\xffepuap.txt"
```

The file commits, and would push and clone, like any other. **The only thing in
the system that could not handle the name was keeper's own rendering of it.**
So, per the brief: nothing here tries to make such a file syncable. It already
is, and §4's engine test asserts that it still is.

---

## 4. The change

### 4.1 `names.rs` — new module, the display/reach separation as a type

`UnspellableName { display, escaped }`, constructed only by `of` / `of_path` /
`of_bytes`, each answering `None` for a name that decodes cleanly — so **the
existence of the value is the finding**; there is no boolean a caller can forget
to check.

* `display` — `U+FFFD` rendering. Lossy, non-injective, for a text node.
* `escaped` — byte-exact ASCII, `\xNN` for everything outside printable ASCII,
  `\\` for a literal backslash. Lossless, so two files a user must tell apart
  never collapse. This is what makes the report *actionable*:
  `doc-\xffepuap.txt` says which bytes to go and look for.
* `for_display()` returns `ForDisplay<'_>`, which implements `Display` **and
  nothing else**. `Path::new`, `PathBuf::push` and `Path::join` all want
  `AsRef<Path>`; `ForDisplay` is not, so `root.join(n.for_display())` **does not
  compile**. Getting a `String` needs an explicit `.to_string()` — a decision a
  reviewer sees rather than a coercion nobody notices.

### 4.2 `browse.rs` — refuse at the join, mark at the row

* `plain_segments` refuses any subpath containing `U+FFFD` →
  `BrowseRefusal::Unspellable`. This is the AD-65 choke point, so `resolve`,
  `lexical_join`, `files_write` and `file_serve` all inherit it with **no logic
  change of their own**.
* `BrowseEntry::unspellable: Option<UnspellableName>` — additive. Still listed,
  because a hole in a browser is the silence this story is about.
  `absolute_path` stays the undecoded `OsString`, so reveal/open still work.
* The false comment is replaced with one that names its own counter-example.

### 4.3 `engine.rs` — the report, in the channel that already exists

`sync_problems` fits, so nothing was invented. `ProblemReport.unspellable:
Vec<UnspellableName>`, filled by `Engine::problems` from the **git index**, in
`spawn_blocking` beside the existing conflict scan.

The index, not a status walk, and that choice is the story: a file committed
long ago and clean ever since is exactly what a status walk never mentions, and
exactly the case the owner hit. The index is already resident and `of_bytes`
allocates nothing for a valid path, so a 100 k-file repo pays one byte-scan.

### 4.4 `git/repo.rs` — a second wrong-file site, found while looking

`tracked_paths` built each `PathBuf` with `BStr::to_string` — a lossy decode.
Its caller is the **LFS materialization scan**, which `lstat`s and rewrites what
it is handed. So an unspellable tracked path was either silently skipped or, in
the decoy folder of §0, resolved onto someone else's file. Now `to_path`, the
byte-exact conversion the status path already used.

---

## 5. I/O matrix

| Input (raw bytes on disk) | browse row | `relative_path` fed back | commit walk | index → `problems.unspellable` |
| --- | --- | --- | --- | --- |
| `notes.md` | listed, `unspellable: None` | resolves | staged | absent |
| `zaświadczenie.pdf` (valid UTF-8, non-ASCII) | listed, `None` | resolves | staged | **absent** — non-ASCII is not non-UTF-8 |
| `doc-\xffepuap.txt` | listed, `Some{display,escaped}` | `Err(Unspellable)` | staged, raw bytes | `doc-\xffepuap.txt` |
| `a\xFF.txt` + `a\u{FFFD}.txt` together | two rows, one marked | `Err(Unspellable)` for both | both staged | one entry |
| `a\u{FFFD}.txt` alone (legitimate) | listed, `None` | **`Err(Unspellable)`** — accepted cost, §7 | staged | absent |
| `dir-\xff/` | listed, marked | `Err(Unspellable)`, cannot expand | contents staged | contents reported |
| name with `\n` in it | listed | refused if lossy | staged | `escaped` renders `\x0a` — cannot forge a report line |

## 6. Edge cases

* **Non-ASCII ≠ non-UTF-8.** A Polish or Japanese filename is text and must not
  be reported, or every non-English user gets a permanent warning. Pinned by
  `an_ordinary_name_is_not_a_finding` and
  `an_ordinary_repository_reports_no_unspellable_paths`.
* **A literal backslash in a name.** Without `\\`, a file called `a\xff.txt`
  (nine ASCII characters) and one holding byte `0xFF` would render identically
  and `escaped` would be as useless as `display`. Pinned.
* **Control bytes.** A newline in a filename could make one report entry look
  like two lines. `escape` renders it `\x0a`. Pinned.
* **Directory with an unspellable name.** Listed and marked; cannot be expanded,
  because expansion goes through the refused join. Its *contents* still commit —
  git walks bytes, not our subpaths.
* **Unmerged index stages.** `unspellable_tracked_paths` sorts and dedups, so
  one path held at several stages is reported once, and the order does not
  reshuffle between polls (the index sorts by raw bytes, not by `escaped`).
* **Not-a-repository / unopenable index.** `problems()` degrades to an empty
  list rather than failing a call that has three other answers to deliver; a
  broken repo surfaces through `error`, loudly, where it belongs.
* **The completeness gate still applies.** An odd name is not a fast path; the
  engine test asserts the first scan stages nothing.

## 7. Deliberately NOT done

1. **Making such a file syncable.** It already is (§3). Nothing about the
   carrying changed.
2. **`exclude.rs`'s lossy `match_string`.** Left alone on purpose. Globbing
   *wants* a lossy, total, `/`-normalised string; the mangled form matches no
   built-in pattern, so the file falls through and is *included*, which is the
   safe direction. Making it byte-aware would be a globset rewrite for a
   file that is already carried.
3. **`Engine::pending`'s lossy `PendingFile.path`.** Left as-is. It is a display
   string that now cannot reach anything (the join refuses it), and it agrees
   with the browse rendering, which is what the pane's join needs. Marking it
   too would duplicate `problems.unspellable` with no new information.
4. **A `trybuild` compile-fail test** for `root.join(n.for_display())`. That
   would be a new dev-dependency for a property the allowlist source-scan
   already defends (§8, M8/M10/M11).
5. **The shell projection.** `sync_problems` cannot show
   `ProblemReport.unspellable` until `SyncProblemsVm` carries it. That is
   `keeper/src/sync_ipc.rs`, owned by L5Tail this wave, and per Main's rule I
   did not take a line in it. **Opened as DW-200** — see §10.

## 8. Mutation table — `MUT47-2`, 13/13 killed, 0 survivors

Isolated `CARGO_TARGET_DIR` so peer builds could not confuse the result.

| # | Mutation | Named test that failed | Verdict |
| --- | --- | --- | --- |
| M1 | delete the `U+FFFD` refusal in `plain_segments` | `browse::…::a_lossy_rendering_is_refused_rather_than_resolved_to_a_different_file` | KILLED |
| M2 | `unspellable = None` in the walk | `browse::…::a_name_that_is_not_utf8_is_listed_and_marked_rather_than_dropped` | KILLED |
| M3 | restore `BStr::to_string` in `tracked_paths` | `git::repo::…::the_index_scan_keeps_the_bytes_of_a_name_that_is_not_utf8` | KILLED |
| M4 | `unspellable_tracked_paths` returns nothing | same | KILLED |
| M5 | `problems()` returns an empty `unspellable` | `engine::…::the_files_pane_and_the_commit_path_agree_…` | KILLED |
| M6 | drop `\\` from `escape` | `names::…::escaping_a_backslash_keeps_the_rendering_injective` | KILLED |
| M7 | drop the valid-name guard in `of` | `names::…::an_ordinary_name_is_not_a_finding` | KILLED |
| M8 | add `impl AsRef<std::path::Path> for ForDisplay` | `names::…::a_source_scan_proves_no_path_conversion_was_ever_added` | KILLED *(see below)* |
| M9 | `#[serde(skip)]` on `escaped` | `engine::…::the_visibility_types_cross_the_ipc_boundary_as_camel_case` | KILLED *(second attempt — see below)* |
| M10 | add `impl core::ops::Deref for ForDisplay` | `names::…::a_source_scan_proves_no_path_conversion_was_ever_added` | KILLED |
| M11 | add `impl From<&UnspellableName> for PathBuf` | same | KILLED |
| M12 | `#[serde(skip)]` on `display` | `engine::…::the_visibility_types_cross_the_ipc_boundary_as_camel_case` | KILLED |
| M13 | `escaped` built by `from_utf8_lossy` instead of `escape` | same | KILLED |

**M8 survived the first pass, and the survivor is worth recording.** The guard
test was a *blocklist* of exact strings including
`"impl AsRef<Path> for ForDisplay"`. The mutation spelled it
`impl AsRef<std::path::Path> for ForDisplay<'_>` and walked straight past. There
is no finite list of ways to write a trait path, so the test was inverted into an
**allowlist**: exactly two `impl` lines may mention these types, and any other is
a failure whatever it is called. M8, M10 and M11 are the three spellings that
now kill it, plus a non-vacuity check so the test cannot pass over a file whose
impls were renamed away.

**M9's first KILL verdict was worthless, and that is the more embarrassing
finding.** The sweep reported it killed, but the test it named was *already
failing unmutated* — my assertion spelled the replacement character as the six
literal characters `\ufffd` inside a Rust raw string, while `serde_json` writes
the character itself. A mutation "kills" a test that never passed, so the run
proved nothing. It surfaced only because I ran the **whole** `--lib` suite
rather than the filtered subset I had been iterating on — the filters I used
during development never once selected that test. Corrected, verified green
unmutated, then M9 re-run alongside M12 and M13 so both renderings and the
byte-exactness of `escaped` are each independently pinned.

The lesson worth carrying: **a filtered green is not a green.** A mutation
verdict is only meaningful against a test that is known to pass first.

**Sweep hygiene.** The first attempt died mid-run (harness timeout). Treated as
a crash: grepped `MUT47-2` immediately — clean — then re-ran in a fresh process.
Restore verified by **reading `git diff`**, which shows exactly the seven lines I
meant to replace and no stray hunk. `names.rs` is a *created* file, invisible to
`git diff`, so it was checked by name and by its eight passing tests.

## 9. What I could not verify here, and why — ordered gate checks

The `keeper` shell crate does not build on this Linux box (no GTK/webkit), so
per the wave rule I never ran `cargo build/check/clippy/test -p keeper`.

Everything in this story is inside `keeper-sync`, which **does** build and test
on Linux, and all of it is proved above. Two things are not:

1. **`keeper` still compiles.** I added fields to two `pub` structs the shell
   reads — `BrowseEntry` and `ProblemReport`. Both are additive and the shell
   constructs neither (grepped: no `BrowseEntry` or `ProblemReport` literal in
   `crates/keeper/src`), so this should be a no-op. `BrowseRefusal` gained a
   variant, which **does** break exhaustive matches; I found and fixed the two
   in this crate (`export.rs`), and L5Tail added the third in `files_write.rs`
   at my request. A fourth outside `keeper-sync` would fail the gate.
2. **The row and the report as a human sees them.** Nothing is wired to a
   surface yet (§7.5 / DW-200).

**Gate order, on `hesperia`:**

```
1  cargo build -p keeper                       # the variant/field blast radius
2  cargo clippy -p keeper-sync -p keeper -- -D warnings
3  cargo test  -p keeper-sync                  # 74 of these are mine
4  cargo test  -p keeper
5  bun run check
```

Step 1 is the one that can fail. If it does, it will be one missing match arm on
`BrowseRefusal::Unspellable`, and the right arm is the one both `export.rs` and
`files_write.rs` already use: map it onto that consumer's existing "escapes"
refusal. Do not invent a new user-facing variant for it — the sentence
`BrowseRefusal` writes is the one the reader needs.

## 10. Ledger

* **Closed:** none. No existing DW entry covered this; grep of
  `deferred-work.md` for `UTF-8`/`utf8`/`non-UTF`/`epuap` found nothing.
* **Opened — DW-200:** *`ProblemReport.unspellable` is produced and reaches no
  surface.* `Engine::problems` now reports every tracked file whose name is not
  valid UTF-8, with a lossy rendering and a byte-exact one, and
  `keeper/src/sync_ipc.rs:1061` hand-projects `ProblemReport` field by field
  into `SyncProblemsVm` — so the field is dropped on the floor. Needs one field
  on `SyncProblemsVm`, one line in that projection, the generated TS type, and a
  row in the Problems pane. Type is
  `keeper_sync::names::UnspellableName { display, escaped }`, serde camelCase;
  render `display` and offer `escaped` as the copyable "how to find it". L5Tail
  has the exact lines and will carry them **iff** layer 2 lands below layer 5;
  otherwise this belongs to a later layer. Gate-only either way — `sync_ipc.rs`
  compiles on neither of our machines.
* **Touched, left open:** none.

## Found at the macOS gate: you cannot make one of these on a Mac

Four of this story's tests were green on Linux and red on hesperia, all with the same error:
`Os { code: 92, kind: Uncategorized, message: "Illegal byte sequence" }` — `EILSEQ`, raised by
`std::fs::write` before keeper's code ran at all.

**APFS validates filename bytes and refuses anything that is not valid UTF-8.** The fixture this
story is about cannot be created on the machine keeper ships from.

That is a fact about the filesystem and not about keeper, and it makes the defect **more**
interesting rather than theoretical:

- A Mac cannot **create** such a name.
- A Mac can very easily **receive** one. Git stores raw bytes, a Linux peer can commit one, and
  the thing that delivers it is this crate. The owner's own report is exactly this shape — the
  file is on an external volume in a repo that syncs to a Linux host.
- So the surface that must not silently resolve it to a different file is precisely the surface a
  Mac user sees, and it is the one platform where no test can build the fixture.

The four disk-dependent tests now step aside where the filesystem refuses, printing
`skipped: this filesystem refuses a non-UTF-8 filename (macOS/APFS EILSEQ)` so a green run that
skipped is distinguishable from a green run that checked. `create_unspellable` swallows **only**
`EILSEQ`; any other error still panics, because a test that skips on a permissions bug reports
success for a run that proved nothing.

The rules those tests are about are pure and keep running on every platform: `UnspellableName::of`,
the display/reach separation, and the U+FFFD refusal in `plain_segments` are asserted in
`names.rs` without touching a disk. What macOS cannot check here is the wiring between the rule
and the filesystem — which is why gate check 3 below is a real one and not a formality.
