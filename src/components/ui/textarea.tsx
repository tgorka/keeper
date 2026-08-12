import type * as React from "react";

import { FOCUS_RING, INVALID_RING } from "@/components/ui/focus-ring";
import { cn } from "@/lib/utils";

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
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
