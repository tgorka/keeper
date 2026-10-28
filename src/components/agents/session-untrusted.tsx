import { ShieldAlert } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import type { AgentUntrustedVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/**
 * The session-state chip on the header's meta line for a session that read outside content
 * (D-34). Its popover says Rust's sentence and, where Rust offers it (never on an archived
 * session, where `/clean` refuses), the honest way out: a fresh chat with the same agent, which
 * starts trusted and keeps the drives (Q109-17 b). The mark on this chat never lowers (V13);
 * nothing here pretends to change it.
 */
export function SessionUntrusted({
  untrusted,
  phone,
  onFreshChat,
}: {
  untrusted: AgentUntrustedVm;
  phone: boolean;
  /** Runs `/clean` through the composer's command path; rejects with the sentence to show. */
  onFreshChat: () => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const fresh = untrusted.freshChat;
  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setFailure(null);
      }}
    >
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="xs"
          data-slot="agent-untrusted-marker"
          className={cn("h-auto gap-1 px-1.5 py-0.5 text-held text-xs", phone && "min-h-11")}
        >
          <ShieldAlert aria-hidden="true" className="size-3.5" />
          Untrusted
          <span className="sr-only">: {untrusted.sentence}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        aria-label="Why commands ask"
        className="flex w-[min(20rem,calc(100vw-1.5rem))] flex-col gap-2 text-sm"
      >
        <p>{untrusted.sentence}</p>
        {fresh && (
          <Button
            variant="outline"
            size="sm"
            className="h-auto min-h-11 whitespace-normal text-left"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              setFailure(null);
              try {
                await onFreshChat();
                setOpen(false);
              } catch (raw) {
                setFailure(raw instanceof Error ? raw.message : String(raw));
              } finally {
                setBusy(false);
              }
            }}
          >
            {fresh}
          </Button>
        )}
        {failure && (
          <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
            {failure}
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}
