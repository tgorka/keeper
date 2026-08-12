import { cva, type VariantProps } from "class-variance-authority";
import { Slot } from "radix-ui";
import type * as React from "react";

import { FOCUS_RING, INVALID_RING } from "@/components/ui/focus-ring";
import { cn } from "@/lib/utils";

// A badge is a chip, not a control, but it spends colour by exactly the same
// rules as `button.tsx` and it had exactly the same faults, because both are the
// same copied shadcn defaults: `bg-primary/80` and `bg-secondary/80` hovers that
// are composites rather than tokens, a `bg-destructive/10` tint drawn in the hue
// of the label sitting on it, and `hover:bg-muted`, which IS `--card` in both
// themes and so repainted a pane header in its own colour.
//
// Two badge-specific ones on top of those. Its `outline` and `ghost` variants
// hovered to `text-muted-foreground`, so the text got QUIETER as the pointer
// arrived — the affordance ran backwards. And its invalid state set
// `aria-invalid:ring-destructive/20` with no `ring-N` anywhere, so it had a ring
// colour and no ring: that declaration had never rendered a pixel.
const badgeVariants = cva(
  [
    "group/badge inline-flex h-5 w-fit shrink-0 items-center justify-center gap-1 overflow-hidden rounded-4xl border border-transparent px-2 py-0.5 text-xs font-medium whitespace-nowrap transition-all has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&>svg]:pointer-events-none [&>svg]:size-3!",
    FOCUS_RING,
    INVALID_RING,
  ],
  {
    variants: {
      variant: {
        default:
          "bg-primary text-primary-foreground [a]:hover:bg-[color-mix(in_oklch,var(--primary),var(--foreground)_12%)]",
        secondary:
          "bg-secondary text-secondary-foreground [a]:hover:bg-[color-mix(in_oklch,var(--secondary),var(--foreground)_5%)]",
        destructive: "border-destructive text-destructive [a]:hover:bg-secondary",
        outline: "border-border text-foreground [a]:hover:bg-secondary",
        ghost: "hover:bg-secondary hover:text-foreground",
        link: "text-primary underline-offset-4 hover:underline",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

function Badge({
  className,
  variant = "default",
  asChild = false,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot.Root : "span";

  return (
    <Comp
      data-slot="badge"
      data-variant={variant}
      className={cn(badgeVariants({ variant }), className)}
      {...props}
    />
  );
}

export { Badge, badgeVariants };
