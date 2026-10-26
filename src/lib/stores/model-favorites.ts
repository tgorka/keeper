/**
 * The models a person starred in the agents' model picker (109.18), remembered in a cookie.
 *
 * A favourite is a lens: it orders the picker and decides nothing an agent does, so it earns no
 * settings row and no IPC write — the same answer `forges.ts` and `fold-cookie.ts` give for a
 * remembered view, and not `localStorage`, which this codebase refuses out loud. Which models are
 * OFFERED, chosen and default is Rust's (`AgentSessionPickerVm`); a starred model the agent does
 * not offer is never listed, because the picker lists Rust's offers and only sorts them by this.
 *
 * Device-wide, not per agent: a model id names the same model whichever agent offers it.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { FOLD_MAX_AGE, persistFold } from "@/lib/stores/fold-cookie";

export const MODEL_FAVORITES_COOKIE = "keeper_model_favorites";

/** A cookie holds ~4 kB; a person stars a handful. The oldest stars go first. */
export const MODEL_FAVORITES_MAX = 32;

/** The starred model ids in `cookie`, oldest first; anything unreadable is dropped. */
export function readModelFavorites(cookie: string): string[] {
  for (const part of cookie.split(";")) {
    const [name, ...value] = part.trim().split("=");
    if (name !== MODEL_FAVORITES_COOKIE) continue;
    try {
      const parsed: unknown = JSON.parse(decodeURIComponent(value.join("=")));
      if (!Array.isArray(parsed)) return [];
      const ids = parsed.filter((id): id is string => typeof id === "string" && id.length > 0);
      return [...new Set(ids)].slice(-MODEL_FAVORITES_MAX);
    } catch {
      return [];
    }
  }
  return [];
}

/** The `document.cookie` assignment that records `ids`. */
export function modelFavoritesCookie(ids: readonly string[]): string {
  const value = encodeURIComponent(JSON.stringify(ids.slice(-MODEL_FAVORITES_MAX)));
  return `${MODEL_FAVORITES_COOKIE}=${value}; path=/; max-age=${FOLD_MAX_AGE}`;
}

interface ModelFavorites {
  /** `null` until read from the cookie on first use. */
  ids: string[] | null;
}

const modelFavoritesStore = createStore<ModelFavorites>()(() => ({ ids: null }));

function held(): string[] {
  const { ids } = modelFavoritesStore.getState();
  if (ids !== null) return ids;
  const read = typeof document === "undefined" ? [] : readModelFavorites(document.cookie);
  modelFavoritesStore.setState({ ids: read });
  return read;
}

/** The starred ids, read from the cookie on first use. */
export function useModelFavorites(): string[] {
  const ids = useStore(modelFavoritesStore, (state) => state.ids);
  return ids ?? held();
}

/** Star `id`, or unstar it when it is starred. */
export function toggleModelFavorite(id: string): void {
  const ids = held();
  const next = ids.includes(id) ? ids.filter((starred) => starred !== id) : [...ids, id];
  const bounded = next.slice(-MODEL_FAVORITES_MAX);
  modelFavoritesStore.setState({ ids: bounded });
  persistFold(modelFavoritesCookie(bounded));
}

/** Test-only reset. */
export function resetModelFavoritesForTest(): void {
  modelFavoritesStore.setState({ ids: null });
}
