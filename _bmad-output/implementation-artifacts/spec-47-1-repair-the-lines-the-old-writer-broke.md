# Spec 47.1 — Repair the Lines the Old Writer Broke

status: implemented
story: Epic 47, Story 47.1
closes: DW-193, DW-194
bindings: FR-137 (one `.gitattributes` write per session), AD-46 (LFS routing at stage time)
crates: `keeper-sync` only — compiled and tested on Linux, in full
frontend: none
files: `src-tauri/crates/keeper-sync/src/lfs/stage.rs`,
`src-tauri/crates/keeper-sync/tests/lfs_roundtrip.rs`
predecessor: `spec-46-1-a-path-with-a-space-is-one-pattern.md`

---

## What this story turned out to be about

46.1 stopped keeper writing broken lines. It did not remove the ones already
written, and the owner has **59 of them in a live repository**. This story is
the removal — and the interesting part is not the rewrite, which
`ensure_attributes` was always structurally capable of (it reads the whole file
and writes the whole file). It is **the boundary**: which lines may be touched.

The two DW entries turned out to be one change, for a reason neither entry
anticipated. Repair must re-emit the pattern through the *current* writer, or
the repaired line is not the line a fresh write would produce and the next run
appends a second copy — reintroducing the duplicate the repair exists to
collapse. So the moment DW-194 changes what the writer emits for a path holding
`[`, DW-193's repair has to change with it. They compose or they fight; there is
no version where only one ships.

---

## The measurements this design rests on, taken before any code was written

git 2.53.0, `git check-attr -z filter`, empty repository, one line at a time.

| `.gitattributes` line | path asked | resolves | stderr |
| --- | --- | --- | --- |
| `/2021 holiday/clip filter=lfs diff=lfs merge=lfs -text` | `2021 holiday/clip` | `unspecified` | `holiday/clip is not a valid attribute name` |
| same | `2021` | **`unspecified`** | same |
| `"/report [final].pdf" filter=lfs` | `report [final].pdf` | **`unspecified`** | — |
| same | `report f.pdf`, `report i.pdf` | **`lfs`** | — |
| `"/report \\[final].pdf" filter=lfs …` | `report [final].pdf` | **`lfs`** | — |
| same | `reportf.pdf`, `report [xinal].pdf` | `unspecified` | — |
| `"/a \\*b" filter=lfs` | `a *b` | `lfs` | — |
| same | `a zb` | `unspecified` | — |
| `/lit\?q filter=lfs` (bare, escaped) | `lit?q` | `lfs` | — |
| same | `litxq` | `unspecified` | — |

Three of these changed the design:

1. **Row 2 corrects 46.1's matrix.** 46.1 measured `/a b filter=lfs …` and found
   `b: set` on the pattern `/a` — a *valid* attribute name really is set. The
   owner's line has `holiday/clip`, which holds a slash and is **not** a valid
   name, and git then discards the **entire line**: even `/2021`, the pattern
   the line silently became, gets nothing. So the owner's 59 lines are wholly
   inert. They are not doing something subtly wrong; they are doing nothing,
   loudly, once per line per operation. Asserted as a pre-condition in
   `the_broken_lines_already_on_disk_are_repaired_and_git_stops_complaining`, so
   the fixture cannot rot into one that no longer reproduces the defect.
2. **Rows 3–4 are DW-194 in two lines.** Quoting does not touch the glob layer;
   it *feeds* it. The merely-quoted line gets the real file exactly backwards.
3. **Rows 9–10 show the escape working without the quoting layer**, which is
   what proves the two are independent layers rather than one mechanism.

---

## DW-194 — the two layers, worked out before coding

A `.gitattributes` line is read in **two passes, in this order**:

```
"/report \\[final].pdf" filter=lfs …      the LINE          (bytes on disk)
        │ C-unquote (only if byte 0 is `"`)
        ▼
/report \[final].pdf                      the PATTERN       (wildmatch input)
        │ wildmatch (`* ? [` operators, `\` escape)
        ▼
report [final].pdf                        the PATH, literally
```

So the producer applies them in the order git strips them off:
**glob-escape the literal first** (that is what makes the pattern), **then
`quote_pattern`** (that is what makes the line). Reversed, `quote_pattern`'s
output would be the thing glob-escaped: its own `"` delimiters and its `\001`
octal escapes would each collect a backslash they must not have, and the line
would decode to something no path equals.

### Where the escaping lives, and why not where the quoting lives

46.1 deliberately kept quoting **out** of `pattern_for`, at the write site,
because quoting is a property of the *line*. Glob escaping is the exact mirror
and belongs **in** `pattern_for`, for the same underlying reason: `\[` is not
another spelling of `[`, it is a **different pattern**. Three consequences, each
checked against a real caller before deciding:

* `attribute_pattern_matches` compares a decoded line field against `pattern`.
  If escaping happened at the write site, the reader would compare a raw pattern
  against an escaped stored field, never match, and re-append on every run —
  46.1's disaster, verbatim.
* `prepare`'s `patterns.contains(&pattern)` dedup would compare two
  representations of one rule.
* `engine.rs:1857` logs the pattern at `INFO`. It now prints `\[`, which is
  correct: that *is* the rule, and a log claiming otherwise would be the lie.

### Which bytes, and one that is deliberately excluded

Escaped: `*`, `?`, `[` (wildmatch operators) and `\` (its escape, so a literal
backslash needs one of its own).

**Not** escaped: `]`. A `]` can only close a character class; every `[` is
escaped, so no class is ever open, and a bare `]` stands for itself. Escaping it
anyway would be a correct pattern spelled in a way keeper does not spell it, and
`repair_managed_block` would then have a second spelling to recognise. Proven
rather than reasoned: the roundtrip test resolves `report [final]` — whose `]`
is emitted bare — through real git.

### Which half of each branch is a literal

| Producer | Syntax that stays live | Literal that is escaped |
| --- | --- | --- |
| `pattern_for` exact-path | leading `/` (root anchor) | the whole path, `/` separators included (never escaped) |
| `pattern_for_extension` | leading `*.` (the wildcard the rule is made of) | the extension text behind it |

Escaping the extension branch's own star would produce `\*.2 final draft` — a
working line matching a file literally called `*`, which is the same class of
silent miss the story exists to end. Mutation F pins it.

### The property that makes composition safe

`needs_quotes` already forces quoting on `\`. So **a pattern carrying a glob
escape is always quoted**, and therefore **no bare line keeper has ever written
can contain a backslash**. That is not a coincidence to be grateful for; it is
the load-bearing fact that lets the repair re-escape a bare line's raw pattern
with no risk of doubling an escape that was already there. Asserted in
`the_glob_escape_and_the_quoting_compose_in_that_order`, not left as prose.

---

## DW-193 — the boundary

`ensure_attributes` gained one pre-pass, `repair_managed_block`, which runs
**before** coverage is computed. Ordering matters: a broken line about to become
coverage for one of `patterns` must not also be appended, or the file ends up
holding the repaired rule *and* a fresh copy of it.

A line is repaired only when **all four** hold:

1. **Below `MANAGED_HEADER`.** Above it is the user's file. A line up there
   broken in precisely the same way is left broken. "It was broken anyway" is
   exactly the reasoning that turns a fixer into a tool that rewrites
   hand-edited files.
2. **It ends with `ATTRIBUTE_SUFFIX` after exactly one blank** — the writer's
   `format!` read backwards. `strip_suffix` matches from the right, which is the
   correct greed: a path literally named
   `x filter=lfs diff=lfs merge=lfs -text` recovers whole rather than truncated.
   The single blank is required for the same reason — the writer emits one, so
   two blanks means a raw pattern that *ends* in a space, which is a path keeper
   can be asked to track.
3. **The raw begins `/` or `*.`** — the only two shapes `pattern_for` and
   `pattern_for_extension` can produce. This also excludes an already-quoted
   line (it begins `"`), which is what stops the repair quoting its own output.
4. **The raw holds a blank**, i.e. the bare line provably does not say what it
   was written to say.

### Condition 4 is the whole argument

It is deliberately narrower than "a line this version would spell differently".

A line git parses exactly as written is **evidence of nothing**.
`/media/** filter=lfs diff=lfs merge=lfs -text` below the header may be a
hand-added recursive glob or a file literally named `**`, and nothing in the
file distinguishes them. Rewriting it would be guessing, and guessing wrong
turns a working rule into a rule for a filename nobody has.

A line git **rejects** has exactly one reading. Nobody writes an unparseable
rule on purpose, and the shape the old writer emitted is known
(`format!("{raw} {ATTRIBUTE_SUFFIX}")`), so `raw` is recoverable **by
construction** rather than by inference. That is what makes the repair
reversible in meaning: the repaired line expresses what the original line was
trying to express, and the claim is provable rather than plausible.

This also settles the authorship question the boundary raises. Condition 2
cannot distinguish keeper from a human who typed keeper's exact four-attribute
suffix by hand — and it does not need to. For a line satisfying condition 4 the
repair is meaning-preserving for **both** authors, because both were trying to
name a path that the bare spelling does not name.

### Repair is re-emission, not patching

The repaired line is `keeper_line(pattern)` — the same call a fresh write uses.
Three things follow for free:

* it is a **fixpoint**: a second call re-derives identical bytes, so no write;
* it **converges with the writer**, so the pattern reads as covered and is not
  appended on top of the line that now covers it;
* it picks up `escape_globs`, which it must, per the opening argument.

### Duplicates

Repair alone would leave 59 lines that all parse and all say the same thing: git
goes quiet and the file is still absurd. **Deduplicating is the right answer and
it is done.** Every rule that a later line repeats is dropped.

**The LAST copy is kept, and the direction is load-bearing.** git applies every
matching line in order with the later winning, so removing an *earlier* line
that a later one repeats provably cannot change what any path resolves to —
everything it set is set again below it. Removing the later one can:

```
# keeper-sync: managed LFS rules — edit above this line
*.mp4 filter=lfs diff=lfs merge=lfs -text
*.mp4 -filter                                 ← hand-added override
*.mp4 filter=lfs diff=lfs merge=lfs -text     ← currently beats it
```

Keep-first would silently flip `*.mp4` out of LFS. Keep-last cannot. Mutation C2
is exactly this file.

**A duplicate that is not byte-identical**: the key is the **decoded pattern**,
not the line's bytes, so it collapses too. `*.mp4 …` and `"*.mp4" …` are one
rule written twice; after repair, so are `/a b …` and `"/a b" …`. Candidacy is
restricted to lines carrying keeper's exact `ATTRIBUTE_SUFFIX`, which is what
makes the drop safe — two such lines assign the identical set to the identical
pattern, so one is unreachable noise. A line assigning anything else
(`*.psd filter=lfs`, or the same pattern with `-text` dropped) is not a
duplicate of anything and is never a candidate, because collapsing it could drop
an attribute the user asked for.

**The key is bytes, not a rendering.** `gix_quote::ansi_c::undo` can decode a
hand-written `"\377"` to a pattern that is not UTF-8. Keying on
`String::from_utf8_lossy` would render `"\377"` and `"\376"` both as U+FFFD,
compare them equal, and **delete one of the user's rules** — the same shape as
the browse defect Story 47.2 found, where two distinct names rendered to one
string and the code acted on the render. `pattern_field` therefore returns
`Cow<[u8]>` and never a `String`. Pinned by
`two_rules_differing_only_in_a_non_utf8_byte_are_not_one_rule`, whose last
assertion is that the two patterns *are* indistinguishable once rendered — so
the test states the hazard rather than merely avoiding it. Mutation G.

Two rules that mean the same thing but decode to different patterns (a
hand-added `/media/**` beside keeper's `/media/x`) are not duplicates and both
survive. Deciding otherwise would mean reasoning about what a pattern *matches*,
which is the guessing condition 4 refuses.

---

## I/O matrix

### `escape_globs(&str) -> Cow<str>` (the literal half of a pattern)

| Input | Output | Why |
| --- | --- | --- |
| `2021/holiday` | borrowed, byte-identical | no wildmatch syntax; `/` is never escaped |
| `café.mp4` | borrowed, byte-identical | `≥0x80` is not syntax |
| `report [final]` | `report \[final]` | class opener |
| `a]b` | borrowed, byte-identical | no class can be open |
| `a*b`, `a?b` | `a\*b`, `a\?b` | operators |
| `a\b` | `a\\b` | the escape needs one of its own |
| `**` | `\*\*` | two literal stars |

### `pattern_for` / `pattern_for_extension`

| Path | Pattern | Note |
| --- | --- | --- |
| `media/clip.mp4` | `*.mp4` | **byte-identical to pre-47.1** |
| `bin/blob` | `/bin/blob` | byte-identical |
| `2021 holiday/clip` | `/2021 holiday/clip` | byte-identical — 46.1's case is untouched |
| `archive/café.mp4` | `*.mp4` | byte-identical |
| `report [final]` | `/report \[final]` | exact-path branch |
| `report [final].pdf` | `*.pdf` | the bracket never reaches a pattern from this name |
| `2021 [q4]/clip` | `/2021 \[q4]/clip` | both defects, one line |
| `a/b*c`, `who?` | `/a/b\*c`, `/who\?` | |
| `v1.a[b]` | `*.a\[b]` | extension branch; its own star stays live |
| `x.a\b` | `*.a\\b` | reachable: `Path::extension` of a name holding a backslash |

### `repaired_rule(body) -> Option<String>`

| Body below the header | Result | Why |
| --- | --- | --- |
| `/2021 holiday/clip filter=lfs diff=lfs merge=lfs -text` | `"/2021 holiday/clip" …` | the owner's line |
| `/my holiday filter=lfs … -text` | `"/my holiday" …` | exact-path branch |
| `*.2 final draft filter=lfs … -text` | `"*.2 final draft" …` | extension branch, star kept live |
| `*.2 fin[a]l filter=lfs … -text` | `"*.2 fin\\[a]l" …` | both layers, extension branch |
| `/2021 [q4]/clip filter=lfs … -text` | `"/2021 \\[q4]/clip" …` | both layers, exact-path branch |
| `/odd filter=lfs … -text filter=lfs … -text` | `"/odd filter=lfs … -text" …` | greedy from the right |
| `/trailing  filter=lfs … -text` (two blanks) | `"/trailing " …` | a pattern ending in a space |
| `*.mp4 filter=lfs … -text` | `None` | parses as written; no blank |
| `/media/** filter=lfs … -text` | `None` | parses as written; intent unknowable |
| `"/my holiday" filter=lfs … -text` | `None` | already this version's output |
| `my holiday filter=lfs … -text` | `None` | neither producer shape |
| `*.psd filter=lfs` | `None` | not keeper's attribute set |
| `# added by hand` | `None` | not a rule |

### `ensure_attributes(root, patterns) -> Result<bool>`

| State | Call | Result |
| --- | --- | --- |
| no file, `["*.mp4"]` | 1st / 2nd | `true`, bare line / `false`, untouched — **unchanged from 46.1** |
| pre-46.1 block, same patterns | 1st | `false`, byte-identical — no churn |
| **no `MANAGED_HEADER` at all**, even holding broken lines | 1st | `false`, **byte-identical**, not written |
| 59 broken copies + a good `*.mp4` line, ask the same pattern | 1st | `true`; one repaired line, `*.mp4` untouched, everything above the marker untouched |
| same | 2nd | `false`, byte-identical |
| broken line above the marker **and** below it | 1st | `true`; only the one below moves |
| duplicates with a hand-added override between them | 1st | `true`; the **last** copy survives, below the override |
| two rules differing only in a non-UTF-8 byte | 1st | `false`, byte-identical — not duplicates |
| CRLF file, one broken line | 1st | `true`; repaired line keeps `\r\n` |
| repair happened, nothing to append | 1st | `true`, written **exactly** as repaired — a file with no final newline does not acquire one |

---

## Edge cases

* **`\` normalisation runs before escaping.** `pattern_for` replaces `\` with
  `/` (the Windows separator rule) and only then escapes. The other order would
  turn every escape it had just inserted into a path component. On Linux this
  means a literal backslash in a filename is still lost at the separator step —
  pre-existing, and out of scope (see "deliberately not done").
* **Two `MANAGED_HEADER` lines.** The first wins; everything below it is
  managed. keeper never writes a second (it appends the header only when
  absent).
* **Header recognised by equality, not `contains`.** The append path uses
  `out.contains(MANAGED_HEADER)` (46.1's behaviour, unchanged); the repair
  requires the line to *be* the header. A line with the header text plus
  trailing prose therefore suppresses a second header but does not open a
  managed block. That is the conservative direction: repair nothing.
* **Line endings.** Each repaired line keeps the ending it had (`\r\n`, `\n`, or
  none for a last line without one). A CRLF file is not converted a line at a
  time.
* **Non-UTF-8 patterns.** `pattern_field` returns **bytes**; see the
  deduplication section for the data-loss shape that forces it.
* **Leading `#` in a raw pattern** is unreachable: condition 3 requires `/` or
  `*.` first, so "a repaired line could become a comment" cannot arise and is
  not defended against.
* **Empty managed block / empty file / no file.** `repair_managed_block` returns
  `None`; nothing is written.
* **`escape_globs` on non-ASCII.** Guarded by `ch.is_ascii()` before the `as u8`
  narrowing, so a multi-byte character is never mistaken for an operator.
* **Allocation.** `escape_globs` returns `Cow::Borrowed` for the overwhelmingly
  common metacharacter-free name; `pattern_field` borrows for a bare field and
  allocates only where `undo` already had to. The dedup key is a linear scan over
  a block of tens of lines rather than a hash set.

---

## Mutation table

**Baseline established GREEN first, in the same scope the sweep uses** —
`cargo test -p keeper-sync --lib lfs::` 130/0 and
`cargo test -p keeper-sync --test lfs_roundtrip` 17/0, run immediately before
the first mutation. A sweep over a red test measures nothing and reports
success; the baseline command and the sweep command are the same two lines, so
they cannot drift apart.

Method: byte-exact backup, one mutation at a time carrying the `MUT47-1`
sentinel, both suites run, restore from the backup, re-verify the hash. Seven
mutations, seven kills, zero survivors.

| # | Mutation | Killed by | Count |
| --- | --- | --- | --- |
| A | `repaired_rule` returns `None` always — repair disabled | 7 unit tests + `the_broken_lines_already_on_disk_are_repaired_and_git_stops_complaining` | **8** |
| B | managed region widened to the whole file — above-header refusal removed | `a_broken_line_above_the_managed_header_is_never_touched` | **1** |
| C | deduplication disabled | `a_duplicate_spelled_differently_still_collapses`, `deduplication_keeps_the_last_copy_…`, `the_broken_lines_the_old_writer_left_are_repaired_and_collapsed`, `…_git_stops_complaining` | **4** |
| C2 | deduplication keeps the FIRST copy instead of the last | `deduplication_keeps_the_last_copy_…`, `a_duplicate_spelled_differently_still_collapses` | **2** |
| D | `escape_globs` returns its input — glob escaping disabled | `glob_metacharacters_in_a_real_name_…`, `the_glob_escape_and_the_quoting_compose_in_that_order`, `gitoxide_reads_the_escaped_pattern_…`, `repair_applies_the_glob_escape_…`, `a_glob_metacharacter_in_a_real_name_is_a_literal_that_git_resolves` | **5** |
| E | condition 4 removed — every keeper-shaped line rewritten | `a_line_below_the_header_that_keeper_did_not_write_is_left_alone` | **1** |
| F | extension branch escapes its own leading `*` | `both_pattern_shapes_are_repaired_and_only_their_literal_half_is_escaped` | **1** |
| G | deduplication key rendered with `from_utf8_lossy` instead of bytes | `two_rules_differing_only_in_a_non_utf8_byte_are_not_one_rule` | **1** |

B, E, F and G are the ones worth naming. Each is killed by **exactly one** test,
which is the point: they are the four ways this change could quietly become a
tool that damages what it does not own, and each has a test whose only job is to
refuse. B and E are the two designs I considered and rejected, pinned so a later
reader cannot re-adopt them by accident. G destroys a user's rule and is
otherwise invisible.

F exists because the `*.` arm of `repaired_rule` was, on first writing, reached
by **no test at all** — every repair fixture used an exact path. The hole was
found by asking which arm a mutation could delete unnoticed, not by reading a
coverage number, and
`both_pattern_shapes_are_repaired_and_only_their_literal_half_is_escaped` was
added for it before F was run. G came the same way, prompted by Story 47.2's
finding: the question "where in *my* code does a rendering stand in for bytes?"
had exactly one answer, and it was a deletion path.

Restoration verified by `sha256sum -c` against the pre-sweep snapshot (both
files OK), by `git diff` on the two files carrying **zero** `MUT47-1` lines, and
by a repo-wide `grep MUT47-1` whose only hits are this paragraph and the one
above it — no source, no test — not by recalling what was changed. `git diff` is
blind to created files; the only file this story creates is this spec, checked
by name. Two mutations (D and G) failed to apply cleanly
on the first attempt — D clobbered a line rather than inserting one, G needed a
type annotation — and each left the crate uncompilable for about a minute. The
automatic restore ran both times before the retry, and the hash confirmed it;
L2Names saw D's window and was told directly rather than left to debug it.

---

## Verification actually run

| Command | Result |
| --- | --- |
| `cargo check -p keeper-sync --lib` | clean |
| `cargo test -p keeper-sync --lib lfs::` | **EXIT=0**, 130 passed / 0 failed |
| `cargo test -p keeper-sync --test lfs_roundtrip` | **EXIT=0**, 17 passed / 0 failed |
| `git check-attr` matrix, git 2.53.0 | as tabulated above |

Fourteen unit tests and two roundtrip tests are new. The roundtrip tests shell
out to the real `git` binary and **assert stderr is empty**, which matters
specifically here: an unparseable gitattributes line is not an error — git exits
0 and only grumbles — so an exit-status assertion alone passes against the bug.
`the_broken_lines_already_on_disk_are_repaired_and_git_stops_complaining`
asserts the complaint is present **before** the repair and absent after.

`gitoxide_reads_the_escaped_pattern_as_the_literal_name` asks
`gix_glob::Pattern` directly with the flags `gix-attributes` uses
(`NO_MATCH_SLASH_LITERAL`, `Case::Sensitive`), which is the path
`routed_through_lfs` takes. A `\[` gitoxide did not understand would not fail a
test anywhere else; it would surface as keeper writing a rule git honours and
keeper cannot see — the same divergence 46.1 pinned one layer up.

### Dependency

**None added.** `gix-quote` was already a direct dependency from 46.1;
`gix::glob` is a re-export of `gix-glob`, already in the tree via `gix`.
`Cargo.toml` and `Cargo.lock` are untouched by this story.

---

## Deliberately NOT done

Items 1–3 are **new deferred work**; they are described rather than numbered,
because only Main writes `deferred-work.md` and the numbering is being assigned
across five layers this wave.

1. **A legacy line whose only defect is a live metacharacter is not repaired.**
   `/report[final] filter=lfs diff=lfs merge=lfs -text` parses exactly as
   written, so condition 4 leaves it, and the new writer appends a correct
   `"/report\\[final]" …` beside it. Measured consequence with both lines
   present: the real file resolves to `lfs` (the new line wins, being later),
   and `reportf` / `reporti` **also** resolve to `lfs` — the stale class still
   over-matches, silently, with empty stderr. That is the honest cost of the
   boundary, and it is the safe direction: over-routing costs a pointer for a
   file that did not need one, while a wrong rewrite of a hand-added glob costs
   the rule. Closing it needs a signal that a `[` was keeper's and not a human's
   — most plausibly a version marker in the managed header, which is a file
   format change and not a bug fix.
2. **A legacy line whose extension holds a backslash is not repaired**
   (`*.a\b filter=lfs …`, from a path `x.a\b`). Same reasoning as 1: it parses,
   and wildmatch quietly reads it as `*.ab`. Rarer than 1; belongs in the same
   entry.
3. **`pattern_for`'s `\` → `/` normalisation is left alone.** On Linux a
   backslash is a legal filename byte and this step destroys it before escaping
   can protect it, so an oversized `a\b` is routed by the pattern `/a/b`.
   Changing it would change which files a pattern covers in existing
   repositories — a behaviour change wearing a bug fix's clothes — and it is the
   Windows separator rule, not a quoting or globbing question. Adjacent to
   Story 47.2's non-UTF-8 path work and probably belongs with it.
4. **Dedup does not merge two rules that mean the same thing in different
   patterns.** Only an identical decoded pattern collapses. Anything else
   requires reasoning about what a pattern matches, which is the guessing
   condition 4 exists to refuse.
5. **Lines above the marker are never touched, for any reason.** Not repaired,
   not deduplicated, not counted as duplicates of anything below.
6. **`Path::extension()`'s last-dot rule is not changed** — same carve-out 46.1
   made, same reason.
7. **`engine.rs:1802`'s whitespace refusal is left in place.** It refuses `/`,
   `\` and whitespace in an extension; it does **not** refuse `*`, `?` or `[`,
   which is why `pattern_for_extension` escapes them itself rather than relying
   on its caller. `engine.rs` is another layer's file this wave and was not
   touched.
8. **No formatter, no linter, no full suite.** Per the wave constraint.

---

## What I could not verify here, and why

`keeper-sync` is `tauri`-free and `keeper-core`-free by design (AD-40), so it
compiles and its tests run in full on Linux. This story touched no shell code,
no frontend and no platform-conditional path. **The `keeper` shell crate was
neither built nor checked — it does not link on Linux** — but nothing here is
reachable from it except through `keeper-sync`'s public API, whose signatures
are unchanged; the only observable difference to a caller is that `pattern_for`
and `pattern_for_extension` may now return a string containing a backslash.
Everything in the matrices above was executed on this machine.

Four residuals:

1. **git on macOS is a different binary.** All of it was measured against git
   2.53.0 on Linux, and the roundtrip tests shell out to whatever `git` is on
   `PATH`. Backslash escaping in wildmatch and C-quoting in gitattributes both
   long predate any version either box will have, so I expect no difference —
   but "I expect" is not "I measured", and the metacharacter rows are new
   behaviour rather than a spelling change.
2. **`cargo fmt --check` and `cargo clippy` have not been run** on the new code.
   Instructed not to. Two spots a linter may have an opinion about:
   `keeper_rule_pattern`'s `and_then(|_| …)` discard, and the `Vec<Vec<u8>>`
   linear-scan dedup, where clippy may suggest a `HashSet`.
3. **The workspace as a whole has not been built.** No manifest changed, so
   nothing else can have moved, but it is unbuilt as a whole.
4. **No real repository with the owner's fifty-nine lines was available.** The
   fixture is reconstructed from the diagnosis — 59 literal copies of the exact
   bytes the pre-46.1 writer emitted — and its brokenness is *asserted against
   real git* before the repair rather than assumed. What that cannot tell me is
   whether the owner's file also holds shapes I have not imagined: a hand-edited
   line inside the managed block, a second marker, mixed line endings. Each of
   those has a unit test and each resolves to "leave it alone", so the failure
   direction is a rule that survives rather than a line that is eaten. Gate
   check 5 is where it is confirmed against the real file.

### Gate checks, in order, on macOS

1. `cargo fmt --check` and
   `cargo clippy -p keeper-sync --all-targets -- -D warnings` — expect clean;
   the only unrun static gate on the new code.
2. `cargo test -p keeper-sync --lib lfs::` — expect EXIT=0, 130 passed.
3. `cargo test -p keeper-sync --test lfs_roundtrip` — expect EXIT=0, 17 passed.
   **This is the macOS-git check.** Two tests assert empty stderr and exact
   resolutions against the real binary; if Apple's git differs at all it fails
   here, and `a_glob_metacharacter_in_a_real_name_is_a_literal_that_git_resolves`
   is the one to read first.
4. `cargo deny check` — expect nothing new to decide; no dependency changed.
5. **Against the owner's real repository, with the app built.** Take a copy of
   the `.gitattributes` holding the 59 lines. Run one sync. Expect: the 59
   collapse to **one** quoted line; everything above the marker byte-identical
   (`git diff` shows deletions below the marker and one changed line, nothing
   above it); and `git status` / `git commit` print **no**
   `is not a valid attribute name`. Run a second sync: `git diff` empty. That
   second run is FR-137, and it is the acceptance the owner will actually see.
6. In the same folder, drop an oversized file whose name contains `[` and has no
   extension (e.g. `report [final]`). Commit twice. Expect exactly one line,
   `"/report \\[final]" filter=lfs diff=lfs merge=lfs -text`, with
   `git check-attr filter -- "report [final]"` reporting `lfs` while
   `git check-attr filter -- reportf` reports `unspecified`.
