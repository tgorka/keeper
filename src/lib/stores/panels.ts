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
 * lens the viewer arranged rather than a fact Rust has any use for. What travels
 * is what the viewer arranged and nothing derived: the targets, which one had
 * focus, and — since Story 46.13 — which of them are folded. An id is
 * regenerated on load, and {@link Panel.replaced} is deliberately transient
 * because it is the state of a gesture rather than of an arrangement.
 *
 * The cookie is versioned, and {@link PANELS_VERSION} carries the one ruling in
 * this module that could not be derived: what a cookie written before folding
 * existed restores as.
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
 * # A note is an ordinary target (Story 46.12)
 *
 * Until 46.12 this module exported `NOTE_PANEL_LIMIT = 1` and `openPanel`
 * retargeted the one note panel instead of appending a twin. It was not
 * tidiness. The note document mirror ({@link "@/lib/stores/notes-editor"}) was
 * a module singleton holding one buffer, one base and one `notes_open`
 * subscription (AD-58), so two mounted `NoteEditor`s would have written each
 * other's text: the second to mount took the store, and the first showed the
 * second's document under the first's title while its autosave wrote the
 * second's body into the first's file. Data loss, not a cosmetic bug — and the
 * model refused it rather than a surface declining to draw it, so no surface
 * could reintroduce it by mounting a second editor of its own.
 *
 * The owner asked for several notes at once and 46.12 did the lift the constant
 * was waiting on: the mirror is keyed by note, reference counted, one channel
 * per note however many views. There is nothing left for a limit to protect, so
 * the limit is gone rather than raised — a note panel is now exactly as
 * unremarkable as a file panel, and `openPanel` has one fewer branch.
 */

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
  /**
   * Whether this panel is folded away: its header only, no body, and no share
   * of the strip's width (Story 46.13, FR-217).
   *
   * A display state, and the first one this model has held — every other field
   * says what the panel IS rather than how much of it is drawn. It is here
   * rather than in `PanelStrip`'s component state for the reason 46.3 moved the
   * Files tree's expansion out of `useState`: the lifetime of a lens the reader
   * arranged is not the lifetime of the component that happens to render it,
   * and `AppShell` unmounts the strip's host whenever the primary view changes.
   *
   * Folding is not closing, and the difference is what makes it worth having.
   * A close is destructive — the target is gone and the last panel refuses to
   * do it at all — where a fold keeps the panel, its place in the order and its
   * target, and gives its width to its neighbours. So the fold is allowed on
   * every panel including the only one: the control that undoes it is sitting
   * in the strip where the panel was, which is exactly what closing the last
   * panel could not offer.
   *
   * **A panel that is given something to show unfolds.** That rule lives in
   * {@link PanelsState.setActiveTarget} and {@link PanelsState.openPanel}
   * rather than here, and it is the whole reason this field is safe: without
   * it, clicking a file in the tree would load it into a panel the reader
   * cannot see, which is the defect shape this epic exists to remove — keeper
   * does the thing and then fails to show you that it did.
   */
  readonly folded: boolean;
}

export interface PanelsState {
  /** Every panel, left to right. Never empty (see {@link closePanel}). */
  readonly panels: readonly Panel[];
  /** The focused panel's id. Always names a member of {@link panels}. */
  readonly activeId: string;
  /**
   * Single click: the active panel now shows this. The list does not grow —
   * that is the whole difference between the two gestures.
   *
   * Unfolds the panel it lands in: a target loaded into a folded panel is a
   * read that the reader was given no sight of, and this store is where that
   * rule belongs, not the four surfaces that call this.
   */
  setActiveTarget: (target: PanelTargetVm) => void;
  /**
   * Double click: open this beside what is already open, and focus it.
   *
   * Two cases it deliberately does not append in, each because appending
   * would be a worse answer than the alternative:
   * - a panel already holds this exact target: focus it. Two identical panels
   *   are two views that can never differ. Folded, it unfolds — the gesture
   *   asked to see the thing.
   * - the active panel is showing nothing: fill it rather than leave an empty
   *   frame sitting beside the thing that was just opened.
   *
   * A note target used to be a third case, retargeted rather than appended.
   * Story 46.12 removed it: see the note above {@link Panel}.
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
   * Fold this panel away, or unfold it.
   *
   * Allowed on every panel including the only one — see {@link Panel.folded}
   * for why that is not the same decision as {@link closePanel}'s refusal. Does
   * not change focus: folding the panel you are looking at leaves it the active
   * one, so the next single click in a browser still lands where the reader
   * pointed it, and lands visibly, because it unfolds on the way in.
   */
  toggleFold: (id: string) => void;
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

function makePanel(target: PanelTargetVm | null, folded = false): Panel {
  const panel: Panel = { id: `panel-${nextPanelId}`, target, replaced: null, folded };
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
 * The version this build writes. Bumped by Story 46.13, which added `f`.
 *
 * The reader accepts every version in {@link PANELS_READABLE_VERSIONS} and
 * nothing else. Deliberately two numbers rather than one, and the reasoning is
 * the interesting part of this module:
 *
 * **A `v: 1` cookie restores as a `v: 2` arrangement with nothing folded, and
 * that is a ruling rather than an accident of the discard rule.** The rule this
 * file shipped with — discard an unrecognised version — exists because the
 * target vocabulary is generated from Rust and a future version might make an
 * old `t` entry *mean* something different, and a panel pointing at something
 * that no longer means what it meant is worse than no panel. That reason does
 * not apply here. `v: 2` adds one field whose absence has an exact, safe reading
 * — nothing is folded, which is both the state `v: 1` shipped with and the state
 * every panel is reachable from — and applying the discard rule to it would cost
 * every existing reader their whole workspace on the first launch after an
 * update, in exchange for nothing. So `v: 1` is read, `f` is read only from
 * `v: 2`, and a target that was folded in a `v: 1` world is a target that never
 * was.
 *
 * The price is paid in the other direction and it is the price the discard rule
 * always charged: a build older than 46.13 reading a `v: 2` cookie discards it
 * and comes up clean. A downgrade costs one arrangement, once, which is why the
 * writer bumps rather than smuggling `f` into a `v: 1` payload — a cookie that
 * lies about its version to stay compatible is a cookie no future reader can
 * trust.
 */
const PANELS_VERSION = 2;

/** Every version this build can read. See {@link PANELS_VERSION}. */
const PANELS_READABLE_VERSIONS: readonly number[] = [1, PANELS_VERSION];

/**
 * The persisted form: the targets, which one had focus, and which are folded.
 *
 * `f` holds INDICES into `t` rather than a parallel array of booleans, because
 * folding is the rare state: an arrangement with nothing folded writes `[]`,
 * where a boolean per panel would spend a fifth of the byte budget saying
 * "false" four times. It is also why the reader can treat an absent `f` as
 * "nothing folded" without inventing a length.
 */
interface PersistedPanels {
  readonly v: number;
  readonly a: number;
  readonly t: readonly PanelTargetVm[];
  readonly f?: readonly number[];
}

/** Structural guard over whatever the cookie actually held. `f` is optional and
 *  is validated where it is used: a `v: 1` cookie has none, and a `v: 2` cookie
 *  someone hand-edited may have anything at all in it. */
function isPersisted(value: unknown): value is PersistedPanels {
  return (
    typeof value === "object" &&
    value !== null &&
    "v" in value &&
    typeof value.v === "number" &&
    PANELS_READABLE_VERSIONS.includes(value.v) &&
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
 *
 * `folded` comes back as indices into the RETURNED targets, not into the
 * cookie's own array. The two differ whenever an entry was dropped for being
 * unknown or unrestorable, and a fold index that still pointed into the original
 * array would silently fold the panel next door.
 */
export function readPanelTargets(cookie: string): {
  targets: PanelTargetVm[];
  activeIndex: number;
  folded: number[];
} {
  const empty = { targets: [] as PanelTargetVm[], activeIndex: 0, folded: [] as number[] };
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
    // A `v: 1` cookie predates folding, so it has no `f` and every panel comes
    // back unfolded — see {@link PANELS_VERSION}. Anything in an `f` that is not
    // a whole number is dropped rather than coerced: the cookie is a string a
    // person can edit, and `NaN` in a Set would fold nothing while looking like
    // it folded something.
    const wanted = new Set<number>(
      decoded.v === PANELS_VERSION && Array.isArray(decoded.f)
        ? decoded.f.filter((at): at is number => Number.isInteger(at))
        : [],
    );
    const targets: PanelTargetVm[] = [];
    const folded: number[] = [];
    for (const [at, entry] of decoded.t.entries()) {
      if (!isTarget(entry) || !isRestorableTarget(entry)) {
        continue;
      }
      if (wanted.has(at)) {
        folded.push(targets.length);
      }
      targets.push(entry);
    }
    if (targets.length === 0) {
      return empty;
    }
    const activeIndex = Math.min(Math.max(Math.trunc(decoded.a), 0), targets.length - 1);
    return { targets, activeIndex, folded };
  }
  return empty;
}

/**
 * The `document.cookie` assignment that records this arrangement.
 *
 * Takes the panels rather than the store so it is assertable without one, the
 * shape {@link "@/lib/column-widths"} established. A panel showing nothing is
 * not persisted — restoring an empty frame is indistinguishable from restoring
 * nothing, and the store makes an empty frame for free. A panel showing nothing
 * takes its fold with it for the same reason: there is no such thing as a folded
 * empty frame worth a byte.
 *
 * The fold indices are counted over the panels that survive that filter, and
 * they are re-counted after every trim, because both operations renumber the
 * list. This is the one place in the module where an off-by-one would fold the
 * wrong document rather than throw.
 */
export function panelsCookie(panels: readonly Panel[], activeId: string): string {
  // Narrowed rather than merely filtered: `encode` below then has nothing to
  // decide about an empty panel, which is what keeps the fold indices honest.
  // A guard inside the encoder would be unreachable code that no test could pin,
  // and unreachable code is where an off-by-one waits.
  const holding = panels.filter((panel): panel is HoldingPanel => panel.target !== null);
  if (holding.length === 0) {
    // Forget the arrangement rather than store an empty one, so a user who
    // closed everything comes back to a clean start instead of to a cookie that
    // decodes to nothing.
    return `${PANELS_COOKIE}=; path=/; max-age=0; samesite=lax`;
  }
  const activeTarget = panels.find((panel) => panel.id === activeId)?.target ?? null;
  const activeIndex = Math.max(
    0,
    holding.findIndex((panel) => sameTarget(panel.target, activeTarget)),
  );
  let kept = holding;
  let value = encode(kept, activeIndex);
  while (value.length > PANELS_COOKIE_BUDGET && kept.length > 1) {
    // Drop from the right — the panels furthest from the one in focus — and say
    // so. A browser silently discarding the whole cookie would lose all of them.
    kept = kept.slice(0, -1);
    value = encode(kept, Math.min(activeIndex, kept.length - 1));
  }
  if (kept.length < holding.length) {
    console.info(
      `keeper: remembering ${kept.length} of ${holding.length} panels — the rest do not fit in a cookie.`,
    );
  }
  return `${PANELS_COOKIE}=${value}; path=/; max-age=${PANELS_COOKIE_MAX_AGE}; samesite=lax`;
}

/** A panel that is actually showing something — the only kind that is worth a
 *  cookie, and therefore the only kind the encoder counts. */
type HoldingPanel = Panel & { readonly target: PanelTargetVm };

/** The encoded cookie value for exactly these panels, in this order, with this
 *  one in focus. Shared by the first attempt and every trim so the two cannot
 *  come to disagree about what `f` counts. */
function encode(kept: readonly HoldingPanel[], activeIndex: number): string {
  const folded: number[] = [];
  const targets: PanelTargetVm[] = [];
  for (const panel of kept) {
    if (panel.folded) {
      folded.push(targets.length);
    }
    targets.push(panel.target);
  }
  return encodeURIComponent(
    JSON.stringify({ v: PANELS_VERSION, a: activeIndex, t: targets, f: folded }),
  );
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
    const next = withPanel(panels, activeId, (panel) =>
      shown({
        ...panel,
        target,
        // The first preview in a run records what the panel really held; the
        // ones after it keep pointing at that, so pinning the fourth preview
        // still puts the original document back.
        replaced: panel.replaced ?? { was: panel.target },
      }),
    );
    set({ panels: next });
    persist(next, activeId);
  },

  openPanel: (target) => {
    const { panels, activeId } = get();

    const existing = panels.find((panel) => sameTarget(panel.target, target));
    if (existing !== undefined && existing.id !== activeId) {
      // Persisting the UPDATED list, not the one that came in: `replaced` is
      // transient and never reached the cookie, but a fold does, so a focus that
      // unfolds has to be written down or the next launch brings the fold back
      // over a panel the reader deliberately opened.
      const next = withPanel(panels, existing.id, (panel) => shown(clearPreview(panel)));
      set({ activeId: existing.id, panels: next });
      persist(next, existing.id);
      return;
    }

    const active = panels.find((panel) => panel.id === activeId);
    if (active !== undefined && sameTarget(active.target, target)) {
      if (active.replaced === null || active.replaced.was === null) {
        // Either no click preceded this and the panel genuinely holds the
        // target, or the click landed in a panel that was showing nothing.
        // Both mean the same thing: this target belongs HERE, and appending
        // would open it beside a frame that is either its own duplicate or
        // empty. Pinning it is the whole of the answer.
        const pinned = withPanel(panels, activeId, (panel) => shown(clearPreview(panel)));
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
      const next = withPanel(panels, activeId, (panel) =>
        shown({
          ...panel,
          target,
          replaced: null,
        }),
      );
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

  toggleFold: (id) => {
    const { panels, activeId } = get();
    if (!panels.some((panel) => panel.id === id)) {
      return;
    }
    const next = withPanel(panels, id, (panel) => ({ ...panel, folded: !panel.folded }));
    // Focus is untouched, deliberately: folding the panel you are looking at
    // does not hand the next single click to a panel you were not pointing at.
    // It comes back unfolded, because a target arriving in a folded panel is
    // {@link shown}'s business.
    set({ panels: next });
    persist(next, activeId);
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

/**
 * A panel that has just been given something to show.
 *
 * Every site that sets a panel's target goes through this, which is the only
 * reason {@link Panel.folded} is safe to have: a load into a folded panel is a
 * read the reader was shown nothing of, and the epic this field arrived in
 * exists because keeper kept doing the thing and failing to show it. Folding is
 * therefore a state a *gesture on the panel itself* puts it in, and any other
 * gesture takes it out.
 */
function shown(panel: Panel): Panel {
  return panel.folded ? { ...panel, folded: false } : panel;
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
  const { targets, activeIndex, folded } = readPanelTargets(cookie);
  if (targets.length === 0) {
    return;
  }
  // A fold survives a restart, which is the point of putting it in the model
  // rather than in the strip's component state. The focused panel is NOT
  // unfolded on the way in: the reader folded it deliberately and focus is not a
  // request to see anything (see `shown`).
  const wasFolded = new Set(folded);
  const panels = targets.map((target, at) => makePanel(target, wasFolded.has(at)));
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
