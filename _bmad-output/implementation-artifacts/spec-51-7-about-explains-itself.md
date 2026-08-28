# Spec 51.7 — About explains itself, and the board comes to the folder shape

story: 51.7
status: in-progress
branch: `feat/51-7-about-explains-itself` (on 51.6, needs 51.1)
binds: FR-298, FR-299; AD-119, AD-120
sentinel: `MUT51-7`

<intent-contract>

**The ask.** *"about space nie ma dodaj nowy note"* and *"tasks chce miec drag and drom zamiast dropdown
przy zmianie z jednej kolumny na druga"*.

**Both are surfaces the owner cannot reach, not features that are missing.**

**About.** Three independent refusals, all with teeth: the live query has two terms so `creatable_kind`
refuses first; About is refused by name; and `kind_dir` refuses it under either shape — *"a session has
one about record — about.md under the flat contract, README.md under the folder one — and keeper edits
it rather than making a second."* A second record would give `shape()` two answers, and `row_for` reads
the session's identity, title, tags, pinned state and lineage out of exactly one file. **The defect is
that the space says nothing**: `no_home` is only computed when `creatable_kind` returns `Some`, so
About renders neither a button nor a reason.

**Tasks.** Drag-and-drop **already ships** — `task-board.tsx:191-210` puts `draggable` and the three
handlers on every card, and a drop writes `status:` and `order:` through `sessions_task_move`. What the
owner has never seen is the BOARD: `session-detail.tsx:526` renders it only for a flat session, because
*"a folder-shaped one has no pool to tag, so its board would be four empty columns saying nothing
true"*. Story 51.1 makes that false — a folder-shaped session's root markdown is now in the pool — so
the reason for hiding it is gone.

**Approach.** Say why About offers nothing, offer the verb that does apply, and let the board render
wherever there is a pool to tag.

**Always.**
- About's sentence is **Rust's**, projected onto the VM like every other refusal, so the wording lives
  in one place (`KindHasNoHome::OnlyOne`).
- Where a create is refused because the record already exists, the space offers *Open the record* — the
  detail already has that button and its label already names the right file per shape.
- The board renders when the session has a pool that can carry a task tag. Under 51.1 that is both
  shapes; the gate becomes "is there a pool", not "is the shape flat".
- The dropdown stays. It is the keyboard path, and this repo does not ship a pointer-only affordance.

**Block if.**
- A space's create is refused for the two-term reason → that sentence is shown too, not swallowed. The
  owner's About query is `tag:about tag:recordings`, and "this space asks for more than one thing" is
  the honest explanation for why it has no create.
- The session has no pool at all → no board, with the existing sentence.

**Never.**
- Never allow a second record. All three refusals stay; only the explanation is added.
- Never replace the dropdown with drag alone.
- Never re-derive the refusal wording in TypeScript. Epic 50 already deleted one such mirror.

**I/O and edge-case matrix.**

| # | input | expected |
|---|---|---|
| 1 | the About space, folder-shaped session | no create button, and the one-record sentence where it would be |
| 2 | the About space with a two-term query | the two-term sentence, naming that the query asks for more than one thing |
| 3 | About's header | an *Open the record* verb, opening `README.md` or `about.md` per shape |
| 4 | a folder-shaped session with root markdown tagged `task` | the board renders with those cards |
| 5 | dragging a card between columns on a folder-shaped session | `status:` and `order:` written, the surface re-reads |
| 6 | the same by keyboard, through the dropdown | unchanged, still works |
| 7 | a folder-shaped session with no markdown in the pool | no board, and the existing sentence |
| 8 | a flat session | the board is unchanged in every respect |
| 9 | the refusal wording | asserted to come from Rust, not a TS constant |

</intent-contract>

## Code Map

| file | change |
|---|---|
| `keeper/src/sessions_ipc.rs:1604-1606` | the `no_home` projection also answers when `creatable_kind` is `None`, distinguishing "one record only" from "this query asks for more than one thing" |
| `keeper-core/src/sessions/spaces.rs` | `creatable_kind` gains a sibling that reports WHY it refused, so the projection has something to say |
| `src/components/sessions/session-spaces.tsx` | renders the sentence; About's header gains *Open the record* |
| `src/components/sessions/session-detail.tsx:521-527` | the board gate becomes "has a pool", not "is flat"; the sentence for a poolless session stays |
| `docs/sessions.md` | About's explanation, and that the board follows the pool |

## Tasks & Acceptance

- [x] the projection distinguishes the two refusals, rows 1–2, wording from Rust (row 9)
- [x] *Open the record*, row 3
- [x] the board gate follows the pool, rows 4–8
- [x] `docs/sessions.md`

**Acceptance.** The About space explains itself and offers the verb that applies; and the owner's
folder-shaped session has a task board he can drag cards on.

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
