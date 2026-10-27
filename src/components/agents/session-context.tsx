import { Gauge } from "lucide-react";
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

type ContextVm = AgentSessionViewVm["context"];

/**
 * The one Details panel for the session's context window. The context reading in the composer's
 * bar and `/context` both open it; `session-view.tsx` holds whether it is open. All counts,
 * rounding, labels and ring geometry come from keeper-core, which sends the context in every view
 * mode and says when it is unknown (owner ruling 2026-10-27).
 */
export function SessionContext({
  context,
  open,
  onOpenChange,
  children,
}: {
  /** Absent until the session's first view arrives. */
  context: ContextVm | undefined;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Where the reading lives: it must render inside this panel's root to be its trigger. */
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
          {context && <ContextBreakdown context={context} />}
        </section>
      </SheetContent>
    </Sheet>
  );
}

/**
 * The bar's context reading (t3.chat's `5.3k / 1M`): a ring and Rust's compact label, always
 * drawn — an empty ring and `—` until an answer has measured it. Render it inside
 * `SessionContext`.
 */
export function SessionContextTrigger({ context }: { context: ContextVm }) {
  return (
    <HoverHint label="Context window" detail={context.summary}>
      <SheetTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          data-slot="agent-context-reading"
          className="min-h-11 shrink-0 gap-1.5 px-2 text-muted-foreground"
          aria-label={`Context details: ${context.summary}`}
        >
          <span className="flex size-4 items-center justify-center">
            {context.meterPercent !== null && (
              <meter
                aria-label="Context window occupancy"
                min={0}
                max={100}
                value={context.meterPercent}
                aria-valuetext={context.summary}
                className="sr-only"
              />
            )}
            <span
              aria-hidden="true"
              data-slot="agent-context-ring"
              className="flex size-4 items-center justify-center rounded-full text-primary"
              style={{ background: context.ringBackground }}
            >
              <span className="size-2.5 rounded-full bg-background" />
            </span>
          </span>
          <span className="text-xs tabular-nums">{context.label}</span>
        </Button>
      </SheetTrigger>
    </HoverHint>
  );
}

function ContextBreakdown({ context }: { context: ContextVm }) {
  if (context.used === null) {
    return (
      <p className="flex items-start gap-2 text-muted-foreground text-sm">
        <Gauge aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
        <span>{context.summary}</span>
      </p>
    );
  }
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
