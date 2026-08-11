/**
 * The lamp: keeper's one status indicator (DESIGN.md → Shapes).
 *
 * A 6px round indicator with four states — live, idle, working, fault —
 * carried by **fill geometry**, with colour as the second channel only.
 *
 * This exists because the shipped status triad encoded its meaning on hue
 * alone. Its three tints sat at mutual luminance ratios of 1.03 / 1.52 / 1.47,
 * all far below the 3:1 that WCAG SC 1.4.1 requires of a non-text channel, and
 * the whole triad collapsed to ΔE 16.3 under protanopia. A lightness ladder
 * cannot rescue that: forcing three colours to AA against one background pins
 * them inside a narrow luminance band by definition, so any third tint is
 * necessarily close to the other two. The redundant channel therefore has to be
 * shape, and once status is legible by shape it stops owning green — which is
 * what freed the accent to be lichen.
 *
 * The four state words are shared 1:1 with the mark's aperture and the macOS
 * tray template family (`tray-{live,idle,working,fault}-template.png`), so the
 * icon, the tray and every dot in the app speak one vocabulary rather than
 * four drawings. Renaming a state here means renaming a shipped image; do not
 * do it casually.
 *
 * Geometry, not opacity or hue: every state survives a greyscale screenshot and
 * a 1-bit template rasterisation.
 */
import type * as React from "react";

import { cn } from "@/lib/utils";

/** The four states of the lamp, of the mark's aperture, and of the tray. */
export type LampState = "live" | "idle" | "working" | "fault";

/**
 * The word a screen reader gets for each state when the call site has no better
 * one of its own.
 *
 * Generic on purpose: a bridge says "Connected", a recording says "Recording",
 * and both are lamps. These are the fallbacks, not the vocabulary.
 */
export const LAMP_STATE_WORD: Record<LampState, string> = {
  live: "Live",
  idle: "Idle",
  working: "Working",
  fault: "Fault",
};

/**
 * The default tint for each state — the second channel, never the only one.
 *
 * The `--bridge-*` tokens are the app's `ok` / `warn` / `danger` ramp under
 * their original names, and they are hue-separated from the accent on purpose
 * (DESIGN.md → Colors). A call site with a state colour of its own — the
 * recording red, say — overrides this through `className`, because the lamp
 * paints from `currentColor`.
 */
const LAMP_TONE_CLASS: Record<LampState, string> = {
  live: "text-bridge-healthy",
  // Full-strength, not `/50`. A 6px ring at half opacity measures 2.45:1 on
  // light and cannot be found on the surface at all, which fails SC 1.4.11 for
  // the non-text contrast of an indicator — and an indicator nobody can see is
  // not a redundant channel, it is an absent one.
  idle: "text-muted-foreground",
  working: "text-bridge-degraded",
  fault: "text-bridge-disconnected",
};

/**
 * The geometry of each state, in a 12-unit box painted at 6px.
 *
 * - `live` — a solid disc.
 * - `idle` — a ring; the ground shows through the middle.
 * - `working` — the same ring, dashed. The circumference of an r=4 ring is
 *   25.13 units, so a 3.14/3.14 dash pattern lands exactly four dashes with no
 *   ragged final segment at any size. A dash pattern rather than a lower
 *   opacity, because opacity is not a channel in monochrome.
 * - `fault` — the solid disc with a bite taken out of its trailing edge. One
 *   path: the long way round the r=5 disc from (10.1, 3.14) to (10.1, 8.86),
 *   then a concave r=3 arc back through (8, 6). The bite is on the RIGHT to
 *   mirror the mark's aperture, and is deliberately not on the left, which is the
 *   margin every hole on the tag is aligned to and means something else.
 *
 * Distinct markup per state is the contract this component exists to keep. Two
 * states sharing a shape is the defect coming back.
 */
const LAMP_SHAPE: Record<LampState, React.ReactElement> = {
  live: <circle cx="6" cy="6" r="5" fill="currentColor" />,
  idle: <circle cx="6" cy="6" r="4" fill="none" stroke="currentColor" strokeWidth="2" />,
  working: (
    <circle
      cx="6"
      cy="6"
      r="4"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeDasharray="3.14 3.14"
    />
  ),
  fault: <path d="M10.1 3.14A5 5 0 1 0 10.1 8.86A3 3 0 0 1 10.1 3.14Z" fill="currentColor" />,
};

export interface LampProps extends Omit<React.ComponentProps<"span">, "children"> {
  /** Which of the four states this lamp is showing. */
  state: LampState;
  /**
   * The word a screen reader gets, rendered as real `sr-only` text beside the
   * glyph — the house pattern for an `aria-hidden` mark with a meaning
   * (`sync-pane`, `recording-row`). Never `title` alone: `title` is not
   * reliably announced and is unreachable by keyboard.
   *
   * Omit it to get {@link LAMP_STATE_WORD}. Pass `null` — explicitly — where
   * the state is already spoken elsewhere, so a reader is not told it twice.
   * `null` is a decision a reviewer can grep for; a missing prop is just a bug.
   *
   * **`null` is REQUIRED whenever the lamp sits inside a control whose name is
   * computed** — a button, a menu item, anything with `aria-label`. Two things
   * go wrong otherwise, and both are silent. An explicit `aria-label` on the
   * ancestor REPLACES its contents, so the word is announced to nobody. And
   * where the name is built from contents instead, the accessible-name
   * algorithm trims each text node before concatenating them, so a lamp beside
   * a row label yields "BridgesDisconnected" — one unreadable token. Neither
   * is something a lamp can fix from the inside: the owning control has to put
   * the state into its own name. `sidebar-pane`, `chat-row`,
   * `phone-inbox-header` and `account-footer` all do exactly that.
   */
  label?: string | null;
}

export function Lamp({ state, label, className, ...props }: LampProps) {
  const word = label === undefined ? LAMP_STATE_WORD[state] : label;
  return (
    <span
      data-slot="lamp"
      data-state={state}
      className={cn("inline-flex shrink-0 items-center", LAMP_TONE_CLASS[state], className)}
      {...props}
    >
      <svg
        aria-hidden="true"
        viewBox="0 0 12 12"
        className="size-1.5 shrink-0 overflow-visible"
        focusable="false"
      >
        {LAMP_SHAPE[state]}
      </svg>
      {word !== null && <span className="sr-only">{word}</span>}
    </span>
  );
}
