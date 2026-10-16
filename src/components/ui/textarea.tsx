import type * as React from "react";

import { FOCUS_RING, INVALID_RING } from "@/components/ui/focus-ring";
import { NO_TEXT_SUBSTITUTION } from "@/lib/text-entry";
import { cn } from "@/lib/utils";

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      // No capital, correction or prediction keeper's person did not type
      // (Story 101.3); a caller may still override one.
      {...NO_TEXT_SUBSTITUTION}
      data-slot="textarea"
      className={cn(
        "flex field-sizing-content min-h-16 w-full rounded-md border border-input bg-transparent px-2.5 py-2 text-base transition-[color,box-shadow] outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50 md:text-sm dark:bg-input/30",
        FOCUS_RING,
        INVALID_RING,
        className,
      )}
      {...props}
    />
  );
}

export { Textarea };
