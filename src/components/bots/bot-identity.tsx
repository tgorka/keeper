/**
 * A bot's identity: a shape, a mark, and — only ever beside a shape — a colour
 * (Story 61.7, FR-383).
 *
 * **The colour palette is bounded, and that is the story rather than a
 * shortcut.** `DESIGN.md` records the arithmetic: AA needs L\* ≤ 46.8 on warm
 * paper and L\* ≥ 51.9 on near-black, so the intersection is empty and no
 * colour of any hue passes in both themes. A free colour picker therefore
 * cannot be shipped honestly — half of what somebody could choose would be
 * unreadable in one of the two themes keeper ships, and the app would have
 * offered it. So {@link BOT_IDENTITY_COLOURS} is a closed set of token names,
 * the hexes live once in `src/index.css` with a value per theme, and
 * `scripts/check-design.mjs` recomputes every one of them against every surface
 * of both themes and fails the build on a member that drops below 4.5:1.
 *
 * **Colour is never the only carrier.** `DESIGN.md:172` requires colour to be
 * paired with a shape, so this component paints no ink at all until a shape is
 * chosen — and {@link botIdentityPhrase} says the shape and the mark out loud,
 * so two bots that differ only in colour are still two different bots to a
 * screen reader and to somebody who cannot tell clay from olive.
 *
 * **The shapes are the lamp's own fill language worn on the mark's cell**, not
 * a second vocabulary: filled, hollow, dashed and one with a bite taken out of
 * it (`DESIGN.md` → Shapes). Worn on the flat-topped hexagon rather than the
 * 6px disc, because a status in this app is round and small and a bot is a
 * cell — so an identity can never be misread as a state. `DESIGN.md`'s "a bot
 * in keeper is a kept instrument, and it wears the same cell" is the sentence
 * this file implements, and it is also why no bot grows a face: the eyes are
 * the mark's and only the mark's.
 *
 * **The mark reuses `space-icons.ts`**, which is the app's existing answer to
 * "a user picks one glyph from a closed curated set", down to the same
 * {@link IconChoice} button. It did not already support a typed grapheme, so
 * this adds one — bounded to a single mark in Rust, and content rather than
 * chrome: `DESIGN.md`'s emoji ban is a ban on keeper decorating itself, not on
 * the product holding what a person typed, exactly as the reaction picker is.
 */
import { type ReactElement, useMemo, useState } from "react";
import { IconChoice } from "@/components/notes/space-editor";
import { matchSpaceIcons, SPACE_ICONS, spaceIcon } from "@/components/notes/space-icons";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { BotVm } from "@/lib/ipc/client";
import { botsBotIdentitySave } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/**
 * The closed set of shapes, in picker order.
 *
 * Must equal `keeper_core::bots::identity::BOT_SHAPES` exactly, and
 * `scripts/check-design.mjs` checks that it does: a shape the picker offers and
 * Rust refuses is a save that fails after the person chose.
 */
export const BOT_IDENTITY_SHAPES = ["filled", "hollow", "dashed", "notched"] as const;

/** One of the four shapes a bot's cell can be drawn in. */
export type BotIdentityShape = (typeof BOT_IDENTITY_SHAPES)[number];

/**
 * The bounded colour palette, as token names, in wheel order.
 *
 * Seven inks, each a material a workroom holds. The wheel skips the lichen
 * band on purpose — the accent is singular (`DESIGN.md` → "No second green"),
 * so no bot may wear it — and purple is absent because purple is banned
 * outright. `scripts/check-design.mjs` reads THIS list, so adding a name here
 * without adding its two hexes to `src/index.css` fails the build rather than
 * shipping a bot nothing can draw.
 */
export const BOT_IDENTITY_COLOURS = [
  "clay",
  "ochre",
  "olive",
  "verdigris",
  "steel",
  "lapis",
  "madder",
] as const;

/** One of the seven inks a bot's cell can be drawn in. */
export type BotIdentityColour = (typeof BOT_IDENTITY_COLOURS)[number];

/**
 * The utility per ink.
 *
 * Spelled out rather than interpolated, because Tailwind sees source text: a
 * `text-bot-ink-${name}` template produces no CSS at all, and the cells would
 * all render in `currentColor` while every test that read the class name
 * passed.
 */
const INK_CLASS: Record<BotIdentityColour, string> = {
  clay: "text-bot-ink-clay",
  ochre: "text-bot-ink-ochre",
  olive: "text-bot-ink-olive",
  verdigris: "text-bot-ink-verdigris",
  steel: "text-bot-ink-steel",
  lapis: "text-bot-ink-lapis",
  madder: "text-bot-ink-madder",
};

/**
 * The flat-topped hexagonal cell, in a 24-unit box.
 *
 * Flat-topped and not domed, for the mark's own reason (`DESIGN.md` →
 * Components): a dome head with antennae is the bugdroid's silhouette and
 * Android's trademark forbids derivatives of it. Every vertex is on a whole
 * unit so the outline lands on whole pixels at 24px and at 12px.
 */
const CELL = "M7 2H17L22 12L17 22H7L2 12Z";

/**
 * The same cell with a wedge cut out of its right side — the lamp's `fault`
 * geometry, which is a filled disc with a bite taken out of its trailing edge.
 * The bite is on the RIGHT for the lamp's reason: the left is the margin
 * everything else is aligned to and means something else.
 */
const CELL_NOTCHED = "M7 2H17L19.5 7L16 12L19.5 17L17 22H7L2 12Z";

/**
 * The geometry per shape. Distinct markup per shape is the contract this
 * component exists to keep — two shapes sharing a drawing is colour becoming
 * the only carrier again, by accident.
 *
 * The dash pattern is arithmetic, not taste: the cell's perimeter is
 * 10 + 10 + 4 × 11.18 = 64.72 units, so 4.045/4.045 lands exactly eight dashes
 * with no ragged final segment at any size. The lamp's ring is dashed for the
 * same reason and by the same method.
 */
const SHAPE_GEOMETRY: Record<BotIdentityShape, ReactElement> = {
  filled: <path d={CELL} fill="currentColor" />,
  hollow: <path d={CELL} fill="none" stroke="currentColor" strokeWidth="2" />,
  dashed: (
    <path
      d={CELL}
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeDasharray="4.045 4.045"
    />
  ),
  notched: <path d={CELL_NOTCHED} fill="currentColor" />,
};

/** Whether `value` is a shape this build draws. */
export function isBotIdentityShape(value: string | null): value is BotIdentityShape {
  return value !== null && (BOT_IDENTITY_SHAPES as readonly string[]).includes(value);
}

/**
 * Whether `value` is a member of the bounded palette.
 *
 * A name from outside it is treated as **absent**, never as a colour to try
 * drawing: a row written by a newer build names an ink this one has no verified
 * hex for, and painting it as `currentColor` would silently claim an identity
 * the person did not choose. The stored value is untouched — `spaceIcon`'s rule
 * for an unknown icon name, for the same reason.
 */
export function isBotIdentityColour(value: string | null): value is BotIdentityColour {
  return value !== null && (BOT_IDENTITY_COLOURS as readonly string[]).includes(value);
}

/** The three stored identity fields, which is all this module needs of a bot. */
export interface BotIdentityFields {
  shape: string | null;
  colour: string | null;
  mark: string | null;
}

/**
 * The identity, in words — the sentence every control carrying a cell puts into
 * its own accessible name.
 *
 * This is the accessibility half of "colour is never the only carrier". Two
 * bots differing only in their ink read as two different identities here,
 * because the ink is named; and a person who cannot tell the two inks apart
 * still has the shape and the mark, which are named first.
 *
 * A shape this build cannot draw says so rather than being dropped silently:
 * an identity that is *there* and unrenderable is a different fact from no
 * identity, and AD-27's posture is that an unknown is never rendered as a
 * `false`.
 */
export function botIdentityPhrase(identity: BotIdentityFields): string {
  const parts: string[] = [];
  if (isBotIdentityShape(identity.shape)) {
    parts.push(
      isBotIdentityColour(identity.colour)
        ? `${identity.shape} cell in ${identity.colour}`
        : `${identity.shape} cell`,
    );
  } else if (identity.shape !== null && identity.shape.trim() !== "") {
    parts.push("a shape this version of keeper cannot draw");
  }
  if (identity.mark !== null && identity.mark.trim() !== "") {
    // An icon's name reads as words; a typed grapheme is named as the
    // character it is, so "marked K" is not heard as an icon called K.
    parts.push(
      identity.mark in SPACE_ICONS
        ? `marked ${identity.mark.replace(/-/g, " ")}`
        : `marked the character ${identity.mark}`,
    );
  }
  return parts.length === 0 ? "no identity chosen" : parts.join(", ");
}

/** The accessible name of a control that switches to `bot`. */
export function botPinLabel(bot: BotVm): string {
  return `${bot.name} — ${botIdentityPhrase(bot)}`;
}

/**
 * The cell itself: shape, ink, mark.
 *
 * `aria-hidden`, always. The identity is spoken by the control that owns it
 * through {@link botPinLabel} — the lamp's rule, and for the lamp's measured
 * reason: an `aria-label` on the ancestor replaces its contents, so a name
 * rendered in here would be announced to nobody, and where the name is built
 * from contents instead the algorithm concatenates trimmed text nodes into one
 * unreadable token.
 */
export function BotIdentityCell({
  identity,
  className,
}: {
  identity: BotIdentityFields;
  className?: string;
}) {
  const shape = isBotIdentityShape(identity.shape) ? identity.shape : null;
  // The pairing, in one expression: no shape, no ink. Removing this guard is
  // exactly the defect `DESIGN.md:172` bans — a bare coloured dot.
  const ink = shape !== null && isBotIdentityColour(identity.colour) ? identity.colour : null;
  const mark = identity.mark !== null && identity.mark.trim() !== "" ? identity.mark : null;
  const Glyph = mark !== null && mark in SPACE_ICONS ? spaceIcon(mark) : null;
  return (
    <span
      aria-hidden="true"
      data-slot="bot-identity"
      data-shape={shape ?? "none"}
      data-colour={ink ?? "none"}
      className={cn(
        "relative inline-flex size-6 shrink-0 items-center justify-center",
        ink === null ? "text-muted-foreground" : INK_CLASS[ink],
        className,
      )}
    >
      {shape !== null && (
        <svg
          aria-hidden="true"
          viewBox="0 0 24 24"
          focusable="false"
          className="absolute inset-0 size-full"
        >
          {SHAPE_GEOMETRY[shape]}
        </svg>
      )}
      {mark !== null && (
        // On a filled cell the mark is cut out of the ink, so it is drawn in the
        // surface colour — which is legible by construction, because every ink
        // in the palette clears 4.5:1 against `--background` in both themes and
        // the gate re-checks it.
        <span
          className={cn(
            "relative inline-flex items-center justify-center",
            shape === "filled" || shape === "notched" ? "text-background" : undefined,
          )}
        >
          {Glyph === null ? (
            <span className="font-mono text-meta leading-none">{mark}</span>
          ) : (
            <Glyph className="size-3" />
          )}
        </span>
      )}
    </span>
  );
}

/** The copy the picker opens with, exported so the test names it once. */
export const BOT_IDENTITY_DIALOG_TITLE = "Bot identity";
export const BOT_IDENTITY_DIALOG_BLURB =
  "A shape and a mark first, a colour second. The colours are a fixed set because keeper only offers inks it has checked for contrast in both themes.";
/** The one-line reason there is no colour wheel here. */
export const BOT_IDENTITY_NO_PICKER_NOTE =
  "No free colour picker: no single colour passes contrast in both the light and the dark theme, so keeper offers the seven that do.";
/** What a colour chosen with no shape is told, before anything is sent. */
export const BOT_IDENTITY_NEEDS_SHAPE =
  "A colour needs a shape beside it — colour alone is not something everyone can see, so choose a shape too.";
/** The mark field's own honest bound. */
export const BOT_IDENTITY_MARK_HINT = "One character, or pick an icon below.";
/** How many icon matches the picker draws before it says there are more. */
export const BOT_IDENTITY_ICON_LIMIT = 24;

/**
 * The editor: shape, colour, mark, and a live preview of the three together.
 *
 * Every refusal is Rust's sentence, rendered verbatim — except the one this
 * dialog can answer without asking, which is a colour with no shape. Answering
 * it here is not a second rule: the same rule is enforced in
 * `keeper_core::bots::identity::parse_identity`, and this is the affordance
 * saying so before the person presses Save.
 */
export function BotIdentityPicker({
  bot,
  open,
  onOpenChange,
  onSaved,
}: {
  bot: BotVm;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSaved: (bot: BotVm) => void;
}) {
  const [shape, setShape] = useState<string | null>(bot.shape);
  const [colour, setColour] = useState<string | null>(bot.colour);
  const [mark, setMark] = useState<string>(bot.mark ?? "");
  const [iconQuery, setIconQuery] = useState("");
  const [refusal, setRefusal] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const iconMatches = useMemo(() => {
    const flat = matchSpaceIcons(iconQuery).flatMap((group) => Object.keys(group.icons));
    return { shown: flat.slice(0, BOT_IDENTITY_ICON_LIMIT), total: flat.length };
  }, [iconQuery]);

  const trimmedMark = mark.trim();
  const preview: BotIdentityFields = {
    shape,
    colour,
    mark: trimmedMark === "" ? null : trimmedMark,
  };
  const needsShape = colour !== null && shape === null;

  const save = async () => {
    if (needsShape) {
      setRefusal(BOT_IDENTITY_NEEDS_SHAPE);
      return;
    }
    setSaving(true);
    setRefusal(null);
    try {
      const saved = await botsBotIdentitySave(
        bot.id,
        shape,
        colour,
        trimmedMark === "" ? null : trimmedMark,
      );
      onSaved(saved);
      onOpenChange(false);
    } catch (raw: unknown) {
      setRefusal(syncErrorMessage(raw, "keeper couldn't save this identity."));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[85vh] flex-col gap-4 overflow-hidden sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{BOT_IDENTITY_DIALOG_TITLE}</DialogTitle>
          <DialogDescription>{BOT_IDENTITY_DIALOG_BLURB}</DialogDescription>
        </DialogHeader>

        <div className="flex items-center gap-3">
          <BotIdentityCell identity={preview} />
          <p className="min-w-0 text-muted-foreground text-sm">{botIdentityPhrase(preview)}</p>
        </div>

        <div className="flex flex-col gap-4 overflow-y-auto">
          <div className="flex flex-col gap-2">
            <Label>Shape</Label>
            {/* biome-ignore lint/a11y/useSemanticElements: `<fieldset>` is the
                semantic form-grouping element and this is a two-state button
                grid inside a dialog that already owns the form. */}
            <div role="group" aria-label="Shape" className="flex flex-wrap gap-2">
              {BOT_IDENTITY_SHAPES.map((option) => (
                <button
                  key={option}
                  type="button"
                  aria-pressed={shape === option}
                  aria-label={`${option} cell`}
                  onClick={() => setShape(shape === option ? null : option)}
                  className={cn(
                    "rounded-md border p-2 outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    shape === option ? "border-ring bg-accent" : "border-input",
                  )}
                >
                  <BotIdentityCell identity={{ shape: option, colour, mark: null }} />
                </button>
              ))}
            </div>
          </div>

          <div className="flex flex-col gap-2">
            <Label>Colour</Label>
            {/* biome-ignore lint/a11y/useSemanticElements: as above — a legend
                cannot be the styled `Label` the other fields share. */}
            <div role="group" aria-label="Colour" className="flex flex-wrap gap-2">
              {BOT_IDENTITY_COLOURS.map((option) => (
                <button
                  key={option}
                  type="button"
                  aria-pressed={colour === option}
                  aria-label={option}
                  onClick={() => setColour(colour === option ? null : option)}
                  className={cn(
                    "rounded-md border p-2 outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    colour === option ? "border-ring bg-accent" : "border-input",
                  )}
                >
                  <BotIdentityCell
                    identity={{ shape: shape ?? "filled", colour: option, mark: null }}
                  />
                </button>
              ))}
            </div>
            <p className="text-muted-foreground text-meta">{BOT_IDENTITY_NO_PICKER_NOTE}</p>
            {needsShape && (
              <p role="alert" className="text-destructive text-sm">
                {BOT_IDENTITY_NEEDS_SHAPE}
              </p>
            )}
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="bot-identity-mark">Mark</Label>
            <Input
              id="bot-identity-mark"
              value={mark}
              onChange={(event) => setMark(event.target.value)}
              placeholder={BOT_IDENTITY_MARK_HINT}
            />
            <Input
              aria-label="Search icons"
              value={iconQuery}
              onChange={(event) => setIconQuery(event.target.value)}
              placeholder="Search icons"
            />
            {/* biome-ignore lint/a11y/useSemanticElements: `<fieldset>` is the
                semantic grouping element and this is a button grid inside a
                dialog that already owns the form — `space-editor.tsx`'s
                chooser makes the same call for the same grid. */}
            <div role="group" aria-label="Icons" className="flex flex-wrap gap-2">
              <IconChoice
                name={null}
                selected={trimmedMark === ""}
                onSelect={() => setMark("")}
                label="No mark"
              />
              {iconMatches.shown.map((name) => (
                <IconChoice
                  key={name}
                  name={name}
                  selected={trimmedMark === name}
                  onSelect={() => setMark(name)}
                  label={name.replace(/-/g, " ")}
                />
              ))}
            </div>
            {iconMatches.total > iconMatches.shown.length && (
              <p className="text-muted-foreground text-meta">
                {iconMatches.total - iconMatches.shown.length} more icons match — narrow the search
                to see them.
              </p>
            )}
          </div>
        </div>

        {refusal !== null && (
          <p role="alert" className="text-destructive text-sm">
            {refusal}
          </p>
        )}

        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="button" disabled={saving} onClick={() => void save()}>
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
