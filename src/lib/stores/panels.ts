/**
 * The panel list: what keeper is showing you, and where (Story 45.1, FR-173,
 * AD-90, UX-DR65).
 *
 * # This is a new model, not the old one made addressable
 *
 * Worth saying plainly, because the two are easy to confuse and this epic has
 * confused them before. {@link "@/lib/stores/primary-view"} chooses which
 * *surface* is on screen — Inbox, Files, Notes, Settings — and it is a single
 * enum with no room for a thing in it. The Notes pane then kept its own
 * one-note cursor, the Files pane kept none at all, and the recordings browser
 * a third. None of that is a panel model; it is three surfaces each holding one
 * slot, and "open this beside that" cannot be expressed in any of them.
 *
 * A panel is a view of an addressable target, panels are a list, and the active
 * one is a member of that list. The old one-note cursor is the degenerate case
 * of this and has been deleted rather than left beside it — two places that both
 * know which note is open is exactly how they come to disagree.
 *
 * # Where the panels live
 *
 * The list is global and singular. Only one primary surface is mounted at a
 * time, so at most one host renders it, and switching from Files to Notes
 * changes the *browser* beside the panels rather than the panels themselves.
 * That is what makes "open the file this note came from" (Story 45.18) a
 * resolution rule and not a navigation system.
 *
 * # Persistence
 *
 * `document.cookie`, following {@link "@/lib/column-widths"} and for the same
 * reason: `localStorage` is refused across this codebase, and a panel list is a
 * lens the viewer arranged rather than a fact Rust has any use for. Only the
 * targets travel — an id is regenerated on load, and {@link Panel.replaced} is
 * deliberately transient.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { writeCookie } from "@/components/ui/cookie-writer";
import type { PanelTargetVm } from "@/lib/ipc/client";

/** The cookie the panel list is remembered in. One cookie for the whole list. */
export const PANELS_COOKIE = "keeper_panels";

/** A year, matching {@link "@/lib/column-widths"}: a workspace that resets on
 *  the user who opens keeper on Mondays is a workspace nobody arranges. */
export const PANELS_COOKIE_MAX_AGE = 60 * 60 * 24 * 365;

/**
 * How many encoded bytes of cookie the panel list may occupy.
 *
 * Browsers drop a cookie over roughly 4096 bytes *silently* — the assignment
 * succeeds, the value is not stored, and the next restart comes up with no
 * panels and no explanation. So the budget is enforced here, where the overflow
 * can be reported, and it is set below the limit to leave room for the name, the
 * attributes and the other cookies this app writes.
 */
export const PANELS_COOKIE_BUDGET = 3500;

/**
 * How many `note` panels may exist at once, and why it is one.
 *
 * The note document mirror ({@link "@/lib/stores/notes-editor"}) is a module
 * singleton holding one buffer, one base and one `notes_open` subscription
 * (AD-58). Two mounted `NoteEditor`s would therefore write each other's text:
 * the second to mount would take the store, and the first would show the
 * second's document under the first's title. That is a data-loss bug, not a
 * cosmetic one.
 *
 * So the model refuses the second note panel rather than the surface refusing to
 * render it: {@link PanelsState.openPanel} retargets the note panel that exists
 * instead of appending a twin. Lifting this is a real piece of work — the mirror
 * has to become per-document — and Story 45.15 (several capture windows, each
 * holding its own note) needs the same lift, so it is one job and not two.
 *
 * Everything else is unlimited: a file panel beside a note panel beside another
 * file panel is the arrangement this epic exists to make possible.
 */
export const NOTE_PANEL_LIMIT = 1;

/** One panel. */
export interface Panel {
  /** Stable for as long as the panel exists; regenerated across a restart,
   *  because nothing outside this store may hold one across one. */
  readonly id: string;
  /** What it shows, or `null` for the one panel a fresh keeper starts with. */
  readonly target: PanelTargetVm | null;
  /**
   * What this panel showed before the single click that set {@link target} —
   * `null` when {@link target} was not set by a single click.
   *
   * This is what makes a double click mean what it says. A double click on a row
   * is preceded by a real single click, so without this the sequence would be
   * "replace the active panel with B, then open B beside itself" and the user
   * would get two panels of B and lose A. With it, {@link PanelsState.openPanel}
   * puts A back where it was and opens B beside it, which is what the gesture
   * looked like. A timer that swallowed the first click would do the same job
   * and would make every test of it a race.
   *
   * `was: null` is the third state and it is not the same as `replaced: null`:
   * the panel WAS previewing, and what it displaced was nothing. Pinning then
   * keeps the target where it is instead of appending, because putting
   * "nothing" back beside it would leave an empty frame the user did not ask
   * for. A run of previews keeps the first `was`, so previewing three files in
   * a row and pinning the third still puts the original document back.
   */
  readonly replaced: { readonly was: PanelTargetVm | null } | null;
}

export interface PanelsState {
  /** Every panel, left to right. Never empty (see {@link closePanel}). */
  readonly panels: readonly Panel[];
  /** The focused panel's id. Always names a member of {@link panels}. */
  readonly activeId: string;
  /**
   * Single click: the active panel now shows this. The list does not grow —
   * that is the whole difference between the two gestures.
   */
  setActiveTarget: (target: PanelTargetVm) => void;
  /**
   * Double click: open this beside what is already open, and focus it.
   *
   * Three cases it deliberately does not append in, each because appending
   * would be a worse answer than the alternative:
   * - a panel already holds this exact target: focus it. Two identical panels
   *   are two views that can never differ.
   * - the target is a note and a note panel exists: retarget that one
   *   ({@link NOTE_PANEL_LIMIT}).
   * - the active panel is showing nothing: fill it rather than leave an empty
   *   frame sitting beside the thing that was just opened.
   */
  openPanel: (target: PanelTargetVm) => void;
  /** Focus a panel without changing what anything shows. */
  focusPanel: (id: string) => void;
  /**
   * Close one panel.
   *
   * Refuses the last one: a workspace with no panels has no way back to having
   * one, and every surface that renders panels would have to grow an empty state
   * that exists only for that. Closing the focused panel moves focus to the
   * panel that slides into its place, or to the one on its left when it was the
   * rightmost.
   */
  closePanel: (id: string) => void;
  /**
   * Stop showing this target anywhere.
   *
   * For the one case that is not "the target no longer resolves": the thing was
   * deliberately deleted, so a panel explaining that it cannot be found would be
   * keeper reporting the user's own action back to them as a fault. Every panel
   * holding it closes; the last panel cannot close, so it is emptied instead and
   * shows the same sentence a fresh keeper shows.
   */
  closeTarget: (target: PanelTargetVm) => void;
}

/** Monotonic, so no two panels in one session share an id even after a close. */
let nextPanelId = 1;

function makePanel(target: PanelTargetVm | null): Panel {
  const panel: Panel = { id: `panel-${nextPanelId}`, target, replaced: null };
  nextPanelId += 1;
  return panel;
}

/**
 * Whether two targets name the same thing.
 *
 * Field-by-field on the tag first, rather than comparing serialised JSON: two
 * objects with the same fields in a different key order are the same target, and
 * a JSON comparison would call them different and open a second panel.
 */
export function sameTarget(a: PanelTargetVm | null, b: PanelTargetVm | null): boolean {
  if (a === null || b === null) {
    return a === b;
  }
  if (a.kind !== b.kind) {
    return false;
  }
  switch (a.kind) {
    case "note":
      return b.kind === "note" && a.vaultId === b.vaultId && a.noteId === b.noteId;
    case "file":
      return b.kind === "file" && a.profileId === b.profileId && a.relativePath === b.relativePath;
    case "recording":
      return b.kind === "recording" && a.sessionId === b.sessionId;
  }
}

/**
 * Whether a target is one this app is willing to restore.
 *
 * Every restored target has been round-tripped through a cookie, which is a
 * string a user can edit, so this is the boundary where a target stops being
 * trusted. A `file` target whose path is absolute or climbs out of its profile
 * is refused here rather than handed to a listing call: AD-65 says no frontend
 * joins a root and a subpath, and the corollary is that a path arriving from
 * outside the app has to be a relative one before it is used as one.
 *
 * Rust re-derives and contains the path again before reading anything (AD-59),
 * so this is the outer of two gates rather than the only one.
 */
export function isRestorableTarget(target: PanelTargetVm): boolean {
  switch (target.kind) {
    case "note":
      return target.vaultId !== "" && target.noteId !== "";
    case "file":
      return (
        target.profileId !== "" &&
        target.relativePath !== "" &&
        !target.relativePath.startsWith("/") &&
        // A Windows drive letter or a UNC path is absolute too, and neither
        // starts with `/`.
        !/^[a-zA-Z]:[\\/]/.test(target.relativePath) &&
        !target.relativePath.startsWith("\\") &&
        !target.relativePath.split("/").includes("..")
      );
    case "recording":
      return target.sessionId !== "";
  }
}

/** The state a keeper that has never opened anything starts in. */
function initialPanels(): { panels: Panel[]; activeId: string } {
  const first = makePanel(null);
  return { panels: [first], activeId: first.id };
}

/**
 * The persisted form: the targets, and which one had focus.
 *
 * Versioned because the vocabulary is generated from Rust and will gain a
 * variant; an unrecognised version is discarded rather than guessed at, which
 * costs the user their arrangement once and never shows them a panel pointing at
 * something that no longer means what it meant.
 */
interface PersistedPanels {
  readonly v: 1;
  readonly a: number;
  readonly t: readonly PanelTargetVm[];
}

/** Structural guard over whatever the cookie actually held. */
function isPersisted(value: unknown): value is PersistedPanels {
  return (
    typeof value === "object" &&
    value !== null &&
    "v" in value &&
    value.v === 1 &&
    "a" in value &&
    typeof value.a === "number" &&
    "t" in value &&
    Array.isArray(value.t)
  );
}

/** Whether a decoded entry is a target this build understands. */
function isTarget(value: unknown): value is PanelTargetVm {
  if (typeof value !== "object" || value === null || !("kind" in value)) {
    return false;
  }
  switch (value.kind) {
    case "note":
      return (
        "vaultId" in value &&
        typeof value.vaultId === "string" &&
        "noteId" in value &&
        typeof value.noteId === "string"
      );
    case "file":
      return (
        "profileId" in value &&
        typeof value.profileId === "string" &&
        "relativePath" in value &&
        typeof value.relativePath === "string"
      );
    case "recording":
      return "sessionId" in value && typeof value.sessionId === "string";
    default:
      return false;
  }
}

/**
 * Read a remembered panel list out of a `document.cookie` string.
 *
 * Pure and total: anything it cannot make sense of comes back as an empty list,
 * which the store renders as one empty panel. A malformed cookie is a lost
 * arrangement, never a thrown error at boot — this runs before anything is on
 * screen, so a throw here is a white window.
 */
export function readPanelTargets(cookie: string): {
  targets: PanelTargetVm[];
  activeIndex: number;
} {
  const empty = { targets: [] as PanelTargetVm[], activeIndex: 0 };
  for (const part of cookie.split(";")) {
    const trimmed = part.trim();
    if (!trimmed.startsWith(`${PANELS_COOKIE}=`)) {
      continue;
    }
    const raw = trimmed.slice(PANELS_COOKIE.length + 1);
    let decoded: unknown;
    try {
      decoded = JSON.parse(decodeURIComponent(raw));
    } catch {
      return empty;
    }
    if (!isPersisted(decoded)) {
      return empty;
    }
    const targets = decoded.t.filter(isTarget).filter(isRestorableTarget);
    if (targets.length === 0) {
      return empty;
    }
    const activeIndex = Math.min(Math.max(Math.trunc(decoded.a), 0), targets.length - 1);
    return { targets, activeIndex };
  }
  return empty;
}

/**
 * The `document.cookie` assignment that records this arrangement.
 *
 * Takes the panels rather than the store so it is assertable without one, the
 * shape {@link "@/lib/column-widths"} established. A panel showing nothing is
 * not persisted — restoring an empty frame is indistinguishable from restoring
 * nothing, and the store makes an empty frame for free.
 */
export function panelsCookie(panels: readonly Panel[], activeId: string): string {
  const targets = panels
    .map((panel) => panel.target)
    .filter((target): target is PanelTargetVm => target !== null);
  if (targets.length === 0) {
    // Forget the arrangement rather than store an empty one, so a user who
    // closed everything comes back to a clean start instead of to a cookie that
    // decodes to nothing.
    return `${PANELS_COOKIE}=; path=/; max-age=0; samesite=lax`;
  }
  const activeTarget = panels.find((panel) => panel.id === activeId)?.target ?? null;
  const activeIndex = Math.max(
    0,
    targets.findIndex((target) => sameTarget(target, activeTarget)),
  );
  let kept = targets;
  let value = encodeURIComponent(JSON.stringify({ v: 1, a: activeIndex, t: kept }));
  while (value.length > PANELS_COOKIE_BUDGET && kept.length > 1) {
    // Drop from the right — the panels furthest from the one in focus — and say
    // so. A browser silently discarding the whole cookie would lose all of them.
    kept = kept.slice(0, -1);
    const clamped = Math.min(activeIndex, kept.length - 1);
    value = encodeURIComponent(JSON.stringify({ v: 1, a: clamped, t: kept }));
  }
  if (kept.length < targets.length) {
    console.info(
      `keeper: remembering ${kept.length} of ${targets.length} panels — the rest do not fit in a cookie.`,
    );
  }
  return `${PANELS_COOKIE}=${value}; path=/; max-age=${PANELS_COOKIE_MAX_AGE}; samesite=lax`;
}

/** Write the arrangement out. Best effort: a document that refuses cookies
 *  costs the user the restore, and must not cost them the click. */
function persist(panels: readonly Panel[], activeId: string): void {
  if (typeof document === "undefined") {
    return;
  }
  try {
    writeCookie(panelsCookie(panels, activeId));
  } catch {
    // Nothing to say and nothing to retry: the panels are on screen either way.
  }
}

/** Replace one panel in a list, by id. */
function withPanel(panels: readonly Panel[], id: string, next: (panel: Panel) => Panel): Panel[] {
  return panels.map((panel) => (panel.id === id ? next(panel) : panel));
}

export const panelsStore = createStore<PanelsState>()((set, get) => ({
  ...initialPanels(),

  setActiveTarget: (target) => {
    const { panels, activeId } = get();
    const active = panels.find((panel) => panel.id === activeId);
    if (active === undefined || sameTarget(active.target, target)) {
      // Already showing it. Re-setting would reset `replaced` and turn the
      // second click of a double click into a lost preview.
      return;
    }
    const next = withPanel(panels, activeId, (panel) => ({
      ...panel,
      target,
      // The first preview in a run records what the panel really held; the ones
      // after it keep pointing at that, so pinning the fourth preview still puts
      // the original document back.
      replaced: panel.replaced ?? { was: panel.target },
    }));
    set({ panels: next });
    persist(next, activeId);
  },

  openPanel: (target) => {
    const { panels, activeId } = get();

    const existing = panels.find((panel) => sameTarget(panel.target, target));
    if (existing !== undefined && existing.id !== activeId) {
      set({ activeId: existing.id, panels: withPanel(panels, existing.id, clearPreview) });
      persist(panels, existing.id);
      return;
    }

    if (target.kind === "note") {
      const notePanels = panels.filter((panel) => panel.target?.kind === "note");
      if (notePanels.length >= NOTE_PANEL_LIMIT) {
        const host = notePanels[0];
        if (host === undefined) {
          return;
        }
        const next = withPanel(panels, host.id, (panel) => ({
          ...panel,
          target,
          replaced: null,
        }));
        set({ panels: next, activeId: host.id });
        persist(next, host.id);
        return;
      }
    }

    const active = panels.find((panel) => panel.id === activeId);
    if (active !== undefined && sameTarget(active.target, target)) {
      if (active.replaced === null || active.replaced.was === null) {
        // Either no click preceded this and the panel genuinely holds the
        // target, or the click landed in a panel that was showing nothing.
        // Both mean the same thing: this target belongs HERE, and appending
        // would open it beside a frame that is either its own duplicate or
        // empty. Pinning it is the whole of the answer.
        const pinned = withPanel(panels, activeId, clearPreview);
        set({ panels: pinned });
        persist(pinned, activeId);
        return;
      }
      // The single click that opened this gesture displaced a real document.
      // Put it back and open the target beside it, which is what the double
      // click looked like.
      const restored = withPanel(panels, activeId, (panel) => ({
        ...panel,
        target: active.replaced === null ? panel.target : active.replaced.was,
        replaced: null,
      }));
      appendBeside(set, restored, activeId, target);
      return;
    }

    if (active !== undefined && active.target === null) {
      const next = withPanel(panels, activeId, (panel) => ({
        ...panel,
        target,
        replaced: null,
      }));
      set({ panels: next });
      persist(next, activeId);
      return;
    }

    appendBeside(set, panels, activeId, target);
  },

  focusPanel: (id) => {
    const { panels, activeId } = get();
    if (id === activeId || !panels.some((panel) => panel.id === id)) {
      return;
    }
    const next = withPanel(panels, id, clearPreview);
    set({ activeId: id, panels: next });
    persist(next, id);
  },

  closePanel: (id) => {
    const { panels, activeId } = get();
    if (panels.length <= 1) {
      return;
    }
    const index = panels.findIndex((panel) => panel.id === id);
    if (index === -1) {
      return;
    }
    const next = panels.filter((panel) => panel.id !== id);
    // The panel that slides into the closed one's place takes focus; at the
    // right-hand end there is none, so the neighbour on the left does.
    const neighbour = next[Math.min(index, next.length - 1)];
    const nextActiveId = id === activeId && neighbour !== undefined ? neighbour.id : activeId;
    set({ panels: next, activeId: nextActiveId });
    persist(next, nextActiveId);
  },

  closeTarget: (target) => {
    const { panels, activeId } = get();
    const kept = panels.filter((panel) => !sameTarget(panel.target, target));
    if (kept.length === panels.length) {
      return;
    }
    // The list may not empty. Blanking the survivor rather than refusing to act
    // is what keeps the deleted thing off the screen: a refusal here would leave
    // the user staring at the note they just threw away.
    const next =
      kept.length === 0
        ? [makePanel(null)]
        : kept.map((panel) => (panel.replaced === null ? panel : { ...panel, replaced: null }));
    // Focus follows the same rule as closing one panel by hand: the panel that
    // slides into the first closed one's place, or the one on its left.
    const stillActive = next.some((panel) => panel.id === activeId);
    const closedAt = panels.findIndex((panel) => sameTarget(panel.target, target));
    const neighbour = next[Math.min(closedAt, next.length - 1)];
    const nextActiveId = stillActive ? activeId : (neighbour?.id ?? activeId);
    set({ panels: next, activeId: nextActiveId });
    persist(next, nextActiveId);
  },
}));

/** A panel that is focused deliberately is no longer a preview of anything. */
function clearPreview(panel: Panel): Panel {
  return panel.replaced === null ? panel : { ...panel, replaced: null };
}

/** Insert a new panel immediately after the active one and focus it. */
function appendBeside(
  set: (partial: Partial<PanelsState>) => void,
  panels: readonly Panel[],
  activeId: string,
  target: PanelTargetVm,
): void {
  const created = makePanel(target);
  const at = panels.findIndex((panel) => panel.id === activeId);
  const next = [...panels];
  next.splice(at === -1 ? next.length : at + 1, 0, created);
  set({ panels: next, activeId: created.id });
  persist(next, created.id);
}

/** Whether {@link hydratePanels} has already run in this document. */
let hydrated = false;

/**
 * Restore the remembered arrangement.
 *
 * Called once from the shell rather than at module load, for two reasons that
 * both cost this codebase a defect before: an effect the shell does not mount is
 * an effect that never runs and no hook-level test can see (DW-172), and a store
 * that read `document.cookie` at import time would be a store no test could give
 * a cookie to without resetting the module registry.
 *
 * Idempotent, so React's double-invoked effects in development restore once.
 */
export function hydratePanels(cookie: string): void {
  if (hydrated) {
    return;
  }
  hydrated = true;
  const { targets, activeIndex } = readPanelTargets(cookie);
  if (targets.length === 0) {
    return;
  }
  const panels = targets.map((target) => makePanel(target));
  const active = panels[activeIndex] ?? panels[0];
  if (active === undefined) {
    return;
  }
  panelsStore.setState({ panels, activeId: active.id });
}

/** The focused panel. Never `undefined` while the store's invariant holds; the
 *  fallback is the first panel, so a caller never has to branch on impossible. */
export function activePanel(state: PanelsState): Panel {
  return state.panels.find((panel) => panel.id === state.activeId) ?? state.panels[0];
}

/** React selector hook over {@link panelsStore}. */
export function usePanelsStore<T>(selector: (state: PanelsState) => T): T {
  return useStore(panelsStore, selector);
}

/** Test-only reset: one empty panel, unhydrated, no cookie written. */
export function resetPanelsStoreForTest(): void {
  hydrated = false;
  panelsStore.setState(initialPanels());
}
