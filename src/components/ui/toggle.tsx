import { cva, type VariantProps } from "class-variance-authority";
import { Toggle as TogglePrimitive } from "radix-ui";
import type * as React from "react";

import { FOCUS_RING } from "@/components/ui/focus-ring";
import { cn } from "@/lib/utils";

// A pressed toggle is the tab trigger's lifted segment: the raised token a
// selected row wears, bounded by the one hairline — never a translucent fill
// that would compute to a surface DESIGN.md does not name (see `tabs.tsx`).
const toggleVariants = cva(
  [
    "inline-flex shrink-0 items-center justify-center gap-1 rounded-md border border-transparent text-sm font-medium whitespace-nowrap text-muted-foreground outline-none transition-colors hover:text-foreground disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
    "data-[state=on]:border-border data-[state=on]:bg-secondary data-[state=on]:text-foreground",
    FOCUS_RING,
  ],
  {
    variants: {
      size: {
        default: "h-8 min-w-8 px-1.5",
        sm: "h-7 min-w-7 px-1",
      },
    },
    defaultVariants: {
      size: "default",
    },
  },
);

function Toggle({
  className,
  size,
  ...props
}: React.ComponentProps<typeof TogglePrimitive.Root> & VariantProps<typeof toggleVariants>) {
  return (
    <TogglePrimitive.Root
      data-slot="toggle"
      className={cn(toggleVariants({ size }), className)}
      {...props}
    />
  );
}

export { Toggle, toggleVariants };
