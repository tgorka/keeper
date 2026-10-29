import { ChevronDown, HardDrive, X } from "lucide-react";
import { useId, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  type AgentSessionDrivesVm,
  type AgentSessionRef,
  agentNoteTickSeen,
  agentSessionDriveSet,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/**
 * The drives a session's agent may read (109.23): a chip per ticked drive and a check-item menu
 * of every drive Rust knows of here, each ticked, offered, or not allowed with Rust's reason.
 * Rust answers who may change it — this renders only what it is given, so for anyone but a
 * person member there is no `drives` and nothing is drawn. What ticking allows (D-34's accepted
 * risk, R-HQ-79) is Rust's sentence, shown in the menu where every tick happens and again under
 * the chips when a tick takes the selection from none to one. A ticked drive Rust gives a reason
 * stopped being allowed and holds every other change back: its chip says so, and its button
 * unticks every such drive at once (Rust's `set`), as the sentence under the chips says.
 *
 * A ticked drive whose current tick this person has not been shown the warning for on this device
 * (`drives.unseen`, each with its tick id: Rust's seen record, round 7 ruling — a tick the Notes
 * Assistant, another device or a carry made, one with no id, or one whose record was lost) draws
 * that warning under the chips, wherever this session is shown, with **Untick** (the person's own
 * untick) and **OK**, which names that tick so it acknowledges this one only. Both read the same
 * Rust view as the chips, so they cannot disagree.
 */
export function SessionDrives({
  target,
  agentName,
  drives,
  open,
  onOpenChange,
}: {
  target: AgentSessionRef;
  agentName: string;
  drives: AgentSessionDrivesVm;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [firstTick, setFirstTick] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const writing = useRef(false);
  const warningId = useId();
  const staleId = useId();
  const ticked = drives.drives.filter((drive) => drive.ticked);
  const failing = ticked.filter((drive) => drive.reason !== undefined);

  const change = async (name: string, tick: boolean) => {
    // Never the `disabled` attribute while a write is in flight: it would drop keyboard focus
    // out of the menu. A tick then is ignored instead.
    if (writing.current) return;
    writing.current = true;
    setBusy(name);
    setError(null);
    const none = ticked.length === 0;
    try {
      await agentSessionDriveSet(target, name, tick);
      if (tick && none) setFirstTick(true);
      if (!tick) setFirstTick(false);
    } catch (raw) {
      setError(syncErrorMessage(raw, "keeper could not change this chat's drives."));
    } finally {
      writing.current = false;
      setBusy(null);
    }
  };

  const seen = async (name: string, tick: string | null) => {
    if (writing.current) return;
    writing.current = true;
    setBusy(name);
    setError(null);
    try {
      await agentNoteTickSeen(target, name, tick);
    } catch (raw) {
      setError(syncErrorMessage(raw, "keeper could not note that."));
    } finally {
      writing.current = false;
      setBusy(null);
    }
  };

  const unseen = drives.unseen ?? [];
  const alert = error ?? drives.refused;
  return (
    <div data-slot="session-drives" className="min-w-0 pt-1">
      <div className="flex min-w-0 flex-wrap items-center gap-1">
        <DropdownMenu open={open} onOpenChange={onOpenChange}>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size="sm" className="min-h-11 shrink-0 sm:min-h-8">
              <HardDrive aria-hidden="true" />
              Drives
              <ChevronDown aria-hidden="true" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="start"
            className="w-80 max-w-[calc(100vw-1rem)]"
            aria-describedby={warningId}
          >
            <DropdownMenuLabel>Drives {agentName} may read in this chat</DropdownMenuLabel>
            <p
              id={warningId}
              className="px-2 pb-2 text-held text-xs [overflow-wrap:anywhere]"
              data-slot="session-drives-warning"
            >
              {drives.warning}
            </p>
            {/* Radix hides everything outside an open menu from assistive technology, and the
                menu stays open across ticks: a refusal arriving meanwhile is said in here. */}
            {open && alert && (
              <p
                role="alert"
                data-slot="session-drives-menu-alert"
                className="px-2 pb-2 text-destructive text-xs [overflow-wrap:anywhere]"
              >
                {alert}
              </p>
            )}
            <DropdownMenuSeparator />
            {drives.drives.map((drive) => {
              const blocked = !drive.ticked && drive.reason !== undefined;
              const noteId = `${warningId}-${drive.name}`;
              return (
                <DropdownMenuCheckboxItem
                  key={drive.name}
                  checked={drive.ticked}
                  disabled={blocked}
                  aria-busy={busy === drive.name}
                  aria-describedby={
                    drive.reason || drive.refused ? `${warningId} ${noteId}` : warningId
                  }
                  className="min-h-11 items-start"
                  onSelect={(event) => {
                    // Keep the menu open: several drives are ticked in one visit.
                    event.preventDefault();
                  }}
                  onCheckedChange={(checked) => void change(drive.name, checked === true)}
                >
                  <span className="flex min-w-0 flex-col gap-0.5">
                    <span className="break-all font-medium">{drive.name}</span>
                    {(drive.reason || drive.refused) && (
                      <span id={noteId} className="flex flex-col gap-0.5 text-xs">
                        {drive.reason && (
                          <span className="text-muted-foreground [overflow-wrap:anywhere]">
                            {drive.reason}
                          </span>
                        )}
                        {drive.refused && (
                          <span className="text-destructive [overflow-wrap:anywhere]">
                            {drive.refused}
                          </span>
                        )}
                      </span>
                    )}
                  </span>
                </DropdownMenuCheckboxItem>
              );
            })}
          </DropdownMenuContent>
        </DropdownMenu>
        <ul aria-label="Drives ticked" className="flex min-w-0 flex-wrap items-center gap-1">
          {ticked.length === 0 ? (
            <li className="text-muted-foreground text-xs">No drive ticked</li>
          ) : (
            ticked.map((drive) =>
              drive.reason ? (
                // Ticked before it stopped being allowed: it holds every other change back,
                // so its chip says so and unticks it in one step.
                <li
                  key={drive.name}
                  data-failing="true"
                  className="flex max-w-full items-center gap-1 rounded-full border border-destructive py-0.5 pr-0.5 pl-2 text-destructive text-xs"
                >
                  <span className="break-all">{drive.name}</span>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    className="size-6 rounded-full"
                    aria-label={`Untick ${drive.name}`}
                    aria-describedby={staleId}
                    aria-busy={busy === drive.name}
                    onClick={() => void change(drive.name, false)}
                  >
                    <X aria-hidden="true" />
                  </Button>
                </li>
              ) : (
                <li
                  key={drive.name}
                  className="max-w-full break-all rounded-full border border-border bg-secondary px-2 py-0.5 text-secondary-foreground text-xs"
                >
                  {drive.name}
                </li>
              ),
            )
          )}
          {drives.pending && (
            <li className="text-muted-foreground text-xs" aria-live="polite">
              Waiting for the server…
            </li>
          )}
        </ul>
      </div>
      {failing[0] && (
        // One sentence for all of them: Rust's, the same on each failing drive.
        <p id={staleId} className="pt-1 text-destructive text-xs [overflow-wrap:anywhere]">
          {failing[0].reason}
        </p>
      )}
      {unseen.map(({ drive: name, tick }) => (
        <div
          key={name}
          role="note"
          aria-label={`What ticking ${name} allows`}
          data-slot="session-drives-unseen"
          className="mt-1 flex min-w-0 flex-col gap-1 rounded-md border border-held/40 px-2 py-1 text-held text-xs"
        >
          <p className="min-w-0 font-medium [overflow-wrap:anywhere]">
            {name} is ticked for {agentName} to read in this chat, and you have not been shown what
            that allows on this device.
          </p>
          <p className="min-w-0 [overflow-wrap:anywhere]">{drives.warning}</p>
          <div className="flex flex-wrap gap-1">
            <Button
              variant="outline"
              size="xs"
              aria-busy={busy === name}
              onClick={() => void change(name, false)}
            >
              Untick {name}
            </Button>
            <Button
              variant="ghost"
              size="xs"
              aria-busy={busy === name}
              onClick={() => void seen(name, tick ?? null)}
            >
              OK
            </Button>
          </div>
        </div>
      ))}
      {firstTick && ticked.length > 0 && (
        <div
          role="note"
          aria-label="What ticking a drive allows"
          className="mt-1 flex min-w-0 flex-wrap items-start gap-2 rounded-md border border-held/40 px-2 py-1 text-held text-xs"
        >
          <p className="min-w-0 flex-1 [overflow-wrap:anywhere]">{drives.warning}</p>
          <Button variant="ghost" size="xs" onClick={() => setFirstTick(false)}>
            Got it
          </Button>
        </div>
      )}
      {!open && alert && (
        <p role="alert" className="pt-1 text-destructive text-xs [overflow-wrap:anywhere]">
          {alert}
        </p>
      )}
    </div>
  );
}
