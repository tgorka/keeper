import { ScrollArea as ScrollAreaPrimitive } from "radix-ui";
import type * as React from "react";

import { FOCUS_RING } from "@/components/ui/focus-ring";
import { cn } from "@/lib/utils";

function ScrollArea({
  className,
  children,
  fitWidth = false,
  ...props
}: React.ComponentProps<typeof ScrollAreaPrimitive.Root> & {
  /**
   * Keep the content inside the viewport's width instead of letting it grow.
   *
   * Radix renders the viewport's only child as `display: table`, so its width
   * is `max-content`: the content never wraps or truncates to the pane, it
   * widens the pane and scrolls sideways. That is right for something that must
   * not reflow — a wide table — and wrong for a column of cards, where it means
   * the longest path in a list decides how far off-screen the buttons sit.
   *
   * Off by default, because switching it globally would silently take
   * horizontal scrolling away from surfaces that may be relying on it.
   */
  fitWidth?: boolean;
}) {
  return (
    <ScrollAreaPrimitive.Root
      data-slot="scroll-area"
      className={cn("relative", className)}
      {...props}
    >
      <ScrollAreaPrimitive.Viewport
        data-slot="scroll-area-viewport"
        className={cn(
          "size-full rounded-[inherit] transition-[color,box-shadow] outline-none",
          // `!` because the display comes from Radix's own inline style.
          fitWidth && "[&>div]:!block [&>div]:!w-full",
          FOCUS_RING,
        )}
      >
        {children}
      </ScrollAreaPrimitive.Viewport>
      <ScrollBar />
      <ScrollAreaPrimitive.Corner />
    </ScrollAreaPrimitive.Root>
  );
}

function ScrollBar({
  className,
  orientation = "vertical",
  ...props
}: React.ComponentProps<typeof ScrollAreaPrimitive.ScrollAreaScrollbar>) {
  return (
    <ScrollAreaPrimitive.ScrollAreaScrollbar
      data-slot="scroll-area-scrollbar"
      data-orientation={orientation}
      orientation={orientation}
      className={cn(
        "flex touch-none p-px transition-colors select-none data-horizontal:h-2.5 data-horizontal:flex-col data-horizontal:border-t data-horizontal:border-t-transparent data-vertical:h-full data-vertical:w-2.5 data-vertical:border-l data-vertical:border-l-transparent",
        className,
      )}
      {...props}
    >
      <ScrollAreaPrimitive.ScrollAreaThumb
        data-slot="scroll-area-thumb"
        className="relative flex-1 rounded-full bg-border"
      />
    </ScrollAreaPrimitive.ScrollAreaScrollbar>
  );
}

export { ScrollArea, ScrollBar };
