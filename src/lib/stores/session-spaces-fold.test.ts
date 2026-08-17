import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  hydrateSessionSpacesFold,
  isSpaceFolded,
  readSessionSpacesFold,
  resetSessionSpacesFoldForTest,
  SESSION_SPACES_FOLD_BUDGET,
  SESSION_SPACES_FOLD_COOKIE,
  SESSION_SPACES_FOLD_LIMIT,
  SESSION_SPACES_FOLD_MAX_AGE,
  sessionSpacesFoldCookie,
  sessionSpacesFoldStore,
  setSpaceFolded,
  setSpacesFoldedDefault,
  spaceFoldKey,
} from "@/lib/stores/session-spaces-fold";

/**
 * A jar with other people's cookies in it, and this fold's somewhere inside.
 *
 * One of the neighbours is `keeper_notes_rail_fold`, and it is here rather than
 * as decoration: the notes rail's first section is also called Spaces, and the
 * whole argument for a fourth cookie is that neither name can ever read the
 * other's value (`fold-cookie.ts:15-19`).
 */
function jar(value: string): string {
  return `theme=dark; ${SESSION_SPACES_FOLD_COOKIE}=${encodeURIComponent(value)}; keeper_notes_rail_fold=spaces%3A1`;
}

/** The assignment {@link sessionSpacesFoldCookie} produces, as a jar. */
function written(recorded: ReadonlyMap<string, boolean>): string {
  const assignment = sessionSpacesFoldCookie(recorded);
  return `theme=dark; ${assignment.slice(0, assignment.indexOf(";"))}`;
}

/**
 * The records a cookie assignment literally carries, decoded WITHOUT going back
 * through {@link readSessionSpacesFold}.
 *
 * The reader bounds a hand-edited jar too, so round-tripping the writer's own
 * limit through it would pass whether the writer bounded anything or not —
 * files-tree's own note, learned from a mutation of its `slice`.
 */
function carried(assignment: string): [string, boolean][] {
  const value = assignment.slice(SESSION_SPACES_FOLD_COOKIE.length + 1, assignment.indexOf(";"));
  const decoded = JSON.parse(decodeURIComponent(value)) as {
    r: Record<string, [string, 0 | 1][]>;
  };
  return Object.entries(decoded.r).flatMap(([rootId, pairs]) =>
    pairs.map(([spaceId, flag]): [string, boolean] => [spaceFoldKey(rootId, spaceId), flag === 1]),
  );
}

const ZONE = "01ZONEZONEZONEZONEZONEZONE";
const OTHER = "01OTHEROTHEROTHEROTHEROTHE";

/** `count` spaces, folded, in the order they were written. */
function many(count: number): Map<string, boolean> {
  const recorded = new Map<string, boolean>();
  for (let index = 0; index < count; index += 1) {
    recorded.set(spaceFoldKey(ZONE, `_spaces/space-${index}.md`), true);
  }
  return recorded;
}

beforeEach(() => {
  resetSessionSpacesFoldForTest();
});

afterEach(() => {
  resetSessionSpacesFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
  document.cookie = `${SESSION_SPACES_FOLD_COOKIE}=; path=/; max-age=0`;
  vi.restoreAllMocks();
});

describe("readSessionSpacesFold", () => {
  it("reads every recorded space out of a jar full of other cookies", () => {
    const recorded = readSessionSpacesFold(
      jar(`{"v":1,"r":{"${ZONE}":[["_spaces/tasks.md",1],["_spaces/refs.md",0]]}}`),
    );

    expect(recorded).toEqual(
      new Map([
        [spaceFoldKey(ZONE, "_spaces/tasks.md"), true],
        [spaceFoldKey(ZONE, "_spaces/refs.md"), false],
      ]),
    );
  });

  /**
   * `false` is a recorded answer and absence is not — the third state the whole
   * store exists for. A reader that dropped the zeroes would make "I opened
   * this one" indistinguishable from "I have never touched it", and the setting
   * would shut it again at the next mount.
   */
  it("keeps an unfolded space as a record rather than as an absence", () => {
    const recorded = readSessionSpacesFold(jar(`{"v":1,"r":{"${ZONE}":[["_spaces/refs.md",0]]}}`));

    expect(recorded.get(spaceFoldKey(ZONE, "_spaces/refs.md"))).toBe(false);
    expect(recorded.has(spaceFoldKey(ZONE, "_spaces/refs.md"))).toBe(true);
  });

  it("keeps two zones apart", () => {
    const recorded = readSessionSpacesFold(
      jar(`{"v":1,"r":{"${ZONE}":[["_spaces/tasks.md",1]],"${OTHER}":[["_spaces/tasks.md",0]]}}`),
    );

    expect(recorded.get(spaceFoldKey(ZONE, "_spaces/tasks.md"))).toBe(true);
    expect(recorded.get(spaceFoldKey(OTHER, "_spaces/tasks.md"))).toBe(false);
    expect(recorded.size).toBe(2);
  });

  /** Matrix row 7: a garbage value is ignored, and every space falls back to
   *  the setting rather than to a thrown render. */
  it("discards a record it cannot read rather than throwing at boot", () => {
    for (const value of [
      "",
      "{",
      "null",
      "[]",
      '"a"',
      "7",
      '{"v":2,"r":{}}',
      '{"v":1}',
      '{"r":{}}',
      `{"v":1,"r":{"${ZONE}":"_spaces/tasks.md"}}`,
    ]) {
      expect(readSessionSpacesFold(jar(value)), value).toEqual(new Map());
    }
  });

  it("drops the entries inside a record that are not a space and a flag", () => {
    const recorded = readSessionSpacesFold(
      jar(
        `{"v":1,"r":{"":[["_spaces/nowhere.md",1]],"${ZONE}":[["_spaces/tasks.md",1],["_spaces/half.md"],[7,1],["",1],["_spaces/two.md",2],["_spaces/refs.md",0]]}}`,
      ),
    );

    expect(recorded).toEqual(
      new Map([
        [spaceFoldKey(ZONE, "_spaces/tasks.md"), true],
        [spaceFoldKey(ZONE, "_spaces/refs.md"), false],
      ]),
    );
  });

  it("is nothing recorded when this fold's own cookie is absent", () => {
    expect(readSessionSpacesFold("")).toEqual(new Map());
    // The collision this cookie name exists to make impossible: a jar holding
    // the notes rail's fold and the chat sidebar's says nothing about a
    // session's spaces.
    expect(
      readSessionSpacesFold("keeper_notes_rail_fold=spaces%3A1; keeper_sidebar_fold=spaces%3A1"),
    ).toEqual(new Map());
  });

  /** Matrix row 6, on the way IN: a hand-edited jar cannot make this bigger
   *  than the writer would have. */
  it("bounds a hand-edited jar to the limit, keeping the most recent", () => {
    const value = `{"v":1,"r":{"${ZONE}":[${[...many(40).keys()]
      .map((key) => `["${key.slice(key.indexOf("\u0000") + 1)}",1]`)
      .join(",")}]}}`;

    const recorded = readSessionSpacesFold(jar(value));

    expect(recorded.size).toBe(SESSION_SPACES_FOLD_LIMIT);
    expect(recorded.has(spaceFoldKey(ZONE, "_spaces/space-39.md"))).toBe(true);
    expect(recorded.has(spaceFoldKey(ZONE, "_spaces/space-0.md"))).toBe(false);
  });
});

describe("sessionSpacesFoldCookie", () => {
  it("round-trips every record it wrote, in the order it wrote them", () => {
    const recorded = new Map([
      [spaceFoldKey(ZONE, "_spaces/tasks.md"), true],
      [spaceFoldKey(ZONE, "_spaces/refs.md"), false],
      [spaceFoldKey(OTHER, "_spaces/log.md"), true],
    ]);

    expect(readSessionSpacesFold(written(recorded))).toEqual(recorded);
    expect([...readSessionSpacesFold(written(recorded)).keys()]).toEqual([...recorded.keys()]);
  });

  it("carries a year and the section-scoped path, like every other fold", () => {
    const assignment = sessionSpacesFoldCookie(
      new Map([[spaceFoldKey(ZONE, "_spaces/tasks.md"), true]]),
    );

    expect(assignment).toContain(`max-age=${SESSION_SPACES_FOLD_MAX_AGE}`);
    expect(assignment).toContain("path=/");
    expect(assignment).toContain("samesite=lax");
  });

  /** Nothing recorded is FORGOTTEN, not written as an empty record: the person
   *  gets their bytes back and a clean start. */
  it("clears the cookie when nothing is recorded any more", () => {
    const assignment = sessionSpacesFoldCookie(new Map());

    expect(assignment).toContain("max-age=0");
    expect(assignment.startsWith(`${SESSION_SPACES_FOLD_COOKIE}=;`)).toBe(true);
  });

  /**
   * Matrix row 6: forty spaces folded.
   *
   * The count is what binds at ordinary id lengths, the oldest records are the
   * ones that go, nothing is corrupted and nothing throws — and nothing is
   * PRINTED: the limit is the documented path, `persist` runs on every toggle,
   * and a zone with 33 recorded folds would otherwise log a line per press.
   */
  it("keeps the most recent records when there are more than it can hold", () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => {});

    const assignment = sessionSpacesFoldCookie(many(40));
    const kept = carried(assignment);

    expect(kept.length).toBe(SESSION_SPACES_FOLD_LIMIT);
    expect(kept[kept.length - 1]?.[0]).toBe(spaceFoldKey(ZONE, "_spaces/space-39.md"));
    expect(kept[0]?.[0]).toBe(spaceFoldKey(ZONE, "_spaces/space-8.md"));
    expect(assignment).toContain(`max-age=${SESSION_SPACES_FOLD_MAX_AGE}`);
    expect(readSessionSpacesFold(written(many(40))).size).toBe(SESSION_SPACES_FOLD_LIMIT);
    // Silent, and this is the assertion that keeps it silent: reporting the
    // ordinary bound on every toggle is noise the operator cannot act on.
    expect(info).not.toHaveBeenCalled();
  });

  /**
   * The byte budget, the backstop the count cannot provide: ids long enough
   * that even a legal number of them would be dropped whole by the browser.
   */
  it("stays inside the budget when the ids are long enough to blow it", () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    const recorded = new Map<string, boolean>();
    for (let index = 0; index < SESSION_SPACES_FOLD_LIMIT; index += 1) {
      recorded.set(spaceFoldKey(ZONE, `_spaces/${"x".repeat(200)}-${index}.md`), true);
    }

    const assignment = sessionSpacesFoldCookie(recorded);
    const value = assignment.slice(SESSION_SPACES_FOLD_COOKIE.length + 1, assignment.indexOf(";"));

    expect(value.length).toBeLessThanOrEqual(SESSION_SPACES_FOLD_BUDGET);
    const kept = carried(assignment);
    expect(kept.length).toBeGreaterThan(0);
    expect(kept.length).toBeLessThan(SESSION_SPACES_FOLD_LIMIT);
    // The newest survives the squeeze; the oldest is what was spent.
    expect(kept[kept.length - 1]?.[0]).toBe(spaceFoldKey(ZONE, `_spaces/${"x".repeat(200)}-31.md`));
    // The one bound worth a line, and it names the real count: nobody can
    // predict from 32 spaces that their ids were long enough to be squeezed.
    expect(info).toHaveBeenCalledWith(
      expect.stringContaining(`${kept.length} of ${SESSION_SPACES_FOLD_LIMIT}`),
    );
  });
});

describe("the composition of a fold, a space's own answer and the default", () => {
  it("follows the setting for a space with nothing recorded and nothing said", () => {
    const key = spaceFoldKey(ZONE, "_spaces/tasks.md");

    expect(isSpaceFolded(sessionSpacesFoldStore.getState(), key, null)).toBe(false);
    setSpacesFoldedDefault(true);
    expect(isSpaceFolded(sessionSpacesFoldStore.getState(), key, null)).toBe(true);
  });

  /** The rule the whole story is about: a hand-made answer outranks the
   *  setting, in BOTH directions. */
  it("keeps a recorded answer when the setting changes under it", () => {
    const opened = spaceFoldKey(ZONE, "_spaces/refs.md");
    const shut = spaceFoldKey(ZONE, "_spaces/tasks.md");
    setSpaceFolded(opened, false);
    setSpaceFolded(shut, true);

    setSpacesFoldedDefault(true);
    expect(isSpaceFolded(sessionSpacesFoldStore.getState(), opened, null)).toBe(false);
    setSpacesFoldedDefault(false);
    expect(isSpaceFolded(sessionSpacesFoldStore.getState(), shut, null)).toBe(true);
  });

  /** Story 51.3 rows 1, 3 and 4: the space's own `keeper.folded` sits between
   *  the hand-fold and the setting, so a file that says `false` arrives unfolded
   *  even with the setting ON — the file beats the setting, in both directions.
   */
  it("rows 1, 3, 4: the space's own answer beats the setting either way", () => {
    const key = spaceFoldKey(ZONE, "_spaces/tasks.md");

    // Row 1: the file says folded, the setting is off.
    expect(isSpaceFolded(sessionSpacesFoldStore.getState(), key, true)).toBe(true);
    setSpacesFoldedDefault(true);
    // Row 4: the file says unfolded, the setting is on.
    expect(isSpaceFolded(sessionSpacesFoldStore.getState(), key, false)).toBe(false);
    // Row 3: the file says nothing, so the setting is what is left.
    expect(isSpaceFolded(sessionSpacesFoldStore.getState(), key, null)).toBe(true);
  });

  /** Row 2: the person's own hand outranks the file, which is the layer order's
   *  whole point — a space you shut stays shut until you open it, whatever its
   *  definition says. */
  it("row 2: a hand-fold beats the space's own answer", () => {
    const opened = spaceFoldKey(ZONE, "_spaces/tasks.md");
    const shut = spaceFoldKey(ZONE, "_spaces/refs.md");
    setSpaceFolded(opened, false);
    setSpaceFolded(shut, true);

    expect(isSpaceFolded(sessionSpacesFoldStore.getState(), opened, true)).toBe(false);
    expect(isSpaceFolded(sessionSpacesFoldStore.getState(), shut, false)).toBe(true);
  });

  it("records a fold in the document and reads it back", () => {
    const key = spaceFoldKey(ZONE, "_spaces/tasks.md");

    setSpaceFolded(key, true);

    expect(document.cookie).toContain(SESSION_SPACES_FOLD_COOKIE);
    expect(readSessionSpacesFold(document.cookie).get(key)).toBe(true);
  });

  /** Recency is what eviction ranks by, so re-recording a space has to move it
   *  to the end — otherwise a space somebody folds every day is evicted while a
   *  space they touched once and forgot survives. */
  it("makes a re-recorded space the most recent one", () => {
    const first = spaceFoldKey(ZONE, "_spaces/a.md");
    const second = spaceFoldKey(ZONE, "_spaces/b.md");
    setSpaceFolded(first, true);
    setSpaceFolded(second, true);

    setSpaceFolded(first, false);

    expect([...sessionSpacesFoldStore.getState().recorded.keys()]).toEqual([second, first]);
  });
});

describe("hydrateSessionSpacesFold", () => {
  it("restores the records and the setting they fall back to", () => {
    hydrateSessionSpacesFold(
      jar(`{"v":1,"r":{"${ZONE}":[["_spaces/tasks.md",1],["_spaces/refs.md",0]]}}`),
      true,
    );

    const state = sessionSpacesFoldStore.getState();
    expect(isSpaceFolded(state, spaceFoldKey(ZONE, "_spaces/tasks.md"), null)).toBe(true);
    expect(isSpaceFolded(state, spaceFoldKey(ZONE, "_spaces/refs.md"), null)).toBe(false);
    // Nothing recorded and nothing said, so the setting decides.
    expect(isSpaceFolded(state, spaceFoldKey(ZONE, "_spaces/log.md"), null)).toBe(true);
  });

  /** Matrix row 7, end to end: a jar somebody edited into nonsense costs the
   *  restore and nothing else — every space falls back to the setting. */
  it("row 7: leaves every space on the setting when the cookie is garbage", () => {
    hydrateSessionSpacesFold(jar("{not json at all"), true);

    const state = sessionSpacesFoldStore.getState();
    expect(state.recorded.size).toBe(0);
    expect(isSpaceFolded(state, spaceFoldKey(ZONE, "_spaces/tasks.md"), null)).toBe(true);
  });

  /**
   * Idempotent, so React's double-invoked development effects restore once and
   * a second detail cannot overwrite a fold the person has changed since the
   * first — including with a setting read that was in flight while they pressed
   * something.
   */
  it("does not run twice over a fold the person has changed since", () => {
    const key = spaceFoldKey(ZONE, "_spaces/tasks.md");
    hydrateSessionSpacesFold("", false);
    setSpaceFolded(key, true);

    hydrateSessionSpacesFold(jar(`{"v":1,"r":{"${ZONE}":[["_spaces/tasks.md",0]]}}`), true);

    const state = sessionSpacesFoldStore.getState();
    expect(state.recorded.get(key)).toBe(true);
    expect(state.defaultFolded).toBe(false);
  });
});
