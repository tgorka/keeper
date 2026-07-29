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
 *   - The three numeric knobs are seconds/MB here and milliseconds/bytes on the
 *     wire, and an empty field means "keeper picks" rather than zero. That is
 *     NOT `null` on the wire: `null` is the omission Rust reads as "leave
 *     whatever is stored" (AD-34-9), so an empty field sends keeper's documented
 *     default, which is the value `effective_settle_ms` reads as "nothing
 *     pinned". Each box's placeholder names the number that will be in force,
 *     and a note appears under it when the typed number is not that number
 *     (AD-34-8).
 *   - The access token is a second write to a different store (the OS
 *     keychain), keyed by the profile id. Its failure is reported as its own
 *     thing, because the profile is stored by then. Reading one back out is a
 *     third call, made only when the user presses for it (AD-34-7) — never as
 *     part of loading the form.
 *   - `SyncProfileReq` carries no `enabled`, and Rust merges an update onto a
 *     CLONE of the stored profile rather than rebuilding it, so saving an edit
 *     leaves a paused folder paused and cannot move any field this form does not
 *     show. An edit must therefore never be routed through anything that also
 *     toggles pause.
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
import { ChevronDown, ChevronRight, Eye, EyeOff } from "lucide-react";
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
// The credential calls are made straight from the form rather than through the
// mirror store: none of them change anything the store mirrors, and the read is
// a one-shot answer to a button press rather than state worth keeping in sync.
import { syncClearCredential, syncGetCredential, syncSetCredential } from "@/lib/ipc/client";
import {
  SYNC_DEFAULT_BRANCH,
  SYNC_DEFAULT_LANE,
  SYNC_DEFAULT_LFS_THRESHOLD_BYTES,
  SYNC_DEFAULT_POLL_INTERVAL_MS,
  SYNC_DEFAULT_SETTLE_MS,
  SYNC_DIRECTIONS,
  SYNC_LFS_MODES,
  SYNC_MIN_POLL_INTERVAL_MS,
  SYNC_REMOVABLE_SETTLE_MS,
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
 * The advanced knobs (Story 32.7, AD-S8; Story 34.5, AD-34-8). Every one of
 * these was already carried by `SyncProfileReq` and hardcoded away by this form.
 *
 * Each numeric knob shows the number that is actually IN FORCE. Leaving one
 * empty means "keeper picks", and the placeholder then names what keeper will
 * pick; typing a number pins it, and if Rust would not honour that number
 * verbatim the note under the field says what it will use instead. A blank box
 * that silently meant 5 000 was the whole of AD-34-8.
 */
export const SYNC_SETTLE_LABEL = "Wait for writes to stop (seconds)";
export const SYNC_SETTLE_NOTE =
  "How long a file must go untouched before keeper copies it. Left empty, keeper picks the wait itself — and picks a longer one on removable or network storage, where changes are reported late.";
export const SYNC_POLL_LABEL = "Re-read the folder every (seconds)";
export const SYNC_POLL_NOTE =
  "How often keeper walks the whole folder looking for changes. Lower notices a change sooner and costs more on a drive or a network share. Left empty, keeper picks the interval itself.";
export const SYNC_SUBJECT_LABEL = "Commit subject";
export const SYNC_SUBJECT_NOTE =
  "The first line of every commit keeper makes for this folder. {profile} is the folder's name; {added}, {modified}, {deleted} and {changed} are file counts. Left empty, keeper writes its own — sync(folder): 3 added, 1 modified.";
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
 * The access-token field (Story 32.7, AD-S7; Story 34.4, AD-34-7). The token is
 * written to the OS keychain in a second call once the profile has an id, and
 * can be read back out of it — but only when the user asks. The field therefore
 * still starts empty in both modes, and the note under it has to say where the
 * token is kept and that showing it is a thing the user does.
 */
export const SYNC_TOKEN_LABEL = "Access token";
export const SYNC_TOKEN_NOTE =
  "Used to authenticate with the remote. keeper stores it in the system keychain when the folder is added, and shows it again only when you ask.";
export const SYNC_TOKEN_EDIT_NOTE =
  "Left empty, whatever is stored stays as it is. Type a new one to replace it, or clear it to remove it. The stored one is in the system keychain, and keeper reads it back only when you ask for it.";
export const SYNC_TOKEN_STORED_LABEL = "Access token stored in the keychain.";
export const SYNC_TOKEN_CLEAR_LABEL = "Clear token";

/**
 * The eye toggle over the field, named for what pressing it will do rather than
 * for the state it is in, because that is what a screen reader announces before
 * the press.
 */
export const SYNC_TOKEN_SHOW_LABEL = "Show token";
export const SYNC_TOKEN_HIDE_LABEL = "Hide token";

/**
 * The reveal and the two answers that are not a token. A keychain read has one
 * outcome a write does not — there may be nothing stored — and that is not a
 * failure, so neither it nor a failed read may be reported by leaving the field
 * blank: blank is what both would look like.
 */
export const SYNC_TOKEN_REVEAL_LABEL = "Show stored token";
export const SYNC_TOKEN_NONE_STORED_LABEL = "No token is stored for this folder.";
export const SYNC_TOKEN_READ_FAILED_PREFIX = "The stored token could not be read: ";

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

/**
 * The line under a numeric knob whose box holds a number Rust will not use.
 *
 * Reachable two ways: a scan cadence below the floor keeper can work with, and
 * a wait of exactly the default on removable storage — which Rust reads as "no
 * wait pinned" and answers with the longer removable window. Either way the box
 * would otherwise show a number that is not the number in force, which is the
 * defect AD-34-8 names.
 */
export function syncInForceNote(seconds: number): string {
  return `keeper is using ${seconds} s here.`;
}

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
  /**
   * The settle window in seconds; empty means the profile pins none and keeper
   * picks — a different thing from pinning keeper's own number, and the
   * distinction is what `effective_settle_ms` reads (AD-34-8).
   */
  settleSeconds: string;
  /** The scan cadence in seconds; empty means keeper picks, same rule. */
  pollSeconds: string;
  /** Comma-separated patterns; empty excludes nothing. */
  excludes: string;
  /** Comma-separated paths inside the folder; empty syncs all of it. */
  subpaths: string;
  /** Comma-separated commit tags; empty adds no tag trailers. */
  tags: string;
  /** The commit-subject template; empty means keeper's mechanical subject. */
  commitSubjectTemplate: string;
  /** The commit-author override; empty keeps the device identity. */
  authorOverride: string;
  /**
   * The access token: typed, or fetched back out of the keychain by an explicit
   * reveal. Never seeded from a profile — a profile carries none.
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
  pollSeconds: "",
  excludes: "",
  subpaths: "",
  tags: "",
  commitSubjectTemplate: "",
  authorOverride: "",
  token: "",
};

/**
 * A stored profile as the fields that carry it. Everything editable starts from
 * what is stored; the token starts empty in every mode, because a profile
 * carries none — reading one is a separate keychain call the user has to ask
 * for (AD-34-7), and loading a form is not asking.
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
    // Back into the units these fields are typed in. `null` on the two windows
    // means the profile pins nothing, and an empty box is how the form says that
    // back — never a number, which the next save would store as a choice the
    // user never made.
    lfsThresholdMb: String(profile.lfsThresholdBytes / 1024 / 1024),
    settleSeconds: profile.settleMs === null ? "" : String(profile.settleMs / 1000),
    pollSeconds: profile.pollIntervalMs === null ? "" : String(profile.pollIntervalMs / 1000),
    removable: profile.removable,
    excludes: profile.excludes.join(", "),
    subpaths: profile.subpaths.join(", "),
    tags: profile.tags.join(", "),
    commitSubjectTemplate: profile.commitSubjectTemplate,
    authorOverride: profile.authorOverride ?? "",
    token: "",
  };
}

/**
 * The number a numeric box holds, or `null` when it holds nothing usable.
 *
 * Empty, unparseable and non-positive all mean the same thing to this form —
 * "keeper picks" — because a zero-second wait would commit half-written files, a
 * zero-second cadence would re-read the tree on every tick, and a zero-byte LFS
 * threshold would route every file through LFS.
 */
function pinnedValue(raw: string): number | null {
  const parsed = Number.parseFloat(raw);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

/**
 * The wait keeper will actually use, given what the form holds right now.
 *
 * Mirrors `SyncProfile::effective_settle_ms`, including the part that surprises:
 * a wait of exactly keeper's own default IS how "keeper picks" is stored, so a
 * removable folder gets the longer window whether the box is empty or holds a 5.
 * Recomputed from the LIVE form rather than read off `effectiveSettleMs`,
 * because ticking the removable box changes the answer before anything is saved.
 *
 * No ceiling clamp: Rust refuses a window above `SETTLE_CEILING_MS` outright, so
 * a value this would clamp cannot be saved in the first place.
 */
function effectiveSettleSeconds(form: SyncFormValues): number {
  const pinned = pinnedValue(form.settleSeconds);
  const ms = pinned === null ? SYNC_DEFAULT_SETTLE_MS : pinned * 1000;
  const effective = form.removable && ms === SYNC_DEFAULT_SETTLE_MS ? SYNC_REMOVABLE_SETTLE_MS : ms;
  return effective / 1000;
}

/** The scan cadence keeper will actually use — mirrors the same-named Rust fn. */
function effectivePollSeconds(form: SyncFormValues): number {
  const pinned = pinnedValue(form.pollSeconds);
  const ms = pinned === null ? SYNC_DEFAULT_POLL_INTERVAL_MS : pinned * 1000;
  return Math.max(ms, SYNC_MIN_POLL_INTERVAL_MS) / 1000;
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
   * the user will ever get that the keychain took it — and the first moment the
   * form holds an id to clear it with, since a folder being added has none.
   */
  const [storedToken, setStoredToken] = useState<{ id: string; name: string } | null>(null);
  /**
   * Whether a clear went through. Nothing else on screen goes quiet when a
   * keychain entry disappears, so this acknowledgement is the whole report.
   */
  const [tokenCleared, setTokenCleared] = useState(false);
  /**
   * Whether the field shows what it holds. About the field, not about where its
   * contents came from: it flips a token being typed and a token just revealed
   * alike.
   */
  const [tokenVisible, setTokenVisible] = useState(false);
  /**
   * That the reveal came back with nothing. Kept apart from both an error and
   * an empty field, because "the keychain holds no token for this folder" is a
   * fact, and the other two are not it.
   */
  const [noStoredToken, setNoStoredToken] = useState(false);
  /**
   * A reveal in flight. Separate from `saving` so a keychain read — which may
   * put an OS prompt in front of the window — disables only its own button
   * rather than the form the user is still filling in.
   */
  const [revealing, setRevealing] = useState(false);
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
    setNoStoredToken(false);
    // An empty box means "keeper picks", and keeper's own number is what it
    // means: `null` on the wire is the OMISSION Rust reads as "leave whatever is
    // stored" (AD-34-9), which is the opposite instruction. Sending the
    // documented default is not a hard-coded user opinion either — a stored
    // value equal to the default is exactly how `effective_settle_ms` encodes
    // "nothing pinned here", so a removable folder still gets its longer window.
    const thresholdMb = pinnedValue(form.lfsThresholdMb);
    const settle = pinnedValue(form.settleSeconds);
    const poll = pinnedValue(form.pollSeconds);
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
          thresholdMb === null
            ? SYNC_DEFAULT_LFS_THRESHOLD_BYTES
            : Math.round(thresholdMb * 1024 * 1024),
        settleMs: settle === null ? SYNC_DEFAULT_SETTLE_MS : Math.round(settle * 1000),
        pollIntervalMs: poll === null ? SYNC_DEFAULT_POLL_INTERVAL_MS : Math.round(poll * 1000),
        tags: splitSyncList(form.tags),
        // Emptying the field on an existing profile *clears* the override, and
        // only an explicit empty string says so — `null` is the omission Rust
        // reads as "leave whatever is stored". On a new profile there is
        // nothing to clear, so the omission is the more precise thing to send.
        authorOverride: author === "" && !editing ? null : author,
        // Always expressed, because the field is always on screen. An empty
        // string is a value here rather than an omission: it IS keeper's own
        // mechanical subject.
        commitSubjectTemplate: form.commitSubjectTemplate.trim(),
      });
      if (!editing) {
        setForm(EMPTY_FORM);
        setTokenVisible(false);
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
        setTokenVisible(false);
        onSaved?.(saved, true);
        return;
      }
      // On a new folder the acknowledgement and its Clear button are the only
      // undo left once the form is gone, so the caller is told to keep it
      // mounted.
      setStoredToken({ id: saved.id, name: saved.name });
      onSaved?.(saved, false);
    } catch (raw) {
      setError(syncErrorMessage(raw));
    } finally {
      setSaving(false);
    }
  };

  /**
   * Forget the stored token. Offered wherever the form has an id to clear
   * against: throughout an edit, and after a new folder's token was just
   * written, which is the only moment an add has one.
   */
  const clearToken = async (id: string) => {
    setSaving(true);
    setError(null);
    try {
      await syncClearCredential(id);
      setStoredToken(null);
      setTokenCleared(true);
      // A revealed token still on screen would contradict the line that just
      // said it is gone — and the next save would write it straight back.
      setForm((live) => ({ ...live, token: "" }));
      setTokenVisible(false);
      setNoStoredToken(false);
    } catch (raw) {
      setError(syncErrorMessage(raw));
    } finally {
      setSaving(false);
    }
  };

  /**
   * Read the stored token out of the keychain, because the user pressed for it.
   * Never called on mount or from any load path (AD-34-7): a secret crosses
   * into the UI on a press and on nothing else. All three answers are reported
   * as themselves, since an empty field is what two of them would look like.
   */
  const revealStoredToken = async (id: string) => {
    setRevealing(true);
    setError(null);
    setNoStoredToken(false);
    try {
      const stored = await syncGetCredential(id);
      if (stored === null) {
        setNoStoredToken(true);
        return;
      }
      setForm((live) => ({ ...live, token: stored }));
      // Asking for a secret and then rendering it as dots would be asking and
      // withholding, so a reveal reveals.
      setTokenVisible(true);
      // The field now holds what the keychain holds, so a prior "removed" no
      // longer describes anything on screen.
      setTokenCleared(false);
    } catch (raw) {
      setError(`${SYNC_TOKEN_READ_FAILED_PREFIX}${syncErrorMessage(raw)}`);
    } finally {
      setRevealing(false);
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
              placeholder={String(SYNC_DEFAULT_LFS_THRESHOLD_BYTES / 1024 / 1024)}
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
              // Empty means "keeper picks", so the box shows nothing and the
              // placeholder names what keeper will pick — 10 s on removable
              // storage, and it follows the checkbox above live.
              placeholder={String(effectiveSettleSeconds({ ...form, settleSeconds: "" }))}
              value={form.settleSeconds}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, settleSeconds: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_SETTLE_NOTE}</p>
          {/* A typed number Rust will not use verbatim. The reachable case is a
              wait of exactly keeper's own default on removable storage, which
              Rust reads as "nothing pinned" and answers with the longer window
              — so the box would otherwise show 5 while 10 was in force. */}
          {pinnedValue(form.settleSeconds) !== null &&
            pinnedValue(form.settleSeconds) !== effectiveSettleSeconds(form) && (
              <p className="text-muted-foreground text-xs">
                {syncInForceNote(effectiveSettleSeconds(form))}
              </p>
            )}
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-poll`}>{SYNC_POLL_LABEL}</Label>
            <Input
              id={`${fieldId}-poll`}
              type="number"
              min={0}
              className="w-24"
              placeholder={String(effectivePollSeconds({ ...form, pollSeconds: "" }))}
              value={form.pollSeconds}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, pollSeconds: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_POLL_NOTE}</p>
          {/* Here the divergence is the floor: a cadence under two seconds would
              put a full re-stat of the tree on every supervisor tick. */}
          {pinnedValue(form.pollSeconds) !== null &&
            pinnedValue(form.pollSeconds) !== effectivePollSeconds(form) && (
              <p className="text-muted-foreground text-xs">
                {syncInForceNote(effectivePollSeconds(form))}
              </p>
            )}
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
            <Label htmlFor={`${fieldId}-subject`}>{SYNC_SUBJECT_LABEL}</Label>
            <Input
              id={`${fieldId}-subject`}
              className="w-56"
              // The placeholder is what an empty field produces, so it is the
              // real subject rather than an example: `{profile}` is substituted
              // with the name being typed, and the counts stand for whatever a
              // commit turns out to carry.
              placeholder={`sync(${form.name.trim() === "" ? "folder" : form.name.trim()}): 3 added, 1 modified`}
              value={form.commitSubjectTemplate}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, commitSubjectTemplate: event.target.value }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_SUBJECT_NOTE}</p>
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
            <div className="flex w-56 items-center gap-1">
              <Input
                id={`${fieldId}-token`}
                type={tokenVisible ? "text" : "password"}
                autoComplete="off"
                className="min-w-0 flex-1"
                value={form.token}
                disabled={disabled || saving}
                onChange={(event) => {
                  // A typed token replaces whatever is stored, so neither a
                  // prior "removed" nor a prior "none stored" still describes
                  // what saving will do.
                  setTokenCleared(false);
                  setNoStoredToken(false);
                  setForm((live) => ({ ...live, token: event.target.value }));
                }}
              />
              {/* A button, not an adornment with a click handler: it changes
                  what is on screen, so it has to be reachable by keyboard, carry
                  its state, and say which way it will flip. */}
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-pressed={tokenVisible}
                aria-label={tokenVisible ? SYNC_TOKEN_HIDE_LABEL : SYNC_TOKEN_SHOW_LABEL}
                disabled={disabled || saving}
                onClick={() => setTokenVisible((shown) => !shown)}
              >
                {tokenVisible ? <EyeOff /> : <Eye />}
              </Button>
            </div>
          </div>
          <p className="text-muted-foreground text-xs">
            {editing ? SYNC_TOKEN_EDIT_NOTE : SYNC_TOKEN_NOTE}
          </p>
          {/* The two keychain calls the form can make against an id it already
              has. Both are offered for as long as there is one: revealing is a
              read the user asks for, clearing a write nothing else on screen
              could report. */}
          {profile !== undefined && (
            <div className="flex flex-wrap items-center gap-2">
              <Button
                type="button"
                variant="ghost"
                size="xs"
                className="w-fit"
                disabled={disabled || saving || revealing}
                onClick={() => {
                  void revealStoredToken(profile.id);
                }}
              >
                {SYNC_TOKEN_REVEAL_LABEL}
              </Button>
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
              {noStoredToken && (
                <p className="text-muted-foreground text-xs">{SYNC_TOKEN_NONE_STORED_LABEL}</p>
              )}
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
