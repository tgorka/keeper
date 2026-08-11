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
 * year-long life, same tolerance for a jar full of other people's cookies. The
 * encoding itself now lives in {@link "@/lib/stores/fold-cookie"}, shared with
 * the notes rail's own fold (Story 47.3) so the two cannot drift; what stays
 * here is what is specific to THIS surface — its cookie name, its closed key
 * set, and the shape the sidebar reads.
 *
 * **This cookie is the chat sidebar's alone.** The notes rail has a section
 * also called Spaces, and it is a different Spaces; it writes a cookie of its
 * own rather than borrowing a key out of this one, so folding a saved-query
 * list cannot fold a Matrix space list.
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
import {
  FOLD_MAX_AGE,
  foldFlagsCookie,
  persistFold,
  readFoldFlags,
} from "@/lib/stores/fold-cookie";

/** The cookie the whole fold state lives in. One cookie, not one per group. */
export const SIDEBAR_FOLD_COOKIE = "keeper_sidebar_fold";

/** A year, matching the panel list and the column widths. A menu that unfolds
 *  itself every Monday is a menu nobody bothers to fold. */
export const SIDEBAR_FOLD_MAX_AGE = FOLD_MAX_AGE;

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

/**
 * Every key this cookie carries, in the order it writes them.
 *
 * `menu` first, then the groups, because that is the order the cookie has been
 * written in since Story 45.20 and a jar written by that build must still read
 * back byte-identically.
 */
const SIDEBAR_FOLD_KEYS = ["menu", ...SIDEBAR_GROUPS] as const;

type SidebarFoldKey = (typeof SIDEBAR_FOLD_KEYS)[number];

/** Every key at "open", which is the state where every control is reachable. */
const SIDEBAR_FOLD_DEFAULTS: Record<SidebarFoldKey, boolean> = {
  menu: false,
  spaces: false,
  networks: false,
};

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
  const flags = readFoldFlags(
    cookie,
    SIDEBAR_FOLD_COOKIE,
    SIDEBAR_FOLD_KEYS,
    SIDEBAR_FOLD_DEFAULTS,
  );
  const fold = unfolded();
  fold.menu = flags.menu;
  for (const group of SIDEBAR_GROUPS) {
    fold.groups[group] = flags[group];
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
  return foldFlagsCookie(SIDEBAR_FOLD_COOKIE, SIDEBAR_FOLD_KEYS, {
    menu: fold.menu,
    ...fold.groups,
  });
}

export interface SidebarFoldState extends SidebarFold {
  /** Fold or unfold the whole menu. */
  toggleMenu: () => void;
  /** Fold or unfold one submenu. */
  toggleGroup: (group: SidebarGroup) => void;
}

/** Write the fold out. */
function persist(fold: SidebarFold): void {
  persistFold(sidebarFoldCookie(fold));
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
