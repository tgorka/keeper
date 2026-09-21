import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  type SyncProfileVm,
  syncProfiles,
  syncTasks,
  syncTasksLedger,
  syncTasksLedgerSet,
  type TasksLedgerVm,
  type TaskVm,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

export function TasksSection({ open }: { open: boolean }) {
  const id = useId();
  const [ledger, setLedger] = useState<TasksLedgerVm | null>(null);
  const [profiles, setProfiles] = useState<SyncProfileVm[]>([]);
  const [tasks, setTasks] = useState<TaskVm[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [subfolder, setSubfolder] = useState("tasks");
  const generation = useRef(0);
  const reload = useCallback(() => {
    const mine = ++generation.current;
    if (!open) return;
    setBusy(true);
    setError(null);
    void Promise.all([syncTasksLedger(), syncProfiles(), syncTasks()])
      .then(([next, folders, listing]) => {
        if (mine !== generation.current) return;
        setLedger(next);
        setSubfolder(next.subfolder);
        setProfiles(folders);
        setTasks(listing.tasks.filter((task) => task.kind === "copy"));
      })
      .catch((cause: unknown) => {
        if (mine === generation.current) setError(syncErrorMessage(cause));
      })
      .finally(() => {
        if (mine === generation.current) setBusy(false);
      });
  }, [open]);
  useEffect(() => {
    reload();
    return () => {
      generation.current += 1;
    };
  }, [reload]);

  const choose = async (profileId: string, folder = subfolder) => {
    if (busy) return;
    const mine = ++generation.current;
    setBusy(true);
    setError(null);
    try {
      const next = await syncTasksLedgerSet(profileId || null, profileId ? folder : null);
      const listing = await syncTasks();
      if (mine !== generation.current) return;
      setLedger(next);
      setSubfolder(next.subfolder);
      setTasks(listing.tasks.filter((task) => task.kind === "copy"));
    } catch (cause) {
      if (mine === generation.current) setError(syncErrorMessage(cause));
    } finally {
      if (mine === generation.current) setBusy(false);
    }
  };

  return (
    <section
      id="settings-tasks"
      aria-labelledby={`${id}-title`}
      className="flex min-w-0 flex-col gap-3 border-t pt-4 text-sm"
    >
      <h3 id={`${id}-title`} className="font-medium">
        Tasks
      </h3>
      <p className="text-muted-foreground text-xs">
        Task configuration and run ledgers live in the folder resolved by the engine. This choice
        applies on this machine; it does not move older logs.
      </p>
      <Label htmlFor={`${id}-folder`}>Ledger folder</Label>
      <select
        id={`${id}-folder`}
        disabled={busy || ledger === null}
        className="h-9 w-full min-w-0 rounded-md border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        value={ledger?.chosenProfileId ?? ""}
        onChange={(event) => void choose(event.target.value)}
      >
        <option value="">Keeper decides</option>
        {ledger?.chosenProfileId &&
          !profiles.some((profile) => profile.id === ledger.chosenProfileId) && (
            <option value={ledger.chosenProfileId}>{ledger.chosenProfileId} — unavailable</option>
          )}
        {profiles.map((profile) => (
          <option key={profile.id} value={profile.id}>
            {profile.name}
          </option>
        ))}
      </select>
      <Label htmlFor={`${id}-subfolder`}>Task subfolder</Label>
      <div className="flex min-w-0 gap-2">
        <Input
          id={`${id}-subfolder`}
          value={subfolder}
          disabled={busy || ledger === null}
          onChange={(event) => setSubfolder(event.target.value)}
        />
        <Button
          variant="outline"
          disabled={busy || !ledger?.chosenProfileId}
          onClick={() => void choose(ledger?.chosenProfileId ?? "")}
        >
          Save subfolder
        </Button>
      </div>
      {busy && (
        <p role="status" className="text-xs">
          Reading task ledger settings…
        </p>
      )}
      {error && (
        <div role="alert">
          <p>{error}</p>
          <Button size="sm" variant="ghost" disabled={busy} onClick={reload}>
            Retry
          </Button>
        </div>
      )}
      {ledger && (
        <>
          <div className="flex min-w-0 flex-col gap-1">
            <p className="font-medium">Engine's resolved ledger folder</p>
            <p>{ledger.resolvedProfileName ?? "No ledger folder configured"}</p>
            {ledger.root && (
              <p className="font-mono text-xs leading-5 [overflow-wrap:anywhere]">{ledger.root}</p>
            )}
            <p className="text-muted-foreground text-xs">Task subfolder: {ledger.subfolder}</p>
            <p className="text-muted-foreground text-xs">
              Subfolder source: {ledger.subfolderSource}. Folder file writable:{" "}
              {ledger.writable ? "yes" : "no"}.
            </p>
            {ledger.root && (
              <p className="text-muted-foreground text-xs">
                {ledger.exists
                  ? "The ledger folder exists."
                  : "The ledger folder will be created by the first run."}
              </p>
            )}
            {ledger.notice && (
              <p role="status" className="text-xs">
                {ledger.notice}
              </p>
            )}
            {!ledger.writable && ledger.subfolderSource === "default" && (
              <p role="status" className="text-xs">
                The folder file could not be written. The engine default keeps task logs on this
                machine.
              </p>
            )}
            {ledger.chosenProfileId !== null &&
              ledger.chosenProfileId !== ledger.resolvedProfileId && (
                <p role="status" className="text-xs">
                  The chosen folder could not be honoured. The engine is using the resolved folder
                  shown above.
                </p>
              )}
          </div>
          {tasks.length === 0 && (
            <p className="text-muted-foreground text-xs">No copy tasks configured.</p>
          )}
          {tasks.map((task) => (
            <div key={task.id} className="flex min-w-0 flex-col gap-1 border-t pt-2">
              <h4 className="font-medium [overflow-wrap:anywhere]">
                {task.description?.trim() || task.id}
              </h4>
              <p className="text-xs">Task configuration (task.toml) and run ledgers:</p>
              <p className="font-mono text-xs leading-5 [overflow-wrap:anywhere]">
                {task.ledgerPath ?? "No ledger folder configured"}
              </p>
              <p className="text-xs">Destination copy logs (keeper-copy-*.log):</p>
              <p className="font-mono text-xs leading-5 [overflow-wrap:anywhere]">
                {task.copyDestination ?? "No destination configured"}
              </p>
            </div>
          ))}
        </>
      )}
    </section>
  );
}
