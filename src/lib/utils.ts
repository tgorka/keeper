import { type ClassValue, clsx } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

/**
 * The project's own font sizes, declared as `--text-*` in `src/index.css`.
 *
 * tailwind-merge has to be told about them. It classifies an unknown `text-*`
 * utility as a COLOUR, so `cn("text-meta", "text-foreground")` decided the two
 * were the same property and dropped `text-meta` — every badge that also
 * carried a colour silently rendered at the inherited 16px instead of 11px,
 * which is how the search bar's chips ended up larger than the prompt they
 * annotate. Registering the scale makes size and colour two axes again.
 */
const twMerge = extendTailwindMerge({
  extend: { classGroups: { "font-size": [{ text: ["display", "title", "meta"] }] } },
});

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
