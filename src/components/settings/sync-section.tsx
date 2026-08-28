/**
 * Settings → Sync section (Epic 29, Stories 29.4 + 29.5, FR-77..FR-93; Epic 34,
 * Story 34.7).
 *
 * Lists every configured folder↔repository binding with the status line Rust
 * composed for it, and lets a folder be synced now, paused, resumed, edited or
 * removed. Both adding and editing are the shared {@link AddFolderForm} (Story
 * 33.4) in its two modes, which the Sync view renders too — the form carries
 * rules Rust enforces, and two copies of it would drift. Removal is shared the
 * same way and for the same reason: the Sync view's folder card renders this
 * section's confirmation constants and calls the same store action, so the two
 * surfaces cannot describe what removal keeps differently. The whole section is
 * capability-gated at its call site (`CapabilitiesVm.sync`): a machine with no
 * usable `git` gets no sync UI at all, never a disabled or failing one.
 *
 * Two rules this surface exists to honor:
 *   - `SyncStatusVm.line` is rendered verbatim. It is composed in Rust so the
 *     tray and this window can never word the same state differently.
 *   - Removing a profile is a configuration change: the folder, its contents
 *     and its git repository stay on disk, and the settings and the stored
 *     access token are the only things deleted. The confirmation says both
 *     halves in those words.
 */
import { useEffect, useState } from "react";
import { AddFolderForm, SYNC_ADD_TITLE, SYNC_EDIT_TITLE } from "@/components/sync/add-folder-form";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import type {
  SyncDeviceVm,
  SyncOutcomeVm,
  SyncProfileVm,
  SyncStatusVm,
  SyncVerifyVm,
} from "@/lib/ipc/client";
// Read and written straight through rather than through the mirror store: the
// device name is one app-global string nothing else changes, so mirroring it
// would be a second source of truth for a value read once per open. Opening a
// folder is here for the adjacent reason — it changes nothing, so there is no
// mirrored state for a store action to keep in step.
import {
  type SyncListSettingsVm,
  syncDevice,
  syncDeviceSetLabel,
  syncListSettingsGet,
  syncListSettingsSet,
  syncOpenPath,
} from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import {
  ensureSyncHydrated,
  isSyncStatusActive,
  removeSyncProfile,
  setSyncProfileEnabled,
  startSyncStatusPolling,
  syncErrorMessage,
  syncProfileNow,
  syncProgressFraction,
  useSyncStore,
  verifySyncProfile,
} from "@/lib/stores/sync";
import { setSyncListSizes } from "@/lib/stores/sync-detail";
import { cn } from "@/lib/utils";

/** Section heading. */
export const SYNC_SECTION_TITLE = "Sync";

/** The honest scope disclosure for the whole section (project voice). */
export const SYNC_SECTION_SENTENCE =
  "keeper syncs each folder below with the git remote you point it at, and nothing else. Pausing or removing a folder here changes only what keeper does — it never deletes what is in the folder.";

/** Shown once the mirror has loaded and there is genuinely nothing configured. */
export const SYNC_NO_PROFILES_SENTENCE = "No folders are set up to sync yet.";

/** Row action labels. */
export const SYNC_NOW_LABEL = "Sync now";
export const SYNC_PAUSE_LABEL = "Pause";
export const SYNC_RESUME_LABEL = "Resume";
export const SYNC_REMOVE_LABEL = "Remove";

/**
 * The folder-path control's verb, worded exactly as the export dialog and the
 * recording completion card word theirs: keeper has one way of saying "show me
 * this in the file manager" and this is it.
 */
export const SYNC_OPEN_PATH_LABEL = "Reveal in Finder";

/** Every folder card's action: re-check the folder's contents. On the card's own
 *  action row since story 56.14, not inside the needs-attention alert — the
 *  counts it reports are exactly what a HEALTHY folder needs to show. */
export const SYNC_VERIFY_LABEL = "Check files";

/**
 * The all-clear line after a check that found nothing wrong.
 *
 * Says what the check actually did. keeper records no per-file digest — the
 * pass reads every file (failing only if it changes under the read) and
 * confirms each large-file object is present at its recorded size. Claiming
 * "matched its recorded digest" described a comparison that never happens.
 *
 * It also stops short of promising that *every* large file's stored copy is
 * present: since Epic 56 a folder may deliberately keep only the pointer for
 * files its own policy authorizes to stay away, and the check counts those as
 * expected rather than as damage. Promising more than the pass checks is the
 * failure DW-1272 exists to prevent.
 */
export const SYNC_VERIFY_CLEAN_SENTENCE =
  "Every file read cleanly, and every large file kept on this machine has its stored copy.";

/**
 * What the pass looked at, said before what it found (Story 56.12).
 *
 * The all-clear sentence below stops short of promising that every large file's
 * copy is present, because since Epic 56 a folder may deliberately keep only
 * the pointer for files its own policy authorizes to stay away. That is honest
 * and it is also silent: "nothing was wrong" is what a folder with nothing in
 * it reports too. These two numbers are what make the difference visible —
 * `Engine::verify` has counted them since Story 56.6 and the IPC boundary threw
 * them away until now.
 *
 * The excused count is left out entirely when it is zero rather than rendered
 * as "0 kept away on purpose", following the folder footprint's rule: a
 * quantity nobody has is not a fact worth a clause.
 */
export function syncVerifyCountSentence(checked: number, virtualPaths: number): string {
  const read = `Read ${checked} file${checked === 1 ? "" : "s"}.`;
  if (virtualPaths === 0) {
    return read;
  }
  return `${read} ${virtualPaths} kept away on purpose.`;
}

/** Used only if Rust flagged attention without naming a reason. */
export const SYNC_ATTENTION_FALLBACK_SENTENCE =
  "This folder needs your attention before it can sync again.";

/** Accessible name for a row's progress meter. */
export const SYNC_PROGRESS_LABEL = "Sync progress";

/**
 * The remove confirmation, shared verbatim with the Sync view's folder card so
 * the two surfaces cannot promise different things about what removal keeps.
 *
 * It has to name both halves, because removal is not symmetric: the settings
 * and the stored access token go (AD-34-14 — the token's keychain key is
 * derived from the profile id, so one that outlived its folder could never be
 * found again, let alone deleted), while the folder, its contents and its git
 * repository are left exactly as they are.
 */
export const SYNC_REMOVE_CONFIRM_TITLE = "Stop syncing this folder?";
export const SYNC_REMOVE_CONFIRM_SENTENCE =
  "keeper stops syncing this folder, forgets its settings and deletes the access token it stored for the remote. The folder, its contents and its git repository are left on disk exactly as they are — removing a folder never deletes your files.";
export const SYNC_REMOVE_CONFIRM_LABEL = "Stop syncing";
export const SYNC_REMOVE_CANCEL_LABEL = "Keep syncing";

/**
 * The device name (Story 34.5). It is not a per-folder setting: one name goes
 * into every commit keeper makes, in every folder, plus the filename of any
 * second copy it has to keep — so it lives beside the folder list rather than in
 * the form.
 *
 * The note has to say what a rename does NOT do. Nothing rewrites history, so a
 * `git log` from before the rename keeps the name this machine had then, and
 * someone who renames a machine to correct a mistake should not be left
 * expecting old commits to change.
 */
export const SYNC_DEVICE_TITLE = "This device";
export const SYNC_DEVICE_NAME_LABEL = "Device name";
export const SYNC_DEVICE_NOTE =
  "keeper writes this name into every commit it makes, in every folder, and into the filename of any second copy it has to keep when two machines change the same file. Renaming it changes what later commits say — the ones already made keep the name this machine had then.";
export const SYNC_DEVICE_SAVE_LABEL = "Rename";
export const SYNC_DEVICE_SAVED_SENTENCE = "The next commit will carry the new name.";
/** The stable id, shown because it is in every trailer and never changes. */
export const SYNC_DEVICE_ID_LABEL = "Identifier";
export const SYNC_DEVICE_ID_NOTE =
  "Not editable and never changes — it is what tells this machine apart from another one with the same name.";

/**
 * The host a git remote points at — the part worth showing in a settings row.
 * Handles the scp-like `git@host:org/repo` form as well as a real URL, and
 * falls back to the raw string when it is neither, so a hand-written remote is
 * still shown honestly rather than blanked.
 */
export function syncRemoteHost(remoteUrl: string): string {
  const trimmed = remoteUrl.trim();
  const scpLike = /^[^/@]+@([^/:]+):/.exec(trimmed);
  if (scpLike !== null) {
    return scpLike[1];
  }
  try {
    const parsed = new URL(trimmed);
    return parsed.host === "" ? trimmed : parsed.host;
  } catch {
    return trimmed;
  }
}

/**
 * A synced folder's path, as the control that opens the folder (Story 32.4).
 *
 * The path was already on screen as plain text on both surfaces and there was no
 * way to reach the folder from the app at all, so the affordance is the thing a
 * person would reach for first: the path itself, a real button and therefore
 * keyboard-reachable. It passes the profile id — the folder is resolved in Rust
 * from the stored profile, so nothing here can ask to open an arbitrary
 * location.
 *
 * Shared by the Settings row and the Sync view's folder card for the reason the
 * removal confirmation is shared: two copies of one affordance drift.
 *
 * Gated on `capabilities.revealInFileManager` and inert text when it is off,
 * matching the recording completion card — a platform with no user-visible file
 * manager gets no affordance rather than one that fails on activation.
 */
export function SyncFolderPath({
  profile,
  className,
  onError,
}: {
  profile: SyncProfileVm;
  /** Text styling from the surface that owns the line: mono here, plain there. */
  className?: string;
  /** Where a refusal goes — the caller's existing action-error line. */
  onError: (message: string) => void;
}) {
  const canReveal = useCapabilitiesStore((s) => s.capabilities.revealInFileManager);

  if (!canReveal) {
    return (
      <span className={className} title={profile.localPath}>
        {profile.localPath}
      </span>
    );
  }

  return (
    <button
      type="button"
      // The visible text is a path, and a path does not say that activating it
      // opens that folder — so the accessible name carries the verb as well.
      // `title` stays the bare path: both surfaces truncate it, and reading the
      // whole thing on hover is what that attribute was already there for.
      aria-label={`${SYNC_OPEN_PATH_LABEL}: ${profile.localPath}`}
      title={profile.localPath}
      className={cn(
        "underline-offset-2 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring",
        className,
      )}
      onClick={() => {
        // Deliberately not through the caller's action lifecycle: opening a
        // folder changes nothing about it, so it must not take the busy lock or
        // clear the last sync report. A refusal — a folder that is gone, a
        // volume that is out — still lands where action errors are shown.
        void syncOpenPath(profile.id).catch((raw: unknown) => {
          onError(syncErrorMessage(raw));
        });
      }}
    >
      {profile.localPath}
    </button>
  );
}

/** List-size copy (the folder cards' Activity / Pending / Problems lists). */
export const SYNC_LISTS_TITLE = "List length";
export const SYNC_LISTS_NOTE =
  "How many rows each folder's Activity, Pending and Problems lists show before you unfold them, and after. The unfolded number is also how much history keeper reads at all, so raising it is what makes older entries reachable.";
export const SYNC_LISTS_FOLDED_LABEL = "Rows shown";
export const SYNC_LISTS_UNFOLDED_LABEL = "Rows when unfolded";
export const SYNC_LISTS_SAVE_LABEL = "Save";
export const SYNC_LISTS_SAVED_SENTENCE = "Saved. It applies on the next refresh.";

/**
 * The bounds, mirrored from `keeper-core::registry` for the number inputs' own
 * spinners and validation.
 *
 * Advisory only: Rust clamps on write and returns what it stored, so these
 * numbers going stale makes the stepper wrong, never the database. That is the
 * same division the recording settings use.
 */
export const SYNC_LIST_FOLDED_MIN = 1;
export const SYNC_LIST_FOLDED_MAX = 50;
export const SYNC_LIST_UNFOLDED_MIN = 10;
export const SYNC_LIST_UNFOLDED_MAX = 1000;

export function SyncSection({ open }: { open: boolean }) {
  const profiles = useSyncStore((state) => state.profiles);
  const statuses = useSyncStore((state) => state.statuses);
  const readError = useSyncStore((state) => state.error);

  /**
   * The list sizes, as text.
   *
   * Held as strings rather than numbers so a half-typed field is representable:
   * with `number` state, clearing the box to retype it becomes `NaN` (or snaps to
   * 0), and the input fights the user mid-edit. Rust clamps whatever is parsed
   * and hands back what it stored.
   */
  const [lists, setLists] = useState<SyncListSettingsVm | null>(null);
  const [foldedText, setFoldedText] = useState("");
  const [unfoldedText, setUnfoldedText] = useState("");
  const [listsBusy, setListsBusy] = useState(false);
  const [listsError, setListsError] = useState<string | null>(null);
  const [listsSaved, setListsSaved] = useState(false);

  // Hydrate on open and poll for as long as the dialog stays open; the stop
  // function tears the poll down on close (and on unmount).
  useEffect(() => {
    if (!open) {
      return;
    }
    void ensureSyncHydrated();
    let live = true;
    void (async () => {
      try {
        const read = await syncListSettingsGet();
        if (live) {
          setLists(read);
          setFoldedText(String(read.folded));
          setUnfoldedText(String(read.unfolded));
        }
      } catch (raw) {
        if (live) {
          setListsError(syncErrorMessage(raw));
        }
      }
    })();
    const stopPolling = startSyncStatusPolling();
    return () => {
      live = false;
      stopPolling();
    };
  }, [open]);

  const saveLists = async () => {
    setListsBusy(true);
    setListsError(null);
    setListsSaved(false);
    try {
      const stored = await syncListSettingsSet({
        folded: Number(foldedText),
        unfolded: Number(unfoldedText),
      });
      setLists(stored);
      // Seeded from what was STORED, exactly as the rename above is: Rust clamps,
      // and a box still showing 9000 would disagree with the database.
      setFoldedText(String(stored.folded));
      setUnfoldedText(String(stored.unfolded));
      // Adopted immediately so the open Sync view folds to the new numbers on its
      // next poll rather than at the next launch.
      setSyncListSizes(stored);
      setListsSaved(true);
    } catch (raw) {
      setListsError(syncErrorMessage(raw));
    } finally {
      setListsBusy(false);
    }
  };
  /**
   * Both fields must be whole numbers, and different from what is stored.
   *
   * `Number("")` is 0 and `Number("12abc")` is `NaN`, either of which would be
   * clamped into something the user did not ask for — so the button refuses
   * rather than silently saving a guess.
   */
  const listsDirty = (() => {
    if (lists === null) {
      return false;
    }
    const folded = Number(foldedText);
    const unfolded = Number(unfoldedText);
    if (!Number.isInteger(folded) || !Number.isInteger(unfolded)) {
      return false;
    }
    if (foldedText.trim() === "" || unfoldedText.trim() === "") {
      return false;
    }
    return folded !== lists.folded || unfolded !== lists.unfolded;
  })();

  return (
    <div className="mt-2 flex flex-col gap-3 border-border border-t pt-3 text-sm">
      <p className="font-medium">{SYNC_SECTION_TITLE}</p>
      <p className="text-muted-foreground">{SYNC_SECTION_SENTENCE}</p>
      {readError !== null && <p className="text-destructive text-xs">{readError}</p>}
      {/* Only claim "nothing configured" once a read has actually landed —
          before that the list is unknown, not empty. */}
      {profiles !== null && profiles.length === 0 && (
        <p className="text-muted-foreground">{SYNC_NO_PROFILES_SENTENCE}</p>
      )}
      {profiles?.map((profile) => {
        // A profile can exist before its first status snapshot arrives; the row
        // then shows no line rather than inventing one.
        const status: SyncStatusVm | undefined = statuses[profile.id];
        return <SyncProfileRow key={profile.id} profile={profile} status={status} />;
      })}
      <div className="mt-1 flex flex-col gap-2 border-border border-t pt-3">
        <p className="font-medium">{SYNC_ADD_TITLE}</p>
        {/* No `onCancel`, unlike the Sync view's disclosure: this form is a
            permanent part of the section, so a discard would have nothing to
            close and would have to mean "clear the fields" instead — a second
            meaning for the same control, on the one surface where abandoning a
            draft already costs a single click on the dialog. */}
        <AddFolderForm disabled={profiles === null} />
      </div>
      <div className="mt-1 flex flex-col gap-2 border-border border-t pt-3">
        <p className="font-medium">{SYNC_LISTS_TITLE}</p>
        <div className="flex items-center justify-between gap-2">
          <Label htmlFor="sync-list-folded">{SYNC_LISTS_FOLDED_LABEL}</Label>
          <Input
            id="sync-list-folded"
            className="w-24"
            type="number"
            inputMode="numeric"
            min={SYNC_LIST_FOLDED_MIN}
            max={SYNC_LIST_FOLDED_MAX}
            value={foldedText}
            disabled={lists === null || listsBusy}
            onChange={(event) => {
              setListsSaved(false);
              setFoldedText(event.target.value);
            }}
          />
        </div>
        <div className="flex items-center justify-between gap-2">
          <Label htmlFor="sync-list-unfolded">{SYNC_LISTS_UNFOLDED_LABEL}</Label>
          <div className="flex items-center gap-1">
            <Input
              id="sync-list-unfolded"
              className="w-24"
              type="number"
              inputMode="numeric"
              min={SYNC_LIST_UNFOLDED_MIN}
              max={SYNC_LIST_UNFOLDED_MAX}
              value={unfoldedText}
              disabled={lists === null || listsBusy}
              onChange={(event) => {
                setListsSaved(false);
                setUnfoldedText(event.target.value);
              }}
            />
            <Button
              type="button"
              variant="outline"
              size="xs"
              disabled={!listsDirty || listsBusy}
              onClick={() => {
                void saveLists();
              }}
            >
              {SYNC_LISTS_SAVE_LABEL}
            </Button>
          </div>
        </div>
        <p className="text-muted-foreground text-xs">{SYNC_LISTS_NOTE}</p>
        {listsSaved && <p className="text-muted-foreground text-xs">{SYNC_LISTS_SAVED_SENTENCE}</p>}
        {listsError !== null && <p className="text-destructive text-xs">{listsError}</p>}
      </div>
    </div>
  );
}

/** One configured folder: its Rust-composed line, where it lives, and its actions. */
function SyncProfileRow({
  profile,
  status,
}: {
  profile: SyncProfileVm;
  status: SyncStatusVm | undefined;
}) {
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  /**
   * What the last `Sync now` on this row did, or `null` before one has run.
   *
   * Held here rather than read off the polled status: a pass that stages
   * nothing finishes in milliseconds while the status poll runs at 2 s (10 s
   * when idle), so a report that waited for the poll would arrive long after
   * the click, if the status moved at all. It never has to — "nothing to do"
   * leaves the status exactly as it was.
   */
  const [outcome, setOutcome] = useState<SyncOutcomeVm | null>(null);
  /**
   * The last check's whole report, or `null` before one has run. The faults
   * alone were never enough to tell a clean pass from a pass that did not
   * happen; the two counts beside them are (Story 56.12).
   */
  const [verified, setVerified] = useState<SyncVerifyVm | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  /**
   * Whether this row's configuration is open for editing. Per row, because the
   * form it reveals is about this folder and nothing else.
   */
  const [editing, setEditing] = useState(false);

  /** Run a row action with the shared busy/error lifecycle. */
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setActionError(null);
    // Any action supersedes the last sync report: Pause and Remove make it
    // stale, and a fresh Sync now is about to replace it.
    setOutcome(null);
    try {
      await action();
    } catch (raw) {
      setActionError(syncErrorMessage(raw));
    } finally {
      setBusy(false);
    }
  };

  // `null` = no total known, which the meter renders as indeterminate.
  const fraction = status === undefined ? null : syncProgressFraction(status);
  const percent = fraction === null ? null : Math.round(fraction * 100);

  return (
    <div className="flex flex-col gap-1 rounded-md border border-border p-2">
      <div className="flex items-start justify-between gap-2">
        <div className="flex min-w-0 flex-col gap-0.5">
          <p className="font-medium">{profile.name}</p>
          {/* Verbatim, never recomposed: the tray renders this same sentence.
              A sentence is the room's voice; `figures` only keeps the counts in
              it from reflowing as they tick. */}
          {status !== undefined && (
            <span className="figures text-muted-foreground text-xs">{status.line}</span>
          )}
          <SyncFolderPath
            profile={profile}
            className="block max-w-full truncate text-left text-muted-foreground text-xs"
            onError={setActionError}
          />
          <p className="text-muted-foreground text-xs">{syncRemoteHost(profile.remoteUrl)}</p>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button
            type="button"
            variant="outline"
            size="xs"
            disabled={busy}
            onClick={() => {
              void run(async () => {
                setOutcome(await syncProfileNow(profile.id));
              });
            }}
          >
            {SYNC_NOW_LABEL}
          </Button>
          {/* Story 56.14: on EVERY folder, healthy or not.

              It used to live only inside the needs-attention alert below, which
              made the two counts story 56.12 added unreachable in exactly the
              case they were added for: `syncVerifyCountSentence`'s own argument
              is that "nothing was wrong" is also what a check over an empty
              folder reports, so the number that separates them — how many paths
              were excused because their content is away on purpose — could only
              be seen on a folder keeper had already flagged. A healthy virtual
              folder, the ordinary case, could never show it.

              Moved rather than duplicated. Two controls with one label in one
              card is two things to keep in step and a reader wondering whether
              they do the same thing; the alert keeps its sentence, and the
              report still renders under it either way. */}
          <Button
            type="button"
            variant="outline"
            size="xs"
            disabled={busy}
            onClick={() => {
              void run(async () => {
                setVerified(await verifySyncProfile(profile.id));
              });
            }}
          >
            {SYNC_VERIFY_LABEL}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="xs"
            disabled={busy}
            onClick={() => {
              void run(async () => {
                await setSyncProfileEnabled(profile.id, !profile.enabled);
              });
            }}
          >
            {profile.enabled ? SYNC_PAUSE_LABEL : SYNC_RESUME_LABEL}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            aria-expanded={editing}
            disabled={busy}
            onClick={() => setEditing((shown) => !shown)}
          >
            {SYNC_EDIT_TITLE}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            disabled={busy}
            onClick={() => setConfirmOpen(true)}
          >
            {SYNC_REMOVE_LABEL}
          </Button>
        </div>
      </div>
      {status !== undefined && isSyncStatusActive(status) && (
        // Indeterminate (no `aria-valuenow`) whenever no total is known — a
        // meter that invents a percentage is worse than one that admits it
        // cannot say. The shadcn wrapper spends `value` on the bar width and
        // never forwards it to Radix (so its `data-state` reads indeterminate
        // either way), which is why the value semantics are stated here:
        // `aria-valuenow` against Radix's 0..100 range, and an `aria-valuetext`
        // that reuses the Rust-composed line rather than wording progress a
        // second way.
        <Progress
          className="mt-1"
          value={percent}
          aria-label={`${SYNC_PROGRESS_LABEL}: ${profile.name}`}
          aria-valuenow={percent ?? undefined}
          aria-valuetext={status.line}
        />
      )}
      {status?.needsAttention === true && (
        // Persistent and non-modal, the ConversationHealthBanner shape: this
        // condition needs a human, so there is no dismiss control — it clears
        // when the profile recovers, not when it is waved away.
        //
        // No inline action since story 56.14: `Check files` was here and nowhere
        // else, which put the counts out of reach of every healthy folder. It is
        // now in the card's own action row above, one line up and always present.
        <Alert role="alert" variant="destructive" className="mt-1">
          <AlertDescription>
            {status.error ?? status.warning ?? SYNC_ATTENTION_FALLBACK_SENTENCE}
          </AlertDescription>
        </Alert>
      )}
      {verified !== null && (
        <>
          {/* What the pass looked at, before what it found. Story 56.6 taught
              verify to excuse a path whose content is away on purpose, and
              until now that excuse was invisible: the folder simply reported
              nothing wrong, which is also what a check over an empty folder
              reports. */}
          <p className="text-muted-foreground text-xs">
            {syncVerifyCountSentence(verified.checked, verified.virtualPaths)}
          </p>
          {verified.problems.length === 0 ? (
            <p className="text-muted-foreground text-xs">{SYNC_VERIFY_CLEAN_SENTENCE}</p>
          ) : (
            <ul className="flex flex-col gap-0.5">
              {verified.problems.map((problem) => (
                <li key={problem} className="text-destructive text-xs">
                  {problem}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
      {outcome !== null && (
        // Inline, in the row that was acted on, exactly where Check files
        // already reports what it found — nothing on the sync surfaces is
        // ephemeral, and a toast for the result of a button in view would be
        // the one exception. Rust composed the sentence (`SyncOutcomeVm.line`)
        // for the same reason it composes the status line: the Sync view says
        // it in these words too. `role="status"` announces it without stealing
        // focus; conflicts take the destructive colour because "both versions
        // survive, go and look" is not a checkmark, but they are non-blocking
        // by contract, so this is never an interrupting alert.
        <p
          role="status"
          className={
            outcome.conflicts.length > 0
              ? "text-destructive text-xs"
              : "text-muted-foreground text-xs"
          }
        >
          {outcome.line}
        </p>
      )}
      {actionError !== null && <p className="text-destructive text-xs">{actionError}</p>}
      {/* Under everything the row reports about the folder, so opening it never
          pushes a live warning off the top. Mounted fresh on each open, so it
          seeds from the profile as it is now; `settled` false leaves it
          standing over a keychain failure nothing else here would report. */}
      {editing && (
        <AddFolderForm
          className="mt-1 border-border border-t pt-2"
          profile={profile}
          onSaved={(_saved, settled) => setEditing(!settled)}
          onCancel={() => setEditing(false)}
        />
      )}
      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{SYNC_REMOVE_CONFIRM_TITLE}</AlertDialogTitle>
            <AlertDialogDescription>{SYNC_REMOVE_CONFIRM_SENTENCE}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{SYNC_REMOVE_CANCEL_LABEL}</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                void run(async () => {
                  await removeSyncProfile(profile.id);
                });
              }}
            >
              {SYNC_REMOVE_CONFIRM_LABEL}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

/**
 * This machine's name and id.
 *
 * Its own section rather than a block inside Sync. The label is not a sync
 * setting: it rides every commit's `Keeper-Device` trailer, names this machine in
 * conflict-copy filenames, and now qualifies the commit subject itself
 * (`sync(tgdrive@hesperia)`). Someone looking for "what is this Mac called"
 * should not have to know that folder sync is where it was filed.
 *
 * Still rendered only where sync exists, because the identity is read and written
 * through the sync engine's own IPC — there is nothing to show on a machine that
 * has none.
 */
export function DeviceSection({ open }: { open: boolean }) {
  const [device, setDevice] = useState<SyncDeviceVm | null>(null);
  const [deviceName, setDeviceName] = useState("");
  const [deviceBusy, setDeviceBusy] = useState(false);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  /** Whether a rename went through; nothing else on screen would report it. */
  const [deviceRenamed, setDeviceRenamed] = useState(false);

  useEffect(() => {
    if (!open) {
      return;
    }
    let live = true;
    void (async () => {
      try {
        const read = await syncDevice();
        if (live) {
          setDevice(read);
          setDeviceName(read.label);
        }
      } catch (raw) {
        if (live) {
          setDeviceError(syncErrorMessage(raw));
        }
      }
    })();
    return () => {
      live = false;
    };
  }, [open]);

  const renameDevice = async () => {
    setDeviceBusy(true);
    setDeviceError(null);
    setDeviceRenamed(false);
    try {
      const stored = await syncDeviceSetLabel(deviceName);
      setDevice(stored);
      // Seeded from what was STORED, not from what was typed: Rust trims, and a
      // box that kept the untrimmed text would disagree with the trailers.
      setDeviceName(stored.label);
      setDeviceRenamed(true);
    } catch (raw) {
      setDeviceError(syncErrorMessage(raw));
    } finally {
      setDeviceBusy(false);
    }
  };
  const renameable =
    device !== null && deviceName.trim() !== "" && deviceName.trim() !== device.label;

  return (
    <div className="mt-2 flex flex-col gap-3 border-border border-t pt-3 text-sm">
      <p className="font-medium">{SYNC_DEVICE_TITLE}</p>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor="sync-device-name">{SYNC_DEVICE_NAME_LABEL}</Label>
        <div className="flex items-center gap-1">
          <Input
            id="sync-device-name"
            className="w-56"
            value={deviceName}
            disabled={device === null || deviceBusy}
            onChange={(event) => {
              setDeviceRenamed(false);
              setDeviceName(event.target.value);
            }}
          />
          <Button
            type="button"
            variant="outline"
            size="xs"
            disabled={!renameable || deviceBusy}
            onClick={() => {
              void renameDevice();
            }}
          >
            {SYNC_DEVICE_SAVE_LABEL}
          </Button>
        </div>
      </div>
      <p className="text-muted-foreground text-xs">{SYNC_DEVICE_NOTE}</p>
      {deviceRenamed && (
        <p className="text-muted-foreground text-xs">{SYNC_DEVICE_SAVED_SENTENCE}</p>
      )}
      {deviceError !== null && <p className="text-destructive text-xs">{deviceError}</p>}
      {device !== null && (
        <p className="text-muted-foreground text-xs">
          {/* The label is prose and the id is a hash: only the second half is
              the register's, and setting the whole line in mono would say the
              words were data too. */}
          {SYNC_DEVICE_ID_LABEL}: <span className="font-mono">{device.id}</span>
        </p>
      )}
      {device !== null && <p className="text-muted-foreground text-xs">{SYNC_DEVICE_ID_NOTE}</p>}
    </div>
  );
}
