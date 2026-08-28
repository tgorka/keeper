# Spec 46.1 — A Path With a Space Is One Pattern

status: implemented
story: Epic 46, Story 46.1
bindings: FR-137 (one `.gitattributes` write per session), AD-46 (LFS routing at stage time)
crates: `keeper-sync` only — compiled and tested on Linux, in full
frontend: none
deferred: DW-193, DW-194

---

## What this story turned out to be about

Not "quote the pattern". **The writer and the reader are one defect wearing two faces, and
fixing either one alone makes the product worse.**

keeper wrote `.gitattributes` patterns unquoted, so a path with a space produced a line git
parses as something else entirely. That is the visible half. The invisible half is that
`ensure_attributes`' own coverage check tokenised the line the same way the bug did — first
blank-separated field, compared raw — so it *agreed with the corruption*. The two errors
cancelled into a third: the broken line's first field is `/2021`, which never equals the pattern
`/2021 holiday/clip`, so the rule read as absent and was appended again on every commit that
re-staged the path. Fifty-nine copies.

Fix the writer alone and the arithmetic gets **worse**: a correctly quoted line tokenises to
`"/2021` under a blank-splitting reader, which also never matches, so the rule is re-appended on
every run — now for *every* spaced path, forever, and FR-137's "one `.gitattributes` write per
session" is violated by construction rather than by accident. That is why this is one commit.

---

## The claim in the brief, verified rather than assumed

The brief asserted that backslash escaping does not work and that quoting is the only escape.
Checked directly against **git 2.53.0** before any code was written, because the whole design
rests on it:

| `.gitattributes` line | `git check-attr filter -- "a b"` | `git check-attr b -- "a"` |
| --- | --- | --- |
| `/a b filter=lfs …` | `unspecified` | **`b: set`** |
| `/a\ b filter=lfs …` | `unspecified` | `unspecified` |
| `"/a b" filter=lfs …` | **`lfs`** | — |

Row 1 is the defect in one line: the pattern silently became `/a`, and `holiday/clip` — here
`b` — was read as an **attribute name to set on it**. Row 2 shows why the obvious cheaper fix is
not a fix: git decides to unquote from the line's first byte, before it has looked for a
separator, so a backslash never gets the chance to protect a blank. It is a wildmatch glob
escape, applied *after* unquoting, which is a different layer (and the subject of DW-194).

**Two things the brief did not ask about, checked because the design depends on them:**

1. **gitoxide agrees with git.** `gix-attributes-0.34.0/src/parse.rs:130` branches on
   `line.starts_with(b"\"")` and calls `gix_quote::ansi_c::undo` — the same two branches from the
   same byte. This matters beyond parsing: `is_false_modification` asks `routed_through_lfs`,
   which resolves attributes through gitoxide's stack. Had gitoxide *not* unquoted, this fix
   would have written lines git honours and keeper cannot see.
2. **Quoting does not rescue a leading `!`.** `"!notes" filter=lfs` still warns
   `Negative patterns are ignored in git attributes` — git checks for negation after unquoting.
   Unreachable from either producer (`pattern_for` always emits `/…` or `*.…`), so it is
   recorded here rather than defended against.

---

## Where the quoting lives, and why not where the brief pointed

The brief said to apply `quote_pattern` at `stage.rs:167` and `:170`, i.e. inside `pattern_for`.
**I put it at the write site in `ensure_attributes` instead**, and this is the one place I
deviated, so here is the reasoning.

Quoting is a property of the **line**, not of the **pattern**. Making `pattern_for` return
`"/my holiday"` would mean:

* the idempotence reader has to unquote *both* sides to compare, since `pattern` is what it is
  compared against — the fix in point 3 of the brief only composes if `pattern` is raw;
* `engine.rs:1857` logs `pattern` to the user's log at `INFO`, and would start printing quotes
  and octal escapes;
* `prepare`'s `patterns.contains(&pattern)` dedup would compare serialized forms.

I checked every producer and every consumer before deciding. `pattern_for` has exactly one
non-test caller (`prepare`, `stage.rs:532`) and `pattern_for_extension` exactly one
(`engine.rs:1850`); both hand the result straight to `ensure_attributes`. So there is no caller
that writes a pattern to a file without passing through the one place quoting now happens, and
the placement costs nothing while keeping the pattern a value. `pattern_for`'s doc comment now
says so explicitly, so the next reader does not have to re-derive it.

---

## I/O matrix

### `quote_pattern(&str) -> String`

| Input pattern | Output | Why |
| --- | --- | --- |
| `*.mp4` | `*.mp4` | **byte-identical** — nothing forces quoting |
| `*.tar.gz` | `*.tar.gz` | byte-identical |
| `/bin/blob` | `/bin/blob` | byte-identical |
| `/archive/café.mp4` | `/archive/café.mp4` | byte-identical; `≥0x80` never forces quoting |
| `/a*b`, `/a!b`, `/a#b` | unchanged | glob operators and a non-leading `#` are not quoting problems |
| `/my holiday` | `"/my holiday"` | blank |
| `*.2 final draft` | `"*.2 final draft"` | blank, from the extension branch |
| `/a\tb` | `"/a\tb"` | C0 control with a letter escape |
| `/a"b` | `"/a\"b"` | the encoding must escape it |
| `/a\b` | `"/a\\b"` | the encoding must escape it |
| `/a\x01b` | `"/a\001b"` | control with no letter escape → three octal digits |
| `/a\x7fb` | `"/a\177b"` | DEL |
| `#notes` | `"#notes"` | a leading `#` is a comment before it is anything else |
| `` (empty) | `""` | would otherwise collapse into the separator |

Forcing bytes: `b <= b' '` (blanks and all C0), `0x7f`, `"`, `\`; plus a leading `#`; plus empty.
Deliberately **not** forcing: `≥ 0x80`. Octal-escaping a UTF-8 name would rewrite
`/archive/café.mp4` into `"/archive/caf\303\251.mp4"` in a versioned, hand-editable file that
reads it fine bare — churn for no behaviour, and the same argument that makes conditional
quoting mandatory in the first place.

### `attribute_pattern_matches(line, pattern) -> bool`

| Line | Pattern asked | Answer | Why |
| --- | --- | --- | --- |
| `"/my holiday" filter=lfs …` | `/my holiday` | `true` | the line keeper writes now |
| `*.mp4 filter=lfs …` | `*.mp4` | `true` | bare legacy line — older keeper, `git lfs track`, or the user |
| `"*.mp4" filter=lfs` | `*.mp4` | `true` | quoted spelling of a pattern that needs no quotes |
| `"/my holiday"` | `/my holiday` | `true` | attributes are checked separately by the caller |
| `"/my other" filter=lfs` | `/my holiday` | `false` | discrimination, not a blanket yes |
| `/2021 holiday/clip filter=lfs` | `/2021 holiday/clip` | `false` | the legacy BROKEN line is not coverage — see DW-193 |
| `"/unterminated filter=lfs` | `/unterminated` | `false` | malformed quoting |
| `"*.mp4"x filter=lfs` | `*.mp4` | `false` | field does not end at its closing quote |

Both failure shapes answer "absent", which costs one appended rule and never a missing one. That
is the safe direction here for the same reason it is in `is_false_modification`: a duplicate rule
is noise, an absent rule turns a multi-gigabyte file into a git blob.

### `ensure_attributes(root, patterns) -> Result<bool>`

| State | Call | Result |
| --- | --- | --- |
| no file, `["*.mp4"]` | 1st | `true`; `*.mp4 filter=lfs diff=lfs merge=lfs -text` — **unchanged from before this story** |
| same | 2nd | `false`, file untouched |
| no file, `["/2021 holiday/clip"]` | 1st | `true`; `"/2021 holiday/clip" filter=lfs …`, exactly one line |
| same | 2nd, 3rd | `false`, file byte-identical |
| file written by pre-46.1 keeper, same patterns | 1st | `false`, file byte-identical — no migration, no churn |
| user's `*.psd filter=lfs`, ask `*.psd` | 1st | `false` — coverage the user spelled is still respected |

---

## Edge cases

* **Extension containing a blank.** `Path::extension()` splits at the *last* dot, so
  `v1.2 final draft` yields the extension `2 final draft` and the pattern `*.2 final draft`.
  Covered and quoted. Whether that is a *sensible* rule is a separate question the story does
  not open — see "deliberately not done".
* **Blank in a parent directory.** `my media/blob` reaches the exact-path branch and produces
  `/my media/blob`. Same treatment; the blank does not have to be in the leaf.
* **`engine.rs:1802` already refuses whitespace extensions**, so `ensure_lfs_rule`'s recording
  path could never produce a spaced pattern. It is left exactly as it is: it also refuses `/`
  and `\`, which is a statement about what an extension *is*, not about quoting.
* **UTF-8.** Passed through verbatim inside quotes when quoting is forced for another reason.
  Verified against git: `"/café.mp4"` with raw UTF-8 bytes resolves.
* **Empty pattern.** Unreachable from both producers; quoted anyway, so a hypothetical one
  produces a line git skips rather than a line that sets `filter` on nothing.
* **Trailing blank** (`/trailing space `): round-trips, and git resolves the trailing-space path.
* **Octal boundary.** Every byte that reaches the octal arm is `< 0x20` or `0x7f`, so its leading
  digit is `0` or `1` — inside the `\0`–`\3` opener `undo` accepts. Asserted, not reasoned about.

---

## Mutation table

Each half reverted **separately**, as the brief required. Method: byte-exact backup
(`sha256 6066ab38…`), mutate with a `MUT461*` sentinel, run, restore from backup, re-verify the
hash and re-grep the sentinel across the worktree.

| # | Mutation | Caught by | Result |
| --- | --- | --- | --- |
| A | Writer: `quote_pattern(pattern)` → `pattern` (revert quoting) | `a_spaced_pattern_is_written_once_and_recognised_on_the_next_run`, `a_path_with_a_space_is_one_pattern_that_git_resolves`, `re_running_ensure_attributes_for_a_spaced_path_leaves_the_file_untouched` | **3 caught** |
| B | Reader: `attribute_pattern_matches` body → pre-46.1 `line.split_whitespace().next() == Some(pattern)` | `coverage_is_recognised_in_both_the_quoted_and_the_bare_spelling`, `a_spaced_pattern_is_written_once_…`, `re_running_ensure_attributes_…` | **3 caught** |
| C | `needs_quotes` → `true` (quote unconditionally) | `a_pattern_needing_no_quotes_is_emitted_byte_identically`, `attributes_are_written_once_and_are_idempotent`, `a_non_lfs_rule_for_the_same_pattern_does_not_count_as_coverage`, `only_the_gitattributes_rule_makes_an_untouched_pointer_dismissible` | **4 caught** |
| D | Encoder: drop the `'\\' => "\\\\"` escape | `quoting_is_the_exact_inverse_of_the_decoder_git_and_gitoxide_use`, `quoting_appears_only_for_a_pattern_that_needs_it` | **2 caught** |

C is the one worth naming: it proves the *byte-identical* acceptance criterion is defended by a
test rather than asserted in prose, and its blast radius — four tests, one of them a pre-existing
roundtrip test in a different file — is the measure of how much churn unconditional quoting would
have caused in the field.

Restoration verified by reading `git diff --stat` and `git diff` on all four touched files plus
`Cargo.lock`, and by a `grep -rn MUT461` sweep over the worktree returning nothing — not by
recalling what was changed. `git diff` is blind to created files; the two files this story
creates are this spec and nothing else (the DW entries are appended to an existing file).

---

## Verification actually run

| Command | Result |
| --- | --- |
| `cargo check -p keeper-sync --lib` | clean |
| `cargo test -p keeper-sync --lib lfs::` | **EXIT=0**, 116 passed / 0 failed |
| `cargo test -p keeper-sync --test lfs_roundtrip` | **EXIT=0**, 15 passed / 0 failed |
| `git check-attr` matrix, git 2.53.0 | as tabulated above |

The roundtrip test invokes the real `git` binary with `check-attr -z` (NUL-separated, so the test
is not parsing git's *own* output quoting) and **asserts stderr is empty** — which matters here
specifically: an unparseable gitattributes pattern is not an error. git exits 0 and says nothing.
An exit-status assertion alone would have passed against the bug.

The roundtrip test also pins the bug's signature rather than only the fix: it asks git about the
path `2021` and asserts `unspecified`. Under the old spelling that path resolved to `lfs`,
because `/2021` was what the pattern had silently become.

### Dependency

`gix-quote = "0.7.2"` added to `[workspace.dependencies]` and to `keeper-sync`. **This adds no
new crate**: `gix-attributes` already depends on it, and it was already resolved at 0.7.2 in
`Cargo.lock`. Proven, not assumed — `git diff Cargo.lock` is exactly **one line**, a new edge in
`keeper-sync`'s dependency list, with no new `[[package]]` block. Licence `MIT OR Apache-2.0`,
single version in the lock, so `cargo deny check` has nothing new to decide.

The crate ships `ansi_c::undo` and **no encoder**, which is why `quote_pattern` is ~20 lines of
hand-written Rust. Being hand-written against a decoder used by both git's peer implementation
and keeper's own attribute resolution is exactly the risk that
`quoting_is_the_exact_inverse_of_the_decoder_git_and_gitoxide_use` exists to pin: a divergence
there would not surface as a test failure, it would surface as `routed_through_lfs` quietly
failing to see a rule keeper itself wrote.

---

## Deliberately NOT done

1. **The fifty-nine existing broken lines are not repaired.** Carved out by the epic spine; filed
   as **DW-193**, naming `ensure_attributes`' whole-file rewrite as the mechanism that would do
   it, and stating why it is a product decision (`.gitattributes` is versioned and
   user-editable; the managed marker says where keeper's block starts, not that every line below
   it is still keeper's) rather than part of a bug fix. After this story they stop multiplying —
   the correctly quoted line is recognised on the next run — they just sit there inert.
2. **Glob metacharacters in exact-path patterns are not neutralised.** Filed as **DW-194** with
   measured evidence: `"/notes/draft[final]"` resolves the real file to `unspecified` and
   `notes/draftf` to `lfs`. Quoting does not help, and the entry says why — unquoting runs first
   and *produces* the pattern, then wildmatch interprets it, so the two layers compose rather
   than cancel. Separate cause, separate fix, and it interacts with DW-193.
3. **`Path::extension()`'s last-dot rule is not changed.** `v1.2 final draft` producing
   `*.2 final draft` is odd, and it is odd for a reason that predates this story. Narrowing what
   counts as an extension would change which files get routed through LFS in existing
   repositories — a behaviour change wearing a bug fix's clothes. This story makes the pattern
   *correct as a line*; it does not relitigate what the pattern should be.
4. **`ensure_lfs_rule`'s whitespace refusal is left in place** (`engine.rs:1802`). It is now
   technically unnecessary for the quoting reason its comment gives, but it also refuses `/` and
   `\`, and the judgement it encodes — "this is not an extension, whatever produced it" —
   survives this story intact. Rewriting a comment in another story's function to say the
   quoting half is handled elsewhere would have widened the diff into `engine.rs` for no
   behaviour.
5. **No formatter, no linter, no full suite.** Per the wave constraint; Main runs those once.

---

## What I could not verify here, and why

Honest scope: **almost nothing was out of reach.** `keeper-sync` is `tauri`-free and
`keeper-core`-free by design (AD-40), so it compiles and its tests run in full on Linux. This
story touched no shell code, no frontend, and no platform-conditional path. Everything in the
matrices above was executed on this machine.

Four residuals:

1. **git on macOS is a different binary.** Everything was measured against git 2.53.0 on Linux.
   Apple ships an older git, and the roundtrip test shells out to whatever `git` is on `PATH`.
   C-style quoting in gitattributes long predates any version either box will have, so I expect
   no difference — but "I expect" is not "I measured".
2. **`cargo fmt --check` and `cargo clippy` have not been run** on the new code. Instructed not
   to; the code was written to match the surrounding style but a formatter has not confirmed it.
3. **The workspace as a whole has not been built.** I added an entry to
   `[workspace.dependencies]`, which no crate but `keeper-sync` names, so it cannot affect
   another crate's resolution — and the lock diff confirms nothing else moved. Still unbuilt as a
   whole, and `-p keeper` cannot be built on Linux at all.
4. **No real repository with the owner's fifty-nine lines was available.** The broken-line shape
   is reproduced from the diagnosis and asserted as a unit case
   (`/2021 holiday/clip filter=lfs` is not coverage), not read off the owner's disk.

### Gate checks, in order, on macOS

1. `cargo fmt --check` and `cargo clippy -p keeper-sync --all-targets -- -D warnings` — expect
   clean; this is the only unrun static gate on the new code.
2. `cargo test -p keeper-sync --lib lfs::` — expect EXIT=0. Re-runs the whole matrix under
   whatever git the box has for the roundtrip file below.
3. `cargo test -p keeper-sync --test lfs_roundtrip` — expect EXIT=0. **This is the macOS-git
   check**: `a_path_with_a_space_is_one_pattern_that_git_resolves` asserts empty stderr and four
   resolved attributes against the real binary. If Apple's git differs at all, it fails here.
4. `cargo deny check` — expect no new advisory or licence decision; `gix-quote` is
   `MIT OR Apache-2.0` and was already in the tree.
5. Build the app and, in a sync folder with LFS enabled, drop an oversized file whose **name
   contains a space** (e.g. `2021 holiday/clip`). Commit it twice. Expect: exactly one line in
   `.gitattributes`, quoted, and `git check-attr filter -- "2021 holiday/clip"` reporting `lfs`.
   The second commit must add nothing — that is FR-137, and it is the acceptance the owner will
   actually see.
6. In the same folder, confirm an existing `*.mp4` line is **byte-identical** after step 5 —
   `git diff .gitattributes` should show only the added line.
