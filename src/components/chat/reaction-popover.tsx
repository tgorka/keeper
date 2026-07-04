/**
 * Curated-emoji reaction Popover (Story 3.5, FR-12).
 *
 * A discoverable "Add reaction" entry point in the per-message action bar: a
 * ghost trigger (a Smile icon) opens a small Popover with a static, curated set of
 * ~8–12 common emoji. Picking one fires `onPick(emoji)` and closes the popover.
 * There is NO emoji-picker dependency — the set is a fixed constant. Purely
 * presentational: it holds no IPC or store knowledge (the parent wires `onPick`).
 */
import { Smile } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";

/**
 * The curated reaction set. Arbitrary Matrix reaction strings pass through
 * unchanged, so these emoji are sent verbatim as the reaction key.
 */
const CURATED_EMOJI = ["👍", "❤️", "😂", "😮", "😢", "🎉", "🙏", "🔥"] as const;

interface ReactionPopoverProps {
  /** Fired with the chosen emoji when the user picks one; closes the popover. */
  onPick: (emoji: string) => void;
}

export function ReactionPopover({ onPick }: ReactionPopoverProps) {
  const [open, setOpen] = useState(false);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button type="button" variant="ghost" size="icon-xs" aria-label="Add reaction">
          <Smile aria-hidden="true" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        className="w-auto flex-row gap-0.5 p-1"
        aria-label="Pick a reaction"
      >
        {CURATED_EMOJI.map((emoji) => (
          <Button
            key={emoji}
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label={`React with ${emoji}`}
            onClick={() => {
              onPick(emoji);
              setOpen(false);
            }}
          >
            <span aria-hidden="true" className="text-base leading-none">
              {emoji}
            </span>
          </Button>
        ))}
      </PopoverContent>
    </Popover>
  );
}
