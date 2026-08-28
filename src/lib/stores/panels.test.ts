import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PanelTargetVm } from "@/lib/ipc/client";
import {
  activePanel,
  hydratePanels,
  isRestorableTarget,
  PANELS_COOKIE,
  PANELS_COOKIE_BUDGET,
  panelsCookie,
  panelsStore,
  readPanelTargets,
  resetPanelsStoreForTest,
  sameTarget,
} from "@/lib/stores/panels";

const A: PanelTargetVm = { kind: "file", profileId: "p1", relativePath: "a.md" };
const B: PanelTargetVm = { kind: "file", profileId: "p1", relativePath: "b.pdf" };
const C: PanelTargetVm = { kind: "file", profileId: "p1", relativePath: "c.csv" };
const NOTE_ONE: PanelTargetVm = { kind: "note", vaultId: "v1", noteId: "n1" };
const NOTE_TWO: PanelTargetVm = { kind: "note", vaultId: "v1", noteId: "n2" };

function store() {
  return panelsStore.getState();
}

/** The targets on screen, left to right — the thing every assertion is about. */
function shown(): (PanelTargetVm | null)[] {
  return store().panels.map((panel) => panel.target);
}

/** Forget every cookie this document holds, so one test's arrangement can never
 *  be the next test's restore. */
function clearCookies(): void {
  for (const part of document.cookie.split(";")) {
    const name = part.split("=")[0]?.trim();
    if (name !== undefined && name !== "") {
      // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
      document.cookie = `${name}=; path=/; max-age=0`;
    }
  }
}

beforeEach(() => {
  clearCookies();
  resetPanelsStoreForTest();
});

describe("the panel list", () => {
  it("starts as one empty panel, because a workspace with none has no way back", () => {
    expect(store().panels).toHaveLength(1);
    expect(shown()).toEqual([null]);
    expect(activePanel(store()).id).toBe(store().activeId);
  });

  it("replaces the active panel's target on a single click and does not grow", () => {
    store().setActiveTarget(A);
    store().setActiveTarget(B);

    expect(store().panels).toHaveLength(1);
    expect(shown()).toEqual([B]);
  });

  it("appends beside the active panel on a double click", () => {
    store().setActiveTarget(A);
    store().openPanel(B);

    expect(shown()).toEqual([A, B]);
    expect(activePanel(store()).target).toEqual(B);
  });

  it("puts back what the click displaced, so a double click never opens a twin", () => {
    // The gesture as the DOM delivers it: a real double click fires `click`
    // before `dblclick`, so the naive model would replace A with B and then open
    // B beside itself — two panels of B, and A gone. This is the whole reason
    // `replaced` exists.
    store().openPanel(A); // A is pinned, the document the user is reading
    store().setActiveTarget(B); // the first click of the double click
    store().openPanel(B); // the double click itself

    expect(shown()).toEqual([A, B]);
    expect(activePanel(store()).target).toEqual(B);
  });

  it("keeps the panel that started a run of previews, not the last preview", () => {
    store().openPanel(A);
    store().setActiveTarget(B);
    store().setActiveTarget(C);
    store().openPanel(C);

    // Previewing B and then C never pinned B; the document the user actually
    // had open is A, and pinning C puts A back beside it.
    expect(shown()).toEqual([A, C]);
  });

  it("pins a preview in place when there was nothing under it to put back", () => {
    // The starting panel shows nothing, so previewing A and then B displaced no
    // document. Double-clicking B must therefore pin B where it is — restoring
    // "nothing" beside it would leave an empty frame the user did not ask for.
    store().setActiveTarget(A);
    store().setActiveTarget(B);
    store().openPanel(B);

    expect(shown()).toEqual([B]);
  });

  it("focuses the panel that already holds a target rather than opening a second", () => {
    store().setActiveTarget(A);
    store().openPanel(B);
    const first = store().panels[0];
    if (first === undefined) {
      throw new Error("expected two panels");
    }

    store().openPanel(A);

    expect(shown()).toEqual([A, B]);
    expect(store().activeId).toBe(first.id);
  });

  it("fills the empty starting panel instead of opening beside it", () => {
    store().openPanel(A);

    expect(shown()).toEqual([A]);
  });

  it("opens a second note beside the first, like any other target", () => {
    // Story 46.12 inverted this. It used to retarget the one note panel, and
    // that was not a policy about tidiness: the note document mirror was a
    // module singleton (AD-58), so a second mounted editor would have taken the
    // store and the first would have shown the second's text under the first's
    // title. The mirror is keyed by note now — one document per note, reference
    // counted — so there is nothing left to protect and a note is an ordinary
    // target.
    store().setActiveTarget(A);
    store().openPanel(NOTE_ONE);
    store().openPanel(NOTE_TWO);

    expect(shown()).toEqual([A, NOTE_ONE, NOTE_TWO]);
    expect(store().panels.filter((panel) => panel.target?.kind === "note")).toHaveLength(2);
  });
});

/**
 * A rename, which is neither of the two click gestures (Story 52.2, FR-302).
 *
 * It shipped as `setActiveTarget`, and that was wrong in two ways that this
 * suite is the only place able to say plainly. It moved the panel with FOCUS
 * rather than the panel holding the file — and a title commits on blur, so
 * "type the new title, then click the other pane" hands focus to the other pane
 * first. And it recorded the rename as a PREVIEW of the path the rename had just
 * emptied, which `openPanel`'s restore branch then put back.
 */
describe("a file that was renamed under the panels showing it", () => {
  /** Where the rename answered it went: a different folder and a different
   *  filename, so nothing here could have been composed from `A`. */
  const RENAMED: PanelTargetVm = {
    kind: "file",
    profileId: "p1",
    relativePath: "archive/2026-02/a-renamed.md",
  };

  it("moves the panel holding the file even when another one has focus", () => {
    // The blur-then-click-the-other-pane sequence, in the state it leaves behind:
    // the reader renamed from the left pane and then clicked into the right one,
    // so by the time the command answers the right pane is the active one.
    store().setActiveTarget(A);
    store().openPanel(B);
    const [left, right] = store().panels;
    if (left === undefined || right === undefined) {
      throw new Error("expected two panels");
    }
    expect(store().activeId).toBe(right.id);

    store().retargetPanels(A, RENAMED);

    // The pane that was showing it followed; the pane the reader clicked into
    // kept what it was showing, and still has focus.
    expect(shown()).toEqual([RENAMED, B]);
    expect(store().activeId).toBe(right.id);
  });

  it("moves every panel holding the file, not whichever one is focused", () => {
    store().setActiveTarget(A);
    store().openPanel(B);
    store().setActiveTarget(A);

    expect(shown()).toEqual([A, A]);

    store().retargetPanels(A, RENAMED);

    // Both, or the second one persists a dead path into the cookie and comes back
    // after a restart showing "is no longer in tgdrive".
    expect(shown()).toEqual([RENAMED, RENAMED]);
  });

  it("does not flag the panel as a preview of the address the rename emptied", () => {
    // The resurrection: the panel really held `A`, so recording the retarget as a
    // preview would record `was: A` — the path that no longer exists. Then the
    // double click that opens the renamed file from the tree hits `openPanel`'s
    // restore branch, puts `A` back and appends the file beside it, so the reader
    // gets the missing-file banner they had just been spared plus a second panel.
    store().openPanel(A);
    store().retargetPanels(A, RENAMED);

    store().openPanel(RENAMED);

    expect(shown()).toEqual([RENAMED]);
    expect(store().panels[0]?.replaced).toBeNull();
  });

  it("leaves a preview a click before it recorded alone when the rename moved nothing", () => {
    // The session record keeps its filename, so `sessions_file_rename` answers
    // with the path it was given. Nothing follows anything — and the preview the
    // click before it recorded has to survive, or the double click it is half of
    // stops putting the displaced document back.
    store().openPanel(A);
    store().setActiveTarget(B);

    store().retargetPanels(B, B);

    expect(store().panels[0]?.replaced).toEqual({ was: A });
    store().openPanel(B);
    expect(shown()).toEqual([A, B]);
  });

  it("leaves the arrangement untouched when no panel is holding the file", () => {
    store().setActiveTarget(B);
    const before = store().panels;

    store().retargetPanels(A, RENAMED);

    // The same list, not an equal one: a rename of a file nobody is looking at
    // must not rewrite the cookie either.
    expect(store().panels).toBe(before);
  });

  it("keeps a folded panel folded, because it is still the document it folded", () => {
    store().setActiveTarget(A);
    const only = store().panels[0];
    if (only === undefined) {
      throw new Error("expected one panel");
    }
    store().toggleFold(only.id);

    store().retargetPanels(A, RENAMED);

    // Every other verb that gives a panel a target unfolds it, because it is
    // showing the reader something new. This one is not: springing a pane open
    // because somebody retitled the file already in it is keeper rearranging the
    // workspace on its own.
    expect(shown()).toEqual([RENAMED]);
    expect(folds()).toEqual([true]);
  });

  it("writes the new address to the cookie, so the restart does not restore the old one", () => {
    store().setActiveTarget(A);

    store().retargetPanels(A, RENAMED);

    resetPanelsStoreForTest();
    hydratePanels(document.cookie);
    expect(shown()).toEqual([RENAMED]);
  });
});

describe("closing a panel", () => {
  it("refuses the last one", () => {
    store().setActiveTarget(A);
    const only = store().panels[0];
    if (only === undefined) {
      throw new Error("expected one panel");
    }

    store().closePanel(only.id);

    expect(store().panels).toHaveLength(1);
    expect(shown()).toEqual([A]);
  });

  it("moves focus to the panel that slides into a closed middle one's place", () => {
    store().setActiveTarget(A);
    store().openPanel(B);
    store().openPanel(C);
    const [, middle, right] = store().panels;
    if (middle === undefined || right === undefined) {
      throw new Error("expected three panels");
    }
    store().focusPanel(middle.id);

    store().closePanel(middle.id);

    expect(shown()).toEqual([A, C]);
    expect(store().activeId).toBe(right.id);
  });

  it("falls back to the left neighbour when the rightmost closes", () => {
    store().setActiveTarget(A);
    store().openPanel(B);
    const [left, right] = store().panels;
    if (left === undefined || right === undefined) {
      throw new Error("expected two panels");
    }

    store().closePanel(right.id);

    expect(shown()).toEqual([A]);
    expect(store().activeId).toBe(left.id);
  });

  it("leaves focus alone when a panel other than the focused one closes", () => {
    store().setActiveTarget(A);
    store().openPanel(B);
    const [left, right] = store().panels;
    if (left === undefined || right === undefined) {
      throw new Error("expected two panels");
    }

    store().closePanel(left.id);

    expect(store().activeId).toBe(right.id);
  });
});

describe("a target that was deliberately deleted", () => {
  it("stops being shown anywhere", () => {
    store().setActiveTarget(A);
    store().openPanel(B);
    store().openPanel(C);

    store().closeTarget(B);

    expect(shown()).toEqual([A, C]);
  });

  it("empties the last panel rather than refusing, so the deleted thing goes away", () => {
    store().setActiveTarget(A);

    store().closeTarget(A);

    expect(store().panels).toHaveLength(1);
    expect(shown()).toEqual([null]);
    expect(activePanel(store()).id).toBe(store().activeId);
  });

  it("does nothing at all for a target no panel holds", () => {
    store().setActiveTarget(A);
    const before = store().panels;

    store().closeTarget(B);

    expect(store().panels).toBe(before);
  });
});

describe("surviving a restart", () => {
  it("round-trips the arrangement and the focused panel through a cookie", () => {
    store().setActiveTarget(A);
    store().openPanel(B);
    store().openPanel(C);
    const middle = store().panels[1];
    if (middle === undefined) {
      throw new Error("expected three panels");
    }
    store().focusPanel(middle.id);

    // What the next launch actually reads.
    resetPanelsStoreForTest();
    hydratePanels(document.cookie);

    expect(shown()).toEqual([A, B, C]);
    expect(activePanel(store()).target).toEqual(B);
  });

  it("restores an unresolvable target rather than dropping it", () => {
    // The drive being out is exactly when the arrangement matters most: the
    // panel has to come back when the drive does, and it cannot come back if the
    // restore quietly filtered it out for being unreachable.
    const cookie = panelsCookie([{ id: "p", target: A, replaced: null, folded: false }], "p");
    resetPanelsStoreForTest();
    hydratePanels(cookie);

    expect(shown()).toEqual([A]);
  });

  it("hydrates once, so a double-invoked effect does not re-restore over a click", () => {
    const cookie = panelsCookie([{ id: "p", target: A, replaced: null, folded: false }], "p");
    resetPanelsStoreForTest();
    hydratePanels(cookie);
    store().setActiveTarget(B);

    hydratePanels(cookie);

    expect(shown()).toEqual([B]);
  });

  it("comes up clean from a corrupt cookie instead of throwing at boot", () => {
    const empty = { targets: [], activeIndex: 0, folded: [] };
    expect(readPanelTargets(`${PANELS_COOKIE}=not-json`)).toEqual(empty);
    expect(
      readPanelTargets(`${PANELS_COOKIE}=${encodeURIComponent('{"v":9,"a":0,"t":[]}')}`),
    ).toEqual(empty);
    expect(readPanelTargets("")).toEqual(empty);
  });

  it("drops an entry whose kind this build does not know", () => {
    const cookie = `${PANELS_COOKIE}=${encodeURIComponent(
      JSON.stringify({ v: 1, a: 0, t: [{ kind: "hologram", id: "x" }, A] }),
    )}`;

    expect(readPanelTargets(cookie).targets).toEqual([A]);
  });

  it("clamps a focused index that no longer names a panel", () => {
    const cookie = `${PANELS_COOKIE}=${encodeURIComponent(
      JSON.stringify({ v: 1, a: 7, t: [A, B] }),
    )}`;

    expect(readPanelTargets(cookie).activeIndex).toBe(1);
  });

  it("forgets the arrangement when nothing is open", () => {
    expect(panelsCookie([{ id: "p", target: null, replaced: null, folded: false }], "p")).toContain(
      "max-age=0",
    );
  });

  it("remembers what fits and says how many it could not", () => {
    // A browser drops an oversized cookie silently — the assignment succeeds and
    // the value is not stored — so an arrangement that overflows must be trimmed
    // here, where it can be reported, rather than lost whole at the next launch.
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    const long = "x".repeat(400);
    const panels = Array.from({ length: 20 }, (_, index) => ({
      id: `p${index}`,
      target: { kind: "file", profileId: "p1", relativePath: `${long}/${index}.md` } as const,
      replaced: null,
      folded: false,
    }));

    const cookie = panelsCookie(panels, "p0");
    const value = cookie.slice(cookie.indexOf("=") + 1, cookie.indexOf(";"));

    expect(value.length).toBeLessThanOrEqual(PANELS_COOKIE_BUDGET);
    expect(readPanelTargets(`${PANELS_COOKIE}=${value}`).targets.length).toBeGreaterThan(0);
    expect(info).toHaveBeenCalled();
    info.mockRestore();
  });
});

describe("what a restored target is allowed to be", () => {
  it("refuses a file path that is absolute or climbs out of its profile", () => {
    // The cookie is a string the user can edit, so this is the boundary where a
    // path stops being trusted. AD-65: the frontend never names a location the
    // engine has not agreed to, and Rust contains it again on arrival (AD-59).
    expect(isRestorableTarget({ kind: "file", profileId: "p", relativePath: "/etc/passwd" })).toBe(
      false,
    );
    expect(
      isRestorableTarget({ kind: "file", profileId: "p", relativePath: "../../.ssh/id_rsa" }),
    ).toBe(false);
    expect(isRestorableTarget({ kind: "file", profileId: "p", relativePath: "a/../../b" })).toBe(
      false,
    );
    expect(isRestorableTarget({ kind: "file", profileId: "p", relativePath: "C:\\Windows" })).toBe(
      false,
    );
    expect(
      isRestorableTarget({ kind: "file", profileId: "p", relativePath: "\\\\server\\share" }),
    ).toBe(false);
    expect(isRestorableTarget({ kind: "file", profileId: "", relativePath: "a.md" })).toBe(false);
  });

  it("allows an ordinary relative path, including one that merely contains dots", () => {
    expect(isRestorableTarget({ kind: "file", profileId: "p", relativePath: "a/b..c/d.md" })).toBe(
      true,
    );
    expect(isRestorableTarget({ kind: "file", profileId: "p", relativePath: "..hidden.md" })).toBe(
      true,
    );
  });

  it("refuses an empty note or session id", () => {
    expect(isRestorableTarget({ kind: "note", vaultId: "v", noteId: "" })).toBe(false);
    expect(isRestorableTarget({ kind: "note", vaultId: "", noteId: "n" })).toBe(false);
    expect(isRestorableTarget({ kind: "recording", sessionId: "" })).toBe(false);
  });

  it("drops a refused target on the way back in", () => {
    const cookie = `${PANELS_COOKIE}=${encodeURIComponent(
      JSON.stringify({
        v: 1,
        a: 0,
        t: [{ kind: "file", profileId: "p", relativePath: "../../secrets" }, A],
      }),
    )}`;

    expect(readPanelTargets(cookie).targets).toEqual([A]);
  });
});

describe("target identity", () => {
  it("does not collapse a profile id and a path that a slash-joined address would", () => {
    expect(
      sameTarget(
        { kind: "file", profileId: "prof", relativePath: "a/b.txt" },
        { kind: "file", profileId: "prof/a", relativePath: "b.txt" },
      ),
    ).toBe(false);
  });

  it("compares by value, not by reference, so a re-read row still matches", () => {
    expect(sameTarget({ ...A }, { ...A })).toBe(true);
  });

  it("never matches across kinds that share their strings", () => {
    expect(
      sameTarget(
        { kind: "note", vaultId: "x", noteId: "y" },
        { kind: "file", profileId: "x", relativePath: "y" },
      ),
    ).toBe(false);
  });
});

/** Which panels are folded, left to right — the thing every fold assertion is
 *  about. Booleans rather than ids, because the interesting claims are all about
 *  WHICH panel in the arrangement came back folded. */
function folds(): boolean[] {
  return store().panels.map((panel) => panel.folded);
}

/** The value half of a `Set-Cookie`-shaped string, so a test can hand it back to
 *  the reader without a browser in between. */
function cookieValue(assignment: string): string {
  return assignment.slice(assignment.indexOf("=") + 1, assignment.indexOf(";"));
}

describe("folding a panel", () => {
  it("folds and unfolds without touching what the panel holds or what has focus", () => {
    store().setActiveTarget(A);
    store().openPanel(B);
    const [first, second] = store().panels;
    if (first === undefined || second === undefined) {
      throw new Error("expected two panels");
    }

    store().toggleFold(first.id);

    expect(folds()).toEqual([true, false]);
    // A fold hides a panel; it does not empty it and it does not move focus.
    // Both matter: the target is what makes unfolding worth anything, and focus
    // is what decides where the next single click in the tree lands.
    expect(shown()).toEqual([A, B]);
    expect(store().activeId).toBe(second.id);

    store().toggleFold(first.id);
    expect(folds()).toEqual([false, false]);
  });

  it("folds the only panel, which closing it refuses to do", () => {
    store().setActiveTarget(A);
    const only = store().panels[0];
    if (only === undefined) {
      throw new Error("expected one panel");
    }

    store().closePanel(only.id);
    expect(store().panels).toHaveLength(1);

    store().toggleFold(only.id);

    // The asymmetry is the point. Closing the last panel is refused because
    // there is no way back to having one; folding it is allowed because the
    // control that undoes it is sitting exactly where the panel was.
    expect(folds()).toEqual([true]);
  });

  it("ignores an id that names no panel", () => {
    store().setActiveTarget(A);
    store().toggleFold("panel-nope");
    expect(folds()).toEqual([false]);
  });

  it("unfolds a panel it is given something to show", () => {
    // The defect this rule exists to refuse, and the one shape this whole epic
    // is about: keeper reads the file, loads it into the panel, and shows the
    // reader nothing at all.
    store().setActiveTarget(A);
    const only = store().panels[0];
    if (only === undefined) {
      throw new Error("expected one panel");
    }
    store().toggleFold(only.id);

    store().setActiveTarget(B);

    expect(shown()).toEqual([B]);
    expect(folds()).toEqual([false]);
  });

  it("unfolds a folded panel that already holds the target being opened", () => {
    store().setActiveTarget(A);
    store().openPanel(B);
    const [first] = store().panels;
    if (first === undefined) {
      throw new Error("expected two panels");
    }
    store().toggleFold(first.id);

    // A double click on A, which is already open in the folded panel: focus it —
    // and a focus that answered a gesture asking to SEE something has to show it.
    store().openPanel(A);

    expect(store().activeId).toBe(first.id);
    expect(folds()).toEqual([false, false]);
  });

  it("leaves a fold alone when focus moves for its own sake", () => {
    store().setActiveTarget(A);
    store().openPanel(B);
    const [first] = store().panels;
    if (first === undefined) {
      throw new Error("expected two panels");
    }
    store().toggleFold(first.id);

    store().focusPanel(first.id);

    // Clicking a folded panel's header focuses it. That is not a request to see
    // anything — the reader is about to press Unfold, or to close it — so the
    // fold stands. Only a target ARRIVING unfolds a panel.
    expect(store().activeId).toBe(first.id);
    expect(folds()).toEqual([true, false]);
  });
});

describe("a fold that survives a restart", () => {
  it("round-trips which panel was folded, and only that one", () => {
    store().setActiveTarget(A);
    store().openPanel(B);
    store().openPanel(C);
    const middle = store().panels[1];
    if (middle === undefined) {
      throw new Error("expected three panels");
    }
    store().toggleFold(middle.id);

    resetPanelsStoreForTest();
    hydratePanels(document.cookie);

    expect(shown()).toEqual([A, B, C]);
    expect(folds()).toEqual([false, true, false]);
  });

  it("counts the fold over the targets that survived the way back in", () => {
    // The cookie's own array and the restored list are not the same list: an
    // entry this build cannot read is dropped, and everything after it shifts
    // left. A fold index that still pointed into the original array would come
    // back folding the panel next door.
    const cookie = `${PANELS_COOKIE}=${encodeURIComponent(
      JSON.stringify({ v: 2, a: 0, t: [{ kind: "hologram", id: "x" }, A, B], f: [2] }),
    )}`;

    const read = readPanelTargets(cookie);

    expect(read.targets).toEqual([A, B]);
    expect(read.folded).toEqual([1]);
  });

  it("restores a cookie written before folding existed, rather than discarding it", () => {
    // The ruling in `PANELS_VERSION`. The discard rule exists because a target's
    // MEANING may change between versions; `f` only adds a field whose absence
    // has one safe reading, so applying the discard rule here would cost every
    // existing reader their whole workspace on the first launch after an update,
    // in exchange for nothing.
    const cookie = `${PANELS_COOKIE}=${encodeURIComponent(
      JSON.stringify({ v: 1, a: 1, t: [A, B] }),
    )}`;

    hydratePanels(cookie);

    expect(shown()).toEqual([A, B]);
    expect(folds()).toEqual([false, false]);
    expect(activePanel(store()).target).toEqual(B);
  });

  it("ignores a fold list a hand-edited cookie could produce", () => {
    // The cookie is a string a person can edit. Anything that is not a whole
    // number in range folds nothing rather than folding something arbitrary:
    // `NaN` in the set would look like it folded a panel and fold none.
    const cookie = `${PANELS_COOKIE}=${encodeURIComponent(
      JSON.stringify({ v: 2, a: 0, t: [A, B], f: ["1", 1.5, -1, 9, null] }),
    )}`;

    expect(readPanelTargets(cookie).folded).toEqual([]);
  });

  it("refuses to read a fold out of a version that had none", () => {
    // A `v: 1` payload carrying an `f` is not a keeper cookie; it is a cookie
    // somebody edited. Reading the field anyway would make the version number
    // decorative.
    const cookie = `${PANELS_COOKIE}=${encodeURIComponent(
      JSON.stringify({ v: 1, a: 0, t: [A, B], f: [0] }),
    )}`;

    expect(readPanelTargets(cookie).folded).toEqual([]);
  });

  it("drops a fold with the panel it belonged to when the budget bites", () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    const long = "x".repeat(400);
    // Every panel folded, so whichever ones survive the trim must all come back
    // folded and none of the indices may point past the end of the list.
    const panels = Array.from({ length: 20 }, (_, index) => ({
      id: `p${index}`,
      target: { kind: "file", profileId: "p1", relativePath: `${long}/${index}.md` } as const,
      replaced: null,
      folded: true,
    }));

    const value = cookieValue(panelsCookie(panels, "p0"));
    const read = readPanelTargets(`${PANELS_COOKIE}=${value}`);

    expect(value.length).toBeLessThanOrEqual(PANELS_COOKIE_BUDGET);
    expect(read.targets.length).toBeGreaterThan(0);
    expect(read.folded).toEqual(read.targets.map((_, at) => at));
    info.mockRestore();
  });

  it("does not spend a byte on a folded panel that holds nothing", () => {
    // An empty panel is not persisted at all, so its fold cannot be either — and
    // the fold indices of the panels that ARE persisted must be counted after it
    // is gone, not before.
    //
    // The middle panel is UNFOLDED on purpose. With every remaining panel folded,
    // an implementation that counted before the drop would write one index too
    // many and the reader would clamp it away, so the bug would be invisible.
    // With a gap, counting too early folds the WRONG document: `A` comes back
    // folded and `B` does not.
    const value = cookieValue(
      panelsCookie(
        [
          { id: "p0", target: null, replaced: null, folded: true },
          { id: "p1", target: A, replaced: null, folded: false },
          { id: "p2", target: B, replaced: null, folded: true },
        ],
        "p1",
      ),
    );

    const read = readPanelTargets(`${PANELS_COOKIE}=${value}`);
    expect(read.targets).toEqual([A, B]);
    expect(read.folded).toEqual([1]);
  });
});
