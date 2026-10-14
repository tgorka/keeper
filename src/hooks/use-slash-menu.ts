/**
 * The slash menu's state, shared by every composer that has one (Story 61.9;
 * the messenger composer since Story 100.5, R385).
 *
 * One state machine for the ⌘9 bot composer and the messenger composer, so
 * the two cannot disagree about what a key does: the preview is asked only
 * while the draft could be a command (a keystroke of prose costs nothing), an
 * answer for text the field no longer holds is dropped, Escape dismisses for
 * the token it dismissed and the dismissal is retired the moment the token
 * changes or the draft stops being a command line, and a row is completed on
 * the first accept and run on the second.
 *
 * The field stays the one focus: {@link SlashMenuState.fieldProps} names the
 * list it controls and the highlighted option as its active descendant, so a
 * screen reader in the textarea hears the row ↑/↓ moved to (R393).
 *
 * A preview that fails is said once under the field and written to
 * `keeper.log` once per draft session — not once per keystroke (R381, R393).
 *
 * Use is counted as 100.1's closed surfaces (R391, DW-1430), once per draft
 * session each: `slashMenuOpened` the first time the menu is drawn, and
 * `slashMenuChosen` the first time a row is accepted from it — completing a
 * name and then running it is one choice. Nothing about the command, the
 * draft or the room is sent.
 *
 * What a line *means* is not here: each composer passes its own Rust preview
 * (`botsCommandPreview`, `chatCommandPreview`) as `resolve`.
 */
import { type KeyboardEvent, useCallback, useEffect, useRef, useState } from "react";
import {
  acceptedDraft,
  isCommandLine,
  menuKeyAction,
  nextIndex,
  SLASH_PREVIEW_FAILED,
  type SlashPreview,
  type SlashRow,
  slashToken,
} from "@/components/slash-menu/slash-menu";
import { captureSurface } from "@/hooks/use-telemetry";
import { reportFrontendError } from "@/lib/crash-report";

/** The ids the drawn menu reports back, so the field can point at them. */
export interface SlashMenuIds {
  /** The listbox's id. */
  list: string;
  /** Each drawn option's id, by row name. */
  options: Readonly<Record<string, string>>;
}

/** The ARIA a composer's textarea wears for the menu it drives. */
export interface SlashFieldProps {
  "aria-autocomplete": "list";
  "aria-expanded": boolean;
  "aria-controls": string | undefined;
  "aria-activedescendant": string | undefined;
}

/** What a composer reads off the menu, and the things it can ask of it. */
export interface SlashMenuState<P extends SlashPreview> {
  /** The newest preview for the draft as it stands, or `null`. */
  preview: P | null;
  /** The rows to draw — empty while closed. */
  rows: readonly SlashRow[];
  /** Whether the menu is drawn. */
  open: boolean;
  /** The highlighted row's index, already clamped to the rows. */
  highlight: number;
  /** The highlighted row, `undefined` while closed. */
  active: SlashRow | undefined;
  /** Move the highlight to a row (a pointer over it). */
  setHighlight: (index: number) => void;
  /** Accept a row: complete its name, or run it when it already is complete. */
  accept: (row: SlashRow) => void;
  /**
   * Offer the menu a keydown. `true` when the menu claimed it (and prevented
   * its default); `false` leaves the key wholly to the composer.
   */
  onKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => boolean;
  /** The drawn menu reports its ids here; `null` while nothing is drawn. */
  setIds: (ids: SlashMenuIds | null) => void;
  /** Spread on the composer's textarea. */
  fieldProps: SlashFieldProps;
  /**
   * The sentence to show under the field while the preview cannot be asked,
   * or `null`. A composer showing its own sentence about the same line shows
   * that one instead, so there is one line, not two.
   */
  failure: string | null;
  /**
   * Say that asking about the draft failed — the menu's own preview, or the
   * composer's submit. Reports once per draft session, shows {@link failure}.
   */
  fail: (raw: unknown) => void;
}

/** The closed menu's rows: one array, so a closed menu is the same menu. */
const NO_ROWS: readonly SlashRow[] = [];

function sameIds(a: SlashMenuIds | null, b: SlashMenuIds | null): boolean {
  if (a === null || b === null) {
    return a === b;
  }
  const names = Object.keys(a.options);
  return (
    a.list === b.list &&
    names.length === Object.keys(b.options).length &&
    names.every((name) => a.options[name] === b.options[name])
  );
}

export function useSlashMenu<P extends SlashPreview>({
  draft,
  enabled,
  resolve,
  onComplete,
  onRun,
  reportAs,
}: {
  /** The composer's draft. */
  draft: string;
  /**
   * Whether the menu may ask or open at all — `false` while the messenger
   * composer edits an existing message, whose text is that message's body.
   */
  enabled: boolean;
  /** The Rust preview for a draft. Must be referentially stable. */
  resolve: (draft: string) => Promise<P>;
  /** Replace the draft with a completed command name. */
  onComplete: (draft: string) => void;
  /** Run the draft as it stands — the composer's own send. */
  onRun: () => void;
  /** The `keeper.log` source a failed preview is written under. */
  reportAs: string;
}): SlashMenuState<P> {
  const [preview, setPreview] = useState<P | null>(null);
  const [highlight, setHighlight] = useState(0);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [ids, setIdsState] = useState<SlashMenuIds | null>(null);
  const [failed, setFailed] = useState(false);
  // Whether this draft session's failure is already in `keeper.log`. A session
  // is one stretch of command-line drafts: it ends when the field stops being
  // one (cleared, sent, turned into prose), and the next one reports afresh.
  const reported = useRef(false);
  // Whether this draft session's opening and choice are already counted.
  const countedOpen = useRef(false);
  const countedChoice = useRef(false);
  const candidate = enabled && isCommandLine(draft);

  const fail = useCallback(
    (raw: unknown) => {
      if (!reported.current) {
        reported.current = true;
        reportFrontendError(reportAs, raw, null);
      }
      setFailed(true);
    },
    [reportAs],
  );

  useEffect(() => {
    if (!candidate) {
      setPreview(null);
      setFailed(false);
      reported.current = false;
      countedOpen.current = false;
      countedChoice.current = false;
      return;
    }
    let live = true;
    void resolve(draft)
      .then((answer) => {
        if (live && answer.draft === draft) {
          setPreview(answer);
          setHighlight(0);
          setFailed(false);
        }
      })
      // The menu closes, and the failure is said where the person is typing
      // and written once to `keeper.log` (R381).
      .catch((raw: unknown) => {
        if (live) {
          setPreview(null);
          fail(raw);
        }
      });
    return () => {
      live = false;
    };
  }, [candidate, draft, resolve, fail]);

  const token = candidate ? slashToken(draft) : null;
  // Escape means "not this token": a different token, or no token at all,
  // retires it, so returning to the old spelling later opens the menu again.
  useEffect(() => {
    if (dismissed !== null && token !== dismissed) {
      setDismissed(null);
    }
  }, [token, dismissed]);

  const shown = preview !== null && preview.draft === draft ? preview : null;
  const open = shown !== null && shown.rows.length > 0 && token !== null && token !== dismissed;
  const rows = open && shown !== null ? shown.rows : NO_ROWS;
  const clamped = Math.min(highlight, Math.max(rows.length - 1, 0));
  const active = rows[clamped];

  useEffect(() => {
    if (open && !countedOpen.current) {
      countedOpen.current = true;
      void captureSurface("slashMenuOpened");
    }
  }, [open]);

  const setIds = useCallback((next: SlashMenuIds | null) => {
    setIdsState((current) => (sameIds(current, next) ? current : next));
  }, []);

  const accept = (row: SlashRow) => {
    if (!countedChoice.current) {
      countedChoice.current = true;
      void captureSurface("slashMenuChosen");
    }
    const completed = acceptedDraft(row);
    // Completing first and running on the second accept is Claude Code's model
    // and the one that works for a command with an argument: accepting `/topic`
    // leaves the caret where the topic goes, never runs it with nothing after.
    // Once anything follows the name the command is chosen and the line is the
    // person's to run — completing it then would replace what they typed after
    // it (`/topic Friday` → `/topic `), so it runs as typed and Rust resolves it.
    const argumentsBegun = /\s/.test(draft.slice(1));
    if (!argumentsBegun && draft !== completed) {
      onComplete(completed);
      return;
    }
    onRun();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    // A composing IME delivers Enter as a candidate selection.
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
      // For this token only: Escape means "not this one", never "never again".
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
    preview: shown,
    rows,
    open,
    highlight: clamped,
    active,
    setHighlight,
    accept,
    onKeyDown,
    setIds,
    fieldProps: {
      "aria-autocomplete": "list",
      "aria-expanded": open,
      "aria-controls": drawn ? ids.list : undefined,
      "aria-activedescendant": drawn && active !== undefined ? ids.options[active.name] : undefined,
    },
    failure: failed ? SLASH_PREVIEW_FAILED : null,
    fail,
  };
}
