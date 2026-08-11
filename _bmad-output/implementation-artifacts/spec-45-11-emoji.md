---
title: 'Story 45.11: Emoji'
type: 'feature'
created: '2026-08-10'
status: 'review'
blocking_condition: ''
baseline_revision: 'd64e5f8'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-45-open-it-change-it-put-it-back.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-9-the-slash-menu-can-open.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-44-13-tag-entry-that-completes.md'
---

<intent-contract>

## Intent

**Problem:** typing `:tada:` in a keeper note leaves `:tada:` in the note. Every other place the
owner writes — the issue tracker, the chat client, the commit message — turns it into 🎉, and the
one place that is supposed to be their own writing surface does not.

FR-185 asks for `:shortcode:` completion from the GitHub cheat-sheet vocabulary. AD-92 fixes how:
the mapping is **generated data, checked in, with the generator beside it**, because nobody
maintains 1855 shortcodes by hand and nobody ships a network call to render `:tada:`.

There is a second problem, and it is the one that decides whether this ships working. keeper had
three completion popups before today, and one of them had never opened for anybody. Story 43.9:
`slashMenuSource` returned `from: line.from`, so the match pattern included the leading `/`, no
command is called `/Task`, every option was filtered out, and a result with no options is not a
menu. Eighteen months, invisible to every unit-level fact about the module. A fourth popup written
from scratch is how that happens a second time.

**Approach:**

1. **A generator, not a table.** `src/lib/emoji/generate.ts` fetches the cheat sheet, derives the
   characters, sorts, and writes `src/lib/emoji/table.ts`. Its pure half is tested; its network
   half is the only part with no test, on purpose.
2. **44.13's machinery, not a fourth popup.** The menu is a fourth `CompletionSource` in the note
   editor's existing `autocompletion()` call, shaped exactly like `tag-complete.ts`.
3. **One matching rule, asked with a different separator.** A shortcode is a path whose segments
   are joined with `_` instead of `/`. 44.13's rule moved from `components/tags/tag-match.ts` into
   `lib/segment-match.ts` and both callers ask it.
4. **A closing colon commits.** `:tada:` typed straight through becomes 🎉 and never has to be
   noticed as a menu interaction. An unknown shortcode is left alone by the same code path.

## The trigger decision, stated

**`:` alone does not open the menu. `:` plus at least one shortcode character does, and only where
the colon starts a word.**

Prose is full of colons — `Note:`, `12:30`, `https://`, `title: value` — and a menu that appears at
every one of them is a menu people switch off. Two rules together, both in `emoji-complete.ts`:

- **At least one shortcode character after the colon.** A bare `:` opens nothing unless it was
  explicitly asked for (Ctrl-Space). This is the identical rule `tag-complete.ts` already applies to
  a bare `#`, for the identical reason — the ambiguity is settled by what was typed, never by a
  setting. `:___` counts as bare, because it cuts into no words and has therefore narrowed nothing.
- **The colon must begin a word.** A colon preceded by a word character, a `/` or another `:` is
  punctuation inside something else. Both halves of the feature ask this same question of the same
  document (`startsAWord`), because a menu that opens where the closing colon will not commit is a
  menu offering something it cannot deliver.

Explicit invocation over a bare `:` still lists the vocabulary, capped at 50. That is the escape
hatch, and it is the same escape hatch the `#` popup has.

## Boundaries & Constraints

**Always:**
- Nothing calls the network at runtime. The only `fetch` in this story is in `generate.ts`, behind
  an entry guard, run by hand via `bun run gen:emoji`.
- `from` sits after the trigger character and `apply` reaches back to `from - 1`. Story 43.9.
- One matching rule. If the emoji menu and the tag chooser ever disagree about whether a query
  matches a name, one of them is wrong and there is no third opinion to consult.
- What lands in the note is a Unicode character. A note is a markdown file that has to survive
  without github.com.

**Block If:**
- Nothing.

**Never:**
- No new dependency. `@codemirror/autocomplete` was already here and the table is 45 KB of our own
  generated data.
- No second classifier, no second vocabulary, no second popup.
- The generated table is never hand-edited. It says so on line 1.

## I/O & Edge-Case Matrix

### The menu

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Prefix | `:tad` | menu opens offering `tada`, row reads `🎉 tada` | none |
| Accept | `:tad` then Enter/Tab | document holds `🎉`; no colon survives | none |
| Word in the middle | `:hands` | offers `raised_hands` — `_` is a segment boundary | none |
| Narrowing | `:sm` then `ile` | strictly fewer rows, `smile` among them | none |
| Ranking, offset | `:woman` | `woman_artist` above `bald_woman` | none |
| Ranking, length | `:smi` | `smirk` above `smile_cat` | none |
| Case | `:Sm`, `:TADA` | offered; the matcher folds case and so does the trigger | none |
| Punctuation shortcodes | `:+1`, `:-1` | offered | none |
| Bare colon at a word start | `say :` | **stays shut** | none |
| Bare colon after a word | `Heading:` | **stays shut** | none |
| Only separators | `say :___` | **stays shut** — no words, so nothing narrowed | none |
| Prose | `Note: se` | **stays shut** — colon follows a letter | none |
| Clock | `meet at 12:30` | **stays shut** | none |
| URL | `see https://ex` | **stays shut** | none |
| Key/value | `title:so` | **stays shut** | none |
| Punctuation before | `(:tad`, `**:tad` | opens — `(` and `*` are not word characters | none |
| Unknown | `:zzzznotanemoji` | no menu; the text is the text | none |
| Explicit over bare `:` | Ctrl-Space | 50 rows, the cap | none |

### The closing colon

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Known, typed in full | `:tada:` | `🎉`, caret after it | none |
| Known with punctuation | `:+1:` | `👍` | none |
| Known, wrong case | `:TADA:` | `🎉` | none |
| Unknown | `:zzzznotanemoji:` | `:zzzznotanemoji:` unchanged | none |
| Colon inside a word | `Reference:tada:` | unchanged — the colon did not begin a word | none |
| Empty | `::` | `::` unchanged | none |
| Paste of one colon | `input.paste` of `:` after `:tada` | unchanged — a paste is not typing | none |
| Paste of a line | `input.paste` of `look: :tada:` | unchanged | none |
| Reconcile from Rust | `dispatch` with no user event | unchanged | none |

### The generator

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Vocabulary | cheat-sheet README | every backticked `:name:`, deduped, in listing order | none |
| Not the cheat sheet | a 404 page | `[]`, and `main` refuses below 1000 | throws, writes nothing |
| Character | `.../unicode/1f389.png?v8` | `🎉` | none |
| Multi-codepoint | `.../unicode/1f1ef-1f1f5.png` | `🇯🇵`, both codepoints | none |
| GitHub's own PNG | `.../octocat.png` | absent from the table, named in the header | none |
| Custom PNG named in hex | `.../cafe.png` | absent — the `/unicode/` segment is the rule | none |
| Malformed unicode path | `.../unicode/notahex.png` | absent | none |
| Aliases | `+1` and `thumbsup` | both present, both `👍` | none |
| Fresher than the cheat sheet | an API entry the README lacks | absent — the cheat sheet decides the vocabulary | none |
| Run twice | same inputs | byte-identical output | none |
| Upstream reshuffles sections | inputs in a different order | byte-identical output | none |

</intent-contract>

## Code Map

**New:**
- `src/lib/segment-match.ts` — 44.13's rule, lifted. `segmentsOf(text, separator)` and
  `segmentMatchOffset(query, candidate)`, the second taking pre-split candidates so a fixed
  vocabulary is cut once rather than on every keystroke.
- `src/lib/emoji/generate.ts` — the generator, beside its output as AD-92 requires. Pure:
  `parseCheatSheet`, `parseLiterals`, `buildTable`, `renderTable`. Impure: `main`, behind an entry
  guard. Wired as `bun run gen:emoji`.
- `src/lib/emoji/table.ts` — generated. 1855 shortcodes, sorted, an array of pairs.
- `src/lib/emoji/match.ts` — `matchEmoji(query, limit)` and `emojiFor(shortcode)`, over the table
  cut into segments once at load.
- `src/components/notes/editor/emoji-complete.ts` — `emojiCompleteSource()` and
  `emojiShortcodeCommit()`.
- Tests: `table.test.ts`, `generate.test.ts`, `match.test.ts`, `emoji-complete.test.ts`,
  `emoji-wiring.test.tsx`.

**Changed:**
- `src/components/tags/tag-match.ts` — delegates to `lib/segment-match.ts`, passing `/`. Public API
  and behaviour identical; its own suite and `tag-combobox.test.tsx` are unedited, which is the
  proof.
- `src/components/notes/note-editor.tsx` — one source in the `override` array, one extension after
  it, one dynamic import.
- `package.json` — `gen:emoji`.

**Changed, and not about emoji at all — see "The jsdom rect shim" below:**
- `src/components/notes/editor/indent-keymap.test.ts`,
  `src/components/notes/editor/tag-complete.test.ts`,
  `src/components/notes/new-note-caret.test.tsx` — the hand-rolled
  `Range.getClientRects` shim replaced with `withRangeRects()` in `beforeAll` and its undo in
  `afterAll`. Shim block and imports only; no test body touched.

### Why the table is an array of pairs and not a record

A record whose keys include `100` and `1234` reorders itself: JavaScript enumerates integer-like
keys first, in ascending numeric order, before every other key. The whole point of committing
generated data is that its order is the generator's decision, so the generator emits a shape that
cannot be re-sorted by the language.

### Why the closing colon is a transaction filter

CodeMirror has a `Completion.commitCharacters` field built for exactly this shape of thing, and it
is the wrong tool. It fires on `keydown`, calls `applyCompletion`, and returns `false` — so the
keystroke is not consumed and the browser then types the character. That is correct for a language
server (`.` commits the member and is then typed) and it would leave `🎉:` in the note.

A `transactionFilter` over the insertion replaces the whole `:tada:` and leaves nothing behind. It
is also the only version of this that can be proved: `EditorView.inputHandler` runs from
`applyDOMChange`, which needs real contenteditable input events, and jsdom cannot produce them —
a filter sees exactly the transaction real typing produces, so the tests type the way every other
editor test in this repo types and are testing the shipped path.

The filter declines for everything that is not one person inserting one colon at one caret: more
than one change, a change that is not an insertion, an insertion that is not `:`, a `userEvent`
that is not `input.type`, or no user event at all. The last of those is load-bearing — the note
editor writes remote revisions into the buffer with no user event on them, and a filter that fired
there would rewrite somebody else's note behind their back.

## Tasks & Acceptance

**Execution:**
- [x] Generator fetches, derives, sorts, writes; output committed; `bun run gen:emoji`.
- [x] 44.13's rule lifted rather than restated, with the tag suites unedited as the evidence.
- [x] Completion source with `from` past the colon, `filter: false`, `apply` swallowing the colon.
- [x] Trigger grammar decided, stated, and enforced from both halves through one predicate.
- [x] Closing colon commits a known shortcode and leaves an unknown one alone.
- [x] Wired into the real `NoteEditor`, and proved there rather than in a stack a test assembles.
- [x] Every matrix row asserted; each part of the change proved by reverting it.

**Acceptance Criteria:**
- The committed table parses, is non-trivial (floor 1500, today 1855), holds `+1`, `-1`, `tada`,
  `rocket`, `smile` and a multi-codepoint flag, has no duplicates, is sorted, and contains no
  shortcode that GitHub renders as its own PNG.
- Completion offers matches for a prefix and inserts the emoji, not the shortcode, driven through a
  real `EditorView`.
- A colon in ordinary prose does not open the menu.
- An unknown shortcode is left as text, both as a query and as a completed `:zzzz:`.
- The generator's output is deterministic: two runs produce identical bytes.
- `slash-menu.test.ts` and the tag-combobox tests stay green, unedited.

## Verification

Every command run in `/home/dev/.paseo/worktrees/2va3pp5x/quick-donkey`.

| Command | Result |
|---|---|
| `bun run vitest run` over the story's five suites plus `slash-menu`, `tag-complete`, `indent-keymap`, `tab-wiring`, `tag-match`, `tag-vocabulary-input`, `tag-combobox`, `no-user-agent-gating` | **153 passed, 13 files** |
| `bun run gen:emoji` twice, `cmp` on the output | identical bytes, sha256 `ad8d71e8…aff62` |
| `bunx tsc` over this story's files with the repo's own compiler options | clean |
| `bunx biome check` over this story's files, `table.ts` included | clean, no exemption |
| Everything above re-run after the shim conversions, **judged by exit code** | `exit=0`, 156 passed, 14 files, zero unhandled errors |

The generated table needs no lint exemption: `renderTable` emits biome's own formatting, so a
regeneration is a data diff and never a formatting diff.

`bunx tsc` reports two errors, both in files this story does not touch and both belonging to
other stories in flight in the same worktree (`components/viewers/document-viewer.tsx`,
`lib/viewers/file-asset-url.ts`); filtered to the files above, it is clean.

### The jsdom rect shim, and a conclusion of mine that was wrong

Three test files outside this story were converted, because this story's own wiring test could not
be correct while they were not, and because leaving one copy behind is how it comes back.

I first argued that a file mounting a real `NoteEditor` must install `withRangeRects` and never
hand it back. That was a conclusion about `afterEach` — restoring the prototype between tests,
while a just-unmounted view still has frames pending, does take the run's exit code — and I had
never tested `afterAll`. W2Marks and W2Table both measured `afterAll` on NoteEditor-mounting files
and it holds. Both of this story's editor suites now use the paired shape, verified over four
consecutive runs **by exit code rather than by the summary line**, which is the only instrument
that sees this failure: a throw escaping CodeMirror's measure pass takes the exit code while the
summary still prints passes.

The surviving hand-rolled copies were worth converting rather than noting, because that shim
returned an **empty** `DOMRectList`: a measure pass that runs reads `rects[0]` as undefined and
throws anyway. It is a permanent latent fault, not a flaky one — what is load-dependent is only
whether an animation frame elapses during a test at all, which is why a busy box turned it into an
occasional red.

**A second thing I wrote here was wrong, and it is corrected by measurement.** I claimed the shim's
`if (!Range.prototype.getClientRects)` guard made it order-dependent, and that the wave's cleanup
turned an occasional hazard into a certain one by making the property reliably absent. W2Table
challenged the premise from the installed vitest defaults and offered to measure it; that was
worth doing rather than swapping one derived claim for another, so I ran a two-file probe in
`~/.W2Emoji/probe` — file A installs a marker on `Range.prototype` at module scope and never hands
it back, file B reports what it sees at its own module scope, `fileParallelism: false` to pin the
order. B sees `absent`, both with this repo's settings and under `--no-isolate`. The probe was then
inverted to assert `INHERITED` and confirmed to FAIL, so it is sensitive rather than vacuous.

So: vitest isolates per file, `Range.prototype` is clean at the start of every test file, that
condition was always true, and the order-dependence never existed — before the cleanup or after.
The remedy is unchanged and the empty rect list was always the whole defect; only the reason I gave
for it was wrong. W2Marks ran the same probe independently and got the same answer.

The six survivors were found by re-running an absence check a different way.
`grep "if (!Range.prototype.getClientRects)" src/` had been reported clean; `grep "Range.prototype" src/`
returned six live copies. The harness grep takes a Rust regex, so the parens in the first query are
a capture group and it can never match text containing a literal `(` — it returns a clean no-match
shaped exactly like success. **An absence asserted with an unescaped-metacharacter regex is not an
absence**, and it fails silently in the reassuring direction.

## Revert proof

27 mutations, each applied to the shipped source by a unique-anchor string replace, the suites run,
the source restored from a pristine copy and the checksum re-verified. Every anchor was then
re-grepped **by name** across `src/` — `if (false`, `?? "?"`, `"tada", "X"`,
`SHORTCODE_SEPARATOR = "/"`, `TAG_SEPARATOR = "_"`, `from: opened.from,`,
`displayLabel: hit.shortcode,` — all absent. Baseline green before and after, at the verdict's
scope. Harness in `~/.W2Emoji/`.

Three mutations were run, found to be uncaught, and the **test** was fixed rather than the count
massaged; the retries are listed with what the original test could not see.

| # | Mutation | Failed | Which |
|---|---|---|---|
| M1 | `from: opened.from + 1` → `opened.from` (the 43.9 defect) | 1 | inserts the emoji, not the shortcode |
| M2 | `apply` writes from `from` instead of `from - 1` | 1 | same |
| M3 | bare-colon guard disabled | 2 | stays shut for a bare colon at a word start; only underscores |
| M4 | `startsAWord` always true | 2 | stays shut after a key; commit does not fire mid-word |
| M6 | commit filter never fires | 4 | every closing-colon commit |
| M7 | commit filter ignores `userEvent` | 2 | leaves a paste alone; leaves a reconcile alone |
| M8 | commit filter accepts multi-character inserts | 6 | every commit case plus the mid-word refusal |
| M9 | unknown shortcode commits to a placeholder | 1 | leaves an unknown shortcode as text |
| M10 | `displayLabel` drops the character | 1 | shows a row a person can recognise |
| M11 | shortcode segments split on `/` | 4 | three matcher tests **and** the editor's mid-word match |
| M12 | ranking drops the match-offset term | 1 | ranks a first-word match above a later-word one |
| M13 | result uncapped | 2 | the cap; the wordless-query contract |
| M14 | exact lookup stops folding case | 1 | folds case, so the menu cannot offer what the colon refuses |
| M15 | ranking drops the length term | 1 | ranks a short name above a longer one |
| M16 | segment rule matches anywhere, not at a boundary | 3 | `tag-match` **and** emoji **and** the editor |
| M17 | segment rule stops requiring earlier segments to match outright | 2 | `tag-match` **and** emoji |
| M18 | `TAG_SEPARATOR` `/` → `_` | **12** | `tag-match` (6), `tag-combobox` (4), `tag-complete` (2) |
| M19 | generator takes only the first codepoint | 1 | joins a multi-codepoint sequence |
| M20 | generator keeps GitHub's own PNGs | 1 | leaves out a custom PNG whose name reads as hex |
| M21 | generator trusts a non-hex codepoint | 14 | every generator test |
| M22 | generator stops sorting | 2 | sorts; identical when upstream reshuffles |
| M24 | editor never registers the completion source | 1 | offers the menu (wiring) |
| M25 | editor never registers the commit filter | 1 | turns a shortcode into its character (wiring) |
| M26 | committed row `["tada", "🎉"]` → `["tada", "X"]` | 10 | table, matcher, completion and wiring |
| M27 | committed table cut to five rows | 24 | almost everything |

### The three that had to be fixed

**M3 survived its first run.** The only bare-colon test typed `Heading` then `:` — which the
*other* rule refuses, because the colon follows a letter. With the bare-colon guard deleted, that
test still passed and a 50-row menu would have opened in the middle of prose. Two tests were added
that put the colon at a word start (`say :` and `say :___`), and both fail without the guard. This
is the story's own version of 43.9: a rule that looked tested and was not.

**M12 and M15 survived their first runs.** The ranking assertion used the query `hands`, where the
length term alone reproduces the offset term's order, and `smi`, where the offset term alone
reproduces the length term's. Each was replaced with a pair the other two comparisons cannot
separate — `woman_artist` vs `bald_woman` for offset, `smirk` vs `smile_cat` for length — measured
against the real table before the assertion was written, not guessed.

### The two equivalent mutants, and how that was established

**M5, `if (matches.length === 0)` → `if (false)`.** Returning a `CompletionResult` with zero
options and returning `null` are indistinguishable to CodeMirror: neither shows a popup, and
`completionStatus` never reaches `active` for either. Equivalent by construction; the branch stays
because it says out loud that an empty vocabulary answer is a decision.

**M23, the parse anchor `` `:name:` `` → bare `:name:`.** Measured rather than assumed:
`grep -o ':[a-z0-9_+-]\+:' README.md | grep -v backtick` over the fetched cheat sheet returns
nothing — every colon-wrapped token on that page is inside a table cell whose rendered column
carries the same names as its backticked one, so the two anchors produce the same set in the same
order. The backticked anchor stays because it is the correct one: the other column exists to be
turned into a picture by GitHub, and the day upstream adds prose the bare anchor starts collecting
sentences.

## Design Notes

**Why the rule moved instead of being reused as-is.** `matchTags` would have worked on shortcodes —
they contain no `/`, so it degenerates to a case-folded prefix match. It would also have meant
`hands` finding nothing, because `raised_hands`'s second word would be unreachable, and a
vocabulary of underscore-joined names is close to useless under first-word-only matching. The
alternative to lifting was transliterating `_` to `/` before calling `matchTags`, which works and is
a trick; `lib/segment-match.ts` is the boring version. M11, M16, M17 and M18 all fail in both the
tag suites and the emoji suites from a single edit, which is what "one rule" means operationally.

**Why candidates arrive pre-split.** `matchTags` re-splits every candidate on every keystroke —
free across a vault's few dozen tags, and ~3700 avoidable allocations per character typed across
1855 shortcodes, all producing the identical answer over a table that cannot change while the app
is running. `matchEmoji` cuts the vocabulary once at module load and cuts only the query per
keystroke. `matchTags` itself was left alone; its vocabulary is small and changes as notes are
edited, so precomputing there would buy nothing and cost a cache to invalidate.

**Why the result is capped at 50.** An explicitly-invoked bare `:` matches all 1855 rows, and
CodeMirror renders at most a hundred: the rest are `Completion` objects with closures, allocated on
every keystroke to be discarded unseen. 50 is well above what fits on a screen, so no query a person
types is truncated in a way they could notice.

**The cheat sheet cannot supply the characters, and that is why there are two URLs.** GitHub renders
`:tada:` server-side, so `README.md` contains the *name* and never the character; the page says so
itself, naming the GitHub Emoji API as its own upstream. So the README decides WHICH shortcodes
(the vocabulary the epic names, and the conservative side to err on for characters that end up
inside other people's files) and the API decides what each one IS. The 59 shortcodes the API has and
the README does not are deliberately absent: the cheat sheet is the vocabulary.

**`:heart:` inserts U+2764 without a variation selector**, because that is the codepoint GitHub's
own asset path names (`2764.png`) and it is the derivation the cheat sheet itself performs. In a
font that defaults U+2764 to text presentation this renders as a monochrome ❤ rather than a red one.
Adding U+FE0F would require the Unicode `Emoji_Presentation` property — a third source — and would
make keeper's table disagree with the source it names. Recorded rather than guessed at; see "what I
could not verify here".

## Deliberately not done

- **A picker, a palette, or a recents list.** FR-185 is completion in the editor. A grid of 1855
  emoji is a different surface and a different story.
- **Emoji in the text-file editor (45.6) or anywhere outside a note.** `:shortcode:` is a markdown
  convention; silently rewriting a colon inside a `.py` or a `.yaml` that a user opened to edit
  would be keeper corrupting a file. The extension is registered by `note-editor.tsx` only.
- **Skin-tone modifiers.** The cheat sheet lists the base shortcodes GitHub lists, and GitHub's
  `:wave:` has no `:wave::skin-tone-3:` form in that vocabulary. Adding a tone picker means
  inventing shortcodes that are not in the named source.
- **Rendering `:shortcode:` that is already in an existing note.** This story is about what happens
  as you type. A live-preview decoration that displayed old shortcodes as characters is 45.10's
  surface, is owned by another story this wave, and would be a *second* place that decides what
  `:tada:` means.
- **A variation-selector pass over the table** (see above).
- **Pinning the exact shortcode count.** The floor is 1500 against today's 1855, so a regeneration
  after a Unicode release is a data diff and not a failing test.
- **A test suite for `lib/segment-match.ts` of its own.** Its behaviour is asserted from both sides
  — `tag-match.test.ts` and `match.test.ts` — and M16/M17 fail in both from one edit. A third suite
  over the same rule would be a third statement of it, which is the thing this file exists to
  prevent.
- **Touching `matchTags`' own performance** (see Design Notes).

## What I could not verify here, and why

- **How `:heart:` actually looks.** keeper's shell crate does not build on Linux and this box has no
  macOS WebView, so every assertion in this story is jsdom's. Whether U+2764 without U+FE0F renders
  as a red emoji or a monochrome text heart is a Core Text question about the shipped font stack,
  and it needs a human looking at the app on hesperia. If it renders monochrome and that is
  unacceptable, the fix is a variation-selector pass in the generator and a regeneration — no code
  outside `generate.ts` changes.
- **That the emoji chunk is not in the startup bundle.** The reasoning is structural —
  `note-editor.tsx` reaches `emoji-complete.ts` through a dynamic `import()`, so the table lands in
  the editor's chunk graph and not the entry — but I did not run `vite build` and read the chunk
  manifest, because a production build is a gate command and this worktree has six agents editing
  it. Worth one look at the manifest when the gate runs.
- **Real keyboard input.** jsdom cannot produce contenteditable input events, so the tests dispatch
  the transaction real typing produces (`userEvent: "input.type"`, one per character) rather than
  pressing keys. That is exactly why the commit is a `transactionFilter` and not an `inputHandler`:
  the shipped path and the tested path are the same path. What remains untested is CodeMirror's own
  DOM-to-transaction step, which is not this story's code.
- **The generator against a changed upstream.** `parseCheatSheet` is tested against a verbatim slice
  of today's README and against a 404 page. If GitHub restructures the page into something that
  still contains backticked shortcodes but means something different by them, the floor check in
  `main` is the only thing standing there, and it only catches a *shortage*.
- **`bun run lint`, `bun run typecheck`, `cargo` anything, or the full suite.** Not run — the gate
  owns them. The narrow equivalents over this story's files are in Verification above.
