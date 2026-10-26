import { CircleHelp } from "lucide-react";
import { type ReactNode, useRef } from "react";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { HoverHint } from "@/components/ui/tooltip";
import type { AgentSessionViewVm } from "@/lib/ipc/client";

type ContextVm = NonNullable<AgentSessionViewVm["context"]>;

/**
 * The one Details panel for the session's context window. The ring beside Send and `/context`
 * both open it; `session-view.tsx` holds whether it is open. All counts, rounding and ring
 * geometry come from keeper-core.
 */
export function SessionContext({
  context,
  verbose,
  open,
  onOpenChange,
  children,
}: {
  context: ContextVm | undefined;
  /** The verbose view mode: the one in which Rust sends an answer's details. */
  verbose: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Where the ring lives: it must render inside this panel's root to be its trigger. */
  children: ReactNode;
}) {
  // Focus returns to whatever opened the panel: the ring, or the message field `/context` ran in.
  const opener = useRef<HTMLElement | null>(null);
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      {children}
      <SheetContent
        className="min-w-0 overflow-y-auto data-[side=right]:w-[min(24rem,100vw)]"
        onOpenAutoFocus={() => {
          const active = document.activeElement;
          opener.current =
            active instanceof HTMLElement && active !== document.body ? active : null;
        }}
        onCloseAutoFocus={(event) => {
          const target = opener.current;
          opener.current = null;
          if (target?.isConnected) {
            event.preventDefault();
            target.focus();
          }
        }}
      >
        <SheetHeader className="pr-14">
          <SheetTitle>Details</SheetTitle>
          <SheetDescription>Context window · latest answer</SheetDescription>
        </SheetHeader>
        <section aria-label="Context breakdown" className="min-w-0 space-y-4 px-4 pb-6">
          {context ? (
            <ContextBreakdown context={context} />
          ) : (
            <p className="text-muted-foreground text-sm">
              {verbose
                ? "The latest answer has no context window measurement."
                : "The context window is one of an answer's details. Choose the Verbose view to see it."}
            </p>
          )}
        </section>
      </SheetContent>
    </Sheet>
  );
}

/** The ring beside Send; render it inside `SessionContext`. */
export function SessionContextTrigger({ context }: { context: ContextVm }) {
  return (
    <HoverHint label="Context window" detail={context.summary}>
      <SheetTrigger asChild>
        <Button
          variant="ghost"
          className="min-h-11 shrink-0 gap-2 px-2 text-muted-foreground"
          aria-label={`Context details: ${context.summary}`}
        >
          {context.ringBackground && context.meterPercent !== null ? (
            <span className="flex size-5 items-center justify-center">
              <meter
                aria-label="Context window occupancy"
                min={0}
                max={100}
                value={context.meterPercent}
                aria-valuetext={context.summary}
                className="sr-only"
              />
              <span
                aria-hidden="true"
                className="flex size-5 items-center justify-center rounded-full text-primary"
                style={{ background: context.ringBackground }}
              >
                <span className="size-3.5 rounded-full bg-background" />
              </span>
            </span>
          ) : (
            <CircleHelp className="size-4" aria-hidden="true" />
          )}
          <span className="text-xs">{context.percent ?? context.summary}</span>
        </Button>
      </SheetTrigger>
    </HoverHint>
  );
}

function ContextBreakdown({ context }: { context: ContextVm }) {
  return (
    <>
      <p className="break-words font-medium text-sm">{context.summary}</p>
      {context.window === null && (
        <p className="text-muted-foreground text-xs">The model's context window is unknown.</p>
      )}
      <p className="text-muted-foreground text-xs">
        Parts are byte-based estimates scaled to the last prompt count, not cumulative usage or
        cost.
      </p>
      <div className="divide-y divide-border rounded-lg border border-border px-3">
        {context.parts.map((part) => (
          <details key={part.label} className="min-w-0 py-3">
            <summary className="cursor-pointer rounded-sm text-sm focus-visible:outline-2 focus-visible:outline-ring">
              <span className="break-words">{part.label}</span>
              <span className="mt-1 block pl-4 text-muted-foreground text-xs tabular-nums">
                {part.tokens} · {part.share}
              </span>
            </summary>
            <p className="mt-2 pl-4 text-muted-foreground text-xs">{part.description}</p>
          </details>
        ))}
      </div>
    </>
  );
}
