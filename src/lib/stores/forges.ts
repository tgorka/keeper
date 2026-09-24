/**
 * The repository sources' mirror (Epic 86, AD-333, AD-338).
 *
 * A vanilla zustand store holding what `forges_list` last answered, so the
 * two entry points (Settings › Sync and the Sync pane), the Browse sheet and
 * the add form's `Sign in with <source>` choice all read ONE list. `null`
 * means Rust has not answered: every surface renders nothing for it, which is
 * also what an empty list renders — an install with no account and no GitHub
 * client id sees keeper exactly as before (AD-27).
 *
 * The last source the person looked at is remembered in a cookie, like every
 * other lens in this app (`fold-cookie.ts` says why not `localStorage`).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { type ForgeSourceVm, forgesList } from "@/lib/ipc/client";
import { FOLD_MAX_AGE, persistFold } from "@/lib/stores/fold-cookie";

export interface ForgesState {
  /** The sources as Rust last described them, or `null` before it answered. */
  sources: ForgeSourceVm[] | null;
}

export const forgesStore = createStore<ForgesState>()(() => ({ sources: null }));

/** React selector hook over {@link forgesStore}. */
export function useForgesStore<T>(selector: (state: ForgesState) => T): T {
  return useStore(forgesStore, selector);
}

/**
 * Read the sources again. A failed read keeps what was held: a surface that
 * showed an entry a moment ago should not lose it to one refused call, and one
 * that showed none keeps showing none.
 */
export async function refreshForgeSources(): Promise<void> {
  try {
    const sources = await forgesList();
    forgesStore.setState({ sources });
  } catch {
    // Kept as it was; see above.
  }
}

/** Replace one source with the snapshot a connect, disconnect or sign-in answered. */
export function putForgeSource(source: ForgeSourceVm): void {
  forgesStore.setState((state) => ({
    sources: state.sources?.map((held) => (held.id === source.id ? source : held)) ?? state.sources,
  }));
}

/** The cookie that remembers the last source looked at. */
export const FORGE_SOURCE_COOKIE = "keeper_forge_source";

/** The remembered source id in `cookie`, or `null`. */
export function readForgeSourceCookie(cookie: string): string | null {
  for (const part of cookie.split(";")) {
    const [name, ...value] = part.trim().split("=");
    if (name === FORGE_SOURCE_COOKIE) {
      return decodeURIComponent(value.join("=")) || null;
    }
  }
  return null;
}

/** Remember `id` as the source to open on next time. */
export function rememberForgeSource(id: string): void {
  persistFold(`${FORGE_SOURCE_COOKIE}=${encodeURIComponent(id)}; path=/; max-age=${FOLD_MAX_AGE}`);
}

/** Test-only reset. */
export function resetForgesStoreForTest(): void {
  forgesStore.setState({ sources: null });
}
