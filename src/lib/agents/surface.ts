/**
 * What an agent asks this device to do in the notes view (AD-383,
 * UX-DR131): open a note at a heading, scroll, highlight, point, and propose
 * an edit the person applies or declines.
 *
 * Rust has already decided that the request is for this device, from an agent
 * of the room, not seen before and not expired, and it has named the target
 * (AD-65: nothing here joins a root and a path). What is left is the device's
 * half: bring the note forward, act on the editor once its text is there, and
 * answer — exactly once per request, never with note text.
 *
 * A request for a note that is not open yet waits in {@link SurfaceState.waiting}
 * until the editor that opens it takes it, the shape `[[note#marker]]` links
 * use (`note-editor.tsx`'s `markerRequests`). A proposal lives in its own slot,
 * {@link SurfaceState.proposals}, never in the notes store's `pending`: that slot
 * is the disk's arriving revision, and a sync landing meanwhile would replace it.
 *
 * *Apply* is the person's own edit through the editor ({@link SurfaceEditor.replaceLines}),
 * one undoable transaction saved with the buffer's own revision, and only while
 * the buffer's lines still read `expected` (ruling R40). Nothing here calls
 * `notes_save`.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { isPhoneTier } from "@/hooks/use-shell-layout";
import {
  agentPresenceView,
  agentSurfaceResult,
  agentSurfaceSubscribe,
  type LineSpan,
  type PanelTargetVm,
  type SurfaceOutcome,
  type SurfaceRequestVm,
} from "@/lib/ipc/client";
import { capabilitiesStore } from "@/lib/stores/capabilities";
import { notesVaultsStore, setActiveVault } from "@/lib/stores/notes-vaults";
import { activePanel, panelsStore, sameTarget } from "@/lib/stores/panels";
import { type PrimaryView, primaryViewStore } from "@/lib/stores/primary-view";

/** The editor operations a request needs; `note-editor.tsx`'s runtime has them. */
export interface SurfaceEditor {
  /** The buffer as it is now (the body, without frontmatter). */
  text: () => string;
  /** Whether the person is in this editor: their caret is the one they type with. */
  hasFocus: () => boolean;
  /** Bring lines into view (the top of the note for `null`), the caret to their first line when `caret`. */
  reveal: (span: LineSpan | null, caret: boolean) => void;
  /** Draw the agent's highlight over lines until replaced, or remove it (`null`). */
  highlight: (span: LineSpan | null) => void;
  /** A short pulse over lines, brought into view. */
  point: (span: LineSpan) => void;
  /** Replace lines with `text` as the person's own edit: one undoable transaction. */
  replaceLines: (span: LineSpan, text: string) => void;
}

export interface SurfaceHighlight {
  readonly requestId: string;
  readonly span: LineSpan;
}

export interface SurfaceState {
  /** Requests for a note whose editor has not taken them yet, by {@link surfaceNoteKey}. */
  readonly waiting: Readonly<Record<string, readonly SurfaceRequestVm[]>>;
  /** The proposal a note's strip shows, one per note. */
  readonly proposals: Readonly<Record<string, SurfaceRequestVm>>;
  /** The agent's highlight in a note, until dismissed or replaced. */
  readonly highlights: Readonly<Record<string, SurfaceHighlight>>;
  /** A sentence shown for a moment where a note's proposal was, by {@link surfaceNoteKey}. */
  readonly notices: Readonly<Record<string, string>>;
  /** Bumped whenever a request brings a note forward: the phone pushes its note level on it. */
  readonly revealed: number;
}

/** The detail an `open` or `scroll` carries when Rust found no heading (AC4). */
export const NO_SUCH_HEADING = "no such heading";
/** Told to the agent when the buffer's lines no longer read what it read. */
export const SURFACE_STALE_DETAIL = "the lines changed since the agent read them";
/** A newer proposal for the same note takes the strip. */
export const SURFACE_REPLACED_DETAIL = "a newer proposal replaced it";
export const SURFACE_NOT_A_NOTE_DETAIL = "only a note can be highlighted, pointed at or edited";
export const SURFACE_NO_NOTES_DETAIL = "notes are not available on this device";
export const SURFACE_NO_FILES_DETAIL = "this device cannot preview files";
/** Said where a proposal was when the agent stopped waiting before the person answered. */
export const SURFACE_EXPIRED_NOTICE = "Your agent stopped waiting for an answer to its proposal.";
/** How long {@link SURFACE_EXPIRED_NOTICE} stays. */
export const SURFACE_NOTICE_MS = 3_000;

const EMPTY: SurfaceState = {
  waiting: {},
  proposals: {},
  highlights: {},
  notices: {},
  revealed: 0,
};

export const surfaceStore = createStore<SurfaceState>()(() => EMPTY);

/** The same key the notes store uses: `\u0000` is in neither id. */
export function surfaceNoteKey(vaultId: string, noteId: string): string {
  return `${vaultId}\u0000${noteId}`;
}

/** `line 3` or `lines 3–5`, for the strips. */
export function linesLabel(span: LineSpan): string {
  return span.from === span.to ? `line ${span.from}` : `lines ${span.from}–${span.to}`;
}

/** Whether `span` names lines a buffer of `lineCount` lines has. */
export function spanFits(span: LineSpan, lineCount: number): boolean {
  return (
    Number.isInteger(span.from) &&
    Number.isInteger(span.to) &&
    span.from >= 1 &&
    span.from <= span.to &&
    span.to <= lineCount
  );
}

/** Lines `span` of `text`, joined by `\n` — how `expected` is written. */
export function linesOf(text: string, span: LineSpan): string {
  return text
    .split("\n")
    .slice(span.from - 1, span.to)
    .join("\n");
}

/** Requests handed on by Rust this session: a second delivery is not a second request. */
const seen = new Set<string>();
/** Requests that brought their note forward (switched the view or the panel to it). */
const brought = new Set<string>();
const expiries = new Map<string, number>();
const noticeTimers = new Map<string, number>();

function without<T>(record: Readonly<Record<string, T>>, key: string): Record<string, T> {
  const { [key]: _gone, ...rest } = record;
  return rest;
}

/** Drop every trace of a request from the waiting queues and the strips. */
function forget(requestId: string): void {
  const state = surfaceStore.getState();
  let waiting = state.waiting;
  for (const [key, queue] of Object.entries(waiting)) {
    if (queue.some((each) => each.requestId === requestId)) {
      const rest = queue.filter((each) => each.requestId !== requestId);
      waiting = rest.length === 0 ? without(waiting, key) : { ...waiting, [key]: rest };
    }
  }
  let proposals = state.proposals;
  for (const [key, proposal] of Object.entries(proposals)) {
    if (proposal.requestId === requestId) {
      proposals = without(proposals, key);
    }
  }
  if (waiting !== state.waiting || proposals !== state.proposals) {
    surfaceStore.setState({ waiting, proposals });
  }
}

/**
 * Answer a request. Once is structural rather than checked: a request reaches
 * this only from where it still is — its waiting queue, its strip, its expiry
 * timer — and answering takes it out of all three, while `seen` keeps a
 * second delivery from putting it back.
 */
function answer(
  request: SurfaceRequestVm,
  outcome: SurfaceOutcome,
  applied: boolean | null = null,
  detail: string | null = null,
): void {
  clearTimeout(expiries.get(request.requestId));
  expiries.delete(request.requestId);
  brought.delete(request.requestId);
  forget(request.requestId);
  void agentSurfaceResult(request.accountId, request.roomId, {
    requestId: request.requestId,
    outcome,
    applied,
    detail,
  }).catch(() => {
    // Rust refuses only a request it no longer waits for; the host has
    // stopped listening, so there is nobody left to tell.
  });
}

function isExpired(request: SurfaceRequestVm): boolean {
  return Date.now() >= request.expiresAtMs;
}

/** Show `sentence` where note `key`'s strips are, for {@link SURFACE_NOTICE_MS}. */
function notice(key: string, sentence: string): void {
  clearTimeout(noticeTimers.get(key));
  surfaceStore.setState((state) => ({ notices: { ...state.notices, [key]: sentence } }));
  noticeTimers.set(
    key,
    window.setTimeout(() => {
      noticeTimers.delete(key);
      surfaceStore.setState((state) => ({ notices: without(state.notices, key) }));
    }, SURFACE_NOTICE_MS),
  );
}

/**
 * The agent stopped waiting. A proposal the person may still be reading goes
 * with a word on why, rather than vanishing (the 60 s is ruled; the sentence
 * costs nothing).
 */
function expire(request: SurfaceRequestVm): void {
  const shownAt = Object.entries(surfaceStore.getState().proposals).find(
    ([, proposal]) => proposal.requestId === request.requestId,
  )?.[0];
  answer(request, "expired");
  if (shownAt !== undefined) {
    notice(shownAt, SURFACE_EXPIRED_NOTICE);
  }
}

/** Show a target the way a person opening it would: the panel holding it, else one beside. */
function showTarget(target: PanelTargetVm): void {
  const panels = panelsStore.getState();
  if (sameTarget(activePanel(panels).target, target)) {
    return;
  }
  if (isPhoneTier()) {
    // A phone has one panel: opening is replacing.
    panels.setActiveTarget(target);
    return;
  }
  panels.openPanel(target);
}

/** Show the note; whether that changed what was in front of the person. */
function bringNoteForward(vaultId: string, noteId: string): boolean {
  const target: PanelTargetVm = { kind: "note", vaultId, noteId };
  const wasInFront =
    primaryViewStore.getState().view === "notes" &&
    sameTarget(activePanel(panelsStore.getState()).target, target);
  primaryViewStore.getState().setView("notes");
  if (notesVaultsStore.getState().activeVaultId !== vaultId) {
    void setActiveVault(vaultId).catch(() => {});
  }
  showTarget(target);
  surfaceStore.setState((state) => ({ revealed: state.revealed + 1 }));
  return !wasInFront;
}

/** Execute one request Rust handed on. */
export function handleSurfaceRequest(request: SurfaceRequestVm): void {
  if (seen.has(request.requestId)) {
    return;
  }
  seen.add(request.requestId);
  if (isExpired(request)) {
    answer(request, "expired");
    return;
  }
  expiries.set(
    request.requestId,
    window.setTimeout(() => expire(request), request.expiresAtMs - Date.now()),
  );

  const { target } = request;
  if (target.kind === "file") {
    if (request.tool !== "open") {
      answer(request, "unavailable", null, SURFACE_NOT_A_NOTE_DETAIL);
      return;
    }
    if (isPhoneTier() || !capabilitiesStore.getState().capabilities.sync) {
      answer(request, "unavailable", null, SURFACE_NO_FILES_DETAIL);
      return;
    }
    primaryViewStore.getState().setView("files");
    showTarget(target);
    answer(request, "done");
    return;
  }
  if (target.kind !== "note") {
    // Rust names a note or a file; anything else is no place to show.
    answer(request, "unavailable", null, SURFACE_NOT_A_NOTE_DETAIL);
    return;
  }
  if (!capabilitiesStore.getState().capabilities.notes) {
    answer(request, "unavailable", null, SURFACE_NO_NOTES_DETAIL);
    return;
  }
  const key = surfaceNoteKey(target.vaultId, target.noteId);
  surfaceStore.setState((state) => ({
    waiting: { ...state.waiting, [key]: [...(state.waiting[key] ?? []), request] },
  }));
  if (bringNoteForward(target.vaultId, target.noteId)) {
    brought.add(request.requestId);
  }
}

function execute(key: string, request: SurfaceRequestVm, editor: SurfaceEditor): void {
  if (isExpired(request)) {
    answer(request, "expired");
    return;
  }
  const text = editor.text();
  const lineCount = text.split("\n").length;
  const span = request.range;
  // Rust found no heading: `open` and `scroll` go to the top and say so (AC4).
  const missingHeading = request.heading !== null && span === null ? NO_SUCH_HEADING : null;
  if (span !== null && !spanFits(span, lineCount)) {
    answer(
      request,
      "unavailable",
      null,
      `${linesLabel(span)} not in the note, which has ${lineCount} lines`,
    );
    return;
  }
  switch (request.tool) {
    case "open":
      // The caret goes to the heading when the request brought the note
      // forward or nobody is typing in it; an editor the person is in only
      // scrolls — the caret is what they type with.
      editor.reveal(span, brought.has(request.requestId) || !editor.hasFocus());
      answer(request, "done", null, missingHeading);
      return;
    case "scroll":
      editor.reveal(span, false);
      answer(request, "done", null, missingHeading);
      return;
    case "highlight":
      if (span === null) {
        answer(request, "unavailable", null, "no lines to highlight");
        return;
      }
      surfaceStore.setState((state) => ({
        highlights: { ...state.highlights, [key]: { requestId: request.requestId, span } },
      }));
      editor.reveal(span, false);
      answer(request, "done");
      return;
    case "point":
      if (span === null) {
        answer(request, "unavailable", null, "no lines to point at");
        return;
      }
      editor.point(span);
      answer(request, "done");
      return;
    case "propose_edit": {
      if (span === null || request.text === null || request.expected === null) {
        answer(request, "unavailable", null, "the proposal names no lines");
        return;
      }
      if (linesOf(text, span) !== request.expected) {
        answer(request, "unavailable", null, SURFACE_STALE_DETAIL);
        return;
      }
      const shown = surfaceStore.getState().proposals[key];
      if (shown !== undefined) {
        answer(shown, "unavailable", null, SURFACE_REPLACED_DETAIL);
      }
      surfaceStore.setState((state) => ({
        proposals: { ...state.proposals, [key]: request },
      }));
      editor.reveal(span, false);
      return;
    }
  }
}

/**
 * Execute every request waiting for this note. Called by the editor that shows
 * it, once the note's text is in the buffer; the first editor to call takes them.
 */
export function takeSurfaceRequests(vaultId: string, noteId: string, editor: SurfaceEditor): void {
  const key = surfaceNoteKey(vaultId, noteId);
  const queue = surfaceStore.getState().waiting[key];
  if (queue === undefined) {
    return;
  }
  surfaceStore.setState((state) => ({ waiting: without(state.waiting, key) }));
  for (const request of queue) {
    execute(key, request, editor);
  }
}

/**
 * Follow this note's surface state from an editor for as long as it lives:
 * draw the highlight the store holds, and take requests that arrive while the
 * note is open. `ready` says whether the note's text has arrived.
 */
export function watchSurfaceNote(
  vaultId: string,
  noteId: string,
  editor: SurfaceEditor,
  ready: () => boolean,
): () => void {
  const key = surfaceNoteKey(vaultId, noteId);
  // Subscribed before anything is taken: a highlight taken below reaches this
  // editor through the store, like one taken later.
  const stop = surfaceStore.subscribe((next, previous) => {
    const highlight = next.highlights[key];
    if (highlight !== previous.highlights[key]) {
      editor.highlight(highlight?.span ?? null);
    }
    if (next.waiting[key] !== undefined && next.waiting[key] !== previous.waiting[key] && ready()) {
      takeSurfaceRequests(vaultId, noteId, editor);
    }
  });
  const current = surfaceStore.getState().highlights[key];
  if (current !== undefined) {
    editor.highlight(current.span);
  }
  if (ready()) {
    takeSurfaceRequests(vaultId, noteId, editor);
  }
  return stop;
}

/** The person put the highlight away. */
export function dismissSurfaceHighlight(vaultId: string, noteId: string): void {
  const key = surfaceNoteKey(vaultId, noteId);
  if (surfaceStore.getState().highlights[key] !== undefined) {
    surfaceStore.setState((state) => ({ highlights: without(state.highlights, key) }));
  }
}

/**
 * *Apply*: replace the proposal's lines in the buffer the person sees, as
 * their own edit, while those lines still read what the agent read.
 */
export function applySurfaceProposal(vaultId: string, noteId: string, editor: SurfaceEditor): void {
  const request = surfaceStore.getState().proposals[surfaceNoteKey(vaultId, noteId)];
  if (request === undefined || request.range === null || request.text === null) {
    return;
  }
  if (isExpired(request)) {
    answer(request, "expired");
    return;
  }
  const text = editor.text();
  if (
    !spanFits(request.range, text.split("\n").length) ||
    linesOf(text, request.range) !== request.expected
  ) {
    answer(request, "unavailable", null, SURFACE_STALE_DETAIL);
    return;
  }
  editor.replaceLines(request.range, request.text);
  answer(request, "done", true);
}

/** *Decline*: nothing changes, and the agent is told so. */
export function declineSurfaceProposal(vaultId: string, noteId: string): void {
  const request = surfaceStore.getState().proposals[surfaceNoteKey(vaultId, noteId)];
  if (request !== undefined) {
    answer(request, "declined", false);
  }
}

/**
 * The view id this device's presence carries: the chat-list windows are one
 * view, `chats`; every other primary view is its own id.
 */
export function presenceViewOf(view: PrimaryView): string {
  switch (view) {
    case "inbox":
    case "archive":
    case "agents":
      return "chats";
    default:
      return view;
  }
}

let started = false;
let reportedView: string | null = null;

function reportView(view: PrimaryView): void {
  const id = presenceViewOf(view);
  if (id === reportedView) {
    return;
  }
  reportedView = id;
  void agentPresenceView(id).catch(() => {
    // No account to publish for yet; the next change tries again.
    reportedView = null;
  });
}

/**
 * Start executing surface requests and reporting the primary view, once per
 * webview: Rust drops requests that arrive before anyone subscribes, so this
 * runs at app start.
 */
export function startAgentSurface(): void {
  if (started) {
    return;
  }
  started = true;
  try {
    void agentSurfaceSubscribe(handleSurfaceRequest).catch(() => {});
    reportView(primaryViewStore.getState().view);
    primaryViewStore.subscribe((state) => reportView(state.view));
  } catch {
    // No Tauri host (a test, a non-desktop port): the surface is inert.
  }
}

/** React selector hook over {@link surfaceStore}. */
export function useSurfaceStore<T>(selector: (state: SurfaceState) => T): T {
  return useStore(surfaceStore, selector);
}

/** Test-only: forget every request, answer, timer and subscription flag. */
export function resetAgentSurfaceForTest(): void {
  for (const timer of expiries.values()) {
    clearTimeout(timer);
  }
  expiries.clear();
  for (const timer of noticeTimers.values()) {
    clearTimeout(timer);
  }
  noticeTimers.clear();
  seen.clear();
  brought.clear();
  started = false;
  reportedView = null;
  surfaceStore.setState(EMPTY, true);
}
