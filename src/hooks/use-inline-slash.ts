/**
 * The mid-text `/` menu's state (Story 101.3, R418): the whole-line menu's
 * keyboard model ({@link menuKeyAction}: ↑/↓ move, Enter or Tab insert, Escape
 * dismisses this token) for the token at the caret.
 *
 * The preview is asked only while {@link inlineCandidate} says the caret could
 * be at a token; an answer for a split of the draft the field no longer holds
 * is dropped. Choosing a row replaces exactly the token Rust named. A failed
 * preview is said once under the field and written to `keeper.log` once per
 * stretch of candidate drafts (R381); the message is still the person's to
 * send — a mid-text slash is text.
 */
import { type KeyboardEvent, useCallback, useEffect, useRef, useState } from "react";
import {
  INLINE_PREVIEW_FAILED,
  inlineCandidate,
  inserted,
} from "@/components/slash-menu/inline-slash";
import { menuKeyAction, nextIndex } from "@/components/slash-menu/slash-menu";
import type { SlashFieldProps, SlashMenuIds } from "@/hooks/use-slash-menu";
import { reportFrontendError } from "@/lib/crash-report";
import type { ChatInlinePreviewVm } from "@/lib/ipc/gen/ChatInlinePreviewVm";
import type { ChatInlineRowVm } from "@/lib/ipc/gen/ChatInlineRowVm";

/** What a composer reads off the mid-text menu. */
export interface InlineSlashState {
  /** The rows to draw — empty while closed. */
  rows: readonly ChatInlineRowVm[];
  /** Whether the menu is drawn. */
  open: boolean;
  /** The highlighted row's index, clamped to the rows. */
  highlight: number;
  /** Move the highlight to a row (a pointer over it). */
  setHighlight: (index: number) => void;
  /** Insert a row in place of the token. */
  accept: (row: ChatInlineRowVm) => void;
  /** Offer the menu a keydown; `true` when it claimed it. */
  onKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => boolean;
  /** The drawn menu reports its ids here. */
  setIds: (ids: SlashMenuIds | null) => void;
  /** Spread on the composer's textarea while this menu is the open one. */
  fieldProps: SlashFieldProps;
  /** The sentence to show under the field while the preview cannot be asked. */
  failure: string | null;
  /** What Rust says beside the rows: a drive searched in part, or unread. */
  note: string | null;
}

const NO_ROWS: readonly ChatInlineRowVm[] = [];

export function useInlineSlash({
  draft,
  caret,
  enabled,
  resolve,
  onInsert,
  reportAs,
}: {
  /** The composer's draft. */
  draft: string;
  /** The caret's offset in the draft, or `null` while text is selected. */
  caret: number | null;
  /** Whether the menu may ask or open at all. */
  enabled: boolean;
  /** The Rust preview for a split draft. Must be referentially stable. */
  resolve: (before: string, after: string) => Promise<ChatInlinePreviewVm>;
  /** Replace the draft and put the caret at `caret`. */
  onInsert: (draft: string, caret: number) => void;
  /** The `keeper.log` source a failed preview is written under. */
  reportAs: string;
}): InlineSlashState {
  const [preview, setPreview] = useState<ChatInlinePreviewVm | null>(null);
  const [highlight, setHighlight] = useState(0);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [ids, setIdsState] = useState<SlashMenuIds | null>(null);
  const [failed, setFailed] = useState(false);
  const reported = useRef(false);
  const before = caret === null ? "" : draft.slice(0, caret);
  const after = caret === null ? "" : draft.slice(caret);
  const candidate = enabled && caret !== null && inlineCandidate(before, after);

  useEffect(() => {
    if (!candidate) {
      setPreview(null);
      setFailed(false);
      reported.current = false;
      return;
    }
    let live = true;
    void resolve(before, after)
      .then((answer) => {
        if (live && answer.before === before && answer.after === after) {
          setPreview(answer);
          setHighlight(0);
          setFailed(false);
        }
      })
      .catch((raw: unknown) => {
        if (!live) {
          return;
        }
        setPreview(null);
        if (!reported.current) {
          reported.current = true;
          reportFrontendError(reportAs, raw, null);
        }
        setFailed(true);
      });
    return () => {
      live = false;
    };
  }, [candidate, before, after, resolve, reportAs]);

  const shown =
    preview !== null && preview.before === before && preview.after === after ? preview : null;
  const token = candidate && shown !== null ? shown.token : null;
  // Escape means "not this token": a different token retires it.
  useEffect(() => {
    if (dismissed !== null && token !== dismissed) {
      setDismissed(null);
    }
  }, [token, dismissed]);

  const open = shown !== null && token !== null && shown.rows.length > 0 && token !== dismissed;
  const rows = open && shown !== null ? shown.rows : NO_ROWS;
  const clamped = Math.min(highlight, Math.max(rows.length - 1, 0));
  const active = rows[clamped];

  const setIds = useCallback((next: SlashMenuIds | null) => {
    setIdsState((current) =>
      current?.list === next?.list &&
      JSON.stringify(current?.options) === JSON.stringify(next?.options)
        ? current
        : next,
    );
  }, []);

  const accept = (row: ChatInlineRowVm) => {
    if (token === null) {
      return;
    }
    const next = inserted(before, after, token, row.insert);
    if (next !== null) {
      onInsert(next.draft, next.caret);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (event.nativeEvent.isComposing) {
      return false;
    }
    const action = menuKeyAction(event.key, open, {
      shiftKey: event.shiftKey,
      metaKey: event.metaKey,
      ctrlKey: event.ctrlKey,
      altKey: event.altKey,
    });
    if (action === null) {
      return false;
    }
    event.preventDefault();
    if (action === "dismiss") {
      setDismissed(token);
    } else if (action === "accept") {
      if (active !== undefined) {
        accept(active);
      }
    } else {
      setHighlight(nextIndex(clamped, rows.length, action));
    }
    return true;
  };

  const drawn = open && ids !== null;
  return {
    rows,
    open,
    highlight: clamped,
    setHighlight,
    accept,
    onKeyDown,
    setIds,
    fieldProps: {
      "aria-autocomplete": "list",
      "aria-expanded": open,
      "aria-controls": drawn ? ids.list : undefined,
      "aria-activedescendant": drawn && active !== undefined ? ids.options[active.key] : undefined,
    },
    failure: failed ? INLINE_PREVIEW_FAILED : null,
    note: open && shown !== null ? shown.note : null,
  };
}
