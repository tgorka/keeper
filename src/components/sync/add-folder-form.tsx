/**
 * The add-a-folder form (Epic 29 Story 29.5, Epic 32 Story 32.7, Epic 33
 * Story 33.4).
 *
 * One component rendered in two places (AD-C7): Settings → Sync, and the Sync
 * view — inline in its empty state, and behind an "Add a folder" action once a
 * folder exists. Two copies would drift, and this form carries rules that are
 * not obvious from a field list:
 *   - The `worktree` lane is an agent airlock Rust accepts only together with
 *     `pushOnly`, so the form never offers it and always sends
 *     {@link SYNC_DEFAULT_LANE}. A second copy could easily grow a lane control
 *     that composes a profile Rust rejects.
 *   - The settle window and LFS threshold are seconds/MB here and
 *     milliseconds/bytes on the wire, and an empty field means "let Rust
 *     choose" rather than zero.
 *   - The access token is a second write to a different store (the OS
 *     keychain), keyed by the id the save mints. Its failure is reported as its
 *     own thing, because the folder exists by then.
 *
 * The heading is deliberately not part of this component: each surface titles
 * it in its own chrome — Settings with a section heading, the Sync view with
 * the disclosure button that revealed it. The `<form>` carries
 * {@link SYNC_ADD_TITLE} as its accessible name instead, so it is named for a
 * screen reader wherever it is rendered, including where nothing visible
 * repeats the title.
 */
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronRight } from "lucide-react";
import { type FormEvent, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { SyncProfileVm } from "@/lib/ipc/client";
// The credential pair is called straight from the form rather than through the
// mirror store: neither write changes anything the store mirrors, and there is
// no read side to keep in sync — nothing can report what a keychain holds.
import { syncClearCredential, syncSetCredential } from "@/lib/ipc/client";
import {
  SYNC_DEFAULT_BRANCH,
  SYNC_DEFAULT_LANE,
  SYNC_DEFAULT_LFS_THRESHOLD_BYTES,
  type SyncDirection,
  type SyncLfsMode,
  saveSyncProfile,
  syncErrorMessage,
} from "@/lib/stores/sync";
import { refreshSyncDetail } from "@/lib/stores/sync-detail";
import { cn } from "@/lib/utils";

/**
 * The form's title. Doubles as the label of the Sync view's header action,
 * because the action and the thing it reveals must not be worded differently.
 */
export const SYNC_ADD_TITLE = "Add a folder";

/** Field labels. */
export const SYNC_NAME_LABEL = "Name";
export const SYNC_FOLDER_LABEL = "Folder";
export const SYNC_CHOOSE_FOLDER_LABEL = "Choose folder";
export const SYNC_NO_FOLDER_CHOSEN_LABEL = "No folder chosen";
export const SYNC_REMOTE_URL_LABEL = "Remote URL";
export const SYNC_BRANCH_LABEL = "Branch";
export const SYNC_DIRECTION_LABEL = "Direction";
export const SYNC_ADVANCED_LABEL = "Advanced options";
export const SYNC_LFS_MODE_LABEL = "Large files";
export const SYNC_LFS_THRESHOLD_LABEL = "Track files at or above (MB)";
export const SYNC_REMOVABLE_LABEL = "This folder is on removable or network storage";
export const SYNC_REMOVABLE_NOTE =
  "keeper waits longer for writes to settle there, because those volumes report changes late.";

/**
 * The advanced knobs (Story 32.7, AD-S8). Every one of these was already
 * carried by `SyncProfileReq` and hardcoded away by this form; a knob that
 * does nothing (`pollIntervalMs`) was deleted in Rust rather than exposed here.
 */
export const SYNC_SETTLE_LABEL = "Wait for writes to stop (seconds)";
export const SYNC_SETTLE_NOTE =
  "How long a file must go untouched before keeper copies it. Left empty, keeper picks the wait itself.";
export const SYNC_EXCLUDES_LABEL = "Skip these files";
export const SYNC_EXCLUDES_NOTE =
  "Comma-separated patterns, for example *.tmp, .DS_Store. Left empty, keeper syncs everything in the folder.";
export const SYNC_SUBPATHS_LABEL = "Sync only these folders";
export const SYNC_SUBPATHS_NOTE =
  "Comma-separated paths inside the folder. Left empty, keeper syncs the whole folder.";
export const SYNC_TAGS_LABEL = "Tags";
export const SYNC_TAGS_NOTE =
  "Comma-separated. Each tag is written into the message of every commit keeper makes for this folder.";
export const SYNC_AUTHOR_LABEL = "Commit author";
export const SYNC_AUTHOR_NOTE =
  "Name <email>, an address, or just a name. Left empty, commits are authored by this device.";

/**
 * The access-token field (Story 32.7, AD-S7). The token is written to the OS
 * keychain in a second call once the profile has an id, and is never rendered
 * back: no command can read a stored token out, so a field that claimed to
 * show one would be showing something it cannot know.
 */
export const SYNC_TOKEN_LABEL = "Access token";
export const SYNC_TOKEN_NOTE =
  "Used to authenticate with the remote. keeper stores it in the system keychain when the folder is added and never shows it again — there is no way to read a stored token back out.";
export const SYNC_TOKEN_STORED_LABEL = "Access token stored in the keychain.";
export const SYNC_TOKEN_CLEAR_LABEL = "Clear token";

/**
 * Reported when the profile was created but the keychain write was not. Two
 * writes, two outcomes: saying "add failed" here would send the user back to a
 * form that can now only reject as a duplicate.
 */
export const SYNC_TOKEN_FAILED_PREFIX = "The folder was added, but the token was not stored: ";
export const SYNC_ADD_SUBMIT_LABEL = "Add folder";

/** Test id for the form's chosen-folder display (a read-only truncated path). */
export const SYNC_FORM_PATH_TESTID = "sync-form-path";

/** Test id for the Advanced disclosure toggle. */
export const SYNC_ADVANCED_TOGGLE_TESTID = "sync-advanced-toggle";

/** Test id for the direction Select trigger. */
export const SYNC_DIRECTION_SELECT_TESTID = "sync-direction-select";

/** Test id for the LFS-mode Select trigger. */
export const SYNC_LFS_SELECT_TESTID = "sync-lfs-select";

/** Honest labels for the three directions (the values mirror Rust). */
const DIRECTION_LABELS: Record<SyncDirection, string> = {
  bidirectional: "Both ways",
  pushOnly: "Push only",
  pullOnly: "Pull only",
};

/** Honest labels for the three LFS modes (the values mirror Rust). */
const LFS_MODE_LABELS: Record<SyncLfsMode, string> = {
  materialize: "Download large files",
  pointerOnly: "Keep pointers only",
  disabled: "Do not use LFS",
};

/** The form's fields, all as typed. */
interface SyncFormValues {
  name: string;
  localPath: string;
  remoteUrl: string;
  branch: string;
  direction: SyncDirection;
  lfsMode: SyncLfsMode;
  /** The LFS threshold in MB; empty defers to the Rust default. */
  lfsThresholdMb: string;
  removable: boolean;
  /** The settle window in seconds; empty defers to the Rust default. */
  settleSeconds: string;
  /** Comma-separated patterns; empty excludes nothing. */
  excludes: string;
  /** Comma-separated paths inside the folder; empty syncs all of it. */
  subpaths: string;
  /** Comma-separated commit tags; empty adds no tag trailers. */
  tags: string;
  /** The commit-author override; empty keeps the device identity. */
  authorOverride: string;
  /**
   * The access token, held only until the profile is saved and the keychain
   * write goes through. Never read back from anywhere.
   */
  token: string;
}

const EMPTY_FORM: SyncFormValues = {
  name: "",
  localPath: "",
  remoteUrl: "",
  branch: SYNC_DEFAULT_BRANCH,
  direction: "bidirectional",
  lfsMode: "materialize",
  lfsThresholdMb: String(SYNC_DEFAULT_LFS_THRESHOLD_BYTES / 1024 / 1024),
  removable: false,
  settleSeconds: "",
  excludes: "",
  subpaths: "",
  tags: "",
  authorOverride: "",
  token: "",
};

/**
 * Split one comma-separated advanced field into the string list Rust expects,
 * dropping blanks so a trailing comma or a stray space never reaches the engine
 * as an empty pattern that matches everything.
 */
function splitSyncList(raw: string): string[] {
  return raw
    .split(",")
    .map((entry) => entry.trim())
    .filter((entry) => entry !== "");
}

/**
 * Create a profile. A rejected save keeps every typed value and shows the
 * Rust validation message inline — losing a half-typed remote URL to a
 * validation error would be infuriating.
 *
 * @param disabled - Suppress every control while the caller's mirror is still
 *   unknown, so a folder cannot be added against a list nobody has read yet.
 * @param onAdded - Called after a folder is created, with `settled` false
 *   while the form is still showing something only it can show: a keychain
 *   failure, or the acknowledgement whose Clear button is the only way to undo
 *   a stored token. A surface that hides the form on success must keep it
 *   mounted until `settled`, or it destroys the one place either is readable.
 */
export function AddFolderForm({
  disabled = false,
  className,
  onAdded,
}: {
  disabled?: boolean;
  className?: string;
  onAdded?: (profile: SyncProfileVm, settled: boolean) => void;
}) {
  const [form, setForm] = useState<SyncFormValues>(EMPTY_FORM);
  const [expanded, setExpanded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /**
   * The profile whose token was just written, if any. The only acknowledgement
   * the user will ever get that the keychain took it — and the only moment the
   * form holds an id to clear it with, since nothing can read a stored token
   * back to offer this later.
   */
  const [storedToken, setStoredToken] = useState<{ id: string; name: string } | null>(null);
  const fieldId = useId();

  /** Open the OS-native directory picker; a cancellation writes nothing. */
  const pickFolder = async () => {
    try {
      const selection = await openFolder({ directory: true });
      if (typeof selection === "string") {
        // Merge into the *live* values, not the render snapshot closed over
        // before the picker opened, so a field typed meanwhile survives.
        setForm((live) => ({ ...live, localPath: selection }));
      }
    } catch {
      // Picker cancellation / failure → keep the current folder (no write).
    }
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSaving(true);
    setError(null);
    // The acknowledgement belongs to the folder that was just added, so a
    // second add never leaves the previous one's Clear button pointing at it.
    setStoredToken(null);
    // An empty or unusable threshold / settle window defers to the Rust
    // default rather than sending a number nobody asked for.
    const thresholdMb = Number.parseFloat(form.lfsThresholdMb);
    const settleSeconds = Number.parseFloat(form.settleSeconds);
    const author = form.authorOverride.trim();
    const { token } = form;
    try {
      const saved = await saveSyncProfile({
        id: null,
        name: form.name.trim(),
        localPath: form.localPath,
        remoteUrl: form.remoteUrl.trim(),
        branch: form.branch.trim(),
        direction: form.direction,
        // The other lane (`worktree`) is an agent airlock Rust accepts only
        // together with `pushOnly`, so it stays a syncd-config affordance
        // rather than a control that can be combined into a rejected profile.
        lane: SYNC_DEFAULT_LANE,
        subpaths: splitSyncList(form.subpaths),
        excludes: splitSyncList(form.excludes),
        removable: form.removable,
        lfsMode: form.lfsMode,
        lfsThresholdBytes:
          Number.isFinite(thresholdMb) && thresholdMb > 0
            ? Math.round(thresholdMb * 1024 * 1024)
            : null,
        settleMs:
          Number.isFinite(settleSeconds) && settleSeconds > 0
            ? Math.round(settleSeconds * 1000)
            : null,
        tags: splitSyncList(form.tags),
        // `null` keeps the device identity; an empty string would *clear* an
        // override, which is meaningless on a profile being created.
        authorOverride: author === "" ? null : author,
      });
      setForm(EMPTY_FORM);
      // `saveSyncProfile` re-reads the profile/status mirror, but the Sync
      // view's three per-folder lists are a *second* mirror on a deliberately
      // slower poll. Reading them here — from whichever surface added the
      // folder — is what keeps its card from sitting blank for a poll
      // interval. Never throws.
      await refreshSyncDetail(saved.id);
      if (token !== "") {
        // A second write, because the keychain entry is keyed by the id the
        // save just minted. Its failure is reported as its own thing: the
        // folder exists by now, and "add failed" would be a lie that sends the
        // user back to a form that would only reject as a duplicate.
        try {
          await syncSetCredential(saved.id, token);
          setStoredToken({ id: saved.id, name: saved.name });
        } catch (raw) {
          setError(`${SYNC_TOKEN_FAILED_PREFIX}${syncErrorMessage(raw)}`);
        }
        // Either way the form now holds something only it can show — the
        // acknowledgement with its Clear button, or the keychain failure — so
        // the caller is told the folder exists but must not hide the form yet.
        onAdded?.(saved, false);
        return;
      }
      onAdded?.(saved, true);
    } catch (raw) {
      setError(syncErrorMessage(raw));
    } finally {
      setSaving(false);
    }
  };

  /** Forget a just-stored token. The only undo there is — nothing reads one back. */
  const clearToken = async (id: string) => {
    setSaving(true);
    setError(null);
    try {
      await syncClearCredential(id);
      setStoredToken(null);
    } catch (raw) {
      setError(syncErrorMessage(raw));
    } finally {
      setSaving(false);
    }
  };

  const incomplete =
    form.name.trim() === "" || form.localPath === "" || form.remoteUrl.trim() === "";

  return (
    <form
      aria-label={SYNC_ADD_TITLE}
      className={cn("flex flex-col gap-2", className)}
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-name`}>{SYNC_NAME_LABEL}</Label>
        <Input
          id={`${fieldId}-name`}
          className="w-56"
          value={form.name}
          disabled={disabled || saving}
          onChange={(event) => setForm((live) => ({ ...live, name: event.target.value }))}
        />
      </div>
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 flex-col gap-0.5">
          <Label>{SYNC_FOLDER_LABEL}</Label>
          <p
            className="truncate font-mono text-muted-foreground text-xs"
            data-testid={SYNC_FORM_PATH_TESTID}
            title={form.localPath === "" ? undefined : form.localPath}
          >
            {form.localPath === "" ? SYNC_NO_FOLDER_CHOSEN_LABEL : form.localPath}
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0"
          disabled={disabled || saving}
          onClick={() => {
            void pickFolder();
          }}
        >
          {SYNC_CHOOSE_FOLDER_LABEL}
        </Button>
      </div>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-remote`}>{SYNC_REMOTE_URL_LABEL}</Label>
        <Input
          id={`${fieldId}-remote`}
          className="w-56"
          value={form.remoteUrl}
          disabled={disabled || saving}
          onChange={(event) => setForm((live) => ({ ...live, remoteUrl: event.target.value }))}
        />
      </div>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-branch`}>{SYNC_BRANCH_LABEL}</Label>
        <Input
          id={`${fieldId}-branch`}
          className="w-32"
          value={form.branch}
          disabled={disabled || saving}
          onChange={(event) => setForm((live) => ({ ...live, branch: event.target.value }))}
        />
      </div>
      <div className="flex items-center justify-between gap-2">
        <Label id={`${fieldId}-direction-label`}>{SYNC_DIRECTION_LABEL}</Label>
        <Select
          value={form.direction}
          disabled={disabled || saving}
          onValueChange={(value) =>
            setForm((live) => ({ ...live, direction: value as SyncDirection }))
          }
        >
          <SelectTrigger
            className="w-40"
            data-testid={SYNC_DIRECTION_SELECT_TESTID}
            aria-labelledby={`${fieldId}-direction-label`}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {Object.entries(DIRECTION_LABELS).map(([value, label]) => (
              <SelectItem key={value} value={value}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="w-fit justify-start gap-1 px-1"
        data-testid={SYNC_ADVANCED_TOGGLE_TESTID}
        aria-expanded={expanded}
        onClick={() => setExpanded((shown) => !shown)}
      >
        {expanded ? <ChevronDown aria-hidden /> : <ChevronRight aria-hidden />}
        {SYNC_ADVANCED_LABEL}
      </Button>
      {expanded && (
        <div className="flex flex-col gap-2 pl-1">
          <div className="flex items-center justify-between gap-2">
            <Label id={`${fieldId}-lfs-label`}>{SYNC_LFS_MODE_LABEL}</Label>
            <Select
              value={form.lfsMode}
              disabled={disabled || saving}
              onValueChange={(value) =>
                setForm((live) => ({ ...live, lfsMode: value as SyncLfsMode }))
              }
            >
              <SelectTrigger
                className="w-52"
                data-testid={SYNC_LFS_SELECT_TESTID}
                aria-labelledby={`${fieldId}-lfs-label`}
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.entries(LFS_MODE_LABELS).map(([value, label]) => (
                  <SelectItem key={value} value={value}>
                    {label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-threshold`}>{SYNC_LFS_THRESHOLD_LABEL}</Label>
            <Input
              id={`${fieldId}-threshold`}
              type="number"
              min={0}
              className="w-24"
              value={form.lfsThresholdMb}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, lfsThresholdMb: event.target.value }))
              }
            />
          </div>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-removable`}>{SYNC_REMOVABLE_LABEL}</Label>
            <Checkbox
              id={`${fieldId}-removable`}
              checked={form.removable}
              disabled={disabled || saving}
              onCheckedChange={(checked) =>
                setForm((live) => ({ ...live, removable: checked === true }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_REMOVABLE_NOTE}</p>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-settle`}>{SYNC_SETTLE_LABEL}</Label>
            <Input
              id={`${fieldId}-settle`}
              type="number"
              min={0}
              className="w-24"
              value={form.settleSeconds}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, settleSeconds: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_SETTLE_NOTE}</p>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-excludes`}>{SYNC_EXCLUDES_LABEL}</Label>
            <Input
              id={`${fieldId}-excludes`}
              className="w-56"
              value={form.excludes}
              disabled={disabled || saving}
              onChange={(event) => setForm((live) => ({ ...live, excludes: event.target.value }))}
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_EXCLUDES_NOTE}</p>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-subpaths`}>{SYNC_SUBPATHS_LABEL}</Label>
            <Input
              id={`${fieldId}-subpaths`}
              className="w-56"
              value={form.subpaths}
              disabled={disabled || saving}
              onChange={(event) => setForm((live) => ({ ...live, subpaths: event.target.value }))}
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_SUBPATHS_NOTE}</p>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-tags`}>{SYNC_TAGS_LABEL}</Label>
            <Input
              id={`${fieldId}-tags`}
              className="w-56"
              value={form.tags}
              disabled={disabled || saving}
              onChange={(event) => setForm((live) => ({ ...live, tags: event.target.value }))}
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_TAGS_NOTE}</p>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-author`}>{SYNC_AUTHOR_LABEL}</Label>
            <Input
              id={`${fieldId}-author`}
              className="w-56"
              value={form.authorOverride}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, authorOverride: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_AUTHOR_NOTE}</p>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-token`}>{SYNC_TOKEN_LABEL}</Label>
            {/* A write-only field: typed here, handed to the keychain once the
                profile has an id, and never rendered back — there is no command
                that could fill it in. */}
            <Input
              id={`${fieldId}-token`}
              type="password"
              autoComplete="off"
              className="w-56"
              value={form.token}
              disabled={disabled || saving}
              onChange={(event) => setForm((live) => ({ ...live, token: event.target.value }))}
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_TOKEN_NOTE}</p>
        </div>
      )}
      {/* Outside the disclosure on purpose: this is the acknowledgement for an
          action that already happened, and it would be invisible collapsed. */}
      {storedToken !== null && (
        <div className="flex items-center justify-between gap-2">
          <p className="text-muted-foreground text-xs">
            {SYNC_TOKEN_STORED_LABEL} ({storedToken.name})
          </p>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            className="shrink-0"
            disabled={saving}
            onClick={() => {
              void clearToken(storedToken.id);
            }}
          >
            {SYNC_TOKEN_CLEAR_LABEL}
          </Button>
        </div>
      )}
      {error !== null && <p className="text-destructive text-xs">{error}</p>}
      <Button
        type="submit"
        variant="outline"
        size="sm"
        className="w-fit"
        disabled={disabled || saving || incomplete}
      >
        {SYNC_ADD_SUBMIT_LABEL}
      </Button>
    </form>
  );
}
