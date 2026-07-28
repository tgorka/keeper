/**
 * The add-and-edit folder form (Epic 29 Story 29.5, Epic 32 Story 32.7, Epic 33
 * Story 33.4).
 *
 * One component rendered in two places (AD-C7) — Settings → Sync, and the Sync
 * view — and in two modes: given a {@link SyncProfileVm} it edits that profile,
 * without one it creates a folder. Two copies would drift, and an edit-only
 * second copy would drift hardest, because this form carries rules that are not
 * obvious from a field list:
 *   - The `worktree` lane is an agent airlock Rust accepts only together with
 *     `pushOnly`, so the form never offers it and always sends
 *     {@link SYNC_DEFAULT_LANE}. A second copy could easily grow a lane control
 *     that composes a profile Rust rejects.
 *   - The settle window and LFS threshold are seconds/MB here and
 *     milliseconds/bytes on the wire, and an empty field means "let Rust
 *     choose" rather than zero.
 *   - The access token is a second write to a different store (the OS
 *     keychain), keyed by the profile id. Its failure is reported as its own
 *     thing, because the profile is stored by then.
 *   - `SyncProfileReq` carries no `enabled`, and Rust merges an update onto the
 *     stored profile rather than rebuilding it, so saving an edit leaves a
 *     paused folder paused. An edit must therefore never be routed through
 *     anything that also toggles pause.
 *   - The local path is not editable. The engine binds a profile to its folder
 *     — and on removable media to a marker written inside it — so repointing a
 *     profile is not an edit to it but a different folder.
 *
 * The heading is deliberately not part of this component: each surface titles
 * it in its own chrome — Settings with a section heading, the Sync view with
 * the disclosure button that revealed it. The `<form>` carries
 * {@link SYNC_ADD_TITLE}, or {@link SYNC_EDIT_TITLE} and the folder's name, as
 * its accessible name instead, so it is named for a screen reader wherever it
 * is rendered, including where nothing visible repeats the title.
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
  SYNC_DIRECTIONS,
  SYNC_LFS_MODES,
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

/**
 * The edit form's title, and the label of every per-folder action that reveals
 * it — the action and the thing it reveals must not be worded differently. The
 * form's accessible name appends the folder's own name, because several of
 * them can be open at once.
 */
export const SYNC_EDIT_TITLE = "Edit folder";

/** Field labels. */
export const SYNC_NAME_LABEL = "Name";
export const SYNC_FOLDER_LABEL = "Folder";
export const SYNC_CHOOSE_FOLDER_LABEL = "Choose folder";
export const SYNC_NO_FOLDER_CHOSEN_LABEL = "No folder chosen";

/**
 * Why an existing profile's folder is shown but not offered. Explaining beats
 * dropping the field: a path that quietly vanished in edit mode would read as
 * one the form forgot rather than one the engine holds fixed.
 */
export const SYNC_PATH_FIXED_NOTE =
  "keeper binds a folder to this profile when it is set up — on removable storage, to a marker written inside it — so the path stays fixed. Syncing a different folder means adding that one separately.";
export const SYNC_REMOTE_URL_LABEL = "Remote URL";
export const SYNC_BRANCH_LABEL = "Branch";
export const SYNC_DIRECTION_LABEL = "Direction";
export const SYNC_ADVANCED_LABEL = "Advanced options";
export const SYNC_LFS_MODE_LABEL = "Large files";
export const SYNC_LFS_THRESHOLD_LABEL = "Track files at or above (MB)";
export const SYNC_REMOVABLE_LABEL = "This folder is on removable or network storage";
export const SYNC_REMOVABLE_NOTE =
  "keeper marks the drive and follows it, so unplugging pauses this folder instead of syncing " +
  "everything on it as deleted. It also waits longer for writes to settle, because those volumes " +
  "report changes late. The folder has to be on the drive, not on this computer's own disk.";

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
 * show one would be showing something it cannot know. That is exactly why
 * editing a profile still starts this field empty, and why the note under it
 * has to say what empty *means* there.
 */
export const SYNC_TOKEN_LABEL = "Access token";
export const SYNC_TOKEN_NOTE =
  "Used to authenticate with the remote. keeper stores it in the system keychain when the folder is added and never shows it again — there is no way to read a stored token back out.";
export const SYNC_TOKEN_EDIT_NOTE =
  "Left empty, whatever is stored stays as it is. Type a new one to replace it, or clear it to remove it. keeper cannot show a stored token — there is no way to read one back out.";
export const SYNC_TOKEN_STORED_LABEL = "Access token stored in the keychain.";
export const SYNC_TOKEN_CLEAR_LABEL = "Clear token";

/** The only report a clear can get: nothing else on screen can reflect it. */
export const SYNC_TOKEN_CLEARED_LABEL = "The stored token was removed.";

/**
 * Reported when the profile was stored but the keychain write was not. Two
 * writes, two outcomes: "add failed" would send the user back to a form that
 * can now only reject as a duplicate, and "save failed" would send them back
 * to redo changes the engine already took.
 */
export const SYNC_TOKEN_FAILED_PREFIX = "The folder was added, but the token was not stored: ";
export const SYNC_TOKEN_EDIT_FAILED_PREFIX =
  "The changes were saved, but the token was not stored: ";

/** The submit, worded for what it does, and the way out of a revealed form. */
export const SYNC_ADD_SUBMIT_LABEL = "Add folder";
export const SYNC_EDIT_SUBMIT_LABEL = "Save changes";
export const SYNC_FORM_CANCEL_LABEL = "Cancel";

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
 * A stored profile as the fields that carry it. Everything editable starts from
 * what is stored; the token starts empty in every mode, because nothing can
 * read one back out to fill it in.
 */
function formValuesFor(profile: SyncProfileVm): SyncFormValues {
  return {
    name: profile.name,
    localPath: profile.localPath,
    remoteUrl: profile.remoteUrl,
    branch: profile.branch,
    // Both enums are closed mirrors of a Rust match that rejects anything
    // else, so the fallback is unreachable in a build whose UI and engine ship
    // together — it exists because `SyncProfileVm` widens them to `string`.
    direction: SYNC_DIRECTIONS.find((legal) => legal === profile.direction) ?? "bidirectional",
    lfsMode: SYNC_LFS_MODES.find((legal) => legal === profile.lfsMode) ?? "materialize",
    // Back into the units these two fields are typed in.
    lfsThresholdMb: String(profile.lfsThresholdBytes / 1024 / 1024),
    settleSeconds: String(profile.settleMs / 1000),
    removable: profile.removable,
    excludes: profile.excludes.join(", "),
    subpaths: profile.subpaths.join(", "),
    tags: profile.tags.join(", "),
    authorOverride: profile.authorOverride ?? "",
    token: "",
  };
}

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
 * Create a folder, or edit one that exists. A rejected save keeps every typed
 * value and shows the Rust validation message inline — losing a half-typed
 * remote URL to a validation error would be infuriating.
 *
 * @param profile - The profile to edit. Absent, the form creates a folder and
 *   behaves exactly as it always has. Present, it starts populated from this
 *   profile and submits with its id, which is what Rust reads as "update that
 *   one". Read once, on mount: the mirror behind it re-polls every couple of
 *   seconds, and re-seeding the fields from each snapshot would wipe out
 *   whatever was being typed when one landed.
 * @param disabled - Suppress every control while the caller's mirror is still
 *   unknown, so a folder cannot be added against a list nobody has read yet.
 * @param onSaved - Called after a profile is created or updated, with `settled`
 *   false while the form is still showing something only it can show: a
 *   keychain failure, or — on a new folder — the acknowledgement whose Clear
 *   button is the only way to undo a stored token. A surface that hides the
 *   form on success must keep it mounted until `settled`, or it destroys the
 *   one place either is readable.
 * @param onCancel - Rendered as a Cancel button beside the submit. A surface
 *   that reveals the form behind an action passes this, so leaving changes
 *   nothing; one that keeps the form permanently on screen passes nothing.
 */
export function AddFolderForm({
  profile,
  disabled = false,
  className,
  onSaved,
  onCancel,
}: {
  profile?: SyncProfileVm;
  disabled?: boolean;
  className?: string;
  onSaved?: (profile: SyncProfileVm, settled: boolean) => void;
  onCancel?: () => void;
}) {
  const editing = profile !== undefined;
  // Seeded once, deliberately (see `profile` above).
  const [form, setForm] = useState<SyncFormValues>(() =>
    profile === undefined ? EMPTY_FORM : formValuesFor(profile),
  );
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
  /**
   * Whether a clear went through. The acknowledgement for a write nothing else
   * can reflect: a keychain holds no read side to go quiet.
   */
  const [tokenCleared, setTokenCleared] = useState(false);
  const fieldId = useId();
  // Several folders can have an edit form open at once, so the name carries the
  // one it belongs to. A folder being added has no name of its own yet.
  const title = profile === undefined ? SYNC_ADD_TITLE : `${SYNC_EDIT_TITLE}: ${profile.name}`;

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
    // Both acknowledgements belong to the write that produced them, so a second
    // save never leaves the previous one's report standing.
    setStoredToken(null);
    setTokenCleared(false);
    // An empty or unusable threshold / settle window defers to the Rust
    // default rather than sending a number nobody asked for.
    const thresholdMb = Number.parseFloat(form.lfsThresholdMb);
    const settleSeconds = Number.parseFloat(form.settleSeconds);
    const author = form.authorOverride.trim();
    const { token } = form;
    try {
      const saved = await saveSyncProfile({
        // Present updates that profile, absent creates one — the only field
        // that separates the two modes on the wire. The request carries no
        // `enabled`, and Rust merges an update onto the stored profile, so
        // saving an edit to a paused folder leaves it paused.
        id: profile?.id ?? null,
        name: form.name.trim(),
        // Carried back unchanged on an edit: the engine binds a profile to this
        // path (and, on removable media, to a marker under it), which is why
        // the field above is read-only rather than a second picker.
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
        // Emptying the field on an existing profile *clears* the override, and
        // only an explicit empty string says so — `null` is the omission Rust
        // reads as "leave whatever is stored". On a new profile there is
        // nothing to clear, so the omission is the more precise thing to send.
        authorOverride: author === "" && !editing ? null : author,
      });
      if (!editing) {
        setForm(EMPTY_FORM);
      }
      // `saveSyncProfile` re-reads the profile/status mirror, but the Sync
      // view's three per-folder lists are a *second* mirror on a deliberately
      // slower poll. Reading them here — from whichever surface saved the
      // folder — is what keeps its card from sitting stale for a poll
      // interval. Never throws.
      await refreshSyncDetail(saved.id);
      if (token === "") {
        onSaved?.(saved, true);
        return;
      }
      // A second write, to a different store, keyed by the profile id. Its
      // failure is reported as its own thing: the profile is stored by now, and
      // a blanket failure would be a lie.
      try {
        await syncSetCredential(saved.id, token);
      } catch (raw) {
        const prefix = editing ? SYNC_TOKEN_EDIT_FAILED_PREFIX : SYNC_TOKEN_FAILED_PREFIX;
        setError(`${prefix}${syncErrorMessage(raw)}`);
        // The form now holds something only it can show, so the caller is told
        // the profile is stored but must not hide the form yet.
        onSaved?.(saved, false);
        return;
      }
      if (editing) {
        // Clear stands under the token field for as long as this profile is
        // being edited, so the write has an undo without the form staying open
        // to carry one.
        setForm((live) => ({ ...live, token: "" }));
        onSaved?.(saved, true);
        return;
      }
      // On a new folder the acknowledgement and its Clear button are the only
      // undo there is — nothing can read a stored token back to offer one later
      // — so the caller is told to keep the form mounted.
      setStoredToken({ id: saved.id, name: saved.name });
      onSaved?.(saved, false);
    } catch (raw) {
      setError(syncErrorMessage(raw));
    } finally {
      setSaving(false);
    }
  };

  /**
   * Forget the stored token. The only undo there is — nothing reads one back —
   * so it is offered wherever the form has an id to clear against: throughout
   * an edit, and after a new folder's token was just written.
   */
  const clearToken = async (id: string) => {
    setSaving(true);
    setError(null);
    try {
      await syncClearCredential(id);
      setStoredToken(null);
      setTokenCleared(true);
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
      aria-label={title}
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
        {!editing && (
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
        )}
      </div>
      {editing && <p className="text-muted-foreground text-xs">{SYNC_PATH_FIXED_NOTE}</p>}
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
                that could fill it in, for a new folder or an existing one. */}
            <Input
              id={`${fieldId}-token`}
              type="password"
              autoComplete="off"
              className="w-56"
              value={form.token}
              disabled={disabled || saving}
              onChange={(event) => {
                // A typed token replaces whatever is stored, so a prior
                // "removed" no longer describes what saving will do.
                setTokenCleared(false);
                setForm((live) => ({ ...live, token: event.target.value }));
              }}
            />
          </div>
          <p className="text-muted-foreground text-xs">
            {editing ? SYNC_TOKEN_EDIT_NOTE : SYNC_TOKEN_NOTE}
          </p>
          {/* Clearing is a keychain write of its own, offered for as long as
              there is an id to clear against, and acknowledged — nothing else
              on screen could report that it happened. */}
          {profile !== undefined && (
            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant="ghost"
                size="xs"
                className="w-fit"
                disabled={disabled || saving}
                onClick={() => {
                  void clearToken(profile.id);
                }}
              >
                {SYNC_TOKEN_CLEAR_LABEL}
              </Button>
              {tokenCleared && (
                <p className="text-muted-foreground text-xs">{SYNC_TOKEN_CLEARED_LABEL}</p>
              )}
            </div>
          )}
        </div>
      )}
      {/* Outside the disclosure on purpose: this is the acknowledgement for an
          action that already happened, and it would be invisible collapsed. Set
          only while adding — an edit keeps its Clear under the token field, and
          two buttons of the same name on one screen would be one too many. */}
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
      <div className="flex items-center gap-2">
        <Button
          type="submit"
          variant="outline"
          size="sm"
          className="w-fit"
          disabled={disabled || saving || incomplete}
        >
          {editing ? SYNC_EDIT_SUBMIT_LABEL : SYNC_ADD_SUBMIT_LABEL}
        </Button>
        {onCancel !== undefined && (
          <Button type="button" variant="ghost" size="sm" disabled={saving} onClick={onCancel}>
            {SYNC_FORM_CANCEL_LABEL}
          </Button>
        )}
      </div>
    </form>
  );
}
