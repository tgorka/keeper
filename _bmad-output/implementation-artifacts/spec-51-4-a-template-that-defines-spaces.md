# Spec 51.4 — A template that defines spaces, and placeholders that mean something

story: 51.4
status: in-progress
branch: `feat/51-4-a-template-that-defines-spaces` (on 51.3)
binds: FR-291, FR-292, FR-293; AD-121, AD-111 (a plan is replayable), AD-65
sentinel: `MUT51-4`

<intent-contract>

**The ask.** *"template nie definiuje spaces"* and *"w template daj informacje co moge uzywac w md
plikach jako placeholdery i co znacza"*.

**Problem, part one.** `template.rs` and `pattern.rs` never mention `_spaces` — a template cannot carry
space definitions, and `pattern::resolve` resolves to `_template/<name>`, a SIBLING of `_spaces/`, so a
create could not copy them anyway. AD-121 refused **per-session** spaces (*"per-session means editing
one query N times and reintroduces a folder into a shape whose point is that there are none"*) and that
refusal has teeth — but a template **seeding the zone's** `_spaces/` is not what it refused, and it is
the honest reading of the ask.

**Problem, part two.** keeper substitutes **nothing** into a copied template: the create plan is
`CopyFile` per pattern file (`plan.rs:139-150`). A `{{token}}` engine exists — `notes/templates.rs`,
with `{{date:FMT}}`, `{{time:FMT}}`, `{{title}}`, `{{id}}`, `{{cursor}}` — and it is notes-only;
`sessions_ipc.rs` never calls it. So item 14 is a FEATURE plus its documentation. Writing the docs
alone today would mean writing "there are none".

**Approach.** A template may hold `_spaces/*.md`; a create seeds any of them the zone does not already
have. And a create expands the notes vocabulary in the markdown it copies — one vocabulary, not two.

**Always.**
- **The vocabulary is `notes/templates.rs`'s.** Two vocabularies is the failure mode that module argues
  against, and its unknown-token rule — *an unknown placeholder is left literal byte for byte* — is
  exactly what makes expansion safe in a template full of literal braces.
- The expansion context is **journaled with the plan**, because a plan must be replayable (AD-111) and
  a re-run must not re-resolve the clock. The ids and the date are already journaled this way.
- Seeding is **additive and never destructive**: a template's `_spaces/tasks.md` is written only if the
  zone has no space with that id. A zone's own edited space always wins.
- Expansion applies to **markdown only**, and never to the record, which is already composed from the
  pattern's headings (`plan::skeleton_from`).

**Block if.**
- A template `_spaces/` entry is not a readable space definition → skipped with a sentence, and the
  create still succeeds. A create must not fail because a space file has a typo.
- Expansion would change a file keeper composes itself → refused by construction: those are
  `WriteFile` steps and never `CopyFile`.

**Never.**
- Never per-session spaces. AD-121 stands.
- Never a second placeholder vocabulary, and never a token that means something different here.
- Never silently rewrite an operator's template: `docs/sessions.md` promises *"keeper copies it and
  never edits it unasked"* — the promise is restated to say expansion happens **into the new session**,
  and the template's own bytes are still untouched.

**I/O and edge-case matrix.**

| # | input | expected |
|---|---|---|
| 1 | template with `_spaces/tasks.md`, zone has no Tasks | the zone gains it on create, journaled |
| 2 | the same, zone already has Tasks | the zone's own file is untouched |
| 3 | template `_spaces/` entry that will not parse | skipped with a sentence; the create still succeeds |
| 4 | template file containing `{{title}}` | the created file carries the session's title |
| 5 | `{{date}}` / `{{date:YYYY}}` / `{{time:HHmm}}` | the create's own date and stamp — not a second clock read |
| 6 | `{{id}}` | the session's ULID, the one in the record's frontmatter |
| 7 | `{{unknown}}` | left literal, byte for byte |
| 8 | a template `.png` containing the bytes `{{title}}` | untouched — expansion is markdown-only |
| 9 | the record | composed from headings as today; not expanded twice |
| 10 | replaying the journaled plan | the same bytes; the clock is not re-read |
| 11 | the Templates room | states the placeholder list and what each means |
| 12 | `docs/sessions.md` | the same list as a table, and the restated copy promise |

</intent-contract>

## Code Map

| file | change |
|---|---|
| `keeper-core/src/sessions/pattern.rs` | a template's `_spaces/` entries are enumerated as seed candidates, distinct from the files that travel into the session |
| `keeper-core/src/sessions/plan.rs:139-157` | markdown `CopyFile` becomes read+expand+`WriteFile` for pattern markdown, with the expansion context carried in the plan; every other file still copies verbatim |
| `keeper-core/src/notes/templates.rs` | reused as-is. If a session-side context type is needed, it is a constructor over the existing `TemplateCtx`, never a parallel expander |
| `keeper/src/sessions_ipc.rs` (`sessions_create`) | passes the context it already has (title, date, stamp, ULID, template name) and seeds the zone's spaces through `spaces::compile_seed` |
| `src/components/sessions/session-templates.tsx` | the room's hint gains the placeholder list — this is where a person stands when they open a template file |
| `docs/sessions.md` | the table, and the restated "never edits it unasked" promise |

## Tasks & Acceptance

- [ ] template `_spaces/` seeding, additive, journaled, rows 1–3
- [ ] expansion through the notes vocabulary, markdown only, context journaled, rows 4–10
- [ ] the room states the vocabulary, row 11
- [ ] `docs/sessions.md`, row 12

**Acceptance.** A template can ship the spaces a new zone should have, and a template file containing
`{{title}}` arrives in the new session carrying its title — with an unknown token left exactly as typed.

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
