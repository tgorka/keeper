---
title: 'Story 43.9: The Slash Menu Can Open'
type: 'bug'
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
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-1-tab-belongs-to-the-editor.md'
---

<intent-contract>

## Intent

**Problem:** the `/` command menu, shipped in Story 37.6, has never opened. Not "opens late", not
"opens with the wrong rows" — it cannot open, for anybody, and it never could.

`slashMenuSource` returns `from: line.from`, which anchors the completion at the `/` itself.
CodeMirror filters a completion's options by fuzzy-matching the document text between `from` and the
caret against each option's label. That pattern therefore always begins with a slash; no command is
called `/Task`; every option is filtered out; and a completion result with zero options is not a
menu, so the popup is never shown. Driven through a real `EditorView`, the state goes `pending` and
then straight back to inactive with an empty option list.

This was found while proving Story 43.1's Tab binding — the first assertion there needed an
open popup to press Tab at, and the slash menu could not supply one.

**Approach:** move `from` past the slash so the match pattern is the word the user is typing, and
make each option's `apply` swallow the slash on the way out so the inserted text is unchanged. One
result field, one offset, and a `validFor` that has to move with them.

## Boundaries & Constraints

**Always:**
- `from` and `validFor` describe the **same span**. `validFor` is what tells CodeMirror an open
  result still covers the next keystroke; pointed at a different span it invalidates on every
  character, and an accept during that window silently refuses.
- What the menu inserts is byte-identical to what it inserted before. This story changes whether the
  menu appears, never what it writes.
- The trigger grammar from Story 37.6 is untouched: a slash at the start of an otherwise-empty line
  with nothing after the caret, and nowhere else.

**Block If:**
- Nothing.

**Never:**
- No new commands, no reordering, no change to `SLASH_COMMANDS`. See "deliberately not done".
- No second trigger position, and no `filter: false` escape hatch — turning the filter off would make
  the menu appear by making it stop narrowing, which is a different feature.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Bare slash | `/` typed on an empty line | menu opens, offering every command in the table | none |
| What the user sees | menu open | the rendered popup contains the row `Task` and its detail `- [ ] …` | none |
| Narrowing | `/tas` | `Task` ranked first, and fewer rows than the full table | none |
| Accepting | `/tas`, accept | document is exactly `- [ ] ` — the slash is gone | none |
| Multi-line insertion | `/tab`, accept | the whole table skeleton, three lines, verbatim | none |
| Not line one | `# Heading\n\n` then `/co` | menu opens, offering `Code fence` | none |
| Still typing | `/` opens, then `tas` typed, accept in the same tick | still offering, accept succeeds, document is `- [ ] ` | none |
| Mid-sentence slash | `see docs/notes` | menu stays shut | none |
| Text after the caret | `/` typed before existing text on the line | menu stays shut | none |

</intent-contract>

## Code Map

- `src/components/notes/editor/slash-menu.ts` — the only file changed. `from: line.from` →
  `from: line.from + 1`; `validFor: OPEN_SLASH` → `validFor: /^\w*$/` (the same span, minus the
  slash); and each option's `apply` now writes from `from - 1` so the slash leaves with the word.
  `OPEN_SLASH` still guards the trigger, which is a different question from the match span, and the
  comment now says which is which.
- `src/components/notes/editor/slash-menu.test.ts` — new. Nine tests, every one of them driving the
  real source through a real `EditorView`.

## Tasks & Acceptance

**Execution:**
- [x] Move the match span past the slash and keep the inserted text identical.
- [x] Keep `validFor` describing the span `from` names.
- [x] Tests: every matrix row, asserted on what the menu offers and what lands in the document.
- [x] Prove each part of the change fails when reverted.

**Acceptance Criteria:**
- The menu opens on `/` and offers named commands, asserted through the real completion source and
  the rendered popup — not through the value of `from`.
- Accepting a row writes exactly the text a user would have typed, with no `/` left behind.
- The Story 37.6 trigger grammar still holds: no menu mid-sentence, none with text after the caret.

**Revert proof.** Each mutation applied to the shipped source, the suite run, the source restored.
9 tests pass unmutated.

| Mutation | Failed | Which |
|---|---|---|
| Full revert (`from` at the slash, `validFor: OPEN_SLASH`, `apply` from `from`) | 7 / 9 | every test except the two that assert the menu stays shut |
| Only `from` moved; `apply` left writing from `from` | 3 | both insertion tests (`/- [ ] `, `/\| Column…`) and the still-typing test |
| `from` and `apply` fixed; `validFor` left as `OPEN_SLASH` | 1 | "keeps offering, and can be accepted, while the user is still typing" |

That last row is the one worth reading. `validFor` looks cosmetic and is not: with it pointed at the
wrong span, typing a second character takes the menu back to `pending` with zero rows, and an accept
in that window returns `false` — the note keeps `/tas` and the user's Enter did nothing. Measured
both ways before the test was written:

| `validFor` | status after the next keystroke | offered | accept | document |
|---|---|---|---|---|
| `/^\w*$/` (shipped) | `active` | `Task`, `Today's date` | `true` | `- [ ] ` |
| `OPEN_SLASH` (mutation) | `pending` | none | `false` | `/tas` |

## Design Notes

**The bug was invisible to every unit-level fact about the module.** The trigger regex was right, the
command table was right, the `apply` closures were right, and the feature did not exist. Nothing in
this suite asserts a position, an offset or a regex — each test types into a real editor and reads
back either the rows on offer or the text in the document, because those are the two things that were
never true and the only two a user can observe.

**Why the slash is swallowed in `apply` rather than kept inside `from`.** Those are two different
spans doing two different jobs: `from` is what the menu *matches on*, and the accept range is what
the menu *replaces*. They were conflated, and matching lost. Reading the slash's position as
`from - 1` keeps them independent without stashing `line.from` in a closure that a later edit could
make stale.

## Deliberately not done

- **Row order.** With an empty pattern CodeMirror sorts the rows itself, so a bare `/` no longer shows
  `Today's date` first the way the table lists it. Ordering could be pinned with per-option `boost`
  values. That is a decision about what the menu should recommend, it was never observable before
  today, and it is not "making it open" — the test asserts the offered rows as a set and says so.
- **The command table itself.** Six commands, unchanged. Whether the right six is a product question.
- **Fuzzy-match reach.** `/tas` also matches `Today's date`, through `T-od-a-y'-s`. That is
  CodeMirror's matcher behaving normally, and `Task` ranks first, which is what the test asserts.
- **Tab as an accept key for this menu.** It works, because Story 43.1 put `acceptCompletion` in front
  of `indentMore` — but 43.1's own test uses the tag source, deliberately, so the two stories can be
  reverted independently.
