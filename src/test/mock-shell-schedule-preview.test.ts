/**
 * The dev harness answers schedule previews from the real dialect (Story 59.7).
 *
 * ## Why this file exists rather than a comment
 *
 * `dev/mock-shell.ts` is the only shell the frontend sees under `bun run dev`,
 * and it is the surface a person judges the schedule helper on. Two things can
 * drift between it and `keeper_sync::tasks`, and each one produces a dev shell
 * that is *more generous than the app* — the worst direction, because the
 * expression looks fine right up until it is saved:
 *
 * 1. **Which expressions it accepts.** Already guarded from the other side:
 *    `keeper-sync/src/tasks.rs`'s
 *    `every_schedule_the_dev_harness_shows_is_one_this_dialect_accepts` reads
 *    this file's stored-schedule literals through `TaskSchedule::parse`. That
 *    guard covers the fixtures; it cannot reach the preview handler, which
 *    computes rather than stores.
 * 2. **How many instants it answers with.** This one has no guard at all until
 *    here, and it is not cosmetic. `TaskSchedule::Every` fires `interval_ms`
 *    after the END of the previous run — `Engine::next_task_window` re-derives
 *    it from `finished_ms` — so instants two and three of an interval schedule
 *    depend on how long the first run takes. `preview_schedule` therefore
 *    answers exactly **one** instant for an interval and up to the full count
 *    for a cron pattern, which names wall-clock instants and has no such
 *    dependency. A harness that chained three for `every 6h` would be showing
 *    arithmetic dressed as knowledge.
 *
 * The second rule was written into the handler as a comment first. A comment is
 * what the next person deletes while tidying; this is the assertion that stops
 * them.
 */
import { describe, expect, it } from "vitest";
import { mockSchedulePreview } from "../../dev/mock-shell";

/** The floor and the ceiling the real parser refuses on either side of. */
const YEAR_MS = 366 * 86_400_000;

describe("the dev harness's schedule preview", () => {
  it("answers one instant for an interval, because the next one is unknowable", () => {
    // The property, stated as the number: an interval's second fire depends on
    // when the first RUN ends, which nothing here — and nothing in Rust — can
    // know before it has run.
    for (const expression of ["every 6h", "every 90m", "every 60s", "every 2 days"]) {
      const preview = mockSchedulePreview(expression);
      expect(preview.refusal, expression).toBeNull();
      expect(preview.instants, expression).toHaveLength(1);
    }
  });

  it("answers three for a calendar pattern, which does not depend on a run", () => {
    // Aliases desugar to cron rather than to intervals — that is why `@daily`
    // keeps meaning night instead of drifting to the last restart — so they
    // belong on this side of the rule and not the other.
    for (const expression of ["0 3 * * *", "@hourly", "@daily", "@weekly"]) {
      const preview = mockSchedulePreview(expression);
      expect(preview.refusal, expression).toBeNull();
      expect(preview.instants, expression).toHaveLength(3);
    }
  });

  it("orders the instants soonest first and never repeats one", () => {
    // "Soonest first" is the wire contract, and a chain that returned the same
    // answer three times would satisfy a length assertion and nothing else.
    const { instants } = mockSchedulePreview("@daily");
    expect(instants).toEqual([...instants].sort((a, b) => a - b));
    expect(new Set(instants).size).toBe(instants.length);
  });

  it("refuses everything outside the dialect, and never with empty prose", () => {
    // Including the two the save door also refuses rather than treating as
    // "store no schedule": a preview that quietly approved whitespace would
    // disagree with the verb it is previewing.
    for (const expression of ["", "   ", "@daily 03:00", "0 3 * *", "every 6 fortnights", "soon"]) {
      const preview = mockSchedulePreview(expression);
      expect(preview.refusal, expression).not.toBeNull();
      expect(preview.refusal, expression).not.toBe("");
      expect(preview.instants, expression).toEqual([]);
    }
  });

  it("quotes the text that was typed, the way Rust's own refusals do", () => {
    // `TaskSchedule::parse` formats the ORIGINAL with `{original:?}` — Rust's
    // `Debug` for a string, so double quotes — and quotes what was typed rather
    // than a lowercased or trimmed copy. A harness that paraphrased would let
    // the app's real wording change while the dev shell went on showing the old
    // one.
    expect(mockSchedulePreview("soon").refusal).toContain('got "soon"');
    expect(mockSchedulePreview("  soon  ").refusal).toContain('got "soon"');
  });

  it("names the floor and the ceiling rather than calling either one malformed", () => {
    // `every 30s` is in the grammar precisely so a person is told about the
    // sixty-second floor instead of about an unknown unit, and the ceiling has
    // its own sentence pointing at calendar patterns.
    const tooOften = mockSchedulePreview("every 30s").refusal ?? "";
    expect(tooOften).toContain("more often than once a minute");
    expect(tooOften).toContain("60000");

    const tooRare = mockSchedulePreview("every 400d").refusal ?? "";
    expect(tooRare).toContain("less often than once a year");
    expect(tooRare).toContain(String(YEAR_MS));
  });

  it("refuses a cron that parses and names no real date", () => {
    // 30 February: the one cron refusal a person actually meets, and the
    // parser's own example. It is a distinct sentence from malformed, because
    // the expression IS well-formed.
    const preview = mockSchedulePreview("0 0 30 2 *");
    expect(preview.refusal).toContain("matches no instant");
    expect(preview.refusal).not.toContain("must be a 5-field cron expression");
  });

  it("echoes back exactly what it was asked about", () => {
    // The caller drops a reply a newer keystroke has made stale by comparing
    // this against the field, so it must be the untrimmed original rather than
    // the normalised copy the parser reasons with.
    for (const expression of ["  @daily  ", "EVERY 6H", "nonsense"]) {
      expect(mockSchedulePreview(expression).expression).toBe(expression);
    }
  });
});
