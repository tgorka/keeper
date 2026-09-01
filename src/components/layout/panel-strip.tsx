/**
 * The panel strip: the shell's document area (Story 45.1, FR-173, AD-90,
 * UX-DR65).
 *
 * One host over {@link "@/lib/stores/panels"}. It renders every panel left to
 * right, resolves each panel's target every time it shows it, and hands the
 * resolved file to the one viewer registry (Story 45.2, AD-87) rather than
 * deciding anything about formats itself.
 *
 * # Resolution is the point, not an error path
 *
 * A panel stores an identity and nothing else — no name, no size, no absolute
 * path (see `keeper_core::panels`). So showing a panel *is* resolving it, every
 * time: the drive may have been unplugged since the last render, the file may
 * have been renamed on another device, the profile may have been removed. A
 * target that no longer resolves renders the reason and **keeps its place**, so
 * the pane comes back when the drive does. Dropping it would be the same
 * mistake as an app that forgets your open tabs because the network blinked.
 *
 * Every reason a `file` target cannot resolve is composed in Rust and rendered
 * verbatim: `keeper_sync::browse` already words "this volume is not attached",
 * "something else is mounted there" and "this folder is not on disk", and it
 * words them from the same function the Files tree shows them from. Two
 * surfaces wording the absent drive differently is how a user concludes they
 * are two different problems.
 *
 * # Not every target is a document
 *
 * Story 59.12 gave the vocabulary a `task` target, and a task is not a file
 * with a viewer behind it: the surface that already draws one is the Tasks
 * pane, so this strip imports that pane's `TaskDetail` and hosts it rather than
 * growing a rendering of its own. The rule that survives is the one above — the
 * panel holds a task **id** and nothing else, and goes to `sync_tasks` for
 * everything it draws, so it can say *this is no longer here* — and the rule
 * that is added is that a second host over one record reads and does not write.
 * When it goes back to `sync_tasks` is a shorter list than "every time", and
 * {@link useTaskResolution} spells it out. See {@link TaskPanelBody}.
 */
import { X } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ExportFileButton } from "@/components/export/export-file-button";
import {
  FOLD_STRIP,
  FOLD_STRIP_SLOT,
  FoldStripHead,
  FoldStripName,
} from "@/components/layout/fold-strip";
import { PaneHeader } from "@/components/layout/pane-header";
import { TASKS_CLOCK_TICK_MS, TaskDetail } from "@/components/layout/tasks-pane";
import { deriveTitle, NoteEditor } from "@/components/notes/note-editor";
import { Button } from "@/components/ui/button";
import {
  type FilesEntryVm,
  type FilesListingVm,
  type IpcError,
  type NoteVaultVm,
  notesBodyRead,
  type PanelTargetVm,
  syncBrowse,
  syncTaskHistory,
  syncTasks,
  type TaskListingVm,
  type TaskRunVm,
  type TaskVm,
} from "@/lib/ipc/client";
import { useNoteDocument } from "@/lib/stores/notes-editor";
import { useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { type Panel, panelsStore, usePanelsStore } from "@/lib/stores/panels";
import { cn } from "@/lib/utils";
import {
  openWithForProfileEntry,
  type ResolvedViewer,
  type ViewerFile,
  viewerComponentFor,
} from "@/lib/viewers";

/** The accessible name of the strip itself. */
export const PANEL_STRIP_LABEL = "Open panels";

/** What the one panel a fresh keeper starts with says. Not an error: nothing is
 *  wrong, nothing has been opened yet, and the sentence says which. */
export const PANEL_EMPTY_SENTENCE = "Nothing is open here yet. Click a file to open it.";

/** The close control on a panel. The last panel has none — see
 *  {@link "@/lib/stores/panels"}'s `closePanel`; a control that refuses on
 *  activation is worse than a control that is not there. */
export const PANEL_CLOSE_LABEL = "Close panel";

/**
 * The fold control, in both of its states (Story 46.13, FR-217).
 *
 * Two labels rather than one toggle word, because the accessible name of a
 * control should say what pressing it does. The `aria-expanded` on the button
 * says which state it is in; the name says which way it goes.
 *
 * A folded panel keeps its place in the strip and its target, and gives up its
 * width. That is what makes it a different act from closing: the last panel may
 * be folded, because the control that undoes it is sitting where the panel was.
 */
export const PANEL_FOLD_LABEL = "Fold panel";
export const PANEL_UNFOLD_LABEL = "Unfold panel";

/** What a panel says while it is finding out whether its target is still there. */
export const PANEL_RESOLVING_SENTENCE = "Reading…";

/** What a panel whose profile is gone says. Distinct from the drive being out:
 *  the folder was removed from keeper, and plugging anything in will not
 *  bring it back. */
export const PANEL_NO_PROFILE_SENTENCE =
  "The folder this file was in is no longer set up in keeper.";

/** What a note panel says when its vault is no longer configured. */
export const PANEL_NO_VAULT_SENTENCE = "The vault this note was in is no longer set up in keeper.";

/**
 * What a panel says for a target this build has no way to show.
 *
 * Reached only by a `recording` target today: nothing in wave 1 opens one, and
 * Story 45.19 is where the Recording surface starts producing them. The panel
 * says so and keeps its place rather than rendering an empty frame — the same
 * rule the registry applies to an unbound viewer id, and for the same DW-172
 * reason: a silent blank pane is a defect nobody can see.
 */
export const PANEL_UNSUPPORTED_SENTENCE = "keeper cannot show a recording in a panel yet.";

/**
 * What a task panel says when the record no longer holds the task it names.
 *
 * {@link panelFileGoneSentence}'s wording and its rule: a "not found" without a
 * name is a sentence about nothing, and the id is the only name a task has.
 */
export function taskPanelGoneSentence(id: string): string {
  return `keeper could not find a task called ${id} in the task record any more.`;
}

/**
 * What a task panel says for a row `db::list_tasks` could not decode.
 *
 * The task IS still in the record — this is not the sentence above — so the
 * panel says the one true thing about it and then quotes Rust. `reason` is
 * `UnknownTaskVm.reason`, composed where the decode failed and rendered
 * verbatim, exactly as the pane's own *Written by a newer keeper* list renders
 * it: two surfaces wording one unreadable row differently is how a reader
 * concludes they are two different faults.
 *
 * The prefix says only that the row cannot be read, and deliberately not *why*.
 * `reason` already carries the why — the decoder's own line is of the form
 * *unknown task kind `transcribe`, written by a newer keeper* — and a prefix
 * that named the cause as well produced one sentence blaming a newer keeper
 * twice.
 */
export function taskPanelUnknownSentence(reason: string): string {
  return `keeper cannot read this task: ${reason}`;
}

/**
 * What a task panel says when the task record itself would not read, and the
 * rejection carried no sentence of its own.
 *
 * Deliberately NOT {@link taskPanelGoneSentence}, which is where the file
 * panel's equivalent fallback ({@link PANEL_NO_PROFILE_SENTENCE}) lands: a read
 * that threw says nothing whatsoever about whether the task is still there, and
 * reporting it as forgotten would invite the reader to re-create a task that
 * already exists. The honest claim is about the read.
 */
export const PANEL_TASK_UNREADABLE_SENTENCE = "keeper could not read the task record.";

/** Test id for one panel frame, suffixed with the panel's id. */
export const PANEL_TESTID = "panel";

/** Test id for the sentence a panel renders instead of its target. A slot, so a
 *  test asserts the sentence rather than re-deriving it. */
export const PANEL_REASON_TESTID = "panel-reason";

/** The last segment of a profile-relative path — the file's own name.
 *
 * Splitting a relative path is not joining a root: AD-65 forbids the frontend
 * composing a location, and this composes nothing. It is used only to name a
 * file that could NOT be resolved, where there is no `FilesEntryVm` to take a
 * name off. */
function fileNameOf(relativePath: string): string {
  const at = relativePath.lastIndexOf("/");
  return at === -1 ? relativePath : relativePath.slice(at + 1);
}

/** The folder a profile-relative path sits in, `""` for the profile root. The
 *  argument `sync_browse` takes to list the directory this file should be in. */
function parentOf(relativePath: string): string {
  const at = relativePath.lastIndexOf("/");
  return at === -1 ? "" : relativePath.slice(0, at);
}

/** What a panel says when its folder listed and its file was not in it. Names
 *  the file, because "not found" without a name is a sentence about nothing. */
export function panelFileGoneSentence(name: string): string {
  return `keeper could not find ${name} in that folder any more.`;
}

/** Structural guard for the IpcError envelope surfaced on a rejection. */
function isIpcError(value: unknown): value is IpcError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof value.code === "string" &&
    "message" in value &&
    typeof value.message === "string"
  );
}

/** What resolving a file target produced. */
type FileResolution =
  | { readonly status: "resolving" }
  | { readonly status: "resolved"; readonly entry: FilesEntryVm }
  | { readonly status: "unresolved"; readonly reason: string };

/** Turn one listing into this panel's answer about one file in it. */
function resolveFrom(listing: FilesListingVm, relativePath: string): FileResolution {
  if (listing.state !== "listed" || listing.entries === null) {
    return {
      status: "unresolved",
      // Rust composed this sentence for exactly this state and the Files tree
      // shows the same one; the fallback covers a state that carries none.
      reason: listing.detail ?? panelFileGoneSentence(fileNameOf(relativePath)),
    };
  }
  const entry = listing.entries.find((candidate) => candidate.relativePath === relativePath);
  if (entry === undefined) {
    return { status: "unresolved", reason: panelFileGoneSentence(fileNameOf(relativePath)) };
  }
  return { status: "resolved", entry };
}

/**
 * Resolve a file target against what is on disk right now.
 *
 * Lists the file's own folder rather than asking for the file, because
 * `sync_browse` is the ONE directory reader (AD-74) and it already carries the
 * containment rule, the volume check and the Rust-composed sentence for every
 * way a folder can fail to be readable. A second command that stat'ed one path
 * would be a second place those rules live.
 *
 * **Lifted into {@link PanelFrame} by Story 53.3, exactly as
 * {@link noteVaultReason} was by 50.1 and for the same reason**: two decisions
 * now turn on this answer and they must not be able to disagree — what the body
 * draws, and whether this panel draws a header row at all. `null` for a target
 * that is not a file, and `null` while the panel is folded, which is what keeps
 * a folded panel from reading a directory nobody can see (its body is unmounted,
 * so this used to stop happening by construction).
 */
function useFileResolution(target: PanelTargetVm | null, folded: boolean): FileResolution | null {
  const profileId = target?.kind === "file" ? target.profileId : null;
  const relativePath = target?.kind === "file" ? target.relativePath : null;
  const [resolution, setResolution] = useState<FileResolution | null>(null);

  useEffect(() => {
    if (profileId === null || relativePath === null || folded) {
      setResolution(null);
      return;
    }
    let live = true;
    setResolution({ status: "resolving" });
    syncBrowse(profileId, parentOf(relativePath))
      .then((listing) => {
        if (live) {
          setResolution(resolveFrom(listing, relativePath));
        }
      })
      .catch((error: unknown) => {
        if (live) {
          setResolution({
            status: "unresolved",
            // Rust words a refused or unknown profile; anything else is shown
            // as it arrived rather than replaced with a guess.
            reason: isIpcError(error) ? error.message : PANEL_NO_PROFILE_SENTENCE,
          });
        }
      });
    return () => {
      live = false;
    };
  }, [profileId, relativePath, folded]);

  return resolution;
}

/**
 * What resolving a task target produced.
 *
 * `readAtMs` rides on the resolved case rather than being read where the detail
 * is drawn, and that is {@link "@/components/layout/tasks-pane"}'s own rule
 * about `now`: every relative time in one detail is measured from ONE instant,
 * so two lines of the same panel cannot disagree about when now is. It is the
 * instant this panel's facts were read, and {@link TaskPanelBody} uses it to
 * re-seed its display clock — not as the clock itself. A panel that measured
 * only from its last read would freeze, which is the defect Story 57.5's sixth
 * finding fixed in the pane: a row reading *in 5 min* still read *in 5 min* an
 * hour later and never reached *due now*.
 */
type TaskResolution =
  | { readonly status: "resolving" }
  | { readonly status: "resolved"; readonly task: TaskVm; readonly readAtMs: number }
  | { readonly status: "unresolved"; readonly reason: string };

/** Turn one listing into this panel's answer about one task in it. */
function resolveTaskFrom(listing: TaskListingVm, taskId: string): TaskResolution {
  const task = listing.tasks.find((candidate) => candidate.id === taskId);
  if (task !== undefined) {
    return { status: "resolved", task, readAtMs: Date.now() };
  }
  // Still in the record, just not readable by this build — a different fact
  // from having gone, and the one the reader can act on (upgrade, or leave it
  // alone). Checked before the gone sentence, because a row that is present and
  // undecodable would otherwise be reported as absent.
  const unknown = listing.unknown.find((row) => row.id === taskId);
  if (unknown !== undefined) {
    return { status: "unresolved", reason: taskPanelUnknownSentence(unknown.reason) };
  }
  return { status: "unresolved", reason: taskPanelGoneSentence(taskId) };
}

/**
 * Resolve a task target against the task record as it is right now.
 *
 * Reads the whole listing rather than asking for one task, {@link
 * useFileResolution}'s reason: `sync_tasks` is the ONE task reader, it already
 * carries the host verdict every field of the detail is composed from, and
 * there is no per-id command to ask instead. It is also what makes the two
 * unresolved cases distinguishable at all — `listing.unknown` is on the same
 * payload, so "gone" and "unreadable" are one read apart rather than a guess.
 *
 * `null` for a target that is not a task, and `null` while the panel is folded:
 * a folded panel's body is unmounted, and a folded panel that kept reading the
 * task record would be a poll nobody can see (AD-62's sentence).
 *
 * **When it re-reads, said plainly, because a comment that overstates this
 * would be worse than no comment.** On mount, when the target changes, and when
 * a folded panel is unfolded — {@link useFileResolution}'s dependency set
 * exactly. **Not** when the record changes underneath it: nothing here polls,
 * and nothing here subscribes, because the Tasks pane does neither (AD-62 —
 * this app has one clock per host and it is not in the webview). So a task
 * edited, run or forgotten in the pane while a panel holds it keeps its last
 * read facts until that panel is folded and unfolded, re-targeted, or the
 * window is reopened. The pane's own region is the live surface; a panel is a
 * reading, and {@link TaskPanelBody}'s clock keeps the relative times in that
 * reading honest rather than pretending the facts behind them are fresh.
 */
function useTaskResolution(target: PanelTargetVm | null, folded: boolean): TaskResolution | null {
  const taskId = target?.kind === "task" ? target.taskId : null;
  const [resolution, setResolution] = useState<TaskResolution | null>(null);

  useEffect(() => {
    if (taskId === null || folded) {
      setResolution(null);
      return;
    }
    let live = true;
    setResolution({ status: "resolving" });
    syncTasks()
      .then((listing) => {
        if (live) {
          setResolution(resolveTaskFrom(listing, taskId));
        }
      })
      .catch((error: unknown) => {
        if (live) {
          setResolution({
            status: "unresolved",
            // Rust words a record that will not read. A rejection that carries
            // no sentence says nothing about whether the task is still there,
            // so the fallback claims only what is known — see
            // {@link PANEL_TASK_UNREADABLE_SENTENCE}.
            reason: isIpcError(error) ? error.message : PANEL_TASK_UNREADABLE_SENTENCE,
          });
        }
      });
    return () => {
      live = false;
    };
  }, [taskId, folded]);

  return resolution;
}

/** The sentence a panel shows instead of its target. */
function PanelReason({ reason }: { reason: string }) {
  return (
    <p
      data-testid={PANEL_REASON_TESTID}
      className="px-4 py-3 text-muted-foreground text-sm"
      role="status"
    >
      {reason}
    </p>
  );
}

/**
 * What a file panel has to show, once its folder has answered (Story 53.3).
 *
 * The pair of states a body can render, and the union is what makes the frame's
 * one decision expressible: only `resolved` can carry a viewer, and only a
 * viewer can promise a header row. `null` — the absence of this whole value — is
 * the resolution still being in flight, or a target that is not a file, or a
 * folded panel that is reading nothing.
 */
type FilePanelView =
  | { readonly status: "unresolved"; readonly reason: string }
  | { readonly status: "resolved"; readonly view: ResolvedViewer & { readonly file: ViewerFile } };

/**
 * One resolved file, ready to hand to the registry (Story 53.3).
 *
 * Built where the resolution is now read, so the frame and the body cannot end
 * up holding two `ViewerFile`s for one row — and so the registry is asked once
 * per resolution rather than once per body render. The decision the frame needs
 * from it is {@link ResolvedViewer.ownsHostRow}; the body needs everything else.
 */
function viewerFor(profileId: string, entry: FilesEntryVm): ResolvedViewer & { file: ViewerFile } {
  const file: ViewerFile = {
    name: entry.name,
    kind: entry.kind,
    relativePath: entry.relativePath,
    profileId,
    // Rust composed it; the panel only passes it on, and only as an action's
    // argument (AD-65). A panel restored where the drive is out never gets this
    // far, so no viewer is ever handed a stale absolute path.
    absolutePath: entry.absolutePath,
    sizeLabel: entry.size?.label ?? null,
    openWith: openWithForProfileEntry(profileId, entry.relativePath),
    // AD-102's standing sentence for a file keeper will write and does not
    // manage. Composed in Rust and carried on the listing row, so the panel
    // neither words it nor decides when it applies.
    writeCaveat: entry.write.caveat,
    // And the one-line form of it, composed in Rust beside the other (Story
    // 53.3): the surface folds the sentence, and never by clipping it.
    writeCaveatShort: entry.write.caveatShort,
    // And the other verdict on the same row: why keeper will not write HERE.
    // `reason` is `Some` exactly when `writable` is false, so the conditional
    // is reading the pair as Rust guarantees it rather than defending against
    // it. The workspace fence (AD-113) arrives this way — `sync_browse` builds
    // the scope with the profile's sessions zone named, so a `workspace/` file
    // is refused on the listing, before any surface offers to edit it.
    writeRefusal: entry.write.writable ? null : entry.write.reason,
  };
  return { ...viewerComponentFor(file), file };
}

/** A file target: resolved by the frame above, drawn by the registry. `frame` is
 *  the host's controls, non-null only when the resolved viewer promised to draw
 *  a row for them (Story 53.3). */
function FilePanelBody({
  view,
  frame,
}: {
  view: ResolvedViewer & { file: ViewerFile };
  frame: ReactNode;
}) {
  return <view.Component file={view.file} entry={view.entry} frame={frame} />;
}

/**
 * Why a note panel cannot show its editor — or null, when it can (Story 50.1).
 *
 * Pure, and lifted out of the body, because two decisions now turn on it and
 * they must not be able to disagree: what the body draws, and whether the
 * FRAME draws a header row of its own. Since 50.1 a note panel that mounts its
 * editor draws no panel header at all — the editor's own row carries the
 * panel's fold and close — so a note whose vault is gone would have had no
 * header, and therefore no way to close the panel, if the two answers were
 * derived separately and came apart.
 */
export function noteVaultReason(
  vaults: readonly NoteVaultVm[] | null,
  vaultId: string,
): string | null {
  if (vaults === null) {
    return PANEL_RESOLVING_SENTENCE;
  }
  return vaults.some((vault) => vault.id === vaultId) ? null : PANEL_NO_VAULT_SENTENCE;
}

/** A note target: the vault has to exist before the editor can open anything.
 *  `reason` is {@link noteVaultReason}'s answer, decided by the frame above so
 *  that the frame knows whether this is going to draw the panel's header. */
function NotePanelBody({
  vaultId,
  noteId,
  reason,
  frame,
}: {
  vaultId: string;
  noteId: string;
  reason: string | null;
  frame: ReactNode;
}) {
  const onOpenNote = useCallback(
    (next: string) =>
      panelsStore.getState().setActiveTarget({ kind: "note", vaultId, noteId: next }),
    [vaultId],
  );

  if (reason !== null) {
    return <PanelReason reason={reason} />;
  }
  return <NoteEditor vaultId={vaultId} noteId={noteId} onOpenNote={onOpenNote} frame={frame} />;
}

/**
 * A task target: the pane's own detail, in a host that only reads (Story 59.12).
 *
 * {@link "@/components/layout/tasks-pane"}'s `TaskDetail`, not a second
 * rendering of a task — two components over one task could word the same fact
 * differently, and that is the defect shape this codebase keeps closing. The
 * whole of what a panel gives up is the `verbs` object: `null` here, so no Run
 * now, no Edit and no Forget is drawn. The reason is in that component's own
 * header, and it is not squeamishness — the pane's `formSaving`, `deleting` and
 * `running` are pane-wide precisely because two write surfaces over one task
 * undo each other, and a second host cannot see the first's in-flight flags.
 *
 * The runs stay, because `sync_task_history` is a **read**. So this holds a
 * history controller of its own: one open section, and the Tasks pane's
 * `historyToken` in miniature — a read is stamped with the token that was
 * current when it was issued, and lands only if that token is still current, so
 * a slow read cannot arrive in a section that has since been closed. Closing
 * forgets what it held for the same reason the pane's does: re-opening should
 * re-read rather than show a list `task_runs` may have trimmed underneath it.
 *
 * **A body per task, which is what makes that last sentence true.** `PanelBody`
 * keys this component on the target's id, so previewing another task into the
 * same panel unmounts this one and mounts a fresh one. Without the key the
 * state survives the change of subject: the section correctly hides while the
 * panel holds somebody else — `historyOpen` compares ids — and then reappears,
 * still holding the run list read minutes ago, the moment the first task is
 * previewed back. That is exactly the stale section the token machinery exists
 * to prevent, arriving by the one route a token cannot see, because nothing was
 * toggled: the target moved out from under it.
 *
 * **And a clock, not a timestamp.** `now` is seeded from the instant the
 * listing landed — so every relative time in one panel is measured from one
 * instant — and then advances on {@link TASKS_CLOCK_TICK_MS}, the pane's own
 * cadence. Story 57.5's sixth finding is why: measured only from the read, a
 * panel left open froze, and *in 5 min* still said *in 5 min* an hour later.
 * The clock moves; the facts under it do not, and
 * {@link useTaskResolution} says exactly when those are re-read.
 *
 * No command this body reaches writes anything. That is worth stating rather
 * than merely being true, because the two it does reach — `sync_tasks` above
 * and `sync_task_history` here — are the only two a task panel is allowed.
 */
function TaskPanelBody({ resolution }: { resolution: TaskResolution | null }) {
  const [history, setHistory] = useState<{
    readonly id: string;
    /** `null` until this section's read lands: unread, and not empty. */
    readonly runs: TaskRunVm[] | null;
    readonly error: string | null;
  } | null>(null);
  // Seeded from the read and advanced on the pane's own cadence — see this
  // component's header. `readAtMs` is read here rather than passed straight
  // through so that a re-read re-seeds the clock instead of leaving it a tick
  // behind the facts it is measuring.
  const readAtMs = resolution?.status === "resolved" ? resolution.readAtMs : null;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (readAtMs !== null) {
      setNow(readAtMs);
    }
  }, [readAtMs]);
  useEffect(() => {
    const clock = setInterval(() => setNow(Date.now()), TASKS_CLOCK_TICK_MS);
    return () => clearInterval(clock);
  }, []);
  const token = useRef(0);
  const openId = history?.id ?? null;
  const onHistoryToggle = useCallback(
    (id: string) => {
      token.current += 1;
      if (openId === id) {
        setHistory(null);
        return;
      }
      const mine = token.current;
      setHistory({ id, runs: null, error: null });
      syncTaskHistory(id).then(
        (runs) => {
          if (mine === token.current) {
            setHistory({ id, runs, error: null });
          }
        },
        (cause: unknown) => {
          if (mine === token.current) {
            // Rust's sentence where there is one. A refused read is a fault to
            // report, never an empty list to invent.
            setHistory({
              id,
              runs: null,
              error: isIpcError(cause) ? cause.message : String(cause),
            });
          }
        },
      );
    },
    [openId],
  );

  if (resolution === null || resolution.status === "resolving") {
    return <PanelReason reason={PANEL_RESOLVING_SENTENCE} />;
  }
  if (resolution.status === "unresolved") {
    return <PanelReason reason={resolution.reason} />;
  }
  const task = resolution.task;
  return (
    <TaskDetail
      task={task}
      now={now}
      // A refusal is what a write was answered with, and this host issues none.
      refusal={null}
      // All three read the ONE slot, so this panel can never be handed another
      // task's runs — the id and the runs it belongs to move together or not at
      // all. They come apart when the target changes under an open section,
      // which is exactly when the section must stop being drawn.
      historyOpen={history?.id === task.id}
      historyRuns={history?.id === task.id ? history.runs : null}
      historyError={history?.id === task.id ? history.error : null}
      onHistoryToggle={onHistoryToggle}
      // The frame above already names this panel — `aria-label` on its
      // `<section>`, and the header row under it — so the id is drawn as text
      // here rather than as a heading. `PanelFrame`'s own rule, for the reason
      // it gives about a file: a second `h2` naming the same thing would put
      // two entries in a screen reader's heading list for one task, and in the
      // lockstep case the pane's region beside this one is already the first.
      heading={false}
      verbs={null}
    />
  );
}

/** What one panel is showing.
 *
 *  `noteReason`, `fileView` and `frame` are all the FRAME's answers, decided
 *  above so that the frame knows whether the thing below it is going to draw the
 *  panel's header row. `frame` is non-null for exactly one of them at a time —
 *  see {@link PanelFrame} — and whichever body is not drawing the row ignores it. */
function PanelBody({
  target,
  emptySentence,
  noteReason,
  fileView,
  taskResolution,
  frame,
}: {
  target: PanelTargetVm | null;
  emptySentence: string;
  noteReason: string | null;
  fileView: FilePanelView | null;
  taskResolution: TaskResolution | null;
  frame: ReactNode;
}) {
  if (target === null) {
    return <PanelReason reason={emptySentence} />;
  }
  switch (target.kind) {
    case "file":
      // `null` is the resolution not having landed yet, or having failed: the
      // frame holds the sentence for both, because the same answer decides
      // whether it kept its own row.
      if (fileView === null) {
        return <PanelReason reason={PANEL_RESOLVING_SENTENCE} />;
      }
      if (fileView.status === "unresolved") {
        return <PanelReason reason={fileView.reason} />;
      }
      return <FilePanelBody view={fileView.view} frame={frame} />;
    case "note":
      return (
        <NotePanelBody
          vaultId={target.vaultId}
          noteId={target.noteId}
          reason={noteReason}
          frame={frame}
        />
      );
    case "recording":
      return <PanelReason reason={PANEL_UNSUPPORTED_SENTENCE} />;
    case "task":
      // Keyed, so a preview of another task into this panel mounts a fresh
      // body rather than reconciling one task's controller into another's —
      // see {@link TaskPanelBody} for the stale run section that survives
      // otherwise. The note and file bodies need no key: neither holds state
      // about the target it happens to be showing.
      return <TaskPanelBody key={target.taskId} resolution={taskResolution} />;
  }
}

/**
 * The note's own title, for a panel that has to say which note it is holding.
 *
 * Two sources, because a panel outlives the editor inside it. While the note is
 * open the title is the FIRST LINE OF THE BUFFER — the same derivation the
 * editor's own heading uses, so a title being typed and the name of the panel
 * holding it never disagree. Folded, there is no buffer: the editor is
 * unmounted, its mirror is dropped, and a panel restored from disk at launch
 * never had one. So a folded note panel reads the note once through
 * `notes_body_read`, which is the call Rust already provides for exactly this —
 * "the read half of the one read-modify-write a surface can do to a note it has
 * not opened in the editor" — and nothing else changes on that strip until it
 * is unfolded.
 *
 * `null` for every other kind of target, and `null` while the one read is in
 * flight: the caller falls back to naming the KIND, which is what a panel said
 * about every note before this.
 */
function useNoteTitle(
  vaultId: string | null,
  noteId: string | null,
  folded: boolean,
): string | null {
  const live = useNoteDocument(vaultId, noteId, (d) =>
    d.text === "" ? null : deriveTitle(d.text),
  );
  const [read, setRead] = useState<{ readonly key: string; readonly title: string } | null>(null);
  useEffect(() => {
    if (!folded || vaultId === null || noteId === null) {
      return;
    }
    let alive = true;
    notesBodyRead(vaultId, noteId).then(
      (body) => {
        if (alive) {
          setRead({ key: `${vaultId}\u0000${noteId}`, title: deriveTitle(body.text) });
        }
      },
      // A note that cannot be read is a note this strip cannot name, which is
      // the state it was already in. The unfolded panel says why; a 48px strip
      // has nowhere to put a sentence and no reason to shout.
      () => {},
    );
    return () => {
      alive = false;
    };
  }, [folded, vaultId, noteId]);
  if (live !== null) {
    return live;
  }
  if (vaultId === null || noteId === null || read === null) {
    return null;
  }
  return read.key === `${vaultId}\u0000${noteId}` ? read.title : null;
}

/** What the panel's header calls it, and what its folded spine reads.
 *
 *  A note is named by its own title where one could be resolved
 *  ({@link useNoteTitle}): "Note" over a strip standing beside three other
 *  panels answers the question a name is asked.
 *
 *  A task needs no such trip. Its id IS its name — the pane's own detail draws
 *  the same string as the region's heading — and the target carries it, so a
 *  task panel names itself with no resolution at all, folded or not, before any
 *  read has landed and after one has failed. */
function panelName(target: PanelTargetVm | null, noteTitle: string | null): string {
  if (target === null) {
    return "Panel";
  }
  switch (target.kind) {
    case "file":
      return fileNameOf(target.relativePath);
    case "note":
      return noteTitle ?? "Note";
    case "recording":
      return "Recording";
    case "task":
      return target.taskId;
  }
}

/**
 * One panel: a header that names it, folds it and can close it, and the target
 * below.
 *
 * # Folded is a different frame, not a hidden body
 *
 * A folded panel renders **no** {@link PanelBody}, and that is deliberate rather
 * than incidental. A body kept mounted behind `hidden` would keep its
 * subscriptions, its `sync_browse` and its editor buffer alive, which is exactly
 * the cost the reader was trying to reclaim — and for a note panel it would keep
 * a document mirror open over a note nobody can see. It also drops `flex-1` and
 * the 280px floor, because the whole visible point of folding is that the
 * neighbours get the width.
 *
 * The header is {@link PaneHeader} (AD-104): identity absorbs the slack, the
 * actions sit last. A panel has no status element yet, so it passes none — see
 * that module for why an empty reserved slot is not the same thing as no slot.
 * Panels are `flex-1` inside a horizontally scrolling strip, so this header gets
 * NARROWER than the note editor's rather than wider, which is the regime the
 * shrink rules were written for.
 *
 * # And no header at all when what is below draws one
 *
 * Story 50.1 for a note, Story 53.3 for a file: the surface inside already draws
 * a row naming the same thing, so the panel hands its controls down instead of
 * drawing a second band above them. The two cases differ only in how the answer
 * is reached — a note's is a pure store read, and a file's is the folder listing
 * plus the registry's answer about what will draw it — and they meet in one
 * `frame` node so a note panel and a file panel cannot come to offer different
 * chrome.
 *
 * **The row is given up only when the thing below has PROMISED to draw one.** A
 * `.pdf` resolves to a viewer with no chrome; a listing that has not landed yet
 * and one that failed are sentences, and a sentence carries no fold and no
 * close. `ownsHostRow` is the registry's promise (`components.tsx`), read here
 * and never guessed at from a format — and `savable` is deliberately NOT part of
 * this decision, because it is decided inside the frame from a read that has not
 * happened yet. A frame handed the controls draws the row in every state,
 * including the four in which it used to draw none.
 */
function PanelFrame({
  panel,
  active,
  closable,
  emptySentence,
}: {
  panel: Panel;
  active: boolean;
  closable: boolean;
  emptySentence: string;
}) {
  const vaults = useNotesVaultsStore((s) => s.vaults);
  // Story 50.1: a note panel draws NO header of its own. The editor below it
  // already draws a row that says which note, where it lives and what can be
  // done to it, and the panel's row said `Note` — a word the note's own title
  // says better — for the price of a 40px band and a seam. So the panel hands
  // its two controls down instead of drawing a row to hold them.
  //
  // Only when the editor is what is going to be there, though. A note whose
  // vault is gone shows a sentence, and a sentence cannot carry a fold or a
  // close: that panel keeps its own row, and the ONE rule that decides which
  // it is is `noteVaultReason`, read here and passed down rather than asked
  // twice.
  const noteReason =
    panel.target?.kind === "note" ? noteVaultReason(vaults, panel.target.vaultId) : null;
  const noteOwnsRow = panel.target?.kind === "note" && noteReason === null;
  // Story 53.3's half of the same rule, and the reason the resolution moved up
  // here: `ownsHostRow` is only knowable once the folder has answered and the
  // registry has said what draws the row it answered with.
  const fileResolution = useFileResolution(panel.target, panel.folded);
  const fileProfileId = panel.target?.kind === "file" ? panel.target.profileId : null;
  const fileView = useMemo<FilePanelView | null>(() => {
    if (fileResolution === null || fileProfileId === null) {
      return null;
    }
    if (fileResolution.status === "resolving") {
      return null;
    }
    return fileResolution.status === "unresolved"
      ? { status: "unresolved", reason: fileResolution.reason }
      : { status: "resolved", view: viewerFor(fileProfileId, fileResolution.entry) };
  }, [fileResolution, fileProfileId]);
  const fileOwnsRow = fileView?.status === "resolved" && fileView.view.ownsHostRow;
  // Read here rather than inside the body, `useFileResolution`'s siting: the
  // hook takes `panel.folded` and answers `null` for a folded panel, and a hook
  // called from a body that is unmounted while folded could never observe the
  // state it is guarding against. A task panel keeps this frame's own header
  // either way — `TaskDetail` draws no host row and never has — so unlike the
  // note and file cases nothing about the chrome turns on this answer.
  const taskResolution = useTaskResolution(panel.target, panel.folded);
  const noteTitle = useNoteTitle(
    panel.target?.kind === "note" ? panel.target.vaultId : null,
    panel.target?.kind === "note" ? panel.target.noteId : null,
    panel.folded,
  );
  const name = panelName(panel.target, noteTitle);
  const FoldGlyph = panel.folded ? FOLD_STRIP.unfoldIcon : FOLD_STRIP.foldIcon;
  // Folded, the tooltip and the accessible name are ONE string and it carries
  // the panel's name, because a folded panel has nothing else on screen: a
  // pointer that hovered a bare chevron would learn only that the strip folds,
  // not which of four files this one is. Open, the panel names itself an inch
  // to the left, so the control only has to say what it does. Whichever it is,
  // `title` and `aria-label` are the same words — a control whose tooltip and
  // whose spoken name disagree cannot be operated by anyone saying what they
  // see (WCAG 2.5.3), and with the text gone the tooltip IS the visible label.
  const foldName = panel.folded ? `${PANEL_UNFOLD_LABEL}: ${name}` : PANEL_FOLD_LABEL;
  const fold = (
    <Button
      type="button"
      variant="ghost"
      size={FOLD_STRIP.headControlSize}
      // The name says which way the control goes; `aria-expanded` says where it
      // is now.
      aria-expanded={!panel.folded}
      aria-label={foldName}
      title={foldName}
      className="shrink-0"
      onClick={() => panelsStore.getState().toggleFold(panel.id)}
    >
      <FoldGlyph aria-hidden="true" />
    </Button>
  );
  const close = closable ? (
    <Button
      type="button"
      variant="ghost"
      size="icon-sm"
      aria-label={PANEL_CLOSE_LABEL}
      title={PANEL_CLOSE_LABEL}
      className="shrink-0"
      onClick={() => panelsStore.getState().closePanel(panel.id)}
    >
      <X aria-hidden="true" />
    </Button>
  ) : null;
  // Story 45.21. A file only: a note panel's Export is in the editor's own
  // Actions menu, which is the surface that can flush the buffer before the
  // bytes are read off the disk. Two Export controls over one note, one of which
  // exported the last autosave, is the shape this placement exists to refuse.
  const exportFile =
    panel.target?.kind === "file" ? (
      <ExportFileButton
        profileId={panel.target.profileId}
        relativePath={panel.target.relativePath}
      />
    ) : null;
  // What the PANEL's controls are, wherever the row that carries them is drawn
  // — this frame's own header, the note editor's, or the file frame's. One node
  // either way, so a note panel and a file panel cannot come to offer different
  // chrome.
  //
  // Export travels with them for a FILE and not for a note, which is the same
  // rule as when this row was the panel's own: the control belongs to whoever
  // can read the bytes off the disk, and for a note that is the editor.
  const frame = (
    <>
      {exportFile}
      {fold}
      {close}
    </>
  );
  // Which surface below is drawing this panel's row, if either. At most one, and
  // the node goes to whichever it is: `PanelBody` hands it to the body it draws,
  // and a body that draws no row ignores it.
  const handedDown = noteOwnsRow || fileOwnsRow ? frame : null;
  return (
    <section
      aria-label={name}
      data-testid={`${PANEL_TESTID}-${panel.id}`}
      data-active={active ? "true" : undefined}
      data-folded={panel.folded ? "true" : undefined}
      data-fold-strip={panel.folded ? FOLD_STRIP_SLOT : undefined}
      // Clicking anywhere in a panel focuses it, which is what makes the next
      // single click in the browser replace THIS panel rather than the one that
      // happened to be focused before.
      //
      // Both presses, and that is not belt-and-braces. A surface inside a panel
      // may cancel its own `pointerdown` — the task board does, to stop WebKit
      // anchoring a text selection under a drag (`task-board.tsx`) — and a
      // cancelled `pointerdown` sets the platform's PREVENT MOUSE EVENT flag, so
      // NO `mousedown` is dispatched at all and this ancestor never hears one. It
      // suppresses the default focus action too, so `onFocusCapture` cannot cover
      // either: no focus event fires. Left on `mousedown` alone, pressing a card
      // in an unfocused panel opened its note into whichever panel `activeId`
      // still pointed at. `focusPanel` is idempotent, so the pair costs a second
      // store read on a press that does fire both.
      onFocusCapture={() => panelsStore.getState().focusPanel(panel.id)}
      onPointerDown={() => panelsStore.getState().focusPanel(panel.id)}
      onMouseDown={() => panelsStore.getState().focusPanel(panel.id)}
      className={cn(
        "flex h-full flex-col overflow-hidden border-border border-r bg-background last:border-r-0",
        // A folded panel is a folded strip, and it is now the same 48px as
        // every other one (`fold-strip.tsx`). `w-auto` made its width a
        // consequence of whatever its one button happened to measure — the
        // only one of the four that nothing measured, and the reason four
        // strips side by side were not the same width.
        panel.folded
          ? cn(FOLD_STRIP.widthClass, "shrink-0 grow-0")
          : cn(PANEL_MIN_WIDTH_CLASS, "flex-1"),
        // The active mark is the ring. An inset ring draws on all four sides,
        // so on top of this panel's own trailing edge the right side would be
        // 2px while the other three are 1px — the panel cancels its border
        // rather than growing an edge, and DESIGN.md's hairline holds.
        active && "border-r-transparent ring-1 ring-ring ring-inset",
      )}
    >
      {panel.folded ? (
        // Folded: the control that undoes it, and the panel's name down the
        // strip. No body — see this function's header for why a folded panel
        // unmounts rather than hides.
        //
        // The band is {@link FoldStripHead}, which is this head, the drawer's
        // and every surface column's: 40px, DESIGN.md's `pane-header`, ending
        // in the rule that runs across every pane beside it. It used to be
        // spelled here, and the OTHER THREE strips were the ones that got it
        // wrong — 44px, with their divider 8px lower. `fold-strip.tsx` owns
        // the sum now, so there is nowhere left for the four to disagree.
        <>
          <FoldStripHead className="justify-center">{fold}</FoldStripHead>
          <FoldStripName name={name} />
        </>
      ) : noteOwnsRow || fileOwnsRow ? null : (
        <PaneHeader
          // No `border-b` and no `py-*`: `PaneHeader` owns its own bottom edge
          // and its own 40px height, and spelling either here draws it twice.
          className="px-3"
          // Deliberately not a heading. The viewer inside draws the document's
          // own heading, and a second `h2` naming the same file would put two
          // entries in a screen reader's heading list for one document. The
          // panel is named by the section's `aria-label`, which is how a reader
          // jumps between panels — a tab strip's job, not an outline's.
          //
          // The treatment is the shared one every foldable surface names itself
          // in, minus the heading semantics: DESIGN.md's `pane-header`
          // typography, which this row was the one place not to use.
          identity={<span className={FOLD_STRIP.titleClass}>{name}</span>}
          actions={frame}
        />
      )}
      {panel.folded ? null : (
        <div className="min-h-0 flex-1 overflow-auto">
          <PanelBody
            target={panel.target}
            emptySentence={emptySentence}
            noteReason={noteReason}
            fileView={fileView}
            taskResolution={taskResolution}
            // Handed to every body and consumed by exactly one: the note editor
            // or the file frame, whichever is drawing this panel's row. A body
            // that draws no row ignores it and the row above is this frame's.
            frame={handedDown}
          />
        </div>
      )}
    </section>
  );
}

/**
 * Every open panel, left to right.
 *
 * `emptySentence` is the one thing the host gets to say, and it exists because
 * the strip has two hosts since Story 46.12. The default names the gesture that
 * fills a panel in the Files surface; the Notes surface passes its own, because
 * "click a file to open it" is the wrong instruction beside a list of notes and
 * a first-run panel is the very first thing either surface shows. It is a prop
 * threaded to the frame rather than a module default or a store value, so the
 * sentence depends on which surface is rendering and not on which one mounted
 * last.
 */
/**
 * The narrowest an open panel is allowed to be, as a class so the panel and the
 * strip that must hold one of them cannot disagree about the number.
 *
 * 280 is where a document panel stops being readable: the header's identity
 * group and its first action, or a line of prose at the body's font size.
 */
export const PANEL_MIN_WIDTH_CLASS = "min-w-[280px]";

/** The same number as a flex basis, so the strip claims it rather than
 * receiving whatever is left. */
export const PANEL_BASIS_CLASS = "basis-[280px]";

export function PanelStrip({ emptySentence = PANEL_EMPTY_SENTENCE }: { emptySentence?: string }) {
  const panels = usePanelsStore((s) => s.panels);
  const activeId = usePanelsStore((s) => s.activeId);
  return (
    <section
      aria-label={PANEL_STRIP_LABEL}
      // `flex-1 min-w-0` was `flex: 1 1 0%` with no floor, and that is two
      // things at once: a scaled shrink factor of `1 x 0 = 0`, so the strip
      // never takes a share of a deficit, and a basis of zero, so it only ever
      // receives what the columns beside it did not want. The surface columns
      // therefore never had to give, and the panel got `surface - 560` however
      // little that was — 120px at the smallest window this app allows, with a
      // 280px panel inside it overflowing into the scroller below.
      //
      // A basis and a floor of one panel make the strip a claimant instead: the
      // columns give first, down to their own floors, and the note keeps a
      // width worth reading. Several panels still scroll inside here, which is
      // this element's other job and is unchanged.
      // `grow shrink basis-…` rather than `flex-1` and a basis beside it: those
      // two are one property and one shorthand at equal specificity, so which
      // wins is decided by the order Tailwind happens to emit them in, not by
      // the order they are written here. Three explicit properties cannot
      // disagree.
      className={cn("flex shrink grow overflow-x-auto", PANEL_BASIS_CLASS, PANEL_MIN_WIDTH_CLASS)}
    >
      {panels.map((panel) => (
        <PanelFrame
          key={panel.id}
          panel={panel}
          active={panel.id === activeId}
          // The last panel cannot be closed, so it does not offer to be.
          closable={panels.length > 1}
          emptySentence={emptySentence}
        />
      ))}
    </section>
  );
}
