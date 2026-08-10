/**
 * The windowed note list (Epic 37, Story 37.2, AD-58, UX-DR37, UX-DR41).
 *
 * The app's first windowed list, and it is windowed for a reason a fixture will
 * not show you: a 10 000-note vault has to paint its first screen inside 100 ms
 * (NFR-28), and 10 000 mounted rows do not. {@link useWindowedRows} — the one
 * window this app has, shared with the recordings list and the Files tree
 * (Story 44.10, AD-84) — mounts the visible rows plus a small overscan. A note
 * row is a fixed 64 px (`h-16` on the row itself), so the estimate here is
 * exactly right and no row ever measures differently; the hook measures anyway,
 * for the two callers whose rows wrap.
 *
 * The list renders {@link NoteRowVm}s in Rust's order and never re-sorts.
 * Conflicts above pins above the active sort is a decision `notes_list` already
 * made, and re-deriving it here would be a second place for the two to disagree.
 *
 * Keyboard is the chat list's grammar over a disjoint scope, which is the point:
 * `j`/`k`/`↑`/`↓` move, `Enter` opens, `e` archives, `u` acknowledges an agent's
 * changes, `p` pins, `Delete`/`⌫` ask to remove. A chat-list user needs no new
 * muscle memory. `f` and `m` have no notes meaning and stay unbound — binding a
 * familiar key to an unfamiliar verb is worse than leaving it silent. `u` is
 * one-directional here, unlike the chat list's toggle: `notes_mark_read` is the
 * whole read-state surface and there is no mark-unread twin, so a read row
 * absorbs the press.
 *
 * `Delete` joined that list in Story 45.17 and it is the same verb the muscle
 * memory already carries, which is why it is here rather than left silent: the
 * chat list binds bare `⌫`/`Delete` on the selected message to a redaction
 * **dialog**, and the Files pane binds them to a delete **confirmation**. All
 * three ask and none of them removes anything on the keystroke. Checked against
 * `conversation-pane.tsx` rather than assumed — a familiar key bound to a
 * destructive verb that behaved differently here would be the exact failure the
 * paragraph above refuses, in the one place it costs a note.
 *
 * The roving cursor is keyed by note id rather than index (the chat list's rule,
 * and for the same reason): a streamed re-order or a filter change must move the
 * row, never the cursor. Rendered rows are held by id too, and not by index: a
 * window remounts rows constantly, and an array indexed by position hands back
 * the element that used to be there.
 */
import { type KeyboardEvent, useCallback, useEffect, useRef, useState } from "react";
import { NoteRow } from "@/components/notes/note-row";
import { useWindowedRows } from "@/components/ui/window-list";
import type { NoteRowVm } from "@/lib/ipc/client";

/** The row height the window paces by; matches chat-row density, and matches
 * `h-16` on the row itself. */
export const NOTE_ROW_HEIGHT = 64;

/** How many rows to render beyond the viewport, so a fast scroll never tears. */
const OVERSCAN = 8;

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
  /**
   * The single-key verbs, `⌘⇧R` and `Delete`, dispatched on the row under the
   * cursor. `d` ASKS — it opens the confirmation and never deletes, which is
   * the Files pane's rule (Story 45.3) and the reason a stray keystroke in a
   * list cannot lose a note.
   */
  onVerb: (row: NoteRowVm, verb: "e" | "p" | "u" | "r" | "d") => void;
  /** Ask for a wider window; called as the viewport nears the last row. */
  onGrow: () => void;
}) {
  const rowRefs = useRef(new Map<string, HTMLButtonElement>());

  // The roving keyboard cursor, kept apart from the open note on purpose: `↓`
  // must move the ring, not stream a body per row. `Enter` and a click are what
  // open. Keyed by note id, so a re-ordered stream or a filter change moves the
  // row and leaves the cursor pointing at the same note — or at nothing, when
  // that note is no longer listed.
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const cursor = focusedId === null ? -1 : rows.findIndex((row) => row.id === focusedId);
  // The list always has exactly one tab stop: the cursor's row, or the open
  // note's, or the first. It is handed to the window as the row that must stay
  // mounted: unmount the tab stop and the list has no tab stop at all, and Tab
  // walks straight past the notes.
  const openAt = selectedId === null ? -1 : rows.findIndex((row) => row.id === selectedId);
  const tabStop = cursor >= 0 ? cursor : openAt >= 0 ? openAt : 0;

  // Keyed by note id so a re-ordered stream carries a row's measurement with it
  // rather than leaving the previous row's geometry at that index.
  const getKey = useCallback((index: number) => rows[index]?.id ?? String(index), [rows]);
  const list = useWindowedRows({
    count: rows.length,
    getKey,
    rowHeight: NOTE_ROW_HEIGHT,
    overscan: OVERSCAN,
    pinnedIndex: tabStop,
    // Runs once the revealed row is mounted, which is the whole reason focus is
    // the window's business at all: `↓` from the last visible row targets a row
    // that does not exist in the DOM yet.
    onReveal: (index) => {
      const id = rows[index]?.id;
      if (id !== undefined) {
        rowRefs.current.get(id)?.focus();
      }
    },
  });

  // Grow in an effect, never during render: `onGrow` writes to a store, and a
  // store write mid-render is a cross-component update React is right to refuse.
  useEffect(() => {
    if (rows.length < total && list.lastVisible >= rows.length - GROW_THRESHOLD) {
      onGrow();
    }
  }, [list.lastVisible, rows.length, total, onGrow]);

  const moveTo = (index: number) => {
    const row = rows[index];
    if (row === undefined) {
      return;
    }
    setFocusedId(row.id);
    list.reveal(index);
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
    // Both spellings, because the key that means "remove this" is `Delete` on
    // a full keyboard and `Backspace` on the laptops this app is used on —
    // binding one of them leaves half the users without the verb. Neither
    // deletes: `onVerb` opens the confirmation.
    if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      onVerb(focused, "d");
    }
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the scroll container hosts the list's roving-key handler; every row inside stays independently focusable and operable, so this is additive rather than a replacement for row semantics.
    <div
      {...list.viewportProps}
      onKeyDown={onKeyDown}
      className="min-h-0 flex-1 overflow-y-auto outline-none"
    >
      <ul aria-label="Notes" className="relative w-full" style={{ height: `${list.totalSize}px` }}>
        {list.rows.map((item) => {
          const row = rows[item.index];
          if (row === undefined) {
            return null;
          }
          return (
            <li key={row.id} {...list.rowProps(item)}>
              <NoteRow
                ref={(element) => {
                  if (element === null) {
                    return;
                  }
                  rowRefs.current.set(row.id, element);
                  return () => {
                    rowRefs.current.delete(row.id);
                  };
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
