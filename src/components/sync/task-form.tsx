/**
 * The one task form (Epic 58, Story 58.1, FR-347, AD-C7).
 *
 * `sync_task_save` has been a registered command, a typed request and a mocked
 * shell call for a whole wave, and nothing in the app ever called it: the Tasks
 * pane could list, inspect and run a task, and the only way to create, change or
 * delete one was `keeper-syncd tasks set` in a terminal. This is the control
 * that closes that, and it is **one component in two modes** — `editing` is
 * `task !== undefined` — revealed inline by the Tasks pane's header for an add
 * and by a row's own disclosure for an edit. That is AD-C7's shape and the
 * reason for it is not tidiness: two forms would be two chances to word or
 * validate the same task differently, and the one that is wrong is the one
 * nobody is looking at.
 *
 * **What this form deliberately does not do.** Every item below is already in
 * Rust, already messaged in Rust's own words, and a second copy here would
 * drift — always in the same direction, toward accepting what the store refuses
 * and then reporting a save that did not happen:
 *
 * - **The id's shape.** Empty, padded, too long: `tasks::validate_id` decides,
 *   and the id is sent **untrimmed** precisely so its refusal can quote what was
 *   typed.
 * - **The schedule grammar.** Five-field cron, the `@` aliases and
 *   `every <n><unit>` are `TaskSchedule::parse`'s to read, with its own floor of
 *   one minute and its own ceiling. There is no regex here, and no coercion:
 *   FR-347 says a schedule that does not parse is refused, never rounded.
 *   Story 59.7 makes the dialect *offerable* without making it checkable here —
 *   a menu of starting points that get typed into the box, and a preview of the
 *   next instants that **Rust** computed. Neither is a validator: nothing is
 *   trimmed, no save is prevented, and a refusal shown before a save is the same
 *   sentence the save itself would show, off the same `Display`.
 * - **The scheduled-with-no-schedule refusal.** `upsert_task` catches the task
 *   that would report itself enabled and never run; this form lets the
 *   combination be expressed and shows the answer.
 * - **Whether a kind, a mode or a missed-window policy is readable.**
 *   `TaskKind::from_stored`, `TaskMode::from_stored` and
 *   `TaskMissedPolicy::from_stored` decide, and `db::decode_task` partitions on
 *   all three, so an unreadable spelling never reaches an edit form in the first
 *   place.
 * - **Whether the row this form seeded from is still current.** `upsert_task`
 *   carries an `updated_ms` compare-and-set (Story 58.4), so a save whose
 *   baseline has moved is refused by the store and the refusal is rendered here.
 *   The alternative — noticing the prop changed and offering to re-seed — is a
 *   smaller version of the same idea that still loses the race.
 * - **Minting an id.** A blank id means keeper mints the ULID (`sync_ipc.rs`),
 *   which is why an add form sends `""` rather than inventing one.
 * - **The engine-owned columns.** `nextDueMs` and both lease columns have no key
 *   on the `TaskSaveReq` wire type at all: the store clears the window whenever
 *   the schedule, the mode or the enabled flag moves, so anything this form sent
 *   would be discarded and the row is re-read instead.
 * - **The host sentence.** Which host will actually run a task is composed by
 *   `keeper_core::tasks::task_host` per row. This form never predicts it; the
 *   pane re-reads the listing after a save and the row states it.
 *
 * The one thing the backend does **not** refuse is a `profileId` that names
 * nothing: it stores it happily, the row comes back `unhosted`, and the run
 * fails with `no such folder`. So the folder is *picked* from `syncProfiles()`
 * and never typed — that picker is the only defence there is.
 */
import { type FormEvent, useEffect, useId, useState } from "react";
import {
  TASK_SCHEDULE_BOUNDS_NOTE,
  TASK_SCHEDULE_OFFERS,
  type TaskScheduleOffer,
} from "@/components/sync/schedule-offers";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import type { SyncProfileVm, TaskSchedulePreviewVm, TaskVm } from "@/lib/ipc/client";
import { syncProfiles, syncTaskSave, syncTaskSchedulePreview } from "@/lib/ipc/client";
import { syncErrorMessage, TASK_KINDS, TASK_MISSED_POLICIES, TASK_MODES } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/** The add form's title, and the label of every control that reveals it. */
export const TASK_FORM_ADD_TITLE = "Add a task";
/**
 * The edit form's title. The accessible name appends the task's id — not
 * because two edit forms can be open at once (the Tasks pane holds one
 * `editingId` slot, so exactly one can) but because the pane's Add form and a
 * row's Edit form can be, and two `<form>`s on one screen must not answer to
 * the same name. The id is also what tells a screen-reader user which task's
 * form they have just entered.
 */
export const TASK_FORM_EDIT_TITLE = "Edit task";

export const TASK_FORM_ID_LABEL = "Id";
/**
 * What a blank id means: keeper mints one, and this form does not.
 *
 * It also has to say what a *taken* id means, because `upsert_task` is an
 * upsert with no create-only mode: a memorable id somebody types twice replaces
 * the task that already has it — kind, mode, schedule, folder and enabled flag
 * — and Rust refuses only rows this build cannot read. A control labelled *Add
 * a task* has to admit that in the one place a person is choosing the id.
 */
export const TASK_FORM_ID_ADD_NOTE =
  "Optional. Leave it blank and keeper mints one, and whatever you do type is sent exactly as typed. An id a task already has replaces that task rather than adding another.";
/**
 * Why the id cannot be changed on an edit form.
 *
 * Not a UI preference: `task_runs.task_id` joins on it, and `upsert_task` keys
 * on it. Typing a new id here would not rename the task — it would create a
 * second one and leave this one's history attached to nothing.
 */
export const TASK_FORM_ID_EDIT_NOTE =
  "The id cannot change: the run history is joined to it, so a new id here would create a second task and orphan everything this one has recorded.";

export const TASK_FORM_DESCRIPTION_LABEL = "Description";
/**
 * What the description is *for*, which is a fact about the id rather than about
 * this box (Story 59.5).
 *
 * Worth stating because neither half is guessable from the control. An add form
 * sends `""` to have Rust mint a ULID ({@link TASK_FORM_ID_ADD_NOTE}), and an
 * edit form cannot change the id at all ({@link TASK_FORM_ID_EDIT_NOTE}) because
 * `task_runs.task_id` joins on it — so between those two rules the id is either
 * a ULID nobody chose or a word chosen once and frozen. This box is therefore
 * the only name of a task anybody can ever revise, and a person who does not
 * know that will keep looking for an editable name where there is not one.
 *
 * One note across both modes, unlike the id's pair, because the sentence is true
 * in both and for the same reason.
 *
 * The last clause is the form's standing rule rather than this field's:
 * everything free-text here goes to Rust exactly as typed. It is repeated on the
 * one field where a reader might otherwise expect tidying, since a description
 * is the only box in this form whose leading space could look like a mistake to
 * correct.
 */
export const TASK_FORM_DESCRIPTION_NOTE =
  "Optional, and the only name of this task you can ever change: an id is minted by keeper when you leave it blank, and it can never be edited afterwards because the run history is joined to it. Leave this empty to store none. Whatever you type is sent exactly as typed.";

export const TASK_FORM_KIND_LABEL = "Kind";
/**
 * The three kinds, in the CLI's own words (Story 59.11).
 *
 * The option text is the stored spelling itself, by the AD-C7 rule stated at
 * the control below: the row's badge renders `task.kind` verbatim and
 * `tasks list --json` prints this vocabulary, so a prettified label would be a
 * second word for one stored value. That leaves nowhere in the menu to say what
 * the three words mean, so the sentence a person actually needs goes here —
 * the {@link TASK_FORM_MODE_NOTE} idiom one control down, one sentence per
 * value, each leading with the spelling it is about.
 *
 * The three sentences are `TaskKindArg`'s own per-value `--help` lines
 * (`keeper-syncd/src/commands.rs:876-884`), which exist for this exact problem
 * and say so: *"a bare `[possible values: …]` list cannot say which of the
 * three a nightly schedule should be pointed at"*. `docs/sync.md`'s §14 kind
 * table corroborates them. Taken rather than invented, so the CLI's help and
 * this menu agree by provenance instead of by coincidence — the drift a second
 * author would introduce here is the same drift AD-C7 forbids in the spellings.
 */
export const TASK_FORM_KIND_NOTE =
  "sync runs one sync pass over the folder, or over every enabled folder. release runs one release sweep, with every refusal dehydrate has. verify re-checks stored content against its recorded digests: it reads only, asks no network, and takes no per-folder reservation.";

export const TASK_FORM_MODE_LABEL = "Mode";
/** The three modes, accurate to `tasks::decide` and no longer than that. */
export const TASK_FORM_MODE_NOTE =
  "off refuses even an explicit run. manual runs only when asked, and remembers a schedule without obeying it. scheduled runs on its schedule, on whichever host's tick sees it due first.";

export const TASK_FORM_ENABLED_LABEL = "Enabled";
/**
 * Why this is not the same control as the mode (AD-135).
 *
 * `decide` reads `!state.enabled || state.mode != Scheduled`: they are two
 * questions, and a form that collapsed them could not express "scheduled, and
 * switched off for now" — the state someone reaches by pausing a task they
 * intend to resume.
 */
export const TASK_FORM_ENABLED_NOTE =
  "Two questions, not one: a task can be scheduled and switched off, or enabled and only ever run when asked.";

export const TASK_FORM_PROFILE_LABEL = "Folder";
/**
 * The `null` profile, worded exactly as the Tasks pane's rows word it.
 *
 * Defined here rather than in the pane, and the pane imports it from this file:
 * the form is the lower-level module of the two — the pane imports
 * {@link TaskForm} — so holding the constant here keeps the dependency in one
 * direction. The other arrangement is an import cycle between a pane and the
 * form it mounts, which resolves differently under Vite and under Vitest.
 */
export const TASK_HOST_WIDE_TEXT = "the whole machine";
/** What a failed folder read is prefixed with, beside the picker it explains. */
export const TASK_FORM_PROFILE_READ_FAILED_PREFIX = "Could not read the folder list: ";
/**
 * What the picker says before its own read has landed.
 *
 * The Tasks pane's rule, applied one level down: *before the first read has
 * landed the list is unknown, not empty.* Without this the picker held exactly
 * one option for the length of the read and was indistinguishable from a
 * machine that syncs no folders — so somebody who opened it in that window read
 * "there is nothing to scope this to" and saved a host-wide task they did not
 * mean.
 */
export const TASK_FORM_PROFILE_READING_NOTE = "Reading the folder list…";
/**
 * The option a stored `profileId` gets when the picker's list does not contain
 * it. See {@link TaskForm} for why it must exist at all.
 */
export function taskFormUnlistedProfileText(profileId: string): string {
  return `${profileId} — names no folder keeper syncs`;
}

export const TASK_FORM_SCHEDULE_LABEL = "Schedule";
/**
 * The accepted dialect, named without being re-implemented.
 *
 * Naming the shapes is help; checking them here would be a second parser. When
 * an expression does not read, `TaskSchedule::parse` refuses it in its own words
 * and quotes it — and that sentence is the one the person needs.
 */
export const TASK_FORM_SCHEDULE_NOTE =
  "Five-field cron, or @hourly / @daily / @weekly, or every <n><unit> such as every 90m. Leave it empty to store no schedule. keeper refuses an expression it cannot read, and quotes what you typed.";
/**
 * What the box refuses at either end, mirrored from Rust's two constants
 * (Story 59.7).
 *
 * Re-exported through this module so every sentence this form renders is
 * reachable from one place, which is how the mirror guard finds them.
 */
export { TASK_SCHEDULE_BOUNDS_NOTE };

export const TASK_FORM_SCHEDULE_OFFER_LABEL = "Or choose a form";
/**
 * The menu's resting option, and the only one it ever shows as selected.
 *
 * Phrased as an instruction rather than as a value (*"Choose one…"*, never
 * *"custom"* or the current expression) because the control's own state is
 * meaningless: see {@link TASK_FORM_SCHEDULE_OFFER_NOTE}.
 */
export const TASK_FORM_SCHEDULE_OFFER_PLACEHOLDER = "Choose one to fill the box…";
/**
 * What the menu of offered forms does, and what it deliberately does not do.
 *
 * The menu **fills the box** and then forgets it happened — it is an action, not
 * a second view of the value. That is why it always shows its own placeholder
 * rather than the current expression: a `<select>` that appeared to mirror the
 * box would be claiming to know which of its options the typed text is, and for
 * anything hand-written the honest answer is *none of them*. The box remains the
 * single place the schedule lives, and anything may be typed there — the menu is
 * a starting point, not a vocabulary.
 */
export const TASK_FORM_SCHEDULE_OFFER_NOTE =
  "Choosing one types it into the box above, replacing what is there; edit it afterwards or ignore the menu entirely and write your own. Nothing is checked here — the box's contents go to keeper exactly as typed.";

/**
 * One option's text: the expression first, then what it means.
 *
 * The expression leads deliberately. The point of the menu is that somebody
 * stops needing it — a person who picks *every day at 03:00* three times has
 * read `0 3 * * *` three times, and the dialect is small enough that this is how
 * it gets learned.
 */
export function taskFormScheduleOfferText(offer: TaskScheduleOffer): string {
  return `${offer.expression} — ${offer.says}`;
}

/** Where the preview is rendered, refusal or instants alike. */
export const TASK_FORM_SCHEDULE_PREVIEW_TESTID = "task-form-schedule-preview";

/**
 * What precedes keeper's own refusal while somebody is still typing.
 *
 * The refusal itself is Rust's, unaltered — the same sentence the save shows,
 * because it comes off the same `Display`. This prefix exists because the two
 * arrive at different moments and mean different things about the world: after a
 * save it is a report that nothing was stored, and here it is a warning that
 * nothing *would* be. Nothing is prevented in the meantime: the save button is
 * live, the box is untouched, and pressing Save sends what is there and shows
 * what keeper says about it.
 */
export const TASK_FORM_SCHEDULE_REFUSAL_PREFIX = "keeper would refuse this: ";

/**
 * The next instants this schedule fires at, as a person reads them
 * (Story 59.7, FR-368).
 *
 * Every number in here was computed by `TaskSchedule::next_due_after` — the same
 * function the engine's tick walks — and this composes them into a sentence
 * without doing any arithmetic of its own. There is no cron parsing in
 * TypeScript anywhere in this codebase, and this is the function that would have
 * been the excuse for some: a preview computed here would have needed the
 * dialect, the calendar, vixie's day rule and the zone, and the first time any
 * of the four drifted the form would have promised a time the engine had no
 * intention of keeping.
 *
 * The count is never named. However many instants arrived are the ones shown —
 * Rust's search window is finite, so a short list is possible, and a sentence
 * saying *the next three* over a list of two would be the sort of small lie this
 * whole story is about. `toLocaleString` for each instant, which is this repo's
 * existing way to stamp an absolute one (`recording-row.tsx`).
 */
export function taskFormScheduleFiresNote(instants: readonly number[]): string {
  return `Next: ${instants.map((instant) => new Date(instant).toLocaleString()).join(", ")}`;
}

export const TASK_FORM_ON_MISSED_LABEL = "If a window is missed";
/**
 * The grace and the delay in minutes, mirroring `TASK_MISSED_GRACE_MS` and
 * `TASK_MISSED_DELAY_MS` (`src-tauri/crates/keeper-sync/src/tasks.rs`).
 *
 * Named rather than written into the sentence, because the sentence they
 * compose was **already wrong**. Story 58.4 wrote *"delay serves it no sooner
 * than fifteen minutes after it fell due"*; the review then rejected that
 * anchor, and the fix moved it to the instant a host **noticed** the window
 * using a separate, longer constant — updating the Rust constant and
 * `tasks::decide`'s doc but neither this string nor the CLI's `--help`. A wrong
 * number therefore survived a review pass and a full gate.
 *
 * The coupling to Rust is mechanical rather than remembered: this file's test
 * reads both constants out of that Rust source and asserts these two numbers
 * and the sentences built from them, so changing either constant fails a test
 * here rather than shipping a form that promises the old behaviour.
 */
export const TASK_MISSED_GRACE_MINUTES = 15;
/** See {@link TASK_MISSED_GRACE_MINUTES} — the two are coupled to Rust together. */
export const TASK_MISSED_DELAY_MINUTES = 30;

/**
 * The three settings, accurate to `tasks::decide` and no longer than that,
 * composed around whatever delay **this task** will actually wait (Story 59.6).
 *
 * Both numbers are named because a person choosing between the settings is
 * choosing about them, and the delay's **anchor** is stated because the number
 * alone is not the behaviour: thirty minutes measured from the window rather
 * than from the noticing would make `delay` and `run_now` the same option for
 * any host that was away longer than half an hour. Nothing here re-implements
 * the rule — this text is read by a human, and `decide` is what decides.
 *
 * A function rather than a constant since 59.6, and for the same defect class
 * the constants above exist for, met from the other side: once a task may carry
 * its own delay, a sentence with `30` baked into it is wrong for every task that
 * chose otherwise. So the number comes from the effective value, and
 * {@link TASK_FORM_ON_MISSED_NOTE} is this function at the default — which is
 * what the mirror test keeps pinned to Rust.
 *
 * The grace is **not** a parameter: it is one boundary for the whole policy and
 * no task may move it, which is why a delay below it is refused rather than
 * offered.
 */
export function taskFormOnMissedNote(delayMinutes: number): string {
  return `A window that fell due while nothing was hosting this task. All three settings serve a window normally while a host is here, and differ only about one nobody was here to serve — which keeper concludes after ${TASK_MISSED_GRACE_MINUTES} minutes. run_now serves it on the first tick that sees it — once, however many windows went by, which is what an ordinary restart already does. delay runs it ${delayMinutes} minutes after a host noticed it: the anchor is the noticing and not the window, so a host back two hours late genuinely waits. skip abandons a window nobody served within those ${TASK_MISSED_GRACE_MINUTES} minutes and arms the next one instead.`;
}

/**
 * The note as a task that has chosen no delay of its own reads it.
 *
 * The default composition, kept as a named export because it is what the 58.9
 * mirror guard asserts against Rust's source text — the DEFAULT is the thing
 * that must not drift from `TASK_MISSED_DELAY_MS`, and a per-task value cannot
 * be checked against a constant that is not its authority.
 */
export const TASK_FORM_ON_MISSED_NOTE = taskFormOnMissedNote(TASK_MISSED_DELAY_MINUTES);

export const TASK_FORM_MISSED_DELAY_LABEL = "Delay by (minutes)";
/**
 * What an empty delay box means, and what the floor is.
 *
 * The floor is stated because it is refused rather than clamped, and a person
 * who is told the bound before they type is a person who does not have to read a
 * refusal. Its *reason* is stated for the same argument the note above makes
 * about the anchor: `15` alone is a number somebody would reasonably try to
 * argue with, and the grace period being the interval that concludes nobody was
 * home is what makes it not arbitrary.
 */
export const TASK_FORM_MISSED_DELAY_NOTE = `Leave it empty to use keeper's own ${TASK_MISSED_DELAY_MINUTES} minutes, which is what every task did before this box existed. At least ${TASK_MISSED_GRACE_MINUTES} minutes: that is how long a window must sit open before keeper concludes nobody was home, and a delay shorter than it would be over before the window it holds back counted as missed.`;

/**
 * What the form says when the delay box holds something that is not a number of
 * minutes.
 *
 * The **one** refusal this form owns, and it is not a copy of a Rust rule: the
 * wire type is `number | null`, so "this box does not contain a number" has no
 * third state to be sent as. Reading it as `null` would silently discard what
 * was typed and store *use keeper's default* instead, which is the failure this
 * whole story is about — a setting that reports one thing and does another. The
 * bounds themselves are still Rust's, and this sentence deliberately does not
 * mention them.
 */
export const TASK_FORM_MISSED_DELAY_NOT_A_NUMBER =
  "The delay must be a whole number of minutes, or empty to use keeper's own.";

/**
 * The delay box's contents as the wire wants them: milliseconds, `null` for an
 * empty box, or `undefined` when it is not a number at all.
 *
 * Exported because it is the whole of the form's unit conversion and the thing
 * worth asserting directly — a test that drove it only through a rendered box
 * could not distinguish `null` from `undefined`, and those two are the
 * difference between *use the default* and *tell them it is not a number*.
 */
export function taskFormMissedDelayMs(minutes: string): number | null | undefined {
  // `=== ""` and not `.trim() === ""`, which is the rule the schedule field
  // already states and its reason: a box holding only spaces is not an empty box,
  // and reading it as absence would store *use keeper's default* where somebody
  // typed something. `Number(" ")` is 0, so spaces reach the write door as a
  // delay it refuses and quotes — which is the answer, rather than a silent one.
  if (minutes === "") {
    return null;
  }
  // `Number` and not `parseInt`: `parseInt("12abc")` is 12, which would store a
  // delay somebody did not type. A trailing `.5` is refused for the same reason
  // rather than rounded — the box asks for minutes.
  const parsed = Number(minutes);
  if (!Number.isInteger(parsed)) {
    return undefined;
  }
  return parsed * 60_000;
}

export const TASK_FORM_ADD_SUBMIT_LABEL = "Add task";
export const TASK_FORM_EDIT_SUBMIT_LABEL = "Save task";
export const TASK_FORM_CANCEL_LABEL = "Cancel";

/** Where a refusal is rendered — the Tasks pane's idiom for one. */
export const TASK_FORM_ERROR_TESTID = "task-form-error";

/**
 * One row of this form: a label and the control it names (Story 59.13).
 *
 * `flex-wrap` and the control's own `shrink-0` are the whole of it, and together
 * they decide WHAT gives when the row is too narrow. It used to be the control,
 * because a flex item's default `flex-shrink: 1` makes the sized box the elastic
 * one: measured in a 1024px window, this form's `w-56` controls were **22px**
 * wide and the form was unfillable. Now the row wraps — the label takes a line
 * of its own and the control keeps the 224px it was designed at — which is the
 * only arrangement in which a narrow form is merely taller rather than broken.
 *
 * A constant rather than ten copies of the string, so the ten rows cannot
 * disagree about it and one guard test can name the shape.
 */
export const TASK_FORM_ROW_CLASS = "flex flex-wrap items-center justify-between gap-2";

/** The width every control in this form is designed at, as a class so the row
 *  above and `TASKS_DETAIL_MIN_WIDTH_PX` are talking about the same number:
 *  `w-56` is 224px, and 224 is what the region's floor is built from. */
export const TASK_FORM_CONTROL_CLASS = "w-56 shrink-0";

/** Matches the two native `<select>`s in `session-space-editor.tsx`. */
const SELECT_CLASS =
  "h-9 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50";

/**
 * What the nine controls hold. `id`, `description`, `schedule` and
 * `missedDelayMinutes` are strings all the way: each has an empty-string state
 * the wire type spells `null`, and keeping them strings here is what lets the
 * box hold exactly what was typed until the moment {@link TaskForm}'s submit
 * converts it.
 */
type TaskFormValues = {
  id: string;
  kind: string;
  mode: string;
  enabled: boolean;
  /** `""` is the sentinel for `profileId: null` — see {@link TaskForm}. */
  profileId: string;
  schedule: string;
  /** `""` means "store no description" — converted on the way out, not here. */
  description: string;
  onMissed: string;
  /**
   * Minutes, as typed, `""` meaning *use keeper's own delay*.
   *
   * Minutes rather than the wire's milliseconds because minutes are what every
   * sentence about this setting is written in, and the conversion is one
   * multiplication in the submit — the same shape `schedule`'s `"" → null`
   * conversion has. Held as a string, so a half-typed `"1"` on the way to
   * `"120"` is not silently a two-minute delay the form then refuses.
   */
  missedDelayMinutes: string;
};

/**
 * Create or change one task.
 *
 * @param task - The stored row to edit, or `undefined` to add. Its presence is
 *   what puts the form in edit mode, and the values below are seeded from it
 *   **once**: the pane hands this component a fresh object on every listing
 *   read, and re-syncing from the prop would overwrite what has been typed since
 *   the form opened. That is `AddFolderForm`'s rule and its reason, verbatim.
 * @param onSaved - Called with the row as the next read reports it, and only
 *   when the save actually succeeded. A rejection calls nothing and keeps every
 *   typed value, because the typed value is what a retry is driven from.
 * @param onCancel - Rendered as a Cancel button. Both surfaces reveal this form
 *   behind a disclosure, so both pass it.
 * @param onSavingChange - Whether a save is in flight. The form disables its
 *   own Submit and Cancel while one is, but the disclosure button that revealed
 *   it lives outside and cannot see that: pressing it mid-save unmounted the
 *   form, so the rejection had nowhere to land and the person was left with a
 *   collapsed disclosure and no message — which reads as a save that happened.
 *   The same flag is what keeps a Forget from deleting the row a save is on its
 *   way to re-inserting: `upsert_task` inserts when the id is absent, so a
 *   confirmed deletion followed by a settling save resurrects the task.
 */
export function TaskForm({
  task,
  className,
  onSaved,
  onCancel,
  onSavingChange,
}: {
  task?: TaskVm;
  className?: string;
  onSaved?: (task: TaskVm) => void;
  onCancel?: () => void;
  onSavingChange?: (saving: boolean) => void;
}) {
  const editing = task !== undefined;
  // Seeded once, deliberately (see `task` above).
  const [form, setForm] = useState<TaskFormValues>(() =>
    task === undefined
      ? {
          id: "",
          kind: "sync",
          mode: "scheduled",
          enabled: true,
          profileId: "",
          schedule: "",
          description: "",
          // The store's own default, spelled here so a created task means what
          // a task created before this control existed meant.
          onMissed: "run_now",
          // Empty: a new task uses keeper's own delay until somebody says
          // otherwise, which is the same compatibility argument the policy above
          // makes.
          missedDelayMinutes: "",
        }
      : {
          id: task.id,
          kind: task.kind,
          mode: task.mode,
          enabled: task.enabled,
          profileId: task.profileId ?? "",
          schedule: task.schedule ?? "",
          // `null` and `""` both seed an empty box, which is the one place the
          // two stop being different facts — and they stop being different
          // precisely because a box cannot show the difference. Sending it back
          // unchanged then re-stores `null`, so an edit that touches nothing else
          // quietly normalizes a blank a person once typed into the absence it
          // already looked like.
          description: task.description ?? "",
          onMissed: task.onMissed,
          // `null` seeds an empty box, which reads correctly: no delay of its
          // own. A stored value is shown in minutes because minutes are the unit
          // every sentence about this setting is written in, and the division is
          // exact for anything either writer can store — the floor is fifteen
          // whole minutes and neither door accepts a sub-minute value. A value
          // from a newer keeper that is not a whole number of minutes is shown
          // rounded, and saving would then store the rounded number: stated
          // rather than hidden, and the alternative — a box that refuses to show
          // what the row holds — is worse than a minute of drift on a value this
          // build could not have written.
          missedDelayMinutes:
            task.missedDelayMs === null ? "" : String(Math.round(task.missedDelayMs / 60_000)),
        },
  );
  /**
   * The reading this form is editing, or `null` on an add form.
   *
   * Seeded once from the same prop and for the same reason the values are —
   * which is exactly what makes it a *baseline*. `upsert_task` refuses a write
   * whose baseline has moved, so this is what turns "the row changed under you"
   * from a silent revert into the sentence rendered below.
   */
  const [baselineUpdatedMs] = useState<number | null>(() => task?.updatedMs ?? null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** `null` until the folder read lands — an unknown list, not an empty one. */
  const [profiles, setProfiles] = useState<SyncProfileVm[] | null>(null);
  const [profilesError, setProfilesError] = useState<string | null>(null);
  /**
   * Rust's answer about the expression currently in the schedule box, or `null`
   * when there is nothing to ask about (Story 59.7).
   *
   * `null` covers an empty box and a read that failed, and both mean the same
   * thing on screen: no preview. A failed read is deliberately silent — the
   * preview is help, and a form that grew a second error paragraph because a
   * read for a *hint* did not land would be worse than one that quietly does not
   * hint.
   */
  const [schedulePreview, setSchedulePreview] = useState<TaskSchedulePreviewVm | null>(null);
  const fieldId = useId();
  const title = editing ? `${TASK_FORM_EDIT_TITLE}: ${task.id}` : TASK_FORM_ADD_TITLE;

  /**
   * Read the folders the picker offers.
   *
   * Non-fatal by construction: a failed read leaves the form perfectly usable —
   * "the whole machine" and, on an edit form, the stored id are both still
   * offered — and says what went wrong beside the control it emptied. A picker
   * that silently offered one option would be the scope change this form exists
   * to prevent.
   */
  useEffect(() => {
    let abandoned = false;
    void (async () => {
      try {
        const listed = await syncProfiles();
        if (abandoned) {
          return;
        }
        setProfiles(listed);
      } catch (raw) {
        if (abandoned) {
          return;
        }
        setProfiles([]);
        setProfilesError(syncErrorMessage(raw));
      }
    })();
    return () => {
      abandoned = true;
    };
  }, []);

  /**
   * Ask Rust when the typed expression would actually fire (Story 59.7,
   * FR-368).
   *
   * One read per change of the box, and the *only* implementation of the dialect
   * answers it. Writing this in TypeScript would have meant a second parser, a
   * second calendar, a second copy of vixie's day rule and a second opinion
   * about the zone; the first time any of the four drifted, this form would have
   * promised an instant the engine had no intention of keeping — and a preview
   * that can disagree with the engine is worse than no preview.
   *
   * An empty box asks nothing, because an empty box is not a bad expression: it
   * means *store no schedule*, which is what the note beside it promises and
   * what `submit` sends as `null`. The verb itself does not special-case `""` —
   * it refuses it, exactly as the write door does — so the decision not to ask
   * lives here, at the one place that knows an empty box is a choice rather than
   * a mistake. Whitespace is **not** empty and is asked about, which keeps this
   * consistent with the `=== ""` rule `submit` uses for the same field.
   *
   * `abandoned` and the echo check are two guards against one hazard, and both
   * are wanted. Replies can land out of order, so a slow answer about `0 3 * * `
   * can arrive after the answer about `0 3 * * *`: the cleanup stops a superseded
   * read from ever calling `setSchedulePreview`, and the echo check below stops a
   * preview being *rendered* against text the box no longer holds even if some
   * later refactor loses the cleanup. A stale preview is precisely the class of
   * defect this story exists to avoid, so it is guarded twice on purpose.
   */
  useEffect(() => {
    if (form.schedule === "") {
      setSchedulePreview(null);
      return;
    }
    let abandoned = false;
    void (async () => {
      try {
        const previewed = await syncTaskSchedulePreview(form.schedule);
        if (!abandoned) {
          setSchedulePreview(previewed);
        }
      } catch {
        // Silent, and the reason is above: this is a hint, not a value.
        if (!abandoned) {
          setSchedulePreview(null);
        }
      }
    })();
    return () => {
      abandoned = true;
    };
  }, [form.schedule]);

  // Reported on every change and on unmount, so the surface that revealed this
  // form always knows whether a write is in flight — see `onSavingChange`.
  useEffect(() => {
    onSavingChange?.(saving);
    return () => onSavingChange?.(false);
  }, [saving, onSavingChange]);

  /**
   * The option the *stored* folder needs when the picker's own list does not
   * offer it, and what that option says.
   *
   * **A `<select>` whose value matches no option renders the FIRST one**, and
   * the first one here is "the whole machine" — so a stored folder the list does
   * not contain would make this control *report* a scope the task does not have.
   * A Save would then not even rescope it: React's fallback selects the first
   * option by mutating the DOM and fires no `change`, so `form.profileId` keeps
   * the stored id and the write silently preserves the old scope. Misinformed
   * about the scope, and unable to change it — the trap and its wording are
   * `template-select.tsx`'s.
   *
   * Keyed off `task.profileId` and not off `form.profileId`, which is finding 7
   * of this story's review: selecting "the whole machine" to compare removed the
   * option in the same render, and the gone folder's id was then unrecoverable —
   * nothing can re-enter it, because the whole design is that a folder is picked
   * and never typed. The only exit was Cancel, which discards every other edit
   * in the form too. It is also not gated on the read having landed, because the
   * whole time the read is in flight is a whole time the stored value is
   * unoffered.
   *
   * What the option *says* comes off the row rather than the read:
   * `TaskVm.profile` is the name Rust resolved, and it is `null` exactly when
   * the id names no current profile — the very fact `task_host` turns into
   * **Unhosted**. So a folder that is gone is named as gone from the first
   * frame, and one that is merely not listed yet is called by its own name
   * instead of being accused of not existing for as long as a read takes.
   */
  const listedProfiles = profiles ?? [];
  const storedProfileId = task?.profileId ?? "";
  const unlistedProfile =
    storedProfileId !== "" && !listedProfiles.some((listed) => listed.id === storedProfileId);
  const unlistedProfileText = task?.profile ?? taskFormUnlistedProfileText(storedProfileId);

  /**
   * The preview, but only while it is still about what the box holds.
   *
   * The echo half of the staleness guard described on the effect above. A
   * preview whose `expression` is not the current text is dropped rather than
   * shown, so the worst this control can do while somebody types quickly is show
   * nothing for a moment — never a time or a refusal belonging to a string that
   * is no longer on screen.
   */
  const shownPreview =
    schedulePreview !== null && schedulePreview.expression === form.schedule
      ? schedulePreview
      : null;
  /**
   * Whether the delay box is on screen.
   *
   * `delay` is the only setting that reads the number, so the control belongs to
   * that setting — a box beside a `<select>` reading `skip` invites a value with
   * no effect. But it is **also** shown whenever the box holds anything, and
   * that is not a hedge: the store keeps a stored delay across a policy change
   * (switching to `skip` and back must not throw away what somebody typed), and
   * the write door refuses an incoherent number whatever the policy is. So a
   * hidden non-empty box could refuse a save with its cause off screen. Nothing
   * this form renders may be the reason for a refusal a person cannot see.
   */
  const showMissedDelay = form.onMissed === "delay" || form.missedDelayMinutes !== "";
  /**
   * The number of minutes the note should claim, which is the whole point of
   * 59.6 reaching this file.
   *
   * The box's value when it holds one, keeper's default when it is empty — the
   * TypeScript mirror of `tasks::effective_missed_delay_ms`, and the reason the
   * note is composed rather than written. A box mid-typing or holding nonsense
   * falls back to the default rather than blanking the sentence: the note is
   * about the setting, and {@link TASK_FORM_MISSED_DELAY_NOT_A_NUMBER} is about
   * the box.
   */
  const typedDelayMs = taskFormMissedDelayMs(form.missedDelayMinutes);
  const effectiveDelayMinutes =
    typedDelayMs === null || typedDelayMs === undefined
      ? TASK_MISSED_DELAY_MINUTES
      : typedDelayMs / 60_000;

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    // Converted before the save is announced, because a box that holds no number
    // has nothing to send and `null` would mean something else entirely — see
    // {@link TASK_FORM_MISSED_DELAY_NOT_A_NUMBER}. `setSaving` has not run yet,
    // so the form is never left disabled by this exit.
    const missedDelayMs = taskFormMissedDelayMs(form.missedDelayMinutes);
    if (missedDelayMs === undefined) {
      setError(TASK_FORM_MISSED_DELAY_NOT_A_NUMBER);
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const saved = await syncTaskSave({
        // Untrimmed, on purpose: `tasks::validate_id` refuses a padded id and
        // quotes it, and tidying it here would turn a refusal the person can act
        // on into a task stored under an id they did not type.
        id: form.id,
        kind: form.kind,
        mode: form.mode,
        enabled: form.enabled,
        profileId: form.profileId === "" ? null : form.profileId,
        // The only normalisation this form performs, and it is not tidying: an
        // empty box means "store nothing", and the wire type spells the absent
        // value `null` — the empty string is a different thing. Both fields below
        // take it, for the same reason and with the same `=== ""` test.
        //
        // `=== ""` and not `.trim() === ""`, which is finding 8 of this story's
        // review: a box holding only spaces is not empty, and coercing it to
        // `null` stored a task with no schedule at all where
        // `TaskSchedule::parse` would have refused it and quoted what was typed.
        // That is the pre-validation this file's header disclaims, and it
        // contradicted the id two lines above, which is deliberately sent
        // untrimmed for exactly the same reason. Every non-empty spelling goes
        // verbatim, whitespace and all.
        schedule: form.schedule === "" ? null : form.schedule,
        // Same test, and here nothing downstream will ever refuse it: a
        // description has no grammar. So this is the one field where sending
        // spaces verbatim has no refusal to justify it — it is justified by the
        // note beside the box, which promises exactly that.
        description: form.description === "" ? null : form.description,
        onMissed: form.onMissed,
        // In milliseconds, which is what the row and every instant on it are in;
        // the box speaks minutes because every sentence about this setting does.
        // Sent whatever the policy is, because the store keeps it across a policy
        // change rather than forgetting what somebody typed.
        missedDelayMs,
        // The reading this form started from, so a save whose row has moved
        // elsewhere is refused rather than reverting it. `null` on an add form:
        // there is no reading to be stale.
        baselineUpdatedMs,
      });
      onSaved?.(saved);
    } catch (raw) {
      setError(syncErrorMessage(raw));
    } finally {
      setSaving(false);
    }
  };

  return (
    <form
      aria-label={title}
      className={cn("flex flex-col gap-2", className)}
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      <div className={TASK_FORM_ROW_CLASS}>
        <Label htmlFor={`${fieldId}-id`}>{TASK_FORM_ID_LABEL}</Label>
        <Input
          id={`${fieldId}-id`}
          className={TASK_FORM_CONTROL_CLASS}
          value={form.id}
          readOnly={editing}
          disabled={saving}
          placeholder={editing ? undefined : "nightly"}
          onChange={(event) => setForm((live) => ({ ...live, id: event.target.value }))}
        />
      </div>
      <p className="text-muted-foreground text-xs">
        {editing ? TASK_FORM_ID_EDIT_NOTE : TASK_FORM_ID_ADD_NOTE}
      </p>

      {/* Directly under the id, because it is the same question answered the
          other way: the box above is a key that is frozen or minted, and this one
          is the name a person actually reads. An `Input` and not a `Textarea` —
          this is a name rather than a note, the row that will draw it has one
          line for it, and a box that invites paragraphs would be promising a
          surface that does not exist. */}
      <div className={TASK_FORM_ROW_CLASS}>
        <Label htmlFor={`${fieldId}-description`}>{TASK_FORM_DESCRIPTION_LABEL}</Label>
        <Input
          id={`${fieldId}-description`}
          className={TASK_FORM_CONTROL_CLASS}
          value={form.description}
          disabled={saving}
          placeholder="nightly backup of the photos"
          onChange={(event) => setForm((live) => ({ ...live, description: event.target.value }))}
        />
      </div>
      <p className="text-muted-foreground text-xs">{TASK_FORM_DESCRIPTION_NOTE}</p>

      {/* The option text is the stored spelling itself: the row's badge already
          shows `task.kind` verbatim, and two words for one stored value is
          exactly the drift AD-C7 forbids. */}
      <div className={TASK_FORM_ROW_CLASS}>
        <Label htmlFor={`${fieldId}-kind`}>{TASK_FORM_KIND_LABEL}</Label>
        <select
          id={`${fieldId}-kind`}
          className={cn(SELECT_CLASS, TASK_FORM_CONTROL_CLASS)}
          value={form.kind}
          disabled={saving}
          onChange={(event) => setForm((live) => ({ ...live, kind: event.target.value }))}
        >
          {TASK_KINDS.map((kind) => (
            <option key={kind} value={kind}>
              {kind}
            </option>
          ))}
        </select>
      </div>
      <p className="text-muted-foreground text-xs">{TASK_FORM_KIND_NOTE}</p>

      <div className={TASK_FORM_ROW_CLASS}>
        <Label htmlFor={`${fieldId}-mode`}>{TASK_FORM_MODE_LABEL}</Label>
        <select
          id={`${fieldId}-mode`}
          className={cn(SELECT_CLASS, TASK_FORM_CONTROL_CLASS)}
          value={form.mode}
          disabled={saving}
          onChange={(event) => setForm((live) => ({ ...live, mode: event.target.value }))}
        >
          {TASK_MODES.map((mode) => (
            <option key={mode} value={mode}>
              {mode}
            </option>
          ))}
        </select>
      </div>
      <p className="text-muted-foreground text-xs">{TASK_FORM_MODE_NOTE}</p>

      <div className={TASK_FORM_ROW_CLASS}>
        <Label htmlFor={`${fieldId}-enabled`}>{TASK_FORM_ENABLED_LABEL}</Label>
        <Switch
          id={`${fieldId}-enabled`}
          // `shrink-0` and not {@link TASK_FORM_CONTROL_CLASS}: a switch is not
          // a 224px box and never was, but it is a flex item in a wrapping row
          // and its default `flex-shrink: 1` would squash a 32px toggle the same
          // way the row used to squash the 224px fields.
          className="shrink-0"
          checked={form.enabled}
          disabled={saving}
          onCheckedChange={(checked) => setForm((live) => ({ ...live, enabled: checked }))}
        />
      </div>
      <p className="text-muted-foreground text-xs">{TASK_FORM_ENABLED_NOTE}</p>

      {/* A native `<select>`, not the Radix one, and the reason is recorded in
          `session-file-actions.tsx`: Radix's `Select` throws on an empty-string
          value by design, and "the whole machine" IS the empty-string sentinel
          for `profileId: null`. A `"__wide__"` sentinel translated back to `null`
          on the way out would be the same thing wearing a disguise. All three
          menus here are native so the form is one idiom rather than two. */}
      <div className={TASK_FORM_ROW_CLASS}>
        <Label htmlFor={`${fieldId}-profile`}>{TASK_FORM_PROFILE_LABEL}</Label>
        <select
          id={`${fieldId}-profile`}
          className={cn(SELECT_CLASS, TASK_FORM_CONTROL_CLASS)}
          value={form.profileId}
          disabled={saving}
          onChange={(event) => setForm((live) => ({ ...live, profileId: event.target.value }))}
        >
          <option value="">{TASK_HOST_WIDE_TEXT}</option>
          {listedProfiles.map((profile) => (
            <option key={profile.id} value={profile.id}>
              {profile.name}
            </option>
          ))}
          {unlistedProfile && <option value={storedProfileId}>{unlistedProfileText}</option>}
        </select>
      </div>
      {/* Before the read lands the list is unknown, not empty — the Tasks pane's
          own rule, and without saying so the picker was indistinguishable from a
          machine that syncs no folders. */}
      {profiles === null && (
        <p className="text-muted-foreground text-xs">{TASK_FORM_PROFILE_READING_NOTE}</p>
      )}
      {profilesError !== null && (
        <p className="text-destructive text-xs">
          {TASK_FORM_PROFILE_READ_FAILED_PREFIX}
          {profilesError}
        </p>
      )}

      <div className={TASK_FORM_ROW_CLASS}>
        <Label htmlFor={`${fieldId}-schedule`}>{TASK_FORM_SCHEDULE_LABEL}</Label>
        <Input
          id={`${fieldId}-schedule`}
          className={TASK_FORM_CONTROL_CLASS}
          value={form.schedule}
          disabled={saving}
          placeholder="0 3 * * *"
          onChange={(event) => setForm((live) => ({ ...live, schedule: event.target.value }))}
        />
      </div>
      <p className="text-muted-foreground text-xs">{TASK_FORM_SCHEDULE_NOTE}</p>

      {/* Directly under the box, because it is the answer to the question the
          box asks. Both spellings are `text-muted-foreground` and neither is a
          `role="alert"`: this is help about text somebody is still writing, and
          the one paragraph in this form that reports a *failure* is the refusal
          at the bottom, which arrives from a save that actually happened. A
          refusal here is louder than nothing and quieter than an error, which is
          exactly its standing.

          Rendered only when there is something to say. A parsed expression with
          no instants is possible in principle — Rust's search window is finite —
          and it renders nothing rather than a sentence about the search window,
          because copy for a state nobody can reach is copy nobody can check. */}
      {shownPreview !== null &&
        (shownPreview.refusal !== null ? (
          <p
            className="text-muted-foreground text-xs"
            data-testid={TASK_FORM_SCHEDULE_PREVIEW_TESTID}
          >
            {TASK_FORM_SCHEDULE_REFUSAL_PREFIX}
            {shownPreview.refusal}
          </p>
        ) : (
          shownPreview.instants.length > 0 && (
            <p
              className="text-muted-foreground text-xs"
              data-testid={TASK_FORM_SCHEDULE_PREVIEW_TESTID}
            >
              {taskFormScheduleFiresNote(shownPreview.instants)}
            </p>
          )
        ))}
      <p className="text-muted-foreground text-xs">{TASK_SCHEDULE_BOUNDS_NOTE}</p>

      {/* An action, not a value — see {@link TASK_FORM_SCHEDULE_OFFER_NOTE}. The
          `value` is pinned to `""` so the control always reads as its own
          placeholder rather than pretending to mirror the box, and native for the
          reason the other four menus are. */}
      <div className={TASK_FORM_ROW_CLASS}>
        <Label htmlFor={`${fieldId}-schedule-offer`}>{TASK_FORM_SCHEDULE_OFFER_LABEL}</Label>
        <select
          id={`${fieldId}-schedule-offer`}
          className={cn(SELECT_CLASS, TASK_FORM_CONTROL_CLASS)}
          value=""
          disabled={saving}
          onChange={(event) => {
            const chosen = event.target.value;
            if (chosen === "") {
              return;
            }
            setForm((live) => ({ ...live, schedule: chosen }));
          }}
        >
          <option value="">{TASK_FORM_SCHEDULE_OFFER_PLACEHOLDER}</option>
          {TASK_SCHEDULE_OFFERS.map((offer) => (
            <option key={offer.expression} value={offer.expression}>
              {taskFormScheduleOfferText(offer)}
            </option>
          ))}
        </select>
      </div>
      <p className="text-muted-foreground text-xs">{TASK_FORM_SCHEDULE_OFFER_NOTE}</p>

      {/* Beside the schedule because it is a question about the schedule, and
          native for the reason the other three menus are. The option text is
          the stored spelling itself — the same rule the kind menu states: two
          words for one stored value is the drift AD-C7 forbids, and this is the
          vocabulary `tasks list --json` prints. */}
      <div className={TASK_FORM_ROW_CLASS}>
        <Label htmlFor={`${fieldId}-on-missed`}>{TASK_FORM_ON_MISSED_LABEL}</Label>
        <select
          id={`${fieldId}-on-missed`}
          className={cn(SELECT_CLASS, TASK_FORM_CONTROL_CLASS)}
          value={form.onMissed}
          disabled={saving}
          onChange={(event) => setForm((live) => ({ ...live, onMissed: event.target.value }))}
        >
          {TASK_MISSED_POLICIES.map((policy) => (
            <option key={policy} value={policy}>
              {policy}
            </option>
          ))}
        </select>
      </div>
      {/* Composed, not written: the sentence has to describe the wait THIS task
          will actually do, and a literal `30` in it would be false for every
          task that chose otherwise — the same defect as the wrong number this
          note shipped with, arrived at from the other side. An unparseable box
          falls back to the default's number rather than saying nothing, because
          the note explains the setting and the refusal below explains the box. */}
      <p className="text-muted-foreground text-xs">{taskFormOnMissedNote(effectiveDelayMinutes)}</p>

      {showMissedDelay && (
        <>
          <div className={TASK_FORM_ROW_CLASS}>
            <Label htmlFor={`${fieldId}-missed-delay`}>{TASK_FORM_MISSED_DELAY_LABEL}</Label>
            <Input
              id={`${fieldId}-missed-delay`}
              className={TASK_FORM_CONTROL_CLASS}
              value={form.missedDelayMinutes}
              disabled={saving}
              inputMode="numeric"
              placeholder={String(TASK_MISSED_DELAY_MINUTES)}
              onChange={(event) =>
                setForm((live) => ({ ...live, missedDelayMinutes: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{TASK_FORM_MISSED_DELAY_NOTE}</p>
        </>
      )}

      {/* Rust's sentence, corrected in no way, in the form that asked for it. */}
      {error !== null && (
        <p role="alert" data-testid={TASK_FORM_ERROR_TESTID} className="text-destructive text-xs">
          {error}
        </p>
      )}
      <div className="flex items-center gap-2">
        <Button type="submit" variant="outline" size="sm" className="w-fit" disabled={saving}>
          {editing ? TASK_FORM_EDIT_SUBMIT_LABEL : TASK_FORM_ADD_SUBMIT_LABEL}
        </Button>
        {onCancel !== undefined && (
          <Button type="button" variant="ghost" size="sm" disabled={saving} onClick={onCancel}>
            {TASK_FORM_CANCEL_LABEL}
          </Button>
        )}
      </div>
    </form>
  );
}
