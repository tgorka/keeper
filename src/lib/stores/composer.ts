/**
 * Composer reply/edit context + message-selection store (Story 3.4, FR-10/FR-11).
 *
 * A vanilla zustand store created at module load *outside* React. It holds only
 * ephemeral composer UI state — never a source of truth, never anything from
 * Rust:
 *  - `pending`: the active reply or edit context (or `null`). A reply carries the
 *    quoted sender + preview for the banner; an edit carries only the target key.
 *  - `stashedDraft`: the composer draft stashed when entering **edit** so `Esc`
 *    can restore it (a reply leaves the draft untouched, so it stashes nothing).
 *  - `selectedKey`: the keyboard-selected message key (drives the `↑`/`↓` ring and
 *    `r`/`e` shortcuts).
 *
 * The store never issues IPC and never inspects message content beyond the
 * non-secret render data the caller already holds. Targets are addressed only by
 * the opaque render `key` (`unique_id`).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/** The active reply/edit context, or `null` when composing a fresh message. */
export type PendingContext =
  | {
      mode: "reply";
      /** The original message's opaque render key (the reply target). */
      targetKey: string;
      /** The original sender's display label, for the banner. */
      sender: string;
      /** A short preview of the original body, for the banner. */
      bodyPreview: string;
    }
  | {
      mode: "edit";
      /** The own message's opaque render key (the edit target). */
      targetKey: string;
    };

/** A reply target: the original message's key plus its banner display data. */
export interface ReplyTarget {
  targetKey: string;
  sender: string;
  bodyPreview: string;
}

/** An edit target: the own message's key plus its current body to prefill. */
export interface EditTarget {
  targetKey: string;
  body: string;
}

export interface ComposerState {
  /** The active reply/edit context, or `null`. */
  pending: PendingContext | null;
  /**
   * The composer draft stashed when entering edit (so `Esc` restores it), or
   * `null` when there is nothing stashed (fresh compose or reply mode).
   */
  stashedDraft: string | null;
  /** The keyboard-selected message key, or `null`. */
  selectedKey: string | null;
  /**
   * A monotonically-increasing focus nonce (Story 6.6). Bumped by
   * {@link ComposerState.requestFocus} to programmatically focus the composer's
   * textarea for the currently-open room — e.g. after a new chat is resolved and
   * opened. Ephemeral UI signal, not a source of truth (mirrors the rooms store's
   * `focusEvent` deep-link pattern).
   */
  focusNonce: number;
  /**
   * Enter reply mode for `target`. The typed draft is left untouched (reply keeps
   * the user's text), so nothing is stashed.
   */
  startReply: (target: ReplyTarget) => void;
  /**
   * Enter edit mode for `target`, stashing `currentDraft` so `Esc` can restore it.
   * Returns the message body the composer should prefill.
   */
  startEdit: (target: EditTarget, currentDraft: string) => string;
  /**
   * Cancel the pending context (`Esc` / banner ×). Returns the draft the composer
   * should restore: the stashed pre-edit draft for an edit, or `null` for a reply
   * (whose draft was never touched, so the composer keeps its current text).
   */
  cancel: () => string | null;
  /** Clear the pending context and stash after a successful send. */
  clear: () => void;
  /** Set the keyboard-selected message key. */
  select: (key: string) => void;
  /** Clear the keyboard selection. */
  clearSelection: () => void;
  /** Request programmatic composer focus by bumping {@link ComposerState.focusNonce}. */
  requestFocus: () => void;
}

/**
 * The vanilla store instance. Created once at module load, shared across the app.
 */
export const composerStore = createStore<ComposerState>()((set, get) => ({
  pending: null,
  stashedDraft: null,
  selectedKey: null,
  focusNonce: 0,
  startReply: ({ targetKey, sender, bodyPreview }) =>
    // Reply leaves the typed draft alone: no stash, clear any prior edit stash.
    set({ pending: { mode: "reply", targetKey, sender, bodyPreview }, stashedDraft: null }),
  startEdit: ({ targetKey, body }, currentDraft) => {
    set({ pending: { mode: "edit", targetKey }, stashedDraft: currentDraft });
    return body;
  },
  cancel: () => {
    const { pending, stashedDraft } = get();
    const restore = pending?.mode === "edit" ? (stashedDraft ?? "") : null;
    set({ pending: null, stashedDraft: null });
    return restore;
  },
  clear: () => set({ pending: null, stashedDraft: null }),
  select: (key) => set({ selectedKey: key }),
  clearSelection: () => set({ selectedKey: null }),
  requestFocus: () => set((state) => ({ focusNonce: state.focusNonce + 1 })),
}));

/**
 * React selector hook over {@link composerStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useComposerStore<T>(selector: (state: ComposerState) => T): T {
  return useStore(composerStore, selector);
}
