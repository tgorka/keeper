/**
 * Whether the menu is folded, and which of its submenus are (Story 45.20,
 * FR-198, UX-DR81, UX-DR82).
 *
 * **There was no fold before this, and there were two things that looked like
 * one.** `useShellLayout` collapses the drawer to a 48px rail below 1080px —
 * a viewport rule with no user in it — and `src/components/ui/sidebar.tsx`
 * carried shadcn's whole collapsible sidebar, a `sidebar_state` cookie and a
 * ⌘B binding, and was imported by nothing. That second one was worse than
 * absent: `column-widths.ts` justified its own cookie by saying it followed
 * "the cookie `SidebarProvider` writes `sidebar_state` to", and `SidebarProvider`
 * has never been rendered, so the sentence was false the day it was written.
 * The dead component is deleted and this is the one fold mechanism.
 *
 * **The cookie, following {@link "@/lib/column-widths"}** — same shape, same
 * year-long life, same tolerance for a jar full of other people's cookies. Not
 * `localStorage`, which this codebase refuses out loud (`iosSyncDisclosureShownGet`
 * says so), and not an IPC command, because which submenus a person has folded
 * is a lens they chose and not a fact Rust has any use for.
 *
 * **Two folds, not one.** The menu folds to the rail; each submenu folds shut on
 * its own. They are independent because they answer different questions — "I
 * want the width back" and "I do not care about Networks today" — and a single
 * flag would make the rail forget which groups you had shut.
 *
 * Everything that parses or renders a cookie is pure and takes the string, so
 * the round trip is assertable without a document. That is deliberately NOT a
 * test of the restore: the restore is `hydrateSidebarFold`, mounted once in
 * `AppShell`, and it is exercised there — a hook-level test can never see that
 * the shell does not call it (DW-172).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/** The cookie the whole fold state lives in. One cookie, not one per group. */
export const SIDEBAR_FOLD_COOKIE = "keeper_sidebar_fold";

/** A year, matching the panel list and the column widths. A menu that unfolds
 *  itself every Monday is a menu nobody bothers to fold. */
export const SIDEBAR_FOLD_MAX_AGE = 60 * 60 * 24 * 365;

/**
 * The submenus that fold on their own.
 *
 * A closed set rather than an open map: an id is written into somebody's cookie
 * and read back by a later build, so a typo must be droppable and a group that
 * no longer exists must not leave a key nothing can clear.
 */
export const SIDEBAR_GROUPS = ["spaces", "networks"] as const;

export type SidebarGroup = (typeof SIDEBAR_GROUPS)[number];

/** What is folded right now. */
export interface SidebarFold {
  /** The whole menu is folded to the icon rail. */
  menu: boolean;
  /** Per submenu: `true` when that group's rows are folded away. */
  groups: Record<SidebarGroup, boolean>;
}

/** Nothing folded: the state a keeper that has never been folded starts in. */
export function unfolded(): SidebarFold {
  return { menu: false, groups: { spaces: false, networks: false } };
}

/** Whether `value` names a submenu this build knows. */
function isGroup(value: string): value is SidebarGroup {
  return (SIDEBAR_GROUPS as readonly string[]).includes(value);
}

/**
 * The fold state remembered in a `document.cookie` string.
 *
 * Total, and tolerant in one direction only: a malformed entry, an unknown key
 * or a value that is not `0`/`1` is dropped and leaves that fold at its default
 * of "open". Refusing to render a menu because a jar holds a stale entry would
 * be a worse outcome than a menu that starts unfolded, and "open" is the state
 * where every control is reachable.
 */
export function readSidebarFold(cookie: string): SidebarFold {
  const fold = unfolded();
  for (const pair of cookie.split(";")) {
    const separator = pair.indexOf("=");
    if (separator === -1 || pair.slice(0, separator).trim() !== SIDEBAR_FOLD_COOKIE) {
      continue;
    }
    for (const entry of decodeURIComponent(pair.slice(separator + 1).trim()).split("|")) {
      const colon = entry.indexOf(":");
      if (colon === -1) {
        continue;
      }
      const key = entry.slice(0, colon);
      const value = entry.slice(colon + 1);
      if (value !== "0" && value !== "1") {
        continue;
      }
      const folded = value === "1";
      if (key === "menu") {
        fold.menu = folded;
      } else if (isGroup(key)) {
        fold.groups[key] = folded;
      }
    }
  }
  return fold;
}

/**
 * The `document.cookie` assignment that records this fold state.
 *
 * Writes every key rather than only the folded ones, because a cookie write
 * replaces the name's whole value: omitting an unfolded group would make
 * "unfolded" indistinguishable from "written by an older build", and the two
 * differ the moment a group's default stops being open.
 */
export function sidebarFoldCookie(fold: SidebarFold): string {
  const value = [
    `menu:${fold.menu ? 1 : 0}`,
    ...SIDEBAR_GROUPS.map((group) => `${group}:${fold.groups[group] ? 1 : 0}`),
  ].join("|");
  return `${SIDEBAR_FOLD_COOKIE}=${encodeURIComponent(value)}; path=/; max-age=${SIDEBAR_FOLD_MAX_AGE}`;
}

export interface SidebarFoldState extends SidebarFold {
  /** Fold or unfold the whole menu. */
  toggleMenu: () => void;
  /** Fold or unfold one submenu. */
  toggleGroup: (group: SidebarGroup) => void;
}

/** Write the fold out. Best effort: a document that refuses cookies costs the
 *  user the restore, and must not cost them the click. */
function persist(fold: SidebarFold): void {
  if (typeof document === "undefined") {
    return;
  }
  try {
    // biome-ignore lint/suspicious/noDocumentCookie: chrome state read before React mounts, following `panels.ts` and `column-widths.ts`; CookieStore is async and unavailable there
    document.cookie = sidebarFoldCookie(fold);
  } catch {
    // A jar that will not take a write is not a reason to refuse the fold.
  }
}

export const sidebarFoldStore = createStore<SidebarFoldState>()((set, get) => ({
  ...unfolded(),
  toggleMenu: () => {
    const next: SidebarFold = { menu: !get().menu, groups: get().groups };
    persist(next);
    set({ menu: next.menu });
  },
  toggleGroup: (group) => {
    const groups = { ...get().groups, [group]: !get().groups[group] };
    persist({ menu: get().menu, groups });
    set({ groups });
  },
}));

/** Whether {@link hydrateSidebarFold} has already run in this document. */
let hydrated = false;

/**
 * Restore the remembered fold.
 *
 * Idempotent, so React's double-invoked development effects restore once, and
 * so a second caller cannot overwrite a fold the user has already changed since
 * the first. Mounted at the shell rather than inside the sidebar: the sidebar is
 * unmounted on the phone tier, and a restore that only happens where the drawer
 * renders is a restore that silently does not happen everywhere else.
 */
export function hydrateSidebarFold(cookie: string): void {
  if (hydrated) {
    return;
  }
  hydrated = true;
  const fold = readSidebarFold(cookie);
  sidebarFoldStore.setState({ menu: fold.menu, groups: fold.groups });
}

/** React selector hook over {@link sidebarFoldStore}. */
export function useSidebarFold<T>(selector: (state: SidebarFoldState) => T): T {
  return useStore(sidebarFoldStore, selector);
}

/** Test-only reset: nothing folded, unhydrated, no cookie written. */
export function resetSidebarFoldForTest(): void {
  hydrated = false;
  sidebarFoldStore.setState(unfolded());
}
