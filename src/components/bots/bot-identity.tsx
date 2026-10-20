import { useMemo, useState } from "react";
import {
  BOT_IDENTITY_COLOURS,
  BOT_IDENTITY_SHAPES,
  BotIdentityCell,
  type BotIdentityFields,
  botIdentityPhrase,
} from "@/components/agents/identity";
import { IconChoice } from "@/components/notes/space-editor";
import { matchSpaceIcons } from "@/components/notes/space-icons";
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
import { logFailure } from "@/lib/failure";
import type { BotVm } from "@/lib/ipc/client";
import { botsBotIdentitySave } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

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
      logFailure("bots.identitySave")(raw);
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
