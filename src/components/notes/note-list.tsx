/**
 * The virtualised note list (Epic 37, Story 37.2, AD-58, UX-DR37, UX-DR41).
 *
 * The app's first virtualised list, and it is virtualised for a reason a
 * fixture will not show you: a 10 000-note vault has to paint its first screen
 * inside 100 ms (NFR-28), and 10 000 mounted rows do not. `@tanstack/react-virtual`
 * renders the visible window plus a small overscan over a fixed 64 px row, which
 * is also why the height is a constant rather than measured — a measured row
 * costs a layout pass per row and buys nothing when every row is the same shape.
 *
 * The list renders {@link NoteRowVm}s in Rust's order and never re-sorts.
 * Conflicts above pins above the active sort is a decision `notes_list` already
 * made, and re-deriving it here would be a second place for the two to disagree.
 *
 * Keyboard is the chat list's grammar over a disjoint scope, which is the point:
 * `j`/`k`/`↑`/`↓` move, `Enter` opens, `e` archives, `u` acknowledges an agent's
 * changes, `p` pins. A chat-list user needs no new muscle memory. `f` and `m`
 * have no notes meaning and stay unbound — binding a familiar key to an
 * unfamiliar verb is worse than leaving it silent. `u` is one-directional here,
 * unlike the chat list's toggle: `notes_mark_read` is the whole read-state
 * surface and there is no mark-unread twin, so a read row absorbs the press.
 *
 * The roving cursor is keyed by note id rather than index (the chat list's rule,
 * and for the same reason): a streamed re-order or a filter change must move the
 * row, never the cursor.
 */
import { useVirtualizer } from "@tanstack/react-virtual";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { NoteRow } from "@/components/notes/note-row";
import type { NoteRowVm } from "@/lib/ipc/client";

/** The row height the virtualiser paces by; matches chat-row density. */
export const NOTE_ROW_HEIGHT = 64;

/** How many rows to render beyond the viewport, so a fast scroll never tears. */
const OVERSCAN = 8;

/**
 * The viewport height the virtualiser assumes before the scroll element has been
 * measured — about one screen of rows. Real layout replaces it on the first
 * measurement pass; environments that never lay out (jsdom) keep it, which is
 * what makes the list testable without faking a bounding rect.
 */
const INITIAL_VIEWPORT_HEIGHT = 640;

/**
 * How close to the end the viewport has to get before more rows are asked for,
 * in rows. Eight is one overscan window: the next page is in flight before the
 * user can scroll past what is rendered.
 */
const GROW_THRESHOLD = 8;

export function NoteList({
  rows,
  total,
  selectedId,
  onSelect,
  onToggleTag,
  onVerb,
  onGrow,
}: {
  rows: NoteRowVm[];
  /** How many notes the filter matches in all, which may exceed `rows.length`. */
  total: number;
  selectedId: string | null;
  onSelect: (row: NoteRowVm) => void;
  onToggleTag: (tag: string) => void;
  /** The single-key verbs and `⌘⇧R`, dispatched on the row under the cursor. */
  onVerb: (row: NoteRowVm, verb: "e" | "p" | "u" | "r") => void;
  /** Ask for a wider window; called as the viewport nears the last row. */
  onGrow: () => void;
}) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const rowRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => NOTE_ROW_HEIGHT,
    overscan: OVERSCAN,
    // A viewport to assume until the real one is measured. Without it the first
    // paint of a freshly mounted list is empty — the scroll element has no
    // measured height yet, so the window is zero rows tall — and it stays empty
    // anywhere layout never runs at all, which is every jsdom test. One screen
    // of rows is the right guess: it is what the measurement almost always
    // comes back with.
    initialRect: { width: 0, height: INITIAL_VIEWPORT_HEIGHT },
    // Keyed by note id so a re-ordered stream moves a row's measurement with it
    // rather than leaving the previous row's geometry at that index.
    getItemKey: (index) => rows[index]?.id ?? index,
  });

  const items = virtualizer.getVirtualItems();
  const lastVisible = items[items.length - 1]?.index ?? -1;
  // Grow in an effect, never during render: `onGrow` writes to a store, and a
  // store write mid-render is a cross-component update React is right to refuse.
  useEffect(() => {
    if (rows.length < total && lastVisible >= rows.length - GROW_THRESHOLD) {
      onGrow();
    }
  }, [lastVisible, rows.length, total, onGrow]);

  // The roving keyboard cursor, kept apart from the open note on purpose: `↓`
  // must move the ring, not stream a body per row. `Enter` and a click are what
  // open. Keyed by note id, so a re-ordered stream or a filter change moves the
  // row and leaves the cursor pointing at the same note — or at nothing, when
  // that note is no longer listed.
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const cursor = focusedId === null ? -1 : rows.findIndex((row) => row.id === focusedId);
  // The list always has exactly one tab stop: the cursor's row, or the open
  // note's, or the first.
  const openAt = selectedId === null ? -1 : rows.findIndex((row) => row.id === selectedId);
  const tabStop = cursor >= 0 ? cursor : openAt >= 0 ? openAt : 0;

  const moveTo = (index: number) => {
    const row = rows[index];
    if (row === undefined) {
      return;
    }
    setFocusedId(row.id);
    virtualizer.scrollToIndex(index);
    // A row the virtualiser has only just scrolled to is not mounted yet, so
    // focus is taken after the browser has had a frame to mount it.
    requestAnimationFrame(() => rowRefs.current[index]?.focus());
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (rows.length === 0) {
      return;
    }
    const focused = cursor >= 0 ? rows[cursor] : undefined;
    // ⌘⇧R is the one chord the list owns, because it acts on the row under the
    // cursor and nothing outside this component knows where that is. Every row
    // in every lens can reveal its real path (UX-DR38).
    if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "r") {
      if (focused !== undefined) {
        event.preventDefault();
        onVerb(focused, "r");
      }
      return;
    }
    // Every other ⌘/⌥/⌃ chord belongs to the global hooks; only bare keys are
    // list-owned.
    if (event.metaKey || event.altKey || event.ctrlKey) {
      return;
    }
    if (event.key === "ArrowDown" || event.key === "j") {
      event.preventDefault();
      moveTo(Math.min(cursor + 1, rows.length - 1));
      return;
    }
    if (event.key === "ArrowUp" || event.key === "k") {
      event.preventDefault();
      moveTo(Math.max((cursor < 0 ? rows.length : cursor) - 1, 0));
      return;
    }
    if (focused === undefined) {
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      onSelect(focused);
      return;
    }
    if (event.key === "e" || event.key === "p" || event.key === "u") {
      event.preventDefault();
      onVerb(focused, event.key);
    }
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the scroll container hosts the list's roving-key handler; every row inside stays independently focusable and operable, so this is additive rather than a replacement for row semantics.
    <div
      ref={scrollRef}
      onKeyDown={onKeyDown}
      className="min-h-0 flex-1 overflow-y-auto outline-none"
    >
      <ul
        aria-label="Notes"
        className="relative w-full"
        style={{ height: `${virtualizer.getTotalSize()}px` }}
      >
        {items.map((item) => {
          const row = rows[item.index];
          if (row === undefined) {
            return null;
          }
          return (
            <li
              key={row.id}
              className="absolute top-0 left-0 w-full"
              style={{ height: `${item.size}px`, transform: `translateY(${item.start}px)` }}
            >
              <NoteRow
                ref={(element) => {
                  rowRefs.current[item.index] = element;
                }}
                row={row}
                selected={row.id === selectedId}
                tabIndex={tabStop === item.index ? 0 : -1}
                onSelect={onSelect}
                onToggleTag={onToggleTag}
              />
            </li>
          );
        })}
      </ul>
    </div>
  );
}
