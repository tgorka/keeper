import { cva, type VariantProps } from "class-variance-authority";
import { Slot } from "radix-ui";
import type * as React from "react";

import { FOCUS_RING, INVALID_RING } from "@/components/ui/focus-ring";
import { cn } from "@/lib/utils";

// The focus and invalid rings come from `focus-ring.ts`, and the story of why
// they are imported rather than written here is a correction of what this
// comment used to claim.
//
// What it got right: shadcn's `ring-3 ring-ring/50` blends to #96a880 over the
// light theme's card and measures 2.12:1, under the 3:1 WCAG 2.4.11 requires of
// a focus indicator, while passing at 3.14:1 in dark — the one-value-two-themes
// trap. Full strength measures 5.61:1 light / 8.73:1 dark.
//
// What it got wrong: it said the fix was made and the copies were redundant. The
// fix was made HERE, on one component, while `input`, `textarea`, `select`,
// `checkbox`, `switch`, `radio-group`, `input-group`, `tabs`, `scroll-area` and
// `badge` all kept the 2.12:1 default. A comment that says a problem is handled
// when it is handled in one of ten places is worse than no comment, because the
// next reader stops looking. The remaining 46 pasted copies across 22 files are
// not redundant either: every one of them is on a raw `<button>`, `<select>` or
// `<input>` rather than on this component, so each is the only ring its element
// has. They are a separate migration, not a leftover.
const buttonVariants = cva(
  [
    "group/button inline-flex shrink-0 items-center justify-center rounded-md border border-transparent bg-clip-padding text-sm font-medium whitespace-nowrap transition-all outline-none select-none active:not-aria-[haspopup]:translate-y-px disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
    FOCUS_RING,
    INVALID_RING,
  ],
  {
    variants: {
      variant: {
        // Hover is a step along the token ladder, not an opacity of the fill.
        // `bg-primary/80` composited to #76a44c over a pane in dark and to
        // #638046 in light, where `--primary-foreground` measured 3.99:1 — the
        // primary button lost AA the instant the pointer touched it, in one
        // theme only, which is the trap the note above is about. A mix toward
        // `--foreground` cannot repeat it: `--primary-foreground` tracks
        // `--background`, so a step toward the foreground is a step AWAY from
        // the label in both themes. Measured 10.00:1 dark / 6.84:1 light,
        // up from 9.35 / 6.04 at rest. The step itself is smaller than the one
        // it replaces (1.070 dark / 1.132 light against 1.446 / 1.515) because
        // the old one bought its size by walking the fill toward the backdrop,
        // which is the same move as walking it toward the label.
        default:
          "bg-primary text-primary-foreground hover:bg-[color-mix(in_oklch,var(--primary),var(--foreground)_12%)]",
        // An outline button is the QUIET SECONDARY ACTION. That decides both of
        // its colours: its surface is the raised token a selected row already
        // uses, and its edge is the one hairline every other boundary in the app
        // is drawn with. It differs from `secondary` by exactly one thing — the
        // drawn edge — which is the whole of what "outline" means here, and it is
        // why an outline button stays findable on a surface its own fill matches.
        //
        // It used to be none of that. `dark:bg-input/30` is translucent, so the
        // variant had no colour of its own: it computed to #181e1b on the ground,
        // #1c2420 on a pane and #212925 on a raised card — three surfaces for one
        // variant, and DESIGN.md names none of them (`surface` #141a17,
        // `surface-raised` #1b221e). A surface that is nearly a token reads as a
        // mistake; a surface that moves with its backdrop cannot be verified at
        // all. `dark:border-input` #303a34 is 38% more luminous than `--line`
        // #28312c, so every outline edge measured 1.50:1 against a pane where
        // every hairline measures 1.32:1. And `shadow-xs` was the only shadow in
        // content chrome, against "no shadows on panes" and the one raking light.
        // In light the old hover was `--muted`, which IS `--card`: hovering an
        // outline button in a pane header repainted it in the pane's own colour.
        outline:
          "border-border bg-secondary text-secondary-foreground hover:bg-[color-mix(in_oklch,var(--secondary),var(--foreground)_5%)] aria-[haspopup]:aria-expanded:bg-[color-mix(in_oklch,var(--secondary),var(--foreground)_5%)]",
        secondary:
          "bg-secondary text-secondary-foreground hover:bg-[color-mix(in_oklch,var(--secondary),var(--foreground)_5%)]",
        // `--muted` is `--card` in BOTH themes, so a ghost button hovering to
        // `muted` on a pane header repainted the pane header: 1.000:1, no hover
        // state at all on the surface most ghost buttons live on. The dark branch
        // spent `muted/50` on top of that, compositing to #111614 — another
        // surface DESIGN.md does not name. One step up the ladder instead, to the
        // surface a selected nav row already wears.
        //
        // The `aria-expanded` fill is gated on `aria-[haspopup]`, and both halves
        // of that spelling are load-bearing.
        //
        // WHY IT IS GATED: `aria-expanded` means two different things depending
        // on the control. On a popover trigger `true` is transient and worth
        // painting — a menu is open right now. On a DISCLOSURE it is the resting
        // state: an unfolded column is expanded, and unfolded is how the app
        // opens. Ungated, the fill lit every fold control in the window on first
        // paint — two Notes columns, the drawer, every open panel's chevron and
        // all three rail section headers — each looking pressed while doing
        // nothing, which is the inverse of the signal. All 18 hand-written
        // `aria-expanded` sites in this codebase are disclosures; the popup
        // triggers get theirs from Radix at runtime, which is what the gate reads.
        //
        // WHY THE BRACKETS: `aria-haspopup:` is not a Tailwind variant, so
        // Tailwind falls back to the boolean form and compiles it to
        // `[aria-haspopup="true"]`. `aria-haspopup` is not a boolean — Radix emits
        // `menu` on a dropdown trigger and `dialog` on a popover trigger, verified
        // against the installed version in `button.test.tsx`. The boolean spelling
        // therefore matches NOTHING, closing the gate completely and deleting the
        // open-menu fill while still emitting a rule that looks right in the
        // stylesheet. `aria-[haspopup]` is the presence selector, the same one the
        // base above already uses for `active:not-aria-[haspopup]`.
        ghost:
          "hover:bg-secondary hover:text-foreground aria-[haspopup]:aria-expanded:bg-secondary aria-[haspopup]:aria-expanded:text-foreground",
        // A destructive button is red BY ITS EDGE AND ITS LABEL, and it does not
        // tint. The tint it used to carry was the whole bug: `bg-destructive/10`
        // moves the surface toward the very hue the label is written in, so the
        // ratio can only fall — 4.57:1 dark / 4.77:1 light on a `--card` pane but
        // 4.18 / 4.37 on `--secondary`, and the hover `/20` measured 3.63-4.06
        // with `dark:hover:/30` at 3.11:1. No alpha fixes that, because bare
        // `--destructive` on `--secondary` is already 4.69:1 dark / 5.04:1 light:
        // the tint budget is ~0.2 of a ratio point wide. Untinted, the label
        // measures 5.46 / 5.10 / 4.69 dark and 5.94 / 5.52 / 5.04 light across
        // ground / surface / surface-raised, and the same value drawn as the edge
        // clears the 3:1 SC 1.4.11 asks of a control boundary on all six.
        //
        // The hover is therefore NEUTRAL — the surface every other quiet control
        // in this file steps to. Because it shares no hue with the label, the
        // label's contrast is independent of the hover state for the first time
        // (4.69:1 dark / 5.04:1 light, both above the floor).
        //
        // The focus override went with it. `ring-destructive/20` measured 1.28:1
        // dark / 1.36:1 light against a pane and `/40` 1.81 / 1.90 — the exact
        // defect the note at the top of this file says was fixed, surviving in
        // the one variant that overrode the fix. It now inherits the measured
        // ring: 5.61:1 light / 8.73:1 dark.
        destructive:
          "border-destructive text-destructive hover:bg-secondary aria-[haspopup]:aria-expanded:bg-secondary",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default:
          "h-9 gap-1.5 px-2.5 in-data-[slot=button-group]:rounded-md has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2",
        xs: "h-6 gap-1 rounded-[min(var(--radius-md),8px)] px-2 text-xs in-data-[slot=button-group]:rounded-md has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-3",
        sm: "h-8 gap-1 rounded-[min(var(--radius-md),10px)] px-2.5 in-data-[slot=button-group]:rounded-md has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5",
        lg: "h-10 gap-1.5 px-2.5 has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2",
        icon: "size-9",
        "icon-xs":
          "size-6 rounded-[min(var(--radius-md),8px)] in-data-[slot=button-group]:rounded-md [&_svg:not([class*='size-'])]:size-3",
        "icon-sm":
          "size-8 rounded-[min(var(--radius-md),10px)] in-data-[slot=button-group]:rounded-md",
        "icon-lg": "size-10",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Button({
  className,
  variant = "default",
  size = "default",
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean;
  }) {
  const Comp = asChild ? Slot.Root : "button";

  return (
    <Comp
      data-slot="button"
      data-variant={variant}
      data-size={size}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };
