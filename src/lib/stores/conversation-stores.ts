/**
 * The three stores one open conversation reads and writes: its timeline
 * mirror, its composer's reply/edit/selection state and its attachment tray.
 *
 * The chat's conversation pane uses the app's singletons ({@link CHAT_STORES}),
 * which the rest of the app also reaches — the quick switcher focuses the
 * chat's composer, sign-out clears the chat's timeline. The notes view's dock
 * holds a second conversation open at the same time as a chat may be selected,
 * so it makes its own set ({@link createConversationStores}) and provides it to
 * the timeline and composer under it. Nothing in the dock can then move the
 * chat's selection, its reply context or its focus.
 */
import { createContext, useContext } from "react";
import { useStore } from "zustand";
import type { StoreApi } from "zustand/vanilla";
import {
  type AttachmentsState,
  attachmentsStore,
  createAttachmentsStore,
} from "@/lib/stores/attachments";
import { type ComposerState, composerStore, createComposerStore } from "@/lib/stores/composer";
import { createTimelineStore, type TimelineState, timelineStore } from "@/lib/stores/timeline";

export interface ConversationStores {
  timeline: StoreApi<TimelineState>;
  composer: StoreApi<ComposerState>;
  attachments: StoreApi<AttachmentsState>;
}

/** The chat's conversation: the app's singletons. */
export const CHAT_STORES: ConversationStores = {
  timeline: timelineStore,
  composer: composerStore,
  attachments: attachmentsStore,
};

/** A fresh set for a conversation held open beside the chat's. */
export function createConversationStores(): ConversationStores {
  return {
    timeline: createTimelineStore(),
    composer: createComposerStore(),
    attachments: createAttachmentsStore(),
  };
}

const ConversationStoresContext = createContext<ConversationStores>(CHAT_STORES);

/** Provides a conversation's stores to the timeline and composer under it. */
export const ConversationStoresProvider = ConversationStoresContext.Provider;

/** The stores of the conversation this component is drawn in. */
export function useConversationStores(): ConversationStores {
  return useContext(ConversationStoresContext);
}

/** A slice of this conversation's timeline mirror. */
export function useConversationTimeline<T>(selector: (state: TimelineState) => T): T {
  return useStore(useConversationStores().timeline, selector);
}

/** A slice of this conversation's composer state. */
export function useConversationComposer<T>(selector: (state: ComposerState) => T): T {
  return useStore(useConversationStores().composer, selector);
}

/** A slice of this conversation's attachment tray. */
export function useConversationAttachments<T>(selector: (state: AttachmentsState) => T): T {
  return useStore(useConversationStores().attachments, selector);
}
