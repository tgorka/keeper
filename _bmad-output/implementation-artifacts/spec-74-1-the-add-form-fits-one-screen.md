---
status: review
baseline_revision: 6e5695e
final_revision: ''
---

# Story 74.1 — The add form fits one screen, and a card's corner is a card's corner

<intent-contract>

## Problem

The owner opened a new task and photographed a form cut mid-sentence. The triage separated two claims that look like one.

The **width** arithmetic is correct and guarded, and cannot produce that clip: at his 1568 px window the detail region is 546 px (156 px drawer + pane basis 600 + strip 280, the surplus split evenly — `tasks-pane.tsx:2942,2947`), the worst legal state is the 360 px floor (`TASKS_DETAIL_MIN_WIDTH_PX`), every row is `flex-wrap` and every control 224 px (`task-form.tsx:450,455`), and the window minimum is asserted against the floors (`window-minimum.test.ts:81-85`).

The **height** is the defect. Nine standing note paragraphs render unconditionally in flow — `TASK_FORM_KIND_NOTE` is ~90 words (`task-form.tsx:226-233`), the description note ~45 (`:124-131`), plus id, mode, enabled, schedule, offer and bounds — and story 72.9 added a copy fieldset with two more. That is 1 100–1 500 px of form in a ~650 px viewport, contained only by a 10 px overlay scrollbar. The card is always cut; the screenshot is just where it happened to be cut.

And the corner: `rounded-xl` computes to `calc(5px × 1.4)` = **7 px**, which is also `--radius-md` (`src/index.css:117,119,185`) — a ~500 px card and its 224 px inputs carry the *identical* corner, so the card reads as neither square nor round. Below the fold, the bottom corners are off-screen and the visible edge is a square cut.

Both real-browser gates epic 72 defined shipped with the words "NOT RUN" (spec-72-3, spec-72-9). This story is the first owner-visible landing of that gap, so it closes the gate as well as the defect.

## Approach

Take the standing prose out of the flow into the hint idiom this surface already uses, and give a container one step more radius than the fields inside it.

## Always

Prose that explains a field lives in an `IconHint` beside its label (AD-240's precedent, `tasks-pane.tsx:194-200`: "the explanation stays reachable without claiming pixels from the identity"). Text that names a refusal the person is hitting right now stays inline.

## Block If

Nothing new is refused.

## Never

Never move a floor constant, never add a second scroll (the region already scrolls, `tasks-pane.tsx:3251`), never change the 224 px control measure or the wrapping rows — the width half is right and is what keeps the form operable at 360 px. Never take the radius to 12 px: DESIGN.md:252's doctrine stands.

</intent-contract>

## Code Map

- `src/components/sync/task-form.tsx` — the nine standing notes and the copy fieldset; the two `type="date"` inputs AD-256 removes (~`:1446-1497`).
- `src/index.css:117,119,185` — the radius scale keyed on `--radius: 5px`; the container step lands here.
- `DESIGN.md:252` — the doctrine paragraph, amended to record the step and why a container must not match its fields.
- `dev/probe/` — the measuring harness; the gate spec-72-3 and spec-72-9 both left unrun.

## Tasks & Acceptance

Acceptance, verbatim from the epic: *a real-browser probe reports the add-form card's `scrollHeight ≤ clientHeight` of its region at 1568 px with a panel open and the drawer expanded, and at 1024 px; `window-minimum.test.ts` and `tasks-pane.test.tsx`'s floor/placement assertions stay green with no constant changed; every note that leaves the flow is still reachable; the card's computed radius differs from its inputs'.*

## Design Notes

Eleven standing paragraphs left the flow — nine became `IconHint` triggers beside their labels (id, description, kind, mode, enabled, schedule + bounds merged into one, schedule offer, on-missed, missed delay, prompt file, replace-existing) and two were deleted with the date inputs. What stayed inline is exactly the class the contract reserves: the `role="alert"` save failure, the edit-mode "the id cannot change" line (read *after* trying), the profile read failure, the schedule preview/refusal, the not-a-number message, and the short transient status lines.

`--radius-xl` was **redefined** 7 px → 10 px (`calc(var(--radius) * 2)`) rather than a new token added, because a new token would have meant editing four `src/components/ui/*` files to say what `rounded-xl` already says. Its whole consumer set in `src/` is the container class AD-255 is about: `card`, `dialog`, `alert-dialog`, `command` (shell + input row). `rounded-2xl`/`3xl` have no consumers; `rounded-4xl`'s only one is an `h-5` badge whose radius its own height clamps at 10 px. `--radius-md` (7 px) is untouched, so every field, chip and small button is unchanged.

## Verification

Measured in a real browser, not estimated: Chrome on the desktop host driven through `dev/probe` over tunnelled CDP, with the working tree flipped between `HEAD` and the change — the gate spec-72-3 and spec-72-9 both shipped marked "NOT RUN".

Detail region = `windowHeight − 68` in every run. Card height, before → after:

| window | kind | before | after |
| --- | --- | --- | --- |
| 1568 × 900 (panel open, form 513 px) | default | 1042 (region 832 — overflowing) | **456** |
| 1568 × 900 | copy | 1357 | **617** |
| 1024 × 900 (form at the 360 px floor, rows wrapped) | default | 1356 | **576** |
| 1024 × 900 | copy | 1745 | **737** |

So `scrollHeight ≤ clientHeight` holds at both widths for every kind on a 900 px-tall window, where before it failed for all four. In-flow paragraphs over 12 words: 9 → **0** (default), 11 → **0** (copy). The card computes `border-radius: 10px`, its inputs `7px`; before, both were `7px`.

- `bunx vitest run src/components/sync/task-form.test.tsx` — 71 passed. Each new rule was hand-mutated to prove it bites (reverting one hint to a paragraph, and re-adding the date keys to the payload, reds five tests between them).
- Full gates at the epic level: `bunx tsc --noEmit` clean, `bunx biome check` clean, `bunx vitest run` — 5912 passed / 353 files.

**What is still owed, measured rather than assumed:** at a 650 px region (a 718 px-tall window) the default form fits at both widths but the **copy** kind does not — 617 fits at 1568 and 737 does not at 1024. The copy fieldset carries four more rows than any other kind and each wrapped row costs ~44 px at 1024. Removing the date pair took 389 px out of it and it is still the tallest kind by 161 px. Recorded as DW-257 rather than fixed here: the remedies are a density pass over the source/destination rows or a collapsed section, and both are decisions this story's Approach does not carry.
