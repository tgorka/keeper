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
   * A pending composer-text restore (Story 8.3, Undo-Send). Set by `restore` when a
   * held send is undone: the composer watches `restoreNonce` and, when it changes,
   * replaces its textarea content with `restoreBody`. `null` means nothing is
   * pending. Ephemeral UI signal (the durable draft is already persisted in Rust by
   * `cancelHeldSend`); this only drives the in-place composer update without a remount.
   */
  restoreBody: string | null;
  /**
   * The `(accountId, roomId)` the pending restore belongs to, or `null`. The composer
   * applies a restore only when this matches its own chat, so a restore never lands in
   * the wrong room's composer if the user switches chats during the async cancel.
   */
  restoreTarget: { accountId: string; roomId: string } | null;
  /**
   * A monotonically-increasing nonce bumped by {@link ComposerState.restore} so the
   * composer applies a restore even when the same body is restored twice in a row.
   */
  restoreNonce: number;
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
  /**
   * Restore `body` into the `(accountId, roomId)` composer (Story 8.3): set
   * {@link ComposerState.restoreBody}/{@link ComposerState.restoreTarget} and bump
   * {@link ComposerState.restoreNonce} so that chat's open composer replaces its text.
   * Used after an Undo-Send cancel returns the held body. Also bumps `focusNonce` so
   * focus lands back in the composer. The target scoping keeps the body out of any
   * other room's composer if the user switched chats during the cancel.
   */
  restore: (accountId: string, roomId: string, body: string) => void;
}

/**
 * The vanilla store instance. Created once at module load, shared across the app.
 */
export const composerStore = createStore<ComposerState>()((set, get) => ({
  pending: null,
  stashedDraft: null,
  selectedKey: null,
  restoreBody: null,
  restoreTarget: null,
  restoreNonce: 0,
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
  restore: (accountId, roomId, body) =>
    set((state) => ({
      restoreBody: body,
      restoreTarget: { accountId, roomId },
      restoreNonce: state.restoreNonce + 1,
      focusNonce: state.focusNonce + 1,
    })),
}));

/**
 * React selector hook over {@link composerStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useComposerStore<T>(selector: (state: ComposerState) => T): T {
  return useStore(composerStore, selector);
}
