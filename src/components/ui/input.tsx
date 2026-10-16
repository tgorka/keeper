import type * as React from "react";

import { FOCUS_RING, INVALID_RING } from "@/components/ui/focus-ring";
import { NO_TEXT_SUBSTITUTION } from "@/lib/text-entry";
import { cn } from "@/lib/utils";

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      // No capital, correction or prediction keeper's person did not type
      // (Story 101.3); a caller may still override one.
      {...NO_TEXT_SUBSTITUTION}
      type={type}
      data-slot="input"
      className={cn(
        "h-9 w-full min-w-0 rounded-md border border-input bg-transparent px-2.5 py-1 text-base transition-[color,box-shadow] outline-none file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm dark:bg-input/30",
        FOCUS_RING,
        INVALID_RING,
        className,
      )}
      {...props}
    />
  );
}

export { Input };
