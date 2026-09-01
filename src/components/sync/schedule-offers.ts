/**
 * The schedule dialect, offered rather than recalled (Epic 59, Story 59.7,
 * FR-368).
 *
 * Before this file the schedule box was a bare text input with one placeholder
 * and one sentence naming the grammar in passing. That is enough for whoever
 * wrote the parser and nobody else: the accepted forms are a five-field cron
 * expression, three `@` aliases and `every <n><unit>`, with a floor of once a
 * minute, a ceiling of once a year, and a refusal for a pattern that parses to
 * a date the calendar does not contain. Nothing on screen offered any of that,
 * so writing a schedule meant either knowing cron by heart or provoking
 * refusals until one stopped arriving.
 *
 * **What this file is not.** It is not a parser, not a validator and not a
 * predictor. Every expression below is a *starting point that gets typed into
 * the box*, and the box's contents still go to `TaskSchedule::parse` verbatim —
 * whitespace and all — to be accepted or refused there. The next-fire instants
 * the form shows beside the box are computed by Rust, by the same
 * `next_due_after` the engine's tick walks; there is deliberately no cron
 * arithmetic in TypeScript anywhere in this codebase, because a second
 * implementation of the dialect would drift and the first symptom would be a
 * preview that disagreed with the engine about when a task runs.
 *
 * **Why the offered list is proved from Rust and not from reading it.** Every
 * expression here is fed through the real `TaskSchedule::parse` by
 * `keeper-sync`'s own test suite — `every_schedule_the_form_offers_is_one_this_dialect_accepts`
 * reads this file, extracts each `expression` literal and parses it. That guard
 * is not decoration: Story 58.4 shipped three dev-harness fixtures describing
 * schedules the parser refuses (`@daily 03:00`, a syntax the dialect has never
 * had), and a help menu that offers a refusal is the same defect aimed at a
 * person instead of a developer. The direction is unusual — Rust reading
 * TypeScript, where this repo's other cross-language guards read the other way —
 * and it is the only direction available, because only Rust can run the parser.
 */

/**
 * One offered form: what gets typed into the box, and what it means.
 *
 * `says` is prose and therefore the part that can be wrong. It is kept short,
 * and it is kept honest by the preview: choosing an offer fills the box, the
 * box asks Rust, and Rust answers with the instants. So a description that
 * drifted from its expression is contradicted on screen by the engine itself
 * rather than believed.
 */
export type TaskScheduleOffer = {
  /** The expression, exactly as it is typed into the schedule box. */
  expression: string;
  /** When it fires, in one clause. */
  says: string;
};

/**
 * The forms the schedule box offers, in increasing order of period.
 *
 * Deliberately a short list rather than a catalogue: it covers the two halves
 * of the dialect that a person cannot guess (the `@` aliases, and `every
 * <n><unit>`'s spelling) plus enough cron shapes — a plain daily time, a
 * weekday, a day of the month, a step — to read the grammar off the examples.
 * A longer list would teach no more and would be a longer list of sentences
 * that can rot.
 *
 * Sunday, not Monday, for `@weekly`: the alias desugars to `0 0 * * 0` and this
 * dialect counts weekdays from Sunday, which is a fact worth stating where
 * somebody is choosing rather than leaving them to find out a week later.
 */
export const TASK_SCHEDULE_OFFERS: readonly TaskScheduleOffer[] = [
  { expression: "*/15 * * * *", says: "every 15 minutes, on the quarter hour" },
  { expression: "@hourly", says: "every hour, on the hour" },
  { expression: "every 90m", says: "90 minutes after each run finishes" },
  { expression: "every 6h", says: "6 hours after each run finishes" },
  { expression: "@daily", says: "every day at midnight" },
  { expression: "0 3 * * *", says: "every day at 03:00" },
  { expression: "@weekly", says: "every Sunday at midnight" },
  { expression: "30 2 * * 1", says: "every Monday at 02:30" },
  { expression: "0 4 1 * *", says: "the 1st of every month at 04:00" },
];

/**
 * The fastest keeper will let a task fire, in minutes.
 *
 * Mirrors `MIN_SCHEDULE_INTERVAL_MS` in `keeper-sync/src/tasks.rs`, and the
 * mirror is asserted mechanically by the guard in `task-form.test.tsx` — Rust
 * cannot import a TypeScript literal and no ts-rs binding carries these two
 * numbers, so reading the Rust source is the only direction available. That
 * guard exists because a sentence in this very form once shipped claiming
 * fifteen minutes against a thirty-minute constant (Story 58.9).
 */
export const TASK_SCHEDULE_FLOOR_MINUTES = 1;
/**
 * The slowest keeper will let a task fire, in days.
 *
 * See {@link TASK_SCHEDULE_FLOOR_MINUTES} — mirrors `MAX_SCHEDULE_INTERVAL_MS`,
 * and is pinned to it by the same guard. 366 rather than 365 because the
 * constant is written as a leap year's worth of days.
 */
export const TASK_SCHEDULE_CEILING_DAYS = 366;

/**
 * *"once a minute"*, *"once every 366 days"* — a period named without a bare
 * `1` standing in front of a singular noun.
 *
 * A two-line function with two call sites, which is normally one too few to
 * earn a name. It earns one here because it is the **test seam**: the guard
 * that pins {@link taskScheduleBoundsNote} to Rust's constants has to name the
 * phrase it expects, and a guard that retyped the phrasing would pass while the
 * sentence drifted — which is the precise defect this whole family of composed
 * notes exists to prevent.
 *
 * Assumes a positive whole number, which is all either caller can hold: the two
 * constants are integer literals, and the mirror guard fails on a Rust constant
 * that stops dividing into whole minutes or days before one could reach here. At
 * nought or below the phrasing would read "once every 0 minutes", which is why
 * the assumption is written down rather than defended — there is no honest
 * sentence for a floor of nought, and the guard is the right place to notice it.
 */
export function taskSchedulePeriodPhrase(count: number, unit: string): string {
  return count === 1 ? `once a ${unit}` : `once every ${count} ${unit}s`;
}

/**
 * What the box refuses at either end, composed from the two constants.
 *
 * Composed and not written, for the reason the missed-window note is composed:
 * a literal here is a number that goes on being displayed after the constant it
 * describes has moved, and this form has already shipped exactly that defect
 * once (Story 58.9).
 */
export function taskScheduleBoundsNote(floorMinutes: number, ceilingDays: number): string {
  const floor = taskSchedulePeriodPhrase(floorMinutes, "minute");
  const ceiling = taskSchedulePeriodPhrase(ceilingDays, "day");
  return `Nothing may fire more often than ${floor}, and nothing less often than ${ceiling} — a longer gap than that is a calendar pattern, which the cron form writes exactly. keeper refuses both ends, and refuses a pattern naming a date the calendar has no room for, quoting what you typed rather than rounding it.`;
}

/** The bounds as the form renders them. */
export const TASK_SCHEDULE_BOUNDS_NOTE = taskScheduleBoundsNote(
  TASK_SCHEDULE_FLOOR_MINUTES,
  TASK_SCHEDULE_CEILING_DAYS,
);
