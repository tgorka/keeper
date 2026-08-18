# Spec 53.4 — A space you can narrow in one press

story: 53.4
status: review
branch: `work/epic-53-narrow-a-space` (on top of `work/epic-53-fold-and-merge`)
baseline_revision: 8c8a3eb
final_revision: ''
binds: FR-319; AD-121 (respected, not overridden)
sentinel: `MUT53-4`

<intent-contract>

**The ask, verbatim.** *"the about space require recordings and about tag by
default - change it to only about"*

**The default is already right, and changing it would not reach him.**
`spaces.rs:126` is `query: "tag:about"` and always has been. The two-term query is
in his own `_spaces/about.md`: `spaces::read_one` builds a space entirely from its
file (`:305`) and consults `DEFAULT_SESSION_SPACES` only to validate the `default:`
marker (`:344`); `plan` never re-seeds a zone that has a `_spaces/` directory
(`:452-469`); and *Restore default spaces* skips anything claimed (`:472-487`).
AD-121 — "the directory is the ledger" — is exactly why. So editing the const is a
no-op for him and for every existing zone.

**And the `recordings` tag on his record is a consequence, not a second bug.** No
keeper writer emits it: `template.rs:266-268,296,366`, `migrate.rs:832`,
`sessions_ipc.rs:941` and `file_properties.rs:202-218` all write `tags: [about]`.
`RECORDINGS_TAG` belongs to the notes-vault recording stub
(`recording_note.rs:117,353`). He typed it — the only way to make his own record
appear in an About space that demanded both terms.

**Always**
- A space whose query asks for more than one thing already renders a disabled
  create carrying Rust's `ManyTerms` sentence. That sentence gains a **repair the
  owner presses**: narrow this space to the single term its default asks for.
- The repair is visible, one press, and reversible — it writes through the same
  `sessions_space_save` the editor uses, so it is an ordinary edit with an ordinary
  undo path (edit the space again).
- It works for any over-specified default space, not only About: the term to keep
  comes from the default the space is claiming (`default_key`), never from a
  hard-coded `"about"`.
- Nothing is rewritten without a press. AD-121 stands.

**Block if**
- The space claims no default (`default_key` is `None`): there is no authority for
  what its single term should be, so no repair is offered — the editor is the
  answer there.
- The query is not a plain conjunction of `tag:` terms — a frozen or
  passthrough query is not narrowed by a button.

**Never**
- Never silently rewrite a `_spaces/*.md` on scan, upgrade or restore.
- Never change `DEFAULT_SESSION_SPACES`' About query: it is already `tag:about`,
  and "fixing" it would look like the fix while changing nothing for anyone.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `keeper-core/src/sessions/spaces.rs` — `Refusal::ManyTerms` | carries the term the default asks for, so the surface can offer the repair without composing a query in TypeScript (AD-65) |
| `keeper/src/sessions_ipc.rs` | the repair verb: narrow a space to its default's single term, through the existing save path |
| `src/components/sessions/session-spaces.tsx` | the control beside the ManyTerms sentence |
| `docs/sessions.md` | records that a default's query never reaches a zone that already has the space, and that this is the repair |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | a space claiming `default: about` whose query is `tag:about tag:recordings` offers the repair, and pressing it writes `tag:about` |
| 2 | after the repair the space's create refusal becomes the one-record sentence, not ManyTerms |
| 3 | a space claiming no default offers no repair |
| 4 | a frozen or passthrough query offers no repair |
| 5 | nothing is written without the press — a scan, a restore and an app start all leave the file byte-identical |
| 6 | the repair reads the term from the claimed default, so a two-term `log` space narrows to `tag:log` |
| 7 | the docs say why editing the default alone would not have reached him |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
