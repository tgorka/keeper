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

  it("retargets the one note panel instead of opening a second", () => {
    // Not a policy about tidiness: the note document mirror is a module
    // singleton (AD-58), so a second mounted editor would take the store and the
    // first would show the second's text under the first's title.
    store().setActiveTarget(A);
    store().openPanel(NOTE_ONE);
    store().openPanel(NOTE_TWO);

    expect(shown()).toEqual([A, NOTE_TWO]);
    expect(store().panels.filter((panel) => panel.target?.kind === "note")).toHaveLength(1);
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
    const cookie = panelsCookie([{ id: "p", target: A, replaced: null }], "p");
    resetPanelsStoreForTest();
    hydratePanels(cookie);

    expect(shown()).toEqual([A]);
  });

  it("hydrates once, so a double-invoked effect does not re-restore over a click", () => {
    const cookie = panelsCookie([{ id: "p", target: A, replaced: null }], "p");
    resetPanelsStoreForTest();
    hydratePanels(cookie);
    store().setActiveTarget(B);

    hydratePanels(cookie);

    expect(shown()).toEqual([B]);
  });

  it("comes up clean from a corrupt cookie instead of throwing at boot", () => {
    expect(readPanelTargets(`${PANELS_COOKIE}=not-json`)).toEqual({ targets: [], activeIndex: 0 });
    expect(
      readPanelTargets(`${PANELS_COOKIE}=${encodeURIComponent('{"v":9,"a":0,"t":[]}')}`),
    ).toEqual({ targets: [], activeIndex: 0 });
    expect(readPanelTargets("")).toEqual({ targets: [], activeIndex: 0 });
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
    expect(panelsCookie([{ id: "p", target: null, replaced: null }], "p")).toContain("max-age=0");
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
