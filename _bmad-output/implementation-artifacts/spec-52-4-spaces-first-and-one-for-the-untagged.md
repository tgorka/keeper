# Spec 52.4 — Spaces first, and one for the untagged

story: 52.4
status: in-progress
branch: `work/epic-52-spaces-first` (on top of `work/epic-52-note-mode-writes`)
baseline_revision: c873fa6
final_revision: ''
binds: FR-306, FR-307, FR-308; AD-120, AD-121
sentinel: `MUT52-4`

<intent-contract>

**Three asks, verbatim.** *"umiesc spaces ponad files"*, *"about space nie ma
przycisku dodaj jak inne (jest tylko edytuj about.md)"*, *"potrzebuje tez space
ktore jest untagged (jako ostatnie space jezeli sa untagged notes w folderze
spaces)"*.

**Always**
- SPACES renders above FILES.
- Every space header carries a create control. About's is DISABLED and says why —
  a session has one record, and `spec-51-7` says never a second. A refusal the
  user can see and read beats a control that is silently absent.
- An `Untagged` space exists, sorts LAST, and renders only when it matches
  something. It is an ordinary saved query over the negation the grammar already
  parses (`-tag:`), not a new mechanism beside spaces.
- The `UNFILED` badge list, which was the old answer, is replaced by that space —
  one place where tagless markdown lives, with a count, a fold and a row menu like
  every other space.

**Block if**
- Untagged's create verb is asked for: it is refused for the same reason the
  grammar gives — a negated query names no kind, so there is nothing to write.
  The control is present and disabled with that sentence.

**Never**
- Never seed Untagged into a zone's `_spaces/` as a file the user must maintain:
  it is a default, and a default the user deletes stays deleted (AD-121).
- Never make the section order a setting. He asked for one order.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src/components/sessions/session-detail.tsx` | the SPACES block moves above the FILES block; the `UNFILED` list at `:492-496` is removed in favour of the space |
| `src/components/sessions/session-spaces.tsx:715-716` and the header row | About renders a disabled create control carrying `create_refused`'s sentence |
| `keeper-core/src/sessions/spaces.rs` — `DEFAULT_SESSION_SPACES` | a last-position `Untagged` entry whose query is the tagless negation; `create_refused` answers for it |
| `keeper-core/src/sessions/pool.rs` | `pool.unfiled` becomes the space's own result rather than a separate bucket the UI renders |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | in the rendered detail, the SPACES heading precedes the FILES heading in document order — asserted on order, not on presence |
| 2 | About's create control is present, disabled, and its accessible description is Rust's refusal sentence |
| 3 | a zone with tagless markdown lists an `Untagged` space last, with the right count |
| 4 | a zone with none does not render it at all |
| 5 | Untagged folds, counts, and its rows carry the same row menu as any other space |
| 6 | Untagged's create control is disabled with the negated-query reason |
| 7 | a user who deleted the Untagged space does not get it back on the next scan |
| 8 | the old `UNFILED` badge list is gone and its test is rewritten against the space |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
