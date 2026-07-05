/**
 * Pending-attachment tray store (Story 3.7, FR-13, AD-4).
 *
 * A vanilla zustand store (created outside React, like {@link composerStore}) that
 * holds the composer's ephemeral tray of attachments awaiting Send. It is pure UI
 * state — never a source of truth, never anything from Rust, and it never issues
 * IPC. Attachments enter as one of two ingestion shapes and never as base64/JSON
 * media over IPC:
 *  - `path`: an OS file path (composer attach button + native drag-drop) — the Rust
 *    core reads the file itself, so no bytes cross IPC.
 *  - `bytes`: raw bytes for a path-less pasted clipboard image — dispatched as a
 *    raw binary IPC body (never base64/JSON).
 *
 * Each entry carries an opaque client `id` so a chip can be removed (a pre-upload
 * cancel) without needing the path/bytes to be unique.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/** An attachment ingested by an OS file path (attach button / drag-drop). */
export interface PathAttachment {
  /** Opaque client id for removal (a pre-upload cancel). */
  id: string;
  kind: "path";
  /** The OS file path; the Rust core reads the file (bytes never cross IPC). */
  path: string;
  /** The display filename derived from the path (for the chip). */
  filename: string;
}

/** An attachment ingested as raw bytes (a path-less pasted clipboard image). */
export interface BytesAttachment {
  /** Opaque client id for removal (a pre-upload cancel). */
  id: string;
  kind: "bytes";
  /** The raw image bytes; dispatched as a raw binary IPC body (never base64). */
  bytes: ArrayBuffer;
  /** The (synthesized) display filename for the chip and the sent event. */
  filename: string;
  /** The clipboard MIME type (e.g. `"image/png"`). */
  mime: string;
  /** The byte size, shown on the chip. */
  size: number;
}

/** A tray entry: an OS-path attachment or a raw-bytes (pasted) attachment. */
export type PendingAttachment = PathAttachment | BytesAttachment;

export interface AttachmentsState {
  /** The tray of attachments awaiting Send (empty when there are none). */
  pending: PendingAttachment[];
  /** Append one attachment to the tray. */
  add: (attachment: PendingAttachment) => void;
  /** Append several attachments to the tray (e.g. a multi-file drop / pick). */
  addMany: (attachments: PendingAttachment[]) => void;
  /** Remove one attachment by its opaque id (a pre-upload cancel). */
  remove: (id: string) => void;
  /** Empty the tray (after a successful Send, or on a room switch). */
  clear: () => void;
}

/** Monotonic source of opaque client attachment ids. */
let nextId = 0;

/** Mint a fresh opaque client attachment id. */
export function attachmentId(): string {
  nextId += 1;
  return `att-${nextId}`;
}

/**
 * The vanilla store instance. Created once at module load, shared across the app
 * (the composer renders the tray + Sends it; the conversation pane's drag-drop
 * listener adds dropped paths to it).
 */
export const attachmentsStore = createStore<AttachmentsState>()((set) => ({
  pending: [],
  add: (attachment) => set((s) => ({ pending: [...s.pending, attachment] })),
  addMany: (attachments) =>
    set((s) => (attachments.length === 0 ? s : { pending: [...s.pending, ...attachments] })),
  remove: (id) => set((s) => ({ pending: s.pending.filter((a) => a.id !== id) })),
  clear: () => set({ pending: [] }),
}));

/**
 * React selector hook over {@link attachmentsStore}. Pass a selector to subscribe
 * to just the slice a component needs.
 */
export function useAttachmentsStore<T>(selector: (state: AttachmentsState) => T): T {
  return useStore(attachmentsStore, selector);
}
