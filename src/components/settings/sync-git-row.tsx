/**
 * Settings → the `git` folder sync runs on, and how to point it somewhere else
 * (Story 34.14, DW-122).
 *
 * # Why this is not inside `SyncSection`
 *
 * The Settings dialog renders `{sync && <SyncSection …>}`, and `capabilities.sync`
 * **is** `git_report(…).state == Ok`. So on precisely the machines this report
 * exists for — no git, a git that cannot run, a git below the 2.42 floor — the
 * Sync section does not render at all, and a report inside it would be
 * unreachable exactly when it is the only thing worth reading.
 *
 * So it lives beside that gate rather than behind it. The engine already refuses
 * to open on those machines and the capability flag already hides the surface
 * correctly; what was missing was any way for the user to act on it. Before this
 * the resolution was computed on every capability probe and thrown away, which
 * made a machine with two gits — one modern, one from the system — a dead end:
 * told sync was unavailable, shown nothing about why, and given nothing to change.
 *
 * # Why it renders when git is fine too
 *
 * Because the path setting has to be reachable in order to be *cleared*. A user
 * who pins a path and later moves that binary would otherwise have a broken
 * setting they could only discover once it had already broken sync. When git is
 * fine this is one quiet line and a field; when it is not, the same block carries
 * the refusal.
 */
import { useCallback, useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { capabilities, type SyncGitVm, syncGitPathSet, syncGitStatus } from "@/lib/ipc/client";
import { capabilitiesStore } from "@/lib/stores/capabilities";
import { syncErrorMessage } from "@/lib/stores/sync";

/** Block heading. Named for the thing, not for the problem. */
export const SYNC_GIT_TITLE = "The git keeper uses";

/**
 * What a healthy machine is told. The summary itself is composed in Rust and
 * rendered verbatim, so this is only the frame around it.
 */
export const SYNC_GIT_OK_NOTE =
  "Folder sync drives this binary for every push, merge and worktree operation. Leave the field empty and keeper finds one itself.";

/**
 * The refusal. Deliberately says what is *not* affected: a user who reads
 * "unavailable" tends to assume their folders are at risk, and they are not —
 * nothing has been synced, so nothing has been touched.
 */
export const SYNC_GIT_PROBLEM_NOTE =
  "Folder sync is unavailable until keeper can find a usable git. Nothing in your folders has been changed or removed, and no folder settings were lost — sync simply has not run.";

/** The path control. */
export const SYNC_GIT_PATH_LABEL = "git binary";
export const SYNC_GIT_PATH_PLACEHOLDER = "Found automatically";
export const SYNC_GIT_SAVE_LABEL = "Use this";
export const SYNC_GIT_CLEAR_LABEL = "Find it for me";

/**
 * Shown after a successful change, because the effect is not visible in this
 * block alone: the Sync section appears or disappears with it.
 */
export const SYNC_GIT_APPLIED_SENTENCE = "Saved. Folder sync now uses this git.";
export const SYNC_GIT_CLEARED_SENTENCE = "Cleared. keeper will find a git itself.";

/**
 * The `git` report and its path control.
 *
 * Renders nothing at all when the build has no folder sync (iOS): telling a phone
 * user about a git version floor would be noise, which is what `unsupported`
 * means. And nothing before the first read lands, because there is nothing
 * honest to say yet — but a read that *fails* is a fact, and gets said.
 */
export function SyncGitRow({ open }: { open: boolean }) {
  const [report, setReport] = useState<SyncGitVm | null>(null);
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Which of the two confirmations to show, or neither. A single boolean would
  // say "Saved" after a clear, which is the wrong sentence for that action.
  const [applied, setApplied] = useState<"set" | "cleared" | null>(null);

  const load = useCallback(async () => {
    try {
      const vm = await syncGitStatus();
      setReport(vm);
      setPath(vm.configuredPath ?? "");
      setError(null);
    } catch (raw) {
      // `syncErrorMessage`, never `String(raw)`: an IPC rejection is a
      // `{ code, message }` object, and stringifying one prints
      // "[object Object]" exactly where the Rust-authored reason belongs.
      setError(syncErrorMessage(raw));
    }
  }, []);

  useEffect(() => {
    if (!open) {
      return;
    }
    void load();
  }, [open, load]);

  const commit = async (next: string) => {
    setBusy(true);
    setError(null);
    setApplied(null);
    try {
      const vm = await syncGitPathSet(next);
      setReport(vm);
      setPath(vm.configuredPath ?? "");
      setApplied(next === "" ? "cleared" : "set");
      // The capability flag is derived from this same resolution, so the Sync
      // section's presence is now one change out of date. Re-reading it is what
      // makes a repaired path reveal that section without reopening the dialog.
      try {
        capabilitiesStore.getState().applySnapshot(await capabilities());
      } catch {
        // The path change itself succeeded and is reported above. A failed
        // capability re-read costs a stale section until the dialog is reopened,
        // which is not worth replacing a success message with an error.
      }
    } catch (raw) {
      setError(syncErrorMessage(raw));
    } finally {
      setBusy(false);
    }
  };

  // A refused probe is the one case where there is no report and something still
  // has to be said. This used to be one guard — `report === null || state ===
  // "unsupported"` — which returned before the error paragraph at the bottom of
  // the block, so `load()`'s `setError` had nowhere to land and a rejected
  // `sync_git_status` rendered the row as blank space. That is the silence this
  // story exists to remove, one layer further out: when the report is missing,
  // the reason it is missing is the only thing worth reading.
  if (report === null) {
    return error === null ? null : (
      <div className="mt-1 flex flex-col gap-2 border-border border-t pt-3 text-sm">
        {/* Titled, because an unlabelled red sentence in a settings dialog says
            nothing about what failed. Named for the thing, as everywhere else. */}
        <p className="font-medium">{SYNC_GIT_TITLE}</p>
        <p className="text-destructive text-xs">{error}</p>
      </div>
    );
  }
  // On a build without folder sync there is nothing honest to render: telling a
  // phone user about a git version floor would be noise.
  if (report.state === "unsupported") {
    return null;
  }
  const healthy = report.state === "ok";
  const trimmed = path.trim();

  return (
    <div className="mt-1 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">{SYNC_GIT_TITLE}</p>
      {/* The Rust-composed line, verbatim in both directions: it is the same
          sentence `keeper-syncd doctor` prints, so the two surfaces cannot word
          one machine's state two different ways. Verbatim is not the same as
          terminal, so it is set in the room's voice; `figures` keeps the version
          numbers in it from reflowing the line. */}
      {healthy
        ? report.summary !== null && (
            <p className="figures text-muted-foreground text-xs">{report.summary}</p>
          )
        : report.problem !== null && (
            <p className="figures whitespace-pre-line text-destructive text-xs">{report.problem}</p>
          )}
      <p className="text-muted-foreground text-xs">
        {healthy ? SYNC_GIT_OK_NOTE : SYNC_GIT_PROBLEM_NOTE}
      </p>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor="sync-git-path">{SYNC_GIT_PATH_LABEL}</Label>
        <div className="flex items-center gap-1">
          <Input
            id="sync-git-path"
            className="w-56 font-mono"
            placeholder={SYNC_GIT_PATH_PLACEHOLDER}
            value={path}
            disabled={busy}
            onChange={(event) => {
              setApplied(null);
              setPath(event.target.value);
            }}
          />
          <Button
            type="button"
            variant="outline"
            size="xs"
            // An empty field is not a save; it is the clear beside it.
            disabled={busy || trimmed === ""}
            onClick={() => {
              void commit(trimmed);
            }}
          >
            {SYNC_GIT_SAVE_LABEL}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="xs"
            // Offered only when there is a stored setting to clear; on an
            // already-automatic install it would do nothing at all.
            disabled={busy || (report.configuredPath ?? "") === ""}
            onClick={() => {
              void commit("");
            }}
          >
            {SYNC_GIT_CLEAR_LABEL}
          </Button>
        </div>
      </div>
      {applied === "set" && (
        <p className="text-muted-foreground text-xs">{SYNC_GIT_APPLIED_SENTENCE}</p>
      )}
      {applied === "cleared" && (
        <p className="text-muted-foreground text-xs">{SYNC_GIT_CLEARED_SENTENCE}</p>
      )}
      {error !== null && <p className="text-destructive text-xs">{error}</p>}
    </div>
  );
}
