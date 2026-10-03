/**
 * The agent's marks in a note (UX-DR131): a highlight that stays until
 * the person dismisses it or the agent replaces it, and a pointer's short
 * pulse. Both are line decorations mapped through edits, installed outside the
 * live-preview compartment so Source shows them too, and neither is an edit:
 * they never reach the undo history or `onEdit`.
 *
 * The highlight is the ring colour, not the search marks' yellow and not the
 * external-change flash, so "the agent is showing you this" never reads as
 * "this matched your search" or "this changed on disk".
 */
import {
  type EditorState,
  type Range,
  StateEffect,
  StateField,
  type Text,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";
import type { LineSpan } from "@/lib/ipc/client";

/** How long the pointer's pulse lasts before its lines are let go. */
export const AGENT_POINT_MS = 2_400;

export const AGENT_HIGHLIGHT_CLASS = "cm-agent-highlight";
export const AGENT_POINT_CLASS = "cm-agent-point";

const setHighlight = StateEffect.define<LineSpan | null>();
const setPoint = StateEffect.define<LineSpan | null>();

/** Offsets of the first character of `span.from` and the last of `span.to`, clamped to the document. */
function offsetsOf(doc: Text, span: LineSpan): { from: number; to: number } {
  const first = Math.min(Math.max(span.from, 1), doc.lines);
  const last = Math.min(Math.max(span.to, first), doc.lines);
  return { from: doc.line(first).from, to: doc.line(last).to };
}

function linesField(effect: typeof setHighlight, className: string) {
  const line = Decoration.line({ class: className });
  return StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(value, transaction) {
      let next = value.map(transaction.changes);
      for (const each of transaction.effects) {
        if (!each.is(effect)) {
          continue;
        }
        if (each.value === null) {
          next = Decoration.none;
          continue;
        }
        const state: EditorState = transaction.state;
        const { from, to } = offsetsOf(state.doc, each.value);
        const ranges: Range<Decoration>[] = [];
        for (let at = state.doc.lineAt(from).number; at <= state.doc.lineAt(to).number; at += 1) {
          ranges.push(line.range(state.doc.line(at).from));
        }
        next = Decoration.set(ranges);
      }
      return next;
    },
    provide: (field) => EditorView.decorations.from(field),
  });
}

const highlightField = linesField(setHighlight, AGENT_HIGHLIGHT_CLASS);
const pointField = linesField(setPoint, AGENT_POINT_CLASS);

const theme = EditorView.baseTheme({
  [`.${AGENT_HIGHLIGHT_CLASS}`]: {
    backgroundColor: "color-mix(in oklch, var(--ring) 16%, transparent)",
    boxShadow: "inset 3px 0 0 var(--ring)",
  },
  [`.${AGENT_POINT_CLASS}`]: {
    animation: `cm-agent-point ${AGENT_POINT_MS / 3}ms ease-out 3`,
  },
  "@keyframes cm-agent-point": {
    "0%": { backgroundColor: "color-mix(in oklch, var(--ring) 38%, transparent)" },
    "100%": { backgroundColor: "transparent" },
  },
  // A pulse is motion: with reduced motion the lines are simply marked for
  // as long as the pulse would have run.
  "@media (prefers-reduced-motion: reduce)": {
    [`.${AGENT_POINT_CLASS}`]: {
      animation: "none",
      backgroundColor: "color-mix(in oklch, var(--ring) 24%, transparent)",
    },
  },
});

export function agentMarks() {
  return [highlightField, pointField, theme];
}

/** Draw the highlight over `span`, replacing any other, or remove it. */
export function highlightLines(view: EditorView, span: LineSpan | null): void {
  view.dispatch({ effects: setHighlight.of(span) });
}

/**
 * Bring lines into view near the top — the top of the note for `null` — and,
 * with `caret`, put the caret at the start of the first line. Not an edit and
 * not in the undo history; focus stays wherever the person had it.
 */
export function revealLines(view: EditorView, span: LineSpan | null, caret: boolean): void {
  const at = span === null ? 0 : offsetsOf(view.state.doc, span).from;
  view.dispatch({
    selection: caret ? { anchor: at } : undefined,
    effects: EditorView.scrollIntoView(at, { y: "start", yMargin: 24 }),
  });
}

/** Pulse over `span`, centred in view, then let it go. */
export function pointAtLines(view: EditorView, span: LineSpan): void {
  const { from } = offsetsOf(view.state.doc, span);
  view.dispatch({
    effects: [setPoint.of(span), EditorView.scrollIntoView(from, { y: "center" })],
  });
  window.setTimeout(() => {
    // The panel may have closed meanwhile; a destroyed view takes no dispatch.
    if (view.dom.isConnected) {
      view.dispatch({ effects: setPoint.of(null) });
    }
  }, AGENT_POINT_MS);
}

/**
 * Replace `span`'s lines with `text`, as the person's own edit: one plain
 * transaction, so it is one ⌘Z, and the update listener reports it to the
 * autosave like typing.
 */
export function replaceLines(view: EditorView, span: LineSpan, text: string): void {
  const { from, to } = offsetsOf(view.state.doc, span);
  view.dispatch({
    changes: { from, to, insert: text },
    selection: { anchor: from + text.length },
    scrollIntoView: true,
  });
  // The person pressed Apply, and the strip goes with the proposal: focus
  // returns to the note, where the edit is, as after `insertAtCursor`.
  view.focus();
}
